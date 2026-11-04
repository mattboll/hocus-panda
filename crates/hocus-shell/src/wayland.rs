//! The shell's Wayland connection: the one the compositor made and handed
//! it (`WAYLAND_SOCKET`, docs/design/lock.md), by which the compositor
//! knows the shell's surfaces (its lock's among them) from any other
//! client's. Without it (the shell run by hand), `WAYLAND_DISPLAY`'s.
//!
//! The descriptor is taken out of the environment first thing, before any
//! library looks: wgpu's look at the adapters may open a GL display, and
//! libwayland's `wl_display_connect(NULL)` takes `WAYLAND_SOCKET` for
//! itself, which would leave the shell a client like any other.
#![allow(unsafe_code)]

use std::os::{
    fd::{FromRawFd, RawFd},
    unix::net::UnixStream,
};

pub const SOCKET_ENV: &str = "WAYLAND_SOCKET";

/// The descriptor the compositor handed, out of the environment. Called
/// first in `main`, before any thread starts.
pub fn take_socket() -> Option<RawFd> {
    let value = std::env::var(SOCKET_ENV).ok();
    // SAFETY: called at the start of `main`, before the shell starts any
    // thread: nothing reads or writes the environment meanwhile.
    unsafe { std::env::remove_var(SOCKET_ENV) };
    let fd: RawFd = value?.trim().parse().ok()?;
    (fd >= 0).then_some(fd)
}

/// The shell's connection: on the descriptor handed, else as the
/// environment says.
pub fn connect(handed: Option<RawFd>) -> Result<wayland_client::Connection, String> {
    let Some(fd) = handed else {
        return wayland_client::Connection::connect_to_env().map_err(|e| e.to_string());
    };
    // SAFETY: `fd` is the socket the compositor handed this process, which
    // nothing else here uses or closes: the stream owns it from now on.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // Not passed on to the programs the shell runs.
    let flags = rustix::io::fcntl_getfd(&stream).map_err(|e| e.to_string())?;
    rustix::io::fcntl_setfd(&stream, flags | rustix::io::FdFlags::CLOEXEC)
        .map_err(|e| e.to_string())?;
    wayland_client::Connection::from_socket(stream).map_err(|e| e.to_string())
}
