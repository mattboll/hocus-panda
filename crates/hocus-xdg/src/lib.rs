//! The freedesktop files Hocuspanda reads as other desktops do, shared by
//! the compositor, the shell and the Settings so that they all read them
//! alike, without smithay or a toolkit:
//!
//! - [`desktop`]: the applications' desktop entries, as the launcher lists
//!   and starts them (5.1);
//! - [`autostart`]: the session's start-up applications (XDG autostart),
//!   which the compositor starts once the bar shows (6.15).

pub mod autostart;
pub mod desktop;
