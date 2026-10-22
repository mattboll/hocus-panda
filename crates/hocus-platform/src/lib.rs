//! System services behind traits, for Linux and FreeBSD: each runs on its
//! own threads and talks to the compositor's loop through channels.

pub mod notifications;
