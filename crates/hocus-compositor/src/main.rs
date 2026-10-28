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
#[command(version)]
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
    /// previous one; with a size in millimetres and no scale, it takes the
    /// scale of its density, as a screen does.
    #[arg(long = "output", value_name = "SPEC")]
    outputs: Vec<OutputSpec>,

    /// Headless renderer.
    #[arg(long, value_enum, default_value_t = RendererArg::Auto)]
    renderer: RendererArg,

    /// Do not start XWayland: X11 applications will not run.
    #[arg(long)]
    no_xwayland: bool,

    /// Configuration file. Default: $XDG_CONFIG_HOME/hocuspanda/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Run as the user's session, as a display manager starts it
    /// (hocuspanda-session): on the virtual terminal unless --backend says
    /// otherwise, and once ready, the user's systemd and D-Bus are told
    /// WAYLAND_DISPLAY and DISPLAY, for the programs they start.
    #[arg(long)]
    session: bool,
}

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
    let mut config = args
        .config
        .or_else(Config::default_path)
        .map(|path| Config::load(&path))
        .unwrap_or_default();
    // Once, before the loop: the keyboard the system was set up with,
    // unless the configuration names its own.
    if config.keyboard.layouts.is_empty() {
        config.system_keyboard = SystemKeyboard::read(Path::new(SystemKeyboard::PATH));
    }
    config.cursor_env = CursorEnv::from_process();
    let options = Options {
        backend: match backend {
            BackendArg::Tty => BackendKind::Tty,
            BackendArg::Winit => BackendKind::Winit,
            BackendArg::Headless => BackendKind::Headless(HeadlessConfig {
                outputs: args.outputs,
                renderer: match args.renderer {
                    RendererArg::Auto => RendererChoice::Auto,
                    RendererArg::Gles => RendererChoice::Gles,
                    RendererArg::Pixman => RendererChoice::Pixman,
                },
            }),
        },
        runtime_dir: args.runtime_dir,
        spawn: args.spawn,
        xwayland: !args.no_xwayland,
        session: args.session,
        config,
    };
    match hocus_compositor::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}
