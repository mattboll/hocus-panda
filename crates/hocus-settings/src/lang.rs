//! The window's language, French or English, and the words it says in
//! both: every string the user reads is a [`Words`].

/// The window's language: the workspace's one.
pub use hocus_words::Lang;

/// Whether a language is French: the sites that still choose between two
/// languages in code, not in the catalogue (#775's next lot).
pub trait French {
    fn fr(self) -> bool;
}

impl French for Lang {
    fn fr(self) -> bool {
        self == Lang::French
    }
}

/// A text in French and in English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub fr: &'static str,
    pub en: &'static str,
}

impl Words {
    pub const fn new(fr: &'static str, en: &'static str) -> Self {
        Self { fr, en }
    }

    pub fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::French => self.fr,
            Lang::English => self.en,
        }
    }
}

/// A text of the catalogue (`hocus_words::settings`), for the tables.
pub const fn w(text: hocus_words::Text) -> Words {
    Words::new(text.get(Lang::French), text.get(Lang::English))
}

/// A text the catalogue does not hold yet: built from parts in code
/// (#775's next lot).
pub const fn pair(fr: &'static str, en: &'static str) -> Words {
    Words::new(fr, en)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Every key the Settings moved to the catalogue says, byte for byte,
    /// what its pair said in the code before (#775).
    #[test]
    fn the_catalogue_says_what_the_code_said() {
        let said = |lang| {
            hocus_words::every(lang)
                .into_iter()
                .collect::<HashMap<_, _>>()
        };
        let (fr, en) = (said(Lang::French), said(Lang::English));
        for (key, before_fr, before_en) in crate::words_before::BEFORE {
            assert_eq!(fr.get(*key).map(String::as_str), Some(*before_fr), "{key}");
            assert_eq!(en.get(*key).map(String::as_str), Some(*before_en), "{key}");
        }
        // The arms that wrote with format!: the same words around their
        // placeholders, whatever their names.
        let bare = |s: &str| {
            let mut out = String::new();
            let mut inside = false;
            for c in s.replace("{{", "\u{1}").replace("}}", "\u{2}").chars() {
                match c {
                    '{' => inside = true,
                    '}' => {
                        inside = false;
                        out.push_str("{}");
                    }
                    _ if inside => {}
                    '\u{1}' => out.push('{'),
                    '\u{2}' => out.push('}'),
                    c => out.push(c),
                }
            }
            out
        };
        for (key, before_fr, before_en) in crate::words_before::FORMATS {
            let fr = fr.get(*key).unwrap_or_else(|| panic!("{key} is gone"));
            let en = en.get(*key).unwrap_or_else(|| panic!("{key} is gone"));
            assert_eq!(bare(fr), bare(before_fr), "{key}");
            assert_eq!(bare(en), bare(before_en), "{key}");
        }
    }
}
