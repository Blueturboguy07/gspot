//! Curated "phrases AI uses more often than people" with Grammarly's three
//! frequency buckets. Matching is case-insensitive on word boundaries.

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    /// "Almost always AI" — over 50× frequency
    Frequent,
    /// "Usually AI" — 10–49× frequency
    Often,
    /// "Sometimes AI" — less than 10× frequency
    Sometimes,
}

impl Bucket {
    pub fn label(&self) -> &'static str {
        match self {
            Bucket::Frequent => "Almost always AI",
            Bucket::Often => "Usually AI",
            Bucket::Sometimes => "Sometimes AI",
        }
    }
    pub fn description(&self) -> &'static str {
        match self {
            Bucket::Frequent => "Over 50× frequency",
            Bucket::Often => "10-49× frequency",
            Bucket::Sometimes => "Less than 10× frequency",
        }
    }
}

pub const FREQUENT: &[&str] = &[
    "delve into", "delve", "delves", "delving", "tapestry", "in today's fast-paced world", "in today's digital age", "in the ever-evolving landscape",
    "ever-evolving", "a testament to", "testament to", "navigate the complexities", "navigating the complexities", "embark on a journey",
    "embark on", "in the realm of", "unlock the potential", "unlock the power", "game-changer", "game changer", "it's important to note that",
    "it is important to note that", "it's worth noting that", "it is worth noting that", "as an AI language model", "I hope this email finds you well",
    "look no further", "let's dive in", "dive into", "in conclusion,", "in summary,", "to sum up,", "multifaceted", "paradigm shift", "synergy",
    "holistic approach", "a beacon of", "stands as a testament", "the intricate", "intricacies", "meticulous", "meticulously", "unleash",
    "elevate your", "revolutionize", "groundbreaking", "transformative power", "treasure trove", "rich tapestry", "vibrant tapestry",
    "whether you're a seasoned", "seamlessly integrate", "seamless integration", "cutting-edge", "state-of-the-art", "harness the power",
    "foster a sense of", "fosters a sense of", "shed light on", "sheds light on", "a myriad of", "myriad of", "plethora of", "a plethora",
    "it goes without saying", "needless to say", "in the digital era", "key takeaways", "bustling", "nestled", "underscores the importance",
    "underscore the importance", "pivotal role", "plays a pivotal role", "plays a crucial role", "crucial role",
];

pub const OFTEN: &[&str] = &[
    "furthermore,", "moreover,", "additionally,", "ultimately,", "notably,", "crucially,", "importantly,", "arguably,", "undoubtedly,",
    "leverage", "leveraging", "robust", "streamline", "streamlined", "empower", "empowering", "enhance", "enhancing", "facilitate", "foster",
    "fostering", "comprehensive", "pivotal", "paramount", "nuanced", "landscape", "navigate", "navigating", "journey", "realm", "elevate",
    "resonate", "resonates", "transformative", "innovative solutions", "actionable insights", "valuable insights", "ensure", "ensuring",
    "it is essential to", "it's essential to", "it is crucial to", "it's crucial to", "when it comes to", "in the world of", "in essence,",
    "a wide range of", "a variety of", "serves as", "serve as", "boasts", "at the forefront", "in light of", "with that said",
    "remember,", "in a nutshell", "the bottom line", "top-notch", "world-class", "best practices", "unprecedented", "profound",
    "enduring", "captivating", "compelling", "illuminating", "insightful", "thought-provoking", "indelible", "enigmatic", "labyrinth",
];

pub const SOMETIMES: &[&str] = &[
    "indeed,", "certainly,", "overall,", "in addition,", "however,", "therefore,", "thus,", "hence,", "consequently,", "as a result,",
    "significant", "significantly", "various", "numerous", "essential", "crucial", "vital", "key", "effective", "efficient", "optimal",
    "optimize", "utilize", "utilizing", "impactful", "dynamic", "diverse", "unique", "powerful", "seamless", "effortless", "ensure that",
    "explore", "exploring", "highlight", "highlights", "underscore", "showcase", "showcasing", "emphasize", "prioritize", "cultivate",
    "mitigate", "bolster", "spearhead", "align", "aligned", "framework", "ecosystem", "stakeholders", "scalable", "sustainable",
];

