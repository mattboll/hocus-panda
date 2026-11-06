//! A password as it is typed in the Compte page's sheet (6.11), as the
//! bar's Wi-Fi password is (3.5): kept in one buffer that never grows (no
//! copy left behind by a reallocation), overwritten with zeros when it is
//! set again, cleared or dropped, never logged (its `Debug` says nothing
//! of it, not even its length), never written anywhere by the Settings. It
//! leaves only for the system's `passwd`, on its terminal.
//!
//! What iced copies while it is typed (the field's own value) is iced's.

/// The longest password taken, in characters.
pub const LONGEST: usize = 256;

pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new() -> Self {
        // Room for its longest in four-byte characters: never moved.
        Self(Vec::with_capacity(LONGEST * 4))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_str(&self) -> &str {
        // Only whole `str`s are put in.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Holds `text` now, if it fits; whether it did.
    pub fn set(&mut self, text: &str) -> bool {
        if text.chars().count() > LONGEST || text.len() > self.0.capacity() {
            return false;
        }
        wipe(&mut self.0);
        self.0.clear();
        self.0.extend_from_slice(text.as_bytes());
        true
    }

    /// A copy, made on purpose: the sheet keeps what was typed while the
    /// system tries it.
    pub fn duplicate(&self) -> Self {
        let mut copy = Self::new();
        copy.0.extend_from_slice(&self.0);
        copy
    }

    pub fn clear(&mut self) {
        wipe(&mut self.0);
        self.0.clear();
    }

    /// Whether it holds the same as `other`, compared in full.
    pub fn same(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(&other.0)
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_holds_what_is_typed_and_never_says_it() {
        let mut secret = Secret::new();
        assert!(secret.set("panda-1234"));
        assert_eq!(secret.as_str(), "panda-1234");
        assert!(secret.set("bambou"));
        assert_eq!(secret.as_str(), "bambou");
        assert_eq!(format!("{secret:?}"), "Secret(…)");
        let copy = secret.duplicate();
        assert!(copy.same(&secret));
        secret.clear();
        assert!(secret.is_empty());
        assert!(!copy.same(&secret));
        assert!(!secret.set(&"x".repeat(LONGEST + 1)));
    }
}
