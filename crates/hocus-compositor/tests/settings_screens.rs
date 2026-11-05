//! The Settings' Écrans page (6.9, issue #409): the real `hocus-settings`
//! window, a client of a headless compositor of two screens (2560 × 1600
//! at 1, 3440 × 1440), its configuration file, state, home and runtime
//! directories the test's own; no bus, no GPU (it draws with tiny-skia).
//!
//! One test per measurable point of the spec: the arrangement drawn from
//! the compositor's screens; a drag that writes both places, which the
//! compositor applies; the scale written at once and taken back with
//! "Annuler", the file byte for byte as before; a resolution tried, kept
//! with "Garder" and written; a resolution tried and taken back by the
//! compositor after 15 s on the held clock, the file untouched; the
//! compositor following the file's screens (mode, place, scale, off, the
//! primary screen); the search's words; at rest, no commit.
//!
//! What the window shows is read from its log ("shows" and "places"
//! lines, JSON), as `settings.rs` does.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, WAIT, chord, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_F: u32 = 33;

/// A configuration of the user's own, its screens not set.
const FILE: &str = "# My desktop, by hand.\n\
    [keyboard]\n\
    layouts = [\"fr\", \"us\"]   # both\n\
    \n\
    [panel]\n\
    form = \"full\"\n";

/// The test's two screens.
const LAPTOP: &str = "HEADLESS-1";
const EXTERNAL: &str = "HEADLESS-2";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

impl Bed {
    /// A compositor of two screens, the file holding `file`; `None` when
    /// the Settings are not built.
    fn start(file: &str) -> anyhow::Result<Option<Self>> {
        Self::start_on(file, &["2560x1600@1", "3440x1440"])
    }

    fn start_on(file: &str, outputs: &[&str]) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
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
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(config_file);
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: outputs
                    .iter()
                    .map(|o| o.parse())
                    .collect::<Result<_, _>>()?,
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        Ok(Some(Self {
            hp,
            dir,
            log,
            script,
        }))
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    fn text(&self) -> String {
        std::fs::read_to_string(self.file()).unwrap_or_default()
    }

    /// The Settings on the Écrans page, the screens drawn.
    fn open(&mut self) -> anyhow::Result<Value> {
        let argv = vec![self.script.display().to_string(), "screens".to_owned()];
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv,
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        let shown = self.shows(|s| {
            s["file"] == "saved"
                && s["screens"]["drawn"]
                    .as_array()
                    .is_some_and(|d| d.len() >= 2)
        })?;
        self.places_with("drawing")?;
        Ok(shown)
    }

    fn plain(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned()
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> {
            let text = String::from_utf8_lossy(&std::fs::read(log).ok()?).into_owned();
            let line = text.lines().rev().find(|l| l.contains(" shows {"))?;
            let at = line.find(" shows {")? + " shows ".len();
            serde_json::from_str(&line[at..]).ok()
        };
        let seen = self
            .hp
            .dispatch_until(WAIT, |_| read(&log).is_some_and(|s| done(&s)))?;
        let last = read(&log);
        anyhow::ensure!(
            seen,
            "the window never showed it; it shows {}",
            last.map_or_else(|| "nothing".to_owned(), |s| s.to_string())
        );
        last.ok_or_else(|| anyhow::anyhow!("nothing shown"))
    }

