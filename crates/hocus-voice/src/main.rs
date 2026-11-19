//! `hocus-voice`: the Hocuspanda microphone's source on PipeWire (#619,
//! lot 5, docs/design/voice.md). The compositor starts it, tells it which
//! voice to make on its standard input (`hocus_link::VoiceLook`, one JSON
//! object a line) and reads what happens to it on its standard output
//! (`hocus_link::FromVoice`). It ends when its input does.
//!
//! PipeWire's socket is its default (`PIPEWIRE_REMOTE`, else `pipewire-0`
//! in the runtime directory), or `--remote PATH`.

use std::{
    io::{BufRead, Write},
    path::PathBuf,
};

use hocus_link::{FromVoice, VoiceLook};
use hocus_voice::serve::{self, Ask, Options};

fn usage() -> ! {
    eprintln!("usage: hocus-voice [--remote PATH]");
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
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--remote" => {
                options.remote = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())));
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
                match VoiceLook::read(&line) {
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
    // Its lines written by a thread of their own: the loop never waits
    // on the compositor.
    let (out, outs) = std::sync::mpsc::channel::<String>();
    let writer = std::thread::Builder::new()
        .name("compositor's output".to_owned())
        .spawn(move || {
            let mut stdout = std::io::stdout().lock();
            for line in outs {
                if writeln!(stdout, "{line}")
                    .and_then(|()| stdout.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(err) = writer {
        tracing::error!(%err, "cannot write to the compositor");
        std::process::exit(1);
    }
    let tell = Box::new(move |said: FromVoice| {
        let _ = out.send(said.line());
    });
    if let Err(err) = serve::run(options, asked, tell) {
        tracing::error!(%err, "PipeWire is not there: no Hocuspanda microphone");
        std::process::exit(1);
    }
}
