//! The emoji the picker offers, and their search (5.7).
//!
//! The table is Unicode's and CLDR's, bundled at build time
//! (`data/emoji/emoji.tsv`, made by `scripts/emoji/generate.py` from
//! Unicode 15.1's `emoji-test.txt` and CLDR 44's annotations, Unicode
//! License v3 beside it): every emoji a keyboard offers, in Unicode's
//! order, its skin tones aside, with its names and keywords in English and
//! in French. It is read once, the first time it is needed.

use std::sync::LazyLock;

use crate::{clock::time::Lang, launcher::search::fold};

static TABLE: &str = include_str!("../../data/emoji/emoji.tsv");

/// The picker's groups, in its order (its tabs, the recent ones aside).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Smileys,
    Gestures,
    Hearts,
    Animals,
    Food,
    Objects,
    Symbols,
    Flags,
}

impl Group {
    pub const ALL: [Self; 8] = [
        Self::Smileys,
        Self::Gestures,
        Self::Hearts,
        Self::Animals,
        Self::Food,
        Self::Objects,
        Self::Symbols,
        Self::Flags,
    ];

    fn of(word: &str) -> Option<Self> {
        Some(match word {
            "smileys" => Self::Smileys,
            "gestures" => Self::Gestures,
            "hearts" => Self::Hearts,
            "animals" => Self::Animals,
            "food" => Self::Food,
            "objects" => Self::Objects,
            "symbols" => Self::Symbols,
            "flags" => Self::Flags,
            _ => return None,
        })
    }

    /// Its tab's emoji, the mockup's.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Smileys => "😀",
            Self::Gestures => "👍",
            Self::Hearts => "❤️",
            Self::Animals => "🐼",
            Self::Food => "☕",
            Self::Objects => "💡",
            Self::Symbols => "✅",
            Self::Flags => "🏳️",
        }
    }

    /// Its heading (in capitals as drawn) and tab's name.
    pub fn name(self, lang: Lang) -> &'static str {
        let fr = lang == Lang::French;
        match self {
            Self::Smileys => "Smileys",
            Self::Gestures if fr => "Gestes",
            Self::Gestures => "Gestures",
            Self::Hearts if fr => "Cœurs",
            Self::Hearts => "Hearts",
            Self::Animals if fr => "Animaux",
            Self::Animals => "Animals",
            Self::Food if fr => "Nourriture",
            Self::Food => "Food",
            Self::Objects if fr => "Objets",
            Self::Objects => "Objects",
            Self::Symbols if fr => "Symboles",
            Self::Symbols => "Symbols",
            Self::Flags if fr => "Drapeaux",
            Self::Flags => "Flags",
        }
    }
}

/// The skin tones: yellow (none), then the five Fitzpatrick modifiers,
/// light to dark.
pub const TONES: [Option<char>; 6] = [
    None,
    Some('\u{1F3FB}'),
    Some('\u{1F3FC}'),
    Some('\u{1F3FD}'),
    Some('\u{1F3FE}'),
    Some('\u{1F3FF}'),
];

/// One emoji.
#[derive(Debug)]
pub struct Emoji {
    pub group: Group,
    pub text: &'static str,
    /// With the light tone on each of its people; `None` without tones.
    light: Option<&'static str>,
    /// Its names, English then French.
    names: [&'static str; 2],
    /// Its names folded, and the folded words of its names and of its
    /// keywords, both languages.
    folded: [String; 2],
    name_words: Vec<String>,
    keyword_words: Vec<String>,
}

impl Emoji {
    /// Its name in `lang`.
    pub fn name(&self, lang: Lang) -> &'static str {
        if lang == Lang::French {
            self.names[1]
        } else {
            self.names[0]
        }
    }

    /// What it is with skin tone `tone` (an index of [`TONES`]): itself
    /// without tones, or for the yellow one.
    pub fn with_tone(&self, tone: usize) -> String {
        match (self.light, TONES.get(tone).copied().flatten()) {
            (Some(light), Some(modifier)) => light.replace('\u{1F3FB}', &modifier.to_string()),
            _ => self.text.to_owned(),
        }
    }
}

/// Every emoji, in the picker's order: by group, Unicode's within each.
#[derive(Debug)]
pub struct Table {
    pub emoji: Vec<Emoji>,
}

static READ: LazyLock<Table> = LazyLock::new(|| Table::read(TABLE));

/// The table, read the first time.
pub fn table() -> &'static Table {
    &READ
}

/// The words of `text`, folded (case and accents ignored, `œ` as `oe`).
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    fold(text)
        .replace('œ', "oe")
        .replace('æ', "ae")
        .split(|c: char| !c.is_alphanumeric() && c != '+')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
}

