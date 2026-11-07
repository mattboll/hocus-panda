//! The search across every setting (the spec's "The search"): the
//! sections' names, the settings' names, a few nearby words for each, in
//! French and in English whatever the window's language, and each
//! setting's keys in the file (`suspend-after`). Accents and case are
//! ignored; several words must all be found.
//!
//! The order: a section's name first, then settings whose name starts a
//! word with the first word typed, then names holding it, then settings
//! found by a nearby word, then shortcuts so found, then the settings of
//! a section found by its name. Eight at most.

use unicode_normalization::UnicodeNormalization as _;

use crate::{
    lang::Lang,
    model::{Kind, Outside, SectionId, Setting, all_settings, outside_of},
};

/// At most this many results.
pub const MOST: usize = 8;

/// What a result opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Section(SectionId),
    Setting(&'static Setting),
    /// A setting outside the file (the Son page's).
    Outside(&'static Outside),
}

impl Target {
    pub fn section(self) -> SectionId {
        match self {
            Self::Section(id) => id,
            Self::Setting(setting) => setting.section,
            Self::Outside(outside) => outside.section,
        }
    }

    /// `power` or `power.suspend-after`, for the log.
    pub fn id(self) -> String {
        match self {
            Self::Section(id) => id.section().slug.to_owned(),
            Self::Setting(setting) => setting.id.to_owned(),
            Self::Outside(outside) => outside.id.to_owned(),
        }
    }
}

/// A result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub target: Target,
    /// Its name in the window's language.
    pub name: &'static str,
    /// The part of the name typed (bytes), to light.
    pub hit: Option<(usize, usize)>,
    /// The nearby word it was found by, as written.
    pub via: Option<String>,
    rank: u8,
}

/// A text with accents and case taken out, and where each of its
/// characters came from in the original (bytes).
#[derive(Debug, Clone)]
struct Folded {
    text: Vec<char>,
    from: Vec<usize>,
    /// The original's length, for a hit reaching its end.
    end: usize,
}

/// `text` without its accents or capitals, `’` as `'`.
pub fn fold(text: &str) -> String {
    folded(text).text.into_iter().collect()
}

fn folded(text: &str) -> Folded {
    let mut out = Folded {
        text: Vec::new(),
        from: Vec::new(),
        end: text.len(),
    };
    for (at, c) in text.char_indices() {
        let c = if c == '’' { '\'' } else { c };
        for d in c.nfd() {
            if unicode_normalization::char::is_combining_mark(d) {
                continue;
            }
            for l in d.to_lowercase() {
                out.text.push(l);
                out.from.push(at);
            }
        }
    }
    out
}

impl Folded {
    /// Where `word` starts in it, the first time.
    fn find(&self, word: &[char]) -> Option<usize> {
        if word.is_empty() || word.len() > self.text.len() {
            return None;
        }
        (0..=self.text.len() - word.len()).find(|&i| self.text[i..i + word.len()] == *word)
    }

    /// Whether a word of it starts at `i`.
    fn starts_word(&self, i: usize) -> bool {
        i == 0 || self.text.get(i - 1).is_some_and(|c| !c.is_alphanumeric())
    }

    /// The original's bytes for `len` characters from `i`.
    fn bytes(&self, i: usize, len: usize) -> (usize, usize) {
        let start = self.from.get(i).copied().unwrap_or(self.end);
        let end = self.from.get(i + len).copied().unwrap_or(self.end);
        (start, end - start)
    }
}

/// What a setting is found by.
struct Entry {
    target: Target,
    name: &'static str,
    folded: Folded,
    /// Nearby words, as written and folded: keywords in both languages,
    /// the name in the other one, the keys.
    near: Vec<(String, Vec<char>)>,
    /// Its section's names, folded.
    section: Vec<Vec<char>>,
}

fn entries(lang: Lang) -> Vec<Entry> {
    let other = match lang {
        Lang::French => Lang::English,
        Lang::English => Lang::French,
    };
    let chars = |s: &str| fold(s).chars().collect::<Vec<_>>();
    let section_names = |id: SectionId| vec![chars(id.name(lang)), chars(id.name(other))];
    let mut list = Vec::new();
    for id in SectionId::ALL {
        let name = id.name(lang);
        list.push(Entry {
            target: Target::Section(id),
            name,
            folded: folded(name),
            near: vec![(id.name(other).to_owned(), chars(id.name(other)))],
            section: Vec::new(),
        });
    }
    for setting in all_settings() {
        let name = setting.name.get(lang);
        let mut near: Vec<(String, Vec<char>)> = Vec::new();
        let (fr, en) = setting.keywords;
        for word in fr.iter().chain(en.iter()) {
            near.push(((*word).to_owned(), chars(word)));
        }
        near.push((
            setting.name.get(other).to_owned(),
            chars(setting.name.get(other)),
        ));
        for place in setting.places.all() {
            let dotted = place.dotted();
            near.push((dotted.clone(), chars(&dotted)));
        }
        list.push(Entry {
            target: Target::Setting(setting),
            name,
            folded: folded(name),
            near,
            section: section_names(setting.section),
        });
    }
    for outside in SectionId::ALL.into_iter().flat_map(outside_of) {
        let name = outside.name.get(lang);
        let (fr, en) = outside.keywords;
        let mut near: Vec<(String, Vec<char>)> = fr
            .iter()
            .chain(en.iter())
            .map(|word| ((*word).to_owned(), chars(word)))
            .collect();
        let other_name = outside.name.get(other);
        near.push((other_name.to_owned(), chars(other_name)));
        list.push(Entry {
            target: Target::Outside(outside),
            name,
            folded: folded(name),
            near,
            section: section_names(outside.section),
        });
    }
    list
}

