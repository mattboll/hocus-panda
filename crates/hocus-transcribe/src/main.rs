//! `hocus-transcribe run DIR` makes the text of the meeting whose sound is
//! in `DIR` (the compositor starts it as a meeting's recording ends);
//! `hocus-transcribe fetch` downloads the models it lacks. What it does is
//! said on its output, one JSON line at a time (`hocus_meetings::Said`).
//!
//! `--models DIR` names where the models are (else
//! `$XDG_DATA_HOME/hocuspanda/models`); `--device auto|cpu|NAME` where it
//! hears (`[meetings] device`, [`device`]). `HOCUS_TRANSCRIBE_FAKE=FILE`: a
//! fake engine says the lines `FILE` holds, without any model nor device
//! (tests).
//!
//! The device is chosen first, from the Vulkan loader's list; the program
//! then starts again as itself (`exec`, the same process), whisper.cpp's
//! Vulkan backend shown that device alone (`GGML_VK_VISIBLE_DEVICES`), or
//! none for the CPU (`HOCUS_TRANSCRIBE_ON` says which, so that it is not
//! chosen twice).

use std::{
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
};

use hocus_meetings::{Places, Said, models};
use hocus_transcribe::{
    device::{self, Choice, Wanted},
    engine::{Engine, Fake},
    fetch,
    real::Real,
    run,
};

/// Where the device chosen is said to the program started again: `gpu`
/// or `cpu`, then its name.
const ON: &str = "HOCUS_TRANSCRIBE_ON";

fn say(said: &Said) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", said.line());
    let _ = out.flush();
}

fn usage() -> ! {
    eprintln!(
        "usage: hocus-transcribe run DIR [--models DIR] [--device auto|cpu|NAME] | fetch [--models DIR]"
    );
    std::process::exit(2);
}

fn fetching(models: &Path, gpu: bool) -> anyhow::Result<()> {
    fetch::missing(models, &models::for_device(gpu), &mut |done, total| {
        say(&Said::Fetching { done, total });
    })
}

/// The device chosen, then this program again on it: returns only when it
/// could not start again (it then hears on the CPU).
fn choose_and_start_again(wanted: &str) {
    let choice = device::choose(&device::list(), &Wanted::of(wanted), device::LARGE_NEEDS);
    let me = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/proc/self/exe"));
    let mut again = std::process::Command::new(me);
    again
        .arg0("hocus-transcribe")
        .args(std::env::args_os().skip(1));
    match &choice {
        Choice::Gpu { device, why } => {
            eprintln!(
                "hocus-transcribe: hears on {} (Vulkan device {}): {why}",
                device.name, device.index
            );
            again
                .env(ON, format!("gpu {}", device.name))
                .env("GGML_VK_VISIBLE_DEVICES", device.index.to_string());
        }
        Choice::Cpu { why } => {
            eprintln!("hocus-transcribe: hears on the CPU: {why}");
            again.env(ON, "cpu").env("GGML_VK_VISIBLE_DEVICES", "");
        }
    }
    let err = again.exec();
    eprintln!("hocus-transcribe: cannot start again on its device ({err}): on the CPU");
}

fn transcribe(dir: &Path, models: &Path) -> anyhow::Result<()> {
    let mut engine: Box<dyn Engine> = match std::env::var_os("HOCUS_TRANSCRIBE_FAKE") {
        Some(file) => Box::new(Fake::read(Path::new(&file))?),
        None => {
            let on = std::env::var(ON).unwrap_or_default();
            let gpu = on.strip_prefix("gpu ").map(str::to_owned);
            say(&Said::Device { gpu: gpu.clone() });
            fetching(models, gpu.is_some())?;
            match Real::load(models, gpu.is_some()) {
                Ok(real) => Box::new(real),
                // The GPU would not take the model: the CPU's, then.
                Err(err) if gpu.is_some() => {
                    eprintln!("hocus-transcribe: the GPU does not hear ({err:#}): on the CPU");
                    say(&Said::Device { gpu: None });
                    fetching(models, false)?;
                    Box::new(Real::load(models, false)?)
                }
                Err(err) => return Err(err),
            }
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
    let mut wanted = String::from("auto");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--models" => models = Some(args.next().unwrap_or_else(|| usage()).into()),
            "--device" => wanted = args.next().unwrap_or_else(|| usage()),
            _ if dir.is_none() && what == "run" => dir = Some(arg.into()),
            _ => usage(),
        }
    }
    let models = models
        .or_else(|| Places::from_env().map(|p| p.models()))
        .unwrap_or_else(|| usage());
    let real = std::env::var_os("HOCUS_TRANSCRIBE_FAKE").is_none();
    if what == "run" && real && std::env::var_os(ON).is_none() {
        choose_and_start_again(&wanted);
    }
    let done = match (what.as_str(), dir) {
        ("run", Some(dir)) => transcribe(&dir, &models),
        ("fetch", None) => fetching(&models, true),
        _ => usage(),
    };
    if let Err(err) = done {
        say(&Said::Failed {
            why: format!("{err:#}"),
        });
        std::process::exit(1);
    }
}
