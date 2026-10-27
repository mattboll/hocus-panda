//! Keyboard layouts as people call them: `fr` is "French (AZERTY)" and
//! `FR` in short, `fr(bepo)` "French (BÉPO)" and `BÉPO`, as board D13 has
//! them. The common ones are named here in both languages, as XKB's
//! registry and its translations name them; the others take the
//! registry's names, read off the loop (`hocus_platform::xkb`), or their
//! code until then.

use std::collections::HashMap;

use crate::{Lang, fuzzy};

/// Names by code, in one language, as the registry gives them.
pub type Names = HashMap<String, String>;

/// The common layouts and variants: code, English and French names.
pub const KNOWN: [(&str, &str, &str); 32] = [
    ("us", "English (US)", "Anglais (US)"),
    (
        "us(intl)",
        "English (US, intl., with dead keys)",
        "Anglais (US, internat., avec touches mortes)",
    ),
    (
        "us(altgr-intl)",
        "English (intl., with AltGr dead keys)",
        "Anglais (internat., touches mortes via AltGr)",
    ),
    ("us(dvorak)", "English (Dvorak)", "Anglais (Dvorak)"),
    ("us(colemak)", "English (Colemak)", "Anglais (Colemak)"),
    ("gb", "English (UK)", "Anglais (Royaume-Uni)"),
    // XKB calls it "French": D13 says which French keyboard it is.
    ("fr", "French (AZERTY)", "Français (AZERTY)"),
    ("fr(bepo)", "French (BÉPO)", "Français (BÉPO)"),
    (
        "fr(bepo_afnor)",
        "French (BÉPO, AFNOR)",
        "Français (BÉPO, AFNOR)",
    ),
    (
        "fr(afnor)",
        "French (AZERTY, AFNOR)",
        "Français (AZERTY, AFNOR)",
    ),
    ("fr(oss)", "French (alt.)", "Français (variante)"),
    ("fr(ergol)", "French (Ergo-L)", "Français (Ergo-L)"),
    ("be", "Belgian", "Belge"),
    ("ch", "German (Switzerland)", "Allemand (Suisse)"),
    ("ch(fr)", "French (Switzerland)", "Français (Suisse)"),
    ("ca", "French (Canada)", "Français (Canada)"),
    ("de", "German", "Allemand"),
    ("es", "Spanish", "Espagnol"),
    (
        "latam",
        "Spanish (Latin American)",
        "Espagnol (Amérique latine)",
    ),
    ("it", "Italian", "Italien"),
    ("pt", "Portuguese", "Portugais"),
    ("br", "Portuguese (Brazil)", "Portugais (Brésil)"),
    ("nl", "Dutch", "Néerlandais"),
    ("se", "Swedish", "Suédois"),
    ("no", "Norwegian", "Norvégien"),
    ("dk", "Danish", "Danois"),
    ("fi", "Finnish", "Finnois"),
    ("pl", "Polish", "Polonais"),
    ("cz", "Czech", "Tchèque"),
    ("ru", "Russian", "Russe"),
    ("ua", "Ukrainian", "Ukrainien"),
    ("gr", "Greek", "Grec"),
];

/// A common layout's name.
pub fn known(code: &str, lang: Lang) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, en, fr)| match lang {
            Lang::En => *en,
            Lang::Fr => *fr,
        })
}

/// A layout's name: a common one's, the registry's, or its code.
pub fn name(code: &str, lang: Lang, names: &Names) -> String {
    known(code, lang)
        .or_else(|| names.get(code).map(String::as_str))
        .unwrap_or(code)
        .to_owned()
}

/// What the column's chip says: `FR`, `US`, `BÉPO`; the layout's code in
/// capitals otherwise, at most four letters.
pub fn short(code: &str) -> String {
    let (layout, variant) = hocus_core::keyboard::split(code).unwrap_or((code, ""));
    if layout == "fr" && variant.starts_with("bepo") {
        return "BÉPO".into();
    }
    layout
        .chars()
        .take(4)
        .flat_map(char::to_uppercase)
        .collect()
}

/// The layouts offered first, as the welcome does: for French, AZERTY and
/// BÉPO.
pub fn common(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::En => &["us", "gb", "fr"],
        Lang::Fr => &["fr", "fr(bepo)", "us"],
    }
}

/// A layout offered in a search: its code, its name in the language, and
/// in English (searched too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    pub code: String,
    pub name: String,
    pub english: String,
}

