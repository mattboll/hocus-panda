//! The Settings (6.1, docs/design/settings.md): an application window of
//! its own that writes the desktop's configuration file so that nobody has
//! to edit it, keeping what the user wrote there by hand.
//!
//! - [`model`]: the 17 sections in 5 groups, and the settings each holds;
//!   a section's ticket fills its own ([`power`] is the first, [`mouse`]
//!   the mice and touchpads; [`screens`] draws its page itself, from the
//!   compositor's line; [`sound`]'s live in PipeWire, not in the file).
//! - [`devices`]: the mice and touchpads plugged in, as the compositor says.
//! - [`keyboard`], [`keys`] and [`xkb`]: the keyboard's settings and the
//!   desktop's shortcuts, the chords as written and shown, XKB's layouts.
//! - [`store`] and [`worker`]: the file read, each change written in place
//!   at once, its changes on disk followed, off the window's thread.
//! - [`search`]: every setting found by its words, in French and English,
//!   and its keys.
//! - [`wallpaper`] and [`portal`]: the wallpaper's page's keys, its
//!   thumbnails and one's own pictures, the desktop portal's file chooser.
//! - [`app`]: the window, drawn with iced as the shell is.

pub mod activate;
pub mod app;
pub mod appearance;
pub mod bar;
pub mod devices;
pub mod keyboard;
pub mod keys;
pub mod lang;
pub mod model;
pub mod mouse;
pub mod portal;
pub mod power;
pub mod renderer;
pub mod screens;
pub mod search;
pub mod single;
pub mod sound;
pub mod store;
pub mod theme;
pub mod wallpaper;
pub mod worker;
pub mod xkb;
