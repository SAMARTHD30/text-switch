use std::collections::HashMap;

/// Tracks the most recently typed characters and detects when they exactly
/// equal a known trigger. Separator keys (Enter/Tab/Space) reset it via the
/// caller; this struct only models the character buffer + lookup.
pub struct Matcher {
    buffer: String,
    cap: usize,
}

impl Matcher {
    /// `cap` bounds the buffer length (memory guard). It only needs to be
    /// >= the longest trigger; matching is exact-equality so a larger cap
    /// never causes false matches.
    pub fn new(cap: usize) -> Self {
        Self {
            buffer: String::new(),
            cap,
        }
    }

    /// Record one typed printable character.
    pub fn push_char(&mut self, c: char) {
        self.buffer.push(c);
        let count = self.buffer.chars().count();
        if count > self.cap {
            let skip = count - self.cap;
            self.buffer = self.buffer.chars().skip(skip).collect();
        }
    }

    /// Handle a Backspace keypress.
    pub fn backspace(&mut self) {
        self.buffer.pop();
    }

    /// Clear the buffer (called on separators and after an expansion).
    pub fn reset(&mut self) {
        self.buffer.clear();
    }

    /// If the current buffer exactly equals a known trigger, return that
    /// trigger string; otherwise None.
    pub fn check(&self, map: &HashMap<String, String>) -> Option<String> {
        if map.contains_key(&self.buffer) {
            Some(self.buffer.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(triggers: &[(&str, &str)]) -> HashMap<String, String> {
        triggers
            .iter()
            .map(|(t, r)| (t.to_string(), r.to_string()))
            .collect()
    }

    #[test]
    fn matches_exact_trigger() {
        let map = map_with(&[("cli", "X")]);
        let mut m = Matcher::new(8);
        for c in "cli".chars() {
            m.push_char(c);
        }
        assert_eq!(m.check(&map), Some("cli".to_string()));
    }

    #[test]
    fn no_match_when_buffer_differs() {
        let map = map_with(&[("cli", "X")]);
        let mut m = Matcher::new(8);
        for c in "cl".chars() {
            m.push_char(c);
        }
        assert_eq!(m.check(&map), None);
    }

    #[test]
    fn longer_word_containing_trigger_does_not_match() {
        // exact-equality: "specli" must NOT fire "cli"
        let map = map_with(&[("cli", "X")]);
        let mut m = Matcher::new(8);
        for c in "specli".chars() {
            m.push_char(c);
        }
        assert_eq!(m.check(&map), None);
    }

    #[test]
    fn backspace_removes_last_char() {
        let map = map_with(&[("cli", "X")]);
        let mut m = Matcher::new(8);
        for c in "clix".chars() {
            m.push_char(c);
        }
        m.backspace();
        assert_eq!(m.check(&map), Some("cli".to_string()));
    }

    #[test]
    fn reset_clears_buffer() {
        let map = map_with(&[("cli", "X")]);
        let mut m = Matcher::new(8);
        for c in "cli".chars() {
            m.push_char(c);
        }
        m.reset();
        assert_eq!(m.check(&map), None);
    }

    #[test]
    fn buffer_is_capped() {
        // cap = 3, type 5 chars; only the last 3 are retained.
        let map = map_with(&[("cde", "X")]);
        let mut m = Matcher::new(3);
        for c in "abcde".chars() {
            m.push_char(c);
        }
        assert_eq!(m.check(&map), Some("cde".to_string()));
    }
}
