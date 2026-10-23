//! System services behind traits, for Linux and FreeBSD: each runs on its
//! own threads and talks to the compositor's loop through channels.

pub mod account;
pub mod adb;
pub mod ai;
pub mod apps;
pub mod auth;
pub mod calendar;
pub mod herdr;
pub mod mime;
pub mod notifications;
pub mod phone;
pub mod polkit;
pub mod power;
pub mod quick;
pub mod sealed;
pub mod secrets;
pub mod session;
pub mod system;
pub mod timedate;
pub mod tray;
pub mod updates;
