//! A meeting's record, beside its sound: what the text's head says, and
//! where the text goes.

use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::RECORD;

/// A meeting being transcribed, as the compositor wrote it when the
/// recording began.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub title: String,
    /// Its start and end in the calendar, RFC 3339 instants.
    pub start: String,
    pub end: String,
    /// The calendar's attendees, as it names them.
    #[serde(default)]
    pub attendees: Vec<String>,
    /// When the tracks' first sample was recorded, an RFC 3339 instant:
    /// what a line's time counts from.
    pub began: String,
    /// The language its words are written in: `fr` or `en`.
    pub lang: String,
    /// The folder the text goes in.
    pub folder: PathBuf,
}

impl Record {
    /// The record in meeting folder `dir`.
    pub fn read(dir: &Path) -> io::Result<Self> {
        let text = std::fs::read(dir.join(RECORD))?;
        serde_json::from_slice(&text).map_err(io::Error::other)
    }

    /// Writes it in meeting folder `dir`.
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        let text = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(dir.join(RECORD), text)
    }
}

/// A meeting's two tracks, never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Track {
    /// The default source: the user's microphone, "Moi".
    Me,
    /// The output streams of the application in the call: the others.
    Call,
}

impl Track {
    pub const ALL: [Self; 2] = [Self::Me, Self::Call];

    /// Its file in the meeting's folder: WAV, 16 kHz, mono, 16-bit.
    pub const fn file(self) -> &'static str {
        match self {
            Self::Me => "me.wav",
            Self::Call => "call.wav",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_written_and_read_back() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let record = Record {
            title: "Point produit".to_owned(),
            start: "2026-11-17T14:00:00Z".to_owned(),
            end: "2026-11-17T14:30:00Z".to_owned(),
            attendees: vec!["Alex".to_owned()],
            began: "2026-11-17T14:01:12Z".to_owned(),
            lang: "fr".to_owned(),
            folder: PathBuf::from("/home/alex/Documents/Réunions"),
        };
        record.write(dir.path())?;
        assert_eq!(Record::read(dir.path())?, record);
        Ok(())
    }
}
