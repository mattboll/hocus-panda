//! The Wayland compositor: smithay glue, backends, input, rendering, XWayland.
//!
//! The calloop event loop owns every piece of state ([`State`]); nothing else
//! holds a reference to it (see docs/architecture.md).

mod actions;
mod api;
mod authorize;
mod bars;
mod capture;
mod clipboard;
mod clock;
mod config_watch;
mod cursor;
mod decoration;
mod desktops;
mod dropdown;
mod flip;
mod fonts;
mod grabs;
mod handlers;
mod harness;
mod headless;
mod idle_notify;
mod input;
mod launcher;
mod leave;
mod lock;
mod notify;
mod output_spec;
mod outputs;
mod quick;
mod recording;
mod scene;
mod screen_capture;
mod screenshot;
mod session;
mod settings;
mod shell;
mod socket;
mod state;
mod stats;
mod surface;
mod tray;
mod tty;
mod wallpaper;
mod watchdog;
mod window_mode;
mod winit;
mod xwayland;

use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};

use anyhow::Context;
use hocus_api::{Permission, Reply};
use smithay::reexports::{
    calloop::{EventLoop, channel},
    wayland_server::Display,
};

pub use clock::Clock;
pub use harness::{HarnessOptions, Headless};
pub use headless::{FRAME, HeadlessConfig, OutputInfo, RendererChoice, RendererKind};
pub use output_spec::{OutputSpec, ParseOutputSpecError};
pub use state::State;
pub use watchdog::WatchdogConfig;

/// The dark theme's base colour, until themes exist.
const BACKGROUND: [f32; 4] = [0.055, 0.059, 0.071, 1.0];

/// Where the compositor draws.
#[derive(Debug, Clone)]
pub enum Backend {
    /// In a window of the current session.
    Winit,
    /// Nowhere: outputs are offscreen buffers.
    Headless(HeadlessConfig),
    /// The session itself, on a virtual terminal: DRM/KMS, libinput,
    /// libseat.
    Tty,
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
    /// `None` runs without a watchdog.
    pub watchdog: Option<WatchdogConfig>,
    /// The configuration file, watched; `None` keeps the defaults.
    pub config: Option<PathBuf>,
    /// How often the configuration file is looked at.
    pub config_poll: Duration,
    /// Runs X11 applications through XWayland.
    pub xwayland: bool,
    /// Shows the desktop's own interface: the widget column.
    pub shell: bool,
    /// Serves notifications on this session bus (with the shell).
    pub session_bus: Option<String>,
    /// Reads the system's state (battery, network…) on this bus instead of
    /// the machine's system bus.
    pub system_bus: Option<String>,
    /// Uses the machine's services as the session itself does (logind,
    /// UPower, sound, saving the session, autostart) without the DRM
    /// backend: for a VM, never next to a live session.
    pub session_services: bool,
    /// Where the session is saved; the session itself uses
    /// `$XDG_STATE_HOME/hocuspanda`, others save nothing unless given one.
    pub state_dir: Option<PathBuf>,
    /// Where the launcher reads desktop entries (`applications/` below
    /// each); the XDG data directories if `None`.
    pub data_dirs: Option<Vec<std::path::PathBuf>>,
    /// What started applications inherit, if not the compositor's own
    /// environment.
    pub child_env: Option<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
    /// Runs this instead of polkit's helper to check passwords (tests).
    pub polkit_helper: Option<PathBuf>,
}

