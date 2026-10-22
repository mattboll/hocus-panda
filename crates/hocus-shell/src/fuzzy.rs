//! Fuzzy matching for the launcher: what was typed against a name, the
//! best match first. A match at the start beats one at a word's start,
//! which beats letters found in order; shorter names win ties.

/// The score of `query` against `text`, higher is better; `None` when the
/// letters are not all there, in order. Case and accents are ignored.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q = fold(query);
    if q.is_empty() {
        return Some(0);
    }
    let t = fold(text);
    let length = -(t.chars().count() as i32);
    if t.starts_with(&q) {
        return Some(10_000 + length);
    }
    let word_start = t
        .match_indices(&q)
        .any(|(i, _)| i == 0 || !t[..i].ends_with(char::is_alphanumeric));
    if word_start {
        return Some(5_000 + length);
    }
    if t.contains(&q) {
        return Some(2_000 + length);
    }
    // Letters in order: closer together is better.
    let mut gaps = 0;
    let mut chars = t.chars();
    for wanted in q.chars() {
        let mut skipped = 0;
        loop {
            match chars.next() {
                Some(c) if c == wanted => break,
                Some(_) => skipped += 1,
                None => return None,
            }
        }
        gaps += skipped;
    }
    Some(1_000 - gaps * 10 + length)
}

/// Lower case, without the accents of Latin letters.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_beats_a_word_beats_letters_in_order() {
        let fir = |t| score("fir", t).unwrap_or(i32::MIN);
        assert!(fir("Firefox") > fir("Mozilla Firefox"));
        assert!(fir("Mozilla Firefox") > fir("Fichiers à renommer"));
        assert_eq!(score("fir", "Terminal"), None);
    }

    #[test]
    fn case_and_accents_do_not_matter() {
        assert!(score("reglages", "Réglages").is_some());
        assert!(score("CALC", "calculatrice").is_some());
    }
}
