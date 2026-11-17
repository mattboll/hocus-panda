//! `hocus-transcribe run DIR` makes the text of the meeting whose sound is
//! in `DIR` (the compositor starts it as a meeting's recording ends);
//! `hocus-transcribe fetch` downloads the models it lacks. What it does is
//! said on its output, one JSON line at a time (`hocus_meetings::Said`).
//!
//! `--models DIR` names where the models are (else
//! `$XDG_DATA_HOME/hocuspanda/models`). `HOCUS_TRANSCRIBE_FAKE=FILE`: a
//! fake engine says the lines `FILE` holds, without any model (tests).

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use hocus_meetings::{Places, Said};
use hocus_transcribe::{
    engine::{Engine, Fake},
    fetch,
    real::Real,
    run,
};

fn say(said: &Said) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", said.line());
    let _ = out.flush();
}

fn usage() -> ! {
    eprintln!("usage: hocus-transcribe run DIR [--models DIR] | fetch [--models DIR]");
    std::process::exit(2);
}

fn fetching(models: &Path) -> anyhow::Result<()> {
    fetch::missing(models, &mut |done, total| {
        say(&Said::Fetching { done, total });
    })
}

fn transcribe(dir: &Path, models: &Path) -> anyhow::Result<()> {
    let mut engine: Box<dyn Engine> = match std::env::var_os("HOCUS_TRANSCRIBE_FAKE") {
        Some(file) => Box::new(Fake::read(Path::new(&file))?),
        None => {
            fetching(models)?;
            Box::new(Real::load(models)?)
        }
    };
    let made = run::run(dir, engine.as_mut(), &mut |said| say(&said))?;
    say(&Said::Written {
        file: made.file,
        heard: made.who,
        length: made.length,
    });
    Ok(())
}

fn main() {
    hocus_panic::install();
    // Never in the way of the desktop: the lowest priority, which the
    // engines' threads take as they start.
    if let Err(err) = rustix::process::setpriority_process(None, 19) {
        eprintln!("hocus-transcribe: its priority stays: {err}");
    }
    let mut args = std::env::args().skip(1);
    let what = args.next().unwrap_or_else(|| usage());
    let mut dir: Option<PathBuf> = None;
    let mut models: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--models" => models = Some(args.next().unwrap_or_else(|| usage()).into()),
            _ if dir.is_none() && what == "run" => dir = Some(arg.into()),
            _ => usage(),
        }
    }
    let models = models
        .or_else(|| Places::from_env().map(|p| p.models()))
        .unwrap_or_else(|| usage());
    let done = match (what.as_str(), dir) {
        ("run", Some(dir)) => transcribe(&dir, &models),
        ("fetch", None) => fetching(&models),
        _ => usage(),
    };
    if let Err(err) = done {
        say(&Said::Failed {
            why: format!("{err:#}"),
        });
        std::process::exit(1);
    }
}
