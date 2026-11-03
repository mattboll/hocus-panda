//! The screen capture's lines (5.8, docs/design/screenshots.md): the
//! compositor owns the capture (what is chosen, the delay, the pointer, the
//! area and its handles, the darkening, the countdown and the flash); the
//! shell draws its toolbar, then (5.9) the capture's thumbnail and its
//! editor, and asks the compositor for what is its own: the clipboard, the
//! file's deletion, a drag of it into an application.

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

/// The namespace of the capture's thumbnail (5.9), at the bottom right of
/// the captured screen against the bar. The compositor slides it up
/// [`CAPTURE_THUMB_RISE`] and fades it in over [`CAPTURE_THUMB_IN`] from
/// its first frame, and draws its shadow; it takes the pointer, never the
/// keyboard.
pub const CAPTURE_THUMB_NAMESPACE: &str = "hocus-capture-thumb";
/// The mockup's `thumb-in`: 14 px up, 240 ms, `cubic-bezier(.2, .8, .2, 1)`.
pub const CAPTURE_THUMB_IN: Duration = Duration::from_millis(240);
pub const CAPTURE_THUMB_RISE: f64 = 14.0;
/// The thumbnail's corners, the mockup's × the launcher's zoom.
pub const CAPTURE_THUMB_RADIUS: f64 = 14.0 * LAUNCHER_ZOOM;
/// The namespace of the capture's editor (5.9): all of its screen, over
/// everything, holding the keyboard. The compositor blurs the desktop
/// behind it and fades it in over [`CAPTURE_EDITOR_IN`] from its first
/// frame; the shell darkens it and draws the panel.
pub const CAPTURE_EDITOR_NAMESPACE: &str = "hocus-capture-editor";
/// The mockup's `ed-in`, its opacity.
pub const CAPTURE_EDITOR_IN: Duration = Duration::from_millis(180);

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
/// seconds, and whether the pointer is in the image; and (5.10) whether it
/// takes a photo or records a video, with the computer's sound and the
/// microphone. Written `capture OUTPUT SCALE MODE DELAY pointer|nopointer
/// photo|video sound|nosound micro|nomicro`: the toolbar opens there, or
/// shows what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureBar {
    pub output: String,
    pub scale: u32,
    pub mode: CaptureMode,
    pub delay: u8,
    pub pointer: bool,
    pub video: bool,
    pub sound: bool,
    pub micro: bool,
}

impl CaptureBar {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capture {} {} {} {} {} {} {} {}",
            self.output,
            self.scale,
            self.mode.name(),
            self.delay,
            if self.pointer { "pointer" } else { "nopointer" },
            if self.video { "video" } else { "photo" },
            if self.sound { "sound" } else { "nosound" },
            if self.micro { "micro" } else { "nomicro" },
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
        let video = flag(words.next(), "video", "photo")?;
        let sound = flag(words.next(), "sound", "nosound")?;
        let micro = flag(words.next(), "micro", "nomicro")?;
        Ok(Self {
            output,
            scale,
            mode,
            delay,
            pointer,
            video,
            sound,
            micro,
        })
    }
}

/// What the user does on the toolbar. Written `capture mode MODE`,
/// `capture delay` (the next delay), `capture pointer` (in the image or
/// not) and `capture shoot` (the green button, or in Vidéo the red
/// "Démarrer"). And (5.10) `capture photo` and `capture video` (the
/// switch), `capture sound` and `capture micro` (on or off).
///
/// And with a capture taken (5.9), by the [`Captured::id`] the compositor
/// gave it: `capture forget ID` (its thumbnail's ✕, its editor's or its
/// history's "Supprimer": its file goes, and the clipboard empties if it
/// still holds it), `capture annotated ID` (the shell wrote the annotated
/// image over its file: the clipboard takes it), `capture drag ID` (its
/// thumbnail dragged, the left button held: the compositor drags its file
/// into the application the pointer lets go over).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAsk {
    Mode(CaptureMode),
    Delay,
    Pointer,
    Shoot,
    Video(bool),
    Sound,
    Micro,
    Forget(u64),
    Annotated(u64),
    Drag(u64),
}

impl CaptureAsk {
    pub(crate) fn write(self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mode(mode) => write!(f, "capture mode {}", mode.name()),
            Self::Delay => f.write_str("capture delay"),
            Self::Pointer => f.write_str("capture pointer"),
            Self::Shoot => f.write_str("capture shoot"),
            Self::Video(true) => f.write_str("capture video"),
            Self::Video(false) => f.write_str("capture photo"),
            Self::Sound => f.write_str("capture sound"),
            Self::Micro => f.write_str("capture micro"),
            Self::Forget(id) => write!(f, "capture forget {id}"),
            Self::Annotated(id) => write!(f, "capture annotated {id}"),
            Self::Drag(id) => write!(f, "capture drag {id}"),
        }
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        fn id<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<u64, BadLine> {
            word(words, "capture")?
                .parse::<u64>()
                .map_err(|e| BadLine(format!("capture: {e}")))
        }
        match word(words, "what the capture is asked")? {
            "mode" => Ok(Self::Mode(word(words, "mode")?.parse()?)),
            "delay" => Ok(Self::Delay),
            "pointer" => Ok(Self::Pointer),
            "shoot" => Ok(Self::Shoot),
            "video" => Ok(Self::Video(true)),
            "photo" => Ok(Self::Video(false)),
            "sound" => Ok(Self::Sound),
            "micro" => Ok(Self::Micro),
            "forget" => Ok(Self::Forget(id(words)?)),
            "annotated" => Ok(Self::Annotated(id(words)?)),
            "drag" => Ok(Self::Drag(id(words)?)),
            other => Err(BadLine(format!("not a capture's ask: {other:?}"))),
        }
    }
}

