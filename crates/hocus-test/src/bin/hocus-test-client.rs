//! A test client as a program, for what the compositor starts itself (the
//! drop-down terminal): one solid window until it is closed. Each start is
//! written to `$XDG_RUNTIME_DIR/<app id>.starts`, one line with its pid.

use std::{io::Write, path::PathBuf};

use anyhow::Context;
use clap::Parser;
use hocus_test::client::SolidWindow;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    app_id: String,
    /// `WIDTHxHEIGHT`.
    #[arg(long, default_value = "400x300")]
    size: String,
    /// `#rrggbb`.
    #[arg(long, default_value = "#33cc66")]
    color: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("no XDG_RUNTIME_DIR")?);
    let socket = runtime.join(std::env::var_os("WAYLAND_DISPLAY").context("no WAYLAND_DISPLAY")?);
    let mut starts = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime.join(format!("{}.starts", args.app_id)))?;
    writeln!(starts, "started {}", std::process::id())?;
    let (w, h) = args.size.split_once('x').context("size is WIDTHxHEIGHT")?;
    let rgb = u32::from_str_radix(args.color.trim_start_matches('#'), 16)?;
    let client = SolidWindow {
        app_id: args.app_id,
        width: w.parse()?,
        height: h.parse()?,
        argb: 0xff00_0000 | rgb,
    }
    .spawn(&socket)?;
    while !client.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}
