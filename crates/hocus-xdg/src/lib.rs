//! The freedesktop files Hocuspanda reads as other desktops do, shared by
//! the compositor, the shell and the Settings so that they all read them
//! alike, without smithay or a toolkit:
//!
//! - [`desktop`]: the applications' desktop entries, as the launcher lists
//!   and starts them (5.1);
//! - [`icons`]: where an icon's file is, in the user's icon theme, as the
//!   launcher draws it;
//! - [`mimeapps`]: the default applications (`mimeapps.list`), read as
//!   `xdg-open` reads them and written as the Settings change them (6.15);
//! - [`autostart`]: the session's start-up applications (XDG autostart),
//!   which the compositor starts once the bar shows (6.15).

pub mod autostart;
pub mod desktop;
pub mod icons;
pub mod mimeapps;
