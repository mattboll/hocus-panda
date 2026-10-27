//! `hocus-settings [--page keyboard]`: the settings window, on the session
//! it was started in.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use hocus_settings::{
    catalogue::Page,
    client::{self, Options},
};

#[derive(Parser)]
#[command(version, about = "Hocuspanda's settings")]
struct Args {
    /// The page to open: appearance, keyboard, screens…
    #[arg(long)]
    page: Option<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("no XDG_RUNTIME_DIR")?);
    let display = std::env::var_os("WAYLAND_DISPLAY").context("no WAYLAND_DISPLAY")?;
    let wayland = runtime.join(display);
    let api = hocus_api::default_socket().context("no control socket")?;
    client::run(
        Options {
            wayland,
            api,
            page: args.page.as_deref().and_then(Page::from_slug),
            lang: hocus_shell::Lang::from_env(),
            app_id: "hocus-settings".into(),
            system_bus: client::SystemBus::Machine,
            xdg: client::Xdg::Machine,
            zoneinfo: hocus_platform::timedate::zoneinfo(),
            xkb: hocus_platform::xkb::Registry::Machine,
            // The session's, as the compositor reads `~/` in [search].
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|h| h.is_absolute()),
        },
        None,
    )
}
