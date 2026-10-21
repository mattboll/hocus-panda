//! The Hocuspanda desktop.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use hocus_compositor::{Backend, Options};
use tracing_subscriber::EnvFilter;

/// The Hocuspanda desktop.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Where to draw.
    #[arg(long, value_enum, default_value_t = BackendArg::Winit)]
    backend: BackendArg,

    /// Directory of the Wayland socket, instead of $XDG_RUNTIME_DIR. Use a
    /// fresh one to run next to a live session without touching it.
    #[arg(long, value_name = "DIR")]
    runtime_dir: Option<PathBuf>,

    /// Command to start once the compositor is ready (repeatable).
    #[arg(long, value_name = "COMMAND")]
    spawn: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    /// In a window of the current session.
    Winit,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    hocus_compositor::run(Options {
        backend: match args.backend {
            BackendArg::Winit => Backend::Winit,
        },
        runtime_dir: args.runtime_dir,
        spawn: args.spawn,
    })
}
