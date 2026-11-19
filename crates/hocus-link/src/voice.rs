//! The Hocuspanda microphone's lines (#619, lot 5, docs/design/voice.md):
//! `hocus-voice` publishes a PipeWire audio source, "Hocuspanda", which
//! reads the real microphone only while an application reads it, and
//! makes the user's voice through the chosen effect.
//!
//! - The compositor starts it and tells it, one JSON object a line on its
//!   standard input, which voice to make ([`VoiceLook`]): as it starts and
//!   when `[voice]` changes.
//! - It tells the compositor, one JSON object a line on its standard
//!   output, what happens to it ([`FromVoice`]): on the graph, read or
//!   not, the real microphone it reads (or would read).
//!
//! The names its nodes go by on PipeWire are here: the shell's in-use sign
//! leaves its own streams out (they are no application's use).

use serde::{Deserialize, Serialize};

/// The source's node's name (`node.name`).
pub const VOICE_NODE: &str = "hocuspanda-voice";
/// What applications list it as (`node.description`).
pub const VOICE_DESCRIPTION: &str = "Hocuspanda";
/// Its own stream reading the real microphone.
pub const VOICE_INPUT_NODE: &str = "hocuspanda-voice-input";
/// The camera and microphone's window's level meter, reading the source.
pub const VOICE_METER_NODE: &str = "hocuspanda-voice-meter";
/// What every node of the Hocuspanda microphone's name starts with.
pub const VOICE_PREFIX: &str = "hocuspanda-voice";
/// Its samples: 48 kHz, mono, 32-bit floats.
pub const VOICE_RATE: u32 = 48_000;

/// The voice the source makes, one at a time (the window's Voix tab).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Voice {
    /// Telle quelle: the microphone as it is.
    #[serde(rename = "none")]
    AsItIs,
    /// Sans bruit: the noise around the voice taken out (DeepFilterNet).
    #[default]
    Quiet,
    /// Studio: an equaliser, a compressor and a gentle de-esser.
    Studio,
    /// Panda: higher, rounder.
    Panda,
    /// Robot: metallic, on one note.
    Robot,
    /// Grave: an octave lower.
    Deep,
}

impl Voice {
    pub const ALL: [Self; 6] = [
        Self::AsItIs,
        Self::Quiet,
        Self::Studio,
        Self::Panda,
        Self::Robot,
        Self::Deep,
    ];

    /// Its word in `[voice] effect`.
    pub const fn word(self) -> &'static str {
        match self {
            Self::AsItIs => "none",
            Self::Quiet => "quiet",
            Self::Studio => "studio",
            Self::Panda => "panda",
            Self::Robot => "robot",
            Self::Deep => "deep",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.word() == word)
    }
}

/// What the compositor tells `hocus-voice`: the voice to make.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceLook {
    pub voice: Voice,
}

impl VoiceLook {
    /// One line of its input, or `None`.
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }

    /// The line to write.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The real microphone `hocus-voice` reads, or would read once read: the
/// session's default source unless it is the Hocuspanda one, else the
/// first other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Microphone {
    /// Its node's name (`node.name`): what a meeting's transcription
    /// records ([`FromVoice::Microphone`]).
    pub name: String,
    /// What people know it by (`node.description`).
    pub description: String,
}

/// What `hocus-voice` tells the compositor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum FromVoice {
    /// Its source is on the graph, as this node.
    Ready { node: u32 },
    /// An application reads it (`true`), or none does any more.
    Read(bool),
    /// The real microphone it reads or would read, or none found; said
    /// as it changes, read or not.
    Microphone(Option<Microphone>),
    /// The voice it makes from now on, once made (Sans bruit's models
    /// take a moment to read).
    Voice(Voice),
}

impl FromVoice {
    /// One line of its output, or `None` (a line of something else).
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }

    /// The line to write.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}
