//! The desktop's own interface, built on hocus-ui: today the widget column;
//! the dock, launcher, notifications and quick settings join it.
//!
//! Nothing here knows about smithay: the compositor passes state in, routes
//! input, shows the pixels and does the actions that come out.

pub mod actions;
pub mod activity;
pub mod agents;
pub mod answer;
pub mod authorize;
pub mod boot;
pub mod calc;
pub mod checkbox;
pub mod circle;
pub mod column;
pub mod dock;
pub mod emoji;
pub mod fuzzy;
pub mod i18n;
pub mod icons;
pub mod identify;
pub mod ink;
pub mod intents;
pub mod journal;
pub mod launcher;
pub mod layouts;
pub mod leave;
pub mod lock;
pub mod matrix;
pub mod panda;
pub mod phone;
pub mod picker;
pub mod proposals;
pub mod providers;
pub mod quick;
pub mod radial;
pub mod readout;
pub mod recording;
pub mod seat;
pub mod share;
pub mod showcase;
pub mod strip;
pub mod switcher;
pub mod titlebar;
pub mod toasts;
pub mod topbar;
pub mod units;
pub mod wallpaper;
pub mod welcome;
pub mod window_switcher;

pub use column::{
    Action, ActiveWindow, Column, ColumnState, KeyboardLayout, NotificationLine, ScreenDesktops,
    TrayApp, TrayMenuEntry, WallTime,
};
pub use i18n::Lang;
pub use launcher::{App, Choice, Launcher, LauncherEvent, Showcase};
pub use quick::{QuickAction, QuickState, Sheet, Tile};
pub use toasts::{Toast, ToastEvent, Toasts};
