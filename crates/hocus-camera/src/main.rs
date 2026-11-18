//! `hocus-camera`: the Hocuspanda camera's source on PipeWire (#619,
//! docs/design/camera.md). The compositor starts it, tells it how its
//! frames look on its standard input (`hocus_link::CameraLook`, one JSON
//! object a line) and reads what happens to it on its standard output
//! (`hocus_link::FromCamera`). It ends when its input does.
//!
//! `--device auto|cpu|any` says where the frames are made: a GPU if there
//! is one (`auto`), the CPU, or any adapter, a CPU's Vulkan too (tests).
//! PipeWire's socket is its default (`PIPEWIRE_REMOTE`, else `pipewire-0`
//! in the runtime directory), or `--remote PATH`.

use std::{
    io::{BufRead, Write},
    path::PathBuf,
};

use hocus_camera::{
    gpu::Wanted,
    presenter::Cutout,
    serve::{self, Ask, Options},
};

/// What the program writes to the compositor.
enum Out {
    Line(String),
    Cut(Cutout),
}
use hocus_link::{CameraLook, FromCamera};

fn usage() -> ! {
    eprintln!("usage: hocus-camera [--device auto|cpu|any] [--remote PATH]");
    std::process::exit(2);
}

fn main() {
    hocus_panic::install();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let mut options = Options {
        remote: None,
        wanted: Wanted::Auto,
        name: hocus_camera::banner::account_name(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => options.wanted = Wanted::of(&args.next().unwrap_or_else(|| usage())),
            "--remote" => {
                options.remote = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            _ => usage(),
        }
    }
    let (asks, asked) = pipewire::channel::channel::<Ask>();
    // The compositor's lines, read on a thread of their own.
    let reader = std::thread::Builder::new()
        .name("compositor's lines".to_owned())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else {
                    break;
                };
                match CameraLook::read(&line) {
                    Some(look) => {
                        let _ = asks.send(Ask::Look(look));
                    }
                    None => tracing::warn!(line, "a line not understood"),
                }
            }
            let _ = asks.send(Ask::Quit);
        });
    if let Err(err) = reader {
        tracing::error!(%err, "cannot read the compositor's lines");
        std::process::exit(1);
    }
    // Its output written by a thread of its own: its lines, and the
    // presenter's cut-outs after theirs, raw. A cut-out the compositor has
    // not taken yet when the next comes is dropped; a line never is.
    let (out, outs) = std::sync::mpsc::sync_channel::<Out>(2);
    let writer = std::thread::Builder::new()
        .name("compositor's output".to_owned())
        .spawn(move || {
            let mut stdout = std::io::stdout().lock();
            for o in outs {
                let written = match o {
                    Out::Line(line) => writeln!(stdout, "{line}"),
                    Out::Cut(cut) => {
                        let said = FromCamera::Presenter {
                            width: cut.width,
                            height: cut.height,
                        };
                        writeln!(stdout, "{}", said.line())
                            .and_then(|()| stdout.write_all(&cut.bgra))
                    }
                };
                if written.and_then(|()| stdout.flush()).is_err() {
                    break;
                }
            }
        });
    if let Err(err) = writer {
        tracing::error!(%err, "cannot write to the compositor");
        std::process::exit(1);
    }
    let lines = out.clone();
    let tell = Box::new(move |said: FromCamera| {
        let _ = lines.send(Out::Line(said.line()));
    });
    let cutout = Box::new(move |cut: Cutout| {
        let _ = out.try_send(Out::Cut(cut));
    });
    if let Err(err) = serve::run(options, asked, tell, cutout) {
        tracing::error!(%err, "PipeWire is not there: no Hocuspanda camera");
        std::process::exit(1);
    }
}
