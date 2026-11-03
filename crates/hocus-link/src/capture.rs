//! The screen capture's lines (5.8, docs/design/screenshots.md): the
//! compositor owns the capture (what is chosen, the delay, the pointer, the
//! area and its handles, the darkening, the countdown and the flash); the
//! shell draws its toolbar and the toast once it is taken.

use std::{fmt, str::FromStr, time::Duration};

use crate::{BadLine, LAUNCHER_ZOOM, flag, word};

/// The namespace of the capture's toolbar, at the bottom of the screen
/// under the pointer as the capture opens. The compositor fades it in
/// from its first frame over [`CAPTURE_TOOLBAR_IN`], blurs behind it and
/// draws its shadow; it takes the pointer, never the keyboard.
pub const CAPTURE_TOOLBAR_NAMESPACE: &str = "hocus-capture-toolbar";
/// The mockup's `rise`: the toolbar fades in, `ease-out`.
pub const CAPTURE_TOOLBAR_IN: Duration = Duration::from_millis(180);
/// The toolbar's corners and its distance from the screen's bottom, the
/// mockup's × the launcher's zoom (the capture is drawn as large).
pub const CAPTURE_TOOLBAR_RADIUS: f64 = 14.0 * LAUNCHER_ZOOM;
pub const CAPTURE_TOOLBAR_BOTTOM: f64 = 22.0 * LAUNCHER_ZOOM;
/// The delays the toolbar's button goes through, in seconds.
pub const CAPTURE_DELAYS: [u8; 3] = [0, 3, 10];

/// What is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureMode {
    /// A rectangle drawn on a screen.
    #[default]
    Area,
    /// The window under the pointer.
    Window,
    /// The screen under the pointer.
    Screen,
    /// All the screens, as they are laid out.
    All,
}

impl CaptureMode {
    pub const ALL: [Self; 4] = [Self::Area, Self::Window, Self::Screen, Self::All];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Window => "window",
            Self::Screen => "screen",
            Self::All => "all",
        }
    }
}

impl FromStr for CaptureMode {
    type Err = BadLine;

    fn from_str(word: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|m| m.name() == word)
            .ok_or_else(|| BadLine(format!("not a capture mode: {word:?}")))
    }
}

/// The capture's toolbar as the compositor has it: on `output` (its scale
/// in 120ths, as [`crate::ToShell::Bar`]'s), what is chosen, the delay in
/// seconds, and whether the pointer is in the image. Written `capture
/// OUTPUT SCALE MODE DELAY pointer|nopointer`: the toolbar opens there, or
/// shows what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureBar {
    pub output: String,
    pub scale: u32,
    pub mode: CaptureMode,
    pub delay: u8,
    pub pointer: bool,
}

impl CaptureBar {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capture {} {} {} {} {}",
            self.output,
            self.scale,
            self.mode.name(),
            self.delay,
            if self.pointer { "pointer" } else { "nopointer" }
        )
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let number = |w: &str, what: &str| {
            w.parse::<u32>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let output = word(words, "output")?.to_owned();
        let scale = number(word(words, "scale")?, "scale")?;
        let mode = word(words, "mode")?.parse()?;
        let delay = number(word(words, "delay")?, "delay")?;
        let delay = u8::try_from(delay)
            .ok()
            .filter(|d| CAPTURE_DELAYS.contains(d))
            .ok_or_else(|| BadLine(format!("not a delay: {delay}")))?;
        let pointer = flag(words.next(), "pointer", "nopointer")?;
        Ok(Self {
            output,
            scale,
            mode,
            delay,
            pointer,
        })
    }
}

/// What the user does on the toolbar. Written `capture mode MODE`,
/// `capture delay` (the next delay), `capture pointer` (in the image or
/// not) and `capture shoot` (the green button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAsk {
    Mode(CaptureMode),
    Delay,
    Pointer,
    Shoot,
}

impl CaptureAsk {
    pub(crate) fn write(self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mode(mode) => write!(f, "capture mode {}", mode.name()),
            Self::Delay => f.write_str("capture delay"),
            Self::Pointer => f.write_str("capture pointer"),
            Self::Shoot => f.write_str("capture shoot"),
        }
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        match word(words, "what the capture is asked")? {
            "mode" => Ok(Self::Mode(word(words, "mode")?.parse()?)),
            "delay" => Ok(Self::Delay),
            "pointer" => Ok(Self::Pointer),
            "shoot" => Ok(Self::Shoot),
            other => Err(BadLine(format!("not a capture's ask: {other:?}"))),
        }
    }
}

/// A capture taken: on `output` (where its toast shows), of what, its
/// size in pixels, and the folder it was saved in as the user reads it
/// (`Images/Captures d'écran`; empty: not saved). Written `captured OUTPUT
/// MODE WIDTH HEIGHT FOLDER`, `-` for no folder; the folder last, as it
/// may hold spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub output: String,
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub folder: String,
}

impl Captured {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let folder = crate::screen_name(&self.folder);
        write!(
            f,
            "captured {} {} {} {} {}",
            self.output,
            self.mode.name(),
            self.width,
            self.height,
            if folder.is_empty() { "-" } else { &folder }
        )
    }

    pub(crate) fn read(line: &str) -> Result<Self, BadLine> {
        let mut fields = line.splitn(6, ' ').skip(1);
        let number = |w: &str, what: &str| {
            w.parse::<u32>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let output = word(&mut fields, "output")?.to_owned();
        let mode = word(&mut fields, "mode")?.parse()?;
        let width = number(word(&mut fields, "width")?, "width")?;
        let height = number(word(&mut fields, "height")?, "height")?;
        let folder = match word(&mut fields, "folder")? {
            "-" => String::new(),
            folder => folder.to_owned(),
        };
        Ok(Self {
            output,
            mode,
            width,
            height,
            folder,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    #[test]
    fn the_captures_lines_read_back_as_written() {
        let to_shell = [
            ToShell::Capture(CaptureBar {
                output: "eDP-1".into(),
                scale: 120,
                mode: CaptureMode::Area,
                delay: 0,
                pointer: false,
            }),
            ToShell::Capture(CaptureBar {
                output: "HDMI-A-1".into(),
                scale: 180,
                mode: CaptureMode::All,
                delay: 10,
                pointer: true,
            }),
            ToShell::CaptureEnd,
            ToShell::Captured(Captured {
                output: "eDP-1".into(),
                mode: CaptureMode::Area,
                width: 600,
                height: 380,
                folder: "Images/Captures d'écran".into(),
            }),
            ToShell::Captured(Captured {
                output: "DP-2".into(),
                mode: CaptureMode::Window,
                width: 1,
                height: 1,
                folder: String::new(),
            }),
        ];
        for message in to_shell {
            assert_eq!(message.to_string().parse::<ToShell>(), Ok(message));
        }
        let asks = CaptureMode::ALL.map(CaptureAsk::Mode).into_iter().chain([
            CaptureAsk::Delay,
            CaptureAsk::Pointer,
            CaptureAsk::Shoot,
        ]);
        for ask in asks {
            let message = ToCompositor::Capture(ask);
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
    }

    #[test]
    fn a_capture_line_that_does_not_read() {
        for bad in [
            "capture eDP-1 120 area 5 pointer",
            "capture eDP-1 120 zone 0 pointer",
            "capture eDP-1 120 area 0 maybe",
            "capture eDP-1",
            "captured eDP-1 area 600",
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad}");
        }
        for bad in [
            "capture",
            "capture mode",
            "capture mode zone",
            "capture again",
        ] {
            assert!(bad.parse::<ToCompositor>().is_err(), "{bad}");
        }
    }
}
