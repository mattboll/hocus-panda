//! A day's CPU and memory (#655, docs/performance.md, "A day's view"):
//! the recorder samples on a thread of its own, every interval, what the
//! loop counted and what happened; it never wakes the loop. Its interval
//! is shortened here (50 ms) so that a test sees many samples.

use crate::common;

use std::{
    io::Write,
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, PerfSettings, RendererChoice,
};
use hocus_perf::{Day, EventKind, files};

const EVERY: Duration = Duration::from_millis(50);

fn start(state: &Path, record: bool) -> anyhow::Result<Headless> {
    let config = Config {
        state_dir: Some(state.to_owned()),
        perf: PerfSettings {
            record,
            interval: Some(EVERY),
        },
        ..Config::default()
    };
    Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

/// Every day recorded under `state` (a test may cross midnight).
fn recorded(state: &Path) -> Day {
    let dir = files::dir_in(state);
    let mut all = Day::default();
    for date in files::days(&dir) {
        if let Ok(day) = Day::read(&dir, date) {
            all.samples.extend(day.samples);
            all.starts.extend(day.starts);
        }
    }
    all
}

/// The loop's turns over `time`, each waiting for whatever comes: one
/// when nothing woke it.
fn turns(hp: &mut Headless, time: Duration) -> anyhow::Result<u32> {
    let until = Instant::now() + time;
    let mut turns = 0;
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    {
        hp.dispatch(left)?;
        turns += 1;
    }
    Ok(turns)
}

/// The budget: with recording on, the loop at rest turns and draws as
/// often as with it off, while the recorder samples 40 times.
#[test]
fn recording_wakes_the_loop_no_more_at_rest() -> anyhow::Result<()> {
    let mut seen = Vec::new();
    for record in [false, true] {
        let dir = tempfile::tempdir()?;
        let mut hp = start(dir.path(), record)?;
        common::at_rest(&mut hp)?;
        common::wait(&mut hp, Duration::from_millis(300))?;
        let frames = hp.frames();
        let samples = recorded(dir.path()).samples.len();
        let turned = turns(&mut hp, Duration::from_secs(2))?;
        seen.push((turned, hp.frames() - frames));
        let day = recorded(dir.path());
        if record {
            let more = day.samples.len() - samples;
            assert!(more >= 20, "{more} samples in 2 s, one each 50 ms");
            assert!(
                day.samples[samples..].iter().all(|s| s.f == 0),
                "frames at rest: {:?}",
                &day.samples[samples..]
            );
        } else {
            assert!(
                !files::dir_in(dir.path()).exists(),
                "recording off, no file"
            );
        }
    }
    assert_eq!(seen[0], seen[1], "(turns, frames) off, then on");
    assert_eq!(seen[1], (1, 0), "nothing woke the loop at rest");
    Ok(())
}

/// A window opened, a screen plugged, the lock and a page of the Settings
/// are each in the sample that follows them; the Settings' process is
/// measured once they say it.
#[test]
fn what_happened_is_written_with_the_next_sample() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = start(dir.path(), true)?;
    common::open(&mut hp, "perf.window", (400, 300))?;
    hp.plug_screen("HEADLESS-2", OutputSpec::new(1280, 800), None)?;
    let socket = hp
        .control_socket()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let mut stream = UnixStream::connect(socket)?;
    writeln!(
        stream,
        "{}",
        hocus_perf::record::settings_line(std::process::id(), "Fond d’écran")
    )?;
    drop(stream);
    hp.lock();
    let wanted = [
        (EventKind::WindowOpened, "perf.window"),
        (EventKind::ScreenPlugged, "HEADLESS-2"),
        (EventKind::SettingsPage, "Fond d’écran"),
        (EventKind::Locked, ""),
    ];
    let mut day = Day::default();
    let all = hp.dispatch_until(Duration::from_secs(20), |_| {
        day = recorded(dir.path());
        let said: Vec<(EventKind, &str)> = day
            .samples
            .iter()
            .flat_map(|s| &s.e)
            .map(|e| (e.kind, e.what.as_str()))
            .collect();
        // The second screen's frames are counted from its first frame on:
        // a sample written before it draws has none, so wait for one that has.
        wanted.iter().all(|w| said.contains(w))
            && day.samples.iter().any(|s| s.m.len() == 3)
            && day
                .samples
                .iter()
                .any(|s| s.o.contains_key("HEADLESS-2") && s.f > 0)
    })?;
    assert!(all, "{:#?}", day.samples);
    for s in &day.samples {
        for e in &s.e {
            assert!(e.t <= s.t, "an event after its sample: {s:?}");
        }
    }
    // The frames of each screen, once there are two.
    assert!(
        day.samples
            .iter()
            .any(|s| s.o.contains_key("HEADLESS-2") && s.f > 0),
        "{:#?}",
        day.samples
    );
    Ok(())
}