    /// The window's places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> {
            let text = String::from_utf8_lossy(&std::fs::read(log).ok()?).into_owned();
            let line = text.lines().rev().find(|l| l.contains(" places {"))?;
            let at = line.find(" places {")? + " places ".len();
            serde_json::from_str(&line[at..]).ok()
        };
        let seen = self
            .hp
            .dispatch_until(WAIT, |_| read(&log).is_some_and(|p| p.get(id).is_some()))?;
        anyhow::ensure!(seen, "{id} is nowhere: {:?}", read(&log));
        read(&log).ok_or_else(|| anyhow::anyhow!("no places"))
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// The middle of `id` on the screen.
    fn at(&mut self, id: &str) -> anyhow::Result<(f64, f64)> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        ))
    }

    fn click_at(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let at = self.at(id)?;
        self.click_at(at)
    }

    /// Screen `name`'s middle on the drawing, on the screen.
    fn screen_at(&mut self, shown: &Value, name: &str) -> anyhow::Result<(f64, f64)> {
        let drawn = drawn(shown, name);
        let n = |i: usize| drawn["at"][i].as_f64().unwrap_or_default();
        let places = self.places_with("drawing")?;
        let d = |i: usize| places["drawing"][i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + d(0) + n(0) + n(2) / 2.0,
            f64::from(window.y) + d(1) + n(1) + n(3) / 2.0,
        ))
    }

    /// A menu's choice `n` of the page: its button, then its option.
    fn choose(&mut self, row: &str, n: usize) -> anyhow::Result<Value> {
        self.click(&format!("control:{row}"))?;
        let shown = self.shows(|s| s["screens"]["menu"] == row)?;
        self.click(&format!("option:{n}"))?;
        Ok(shown)
    }

    /// Waits for the file to hold `done`.
    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = self.text();
        anyhow::ensure!(
            seen,
            "the file never held it:\n{text}\nthe window said:\n{}",
            self.plain()
        );
        Ok(text)
    }

    /// Screen `name`'s place on the desktop, once `done` holds of it.
    fn output(&mut self, name: &str, done: impl Fn(Rect) -> bool) -> anyhow::Result<Rect> {
        let get = |hp: &Headless| {
            hp.outputs()
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, r)| r)
        };
        let seen = self
            .hp
            .dispatch_until(WAIT, |hp| get(hp).is_some_and(&done))?;
        let now = get(&self.hp);
        anyhow::ensure!(seen, "{name} is at {now:?}: {:?}", self.hp.outputs());
        now.ok_or_else(|| anyhow::anyhow!("no {name}"))
    }
}