impl Offered {
    /// The registry's entry, a common layout named as above.
    pub fn new(code: &str, lang: Lang, translated: Option<&str>, english: &str) -> Self {
        Self {
            code: code.to_owned(),
            name: known(code, lang)
                .or(translated)
                .unwrap_or(english)
                .to_owned(),
            english: known(code, Lang::En).unwrap_or(english).to_owned(),
        }
    }

    /// A common layout, without a registry.
    pub fn known(code: &str, lang: Lang) -> Option<Self> {
        Some(Self::new(code, lang, None, known(code, Lang::En)?))
    }
}

/// The best `most` of `offered` for `query`, by name, English name and
/// code; accents and case do not matter ("bepo" finds "BÉPO"). The query
/// is found whole: among six hundred layouts, letters scattered in a long
/// name match too much.
pub fn search(query: &str, offered: &[Offered], most: usize) -> Vec<Offered> {
    /// `fuzzy::score` gives letters found in order at most this; a query
    /// found whole, more.
    const SCATTERED: i32 = 1_000;
    let mut scored: Vec<(i32, usize)> = offered
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            [&o.name, &o.english, &o.code]
                .into_iter()
                .filter_map(|text| fuzzy::score(query, text))
                .max()
                .filter(|s| *s > SCATTERED)
                .map(|s| (s, i))
        })
        .collect();
    // The best first; the registry's order between equals.
    scored.sort_by_key(|(s, i)| (-s, *i));
    scored
        .into_iter()
        .take(most)
        .filter_map(|(_, i)| offered.get(i).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_as_board_d13_has_them() {
        let none = Names::new();
        assert_eq!(name("fr", Lang::Fr, &none), "Français (AZERTY)");
        assert_eq!(name("us", Lang::Fr, &none), "Anglais (US)");
        assert_eq!(name("fr(bepo)", Lang::Fr, &none), "Français (BÉPO)");
        assert_eq!(name("fr(bepo)", Lang::En, &none), "French (BÉPO)");
        assert_eq!(
            (short("fr"), short("us"), short("fr(bepo)")),
            ("FR".into(), "US".into(), "BÉPO".into())
        );
        assert_eq!(short("us(intl)"), "US");
        assert_eq!(short("latam"), "LATA");
    }

    #[test]
    fn the_registry_names_the_others_and_the_code_stands_in_until_then() {
        let mut names = Names::new();
        assert_eq!(name("de(neo)", Lang::Fr, &names), "de(neo)");
        names.insert("de(neo)".into(), "Allemand (Neo 2)".into());
        assert_eq!(name("de(neo)", Lang::Fr, &names), "Allemand (Neo 2)");
        // A common one keeps its own.
        names.insert("fr".into(), "Français".into());
        assert_eq!(name("fr", Lang::Fr, &names), "Français (AZERTY)");
    }

    #[test]
    fn bepo_is_found_with_or_without_its_accent() {
        let offered = [
            Offered::new("fr", Lang::Fr, Some("Français"), "French"),
            Offered::new(
                "fr(oss)",
                Lang::Fr,
                Some("Français (variante)"),
                "French (alt.)",
            ),
            Offered::new(
                "fr(bepo)",
                Lang::Fr,
                Some("Français (BÉPO)"),
                "French (BEPO)",
            ),
            Offered::new("fr(bepo_afnor)", Lang::Fr, None, "French (BEPO, AFNOR)"),
            Offered::new("us", Lang::Fr, Some("Anglais (US)"), "English (US)"),
        ];
        for query in ["bépo", "bepo", "BEPO"] {
            let found = search(query, &offered, 8);
            let codes: Vec<_> = found.iter().map(|o| o.code.as_str()).collect();
            assert_eq!(codes, ["fr(bepo)", "fr(bepo_afnor)"], "{query}");
        }
        assert_eq!(search("bepo", &offered, 8)[0].name, "Français (BÉPO)");
        // In English too, and by code.
        assert_eq!(search("english", &offered, 8)[0].code, "us");
        assert_eq!(search("fr(oss", &offered, 8)[0].code, "fr(oss)");
        assert_eq!(search("bepo", &offered, 1).len(), 1);
        // Not letters here and there in a long name.
        let olpc = [Offered::new(
            "us(olpc2)",
            Lang::Fr,
            Some("Anglais (diviser/multiplier bascule la disposition)"),
            "English (the divide/multiply toggle the layout)",
        )];
        assert!(search("bepo", &olpc, 8).is_empty());
    }
}
