//! The emoji picker's table (#71): every emoji with its English and French
//! names and keywords, from Unicode's CLDR data (`data/emoji.tsv`, made by
//! `scripts/emoji-table.py`), searched in both languages, with skin tones.

use std::sync::OnceLock;

use crate::i18n::Lang;

/// One emoji and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emoji {
    pub glyph: &'static str,
    /// It takes a skin tone.
    pub tone: bool,
    pub en: &'static str,
    en_words: &'static str,
    pub fr: &'static str,
    fr_words: &'static str,
}

impl Emoji {
    pub fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Fr => self.fr,
        }
    }

    /// The glyph with `tone` (0 none, 1 light to 5 dark), when it takes one.
    pub fn with_tone(&self, tone: u8) -> String {
        if !self.tone || tone == 0 || tone > 5 {
            return self.glyph.to_owned();
        }
        let modifier = char::from_u32(0x1F3FA + u32::from(tone)).unwrap_or_default();
        // After the first character, before what joins it (a person and a
        // job, a gesture and its direction).
        let mut chars = self.glyph.chars();
        let first = chars.next().unwrap_or_default();
        let rest: String = chars.collect::<String>();
        let rest = rest.strip_prefix('\u{FE0F}').unwrap_or(&rest);
        format!("{first}{modifier}{rest}")
    }
}

/// Every emoji, in Unicode's order.
pub fn all() -> &'static [Emoji] {
    static TABLE: OnceLock<Vec<Emoji>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/emoji.tsv")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some(Emoji {
                    glyph: f.next()?,
                    tone: f.next()? == "1",
                    en: f.next()?,
                    en_words: f.next()?,
                    fr: f.next()?,
                    fr_words: f.next()?,
                })
            })
            .collect()
    })
}

/// Lower case, without accents or ligatures: "coeur" finds "cœur".
fn fold(s: &str) -> String {
    crate::fuzzy::fold(s).replace('œ', "oe").replace('æ', "ae")
}

/// An emoji's names and keywords as they are searched, folded once: a
/// search goes through them all, and folding them at each one took
/// longer than a frame (#288).
struct Folded {
    names: [String; 2],
    words: Vec<String>,
}

/// The folded names of every emoji, in the order of [`all`].
fn folded() -> &'static [Folded] {
    static TABLE: OnceLock<Vec<Folded>> = OnceLock::new();
    TABLE.get_or_init(|| {
        all()
            .iter()
            .map(|e| Folded {
                names: [fold(e.en), fold(e.fr)],
                words: e
                    .en_words
                    .split('|')
                    .chain(e.fr_words.split('|'))
                    .map(fold)
                    .collect(),
            })
            .collect()
    })
}

/// The emoji matching `query` in either language, best first: a name that
/// is it or starts with it as a word, then a name with a word starting
/// with it, then a keyword that is it, starts with it or holds it; in
/// Unicode's order within each.
pub fn search(query: &str) -> Vec<&'static Emoji> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let q_word = format!("{q} ");
    let rank_of = |f: &Folded| -> Option<u8> {
        let names = &f.names;
        let in_name = |m: &dyn Fn(&str) -> bool| names.iter().any(|n| n.split(' ').any(m));
        // A name that is it or starts with it ("cœur" is "cœur rouge"
        // before "visage avec des cœurs").
        if names.iter().any(|n| *n == q || n.starts_with(&q_word)) {
            return Some(0);
        }
        if in_name(&|w| w == q || w.starts_with(q.as_str())) {
            return Some(1);
        }
        let words = &f.words;
        if words.contains(&q) {
            Some(2)
        } else if words.iter().any(|w| w.starts_with(&q)) {
            Some(3)
        } else if names.iter().chain(words).any(|w| w.contains(&q)) {
            Some(4)
        } else {
            None
        }
    };
    let mut ranked: Vec<(u8, usize, &Emoji)> = all()
        .iter()
        .zip(folded())
        .enumerate()
        .filter_map(|(i, (e, f))| Some((rank_of(f)?, i, e)))
        .collect();
    ranked.sort_by_key(|(rank, i, _)| (*rank, *i));
    ranked.into_iter().map(|(_, _, e)| e).collect()
}

/// The emoji whose glyph is `glyph`, with or without a tone, its
/// variation selectors aside. Compared as they are, without a copy: the
/// recent ones are looked for each time emoji show (#288).
pub fn find(glyph: &str) -> Option<&'static Emoji> {
    let bare = |s: &'static str| s.chars().filter(|c| *c != '\u{FE0F}');
    let wanted: Vec<char> = glyph
        .chars()
        .filter(|c| *c != '\u{FE0F}' && !('\u{1F3FB}'..='\u{1F3FF}').contains(c))
        .collect();
    all()
        .iter()
        .find(|e| bare(e.glyph).eq(wanted.iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeur_finds_a_heart_in_either_spelling() {
        for query in ["coeur", "cœur", "heart"] {
            let first = search(query).first().map(|e| (e.en, e.fr));
            assert!(
                first.is_some_and(|(en, fr)| en.contains("heart") && fr.contains("cœur")),
                "{query}: {first:?}"
            );
        }
        assert_eq!(search("red heart").first().map(|e| e.glyph), Some("❤️"));
        assert!(search("zzzz").is_empty());
    }

    #[test]
    fn a_tone_goes_after_the_first_character() {
        let thumbs = search("thumbs up")[0];
        assert!(thumbs.tone);
        assert_eq!(thumbs.with_tone(3), "👍🏽");
        assert_eq!(thumbs.with_tone(0), "👍");
        let heart = search("red heart")[0];
        assert_eq!(heart.with_tone(3), "❤️", "a heart takes no tone");
        assert_eq!(find("👍🏽").map(|e| e.en), Some("thumbs up"));
        // With or without its variation selector.
        assert_eq!(find("❤").map(|e| e.glyph), Some("❤️"));
        assert_eq!(find("❤️").map(|e| e.glyph), Some("❤️"));
        assert_eq!(find("zz"), None);
    }
}
