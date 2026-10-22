//! The desktop's own interface, built on hocus-ui: today the widget column;
//! the dock, launcher, notifications and quick settings join it.
//!
//! Nothing here knows about smithay: the compositor passes state in, routes
//! input, shows the pixels and does the actions that come out.

pub mod column;
pub mod i18n;
pub mod icons;

pub use column::{Action, ActiveWindow, Column, ColumnState, ScreenDesktops, WallTime};
pub use i18n::Lang;