/// How often the configuration file is looked at by default.
pub const CONFIG_POLL: Duration = Duration::from_millis(500);

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
    Tty(Box<tty::TtyBackend>),
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
            Backend::Winit | Backend::Tty => std::env::var_os("XDG_RUNTIME_DIR")
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
                app.timed("screenshot reply", |app| {
                    app.state
                        .api
                        .reply(connection, id, result.map(Reply::Screenshot));
                });
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
    screen_capture::init(&display.handle());
    idle_notify::init(&display.handle());
    socket::insert(&event_loop.handle(), socket, display)?;
    if matches!(state.clock, Clock::Monotonic { .. }) {
        fonts::start(&event_loop.handle());
    }
    if let Some(config) = &options.watchdog {
        state.watchdog = Some(watchdog::start(&event_loop.handle(), config.clone())?);
    }
    state.loop_handle = Some(event_loop.handle());
    state.system_bus = options.system_bus.clone();
    state.is_session = options.session_services || matches!(options.backend, Backend::Tty);
    lock::start(&event_loop.handle(), &mut state);
    leave::start(&event_loop.handle(), &mut state);
    screenshot::start(&event_loop.handle(), &mut state);
    recording::start(&event_loop.handle(), &mut state);
    authorize::start(
        &event_loop.handle(),
        &mut state,
        options.polkit_helper.clone(),
    );
    lock::start_session(
        &event_loop.handle(),
        &mut state,
        options.session_bus.as_deref(),
    );
    let state_dir = options
        .state_dir
        .clone()
        .or_else(|| state.is_session.then(session::default_dir).flatten());
    clipboard::start(&event_loop.handle(), &mut state, state_dir.clone());
    state.start_sessions(state_dir);
    if options.shell {
        state.child_env = options.child_env.clone();
        shell::start(
            &event_loop.handle(),
            &mut state,
            options.session_bus.as_deref(),
            options.data_dirs.clone(),
        );
    }
    if options.xwayland {
        xwayland::start(&event_loop.handle(), &mut state);
    }
    state.config_path = options.config.clone();
    if let Some(path) = &options.config {
        state.config_watch = Some(config_watch::start(
            &event_loop.handle(),
            path.clone(),
            options.config_poll,
        )?);
    }

    let backend = match &options.backend {
        Backend::Winit => {
            BackendState::Winit(Box::new(winit::init(&event_loop.handle(), &mut state)?))
        }
        Backend::Headless(config) => BackendState::Headless(headless::init(config, &mut state)?),
        Backend::Tty => BackendState::Tty(Box::new(tty::init(&event_loop.handle(), &mut state)?)),
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
    if options.xwayland {
        // Commands started with the session may be X11 ones.
        let ready = std::time::Instant::now() + Duration::from_secs(5);
        while app.state.xdisplay.is_none() && std::time::Instant::now() < ready {
            event_loop.dispatch(Some(Duration::from_millis(20)), &mut app)?;
            if let Err(err) = app.state.display_handle.flush_clients() {
                tracing::warn!(%err, "flushing clients failed");
            }
        }
    }
    let mut children = spawn(
        &options.spawn,
        runtime_dir.path(),
        &app.state.socket_path,
        app.state.api.path(),
        app.state.xdisplay,
    );

    let handle = event_loop.handle();
    event_loop.run(None, &mut app, |app| {
        app.state.refresh_shell();
        app.serve_captures();
        app.serve_screenshots();
        app.serve_recording();
        app.settle_outputs();
        match &mut app.backend {
            BackendState::Winit(winit) => winit.after_dispatch(&mut app.state),
            BackendState::Headless(headless) => headless.after_dispatch(&handle, &app.state),
            BackendState::Tty(tty) => tty.after_dispatch(&mut app.state),
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
fn spawn(
    commands: &[String],
    runtime_dir: &Path,
    socket: &Path,
    api: &Path,
    xdisplay: Option<u32>,
) -> Vec<Child> {
    commands
        .iter()
        .filter_map(|command| {
            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c")
                .arg(command)
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("WAYLAND_DISPLAY", socket)
                .env(hocus_api::SOCKET_ENV, api)
                .env("XDG_CURRENT_DESKTOP", hocus_platform::apps::DESKTOP)
                .env("XDG_SESSION_TYPE", "wayland")
                .env_remove("DISPLAY");
            if let Some(n) = xdisplay {
                cmd.env("DISPLAY", format!(":{n}"));
            }
            cmd.spawn()
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
