//! The Hocuspanda compositor: smithay's protocols, the backends (nested in
//! a window, the real screens on a virtual terminal, offscreen for tests),
//! rendering, input and XWayland.
//!
//! The calloop event loop owns every piece of state ([`State`] and the
//! backend, together in [`App`]); nothing else holds a reference to it.
//! Work that could block (starting programs) runs on threads of its own.

mod agents;
mod appearance;
pub mod auth;
pub mod away;
mod backend;
mod bubbles;
mod capture;
mod clipboard;
mod config;
mod cursor;
mod decoration;
mod desktop_set;
pub mod desktops;
mod displays;
mod emoji;
mod focus;
mod foreign;
mod front;
mod gestures;
mod glass;
pub mod going;
mod grabs;
mod grip_tone;
pub mod halo;
mod handlers;
mod harness;
pub mod hot_corner;
pub mod idle;
mod ink;
mod input;
pub mod input_devices;
mod launcher;
mod layouts;
mod leave;
mod levels;
pub mod lock;
mod magnet;
pub mod model;
pub mod motion;
mod output_spec;
mod overlay;
mod overview;
mod overview_draw;
mod panel;
pub mod peek;
mod popover;
mod popups;
mod programs;
pub mod radial;
mod radial_draw;
pub mod record;
mod reload;
mod reminders;
mod render;
mod rules;
mod scales;
mod screen_sets;
mod screens;
mod session;
mod share;
mod shell;
mod shortcuts;
mod snapshot;
mod socket;
mod state;
mod switcher;
pub mod test_gpu;
mod text_input;
pub mod tile_drop;
mod tray;
mod wallpaper;
mod windows;
mod wobble;
mod xwayland;

use std::path::{Path, PathBuf};

use anyhow::Context;
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

pub use backend::headless::{HeadlessConfig, RendererChoice};
pub use capture::{Area as CaptureArea, Shown as CaptureShown, pictures_dir};
pub use clipboard::application_dirs;
pub use config::{
    Agents as AgentsSettings, AltTab, Appearance, Capture as CaptureSettings, CaptureFormat,
    Clipboard as ClipboardSettings, Config, Delay as PowerDelay, Desktops, DragKey, FormChoice,
    HaloRule, Idle as IdleSettings, Ink, Lid, OldKeys as OldPowerKeys, OpenSize, OutputSettings,
    Overview as OverviewSettings, PanelScreens, PeekRule, Power as PowerSettings, PowerAc,
    PowerBattery, PowerButton, Recording, Remind, Rule as WindowRule, ScaleFrom, ScreenSet,
    SetScreen, SoundPrograms, SystemKeyboard, Windows,
};
pub use cursor::CursorEnv;
/// An Xcursor file, for tests' cursor themes.
#[doc(hidden)]
pub use cursor::theme::{TestImage, encode as encode_xcursor};
pub use harness::{Headless, Placed, WindowInfo};
/// The bar's forms.
pub use hocus_link::Form as BarForm;
pub use lock::{Authenticator, LockState};
pub use output_spec::{OutputSpec, ParseOutputSpecError};
pub use record::{RecordInfo, Source as RecordSource};
pub use screen_sets::Identity as ScreenIdentity;
pub use share::ShareInfo;
pub use shortcuts::{Action, CaptureNow, Chord, Ending, Shortcuts};
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
    /// The shell's program, started once the compositor is ready and again
    /// whenever it ends; `None`: no shell (the bar's space is still kept).
    pub shell: Option<PathBuf>,
    /// The session bus given (`--session-bus`), an address: the shell
    /// serves the notifications there, and the programs started use it.
    /// Without it, only the session on a virtual terminal serves them, on
    /// the session's own bus; nested or headless, none is served.
    pub session_bus: Option<String>,
    /// The system bus given (`--system-bus`), an address: logind there
    /// does the session menu's power actions (7.4) and says the sleeps
    /// (7.2), UPower the power source. Without it, only the session on a
    /// virtual terminal uses the system's; nested or headless, there are
    /// none, so that the developer's machine is never suspended or powered
    /// off, nor waits for a test.
    pub system_bus: Option<String>,
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
    state.shell.program = options.shell;
    let system_bus = match (options.system_bus.clone(), &options.backend) {
        (Some(address), _) => Some(away::system::Bus::Given(address)),
        (None, BackendKind::Tty) => Some(away::system::Bus::System),
        (None, _) => None,
    };
    state.shell.bus = match (options.session_bus, &options.backend) {
        (Some(address), _) => Some(shell::Bus::Given(address)),
        (None, BackendKind::Tty) => Some(shell::Bus::Session),
        (None, _) => None,
    };
    // On a virtual terminal the compositor is the session: its shell is
    // the session's polkit agent (7.5).
    state.shell.auth_agent = matches!(options.backend, BackendKind::Tty);
    shell::listen(&event_loop.handle(), &mut state)?;
    lock::listen(&event_loop.handle(), &mut state)?;
    state.leave_start(match (options.system_bus, &options.backend) {
        (Some(address), _) => Some(address),
        (None, BackendKind::Tty) => Some("system".to_owned()),
        (None, _) => None,
    });
    state.start_sharing();
    state.start_idle();
    state.start_away(system_bus);
    agents::start(&event_loop.handle(), &mut state)?;
    wallpaper::start(&event_loop.handle(), &mut state);
    if let Some(file) = state.config.file.clone() {
        reload::start(&event_loop.handle(), &file, &state.config);
    }
    let backend = Backend::init(&options.backend, &event_loop.handle(), &mut state)?;
    state.programs.wait(options.spawn, options.session);
    if options.session {
        state.autostart = session::autostart::Autostart::Waiting(hocus_xdg::autostart::dirs());
    }
    if options.xwayland {
        xwayland::start(&event_loop.handle(), &mut state);
    } else {
        state.programs_ready();
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
    files.extend(state.shell.socket.clone());
    files.extend(state.agents.socket.clone());
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
        // The agents' changes of this turn, in one line.
        state.flush_agents();
        // The screen the notifications' bubbles show on, when it changed.
        state.flush_bubbles_screen();
        // The screens a fullscreen window covers, when they changed.
        state.flush_fullscreen();
        // Whether something holds idleness off, when that changed.
        state.flush_idle();
        // The lock (7.1): an external locker's unlock judged, the screens
        // that drew it.
        state.lock_after_turn();
        // Before sleep (7.2): the machine sleeps once the lock shows.
        state.away_after_turn();
        // The session's end (7.4): the windows that closed, the lock drawn
        // before suspending.
        state.leave_after_turn();
        // The screens' settings (6.9): the layout applied when it was
        // asked, then the Settings told what changed.
        state.displays_after_turn(backend);
        backend.after_dispatch(state);
        state.space.refresh();
        state.popups.cleanup();
        if let Err(err) = state.display_handle.flush_clients() {
            tracing::warn!(%err, "could not flush the clients");
        }
    }
}
