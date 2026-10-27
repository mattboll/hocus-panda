//! Wall-clock timings of the desktop (#88): startup, a turn of the loop at
//! rest, input to photon, opening the launcher and the settings, frame
//! times. They measure the machine as much as the code, so they never fail
//! a build: CI's timing job runs them alone and publishes the table (with
//! a warning where a frame's budget is passed). Counted budgets, which do
//! gate, are in `budgets` and the `budget-twenty-windows` scenario.
//!
//! `cargo nextest run -p hocus-test --test timings --run-ignored only
//! --no-capture` (`--release` for the numbers of a release build); with
//! `GITHUB_STEP_SUMMARY` set, the table is appended to that file.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_api::{Permission, Request};
use hocus_compositor::{
    FRAME, HarnessOptions, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};
use hocus_test::{
    client::{SolidWindow, TestClient},
    scenario::{RunOptions, Scenario},
};

// The scenario's allocations are counted too, for the table.
#[global_allocator]
static ALLOCATOR: hocus_compositor::allocations::Counting = hocus_compositor::allocations::Counting;

const TIMEOUT: Duration = Duration::from_secs(10);
/// A frame at 60 Hz: the one budget of time the architecture sets (every
/// loop handler is timed against it).
const FRAME_BUDGET_MS: f64 = 16.0;
/// What Alex waits for without noticing a wait, as the maintainer set them
/// (#88): warnings past them, never failures.
const STARTUP_TARGET_MS: f64 = 1000.0;
const OPENING_TARGET_MS: f64 = 100.0;
/// evdev codes.
const KEY_LEFTMETA: u32 = 125;
const KEY_SPACE: u32 = 57;
const KEY_ESC: u32 = 1;
const KEY_A: u32 = 30;
const KEY_P: u32 = 25;

/// So many applications installed: the launcher opens on all of them
/// (#254), as on a desktop of some years.
const APPLICATIONS: usize = 60;