pub struct Phrase {
    pub text: &'static str,
    pub bucket: Bucket,
    pub re: Regex,
}

pub fn phrases() -> &'static Vec<Phrase> {
    static P: OnceLock<Vec<Phrase>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = Vec::new();
        let mut add = |list: &[&'static str], bucket: Bucket| {
            for &p in list {
                // Word boundaries only where the phrase itself starts/ends with a word character;
                // "Furthermore," must still match when followed by a space.
                let lead = if p.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false) { r"\b" } else { "" };
                let trail = if p.chars().last().map(|c| c.is_alphanumeric()).unwrap_or(false) { r"\b" } else { "" };
                let body = regex::escape(p).replace(' ', r"\s+");
                let pat = format!("(?i){lead}{body}{trail}");
                if let Ok(re) = Regex::new(&pat) {
                    v.push(Phrase { text: p, bucket, re });
                }
            }
        };
        add(FREQUENT, Bucket::Frequent);
        add(OFTEN, Bucket::Often);
        add(SOMETIMES, Bucket::Sometimes);
        v.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
        v
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct Found {
    pub id: String,
    pub phrase: String,
    pub bucket: Bucket,
    pub bucket_label: &'static str,
    /// char offsets
    pub start: usize,
    pub end: usize,
    pub u16_start: usize,
    pub u16_end: usize,
    pub sentence: String,
}

pub fn find(text: &str) -> Vec<Found> {
    use crate::textmap::TextMap;
    let map = TextMap::new(text);
    let prefix = byte_to_char(text);
    let mut out: Vec<Found> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for p in phrases() {
        for m in p.re.find_iter(text) {
            let (bs, be) = (m.start(), m.end());
            if taken.iter().any(|(s, e)| bs < *e && be > *s) {
                continue;
            }
            taken.push((bs, be));
            let start = prefix[bs];
            let end = prefix[be];
            out.push(Found {
                id: format!("p{}-{}", start, end),
                phrase: m.as_str().to_string(),
                bucket: p.bucket,
                bucket_label: p.bucket.label(),
                start,
                end,
                u16_start: map.char_to_utf16(start),
                u16_end: map.char_to_utf16(end),
                sentence: sentence_around(text, bs, be),
            });
        }
    }
    out.sort_by_key(|f| f.start);
    out
}

fn sentence_around(text: &str, bs: usize, be: usize) -> String {
    let bytes = text.as_bytes();
    let mut s = bs;
    while s > 0 && !matches!(bytes[s - 1], b'.' | b'!' | b'?' | b'\n') {
        s -= 1;
    }
    let mut e = be;
    while e < bytes.len() && !matches!(bytes[e], b'.' | b'!' | b'?' | b'\n') {
        e += 1;
    }
    if e < bytes.len() {
        e += 1;
    }
    // align to char boundaries
    while !text.is_char_boundary(s) {
        s -= 1;
    }
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    text[s..e].trim().to_string()
}

fn byte_to_char(text: &str) -> Vec<usize> {
    let mut v = vec![0usize; text.len() + 1];
    let mut ci = 0;
    let mut last = 0;
    for (bi, _) in text.char_indices() {
        for b in last..=bi {
            v[b] = ci;
        }
        last = bi + 1;
        ci += 1;
    }
    for b in last..=text.len() {
        v[b] = ci;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bucketed_phrases_with_offsets() {
        let t = "Let's delve into the rich tapestry of history. Furthermore, it is essential to leverage synergy.";
        let f = find(t);
        let names: Vec<&str> = f.iter().map(|x| x.phrase.as_str()).collect();
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("delve into")), "{names:?}");
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("rich tapestry")));
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("Furthermore,")));
        let d = f.iter().find(|x| x.phrase.eq_ignore_ascii_case("delve into")).unwrap();
        assert_eq!(&t[..], t);
        assert_eq!(t.chars().skip(d.start).take(d.end - d.start).collect::<String>(), "delve into");
        assert_eq!(d.bucket, Bucket::Frequent);
        assert!(d.sentence.starts_with("Let's delve"));
    }

    #[test]
    fn no_overlapping_double_counts() {
        let f = find("A testament to a testament to.");
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn plain_prose_is_clean() {
        assert!(find("My dog likes to run in the park on Sundays.").is_empty());
    }
}
