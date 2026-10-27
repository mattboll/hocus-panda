//! Pure model of the desktop: outputs, per-screen desktops, windows, focus, layout.
//!
//! No smithay here: the model is fast to build and to test, and the
//! compositor only translates between it and Wayland.

pub mod clipboard;
pub mod config;
pub mod config_edit;
pub mod keyboard;
pub mod model;
pub mod night;
pub mod notes;
pub mod outputs;
pub mod session;
pub mod shortcuts;
pub mod snap;
pub mod tiling;

pub use config::{Config, ConfigError};
pub use model::{Desktop, DesktopId, Model, ModelError, Output, OutputName, Rect, WindowId};
pub use shortcuts::{Action, Combo, Key, Modifiers, Scope, ScreenshotMode, Shortcuts};
pub use snap::Zone;
