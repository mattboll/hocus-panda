//! The screen capture's lines (5.8, docs/design/screenshots.md): the
//! compositor owns the capture (what is chosen, the delay, the pointer, the
//! area and its handles, the darkening, the countdown and the flash); the
//! shell draws its toolbar, then (5.9) the capture's thumbnail and its
//! editor, and asks the compositor for what is its own: the clipboard, the
//! file's deletion, a drag of it into an application.

use std::{str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{BadLine, LAUNCHER_ZOOM};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// The capture's toolbar as the compositor has it: on `output` (its scale in
/// 120ths, as [`crate::ToShell::Bar`]'s), what is chosen, the delay in seconds,
/// and whether the pointer is in the image; and (5.10) whether it takes a photo
/// or records a video, with the computer's sound and the microphone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// What the user does on the toolbar. And (5.10) `capture photo` and `capture
/// video` (the switch), `capture sound` and `capture micro` (on or off).
///
/// And with a capture taken (5.9), by the [`Captured::id`] the compositor
/// gave it: `capture forget ID` (its thumbnail's ✕, its editor's or its
/// history's "Supprimer": its file goes, and the clipboard empties if it
/// still holds it), `capture annotated ID` (the shell wrote the annotated
/// image over its file: the clipboard takes it), `capture drag ID` (its
/// thumbnail dragged, the left button held: the compositor drags its file
/// into the application the pointer lets go over).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// A capture taken: the compositor's number for it (5.9: what the shell asks
/// about it by), on `output` (where its thumbnail or toast shows), of what, its
/// size in pixels, the folder it was saved in as the user reads it
/// (`Images/Captures d'écran`; empty: not saved) and its file (empty: not
/// saved). A file whose path holds a tab or a line break is not said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Captured {
    pub id: u64,
    pub output: String,
    pub mode: CaptureMode,
    pub width: u32,
    pub height: u32,
    pub folder: String,
    pub file: String,
}