/// Screen `name` as drawn.
fn drawn(shown: &Value, name: &str) -> Value {
    shown["screens"]["drawn"]
        .as_array()
        .and_then(|d| d.iter().find(|s| s["name"] == name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Row `id` of the chosen screen.
fn row(shown: &Value, id: &str) -> Value {
    shown["screens"]["rows"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

fn f(v: &Value, i: usize) -> f64 {
    v["at"][i].as_f64().unwrap_or(f64::NAN)
}

#[test]
fn the_arrangement_is_drawn_from_the_compositors_screens() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let laptop = drawn(&shown, LAPTOP);
    let external = drawn(&shown, EXTERNAL);
    assert_eq!(laptop["number"], 1, "{shown}");
    assert_eq!(external["number"], 2, "{shown}");
    // At their logical sizes, side by side, tops aligned, as the
    // compositor has them.
    let ratio = f(&external, 2) / f(&laptop, 2);
    assert!(
        (ratio - 3440.0 / 2560.0).abs() < 0.02,
        "{laptop} {external}"
    );
    assert!((f(&external, 0) - (f(&laptop, 0) + f(&laptop, 2))).abs() <= 1.0);
    assert!((f(&external, 1) - f(&laptop, 1)).abs() <= 1.0);
    assert_eq!(laptop["line"], "2560 × 1600 · 60 Hz");
    // The first is chosen, its bar drawn: the primary one.
    assert_eq!(laptop["selected"], true);
    assert_eq!(laptop["bar"], true);
    assert_eq!(shown["screens"]["chosen"], LAPTOP);
    // Its resolutions, the native one first.
    let shown = {
        bed.click("control:screens.resolution")?;
        bed.shows(|s| s["screens"]["menu"] == "screens.resolution")?
    };
    let choices: Vec<&str> = shown["screens"]["choices"]
        .as_array()
        .map(|c| c.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        choices.first(),
        Some(&"2560 × 1600 (native)"),
        "{choices:?}"
    );
    assert!(choices.contains(&"1920 × 1080"), "{choices:?}");
    // A click on the other chooses it: its rows.
    chord(&mut bed.hp, &[common::KEY_ESC]);
    bed.shows(|s| s["screens"]["menu"].is_null())?;
    let at = bed.screen_at(&shown, EXTERNAL)?;
    bed.click_at(at)?;
    let shown = bed.shows(|s| s["screens"]["chosen"] == EXTERNAL)?;
    assert_eq!(
        row(&shown, "screens.resolution")["value"],
        "3440 × 1440 (native)"
    );
    // Both on: either may be turned off; one rate: its menu is inert.
    assert_eq!(row(&shown, "screens.enabled")["inert"], false);
    assert_eq!(row(&shown, "screens.rate")["inert"], true);
    Ok(())
}

#[test]
fn dragging_the_external_above_the_laptop_writes_both_places() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let laptop = drawn(&shown, LAPTOP);
    let external = drawn(&shown, EXTERNAL);
    let from = bed.screen_at(&shown, EXTERNAL)?;
    let to_laptop = bed.screen_at(&shown, LAPTOP)?;
    // Its middle over the laptop's middle, a little above it.
    let to = (
        to_laptop.0,
        to_laptop.1 - f(&laptop, 3) / 2.0 - f(&external, 3) / 2.0 - 8.0,
    );
    bed.hp.pointer_move(from.0, from.1);
    wait(&mut bed.hp, Duration::from_millis(30))?;
    bed.hp.button(BTN_LEFT, true);
    for i in 1..=12 {
        let k = f64::from(i) / 12.0;
        bed.hp
            .pointer_move(from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
        wait(&mut bed.hp, Duration::from_millis(15))?;
    }
    // Held there: the dragged screen drawn where the pointer took it.
    let target = (
        f(&laptop, 0) + f(&laptop, 2) / 2.0,
        f(&laptop, 1) - f(&external, 3) / 2.0 - 8.0,
    );
    bed.shows(|s| {
        let held = drawn(s, EXTERNAL);
        s["screens"]["dragging"] == true
            && (f(&held, 0) + f(&held, 2) / 2.0 - target.0).abs() <= 1.5
            && (f(&held, 1) + f(&held, 3) / 2.0 - target.1).abs() <= 1.5
    })?;
    bed.hp.button(BTN_LEFT, false);
    // Above, flush and centred: both places written, the smallest 0.
    let text = bed.written(|t| t.contains("position"))?;
    assert!(text.starts_with(FILE), "{text}");
    assert!(
        text.contains("[outputs.HEADLESS-1]\nposition = [440, 1440]\n"),
        "{text}"
    );
    assert!(
        text.contains("[outputs.HEADLESS-2]\nposition = [0, 0]\n"),
        "{text}"
    );
    let shown = bed.shows(|s| s["toast"].as_str().is_some_and(|t| t.contains("au-dessus")))?;
    assert_eq!(
        shown["toast"], "Écran 2 au-dessus de l’écran 1, centrés",
        "{shown}"
    );
    // The compositor follows the file.
    bed.output(EXTERNAL, |r| r == Rect::new(0, 0, 3440, 1440))?;
    bed.output(LAPTOP, |r| r == Rect::new(440, 1440, 2560, 1600))?;
    Ok(())
}

#[test]
fn the_scale_is_written_at_once_and_annuler_puts_the_file_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    // Automatique, 100, 125…: 125 % is the third.
    bed.choose("screens.scale", 2)?;
    let text = bed.written(|t| t.contains("scale"))?;
    assert_eq!(
        text,
        format!("{FILE}\n[outputs.HEADLESS-1]\nscale = 1.25\n")
    );
    let shown = bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("Échelle"))
    })?;
    assert_eq!(shown["toast"], "Échelle de l’écran 1 : 125 %");
    assert_eq!(shown["toast_action"], "undo");
    // The compositor takes it: 2048 × 1280 logical, the other beside it.
    bed.output(LAPTOP, |r| (r.width, r.height) == (2048, 1280))?;
    bed.output(EXTERNAL, |r| r.x == 2048)?;
    let shown = bed.shows(|s| row(s, "screens.scale")["value"] == "125 %" && s["scale"] == 1.25)?;
    assert_eq!(row(&shown, "screens.scale")["inert"], false);
    // "Annuler": the key taken out, the file as it was.
    // The window draws at its screen's new scale before it is clicked.
    bed.shows(|s| s["scale"] == 1.25)?;
    bed.click("toast-action")?;
    bed.written(|t| t == FILE)?;
    bed.output(LAPTOP, |r| (r.width, r.height) == (2560, 1600))?;
    bed.shows(|s| {
        row(s, "screens.scale")["value"] == "Automatique (100 %)"
            && s["toast"].as_str().is_some_and(|t| t.starts_with("Annulé"))
    })?;
    Ok(())
}

