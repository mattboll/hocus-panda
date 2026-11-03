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

use std::fmt;

use crate::{BadLine, flag, screen_name, word};

/// The recorded frames a second.
pub const RECORD_FPS: u32 = 30;
/// The countdown before a recording starts, in seconds.
pub const RECORD_COUNTDOWN: u8 = 3;

/// A recording's file format: MP4 (H.264 and AAC), or WebM (VP9 and
/// Opus), as `[recording] format` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// A recording under way, for the bar's item: its id, whether it is
/// paused, and how long it has recorded so far, in milliseconds (the
/// pauses left out). Written `recording ID running|paused ELAPSED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingShown {
    pub id: u64,
    pub paused: bool,
    pub elapsed: u64,
}

impl RecordingShown {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "recording {} {} {}",
            self.id,
            if self.paused { "paused" } else { "running" },
            self.elapsed
        )
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let number = |w: &str, what: &str| {
            w.parse::<u64>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let id = number(word(words, "recording")?, "recording")?;
        let paused = flag(words.next(), "paused", "running")?;
        let elapsed = number(word(words, "elapsed")?, "elapsed")?;
        Ok(Self {
            id,
            paused,
            elapsed,
        })
    }
}

/// A recording saved: the compositor's number for it (what the shell asks
/// about it by), on `output` (where its thumbnail shows), its size in
/// pixels, how long it lasts (milliseconds), its format, the folder as the
/// user reads it (`Vidéos/Enregistrements d'écran`), its file, and its
/// first frame as a PNG (empty: none) for the thumbnail. Written
/// `recorded ID OUTPUT WIDTH HEIGHT DURATION FORMAT`, then the folder, the
/// file and the first frame each after a tab, `-` for none.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Recorded {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = |p: &str| {
            if p.is_empty() || p.chars().any(char::is_control) {
                "-".to_owned()
            } else {
                p.to_owned()
            }
        };
        let folder = screen_name(&self.folder);
        write!(
            f,
            "recorded {} {} {} {} {} {}\t{}\t{}\t{}",
            self.id,
            self.output,
            self.width,
            self.height,
            self.duration,
            self.format.name(),
            path(&folder),
            path(&self.file),
            path(&self.poster)
        )
    }

    pub(crate) fn read(line: &str) -> Result<Self, BadLine> {
        let mut parts = line.split('\t');
        let head = parts.next().unwrap_or_default();
        let mut fields = head.split(' ').skip(1);
        let number = |w: &str, what: &str| {
            w.parse::<u64>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let id = number(word(&mut fields, "id")?, "id")?;
        let output = word(&mut fields, "output")?.to_owned();
        let width = number(word(&mut fields, "width")?, "width")?;
        let height = number(word(&mut fields, "height")?, "height")?;
        let duration = number(word(&mut fields, "duration")?, "duration")?;
        let format = RecordFormat::of(word(&mut fields, "format")?)?;
        if fields.next().is_some() {
            return Err(BadLine(format!("recorded: too many fields in {head:?}")));
        }
        let mut text = |what: &str| -> Result<String, BadLine> {
            match parts.next() {
                Some("-") => Ok(String::new()),
                Some(text) if !text.is_empty() => Ok(text.to_owned()),
                _ => Err(BadLine(format!("recorded: no {what}"))),
            }
        };
        let folder = text("folder")?;
        let file = text("file")?;
        let poster = text("first frame")?;
        let size =
            |v: u64, what: &str| u32::try_from(v).map_err(|e| BadLine(format!("{what}: {e}")));
        Ok(Self {
            id,
            output,
            width: size(width, "width")?,
            height: size(height, "height")?,
            duration,
            format,
            folder,
            file,
            poster,
        })
    }
}

/// What the shell asks of a recording: from the bar's item, `record
/// pause`, `record resume`, `record stop`; of one saved (its thumbnail, its
/// history), `record forget ID` (its file goes) and `record drag ID` (the
/// compositor drags its file into the application the pointer lets go
/// over).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordAsk {
    Pause,
    Resume,
    Stop,
    Forget(u64),
    Drag(u64),
}

impl RecordAsk {
    pub(crate) fn write(self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pause => f.write_str("record pause"),
            Self::Resume => f.write_str("record resume"),
            Self::Stop => f.write_str("record stop"),
            Self::Forget(id) => write!(f, "record forget {id}"),
            Self::Drag(id) => write!(f, "record drag {id}"),
        }
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let id = |words: &mut dyn Iterator<Item = &'a str>| {
            words
                .next()
                .ok_or_else(|| BadLine("no recording".to_owned()))?
                .parse::<u64>()
                .map_err(|e| BadLine(format!("recording: {e}")))
        };
        match word(words, "what the recording is asked")? {
            "pause" => Ok(Self::Pause),
            "resume" => Ok(Self::Resume),
            "stop" => Ok(Self::Stop),
            "forget" => Ok(Self::Forget(id(words)?)),
            "drag" => Ok(Self::Drag(id(words)?)),
            other => Err(BadLine(format!("not a recording's ask: {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    #[test]
    fn the_recordings_lines_read_back_as_written() {
        let to_shell = [
            ToShell::Recording(RecordingShown {
                id: 1,
                paused: false,
                elapsed: 0,
            }),
            ToShell::Recording(RecordingShown {
                id: 7,
                paused: true,
                elapsed: 754_321,
            }),
            ToShell::RecordingEnd,
            ToShell::Recorded(Recorded {
                id: 2,
                output: "eDP-1".into(),
                width: 900,
                height: 760,
                duration: 12_400,
                format: RecordFormat::Mp4,
                folder: "Vidéos/Enregistrements d'écran".into(),
                file: "/home/alex/Vidéos/Enregistrements d'écran/Enregistrement du 2026-11-03 à 14-32-05.mp4"
                    .into(),
                poster: "/run/user/1000/hocuspanda-recording-2.png".into(),
            }),
            ToShell::Recorded(Recorded {
                id: 3,
                output: "DP-2".into(),
                width: 2,
                height: 2,
                duration: 1,
                format: RecordFormat::WebM,
                folder: String::new(),
                file: "/tmp/a.webm".into(),
                poster: String::new(),
            }),
        ];
        for message in to_shell {
            assert_eq!(message.to_string().parse::<ToShell>(), Ok(message));
        }
        for ask in [
            RecordAsk::Pause,
            RecordAsk::Resume,
            RecordAsk::Stop,
            RecordAsk::Forget(4),
            RecordAsk::Drag(u64::MAX),
        ] {
            let message = ToCompositor::Record(ask);
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
    }

    #[test]
    fn a_recording_line_that_does_not_read() {
        for bad in [
            "recording",
            "recording 1 maybe 3",
            "recording 1 running",
            "recorded 1 eDP-1 900 760 12 mp4",
            "recorded 1 eDP-1 900 760 12 avi\t-\t-\t-",
            "recorded 1 eDP-1 900 760 12 mp4\t-\t-",
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad}");
        }
        for bad in ["record", "record again", "record forget", "record drag me"] {
            assert!(bad.parse::<ToCompositor>().is_err(), "{bad}");
        }
    }
}
