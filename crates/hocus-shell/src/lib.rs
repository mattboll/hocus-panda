//! The desktop's own interface, built on hocus-ui: today the widget column;
//! the dock, launcher, notifications and quick settings join it.
//!
//! Nothing here knows about smithay: the compositor passes state in, routes
//! input, shows the pixels and does the actions that come out.

pub mod calc;
pub mod column;
pub mod fuzzy;
pub mod i18n;
pub mod icons;
pub mod launcher;
pub mod lock;
pub mod providers;
pub mod quick;
pub mod showcase;
pub mod titlebar;
pub mod toasts;
pub mod units;
pub mod wallpaper;

pub use column::{Action, ActiveWindow, Column, ColumnState, ScreenDesktops, WallTime};
pub use i18n::Lang;
pub use launcher::{App, Choice, Launcher, LauncherEvent, Showcase};
pub use quick::{QuickAction, QuickState, Sheet, Tile};
pub use toasts::{Toast, ToastEvent, Toasts};
