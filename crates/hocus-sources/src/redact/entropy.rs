//! Strings too random to be words: a token nobody named, pasted as is.
//! Outside links (a link's secret parameters have their own rule) and
//! known ids: commit hashes and other hexadecimal digests, UUIDs, message
//! ids, file names, dates and versions are not secrets.

use std::sync::LazyLock;

use regex::Regex;

use super::{Found, SecretKind};

static CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{20,}").expect("a regex"));
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:https?|ftp|mailto)://\S+|\bwww\.\S+").expect("a regex"));
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .expect("a regex")
});

/// Bits per character above which a mixed string is taken for a secret.
const THRESHOLD: f64 = 3.7;

pub(super) fn find(text: &str, out: &mut Vec<Found>) {
    let links: Vec<(usize, usize)> = LINK.find_iter(text).map(|m| (m.start(), m.end())).collect();
    for m in CANDIDATE.find_iter(text) {
        if links.iter().any(|&(s, e)| m.start() < e && s < m.end()) {
            continue;
        }
        // Part of an address, a file's name or a path.
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if matches!(before, Some('@' | '.' | '/' | '\\'))
            || matches!(after, Some('@'))
            || text[m.end()..].starts_with(['.'])
                && text[m.end() + 1..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphabetic)
        {
            continue;
        }
        let token = m.as_str().trim_matches(['-', '_', '=']);
        if random(token) {
            out.push(Found::new(m.start(), m.end(), SecretKind::HighEntropy));
        }
    }
}

/// Whether `token` looks drawn at random: long enough, letters and digits
/// mixed, and its characters spread enough.
pub fn random(token: &str) -> bool {
    if token.len() < 20 {
        return false;
    }
    let hex = token.chars().all(|c| c.is_ascii_hexdigit());
    if hex || UUID.is_match(token) {
        // Digests and ids: a commit, a SHA-256, a UUID.
        return false;
    }
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let lower = token.chars().filter(char::is_ascii_lowercase).count();
    let upper = token.chars().filter(char::is_ascii_uppercase).count();
    if digits == 0 && (lower == 0 || upper == 0) {
        return false;
    }
    if digits + lower + upper == 0 || (lower + upper) == 0 {
        // Only digits: a number (a card's has its own check).
        return false;
    }
    // Words and dates joined by dashes ("release-2026-11-08-final").
    let parts: Vec<&str> = token.split(['-', '_']).filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 && parts.iter().all(|p| p.len() <= 10 && wordy(p)) {
        return false;
    }
    entropy(token) >= THRESHOLD
}

/// A word, or a number: not a run of mixed characters.
fn wordy(part: &str) -> bool {
    part.chars().all(|c| c.is_ascii_digit())
        || part.chars().all(char::is_alphabetic)
        || part
            .chars()
            .skip(1)
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part.chars().filter(char::is_ascii_digit).count() <= 2
}

/// Shannon's entropy, in bits per character.
pub fn entropy(text: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for c in text.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let total = text.chars().count() as f64;
    counts
        .values()
        .map(|&n| {
            let p = n as f64 / total;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_random_token_is_one_and_a_commit_is_not() {
        assert!(random("q8ZtR2vXyLp4MnB7wKs9Hd3F"));
        assert!(!random("3f9a1c2e4b5d6f708192a3b4c5d6e7f809a1b2c3"));
        assert!(!random("release-2026-11-08-final"));
        assert!(!random("anticonstitutionnellement"));
    }
}
