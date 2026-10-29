//! A Wi-Fi password as it is typed: kept in one buffer that never grows
//! (no copy left behind by a reallocation), overwritten with zeros when it
//! is cleared or dropped, never logged (its `Debug` says its length only),
//! never written anywhere by the shell. It leaves only for NetworkManager,
//! over D-Bus.
//!
//! Overwriting is the best Rust allows without `unsafe`: the zeros go
//! through `std::hint::black_box`, so that the compiler does not drop the
//! writes as dead. What D-Bus copies into its message, and what iced
//! shapes when the password is shown on purpose, are theirs.

/// A WPA passphrase is 8 to 63 characters, or 64 hexadecimal digits.
pub const SHORTEST: usize = 8;
pub const LONGEST: usize = 64;

pub struct Secret(Vec<u8>);

impl Secret {
    /// Room for the longest password in four-byte characters, so that the
    /// buffer is never moved.
    const ROOM: usize = LONGEST * 4;

    pub fn new() -> Self {
        Self(Vec::with_capacity(Self::ROOM))
    }

    /// A copy, made on purpose (the popover keeps what was typed while
    /// NetworkManager tries it).
    pub fn duplicate(&self) -> Self {
        let mut copy = Self::new();
        copy.0.extend_from_slice(&self.0);
        copy
    }

    pub fn chars(&self) -> usize {
        self.as_str().chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Long enough to try.
    pub fn complete(&self) -> bool {
        self.chars() >= SHORTEST
    }

    pub fn as_str(&self) -> &str {
        // Only whole `str`s are pushed, and only whole characters popped.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Adds `text` if it fits; whether it did.
    pub fn push(&mut self, text: &str) -> bool {
        let fits = self.chars() + text.chars().count() <= LONGEST
            && self.0.len() + text.len() <= Self::ROOM;
        if fits {
            self.0.extend_from_slice(text.as_bytes());
        }
        fits
    }

    /// Takes its last character away.
    pub fn pop(&mut self) {
        let Some((at, _)) = self.as_str().char_indices().next_back() else {
            return;
        };
        wipe(&mut self.0[at..]);
        self.0.truncate(at);
    }

    pub fn clear(&mut self) {
        wipe(&mut self.0);
        self.0.clear();
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
        write!(f, "Secret({} characters)", self.chars())
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
    fn it_takes_whole_characters_up_to_64_and_never_says_itself() {
        let mut secret = Secret::new();
        for text in ["a", "z", "e", "r", "t", "y", "&"] {
            assert!(secret.push(text));
        }
        assert!(!secret.complete(), "7 characters");
        assert!(secret.push("1"));
        assert!(secret.complete());
        assert!(secret.push("@") && secret.push("ê"));
        assert_eq!(secret.as_str(), "azerty&1@ê");
        assert_eq!(secret.chars(), 10);
        assert!(secret.complete());
        secret.pop();
        assert_eq!(secret.as_str(), "azerty&1@");
        assert_eq!(format!("{secret:?}"), "Secret(9 characters)");
        let copy = secret.duplicate();
        secret.clear();
        assert!(secret.is_empty());
        assert_eq!(copy.as_str(), "azerty&1@");
        // 64 at most, and the buffer is never moved.
        let mut long = Secret::new();
        let at = long.0.as_ptr();
        for _ in 0..64 {
            assert!(long.push("€"));
        }
        assert!(!long.push("a"));
        assert_eq!(long.0.as_ptr(), at);
        let mut short = Secret::new();
        short.pop();
        assert!(short.is_empty() && !short.complete());
    }
}