/// The external chosen, a resolution of its tried: the dialog shows.
fn try_resolution(bed: &mut Bed) -> anyhow::Result<Value> {
    let shown = bed.open()?;
    let at = bed.screen_at(&shown, EXTERNAL)?;
    bed.click_at(at)?;
    bed.shows(|s| s["screens"]["chosen"] == EXTERNAL)?;
    // 3440 × 1440 (native), 2560 × 1440, 2560 × 1080…
    bed.choose("screens.resolution", 2)?;
    let shown = bed.shows(|s| !s["screens"]["dialog"].is_null())?;
    assert_eq!(shown["screens"]["dialog"]["mode"], "2560x1080@60");
    assert_eq!(shown["screens"]["dialog"]["keep_focused"], true);
    // Tried, not written: the compositor shows it.
    bed.output(EXTERNAL, |r| (r.width, r.height) == (2560, 1080))?;
    Ok(shown)
}

#[test]
fn a_resolution_kept_with_garder_is_written() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    try_resolution(&mut bed)?;
    assert_eq!(bed.text(), FILE, "nothing written while it is tried");
    bed.click("dialog:keep")?;
    let text = bed.written(|t| t.contains("mode"))?;
    assert_eq!(
        text,
        format!("{FILE}\n[outputs.HEADLESS-2]\nmode = \"2560x1080@60\"\n")
    );
    let shown = bed.shows(|s| s["screens"]["dialog"].is_null() && !s["toast"].is_null())?;
    assert_eq!(
        shown["toast"], "Gardé : 2560 × 1080 à 60 Hz sur l’écran 2",
        "{shown}"
    );
    // Kept, and still so once the compositor read the file.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.output(EXTERNAL, |r| (r.width, r.height) == (2560, 1080))?;
    // Its "Annuler" goes straight back, without a countdown.
    bed.click("toast-action")?;
    bed.written(|t| t == FILE)?;
    bed.output(EXTERNAL, |r| (r.width, r.height) == (3440, 1440))?;
    let shown = bed.shows(|s| row(s, "screens.resolution")["value"] == "3440 × 1440 (native)")?;
    assert!(shown["screens"]["dialog"].is_null());
    Ok(())
}

#[test]
fn a_resolution_not_kept_goes_back_after_15_s_on_the_held_clock() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.hp.hold_clock();
    let shown = try_resolution(&mut bed)?;
    assert_eq!(shown["screens"]["dialog"]["left"], 15);
    // The compositor counts, on its clock: the window shows its seconds.
    for _ in 0..3 {
        bed.hp.step(Duration::from_secs(1))?;
    }
    let shown = bed.shows(|s| s["screens"]["dialog"]["left"] == 12)?;
    assert_eq!(shown["page"], "screens");
    bed.output(EXTERNAL, |r| (r.width, r.height) == (2560, 1080))?;
    for _ in 0..12 {
        bed.hp.step(Duration::from_secs(1))?;
    }
    // Back by itself, nothing written.
    bed.output(EXTERNAL, |r| (r.width, r.height) == (3440, 1440))?;
    bed.shows(|s| {
        s["screens"]["dialog"].is_null()
            && row(s, "screens.resolution")["value"] == "3440 × 1440 (native)"
    })?;
    assert_eq!(bed.text(), FILE);
    Ok(())
}

