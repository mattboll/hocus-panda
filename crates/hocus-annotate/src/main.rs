//! `hocus-annotate FILE`: the annotation window for a capture, on the
//! session it was started in.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use hocus_annotate::client::{self, Options};
use hocus_api::{Client, Reply, Request};
use hocus_ui::{Accent, Theme, ThemeName};

#[derive(Parser)]
#[command(version, about = "Annotate a screenshot")]
struct Args {
    /// The capture, a PNG.
    file: PathBuf,
}

/// The theme in effect, read from the compositor; the default without it.
fn theme(api: Option<&PathBuf>) -> Theme {
    let toml = api
        .and_then(|path| Client::connect(path).ok())
        .and_then(|mut c| match c.request(Request::Config).ok()? {
            Reply::Config(info) => Some(info.toml),
            _ => None,
        });
    let Some(config) = toml.and_then(|t| hocus_core::Config::parse(&t).ok()) else {
        return Theme::default();
    };
    let a = &config.appearance;
    let mut theme = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark));
    if let Ok(named) = a.accent.parse::<Accent>() {
        theme = theme.with_accent(named);
    }
    if a.high_contrast {
        theme = theme.with_high_contrast();
    }
    theme
        .with_text_size(a.text_size)
        .with_reduced_motion(a.reduced_motion)
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
            theme: theme(api.as_ref()),
            api,
            file: args.file,
            lang: hocus_shell::Lang::from_env(),
        },
        None,
    )
}
