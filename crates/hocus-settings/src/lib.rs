//! The Settings (6.1, docs/design/settings.md): an application window of
//! its own that writes the desktop's configuration file so that nobody has
//! to edit it, keeping what the user wrote there by hand.
//!
//! - [`model`]: the 17 sections in 5 groups, and the settings each holds;
//!   a section's ticket fills its own ([`power`] is the first; [`sound`]'s
//!   live in PipeWire, not in the file).
//! - [`store`] and [`worker`]: the file read, each change written in place
//!   at once, its changes on disk followed, off the window's thread.
//! - [`search`]: every setting found by its words, in French and English,
//!   and its keys.
//! - [`app`]: the window, drawn with iced as the shell is.

pub mod activate;
pub mod app;
pub mod lang;
pub mod model;
pub mod power;
pub mod renderer;
pub mod search;
pub mod single;
pub mod sound;
pub mod store;
pub mod worker;
