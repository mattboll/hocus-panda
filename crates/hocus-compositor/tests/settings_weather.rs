//! Settings › Weather (#814) and the rain's "Quand": the real
//! `hocus-settings` window, a client of a headless compositor, its
//! configuration file, home and state the test's own; the places found
//! in a saved geocoding answer (`HOCUS_WEATHER_PLACES`), never the
//! network.
//!
//! A place typed is found and, chosen, written with where it is; the
//! rain's "Quand" waits for a place, then writes its key. The pull
//! request's screenshots are an ignored test (`HOCUS_SHOTS`).

use crate::common;
use crate::settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
/// `s` and `a` on a US keyboard, by their evdev codes.
const KEY_S: u32 = 31;
const KEY_A: u32 = 30;

const FILE: &str = "# Mine.\n[panel]\nform = \"full\"\n";
const PLACED: &str = "# Mine.\n[weather]\nplace = \"Saint-Étienne\"\nlatitude = 45.4339\n\
    longitude = 4.39\n\n[effects]\nrain-on-glass = true\n";

/// The saved geocoding answer.
fn places() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hocus-weather/fixtures/places.json")
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
}

impl Bed {
    /// A compositor, the configuration file holding `file`, the Settings'
    /// script in `locale`.
    fn start(file: &str, locale: &str) -> anyhow::Result<Self> {
        let program = common::settings_program();
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
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL={locale} LANG={locale} HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex HOCUS_WEATHER_PLACES='{}' \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                places().display(),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(config_file);
        config.appearance.reduced_motion = true;
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        Ok(Self {
            hp,
            dir,
            log,
            script,
        })
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    /// The Settings opened on `section`.
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
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = common::last_json(&log, " shows {").filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "the window never showed it; it shows {:?}",
                common::last_json(&log, " shows {")
            )
        })
    }

    fn window(&self) -> anyhow::Result<hocus_compositor::model::Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// The middle of `id` on the screen, scrolled into view.
    fn at(&mut self, id: &str) -> anyhow::Result<(f64, f64)> {
        let wheel = Wheel {
            x: 600.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)?;
        let places = settings_page::places_with(&mut self.hp, &self.log, id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        ))
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let at = self.at(id)?;
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn keys(&mut self, codes: &[u32]) -> anyhow::Result<()> {
        for code in codes {
            self.hp.key(*code, true);
            self.hp.key(*code, false);
            wait(&mut self.hp, Duration::from_millis(10))?;
        }
        Ok(())
    }

    /// Waits for the file to hold `done`.
    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(
            seen,
            "the file never held it:\n{text}\nthe window said:\n{}",
            common::logged(&self.log)
        );
        Ok(text)
    }

    /// Typed "sa" in the place's field, what it found shown.
    fn search(&mut self) -> anyhow::Result<Value> {
        self.click("weather:field")?;
        self.keys(&[KEY_S, KEY_A])?;
        self.shows(|s| s["weather"]["found"].as_array().is_some_and(|f| f.len() == 3))
    }
}

/// `id`'s row as shown.
fn row(shown: &Value, id: &str) -> Value {
    shown["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn a_place_typed_is_found_and_chosen_with_where_it_is() -> anyhow::Result<()> {
    let mut bed = Bed::start(FILE, "fr_FR.UTF-8")?;
    let shown = bed.open_on("weather")?;
    assert_eq!(shown["weather"]["place"], "");
    assert_eq!(row(&shown, "weather.enabled")["value"], "oui");
    assert_eq!(row(&shown, "weather.units")["value"], "°C · mm");
    let shown = bed.search()?;
    assert_eq!(
        shown["weather"]["found"][0],
        "Saint-Étienne (Loire, France)"
    );
    bed.click("weather:pick:0")?;
    let text = bed.written(|t| t.contains("[weather]"))?;
    assert!(text.starts_with("# Mine.\n"), "{text}");
    assert!(
        text.contains("place = \"Saint-Étienne\"\nlatitude = 45.4339\nlongitude = 4.3900"),
        "{text}"
    );
    bed.shows(|s| s["weather"]["place"] == "Saint-Étienne")?;
    Ok(())
}

#[test]
fn the_rains_when_waits_for_a_place_then_writes_its_key() -> anyhow::Result<()> {
    // No place: inert, saying where to choose one.
    let mut bed = Bed::start("# Mine.\n[effects]\nrain-on-glass = true\n", "fr_FR.UTF-8")?;
    let shown = bed.open_on("appearance")?;
    let when = row(&shown, "effects.rain-when");
    assert_eq!(when["inert"], true, "{when}");
    assert_eq!(when["value"], "Toujours");
    drop(bed);
    // A place: "Seulement quand il pleut" written.
    let mut bed = Bed::start(PLACED, "fr_FR.UTF-8")?;
    let shown = bed.open_on("appearance")?;
    assert_eq!(row(&shown, "effects.rain-when")["inert"], false);
    bed.click("segment:effects.rain-when:1")?;
    let text = bed.written(|t| t.contains("rain-when = \"raining\""))?;
    assert!(text.contains("rain-on-glass = true"), "{text}");
    Ok(())
}

/// The window, cropped, saved as `name` in `out` once it is still.
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

/// The pull request's screenshots, to `$HOCUS_SHOTS` when it names a
/// folder; run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut bed = Bed::start(FILE, "fr_FR.UTF-8")?;
    bed.open_on("weather")?;
    bed.search()?;
    bed.hp.pointer_move(1900.0, 1060.0);
    shoot(&mut bed, &out, "settings-weather-search")?;
    bed.click("weather:pick:0")?;
    bed.shows(|s| s["weather"]["place"] == "Saint-Étienne")?;
    bed.hp.pointer_move(1900.0, 1060.0);
    shoot(&mut bed, &out, "settings-weather")?;
    drop(bed);
    for (file, name) in [
        ("# Mine.\n[effects]\nrain-on-glass = true\n", "settings-effects-no-place"),
        (PLACED, "settings-effects-rain-when"),
    ] {
        let mut bed = Bed::start(file, "fr_FR.UTF-8")?;
        bed.open_on("appearance")?;
        bed.at("row:effects.rain-when")?;
        bed.hp.pointer_move(1900.0, 1060.0);
        shoot(&mut bed, &out, name)?;
    }
    Ok(())
}
