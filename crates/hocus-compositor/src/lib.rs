//! The Hocuspanda compositor: smithay's protocols, the backends (nested in
//! a window, the real screens on a virtual terminal, offscreen for tests),
//! rendering, input and XWayland.
//!
//! The calloop event loop owns every piece of state ([`State`] and the
//! backend, together in [`App`]); nothing else holds a reference to it.
//! Work that could block (starting programs) runs on threads of its own.

mod backend;
mod config;
mod cursor;
mod decoration;
mod focus;
mod grabs;
mod handlers;
mod harness;
mod input;
pub mod model;
pub mod motion;
mod output_spec;
mod popups;
mod programs;
mod render;
mod session;
mod shortcuts;
mod snapshot;
mod socket;
mod state;
mod windows;
mod wobble;
mod xwayland;

use std::path::{Path, PathBuf};

use anyhow::Context;
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

pub use backend::headless::{HeadlessConfig, RendererChoice};
pub use config::{Appearance, Config, OutputSettings, ScaleFrom, SystemKeyboard};
pub use harness::{Headless, WindowInfo};
pub use output_spec::{OutputSpec, ParseOutputSpecError};
pub use shortcuts::{Action, Chord, Ending, Shortcuts};
pub use state::State;

use backend::Backend;

/// Where the compositor draws.
#[derive(Debug, Clone)]
pub enum BackendKind {
    /// In a window of the current session.
    Winit,
    /// The session itself, on a virtual terminal: DRM/KMS, libinput, a
    /// seat from logind or seatd.
    Tty,
    /// Nowhere: outputs are offscreen buffers.
    Headless(HeadlessConfig),
}

/// How to start the compositor.
#[derive(Debug, Clone)]
pub struct Options {
    pub backend: BackendKind,
    /// Directory of the Wayland socket. When `None`, `$XDG_RUNTIME_DIR`,
    /// except headless: a fresh private directory, so that it never meets
    /// the session's clients.
    pub runtime_dir: Option<PathBuf>,
    /// Commands started through `sh -c` once the compositor is ready (and
    /// XWayland, when it runs), with `WAYLAND_DISPLAY` set to it.
    pub spawn: Vec<String>,
    /// Runs X11 applications through XWayland.
    pub xwayland: bool,
    /// The user's session, started by a display manager: once ready, the
    /// user's service manager and D-Bus are told where its displays are.
    pub session: bool,
    pub config: Config,
}

/// What the event loop owns: the compositor's state and its backend.
#[derive(Debug)]
pub struct App {
    pub state: State,
    backend: Backend,
}

/// The directory holding the Wayland socket; a private one is removed on
/// drop.
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
            BackendKind::Winit | BackendKind::Tty => std::env::var_os("XDG_RUNTIME_DIR")
                .map(|dir| Self::Shared(PathBuf::from(dir)))
                .context("XDG_RUNTIME_DIR is not set; pass --runtime-dir"),
            BackendKind::Headless(_) => Self::private(),
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

/// A compositor ready to run.
struct Started {
    event_loop: EventLoop<'static, App>,
    app: App,
    runtime_dir: RuntimeDir,
}

fn start(options: Options) -> anyhow::Result<Started> {
    let runtime_dir = RuntimeDir::for_options(&options)?;
    let event_loop: EventLoop<App> = EventLoop::try_new().context("could not create the loop")?;
    let display: Display<State> = Display::new().context("could not create the display")?;
    let socket = socket::bind(runtime_dir.path())?;
    let nested = matches!(options.backend, BackendKind::Winit);
    let mut state = State::new(
        &display.handle(),
        event_loop.handle(),
        event_loop.get_signal(),
        socket.path.clone(),
        runtime_dir.path().to_path_buf(),
        options.config,
        nested,
    )?;
    socket::insert(&event_loop.handle(), socket, display)?;
    let backend = Backend::init(&options.backend, &event_loop.handle(), &mut state)?;
    state.programs.wait(options.spawn, options.session);
    if options.xwayland {
        xwayland::start(&event_loop.handle(), &mut state);
    } else {
        let env = programs::Env::of(&state);
        state.programs.ready(&env);
    }
    tracing::info!(
        socket = %state.socket_path.display(),
        backend = backend.name(),
        "compositor ready"
    );
    Ok(Started {
        event_loop,
        app: App { state, backend },
        runtime_dir,
    })
}

/// Runs the compositor until its window closes or it is asked to stop.
pub fn run(options: Options) -> anyhow::Result<()> {
    // Before any thread starts (GL drivers', programs'), so that they all
    // leave the signals to the loop.
    let signals = calloop::signals::Signals::new(&[
        calloop::signals::Signal::SIGTERM,
        calloop::signals::Signal::SIGINT,
    ])
    .context("could not watch signals")?;
    let Started {
        mut event_loop,
        mut app,
        runtime_dir,
    } = start(options)?;
    stop_on_signals(&event_loop, signals)?;
    event_loop
        .run(None, &mut app, App::after_dispatch)
        .context("the event loop failed")?;
    remove_sockets(&app.state);
    let ending = app.state.ending;
    drop(app);
    drop(event_loop);
    drop(runtime_dir);
    tracing::info!(?ending, "compositor stopped");
    Ok(())
}

/// Removes the Wayland socket and XWayland's display, which their owners
/// would on drop: smithay's X11 window manager keeps a handle to the loop
/// in one of the loop's own sources, so that the loop and what it holds
/// are never dropped once XWayland ran.
fn remove_sockets(state: &State) {
    let mut lock = state.socket_path.clone().into_os_string();
    lock.push(".lock");
    let mut files = vec![state.socket_path.clone(), PathBuf::from(lock)];
    if let Some(n) = state.xdisplay {
        files.push(PathBuf::from(format!("/tmp/.X11-unix/X{n}")));
        files.push(PathBuf::from(format!("/tmp/.X{n}-lock")));
    }
    for file in files {
        if let Err(err) = std::fs::remove_file(&file)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(file = %file.display(), %err, "could not remove");
        }
    }
}

/// SIGTERM and SIGINT end the compositor as its window closing does, so
/// that a private runtime directory is removed.
fn stop_on_signals(
    event_loop: &EventLoop<'static, App>,
    signals: calloop::signals::Signals,
) -> anyhow::Result<()> {
    event_loop
        .handle()
        .insert_source(signals, |event, _, app| {
            tracing::info!(signal = ?event.signal(), "stopping");
            app.state.end(shortcuts::Ending::Signalled);
        })
        .map_err(|err| err.error)
        .context("could not watch signals")?;
    Ok(())
}

impl App {
    /// After each turn of the loop: draws what changed, and sends what
    /// clients were told.
    fn after_dispatch(&mut self) {
        let Self { state, backend } = self;
        // Motions under way ask for the next frame.
        state.tick();
        backend.after_dispatch(state);
        state.space.refresh();
        state.popups.cleanup();
        if let Err(err) = state.display_handle.flush_clients() {
            tracing::warn!(%err, "could not flush the clients");
        }
    }
}