/// Desktop entries of `APPLICATIONS` applications under `dir`, named as
/// applications are, of a word or a few.
fn applications(dir: &Path) -> anyhow::Result<()> {
    const WORDS: [&str; 12] = [
        "Text", "Image", "Sound", "Video", "Mail", "Map", "Disk", "Clock", "Code", "Chess",
        "Paint", "Notes",
    ];
    let apps = dir.join("applications");
    std::fs::create_dir_all(&apps)?;
    for i in 0..APPLICATIONS {
        let name = match i % 3 {
            0 => WORDS[i % WORDS.len()].to_owned(),
            1 => format!("{} Editor", WORDS[i % WORDS.len()]),
            _ => format!(
                "{} {} Viewer",
                WORDS[i % WORDS.len()],
                WORDS[(i / 3) % WORDS.len()]
            ),
        };
        std::fs::write(
            apps.join(format!("org.example.App{i}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={name} {i}\nExec=true\n"),
        )?;
    }
    Ok(())
}

fn options(data: &Path) -> HarnessOptions {
    HarnessOptions {
        headless: HeadlessConfig {
            outputs: vec![
                OutputSpec::new(2560, 1600).scale(2.0),
                OutputSpec::new(3440, 1440),
            ],
            renderer: RendererChoice::Pixman,
        },
        api_permission: Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: Some(vec![data.to_path_buf()]),
        child_env: None,
        polkit_helper: None,
        motion: true,
        first_run: false,
        google_caldav: None,
    }
}

/// One line of the table.
struct Row {
    what: String,
    value: String,
    /// Over its budget: said as a warning.
    over: Option<String>,
}

#[derive(Default)]
struct Report {
    rows: Vec<Row>,
}

impl Report {
    fn add(&mut self, what: impl Into<String>, value: impl Into<String>) {
        self.rows.push(Row {
            what: what.into(),
            value: value.into(),
            over: None,
        });
    }

    /// `ms` against the frame's budget.
    fn add_frame_ms(&mut self, what: impl Into<String>, ms: f64) {
        self.add_target_ms(what, ms, FRAME_BUDGET_MS, "a frame");
    }

    /// `ms` against `target`, named `name` in the warning.
    fn add_target_ms(&mut self, what: impl Into<String>, ms: f64, target: f64, name: &str) {
        let what = what.into();
        let over = (ms > target).then(|| format!("{what}: {ms:.1} ms, over {name} ({target} ms)"));
        self.rows.push(Row {
            what,
            value: format!("{ms:.1} ms"),
            over,
        });
    }

    fn publish(&self) -> anyhow::Result<()> {
        let mut table = String::from(
            "### Timings (#88)\n\nWall-clock times on this machine: they never fail the build.\n\n| What | Time |\n|---|---|\n",
        );
        for row in &self.rows {
            let mark = if row.over.is_some() { " ⚠" } else { "" };
            writeln!(table, "| {} | {}{mark} |", row.what, row.value)?;
        }
        println!("{table}");
        for row in &self.rows {
            if let Some(warning) = &row.over {
                // GitHub Actions shows it on the run, without failing it.
                println!("::warning title=Timing over budget::{warning}");
            }
        }
        if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(summary)?;
            file.write_all(table.as_bytes())?;
        }
        Ok(())
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The `p`th percentile of `samples`, sorted.
fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let i = ((samples.len() - 1) as f64 * p).round() as usize;
    samples[i.min(samples.len() - 1)]
}

fn repository() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

fn open_windows(desktop: &mut Headless, count: u32) -> anyhow::Result<Vec<TestClient>> {
    let mut clients = Vec::new();
    for i in 0..count {
        if i == count / 2 {
            desktop.request(Request::PointerMove {
                x: 2900.0,
                y: 700.0,
            })?;
        }
        let window = SolidWindow {
            app_id: format!("timing.w{i:02}"),
            width: 480 + (i % 5) * 60,
            height: 320 + (i % 4) * 40,
            argb: 0xff_33_66_99 + i * 0x0a_0a,
        };
        let before = desktop.window_count();
        clients.push(window.spawn(desktop.socket_path())?);
        desktop.dispatch_until(TIMEOUT, |d| d.window_count() > before)?;
        desktop.advance(Duration::from_millis(300))?;
    }
    desktop.request(Request::PointerMove { x: 640.0, y: 400.0 })?;
    desktop.advance(Duration::from_secs(1))?;
    Ok(clients)
}

/// Presses a key and lets it go.
fn press(desktop: &mut Headless, code: u32) -> anyhow::Result<()> {
    desktop.request(Request::Key {
        code,
        pressed: true,
    })?;
    desktop.request(Request::Key {
        code,
        pressed: false,
    })?;
    Ok(())
}

/// Super+Space, to the launcher's first frame, in milliseconds.
fn open_launcher(desktop: &mut Headless) -> anyhow::Result<f64> {
    let frames = |d: &Headless| d.outputs().iter().map(|o| o.frames).sum::<u64>();
    let before = frames(desktop);
    let start = Instant::now();
    for (code, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_SPACE, true),
        (KEY_SPACE, false),
        (KEY_LEFTMETA, false),
    ] {
        desktop.request(Request::Key { code, pressed })?;
    }
    while frames(desktop) == before {
        desktop.advance(FRAME)?;
        anyhow::ensure!(start.elapsed() < TIMEOUT, "the launcher never drew");
    }
    Ok(ms(start.elapsed()))
}

/// The cost of a turn of the loop with nothing to do: what every wakeup
/// pays, the watchdog's included.
fn idle_turn_us(desktop: &mut Headless) -> anyhow::Result<f64> {
    const TURNS: u32 = 2000;
    let start = Instant::now();
    for _ in 0..TURNS {
        desktop.dispatch(Duration::ZERO)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1e6 / f64::from(TURNS))
}

