//! Performance score: the same labels Grammarly shows, computed locally from
//! alert density and readability.

use crate::engine::{Alert, Category};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub words: usize,
    pub sentences: usize,
    pub characters: usize,
    pub reading_time_sec: usize,
    pub flesch: f64,
    pub overall: Option<u8>,
    pub correctness_count: usize,
    pub clarity_count: usize,
    pub engagement_count: usize,
    pub delivery_count: usize,
    pub correctness_label: String,
    pub clarity_label: String,
    pub engagement_label: String,
    pub delivery_label: String,
    pub correctness_fill: u8,
    pub clarity_fill: u8,
    pub engagement_fill: u8,
    pub delivery_fill: u8,
    pub note: Option<String>,
}

pub const MIN_WORDS: usize = 30;

pub fn word_count(text: &str) -> usize {
    text.unicode_words().count()
}

pub fn sentence_count(text: &str) -> usize {
    text.unicode_sentences().filter(|s| s.trim().len() > 1).count().max(if text.trim().is_empty() { 0 } else { 1 })
}

fn syllables(word: &str) -> usize {
    let w = word.to_lowercase();
    let mut count = 0;
    let mut prev_vowel = false;
    for c in w.chars() {
        let v = "aeiouy".contains(c);
        if v && !prev_vowel {
            count += 1;
        }
        prev_vowel = v;
    }
    if w.ends_with('e') && count > 1 && !w.ends_with("le") {
        count -= 1;
    }
    count.max(1)
}

pub fn flesch_reading_ease(text: &str) -> f64 {
    let words: Vec<&str> = text.unicode_words().collect();
    let w = words.len().max(1) as f64;
    let s = sentence_count(text).max(1) as f64;
    let syl: usize = words.iter().map(|x| syllables(x)).sum();
    206.835 - 1.015 * (w / s) - 84.6 * (syl as f64 / w)
}

pub fn compute(text: &str, alerts: &[Alert]) -> Score {
    let words = word_count(text);
    let sentences = sentence_count(text);
    let characters = text.chars().count();
    let flesch = if words > 0 { flesch_reading_ease(text) } else { 0.0 };
    let c = |cat: Category| alerts.iter().filter(|a| a.category == cat).count();
    let (cc, cl, ce, cd) = (c(Category::Correctness), c(Category::Clarity), c(Category::Engagement), c(Category::Delivery));
    let per100 = |n: usize| if words == 0 { 0.0 } else { n as f64 * 100.0 / words as f64 };
    let (pc, pl, pe, pd) = (per100(cc), per100(cl), per100(ce), per100(cd));

    let overall = if words < MIN_WORDS {
        None
    } else {
        let readability_penalty = if flesch >= 60.0 { 0.0 } else { ((60.0 - flesch) / 3.0).min(20.0) };
        let penalty = 8.0 * pc + 4.0 * pl + 2.0 * pe + 3.0 * pd + readability_penalty;
        Some((100.0 - penalty).clamp(0.0, 100.0).round() as u8)
    };

    let fill = |p: f64, k: f64| (100.0 - (p * k).min(100.0)).round() as u8;

    let correctness_label = if cc == 0 { "Looking good".into() } else { format!("{} suggestion{}", cc, if cc == 1 { "" } else { "s" }) };
    let clarity_label = if words == 0 {
        "So far so good".into()
    } else if cl == 0 {
        "Very clear".into()
    } else if pl < 0.6 {
        "Clear".into()
    } else if pl < 1.5 {
        "Mostly clear".into()
    } else if pl < 3.0 {
        "A bit unclear".into()
    } else {
        "Unclear".into()
    };
    let engagement_label = if words == 0 {
        "Nothing yet".into()
    } else if ce == 0 {
        "Very engaging".into()
    } else if pe < 1.0 {
        "Engaging".into()
    } else if pe < 2.0 {
        "A bit bland".into()
    } else {
        "Bland".into()
    };
    let delivery_label = if words == 0 {
        "Nothing yet".into()
    } else if cd == 0 {
        "Just right".into()
    } else if pd < 1.2 {
        "Slightly off".into()
    } else {
        "Very off".into()
    };
    let note = if words < MIN_WORDS {
        Some(format!("GSpot needs at least {} words to calculate a score.", MIN_WORDS))
    } else if overall == Some(100) {
        Some("Great! This text scores 100 out of 100.".into())
    } else {
        None
    };

    Score {
        words,
        sentences,
        characters,
        reading_time_sec: (words as f64 / 238.0 * 60.0).round() as usize,
        flesch,
        overall,
        correctness_count: cc,
        clarity_count: cl,
        engagement_count: ce,
        delivery_count: cd,
        correctness_label,
        clarity_label,
        engagement_label,
        delivery_label,
        correctness_fill: fill(pc, 12.0),
        clarity_fill: fill(pl, 25.0),
        engagement_fill: fill(pe, 30.0),
        delivery_fill: fill(pd, 30.0),
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Category;

    fn alert(cat: Category) -> Alert {
        Alert {
            id: "x".into(),
            category: cat,
            kind: "k".into(),
            title: "t".into(),
            message: "m".into(),
            start: 0,
            end: 1,
            u16_start: 0,
            u16_end: 1,
            original: "a".into(),
            suggestions: vec![],
            priority: 1,
            stable_hash: 1,
        }
    }

    #[test]
    fn short_text_has_no_score_but_has_labels() {
        let s = compute("Hello there friend.", &[]);
        assert_eq!(s.overall, None);
        assert!(s.note.unwrap().contains("30 words"));
        assert_eq!(s.correctness_label, "Looking good");
    }

    #[test]
    fn clean_long_text_scores_high() {
        let text = "The cat sat on the mat. ".repeat(10);
        let s = compute(&text, &[]);
        assert!(s.overall.unwrap() >= 95, "{:?}", s.overall);
        assert_eq!(s.delivery_label, "Just right");
        assert_eq!(s.engagement_label, "Very engaging");
    }

    #[test]
    fn many_correctness_alerts_drag_the_score_down() {
        let text = "word ".repeat(700);
        let alerts: Vec<Alert> = (0..30).map(|_| alert(Category::Correctness)).collect();
        let s = compute(&text, &alerts);
        let v = s.overall.unwrap();
        assert!(v < 75 && v > 30, "score {}", v);
        assert_eq!(s.correctness_label, "30 suggestions");
    }

    #[test]
    fn labels_follow_density() {
        let text = "word ".repeat(100);
        let a: Vec<Alert> = (0..4).map(|_| alert(Category::Clarity)).collect();
        assert_eq!(compute(&text, &a).clarity_label, "Unclear");
        let a: Vec<Alert> = (0..1).map(|_| alert(Category::Clarity)).collect();
        assert_eq!(compute(&text, &a).clarity_label, "Mostly clear");
        let a: Vec<Alert> = (0..2).map(|_| alert(Category::Delivery)).collect();
        assert_eq!(compute(&text, &a).delivery_label, "Very off");
    }

    #[test]
    fn flesch_is_sane() {
        let easy = flesch_reading_ease("The dog ran. The cat sat. It was fun.");
        let hard = flesch_reading_ease("Notwithstanding the aforementioned considerations, the implementation necessitates comprehensive reevaluation.");
        assert!(easy > hard);
    }
}
