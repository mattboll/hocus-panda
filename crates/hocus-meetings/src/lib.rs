//! What the compositor, the shell and `hocus-transcribe` share of a
//! meeting's transcription (docs/design/meetings.md), without smithay, a
//! toolkit or the engines:
//!
//! - where things are: the sound of a meeting being transcribed in
//!   `$XDG_STATE_HOME/hocuspanda/meetings/<id>/` ([`Places`]), the models
//!   in `$XDG_DATA_HOME/hocuspanda/models`, the text in the documents'
//!   folder's "Réunions" ("Meetings");
//! - the meeting's [`Record`] beside its sound (`meeting.json`): what the
//!   text's head says, and where it goes;
//! - the [`models`] the text is made with, downloaded on first use;
//! - the tracks' files ([`wav`]);
//! - what `hocus-transcribe` says as it works ([`Said`]).

pub mod models;
mod record;
mod said;
pub mod wav;

pub use record::{Record, Track};
pub use said::{Heard, Said};

use std::path::{Path, PathBuf};

/// The tracks' rate: 16 kHz, mono, 16-bit, as whisper.cpp and sherpa-onnx
/// take them.
pub const RATE: u32 = 16_000;
/// A meeting's record, beside its tracks.
pub const RECORD: &str = "meeting.json";
/// "Ne plus demander" (the asking popover's), kept in the state folder.
pub const NEVER_ASK: &str = "transcribe-never-ask";

/// Where a session's meetings are kept and read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// `$XDG_STATE_HOME/hocuspanda`.
    pub state: PathBuf,
    /// `$XDG_DATA_HOME/hocuspanda`.
    pub data: PathBuf,
}

impl Places {
    /// The environment's: `$XDG_STATE_HOME` (else `~/.local/state`) and
    /// `$XDG_DATA_HOME` (else `~/.local/share`), each `hocuspanda` in it;
    /// none without a home.
    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let dir = |key: &str, fallback: &str| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
                .join("hocuspanda")
        };
        Some(Self {
            state: dir("XDG_STATE_HOME", ".local/state"),
            data: dir("XDG_DATA_HOME", ".local/share"),
        })
    }

    /// Where the meetings' sound waits for its text.
    pub fn meetings(&self) -> PathBuf {
        self.state.join("meetings")
    }

    /// Meeting `id`'s folder.
    pub fn meeting(&self, id: &str) -> PathBuf {
        self.meetings().join(id)
    }

    /// Where the models are kept.
    pub fn models(&self) -> PathBuf {
        self.data.join("models")
    }

    /// The asking popover's "Ne plus demander".
    pub fn never_ask(&self) -> PathBuf {
        self.state.join(NEVER_ASK)
    }
}

/// The documents' folder: `$XDG_DOCUMENTS_DIR`, else what
/// `$XDG_CONFIG_HOME/user-dirs.dirs` says, else `~/Documents`.
pub fn documents_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if let Some(dir) = std::env::var_os("XDG_DOCUMENTS_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(dir);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let said = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| user_dir(&text, "XDG_DOCUMENTS_DIR", &home));
    Some(said.unwrap_or_else(|| home.join("Documents")))
}

/// The folder `key` names in a `user-dirs.dirs` file: `"$HOME/Documents"`
/// or an absolute path; the home itself means none (the specification's
/// "disabled").
fn user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let value = text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
    })?;
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(&value),
    };
    (path.is_absolute() && path != home).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documents_folder_as_user_dirs_names_it() {
        let home = Path::new("/home/alex");
        let text = "# comment\nXDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n";
        assert_eq!(
            user_dir(text, "XDG_DOCUMENTS_DIR", home),
            Some(PathBuf::from("/home/alex/Docs"))
        );
        assert_eq!(
            user_dir("XDG_DOCUMENTS_DIR=\"$HOME/\"\n", "XDG_DOCUMENTS_DIR", home),
            None
        );
        assert_eq!(user_dir("", "XDG_DOCUMENTS_DIR", home), None);
    }

    #[test]
    fn a_meetings_places() {
        let places = Places {
            state: PathBuf::from("/s/hocuspanda"),
            data: PathBuf::from("/d/hocuspanda"),
        };
        assert_eq!(
            places.meeting("x"),
            PathBuf::from("/s/hocuspanda/meetings/x")
        );
        assert_eq!(places.models(), PathBuf::from("/d/hocuspanda/models"));
        assert_eq!(
            places.never_ask(),
            PathBuf::from("/s/hocuspanda/transcribe-never-ask")
        );
    }
}
