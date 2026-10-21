//! The Wayland compositor: smithay glue, backends, input, rendering, XWayland.
//!
//! The calloop event loop owns every piece of state ([`State`]); nothing else
//! holds a reference to it (see docs/architecture.md).

mod api;
mod capture;
mod clock;
mod grabs;
mod handlers;
mod harness;
mod headless;
mod input;
mod output_spec;
mod socket;
mod state;
mod winit;

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
};

use anyhow::Context;
use hocus_api::{Permission, Reply};
use smithay::reexports::{
    calloop::{EventLoop, channel},
    wayland_server::Display,
};

pub use clock::Clock;
pub use harness::Headless;
pub use headless::{FRAME, HeadlessConfig, OutputInfo, RendererChoice, RendererKind};
pub use output_spec::{OutputSpec, ParseOutputSpecError};
pub use state::State;

/// The dark theme's base colour, until themes exist.
const BACKGROUND: [f32; 4] = [0.055, 0.059, 0.071, 1.0];

/// Where the compositor draws.
#[derive(Debug, Clone)]
pub enum Backend {
    /// In a window of the current session.
    Winit,
    /// Nowhere: outputs are offscreen buffers.
    Headless(HeadlessConfig),
}

/// How to start the compositor.
#[derive(Debug, Clone)]
pub struct Options {
    pub backend: Backend,
    /// Directory of the Wayland socket. When `None`, `$XDG_RUNTIME_DIR` for
    /// the nested backend, and a fresh private directory for the headless
    /// one, so that it never meets the session's clients.
    pub runtime_dir: Option<PathBuf>,
    /// Commands started once the socket is ready, with `WAYLAND_DISPLAY` and
    /// `HOCUS_SOCKET` set.
    pub spawn: Vec<String>,
    /// What clients of the control socket may do.
    pub api_permission: Permission,
}

/// What the event loop owns: the compositor state and its backend.
#[derive(Debug)]
struct App {
    state: State,
    backend: BackendState,
}

#[derive(Debug)]
enum BackendState {
    // Its GLES renderer is large; there is one per compositor.
    Winit(Box<winit::WinitBackend>),
    Headless(headless::HeadlessBackend),
}

/// The directory holding the Wayland socket; a private one is removed on drop.
#[derive(Debug)]
enum RuntimeDir {
    Shared(PathBuf),
    Private(tempfile::TempDir),
}

impl RuntimeDir {
    fn for_options(options: &Options) -> anyhow::Result<Self> {
        if let Some(dir) = &options.runtime_dir {
            return Ok(Self::Shared(dir.clone()));
        }
        match options.backend {
            Backend::Winit => std::env::var_os("XDG_RUNTIME_DIR")
                .map(|dir| Self::Shared(PathBuf::from(dir)))
                .context("XDG_RUNTIME_DIR is not set; pass a runtime directory"),
            Backend::Headless(_) => Self::private(),
        }
    }

    /// A fresh directory under the temporary directory: socket paths are
    /// limited to 108 bytes, so it stays short.
    fn private() -> anyhow::Result<Self> {
        tempfile::Builder::new()
            .prefix("hocus-")
            .tempdir()
            .map(Self::Private)
            .context("could not create a private runtime directory")
    }

    fn path(&self) -> &Path {
        match self {
            Self::Shared(path) => path,
            Self::Private(dir) => dir.path(),
        }
    }
}

/// A compositor ready to run: its loop, its state, its socket.
struct Started {
    event_loop: EventLoop<'static, App>,
    app: App,
    runtime_dir: RuntimeDir,
}

fn start(options: &Options, clock: Clock) -> anyhow::Result<Started> {
    let runtime_dir = RuntimeDir::for_options(options)?;
    let event_loop: EventLoop<App> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let socket = socket::bind(runtime_dir.path())?;
    let api = hocus_api::Server::bind(
        &hocus_api::socket_for(&socket.path),
        options.api_permission,
        &event_loop.handle(),
    )
    .context("could not open the control socket")?;
    let (screenshots, encoded) = channel::channel::<api::EncodedScreenshot>();
    event_loop
        .handle()
        .insert_source(encoded, |event, _, app: &mut App| {
            if let channel::Event::Msg((connection, id, result)) = event {
                app.state
                    .api
                    .reply(connection, id, result.map(Reply::Screenshot));
            }
        })
        .map_err(|err| err.error)
        .context("could not watch screenshot workers")?;

    let mut state = State::new(
        &display.handle(),
        event_loop.get_signal(),
        socket.path.clone(),
        clock,
        api,
        screenshots,
    )?;
    socket::insert(&event_loop.handle(), socket, display)?;

    let backend = match &options.backend {
        Backend::Winit => {
            BackendState::Winit(Box::new(winit::init(&event_loop.handle(), &mut state)?))
        }
        Backend::Headless(config) => BackendState::Headless(headless::init(config, &mut state)?),
    };
    tracing::info!(
        socket = %state.socket_path.display(),
        control = %state.api.path().display(),
        permission = ?state.api.permission(),
        "compositor ready"
    );
    Ok(Started {
        event_loop,
        app: App { state, backend },
        runtime_dir,
    })
}

/// Runs the compositor until it is asked to quit.
pub fn run(options: Options) -> anyhow::Result<()> {
    let Started {
        mut event_loop,
        mut app,
        runtime_dir,
    } = start(&options, Clock::monotonic())?;
    let mut children = spawn(
        &options.spawn,
        runtime_dir.path(),
        &app.state.socket_path,
        app.state.api.path(),
    );

    let handle = event_loop.handle();
    event_loop.run(None, &mut app, |app| {
        match &mut app.backend {
            BackendState::Winit(winit) => winit.after_dispatch(&mut app.state),
            BackendState::Headless(headless) => headless.after_dispatch(&handle, &app.state),
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
fn spawn(commands: &[String], runtime_dir: &Path, socket: &Path, api: &Path) -> Vec<Child> {
    commands
        .iter()
        .filter_map(|command| {
            Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("WAYLAND_DISPLAY", socket)
                .env(hocus_api::SOCKET_ENV, api)
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
