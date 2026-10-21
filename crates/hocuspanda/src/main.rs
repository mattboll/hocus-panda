//! The Hocuspanda desktop.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use hocus_compositor::{Backend, HeadlessConfig, Options, OutputSpec, RendererChoice};
use tracing_subscriber::EnvFilter;

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
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
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
    })
}
