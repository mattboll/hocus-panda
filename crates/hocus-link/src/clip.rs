//! What the clipboard's history is told and asks (bar.md 5.6, 6.17).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::BadLine;

/// What the clipboard's history keeps of a selection (5.6): its text (the
/// `text/plain;charset=utf-8` offer), or a PNG image, as offered.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub kind: ClipKind,
    /// On the line, in base64.
    #[serde(with = "crate::wire::base64")]
    pub data: Vec<u8>,
}

impl fmt::Debug for Clip {
    // What was copied may be private: only its kind and its size.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Clip({}, {} bytes)", self.kind.word(), self.data.len())
    }
}

/// A [`Clip`]'s kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClipKind {
    Text,
    Png,
}

impl ClipKind {
    pub fn word(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Png => "png",
        }
    }

    /// The types a selection of this kind is offered as.
    pub fn types(self) -> Vec<String> {
        match self {
            Self::Text => vec![
                "text/plain;charset=utf-8",
                "text/plain",
                "UTF8_STRING",
                "TEXT",
                "STRING",
            ],
            Self::Png => vec!["image/png"],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
}

/// The most a clip holds: a larger offer is not kept (5.6).
pub const CLIP_MAX: usize = 8 * 1024 * 1024;

/// How long the clipboard's history keeps what is not pinned
/// (`[clipboard] keep`, 6.17): until the session ends (never on disk), a
/// day, a week or 30 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClipKeep {
    Session,
    Day,
    Week,
    #[default]
    Month,
}

impl ClipKeep {
    pub const ALL: [Self; 4] = [Self::Session, Self::Day, Self::Week, Self::Month];

    /// As the file and the line say it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Day => "1d",
            Self::Week => "7d",
            Self::Month => "30d",
        }
    }

    pub fn of(word: &str) -> Result<Self, BadLine> {
        Self::ALL
            .into_iter()
            .find(|k| k.word() == word)
            .ok_or_else(|| BadLine(format!("keep {word:?}: session, 1d, 7d or 30d")))
    }

    /// How many days an item is kept; none: as long as the session.
    pub const fn days(self) -> Option<i64> {
        match self {
            Self::Session => None,
            Self::Day => Some(1),
            Self::Week => Some(7),
            Self::Month => Some(30),
        }
    }
}

/// What the clipboard's history keeps (`[clipboard]`, 6.17): whether it
/// keeps anything new (off, only the pinned items show, the others erased
/// a moment later), and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRules {
    pub history: bool,
    pub keep: ClipKeep,
}

impl Default for ClipRules {
    fn default() -> Self {
        Self {
            history: true,
            keep: ClipKeep::Month,
        }
    }
}

/// The application the clipboard's history says a capture was copied from
/// (5.8b): the history names it "Capture d'écran" / "Screenshot".
pub const CAPTURE_APP: &str = "hocuspanda.capture";
