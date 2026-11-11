//! The Settings' Performances page (#655, docs/design/settings.md
//! "Performances"): the real `hocus-settings` window, a client of a
//! headless compositor whose recorder samples every 200 ms; its state,
//! home and runtime directories the test's own, its system bus a private
//! `dbus-daemon`. The page's logic (the day read, its tiles, its chart's
//! buckets) is tested in the crate (`hocus_settings::perf`); its look on
//! screenshots, of an invented day (`hocus_perf::fixture`).

use crate::common;
use crate::fakes;
use crate::settings_page;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_perf::{Day, EventKind, files};
use serde_json::Value;
use settings_page::{Floor, Wheel};

const APP_ID: &str = "hocus-settings";

struct Bed {
    hp: Headless,
    log: PathBuf,
    script: PathBuf,
    state: PathBuf,
    _fakes: Fakes,
    _dir: tempfile::TempDir,
}

impl Bed {
    /// The compositor (recording every 200 ms, where the Settings read
    /// when `shared`, else in a folder of its own), a bus of its own, and
    /// the Settings' script in `locale`; `None` when the Settings or
    /// `dbus-daemon` are not there.
    fn start(locale: &str, shared: bool) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        let bus = Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !bus {
            common::missing("dbus-daemon");
        }
        let mut fakes = Fakes::empty()?;
        fakes.start_bus()?;
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state/hocuspanda",
            "data",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, "# Mine.\n")?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{}' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL={locale} LANG={locale} TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                fakes.address,
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let state = root.join("state/hocuspanda");
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(if shared {
            state.clone()
        } else {
            root.join("compositor")
        });
        config.appearance.reduced_motion = true;
        config.perf.interval = Some(Duration::from_millis(200));
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        Ok(Some(Self {
            hp,
            log,
            script,
            state,
            _fakes: fakes,
            _dir: dir,
        }))
    }

    /// The Settings launched on `section`, their window open.
    fn open_on(&mut self, section: &str) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), section.to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(
            open,
            "the window never opened: {}",
            common::logged(&self.log)
        );
        self.shows(|s| s["page"] == section)
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " shows {") };
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                common::logged(&log)
            )
        })
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let places = settings_page::places_with(&mut self.hp, &self.log, id)?;
        let n = |i: usize| places[id][i].as_f64().unwrap_or_default();
        let window = self.window()?;
        let at = (
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        );
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    /// `id` scrolled into view.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        settings_page::reveal(
            &mut self.hp,
            &self.log,
            APP_ID,
            id,
            &Wheel {
                x: 600.0,
                notches: 3,
                top: 56.0,
                floor: Floor::Margin(16.0),
            },
        )
    }
}

/// Every sample the compositor's recorder wrote under `state`.
fn recorded(state: &Path) -> Day {
    let dir = files::dir_in(state);
    let mut all = Day::default();
    for date in files::days(&dir) {
        if let Ok(day) = Day::read(&dir, date) {
            all.samples.extend(day.samples);
        }
    }
    all
}

/// The page tells the compositor its name, and the Settings' process:
/// both in the day the compositor records, with the page's next sample.
#[test]
fn the_page_opened_is_in_the_day_and_the_settings_are_measured() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("fr_FR.UTF-8", true)? else {
        return Ok(());
    };
    bed.open_on("performance")?;
    let mut day = Day::default();
    let state = bed.state.clone();
    let said = bed.hp.dispatch_until(Duration::from_secs(20), |_| {
        day = recorded(&state);
        let page = day
            .samples
            .iter()
            .flat_map(|s| &s.e)
            .any(|e| e.kind == EventKind::SettingsPage && e.what == "Performances");
        page && day.samples.iter().any(|s| s.m.len() == 3 && s.m[2] > 0)
    })?;
    assert!(said, "{:#?}", day.samples.last());
    // The page read the day the compositor is recording.
    let shown = bed.shows(|s| s["perf"]["read"] == true)?;
    assert_eq!(shown["perf"]["date"], files_today());
    Ok(())
}

fn files_today() -> String {
    jiff::Zoned::now()
        .with_time_zone(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

/// The window, cropped from the screen, to `out/name.png`, once still.
fn shoot(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..30 {
        if still >= 5 {
            break;
        }
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
    }
    let window = bed.window()?;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let crop = image::imageops::crop_imm(
        &image,
        window.x.max(0) as u32,
        window.y.max(0) as u32,
        window.width as u32,
        window.height as u32,
    )
    .to_image();
    let file = out.join(format!("{name}.png"));
    crop.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The invented day today, and two days before it, where the Settings
/// read them.
fn invent(state: &Path) -> anyhow::Result<()> {
    let dir = files::dir_in(state);
    std::fs::create_dir_all(&dir)?;
    let tz = jiff::tz::TimeZone::UTC;
    let today = jiff::Zoned::now().with_time_zone(tz.clone()).date();
    for back in [0, 1, 3] {
        let date = today.checked_sub(jiff::Span::new().days(back))?;
        let day = hocus_perf::fixture::working_day(date, &tz);
        std::fs::write(files::path(&dir, date), hocus_perf::fixture::text(&day))?;
    }
    Ok(())
}

/// The pull request's screenshots, to `$HOCUS_SHOTS` when it names a
/// folder; run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    for (locale, tag) in [("fr_FR.UTF-8", "fr"), ("en_US.UTF-8", "en")] {
        let Some(mut bed) = Bed::start(locale, false)? else {
            return Ok(());
        };
        invent(&bed.state)?;
        bed.open_on("performance")?;
        bed.shows(|s| s["perf"]["read"] == true && s["perf"]["samples"].as_u64() > Some(0))?;
        bed.hp.pointer_move(1900.0, 1060.0);
        shoot(&mut bed, &out, &format!("performance-{tag}-top"))?;
        if tag == "en" {
            continue;
        }
        bed.reveal("perf:chart")?;
        bed.hp.pointer_move(1900.0, 1060.0);
        shoot(&mut bed, &out, "performance-fr-chart")?;
        bed.reveal("row:perf.record")?;
        bed.hp.pointer_move(1900.0, 1060.0);
        shoot(&mut bed, &out, "performance-fr-bottom")?;
        bed.reveal("perf:week")?;
        bed.click("perf:week")?;
        bed.shows(|s| s["perf"]["week"] == true && s["perf"]["read"] == true)?;
        bed.hp.pointer_move(1900.0, 1060.0);
        shoot(&mut bed, &out, "performance-fr-week")?;
    }
    Ok(())
}
