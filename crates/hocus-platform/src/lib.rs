//! System services behind traits, for Linux and FreeBSD: each runs on its
//! own threads and talks to the compositor's loop through channels.

pub mod apps;
pub mod auth;
pub mod calendar;
pub mod notifications;
pub mod phone;
pub mod polkit;
pub mod power;
pub mod quick;
pub mod sealed;
pub mod session;
pub mod system;
pub mod tray;
pub mod updates;