#[test]
fn the_compositor_follows_the_files_screens() -> anyhow::Result<()> {
    // The compositor alone: the file changed by hand.
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, FILE)?;
    let config = Config {
        file: Some(file.clone()),
        ..Config::default()
    };
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["2560x1600@1".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let rect = |hp: &Headless, name: &str| {
        hp.outputs()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r)
    };
    assert_eq!(rect(&hp, LAPTOP), Some(Rect::new(0, 0, 2560, 1600)));
    assert_eq!(rect(&hp, EXTERNAL), Some(Rect::new(2560, 0, 3440, 1440)));
    let until = |hp: &mut Headless, what: &str, done: &dyn Fn(&Headless) -> bool| {
        let seen = hp.dispatch_until(WAIT, |hp| done(hp))?;
        anyhow::ensure!(seen, "{what}: {:?}", hp.outputs());
        Ok::<(), anyhow::Error>(())
    };
    // A mode, a place on the left, a scale.
    std::fs::write(
        &file,
        format!(
            "{FILE}\n[outputs.HEADLESS-2]\nmode = \"1920x1080@60\"\nposition = [0, 0]\n\n\
             [outputs.HEADLESS-1]\nscale = 2\nposition = [1920, 0]\n"
        ),
    )?;
    until(&mut hp, "the file's screens", &|hp| {
        rect(hp, EXTERNAL) == Some(Rect::new(0, 0, 1920, 1080))
            && rect(hp, LAPTOP) == Some(Rect::new(1920, 0, 1280, 800))
    })?;
    // Overlapping places: left to right, the file's sizes kept.
    std::fs::write(
        &file,
        format!(
            "{FILE}\n[outputs.HEADLESS-2]\nmode = \"1920x1080@60\"\nposition = [0, 0]\n\n\
             [outputs.HEADLESS-1]\nscale = 2\nposition = [100, 0]\n"
        ),
    )?;
    until(&mut hp, "left to right", &|hp| {
        rect(hp, LAPTOP) == Some(Rect::new(0, 0, 1280, 800))
            && rect(hp, EXTERNAL) == Some(Rect::new(1280, 0, 1920, 1080))
    })?;
    // The external off: off the desktop; the primary screen named.
    std::fs::write(
        &file,
        format!("{FILE}output = \"HEADLESS-2\"\n\n[outputs.HEADLESS-2]\nenabled = false\n"),
    )?;
    until(&mut hp, "one screen on", &|hp| {
        hp.outputs().len() == 1 && rect(hp, LAPTOP) == Some(Rect::new(0, 0, 2560, 1600))
    })?;
    // On again, and the bar's screen the one named.
    std::fs::write(&file, format!("{FILE}output = \"HEADLESS-2\"\n"))?;
    until(&mut hp, "both on", &|hp| hp.outputs().len() == 2)?;
    until(&mut hp, "the bar on the one named", &|hp| {
        hp.state().displays_json()["outputs"]
            .as_array()
            .is_some_and(|o| {
                o.iter()
                    .any(|o| o["name"] == EXTERNAL && o["primary"] == true)
            })
    })?;
    // A file that does not read changes nothing.
    std::fs::write(&file, "[outputs.HEADLESS-2\nmode = 3\n")?;
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.outputs().len(), 2);
    assert_eq!(rect(&hp, LAPTOP), Some(Rect::new(0, 0, 2560, 1600)));
    Ok(())
}

#[test]
fn searching_hidpi_and_miroir_finds_the_scale_and_mirroring() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    for (word, found) in [("hidpi", "Échelle"), ("miroir", "Recopie")] {
        chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_F]);
        bed.shows(|s| s["focus"] == "search")?;
        wait(&mut bed.hp, Duration::from_millis(100))?;
        for c in word.chars() {
            let code = match c {
                'h' => 35,
                'i' => 23,
                'd' => 32,
                'p' => 25,
                'm' => 50,
                'r' => 19,
                'o' => 24,
                _ => anyhow::bail!("no key for {c}"),
            };
            bed.hp.key(code, true);
            bed.hp.key(code, false);
            wait(&mut bed.hp, Duration::from_millis(10))?;
        }
        let shown = bed.shows(|s| {
            s["query"] == word && s["results"].as_array().is_some_and(|r| !r.is_empty())
        })?;
        assert_eq!(shown["results"][0]["name"], found, "{shown}");
        chord(&mut bed.hp, &[common::KEY_ESC]);
        bed.shows(|s| s["query"] == "")?;
    }
    Ok(())
}

#[test]
fn at_rest_the_screens_page_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FILE)? else {
        return Ok(());
    };
    bed.open()?;
    bed.hp.pointer_move(5.0, 5.0);
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 10 {
            break;
        }
    }
    anyhow::ensure!(still >= 10, "the window never came to rest");
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.window_commits(), last, "the window drew at rest");
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the page with two
/// screens (a laptop's panel at 2 and a 34-inch one), the external one
/// chosen; the "Garder ?" countdown; the external unplugged.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(FILE, &["2880x1800@2:340x210mm", "3440x1440:800x335mm"])?
    else {
        return Ok(());
    };
    bed.hp.set_internal(LAPTOP);
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        // Drawn, and still.
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
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let pad = 24;
        let x = (window.x * 2 - pad).max(0) as u32;
        let y = (window.y * 2 - pad).max(0) as u32;
        let cw = ((window.width * 2 + 2 * pad) as u32).min(w as u32 - x);
        let ch = ((window.height * 2 + 2 * pad) as u32).min(h as u32 - y);
        let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    let shown = bed.open()?;
    let at = bed.screen_at(&shown, EXTERNAL)?;
    bed.click_at(at)?;
    bed.shows(|s| s["screens"]["chosen"] == EXTERNAL)?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "screens.png")?;
    bed.hp.hold_clock();
    bed.choose("screens.resolution", 2)?;
    bed.shows(|s| !s["screens"]["dialog"].is_null())?;
    for _ in 0..3 {
        bed.hp.step(Duration::from_secs(1))?;
    }
    bed.shows(|s| s["screens"]["dialog"]["left"] == 12)?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "keep.png")?;
    bed.click("dialog:revert")?;
    bed.shows(|s| s["screens"]["dialog"].is_null())?;
    bed.hp.release_clock();
    bed.hp.unplug(EXTERNAL)?;
    bed.shows(|s| {
        drawn(s, EXTERNAL)["state"]
            .as_str()
            .is_some_and(|t| t.starts_with("Débranché"))
    })?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "unplugged.png")?;
    Ok(())
}

