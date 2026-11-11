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

/// Shorthand for the catalogue's tables.
pub const fn w(fr: &'static str, en: &'static str) -> Words {
    Words::new(fr, en)
}
