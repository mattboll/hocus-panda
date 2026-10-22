//! `hocus-trim FILE`: the trim editor for a recording (#132), on the
//! session it was started in.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use hocus_annotate::client::{self, Editing, Options};

#[derive(Parser)]
#[command(version, about = "Trim and export a screen recording")]
struct Args {
    /// The recording.
    file: PathBuf,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("no XDG_RUNTIME_DIR")?);
    let display = std::env::var_os("WAYLAND_DISPLAY").context("no WAYLAND_DISPLAY")?;
    let api = hocus_api::default_socket();
    client::run(
        Options {
            wayland: runtime.join(display),
            theme: client::theme_in_effect(api.as_ref()),
            api,
            file: args.file,
            lang: hocus_shell::Lang::from_env(),
            editing: Editing::Trim,
        },
        None,
    )
}
