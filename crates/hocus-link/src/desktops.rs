//! The desktops as the bars' tiles show them (bar.md 3.9, 8.4, 8.5).

use serde::{Deserialize, Serialize};

use crate::text::tidy;

/// A screen's desktops as its bar's tiles show them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Desktops {
    pub output: String,
    pub count: u8,
    pub current: u8,
    pub windows: Vec<DeskWindow>,
}

/// A window as its desktop's tile sketches it and its list lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskWindow {
    pub desktop: u8,
    /// Left, top, width, height, in [`SHARE`]s of the screen, inside it.
    pub rect: [u16; 4],
    pub look: Look,
    /// The compositor's id for it: what [`ToCompositor::Bring`](crate::ToCompositor::Bring) names.
    pub id: u64,
    /// Its application's id, for its icon (empty: none).
    pub app: String,
}

/// How a window shows in its desktop's tile and list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Look {
    #[default]
    Plain,
    /// The focused window, on the current desktop: drawn brighter.
    Focused,
    /// Minimised: not sketched, listed after the others, greyed (3.9c).
    Minimized,
}

/// A screen whose desktops a bar shows (8.4): its name, its logical size (its
/// tiles take its shape), and what names it as the Settings' Écrans page does:
/// whether it is the built-in panel, its diagonal in inches (0: not known) and
/// its number there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskScreen {
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub built_in: bool,
    pub inches: u32,
    pub number: u32,
}

/// A desktop's tile in a bar (8.5 B): its screen, its desktop (from 0), and
/// where it is on the bar's desktops' surface, logical pixels from its top-left
/// corner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskTile {
    pub output: String,
    pub desktop: u8,
    pub rect: [i32; 4],
}

/// A screen's windows' titles, by their ids, in [`Desktops::windows`]'
/// order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Titles {
    pub output: String,
    #[serde(serialize_with = "tidy::titles_by_id")]
    pub titles: Vec<(u64, String)>,
}

/// A screen's windows, the most recently focused first ([`ToShell::Recent`](crate::ToShell::Recent)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub output: String,
    pub ids: Vec<u64>,
}

/// A whole screen, in the units of [`DeskWindow::rect`]: a thousandth of it
/// is a fraction of a pixel in a tile, and moves smaller than that send
/// nothing.
pub const SHARE: u16 = 1000;

/// The desktop the shell asks for: one by its index (from 0), or the next
/// or the previous one, which the compositor takes no further than the
/// last or the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopTo {
    At(u8),
    Next,
    Previous,
}
