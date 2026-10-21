//! The Hocuspanda desktop.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, ValueEnum};
use hocus_api::Permission;
use hocus_compositor::{
    Backend, HeadlessConfig, Options, OutputSpec, RendererChoice, WatchdogConfig,
};
use tracing_subscriber::{
    EnvFilter, Layer,
    field::RecordFields,
    fmt::{
        FormatFields,
        format::{DefaultFields, Writer},
    },
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// The Hocuspanda desktop.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Where to draw.
    #[arg(long, value_enum, default_value_t = BackendArg::Winit)]
    backend: BackendArg,

    /// Directory of the Wayland socket. Default: $XDG_RUNTIME_DIR for the
    /// nested backend, a fresh private directory for the headless one.
    #[arg(long, value_name = "DIR")]
    runtime_dir: Option<PathBuf>,

    /// Command to start once the compositor is ready (repeatable).
    #[arg(long, value_name = "COMMAND")]
    spawn: Vec<String>,

    /// Headless output, WIDTHxHEIGHT[@SCALE][+X+Y] (repeatable). Without a
    /// position, it goes to the right of the previous one.
    #[arg(long = "output", value_name = "SPEC")]
    outputs: Vec<OutputSpec>,

    /// Headless renderer.
    #[arg(long, value_enum, default_value_t = RendererArg::Auto)]
    renderer: RendererArg,

    /// What clients of the control socket may do. `test` lets them drive
    /// input and read the whole scene: only for test sessions.
    #[arg(long, value_enum, default_value_t = PermissionArg::Observe)]
    api_permission: PermissionArg,

    /// Log file, besides the terminal. Default for the nested backend:
    /// $XDG_STATE_HOME/hocuspanda/hocuspanda.log; none for the headless one.
    /// The previous log is kept with an `.old` suffix.
    #[arg(long, value_name = "FILE", conflicts_with = "no_log_file")]
    log_file: Option<PathBuf>,

    /// Log to the terminal only.
    #[arg(long)]
    no_log_file: bool,

    /// Do not watch the event loop.
    #[arg(long)]
    no_watchdog: bool,

    /// Configuration file, watched and applied on change. Default:
    /// $XDG_CONFIG_HOME/hocuspanda/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    /// In a window of the current session.
    Winit,
    /// Without a screen, into offscreen buffers.
    Headless,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PermissionArg {
    /// List outputs and windows, follow events, take screenshots.
    Observe,
    /// Also inject input and dump the scene.
    Test,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RendererArg {
    /// GLES on a render node when there is one, pixman otherwise.
    Auto,
    Gles,
    /// Software rendering.
    Pixman,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let state_dir = state_dir();
    let log_file = match (&args.log_file, args.no_log_file, args.backend) {
        (_, true, _) => None,
        (Some(path), false, _) => Some(path.clone()),
        (None, false, BackendArg::Winit) => state_dir.as_ref().map(|d| d.join("hocuspanda.log")),
        (None, false, BackendArg::Headless) => None,
    };
    // Kept to the end: dropping it flushes the file.
    let _log_guard = init_logs(log_file.as_deref())?;

    let watchdog = (!args.no_watchdog).then(|| {
        // Dumps are rare and wanted after the session is gone.
        WatchdogConfig::new(state_dir.unwrap_or_else(std::env::temp_dir))
    });
    let config = args.config.clone().or_else(|| {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;
        Some(base.join("hocuspanda/config.toml"))
    });
    hocus_compositor::run(Options {
        watchdog,
        config,
        config_poll: hocus_compositor::CONFIG_POLL,
        backend: match args.backend {
            BackendArg::Winit => Backend::Winit,
            BackendArg::Headless => Backend::Headless(HeadlessConfig {
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
        api_permission: match args.api_permission {
            PermissionArg::Observe => Permission::Observe,
            PermissionArg::Test => Permission::Test,
        },
    })
}

/// `$XDG_STATE_HOME/hocuspanda`, or `~/.local/state/hocuspanda`.
fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
    Some(base.join("hocuspanda"))
}

/// Logs to the terminal and, when given, to `file`. The file is written by a
/// worker thread through a bounded queue: the loop never waits on the disk.
/// Levels come from `RUST_LOG` (default `info`) until the configuration file
/// exists.
fn init_logs(
    file: Option<&Path>,
) -> anyhow::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let terminal = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(filter());
    let Some(path) = file else {
        tracing_subscriber::registry().with(terminal).init();
        return Ok(None);
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    if path.exists() {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        std::fs::rename(path, old)
            .with_context(|| format!("could not keep the previous {}", path.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    let to_file = tracing_subscriber::fmt::layer()
        .fmt_fields(PlainFields::default())
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(filter());
    tracing_subscriber::registry()
        .with(terminal)
        .with(to_file)
        .init();
    tracing::info!(file = %path.display(), "logging");
    Ok(Some(guard))
}

/// Span fields are formatted once per formatter type and cached: the file
/// needs a type of its own, or it gets the terminal's colours.
#[derive(Debug, Default)]
struct PlainFields(DefaultFields);

impl<'w> FormatFields<'w> for PlainFields {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'w>, fields: R) -> std::fmt::Result {
        self.0.format_fields(writer, fields)
    }
}
