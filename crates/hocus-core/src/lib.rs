//! Pure model of the desktop: outputs, per-screen desktops, windows, focus, layout.
//!
//! No smithay here: the model is fast to build and to test, and the
//! compositor only translates between it and Wayland.

pub mod model;

pub use model::{Desktop, DesktopId, Model, ModelError, Output, OutputName, Rect, WindowId};