#[test]
fn windows_go_along_with_their_screen() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, FILE)?;
    let config = Config {
        file: Some(file.clone()),
        ..Config::default()
    };
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080@1".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let before = common::open(&mut hp, "carried", (700, 500))?;
    let area = hp
        .work_area_of("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("no area"))?;
    // The screen at 1.5: 1280 × 720 logical; the window keeps its share of
    // the work area, inside it.
    std::fs::write(
        &file,
        format!("{FILE}\n[outputs.HEADLESS-1]\nscale = 1.5\n"),
    )?;
    let seen = hp.dispatch_until(WAIT, |hp| {
        hp.outputs().first().is_some_and(|(_, r)| r.width == 1280)
    })?;
    anyhow::ensure!(seen, "never scaled: {:?}", hp.outputs());
    wait(&mut hp, Duration::from_millis(200))?;
    let now_area = hp
        .work_area_of("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("no area"))?;
    let after = common::rect_of(&hp, "carried").ok_or_else(|| anyhow::anyhow!("no window"))?;
    let share = |r: Rect, a: Rect| {
        (
            (f64::from(r.x) + f64::from(r.width) / 2.0 - f64::from(a.x)) / f64::from(a.width),
            (f64::from(r.y) + f64::from(r.height) / 2.0 - f64::from(a.y)) / f64::from(a.height),
        )
    };
    eprintln!("before {before:?} in {area:?}, after {after:?} in {now_area:?}");
    assert!(
        after.x >= now_area.x
            && after.y >= now_area.y
            && after.x + after.width <= now_area.x + now_area.width
            && after.y + after.height <= now_area.y + now_area.height,
        "{after:?} out of {now_area:?}"
    );
    let (b, a) = (share(before, area), share(after, now_area));
    assert!(
        (b.0 - a.0).abs() < 0.05 && (b.1 - a.1).abs() < 0.05,
        "{b:?} {a:?}"
    );
    Ok(())
}

#[test]
fn a_window_goes_back_to_its_screen_turned_on_again() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, FILE)?;
    let config = Config {
        file: Some(file.clone()),
        ..Config::default()
    };
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080@1".parse()?, "1920x1080@1".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    // On the second screen.
    hp.pointer_move(2880.0, 540.0);
    let first = common::open(&mut hp, "away", (600, 400))?;
    anyhow::ensure!(first.x >= 1920, "{first:?}");
    // Turned off: the window goes to the first screen.
    std::fs::write(
        &file,
        format!("{FILE}\n[outputs.HEADLESS-2]\nenabled = false\n"),
    )?;
    let gone = hp.dispatch_until(WAIT, |hp| {
        hp.outputs().len() == 1 && common::rect_of(hp, "away").is_some_and(|r| r.x < 1920)
    })?;
    anyhow::ensure!(
        gone,
        "{:?} {:?}",
        hp.outputs(),
        common::rect_of(&hp, "away")
    );
    // On again while the first one's scale changes: it goes back all the
    // same, the desktop having moved it, not the user.
    std::fs::write(
        &file,
        format!("{FILE}\n[outputs.HEADLESS-1]\nscale = 1.5\n"),
    )?;
    let back = hp.dispatch_until(WAIT, |hp| {
        hp.outputs().len() == 2 && common::rect_of(hp, "away").is_some_and(|r| r.x >= 1280)
    })?;
    anyhow::ensure!(
        back,
        "{:?} {:?}",
        hp.outputs(),
        common::rect_of(&hp, "away")
    );
    Ok(())
}
