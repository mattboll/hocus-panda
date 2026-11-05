//! The desktop's language and formats (6.2, issue #402): `[locale]
//! language`, `date` and `time`, once for the whole desktop. The compositor
//! reads the file and tells the shell ([`crate::ToShell::Locale`]); the
//! Settings read the same keys themselves. The bar's clock, the calendar,
//! the reminders and the lock write the time and the date as they say.

use std::{fmt, str::FromStr};

use crate::BadLine;

/// `[locale] language`: the desktop's own words. `System` follows the
/// session's locale (`LC_ALL`, `LC_MESSAGES`, `LANG`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    System,
    French,
    English,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::System, Self::French, Self::English];

    pub const fn word(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::French => "fr",
            Self::English => "en",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.word() == word)
    }

    /// What programs started afterwards are given as `LANGUAGE`, when the
    /// desktop's language is not the system's.
    pub const fn env(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::French => Some("fr"),
            Self::English => Some("en"),
        }
    }
}

/// `[locale] date`: how a date is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DateFormat {
    /// "jeudi 5 novembre", "Thursday 5 November".
    #[default]
    Long,
    /// "jeu. 5 nov.", "Thu 5 Nov".
    Short,
    /// "05/11/2026".
    Numeric,
    /// "2026-11-05".
    Iso,
}

impl DateFormat {
    pub const ALL: [Self; 4] = [Self::Long, Self::Short, Self::Numeric, Self::Iso];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
            Self::Numeric => "numeric",
            Self::Iso => "iso",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.word() == word)
    }
}

/// `[locale] time`: over 24 hours, or 12 with AM and PM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeFormat {
    H24,
    H12,
}

impl TimeFormat {
    pub const ALL: [Self; 2] = [Self::H24, Self::H12];

    pub const fn word(self) -> &'static str {
        match self {
            Self::H24 => "24h",
            Self::H12 => "12h",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.word() == word)
    }
}

/// The desktop's language and formats: written `locale LANGUAGE DATE
/// TIME` on the line, `TIME` being `locale` when the file says none (the
/// session's locale then decides: 12 hours in the English of the
/// countries that write it so).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Locale {
    pub language: Language,
    pub date: DateFormat,
    pub time: Option<TimeFormat>,
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.language.word(),
            self.date.word(),
            self.time.map_or("locale", TimeFormat::word)
        )
    }
}

impl FromStr for Locale {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.split_whitespace();
        let mut next = |what: &str| {
            words
                .next()
                .ok_or_else(|| BadLine(format!("locale: no {what}")))
        };
        let language = next("language")?;
        let date = next("date")?;
        let time = next("time")?;
        Ok(Self {
            language: Language::of(language)
                .ok_or_else(|| BadLine(format!("no language {language:?}")))?,
            date: DateFormat::of(date).ok_or_else(|| BadLine(format!("no date {date:?}")))?,
            time: match time {
                "locale" => None,
                word => {
                    Some(TimeFormat::of(word).ok_or_else(|| BadLine(format!("no time {word:?}")))?)
                }
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_reads_back_as_written() {
        for locale in [
            Locale::default(),
            Locale {
                language: Language::English,
                date: DateFormat::Iso,
                time: Some(TimeFormat::H12),
            },
        ] {
            assert_eq!(locale.to_string().parse::<Locale>().ok(), Some(locale));
        }
        assert!("fr long 13h".parse::<Locale>().is_err());
    }
}
