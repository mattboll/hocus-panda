//! The clipboard history (#71): what was copied, newest first, pinned
//! entries kept above the others and never dropped. A copy already there
//! comes back to the top; the oldest unpinned ones go past the limits.

use serde::{Deserialize, Serialize};

/// One copied text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    pub pinned: bool,
}

/// What the history keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub entries: usize,
    /// All texts together, bytes.
    pub bytes: usize,
    /// One text, bytes; a longer copy is not kept.
    pub entry_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 100,
            bytes: 1 << 20,
            entry_bytes: 256 << 10,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    /// Newest first.
    entries: Vec<Entry>,
}

impl History {
    pub fn from_entries(entries: Vec<Entry>, limits: Limits) -> Self {
        let mut history = Self { entries };
        history.trim(limits);
        history
    }

    /// Pinned first, then the others; newest first in each.
    pub fn entries(&self) -> Vec<&Entry> {
        let (pinned, rest): (Vec<&Entry>, Vec<&Entry>) =
            self.entries.iter().partition(|e| e.pinned);
        pinned.into_iter().chain(rest).collect()
    }

    /// A copy: on top, once. Whether it was kept.
    pub fn push(&mut self, text: &str, limits: Limits) -> bool {
        self.insert(text, 0, limits)
    }

    /// A copy read late: below the `newer` copies made after it, once.
    pub fn insert(&mut self, text: &str, newer: usize, limits: Limits) -> bool {
        if text.trim().is_empty() || text.len() > limits.entry_bytes {
            return false;
        }
        let pinned = self
            .entries
            .iter()
            .position(|e| e.text == text)
            .map(|i| self.entries.remove(i).pinned)
            .unwrap_or(false);
        let at = newer.min(self.entries.len());
        self.entries.insert(
            at,
            Entry {
                text: text.to_owned(),
                pinned,
            },
        );
        self.trim(limits);
        true
    }

    pub fn toggle_pin(&mut self, text: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.text == text) {
            e.pinned = !e.pinned;
        }
    }

    pub fn remove(&mut self, text: &str) {
        self.entries.retain(|e| e.text != text);
    }

    /// Forgets what is not pinned.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    fn trim(&mut self, limits: Limits) {
        let mut bytes: usize = self.entries.iter().map(|e| e.text.len()).sum();
        while self.entries.len() > limits.entries || bytes > limits.bytes {
            let Some(i) = self.entries.iter().rposition(|e| !e.pinned) else {
                break;
            };
            bytes -= self.entries.remove(i).text.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(h: &History) -> Vec<&str> {
        h.entries().iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn newest_first_once_pinned_above() {
        let l = Limits::default();
        let mut h = History::default();
        for t in ["one", "two", "three", "two"] {
            h.push(t, l);
        }
        assert_eq!(texts(&h), ["two", "three", "one"]);
        h.toggle_pin("one");
        assert_eq!(texts(&h), ["one", "two", "three"]);
        h.clear();
        assert_eq!(texts(&h), ["one"]);
        h.remove("one");
        assert!(texts(&h).is_empty());
        assert!(!h.push("  ", l), "blank copies are not kept");
    }

    #[test]
    fn a_copy_read_late_goes_below_newer_ones() {
        let l = Limits::default();
        let mut h = History::default();
        h.push("one", l);
        h.push("three", l);
        h.insert("two", 1, l);
        assert_eq!(texts(&h), ["three", "two", "one"]);
    }

    #[test]
    fn the_oldest_unpinned_go_past_the_limits() {
        let l = Limits {
            entries: 3,
            bytes: 12,
            entry_bytes: 8,
        };
        let mut h = History::default();
        h.push("aaaa", l);
        h.toggle_pin("aaaa");
        for t in ["bbbb", "cccc", "dddd"] {
            h.push(t, l);
        }
        // Three entries, twelve bytes: the oldest unpinned went.
        assert_eq!(texts(&h), ["aaaa", "dddd", "cccc"]);
        assert!(!h.push("way too long", l));
    }
}
