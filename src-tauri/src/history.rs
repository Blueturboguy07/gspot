//! Recent writing sessions (the clock icon in the panel).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub doc_id: String,
    pub app_name: String,
    pub bundle_id: String,
    pub title: String,
    pub words: usize,
    pub score: Option<u8>,
    pub last_seen: i64,
}

fn path() -> std::path::PathBuf {
    crate::settings::support_dir().join("history.json")
}

pub fn load() -> Vec<Session> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(list: &[Session]) {
    if let Ok(j) = serde_json::to_string(list) {
        let _ = std::fs::write(path(), j);
    }
}

pub fn title_from_text(text: &str) -> String {
    let first = text.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("Untitled");
    let mut t: String = first.chars().take(64).collect();
    if first.chars().count() > 64 {
        t.push('…');
    }
    t
}

/// Insert or refresh a session, newest first, capped at 60.
pub fn upsert(list: &mut Vec<Session>, s: Session) {
    list.retain(|x| x.doc_id != s.doc_id);
    list.insert(0, s);
    list.truncate(60);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_uses_first_nonblank_line_and_truncates() {
        assert_eq!(title_from_text("\n\n  Hello world  \nmore"), "Hello world");
        let long = "x".repeat(100);
        assert_eq!(title_from_text(&long).chars().count(), 65);
        assert_eq!(title_from_text("   "), "Untitled");
    }

    #[test]
    fn upsert_dedupes_and_moves_to_front() {
        let mk = |id: &str| Session { doc_id: id.into(), app_name: "a".into(), bundle_id: "b".into(), title: "t".into(), words: 1, score: None, last_seen: 0 };
        let mut l = vec![mk("1"), mk("2")];
        upsert(&mut l, mk("2"));
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].doc_id, "2");
    }
}
