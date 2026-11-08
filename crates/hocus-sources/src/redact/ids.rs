//! Numbers that are secrets by their form, each checked by its own key so
//! that a price or an order number is not taken for one: IBANs (mod 97),
//! card numbers (Luhn and their networks' prefixes), French social
//! security numbers (NIR, their key), French passport numbers (their
//! form). Identity card and tax numbers have no form of their own: they
//! are found by their labels ([`super::labelled`]).

use std::sync::LazyLock;

use regex::Regex;

use super::{Found, SecretKind};

static IBAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,3})?\b").expect("a regex")
});

/// Digits in groups of 4 (spaces or dashes), Amex's 4-6-5, or a run of
/// 13 to 19.
static CARD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:\d{4}(?: \d{4}){2} \d{1,7}|\d{4}(?:-\d{4}){2}-\d{1,7}|\d{4} \d{6} \d{4,5}|\d{13,19})\b",
    )
    .expect("a regex")
});

/// Sex, year, month (or 20 and over, as the register writes some), the
/// department (2A and 2B for Corsica), the commune, the order, the key.
static NIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b([12])[ .]?(\d{2})[ .]?(0[1-9]|1[0-2]|[2-9]\d)[ .]?(\d{2}|2[AB])[ .]?(\d{3})[ .]?(\d{3})[ .]?(\d{2})\b",
    )
    .expect("a regex")
});

static PASSPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{2}[A-Z]{2}\d{5}\b").expect("a regex"));

pub(super) fn find(text: &str, out: &mut Vec<Found>) {
    for m in IBAN.find_iter(text) {
        if iban_valid(m.as_str()) {
            out.push(Found::new(m.start(), m.end(), SecretKind::Iban));
        }
    }
    for m in CARD.find_iter(text) {
        if card_valid(m.as_str()) {
            out.push(Found::new(m.start(), m.end(), SecretKind::Card));
        }
    }
    for c in NIR.captures_iter(text) {
        let Some(m) = c.get(0) else { continue };
        if nir_valid(m.as_str()) {
            out.push(Found::new(m.start(), m.end(), SecretKind::SocialSecurity));
        }
    }
    for m in PASSPORT.find_iter(text) {
        out.push(Found::new(m.start(), m.end(), SecretKind::Passport));
    }
}

/// An IBAN whose length suits its country (when known) and whose mod 97
/// is 1.
pub fn iban_valid(text: &str) -> bool {
    let iban: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !(15..=34).contains(&iban.len()) {
        return false;
    }
    let lengths = [
        ("FR", 27),
        ("MC", 27),
        ("DE", 22),
        ("BE", 16),
        ("ES", 24),
        ("IT", 27),
        ("NL", 18),
        ("LU", 20),
        ("CH", 21),
        ("GB", 22),
        ("PT", 25),
        ("IE", 22),
    ];
    if let Some((_, length)) = lengths.iter().find(|(c, _)| iban.starts_with(c))
        && iban.len() != *length
    {
        return false;
    }
    let (head, tail) = iban.split_at(4);
    let mut remainder: u32 = 0;
    for c in tail.chars().chain(head.chars()) {
        let value = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        let digits = if value >= 10 { 100 } else { 10 };
        remainder = (remainder * digits + value) % 97;
    }
    remainder == 1
}

/// Luhn's sum, on a number of a card network's length and prefix (Visa,
/// Mastercard, Amex, Discover, CB on Visa and Mastercard, JCB, Maestro).
pub fn card_valid(text: &str) -> bool {
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let prefix = |n: usize| digits.iter().take(n).fold(0u32, |acc, d| acc * 10 + d);
    let network = match digits[0] {
        4 => true,
        5 => (51..=55).contains(&prefix(2)) || (50..=59).contains(&prefix(2)),
        2 => (2221..=2720).contains(&prefix(4)),
        3 => matches!(prefix(2), 34 | 37) || (3528..=3589).contains(&prefix(4)),
        6 => prefix(4) == 6011 || prefix(2) == 65 || matches!(prefix(2), 62 | 67),
        _ => false,
    };
    if !network {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let d = d * 2;
                if d > 9 { d - 9 } else { d }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// A NIR whose key is 97 less its first 13 digits modulo 97 (Corsica's
/// 2A and 2B counted as 19 and 18).
pub fn nir_valid(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if compact.len() != 15 {
        return false;
    }
    let (body, key) = compact.split_at(13);
    let Ok(key) = key.parse::<u64>() else {
        return false;
    };
    let body = body.replace("2A", "19").replace("2B", "18");
    let Ok(number) = body.parse::<u64>() else {
        return false;
    };
    97 - number % 97 == key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ibans_are_checked_by_their_key() {
        assert!(iban_valid("FR76 3000 6000 0112 3456 7890 189"));
        assert!(iban_valid("DE89370400440532013000"));
        assert!(!iban_valid("FR76 3000 6000 0112 3456 7890 188"));
    }

    #[test]
    fn cards_are_checked_by_luhn() {
        assert!(card_valid("4970 1012 3456 7893"));
        assert!(card_valid("4111111111111111"));
        assert!(!card_valid("4111111111111112"));
        // A number Luhn takes but no network starts with.
        assert!(!card_valid("0000000000000000"));
    }

    #[test]
    fn a_social_security_number_is_checked_by_its_key() {
        assert!(nir_valid("1 85 05 78 006 084 91"));
        assert!(!nir_valid("1 85 05 78 006 084 36"));
        assert!(nir_valid("2 69 05 2A 004 015 25"));
    }
}