/// A capture taken: the compositor's number for it (5.9: what the shell
/// asks about it by), on `output` (where its thumbnail or toast shows), of
/// what, its size in pixels, the folder it was saved in as the user reads
/// it (`Images/Captures d'écran`; empty: not saved) and its file (empty:
/// not saved). Written `captured ID OUTPUT MODE WIDTH HEIGHT`, then the
/// folder and the file each after a tab, `-` for none: they may hold
/// spaces. A file whose path holds a tab or a line break is not said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub id: u64,
    pub output: String,
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub folder: String,
    pub file: String,
}

impl Captured {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let folder = crate::screen_name(&self.folder);
        let file = if self.file.chars().any(char::is_control) {
            ""
        } else {
            &self.file
        };
        let or_none = |s: &str| {
            if s.is_empty() {
                "-".to_owned()
            } else {
                s.to_owned()
            }
        };
        write!(
            f,
            "captured {} {} {} {} {}\t{}\t{}",
            self.id,
            self.output,
            self.mode.name(),
            self.width,
            self.height,
            or_none(&folder),
            or_none(file)
        )
    }

    pub(crate) fn read(line: &str) -> Result<Self, BadLine> {
        let mut parts = line.split('\t');
        let head = parts.next().unwrap_or_default();
        let mut fields = head.split(' ').skip(1);
        let number = |w: &str, what: &str| {
            w.parse::<u32>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let id = word(&mut fields, "id")?
            .parse::<u64>()
            .map_err(|e| BadLine(format!("id: {e}")))?;
        let output = word(&mut fields, "output")?.to_owned();
        let mode = word(&mut fields, "mode")?.parse()?;
        let width = number(word(&mut fields, "width")?, "width")?;
        let height = number(word(&mut fields, "height")?, "height")?;
        if fields.next().is_some() {
            return Err(BadLine(format!("captured: too many fields in {head:?}")));
        }
        let mut text = |what: &str| -> Result<String, BadLine> {
            match parts.next() {
                Some("-") => Ok(String::new()),
                Some(text) if !text.is_empty() => Ok(text.to_owned()),
                _ => Err(BadLine(format!("captured: no {what}"))),
            }
        };
        let folder = text("folder")?;
        let file = text("file")?;
        Ok(Self {
            id,
            output,
            mode,
            width,
            height,
            folder,
            file,
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
                video: false,
                sound: true,
                micro: false,
            }),
            ToShell::Capture(CaptureBar {
                output: "HDMI-A-1".into(),
                scale: 180,
                mode: CaptureMode::All,
                delay: 10,
                pointer: true,
                video: true,
                sound: false,
                micro: true,
            }),
            ToShell::CaptureEnd,
            ToShell::Captured(Captured {
                id: 1,
                output: "eDP-1".into(),
                mode: CaptureMode::Area,
                width: 600,
                height: 380,
                folder: "Images/Captures d'écran".into(),
                file: "/home/alex/Images/Captures d'écran/Capture du 2026-11-03 à 14-32-05.png"
                    .into(),
            }),
            ToShell::Captured(Captured {
                id: 12,
                output: "DP-2".into(),
                mode: CaptureMode::Window,
                width: 1,
                height: 1,
                folder: String::new(),
                file: String::new(),
            }),
        ];
        for message in to_shell {
            assert_eq!(message.to_string().parse::<ToShell>(), Ok(message));
        }
        let asks = CaptureMode::ALL.map(CaptureAsk::Mode).into_iter().chain([
            CaptureAsk::Delay,
            CaptureAsk::Pointer,
            CaptureAsk::Shoot,
            CaptureAsk::Video(true),
            CaptureAsk::Video(false),
            CaptureAsk::Sound,
            CaptureAsk::Micro,
            CaptureAsk::Forget(3),
            CaptureAsk::Annotated(4),
            CaptureAsk::Drag(u64::MAX),
        ]);
        for ask in asks {
            let message = ToCompositor::Capture(ask);
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
    }

    #[test]
    fn a_capture_line_that_does_not_read() {
        for bad in [
            "capture eDP-1 120 area 5 pointer photo sound nomicro",
            "capture eDP-1 120 area 0 pointer",
            "capture eDP-1 120 area 0 pointer film sound nomicro",
            "capture eDP-1 120 zone 0 pointer photo sound nomicro",
            "capture eDP-1 120 area 0 maybe",
            "capture eDP-1",
            "captured eDP-1 area 600",
            "captured 1 eDP-1 area 600 380",
            "captured 1 eDP-1 area 600 380\t-",
            "captured x eDP-1 area 600 380\t-\t-",
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad}");
        }
        for bad in [
            "capture",
            "capture mode",
            "capture mode zone",
            "capture again",
            "capture forget",
            "capture drag me",
        ] {
            assert!(bad.parse::<ToCompositor>().is_err(), "{bad}");
        }
    }
}
