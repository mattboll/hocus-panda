//! What `hocus-transcribe` says on its output as it works, one JSON
//! object a line, for the compositor's notification.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Who the text heard: the user, and the call's voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Heard {
    /// The user's microphone said something.
    pub me: bool,
    /// The call's voices told apart: "Voix 1", "Voix 2"… (2 at least).
    pub voices: u32,
    /// The call said something its voices were not told apart in: "Les
    /// autres".
    pub others: bool,
}

/// A line of `hocus-transcribe`'s output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Said {
    /// The models are being downloaded: so many bytes of so many.
    Fetching { done: u64, total: u64 },
    /// The text is being made: `done` of it (0 to 1), and about how many
    /// seconds are left, once it can tell.
    Working { done: f32, left: Option<u64> },
    /// The text is written in `file`; the sound went.
    Written {
        file: PathBuf,
        heard: Heard,
        /// The sound's length, in seconds.
        length: u64,
    },
    /// It could not be made: the sound is kept.
    Failed { why: String },
}

impl Said {
    /// The line it is written as.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A line read, if it is one.
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_lines_read_back() {
        for said in [
            Said::Fetching { done: 1, total: 2 },
            Said::Working {
                done: 0.5,
                left: Some(30),
            },
            Said::Written {
                file: PathBuf::from("/d/Réunions/a.md"),
                heard: Heard {
                    me: true,
                    voices: 2,
                    others: false,
                },
                length: 1920,
            },
            Said::Failed {
                why: "no model".to_owned(),
            },
        ] {
            assert_eq!(Said::read(&said.line()), Some(said));
        }
        assert_eq!(Said::read("whisper_init: loading"), None);
    }
}
