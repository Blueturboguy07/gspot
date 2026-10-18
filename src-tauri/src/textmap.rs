//! Index translation between Rust `char` positions (what Harper reports) and
//! UTF-16 code-unit positions (what the macOS Accessibility API uses).

#[derive(Debug, Clone)]
pub struct TextMap {
    /// `prefix[i]` = number of UTF-16 units before char `i`. Length = chars + 1.
    prefix: Vec<usize>,
}

impl TextMap {
    pub fn new(text: &str) -> Self {
        let mut prefix = Vec::with_capacity(text.chars().count() + 1);
        let mut acc = 0usize;
        prefix.push(0);
        for ch in text.chars() {
            acc += ch.len_utf16();
            prefix.push(acc);
        }
        TextMap { prefix }
    }

    pub fn char_len(&self) -> usize {
        self.prefix.len() - 1
    }

    pub fn utf16_len(&self) -> usize {
        *self.prefix.last().unwrap_or(&0)
    }

    pub fn char_to_utf16(&self, ci: usize) -> usize {
        let ci = ci.min(self.char_len());
        self.prefix[ci]
    }

    /// Maps a UTF-16 unit index to the char that contains it.
    pub fn utf16_to_char(&self, u: usize) -> usize {
        match self.prefix.binary_search(&u) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }
}

/// Slice a string by UTF-16 unit range (clamped).
pub fn slice_utf16(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let s = start.min(units.len());
    let e = end.min(units.len()).max(s);
    String::from_utf16_lossy(&units[s..e])
}

/// Replace a UTF-16 unit range with `replacement`.
pub fn splice_utf16(text: &str, start: usize, len: usize, replacement: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let s = start.min(units.len());
    let e = (start + len).min(units.len()).max(s);
    let mut out: Vec<u16> = Vec::with_capacity(units.len() + replacement.len());
    out.extend_from_slice(&units[..s]);
    out.extend(replacement.encode_utf16());
    out.extend_from_slice(&units[e..]);
    String::from_utf16_lossy(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_identity() {
        let m = TextMap::new("hello world");
        assert_eq!(m.char_to_utf16(6), 6);
        assert_eq!(m.utf16_to_char(6), 6);
        assert_eq!(m.utf16_len(), 11);
    }

    #[test]
    fn astral_chars_take_two_units() {
        // "a😀b" → chars: a(0) 😀(1) b(2); utf16: a(0) 😀(1,2) b(3)
        let m = TextMap::new("a😀b");
        assert_eq!(m.char_len(), 3);
        assert_eq!(m.utf16_len(), 4);
        assert_eq!(m.char_to_utf16(2), 3);
        assert_eq!(m.utf16_to_char(3), 2);
        assert_eq!(m.utf16_to_char(2), 1, "a unit inside the surrogate pair maps to the emoji");
    }

    #[test]
    fn splice_and_slice_round_trip() {
        let t = "I has a 😀 cat";
        let s = splice_utf16(t, 2, 3, "have");
        assert_eq!(s, "I have a 😀 cat");
        assert_eq!(slice_utf16(&s, 2, 6), "have");
    }

    #[test]
    fn out_of_range_indices_clamp() {
        let m = TextMap::new("ab");
        assert_eq!(m.char_to_utf16(99), 2);
        assert_eq!(m.utf16_to_char(99), 2);
        assert_eq!(splice_utf16("ab", 50, 5, "x"), "abx");
    }
}
