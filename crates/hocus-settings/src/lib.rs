//! The Settings (6.1, docs/design/settings.md): an application window of
//! its own that writes the desktop's configuration file so that nobody has
//! to edit it, keeping what the user wrote there by hand.
//!
//! - [`model`]: the 17 sections in 5 groups, and the settings each holds;
//!   a section's ticket fills its own ([`power`] is the first, [`mouse`]
//!   the mice and touchpads; [`screens`] draws its page itself, from the
//!   compositor's line; [`sound`]'s live in PipeWire, [`network`]'s in
//!   NetworkManager and rfkill, not in the file).
//! - [`devices`]: the mice and touchpads plugged in, as the compositor says.
//! - [`keyboard`], [`keys`] and [`xkb`]: the keyboard's settings and the
//!   desktop's shortcuts, the chords as written and shown, XKB's layouts.
//! - [`store`] and [`worker`]: the file read, each change written in place
//!   at once, its changes on disk followed, off the window's thread.
//! - [`search`]: every setting found by its words, in French and English,
//!   and its keys.
//! - [`desktops`]: the desktops' count and names, how windows move and
//!   snap, Alt+Tab and the ink (6.14); [`trial`] its trial area, on the
//!   compositor's own geometry.
//! - [`wallpaper`] and [`portal`]: the wallpaper's page's keys, its
//!   thumbnails and one's own pictures, the desktop portal's file chooser.
//! - [`bluetooth`]: the Bluetooth page's words and search; BlueZ itself is
//!   `hocus-bluetooth`'s, as the bar's.
//! - [`captures`]: where screenshots and recordings go, their formats,
//!   the shutter, and what the clipboard's history keeps (6.17).
//! - [`time`] and [`timedate`]: the Date et heure page's zones and words,
//!   and systemd's timedated followed and asked off the window's thread.
//! - [`agents`]: the agents' tools as the compositor says them, the halo
//!   and the pandas' keys (6.16).
//! - [`applications`]: the Applications page's default applications
//!   (`mimeapps.list`, the terminal) and start-up applications (XDG
//!   autostart), read and written on its thread (6.15).
//! - [`app`]: the window, drawn with iced as the shell is.

pub mod activate;
pub mod agents;
pub mod app;
pub mod appearance;
pub mod applications;
pub mod apps;
pub mod bar;
pub mod bluetooth;
pub mod captures;
pub mod desktops;
pub mod devices;
pub mod keyboard;
pub mod keys;
pub mod lang;
pub mod model;
pub mod mouse;
pub mod network;
pub mod portal;
pub mod power;
pub mod power_bus;
pub mod renderer;
pub mod screens;
pub mod search;
pub mod single;
pub mod sound;
pub mod store;
pub mod theme;
pub mod time;
pub mod timedate;
pub mod trial;
pub mod wallpaper;
pub mod worker;
pub mod xkb;
