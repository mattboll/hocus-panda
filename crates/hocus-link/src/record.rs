//! Screen recording's lines (5.10, docs/design/screenshots.md, "5.10"):
//! the compositor records (what, its countdown, its frame, the encoder off
//! the loop, the file); the shell draws the capture's toolbar in Vidéo
//! (its Photo / Vidéo switch, Son and Micro, [`crate::CaptureBar`]), the
//! bar's item while recording, and the thumbnail once it is saved.
//!
//! - `recording ID running|paused ELAPSED`: a recording under way, how
//!   long it has recorded so far (milliseconds, the pauses left out); the
//!   shell counts on from there while it runs. `recording-end`: none is.
//! - `recorded …`: a recording saved ([`Recorded`]): its thumbnail.
//! - From the shell: `record pause`, `record resume`, `record stop` (the
//!   bar's item), `record forget ID` (its thumbnail's ✕: the file goes) and
//!   `record drag ID` (its thumbnail dragged: the compositor drags its file).

use serde::{Deserialize, Serialize};

use crate::BadLine;

/// The recorded frames a second.
pub const RECORD_FPS: u32 = 30;
/// The countdown before a recording starts, in seconds.
pub const RECORD_COUNTDOWN: u8 = 3;

/// A recording's file format: MP4 (H.264 and AAC), or WebM (VP9 and
/// Opus), as `[recording] format` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordFormat {
    #[default]
    Mp4,
    WebM,
}

impl RecordFormat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
        }
    }

    /// As the thumbnail says it: "MP4".
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::WebM => "WebM",
        }
    }

    pub fn of(word: &str) -> Result<Self, BadLine> {
        match word {
            "mp4" => Ok(Self::Mp4),
            "webm" => Ok(Self::WebM),
            other => Err(BadLine(format!("not a recording's format: {other:?}"))),
        }
    }
}

/// A recording under way, for the bar's item: its id, whether it is paused, and
/// how long it has recorded so far, in milliseconds (the pauses left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingShown {
    pub id: u64,
    pub paused: bool,
    pub elapsed: u64,
}

/// A recording saved: the compositor's number for it (what the shell asks about
/// it by), on `output` (where its thumbnail shows), its size in pixels, how
/// long it lasts (milliseconds), its format, the folder as the user reads it
/// (`Vidéos/Enregistrements d'écran`), its file, and its first frame as a PNG
/// (empty: none) for the thumbnail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub id: u64,
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub duration: u64,
    pub format: RecordFormat,
    pub folder: String,
    pub file: String,
    pub poster: String,
}

/// What the shell asks of a recording: from the bar's item, `record
/// pause`, `record resume`, `record stop`; of one saved (its thumbnail, its
/// history), `record forget ID` (its file goes) and `record drag ID` (the
/// compositor drags its file into the application the pointer lets go
/// over).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordAsk {
    Pause,
    Resume,
    Stop,
    Forget(u64),
    Drag(u64),
}