impl Table {
    fn read(text: &'static str) -> Self {
        let mut emoji: Vec<Emoji> = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let group = Group::of(fields.next()?)?;
                let text = fields.next()?;
                let light = fields.next().filter(|l| *l != "-");
                let (en, en_keys) = (fields.next()?, fields.next()?);
                let (fr, fr_keys) = (fields.next()?, fields.next()?);
                let name_words = words(en).chain(words(fr)).collect();
                let whole = |n: &str| words(n).collect::<Vec<_>>().join(" ");
                let folded = [whole(en), whole(fr)];
                let keyword_words = [en_keys, fr_keys]
                    .iter()
                    .flat_map(|keys| keys.split('|'))
                    .flat_map(words)
                    .collect();
                Some(Emoji {
                    group,
                    text,
                    light,
                    names: [en, fr],
                    folded,
                    name_words,
                    keyword_words,
                })
            })
            .collect();
        // Unicode's order puts a few of a group's after another's (the
        // emotions after the hearts): grouped, the order kept within.
        emoji.sort_by_key(|e| Group::ALL.iter().position(|g| *g == e.group));
        Self { emoji }
    }

    /// The emoji whose text is `text`.
    pub fn find(&self, text: &str) -> Option<usize> {
        self.emoji.iter().position(|e| e.text == text)
    }

    /// Where each group starts, in [`Group::ALL`]'s order.
    pub fn groups(&self) -> Vec<(Group, std::ops::Range<usize>)> {
        Group::ALL
            .iter()
            .filter_map(|g| {
                let start = self.emoji.iter().position(|e| e.group == *g)?;
                let end = self.emoji[start..]
                    .iter()
                    .position(|e| e.group != *g)
                    .map_or(self.emoji.len(), |n| start + n);
                Some((*g, start..end))
            })
            .collect()
    }

    /// The emoji matching `query`: each of its words starts a word of an
    /// emoji's names or keywords, in either language (or is inside one,
    /// from three letters). The best first: its whole name, then a whole
    /// word of its name, then its name's words' starts, then its
    /// keywords', then insides; Unicode's order within each.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let wanted: Vec<String> = words(query).collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let whole = wanted.join(" ");
        let mut found: Vec<(u8, usize)> = self
            .emoji
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let mut worst = 0;
                for w in &wanted {
                    worst = worst.max(rank(e, w)?);
                }
                // Its whole name, first.
                if e.folded.contains(&whole) {
                    worst = 0;
                }
                Some((worst, i))
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, i)| i).collect()
    }
}

/// How well the folded word `w` matches emoji `e`: lower is better.
fn rank(e: &Emoji, w: &str) -> Option<u8> {
    let inside = w.chars().count() >= 3;
    if e.name_words.iter().any(|n| n == w) {
        Some(1)
    } else if e.name_words.iter().any(|n| n.starts_with(w)) {
        Some(2)
    } else if e.keyword_words.iter().any(|k| k.starts_with(w)) {
        Some(3)
    } else if inside
        && e.name_words
            .iter()
            .chain(&e.keyword_words)
            .any(|n| n.contains(w))
    {
        Some(4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(found: &[usize]) -> Vec<&'static str> {
        found.iter().map(|i| table().emoji[*i].text).collect()
    }

    #[test]
    fn the_table_holds_every_group_in_order() {
        let table = table();
        assert!(table.emoji.len() > 1800, "{}", table.emoji.len());
        let groups: Vec<Group> = table.groups().into_iter().map(|(g, _)| g).collect();
        assert_eq!(groups, Group::ALL);
        let hearts = &table.groups()[2].1;
        assert!(texts(&hearts.clone().collect::<Vec<_>>()).contains(&"❤️"));
        assert_eq!(table.emoji[0].text, "😀");
    }

    #[test]
    fn a_search_reads_french_and_english_without_accents_nor_case() {
        let table = table();
        let chat = texts(&table.search("chat"));
        assert!(chat.contains(&"🐱") && chat.contains(&"🐈"), "{chat:?}");
        // The emoji named so first.
        assert_eq!(chat[0], "🐈");
        let cat = texts(&table.search("Cat"));
        assert!(cat.contains(&"🐱"), "{cat:?}");
        // "coeur" finds "cœur", "etoile" finds "étoile".
        assert!(texts(&table.search("coeur")).contains(&"❤️"));
        assert!(texts(&table.search("ÉTOILE")).contains(&"⭐"));
        // Words match from their start; inside, from three letters.
        assert!(texts(&table.search("pan")).contains(&"🐼"));
        assert!(!texts(&table.search("nd")).contains(&"🐼"));
        assert!(texts(&table.search("ire")).contains(&"🔥"));
        assert!(table.search("zzzz").is_empty());
        // Every word of the query.
        let thumbs = texts(&table.search("pouce haut"));
        assert_eq!(thumbs.first(), Some(&"👍"), "{thumbs:?}");
    }

    #[test]
    fn a_tone_goes_on_each_person() {
        let table = table();
        let thumbs = &table.emoji[table.find("👍").expect("👍")];
        assert!(thumbs.light.is_some());
        assert_eq!(thumbs.with_tone(0), "👍");
        assert_eq!(thumbs.with_tone(3), "👍\u{1F3FD}");
        let pair = &table.emoji[table.find("🧑‍🤝‍🧑").expect("pair")];
        assert_eq!(
            pair.with_tone(5),
            "🧑\u{1F3FF}\u{200D}🤝\u{200D}🧑\u{1F3FF}"
        );
        let cat = &table.emoji[table.find("🐱").expect("🐱")];
        assert!(cat.light.is_none());
        assert_eq!(cat.with_tone(2), "🐱");
        assert_eq!(cat.name(Lang::French), "tête de chat");
        assert_eq!(cat.name(Lang::English), "cat face");
    }
}
