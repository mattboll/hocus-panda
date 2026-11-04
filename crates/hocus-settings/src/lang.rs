//! The window's language, French or English, and the words it says in
//! both: every string the user reads is a [`Words`].

/// The two languages shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    French,
    #[default]
    English,
}

impl Lang {
    /// The messages' language: `LC_ALL`, `LC_MESSAGES`, then `LANG`, as
    /// the shell reads it.
    pub fn of_messages() -> Self {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::of(&locale)
    }

    /// From a locale's name, `fr_FR.UTF-8`.
    pub fn of(locale: &str) -> Self {
        if locale.starts_with("fr") {
            Self::French
        } else {
            Self::English
        }
    }

    pub fn fr(self) -> bool {
        self == Self::French
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