#[test]
#[ignore = "wall-clock timings: run alone, by CI's timing job"]
fn timings() -> anyhow::Result<()> {
    let mut report = Report::default();

    // Startup, to both screens drawn with their wallpapers and the shell.
    let start = Instant::now();
    let data = tempfile::tempdir()?;
    applications(data.path())?;
    let mut desktop = Headless::start_options(options(data.path()))?;
    desktop.dispatch_until(TIMEOUT, Headless::wallpapers_ready)?;
    desktop.advance(FRAME)?;
    report.add_target_ms(
        "Startup: two screens and the shell drawn",
        ms(start.elapsed()),
        STARTUP_TARGET_MS,
        "its target",
    );
    desktop.advance(Duration::from_secs(2))?;
    report.add(
        "A turn of the loop at rest",
        format!("{:.1} µs", idle_turn_us(&mut desktop)?),
    );

    // The launcher: Super+Space, to its first frame, every application
    // shown; drawn as it opens while closed (#242).
    report.add_frame_ms(
        format!("Opening the launcher on {APPLICATIONS} applications, to its first frame"),
        open_launcher(&mut desktop)?,
    );
    // A search, closed, and opened again at once: the grid comes back
    // before the closed card was drawn again.
    for code in [KEY_A, KEY_P, KEY_ESC] {
        press(&mut desktop, code)?;
        desktop.advance(FRAME)?;
    }
    report.add_frame_ms(
        "Opening it again right after a search",
        open_launcher(&mut desktop)?,
    );
    press(&mut desktop, KEY_ESC)?;
    desktop.advance(Duration::from_secs(2))?;

    // The settings, a client of their own, to their first frame.
    let dirs = tempfile::tempdir()?;
    let wayland = desktop.socket_path().to_path_buf();
    let windows = desktop.window_count();
    let start = Instant::now();
    let settings = hocus_settings::client::spawn(hocus_settings::client::Options {
        api: hocus_api::socket_for(&wayland),
        wayland,
        page: None,
        lang: hocus_shell::Lang::En,
        app_id: "hocus-settings".into(),
        system_bus: hocus_settings::client::SystemBus::Off,
        xdg: hocus_settings::client::Xdg::At(hocus_platform::mime::Dirs {
            config: vec![dirs.path().join("config")],
            data: Vec::new(),
        }),
        zoneinfo: repository().join("tests/scenarios/zoneinfo"),
        xkb: hocus_platform::xkb::Registry::At(repository().join("tests/scenarios/xkb")),
        home: None,
    })?;
    desktop.dispatch_until(TIMEOUT, |d| {
        d.window_count() > windows && settings.access().is_some()
    })?;
    report.add_target_ms(
        "Opening the settings, to their first frame",
        ms(start.elapsed()),
        OPENING_TARGET_MS,
        "its target",
    );
    drop(settings);

    // Twenty windows on the two screens.
    let _clients = open_windows(&mut desktop, 20)?;
    report.add(
        "A turn of the loop at rest, twenty windows",
        format!("{:.1} µs", idle_turn_us(&mut desktop)?),
    );

    // Input to photon: the pointer moves over the windows, and the frame
    // that shows it is drawn (the wait for the screen's refresh aside).
    let mut samples = Vec::new();
    for i in 0..200 {
        let x = 200.0 + f64::from(i % 50) * 18.0;
        let y = 150.0 + f64::from(i / 50) * 120.0;
        let start = Instant::now();
        desktop.request(Request::PointerMove { x, y })?;
        desktop.advance(FRAME)?;
        samples.push(ms(start.elapsed()));
    }
    samples.sort_by(f64::total_cmp);
    report.add_frame_ms("Input to photon, median", percentile(&samples, 0.5));
    report.add_frame_ms(
        "Input to photon, 95th percentile",
        percentile(&samples, 0.95),
    );
    report.add_frame_ms("Input to photon, worst", percentile(&samples, 1.0));
    drop(desktop);

    // Frame times of the twenty windows' scenario.
    let scenario =
        Scenario::load(&repository().join("tests/scenarios/budget-twenty-windows.toml"))?;
    let options = RunOptions {
        artifacts: repository().join("target/test-artifacts"),
        bless: false,
        test_client: Some(PathBuf::from(env!("CARGO_BIN_EXE_hocus-test-client"))),
    };
    let start = Instant::now();
    // Its counted budgets gate in the scenario tests; here it is timed.
    match scenario.run_reporting(&options) {
        Ok(measured) => {
            report.add(
                "The twenty windows' scenario, whole",
                format!("{:.1} s", start.elapsed().as_secs_f64()),
            );
            for (i, cost) in measured.costs.iter().enumerate() {
                report.add(
                    format!(
                        "The twenty windows' scenario, part {}: frames, scenes built, allocations",
                        i + 1
                    ),
                    format!(
                        "{:?}, {:?}, {}",
                        cost.frames, cost.renders, cost.allocations.count
                    ),
                );
            }
            for output in &measured.stats.outputs {
                let over: u64 = output
                    .histogram
                    .iter()
                    .filter(|b| b.up_to_ms.is_none_or(|up| up > FRAME_BUDGET_MS))
                    .map(|b| b.count)
                    .sum();
                report.add(
                    format!("{}: frames drawn, over {FRAME_BUDGET_MS} ms", output.name),
                    format!("{} frames, {over} over", output.frames),
                );
                report.add_frame_ms(format!("{}: slowest frame", output.name), output.max_ms);
            }
        }
        Err(err) => report.add("The twenty windows' scenario", format!("failed: {err:#}")),
    }
    report.publish()
}
