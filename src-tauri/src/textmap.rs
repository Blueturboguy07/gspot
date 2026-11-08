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

/// A UTF-16 offset into `text`, counted the way a text provider counts
/// characters: CR LF as one unit (`crlf_as_one`) and/or one unit per code
/// point (`by_codepoint`) instead of one per UTF-16 unit. Windows UI Automation
/// providers differ here; `uia.rs` probes which convention a field uses.
pub fn provider_units(text: &str, u16_off: usize, crlf_as_one: bool, by_codepoint: bool) -> usize {
    if !crlf_as_one && !by_codepoint {
        return u16_off;
    }
    let mut u16 = 0usize;
    let mut units = 0usize;
    let mut prev_cr = false;
    for ch in text.chars() {
        if u16 >= u16_off {
            break;
        }
        let l = ch.len_utf16();
        if !(crlf_as_one && ch == '\n' && prev_cr) {
            units += if by_codepoint { 1 } else { l };
        }
        prev_cr = ch == '\r';
        u16 += l;
    }
    units + u16_off.saturating_sub(u16)
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

    #[test]
    fn provider_units_follow_the_convention() {
        let t = "ab\r\ncd😀e";
        // UTF-16 offsets: a0 b1 \r2 \n3 c4 d5 😀6-7 e8
        assert_eq!(provider_units(t, 4, false, false), 4);
        assert_eq!(provider_units(t, 4, true, false), 3, "CR LF counts once");
        assert_eq!(provider_units(t, 8, false, true), 7, "the emoji counts once");
        assert_eq!(provider_units(t, 8, true, true), 6);
        assert_eq!(provider_units(t, 9, true, true), 7);
        assert_eq!(provider_units(t, 12, true, false), 11, "past the end counts on");
    }
}
