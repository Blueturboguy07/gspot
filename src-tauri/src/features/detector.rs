//! AI Detector v1: stylometric signals + an LLM judge, combined into one
//! "authenticity" score. Honest about what it is: a heuristic detector.

use crate::aiphrases;
use crate::llm;
use crate::settings::LlmConfig;
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Serialize)]
pub struct Signals {
    pub words: usize,
    pub sentences: usize,
    pub avg_sentence_len: f64,
    /// Coefficient of variation of sentence length (low = uniform = AI-like).
    pub burstiness: f64,
    /// Type-token ratio over the first 300 words.
    pub ttr: f64,
    pub ai_phrases_per_100w: f64,
    pub human_markers: usize,
    /// 0..100 — stylometric AI likelihood before the judge.
    pub stylometric_ai: f64,
}

pub fn signals(text: &str) -> Signals {
    let words: Vec<&str> = text.unicode_words().collect();
    let sentences: Vec<&str> = text.unicode_sentences().map(|s| s.trim()).filter(|s| s.split_whitespace().count() >= 2).collect();
    let lens: Vec<f64> = sentences.iter().map(|s| s.split_whitespace().count() as f64).collect();
    let n = lens.len().max(1) as f64;
    let mean = lens.iter().sum::<f64>() / n;
    let var = lens.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / n;
    let burstiness = if mean > 0.0 { var.sqrt() / mean } else { 0.0 };
    let sample: Vec<String> = words.iter().take(300).map(|w| w.to_lowercase()).collect();
    let uniq: std::collections::HashSet<&String> = sample.iter().collect();
    let ttr = if sample.is_empty() { 0.0 } else { uniq.len() as f64 / sample.len() as f64 };
    let phrases = aiphrases::find(text);
    let weighted: f64 = phrases
        .iter()
        .map(|p| match p.bucket {
            aiphrases::Bucket::Frequent => 3.0,
            aiphrases::Bucket::Often => 1.5,
            aiphrases::Bucket::Sometimes => 0.5,
        })
        .sum();
    let ai_phrases_per_100w = if words.is_empty() { 0.0 } else { weighted * 100.0 / words.len() as f64 };
    let lower = text.to_lowercase();
    let human_markers = [" lol", " lmao", "fuck", "shit", "damn", " yall", "y'all", " im ", " dont ", " cant ", " wasnt", " didnt", "haha", "…", "!!", "tbh", " idk", " ngl", " gonna", " wanna", " kinda"]
        .iter()
        .filter(|m| lower.contains(*m))
        .count()
        + lower.matches(" i ").count().min(10) / 3;

    // Stylometric likelihood: uniform sentences, high TTR with abstract vocabulary and AI phrases push up;
    // bursty, marker-rich text pushes down.
    let mut score = 35.0;
    score += ((0.55 - burstiness).max(0.0) / 0.55) * 30.0; // uniform rhythm
    score += (ai_phrases_per_100w * 8.0).min(30.0);
    if mean > 22.0 {
        score += 8.0;
    }
    score -= (human_markers as f64 * 6.0).min(35.0);
    if ttr < 0.45 && words.len() > 120 {
        score -= 5.0;
    }
    Signals {
        words: words.len(),
        sentences: sentences.len(),
        avg_sentence_len: mean,
        burstiness,
        ttr,
        ai_phrases_per_100w,
        human_markers,
        stylometric_ai: score.clamp(0.0, 100.0),
    }
}

pub async fn detect(cfg: &LlmConfig, text: &str) -> Result<Value> {
    let sig = signals(text);
    if sig.words < 8 {
        return Ok(json!({"too_short": true, "min_words": 8}));
    }
    let judge = llm::complete_json(cfg, super::prompts::DETECTOR_JUDGE_SYSTEM, &format!("TEXT:\n{}", super::clip(text, 10000)), 0.1).await;
    let (judge_ai, reasons, most_ai) = match judge {
        Ok(v) => (
            v["ai_likelihood"].as_f64().unwrap_or(50.0).clamp(0.0, 100.0),
            v["reasons"].as_array().cloned().unwrap_or_default(),
            v["most_ai_sentence"].as_str().unwrap_or("").to_string(),
        ),
        Err(e) => (sig.stylometric_ai, vec![json!(format!("Judge unavailable: {e}"))], String::new()),
    };
    let ai = 0.55 * judge_ai + 0.45 * sig.stylometric_ai;
    let human = (100.0 - ai).round() as u8;
    let verdict = if ai < 25.0 {
        "Looks human-written"
    } else if ai < 55.0 {
        "Mostly human, with some AI-like patterns"
    } else if ai < 80.0 {
        "Possible AI text detected"
    } else {
        "Likely AI-generated"
    };
    Ok(json!({
        "authenticity": human,
        "ai_likelihood": ai.round(),
        "verdict": verdict,
        "reasons": reasons,
        "most_ai_sentence": most_ai,
        "signals": sig,
        "method": "stylometry + LLM judge (not a trained classifier)",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_cliche_text_scores_more_ai_than_messy_personal_text() {
        let ai = "In today's fast-paced world, it is important to note that leveraging robust solutions is crucial. Furthermore, embracing a holistic approach fosters a sense of innovation. Ultimately, navigating the complexities of the digital landscape requires a comprehensive strategy. Moreover, stakeholders must prioritize seamless integration to unlock the potential of transformative technologies.";
        let human = "ok so i didnt sleep last night lol. my roommate's cat knocked the router off the shelf at 3am and i spent an hour fighting with comcast. anyway!! the demo is at 9, pray for me. if it breaks im blaming the cat.";
        let a = signals(ai);
        let h = signals(human);
        assert!(a.stylometric_ai > h.stylometric_ai + 20.0, "ai={} human={}", a.stylometric_ai, h.stylometric_ai);
        assert!(h.human_markers >= 2);
    }

    #[test]
    fn short_text_is_reported_as_too_short() {
        let s = signals("too short");
        assert!(s.words < 8);
    }
}
