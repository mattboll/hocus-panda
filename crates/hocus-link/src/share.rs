//! Screen sharing's lines (5.11, docs/design/sharing.md): the compositor
//! serves the desktop portal's screen cast and streams to PipeWire; the
//! shell shows the chooser (what to share) and the bar's sign while
//! something is shared, and holds the notifications meanwhile.
//!
//! - `share-choose`: a request to share, with what can be shared: the
//!   chooser opens on that screen (or shows the sources anew).
//! - `share-choose-end`: it goes (chosen, cancelled, or the request went).
//! - `shares`: what is shared now, each with its application; none, the
//!   bar's sign goes and the notifications held are summed up.
//! - From the shell: where the chooser's thumbnails are (the compositor
//!   draws them over its surface), what was chosen, cancelled, and a
//!   share stopped from the bar.

use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{LAUNCHER_ZOOM, Rect, text::tidy};

/// The chooser's surface: the panel alone, centred on its screen, in the
/// overlay layer, holding the keyboard. The compositor darkens the screen
/// under it, draws its shadow and its thumbnails, and pops it in from its
/// first frame over [`SHARE_CHOOSER_IN`].
pub const SHARE_CHOOSER_NAMESPACE: &str = "hocus-share-chooser";
/// The bar's sign that something is in use (the screen, so far: 5.12 adds
/// the microphone and the camera), under the clock.
pub const IN_USE_NAMESPACE: &str = "hocus-bar-in-use";
/// The mockup's `pop`: from 0.97 of its size and transparent, over this,
/// `cubic-bezier(.2, .8, .2, 1)`.
pub const SHARE_CHOOSER_IN: Duration = Duration::from_millis(180);
pub const SHARE_CHOOSER_FROM: f64 = 0.97;
/// The chooser's corners and its thumbnails', the mockup's × the
/// launcher's zoom (the chooser is drawn as large).
pub const SHARE_CHOOSER_RADIUS: f64 = 16.0 * LAUNCHER_ZOOM;
pub const SHARE_THUMB_RADIUS: f64 = 8.0 * LAUNCHER_ZOOM;
/// The thumbnails are drawn again at most this often while the chooser
/// shows.
pub const SHARE_THUMBS_EVERY: Duration = Duration::from_millis(500);
/// The portal's source types, as its `types` bitmask says them.
pub const SHARE_MONITOR: u32 = 1;
pub const SHARE_WINDOW: u32 = 2;

/// What can be shared: a screen, by its connector's name, or a window, by the
/// compositor's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShareSource {
    Screen(String),
    Window(u64),
}

/// For the log, not the line.
impl fmt::Display for ShareSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Screen(name) => write!(f, "screen:{name}"),
            Self::Window(id) => write!(f, "window:{id}"),
        }
    }
}

/// A screen the chooser offers: its connector's name, its size in pixels,
/// whether it is the built-in panel, and the name people know it by (its
/// EDID's model; empty: none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareScreen {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub builtin: bool,
    #[serde(serialize_with = "tidy::name")]
    pub model: String,
}

/// A window the chooser offers: its id, its desktop (from 1) on its
/// screen, whether that is the desktop its screen shows, whether it is
/// minimised, its application's id (empty: none) and its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareWindow {
    pub id: u64,
    pub desktop: u8,
    pub here: bool,
    pub minimized: bool,
    pub app: String,
    #[serde(serialize_with = "tidy::title")]
    pub title: String,
}

/// A request to share, as the chooser shows it: on `output` (its scale in
/// 120ths, its size `width` × `height` logical pixels), the portal's `types` asked ([`SHARE_MONITOR`],
/// [`SHARE_WINDOW`]), the pointer shown by default or not, the asking
/// application's id (empty: not known) and what it says of itself (its
/// window's title; empty: nothing), then the screens and the windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareChoose {
    pub request: u64,
    pub output: String,
    pub scale: u32,
    pub width: i32,
    pub height: i32,
    pub types: u32,
    pub pointer: bool,
    pub app: String,
    #[serde(serialize_with = "tidy::title")]
    pub detail: String,
    pub screens: Vec<ShareScreen>,
    pub windows: Vec<ShareWindow>,
}

/// What a share shows of itself in the bar: a screen (by the name people
/// know it by, else its connector's) or a window (its title).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SharedThing {
    Screen(#[serde(serialize_with = "tidy::name")] String),
    Window(#[serde(serialize_with = "tidy::title")] String),
}

/// A share under way, as the bar shows it: its id, the application that sees it
/// (its id; empty: not known), and what it sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareShown {
    pub id: u64,
    pub app: String,
    pub thing: SharedThing,
}

/// What the shell says of sharing: where the chooser's thumbnails are on
/// its surface (logical pixels), what was chosen and whether the pointer
/// shows, the chooser cancelled, a share stopped from the bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShareAsk {
    Thumbs {
        request: u64,
        rects: Vec<(ShareSource, Rect)>,
    },
    Chosen {
        request: u64,
        source: ShareSource,
        pointer: bool,
    },
    Cancel {
        request: u64,
    },
    Stop {
        id: u64,
    },
}
