//! The Hocuspanda desktop's compositor.

use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, ValueEnum};
use hocus_compositor::{
    BackendKind, Config, CursorEnv, HeadlessConfig, Options, OutputSpec, RendererChoice,
    SystemKeyboard,
};
use tracing_subscriber::EnvFilter;

/// The Hocuspanda desktop's compositor.
#[derive(Debug, Parser)]
#[command(name = "hocuspanda", version)]
struct Args {
    /// Where to draw. Default: in a window when started from a graphical
    /// session, as the session itself on a virtual terminal otherwise.
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,

    /// Directory of the Wayland socket. Default: $XDG_RUNTIME_DIR, or a
    /// fresh private directory for the headless backend.
    #[arg(long, value_name = "DIR")]
    runtime_dir: Option<PathBuf>,

    /// Command to start once the compositor is ready (repeatable).
    #[arg(long, value_name = "COMMAND")]
    spawn: Vec<String>,

    /// Headless output, WIDTHxHEIGHT[@SCALE][+X+Y][:WIDTHxHEIGHTmm]
    /// (repeatable). Without a position, it goes to the right of the
    /// previous one; with a size in millimetres and no scale, it takes 1
    /// or 2 by its density, as a screen does.
    #[arg(long = "output", value_name = "SPEC")]
    outputs: Vec<OutputSpec>,

    /// Renderer, headless and on a virtual terminal. Auto: GLES, else
    /// pixman (on a virtual terminal, when GLES does not start on the GPU).
    #[arg(long, value_enum, default_value_t = RendererArg::Auto)]
    renderer: RendererArg,

    /// The GPU the headless renderer draws on, by its render node
    /// (/dev/dri/renderD*). Default: the first GPU that has one.
    #[arg(long, value_name = "PATH")]
    render_node: Option<PathBuf>,

    /// Do not start XWayland: X11 applications will not run.
    #[arg(long)]
    no_xwayland: bool,

    /// Configuration file. Default: $XDG_CONFIG_HOME/hocuspanda/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Do not start the shell (the bar); its space is kept all the same.
    #[arg(long)]
    no_shell: bool,

    /// The shell's program. Default: hocuspanda-shell next to this
    /// program, else in PATH.
    #[arg(long, value_name = "PATH")]
    shell: Option<PathBuf>,

    /// Run as the user's session, as a display manager starts it
    /// (hocuspanda-session): on the virtual terminal unless --backend says
    /// otherwise, and once ready, the user's systemd and D-Bus are told
    /// WAYLAND_DISPLAY and DISPLAY, for the programs they start.
    #[arg(long)]
    session: bool,

    /// The session bus the desktop serves the notifications on, and the
    /// programs it starts use: an address, as `dbus-daemon --session
    /// --print-address` prints it. Without it, only the session on a
    /// virtual terminal serves them, on its own session bus; nested or
    /// headless, none is served, so that the live session's is never
    /// taken.
    #[arg(long, value_name = "ADDRESS")]
    session_bus: Option<String>,

    /// The system bus whose logind the session menu asks to suspend,
    /// restart or power off, and whose logind's sleeps and UPower's power
    /// source the session follows (locking before sleep, the idle delay on
    /// battery): an address. Without it, only the session on a virtual
    /// terminal uses the system's; nested or headless, there are none, so
    /// that the machine it runs on is never powered off nor waits for it.
    #[arg(long, value_name = "ADDRESS")]
    system_bus: Option<String>,
}

/// The shell's program.
const SHELL: &str = "hocuspanda-shell";

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    /// The session itself, on a virtual terminal: the screens, keyboards
    /// and mice of the seat.
    Tty,
    /// In a window of the current session.
    Winit,
    /// Without a screen, into offscreen buffers.
    Headless,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RendererArg {
    /// GLES on a render node when there is one, pixman otherwise.
    Auto,
    Gles,
    /// Software rendering.
    Pixman,
}

fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        // A session's stderr goes to the journal, which shows no colours.
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let backend = args.backend.unwrap_or_else(|| {
        let graphical = ["WAYLAND_DISPLAY", "DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|v| !v.is_empty()));
        if graphical && !args.session {
            BackendArg::Winit
        } else {
            BackendArg::Tty
        }
    });
    let file = args.config.or_else(Config::default_path);
    let mut config = file.as_deref().map(Config::load).unwrap_or_default();
    config.file = file;
    // Once, before the loop: the keyboard the system was set up with, for
    // when the configuration names none (now, or once it changes).
    config.system_keyboard = SystemKeyboard::read(Path::new(SystemKeyboard::PATH));
    config.cursor_env = CursorEnv::from_process();
    // PAM; a test's password only in a build for tests (docs/design/lock.md).
    config.lock_auth = hocus_compositor::Authenticator::from_env();
    config.state_dir = Config::default_state_dir();
    // The hooks run the `hocus` beside this program, else the one in PATH.
    let hocus = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("hocus")))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("hocus"));
    config.agent_places = hocus_agents::tools::Places::from_env(hocus, config.state_dir.clone());
    config.applications = hocus_compositor::application_dirs();
    config.pictures = hocus_compositor::pictures_dir();
    config.home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    config.videos = hocus_compositor::record::save::videos_dir();
    // The shutter is heard in a session, not from a headless compositor
    // next to one (6.17).
    if !matches!(backend, BackendArg::Headless) {
        config.sound_programs = hocus_compositor::SoundPrograms::Path;
    }
    config.french = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .is_some_and(|locale| locale.starts_with("fr"));
    let shell = (!args.no_shell).then(|| {
        args.shell.unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| Some(exe.parent()?.join(SHELL)))
                .filter(|path| path.exists())
                .unwrap_or_else(|| PathBuf::from(SHELL))
        })
    });
    let renderer = match args.renderer {
        RendererArg::Auto => RendererChoice::Auto,
        RendererArg::Gles => RendererChoice::Gles,
        RendererArg::Pixman => RendererChoice::Pixman,
    };
    let options = Options {
        backend: match backend {
            BackendArg::Tty => BackendKind::Tty(renderer),
            BackendArg::Winit => BackendKind::Winit,
            BackendArg::Headless => BackendKind::Headless(HeadlessConfig {
                outputs: args.outputs,
                renderer,
                render_node: args.render_node,
            }),
        },
        runtime_dir: args.runtime_dir,
        spawn: args.spawn,
        xwayland: !args.no_xwayland,
        session: args.session,
        config,
        shell,
        session_bus: args.session_bus,
        system_bus: args.system_bus,
    };
    match hocus_compositor::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}
