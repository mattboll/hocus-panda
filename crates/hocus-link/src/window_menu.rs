//! A window's actions (#723, docs/design/windows.md §10b, bar.md): the
//! compositor's radial menu runs them itself; the bar's menu of a window
//! asks for them on the line.
//!
//! - From the shell: `window-action ID ACTION` ([`WindowAction`]) runs an
//!   action on window `ID`; `send-to ID OUTPUT N` sends it to desktop `N`
//!   (from 0) of screen `OUTPUT`; `window-menu ID` asks what its menu
//!   shows, which the compositor answers at once.
//! - To the shell: `window-menu ID FLAGS\tSCREEN…` ([`WindowMenu`]), that
//!   answer; `window-states ABOVE ALL` ([`WindowStates`]): the windows
//!   kept above the others and those on all desktops, for the marks after
//!   their titles, sent as the shell connects and when they change.

use serde::{Deserialize, Serialize};

use crate::text::tidy;

/// An action on a window, the same from the radial menu and the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowAction {
    /// Maximised, or back (`Super+↑` / `Super+↓`).
    Maximize,
    /// `Super+H`.
    Minimize,
    /// Sent to the desktop on that side of its screen's current one; the
    /// screen stays.
    PreviousDesktop,
    NextDesktop,
    /// Kept above the others, or no longer.
    Above,
    /// Shown on every desktop of its screen, or on the current one only.
    AllDesktops,
    /// Sent to the next screen by position, onto the desktop it shows.
    OtherScreen,
    /// Asked to close, as `Super+Q`: the application decides.
    Close,
}

impl WindowAction {
    pub const ALL: [Self; 8] = [
        Self::Maximize,
        Self::Minimize,
        Self::PreviousDesktop,
        Self::NextDesktop,
        Self::Above,
        Self::AllDesktops,
        Self::OtherScreen,
        Self::Close,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Maximize => "maximize",
            Self::Minimize => "minimize",
            Self::PreviousDesktop => "previous-desktop",
            Self::NextDesktop => "next-desktop",
            Self::Above => "above",
            Self::AllDesktops => "all-desktops",
            Self::OtherScreen => "other-screen",
            Self::Close => "close",
        }
    }
}

/// A screen as the bar's menu of a window shows it under "Send to".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuScreen {
    /// Its connector's name, which [`crate::ToCompositor::SendTo`] names.
    pub output: String,
    /// How many desktops it has, and the one it shows (from 0).
    pub count: u8,
    pub current: u8,
    /// Its size, logical pixels: the tiles take its shape.
    pub width: u32,
    pub height: u32,
    /// The window's desktop, when the window is on this screen.
    pub here: Option<u8>,
    /// The built-in screen ("this laptop").
    pub internal: bool,
    /// What the user knows it by (its model), else its connector's name.
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
}

/// What the bar's menu of window `id` shows: its states and the screens
/// it can be sent to, left to right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMenu {
    pub id: u64,
    pub maximized: bool,
    pub above: bool,
    pub all_desktops: bool,
    pub screens: Vec<MenuScreen>,
}

/// The windows in the two states the bar marks after their titles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStates {
    /// Kept above the others.
    pub above: Vec<u64>,
    /// On every desktop of their screen.
    pub all_desktops: Vec<u64>,
}
