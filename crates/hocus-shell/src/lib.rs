//! The desktop's own interface, built on hocus-ui: today the widget column;
//! the dock, launcher, notifications and quick settings join it.
//!
//! Nothing here knows about smithay: the compositor passes state in, routes
//! input, shows the pixels and does the actions that come out.

pub mod actions;
pub mod authorize;
pub mod calc;
pub mod checkbox;
pub mod column;
pub mod dock;
pub mod emoji;
pub mod fuzzy;
pub mod i18n;
pub mod icons;
pub mod identify;
pub mod launcher;
pub mod leave;
pub mod lock;
pub mod picker;
pub mod providers;
pub mod quick;
pub mod recording;
pub mod share;
pub mod showcase;
pub mod switcher;
pub mod titlebar;
pub mod toasts;
pub mod units;
pub mod wallpaper;
pub mod welcome;
pub mod window_switcher;

pub use column::{
    Action, ActiveWindow, Column, ColumnState, ScreenDesktops, TrayApp, TrayMenuEntry, WallTime,
};
pub use i18n::Lang;
pub use launcher::{App, Choice, Launcher, LauncherEvent, Showcase};
pub use quick::{QuickAction, QuickState, Sheet, Tile};
pub use toasts::{Toast, ToastEvent, Toasts};