/// What `query` finds, best first, eight at most.
pub fn find(query: &str, lang: Lang) -> Vec<Found> {
    let words: Vec<Vec<char>> = fold(query)
        .split_whitespace()
        .map(|w| w.chars().collect())
        .collect();
    let Some(first) = words.first() else {
        return Vec::new();
    };
    let has = |hay: &[char], word: &[char]| {
        word.len() <= hay.len() && hay.windows(word.len()).any(|w| w == word)
    };
    let mut found = Vec::new();
    for entry in entries(lang) {
        let all = words.iter().all(|word| {
            entry.folded.find(word).is_some()
                || entry.near.iter().any(|(_, n)| has(n, word))
                || entry.section.iter().any(|s| has(s, word))
        });
        if !all {
            continue;
        }
        let at = entry.folded.find(first);
        let via = entry.near.iter().find(|(_, n)| has(n, first));
        let rank = match (entry.target, at, via) {
            (Target::Section(_), Some(_), _) => 0,
            // A section found by its other language's name.
            (Target::Section(_), None, Some(_)) => 0,
            (Target::Section(_), None, None) => continue,
            (_, Some(i), _) if entry.folded.starts_word(i) => 1,
            (_, Some(_), _) => 2,
            // A shortcut found by a nearby word after the settings so
            // found: « geste » lists the touchpad's gestures, then the
            // keys that do the same (#681).
            (Target::Setting(setting), None, Some(_)) if setting.kind == Kind::Shortcut => 4,
            (_, None, Some(_)) => 3,
            (_, None, None) => 5,
        };
        found.push(Found {
            target: entry.target,
            name: entry.name,
            hit: at.map(|i| entry.folded.bytes(i, first.len())),
            via: match (at, entry.target) {
                (None, Target::Setting(_) | Target::Outside(_)) => {
                    via.map(|(word, _)| word.clone())
                }
                _ => None,
            },
            rank,
        });
    }
    // Stable: the catalogue's order within a rank.
    found.sort_by_key(|f| f.rank);
    found.truncate(MOST);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(query: &str, lang: Lang) -> Vec<&'static str> {
        find(query, lang).iter().map(|f| f.name).collect()
    }

    #[test]
    fn veille_finds_the_suspend_delay_then_the_lid_and_the_button() {
        let found = find("veille", Lang::French);
        let names: Vec<_> = found.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            [
                "Mettre en veille après",
                "Verrouiller avant la mise en veille",
                "Éteindre l’écran",
                "Couvercle fermé",
                "Bouton d’alimentation"
            ]
        );
        // The typed part of the first lit; the others by « veille ».
        let first = &found[0];
        assert_eq!(
            first.hit.map(|(a, l)| &first.name[a..a + l]),
            Some("veille")
        );
        assert_eq!(first.via, None);
        assert_eq!(found[3].via.as_deref(), Some("veille"));
        assert_eq!(found[3].hit, None);
    }

    #[test]
    fn geste_lists_the_gestures_then_the_keys_that_change_desktop() {
        let ids: Vec<String> = find("geste", Lang::French)
            .iter()
            .map(|f| f.target.id())
            .collect();
        assert_eq!(
            ids,
            [
                "touchpad.swipe-desktops",
                "touchpad.swipe-launcher",
                "touchpad.left-to-applications",
                "shortcuts.desktop-left",
                "shortcuts.desktop-right",
            ]
        );
        assert_eq!(names("gesture", Lang::English)[3], "Desktop on the left");
    }

    #[test]
    fn accents_and_case_are_ignored_and_a_section_comes_first() {
        let found = find("energie", Lang::French);
        assert_eq!(found[0].target, Target::Section(SectionId::Power));
        assert_eq!(found[0].hit, Some((0, "Énergie".len())));
        // Its settings follow, found by their section's name.
        assert!(found.len() > 1 && found.len() <= MOST, "{found:?}");
        assert!(
            found[1..]
                .iter()
                .all(|f| f.target.section() == SectionId::Power)
        );
    }

    #[test]
    fn the_moving_wallpapers_and_the_rain_are_found() {
        let ids = |q| -> Vec<String> {
            find(q, Lang::French)
                .iter()
                .map(|f| f.target.id())
                .collect()
        };
        for q in ["animé", "pluie", "étoiles"] {
            assert!(
                ids(q).iter().any(|id| id == "wallpaper.moving"),
                "{q}: {:?}",
                ids(q)
            );
        }
        assert!(
            ids("batterie")
                .iter()
                .any(|id| id == "wallpaper.on-battery")
        );
        assert!(ids("pluie").iter().any(|id| id == "windows.animation"));
        assert!(ids("animer").iter().any(|id| id == "wallpaper.animate"));
    }

    #[test]
    fn english_words_and_the_files_keys_find_too() {
        assert_eq!(names("sleep", Lang::French)[0], "Mettre en veille après");
        assert_eq!(
            names("suspend-after", Lang::French),
            ["Mettre en veille après"]
        );
        assert_eq!(
            names("idle.delay", Lang::French),
            ["Assombrir l’écran après"]
        );
        assert_eq!(names("couvercle", Lang::English)[0], "Lid closed");
        // Every word must be found.
        assert_eq!(names("veille couvercle", Lang::French), ["Couvercle fermé"]);
        assert!(find("xyzzy", Lang::French).is_empty());
        assert!(find("  ", Lang::French).is_empty());
    }
}
