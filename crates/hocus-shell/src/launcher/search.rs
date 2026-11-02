//! What the launcher's field finds (5.1): a fuzzy match on an
//! application's name, its generic name and its keywords, the letters in
//! their order, word starts and letters that follow each other counting
//! more; accents and case ignored. The 5.1 mockup's scoring, in Rust.

use unicode_normalization::UnicodeNormalization;

/// `text` as the search compares it: in lower case, without its accents,
/// its apostrophes made spaces.
pub fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .flat_map(char::to_lowercase)
        .map(|c| if matches!(c, '\'' | '’') { ' ' } else { c })
        .collect()
}

/// Whether the letter at `at` of `text` starts a word.
fn starts_word(text: &[char], at: usize) -> bool {
    at == 0 || matches!(text.get(at - 1), Some(' ' | '-'))
}

/// How well `query` (folded) matches `text` (folded): its letters, spaces
/// left out, found in order, each preferring a word's start (or the letter
/// right after the last one found); `None` when one is not there.
pub fn fuzzy(text: &str, query: &str) -> Option<f64> {
    let text: Vec<char> = text.chars().collect();
    let mut from = 0;
    let mut score = 0.0;
    let mut last: Option<usize> = None;
    for wanted in query.chars().filter(|c| *c != ' ') {
        let next = (from..text.len()).find(|&k| text[k] == wanted)?;
        let follows = last.is_some_and(|l| next == l + 1);
        let found = if follows {
            next
        } else {
            (from..text.len())
                .find(|&k| text[k] == wanted && starts_word(&text, k))
                .unwrap_or(next)
        };
        if found == 0 {
            score += 12.0;
        } else if starts_word(&text, found) {
            score += 9.0;
        }
        if last.is_some_and(|l| found == l + 1) {
            score += 6.0;
        }
        score += 1.0 - (found - from).min(6) as f64 * 0.4;
        last = Some(found);
        from = found + 1;
    }
    Some(score - text.len() as f64 * 0.05)
}

/// What the search compares of an application, folded once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Folded {
    pub name: String,
    pub generic: String,
    pub keywords: String,
}

impl Folded {
    pub fn of(name: &str, generic: Option<&str>, keywords: &[String]) -> Self {
        Self {
            name: fold(name),
            generic: generic.map(fold).unwrap_or_default(),
            keywords: fold(&keywords.join(" ")),
        }
    }

    /// Its best score for `query` (folded): the name's, the generic
    /// name's at three quarters, the keywords' at 0.6; `None` when none
    /// match.
    pub fn score(&self, query: &str) -> Option<f64> {
        let parts = [
            (self.name.as_str(), 1.0),
            (self.generic.as_str(), 0.75),
            (self.keywords.as_str(), 0.6),
        ];
        parts
            .into_iter()
            .filter(|(text, _)| !text.is_empty())
            .filter_map(|(text, weight)| fuzzy(text, query).map(|s| s * weight))
            .reduce(f64::max)
    }
}

/// The applications matching `query` (as typed), best first: by score,
/// ties going to the most launched (`launches` of each), then by name.
/// Each is `(folded, launches)`; what comes back are their indices.
pub fn rank(apps: &[(&Folded, u32)], query: &str) -> Vec<usize> {
    let query = fold(query.trim());
    if query.chars().all(|c| c == ' ') {
        return Vec::new();
    }
    let mut found: Vec<(usize, f64)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, (folded, _))| Some((i, folded.score(&query)?)))
        .collect();
    found.sort_by(|(a, sa), (b, sb)| {
        sb.total_cmp(sa)
            .then(apps[*b].1.cmp(&apps[*a].1))
            .then(apps[*a].0.name.cmp(&apps[*b].0.name))
    });
    found.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_case_and_apostrophes_are_folded() {
        assert_eq!(fold("Éditeur de Texte"), "editeur de texte");
        assert_eq!(fold("Retouche d’images"), "retouche d images");
        assert_eq!(fold("Paramètres"), "parametres");
    }

    #[test]
    fn word_starts_and_runs_count_more() {
        // "fire" in Firefox: from its start, every letter after the last.
        let firefox = fuzzy("firefox", "fire").expect("a match");
        let scattered = fuzzy("forest fire", "fire").expect("a match");
        assert!(firefox > scattered, "{firefox} {scattered}");
        assert_eq!(fuzzy("kitty", "fire"), None);
        // Letters in their order only.
        assert_eq!(fuzzy("abc", "cba"), None);
        // A word's start in the middle of a name.
        let calc = fuzzy("libreoffice calc", "calc").expect("a match");
        let inside = fuzzy("localc", "calc").expect("a match");
        assert!(calc > inside, "{calc} {inside}");
    }

    #[test]
    fn the_best_first_ties_to_the_most_launched() {
        let firefox = Folded::of("Firefox", Some("Navigateur web"), &["browser".into()]);
        let chromium = Folded::of("Chromium", Some("Navigateur web"), &["browser".into()]);
        let editor = Folded::of("Éditeur de texte", Some("Éditeur"), &[]);
        let apps = [(&firefox, 0), (&chromium, 0), (&editor, 4)];
        assert_eq!(rank(&apps, "fire"), [0]);
        // Both are browsers by their generic name: a tie, Chromium first
        // by its name, then Firefox when it is launched more.
        assert_eq!(rank(&apps, "navig"), [1, 0]);
        let apps = [(&firefox, 3), (&chromium, 1), (&editor, 4)];
        assert_eq!(rank(&apps, "navig"), [0, 1]);
        // Accents and case do not count.
        assert_eq!(rank(&apps, "EDIT"), [2]);
        assert_eq!(rank(&apps, "édit"), [2]);
        assert_eq!(rank(&apps, "zzz"), Vec::<usize>::new());
        assert_eq!(rank(&apps, "   "), Vec::<usize>::new());
    }
}
