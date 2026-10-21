//! The Wayland compositor: smithay glue, backends, input, rendering, XWayland.
//!
//! The calloop event loop owns every piece of state ([`State`]); nothing else
//! holds a reference to it (see docs/architecture.md).

mod grabs;
mod handlers;
mod input;
mod socket;
mod state;
mod winit;

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
};

use anyhow::Context;
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

pub use state::State;

/// Where the compositor draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// In a window of the current session.
    Winit,
}

/// How to start the compositor.
#[derive(Debug, Clone)]
pub struct Options {
    pub backend: Backend,
    /// Directory of the Wayland socket; `$XDG_RUNTIME_DIR` when `None`.
    pub runtime_dir: Option<PathBuf>,
    /// Commands started once the socket is ready, with `WAYLAND_DISPLAY` set.
    pub spawn: Vec<String>,
}

/// What the event loop owns: the compositor state and its backend.
#[derive(Debug)]
struct App {
    state: State,
    winit: Option<winit::WinitBackend>,
}

/// Runs the compositor until it is asked to quit.
pub fn run(options: Options) -> anyhow::Result<()> {
    let runtime_dir = match options.runtime_dir {
        Some(dir) => dir,
        None => std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .context("XDG_RUNTIME_DIR is not set; pass a runtime directory")?,
    };

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let socket = socket::bind(&runtime_dir)?;
    let socket_path = socket.path.clone();

    let state = State::new(
        &display.handle(),
        event_loop.get_signal(),
        socket_path.clone(),
    )?;
    socket::insert(&event_loop.handle(), socket, display)?;

    let mut app = App { state, winit: None };
    match options.backend {
        Backend::Winit => app.winit = Some(winit::init(&event_loop.handle(), &mut app.state)?),
    }

    tracing::info!(socket = %socket_path.display(), "compositor ready");
    let mut children = spawn(&options.spawn, &runtime_dir, &socket_path);

    event_loop.run(None, &mut app, |app| {
        if let Some(winit) = &mut app.winit {
            winit.after_dispatch(&mut app.state);
        }
        if let Err(err) = app.state.display_handle.flush_clients() {
            tracing::warn!(%err, "flushing clients failed");
        }
    })?;

    stop(&mut children);
    tracing::info!("compositor stopped");
    Ok(())
}

/// Starts each command through the shell, connected to this compositor only.
fn spawn(commands: &[String], runtime_dir: &Path, socket: &Path) -> Vec<Child> {
    commands
        .iter()
        .filter_map(|command| {
            Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("WAYLAND_DISPLAY", socket)
                .env_remove("DISPLAY")
                .spawn()
                .inspect_err(|err| tracing::warn!(%err, command, "could not start"))
                .ok()
        })
        .collect()
}

/// Ends the commands we started that are still running.
fn stop(children: &mut [Child]) {
    for child in children {
        if let Ok(None) = child.try_wait() {
            if let Err(err) = child.kill() {
                tracing::warn!(%err, pid = child.id(), "could not stop child");
            }
            let _ = child.wait();
        }
    }
}
