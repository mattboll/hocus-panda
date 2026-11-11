//! The Settings' Date et heure page (6.11, issue #411): the real
//! `hocus-settings` window, a client of a headless compositor, against a
//! timedated faked on a private bus (`levels/timedate.rs`), never the
//! machine's. Its configuration, state, home and runtime directories are
//! the test's own; no session bus, no PipeWire; it draws with tiny-skia.
//!
//! One test per point of the approved spec: the clock, the zone and the
//! automatic time as timedated says them (right, not right yet, no time
//! service); the automatic time turned off, and the time set by hand; the
//! zone picker finding Montréal by "mon", setting it, "Annuler" taking it
//! back; polkit's refusal leaving every value as it was; the format's
//! link and the search; at rest, a draw once a minute at most. What the
//! window shows is read from its log, never on a time.

use crate::common;
use crate::fakes;
use crate::settings_page;
use crate::timedate;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{BTN_LEFT, KEY_DOWN, KEY_LEFTCTRL, KEY_LEFTSHIFT, WAIT, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;
use timedate::{Call, State, TimeDate};

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;
const KEY_A: u32 = 30;
const KEY_SEMICOLON: u32 = 39;

/// The keys of a US keyboard that type `c`.
fn key_of(c: char) -> Option<u32> {
    Some(match c {
        '1'..='9' => 2 + (c as u32 - '1' as u32),
        '0' => 11,
        'a' => 30,
        'e' => 18,
        'f' => 33,
        'm' => 50,
        'n' => 49,
        'o' => 24,
        's' => 31,
        'u' => 22,
        '/' => 53,
        _ => return None,
    })
}

struct Bed {
    hp: Headless,
    timedate: Option<TimeDate>,
    log: PathBuf,
    script: PathBuf,
    // Dropped after timedated: its connection goes before the bus.
    fakes: Fakes,
    _dir: tempfile::TempDir,
}

impl Bed {
    /// The compositor, a bus of its own with timedated holding `state`
    /// (none: not there), and the Settings' script; `None` when the
    /// Settings or `dbus-daemon` are not there.
    fn start(state: Option<State>) -> anyhow::Result<Option<Self>> {
        Self::start_on(state, "1920x1080")
    }

    fn start_on(state: Option<State>, output: &str) -> anyhow::Result<Option<Self>> {
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
        let timedate = state
            .map(|state| TimeDate::start(fakes.runtime(), &fakes.address, state))
            .transpose()?;
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
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
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
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
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.appearance.reduced_motion = true;
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec![output.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        Ok(Some(Self {
            hp,
            timedate,
            log,
            script,
            fakes,
            _dir: dir,
        }))
    }

    fn timedate(&self) -> &TimeDate {
        self.timedate.as_ref().expect("timedated")
    }

    /// The Settings launched on `section`, their window open with the
    /// keyboard.
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
        self.shows(|s| s["file"] == "saved" && s["page"] == section)
    }

    /// The Settings on Date et heure, timedated heard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.open_on("date-and-time")?;
        self.shows(|s| s["time"]["heard"] == true)
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
            let said = common::logged(&log);
            let trouble: Vec<&str> = said
                .lines()
                .filter(|l| {
                    l.contains("WARN")
                        || l.contains("ERROR")
                        || l.contains("panicked")
                        || l.contains("timedate")
                })
                .collect();
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                trouble.join("\n")
            )
        })
    }

    /// Waits until timedated was asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let done = {
            let td = self.timedate.as_ref().expect("timedated");
            self.hp
                .dispatch_until(WAIT, |_| td.calls().contains(call))?
        };
        anyhow::ensure!(
            done,
            "timedated was never asked {call:?}: {:?}",
            self.timedate().calls()
        );
        Ok(())
    }

    /// The window's current places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        settings_page::places_with(&mut self.hp, &self.log, id)
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        let [x, y, w, h] = self.place_of(id)?;
        let window = self.window()?;
        let at = (
            f64::from(window.x) + x + w / 2.0,
            f64::from(window.y) + y + h / 2.0,
        );
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    /// `text` typed on the keyboard (a US one), `:` with Shift.
    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            if c == ':' {
                self.hp.key(KEY_LEFTSHIFT, true);
                self.key(KEY_SEMICOLON)?;
                self.hp.key(KEY_LEFTSHIFT, false);
                continue;
            }
            let code = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            self.key(code)?;
        }
        Ok(())
    }

    /// The field `id` holding the keyboard, its text all chosen.
    fn fill_field(&mut self, id: &str) -> anyhow::Result<()> {
        self.click(id)?;
        wait(&mut self.hp, Duration::from_millis(50))?;
        self.hp.key(KEY_LEFTCTRL, true);
        self.key(KEY_A)?;
        self.hp.key(KEY_LEFTCTRL, false);
        Ok(())
    }
}

/// "UTC+01:00" for `zone` now, as the page says it.
fn offset_now(zone: &str) -> String {
    let tz = jiff::tz::TimeZone::get(zone).expect("tzdata");
    let seconds = tz.to_offset(jiff::Timestamp::now()).seconds();
    let sign = if seconds < 0 { '−' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

#[test]
fn the_page_shows_the_clock_the_zone_and_the_automatic_time() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    // Read before the page is: its clock then says this hour, or the next
    // when the hour changes meanwhile.
    let there = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::get("Europe/Paris")?);
    let shown = bed.open()?;
    let paris = offset_now("Europe/Paris");
    let time = &shown["time"];
    assert_eq!(time["zone"], "Europe/Paris");
    let line = time["clock_line"].as_str().unwrap_or_default();
    assert!(
        line.starts_with(&format!("Europe/Paris · {paris}, heure d’"))
            && line.contains(" jusqu’au "),
        "{line}"
    );
    // The clock in Paris's time, whatever the process's own zone (UTC).
    let clock = time["clock"].as_str().unwrap_or_default();
    assert_eq!(clock.len(), 5, "{clock}");
    assert!(
        [there.hour(), (there.hour() + 1) % 24].contains(&clock[..2].parse::<i8>()?),
        "{clock} in Paris at {there}"
    );
    assert_eq!(time["ntp"], true);
    assert_eq!(time["ntp_line"], "Par Internet (NTP) · à l’heure");
    assert_eq!(time["zone_line"], format!("Paris, France · {paris}"));
    assert_eq!(time["fields"]["editable"], false);
    let format = time["format_line"].as_str().unwrap_or_default();
    assert!(
        format.starts_with("24 h · « ")
            && format.ends_with("la barre, l’agenda, l’écran verrouillé."),
        "{format}"
    );
    // Nothing is written in the file.
    bed.places_with("row:time.zone")?;
    assert_eq!(bed.timedate().calls(), []);
    Ok(())
}

#[test]
fn not_on_time_yet_and_no_time_service_are_said() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State {
        synced: false,
        ..State::default()
    }))?
    else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(
        shown["time"]["ntp_line"],
        "Par Internet (NTP) · pas encore à l’heure"
    );
    drop(bed);
    let Some(mut bed) = Bed::start(Some(State {
        ntp: false,
        can_ntp: false,
        ..State::default()
    }))?
    else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(
        shown["time"]["ntp_line"],
        "Aucun service de mise à l’heure sur cet ordinateur."
    );
    assert_eq!(shown["time"]["can_ntp"], false);
    // The switch is inert: a click asks nothing.
    bed.click("control:time.auto")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.timedate().calls(), []);
    Ok(())
}

#[test]
fn automatic_time_off_lets_the_time_be_set() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:time.auto")?;
    bed.asked(&Call::Ntp(false))?;
    let shown =
        bed.shows(|s| s["time"]["ntp"] == false && s["time"]["fields"]["editable"] == true)?;
    assert_eq!(
        shown["time"]["ntp_line"],
        "Coupé : l’heure ne se corrige plus seule."
    );
    assert!(!bed.timedate().now().ntp);
    let date = shown["time"]["fields"]["date"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    bed.fill_field("box:time-time")?;
    bed.type_text("16:45")?;
    bed.shows(|s| s["time"]["fields"]["time"] == "16:45")?;
    bed.click("time-set")?;
    // The day shown, at 16:45 in Paris.
    let day = jiff::civil::Date::strptime("%d/%m/%Y", &date)?;
    let usec = day
        .at(16, 45, 0, 0)
        .to_zoned(jiff::tz::TimeZone::get("Europe/Paris")?)?
        .timestamp()
        .as_microsecond();
    bed.asked(&Call::Time(usec))?;
    let shown = bed.shows(|s| s["toast"] == "Heure réglée : 16:45.")?;
    assert!(shown["toast_action"].is_null(), "no Annuler: {shown}");
    // The fields follow the clock again.
    assert_eq!(shown["time"]["fields"]["typed"], false);
    Ok(())
}

#[test]
fn the_time_zone_picker_finds_montreal_by_mon_and_sets_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:time.zone")?;
    let shown = bed.shows(|s| {
        s["time"]["picker"]["rows"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    })?;
    // Nothing typed: the zone first.
    assert_eq!(shown["time"]["picker"]["rows"][0]["city"], "Paris, France");
    // The search holds the keyboard.
    bed.type_text("mon")?;
    let shown = bed.shows(|s| s["time"]["picker"]["query"] == "mon")?;
    let first = &shown["time"]["picker"]["rows"][0];
    let toronto = offset_now("America/Toronto");
    assert_eq!(first["city"], "Montréal, Canada");
    assert_eq!(first["marked"], "Mon");
    assert_eq!(first["line"], format!("America/Toronto · {toronto}"));
    let cities: Vec<&str> = shown["time"]["picker"]["rows"]
        .as_array()
        .map(|r| r.iter().filter_map(|c| c["city"].as_str()).collect())
        .unwrap_or_default();
    assert!(cities.contains(&"Monaco"), "{cities:?}");
    assert!(cities.contains(&"Montevideo, Uruguay"), "{cities:?}");
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Zone("America/Toronto".to_owned()))?;
    let shown = bed.shows(|s| {
        s["time"]["zone"] == "America/Toronto"
            && s["toast"].as_str().is_some_and(|t| t.starts_with("Fuseau"))
    })?;
    assert_eq!(
        shown["toast"],
        format!("Fuseau horaire : Montréal ({toronto}).")
    );
    assert_eq!(shown["toast_action"], "undo");
    assert!(shown["time"]["picker"].is_null());
    assert_eq!(
        shown["time"]["zone_line"],
        format!("Toronto, Canada · {toronto}")
    );
    // "Annuler" takes it back.
    bed.click("toast-action")?;
    bed.asked(&Call::Zone("Europe/Paris".to_owned()))?;
    let shown = bed.shows(|s| {
        s["time"]["zone"] == "Europe/Paris"
            && s["toast"].as_str().is_some_and(|t| t.starts_with("Annulé"))
    })?;
    assert_eq!(
        shown["toast"],
        format!(
            "Annulé : le fuseau horaire revient à Paris ({}).",
            offset_now("Europe/Paris")
        )
    );
    assert_eq!(bed.timedate().now().zone, "Europe/Paris");
    Ok(())
}

#[test]
fn the_picker_moves_with_the_arrows_and_closes_with_esc() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:time.zone")?;
    bed.shows(|s| s["time"]["picker"]["selected"] == 0)?;
    bed.key(KEY_DOWN)?;
    bed.shows(|s| s["time"]["picker"]["selected"] == 1)?;
    bed.key(common::KEY_ESC)?;
    bed.shows(|s| s["time"]["picker"].is_null() && s["page"] == "date-and-time")?;
    assert_eq!(bed.timedate().calls(), []);
    Ok(())
}

#[test]
fn a_refusal_by_polkit_leaves_the_values_unchanged() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.timedate().refuse(true);
    bed.click("control:time.auto")?;
    bed.asked(&Call::Ntp(false))?;
    let shown = bed.shows(|s| s["toast"] == "Le système a refusé.")?;
    assert_eq!(shown["time"]["ntp"], true);
    assert_eq!(shown["time"]["fields"]["editable"], false);
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.click("control:time.zone")?;
    bed.shows(|s| {
        s["time"]["picker"]["rows"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    })?;
    bed.type_text("mon")?;
    bed.shows(|s| s["time"]["picker"]["query"] == "mon")?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Zone("America/Toronto".to_owned()))?;
    let shown = bed
        .shows(|s| s["toast"] == "Le système a refusé." && s["time"]["asked"]["zone"].is_null())?;
    assert_eq!(shown["time"]["zone"], "Europe/Paris");
    assert_eq!(shown["undo"], 0, "nothing to take back: {shown}");
    let now = bed.timedate().now();
    assert!(now.ntp);
    assert_eq!(now.zone, "Europe/Paris");
    Ok(())
}

#[test]
fn a_zone_changed_elsewhere_shows_at_once() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.timedate().set_zone(bed.fakes.runtime(), "Asia/Tokyo");
    let shown = bed.shows(|s| s["time"]["zone"] == "Asia/Tokyo")?;
    assert_eq!(
        shown["time"]["clock_line"],
        "Asia/Tokyo · UTC+09:00, pas de changement d’heure"
    );
    assert_eq!(shown["time"]["zone_line"], "Tokyo, Japon · UTC+09:00");
    Ok(())
}

#[test]
fn without_timedated_the_page_says_so() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(None)? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["time"]["service"], false);
    assert_eq!(
        shown["time"]["ntp_line"],
        "Le service de l’heure du système (timedated) ne répond pas."
    );
    assert_eq!(shown["time"]["fields"]["editable"], false);
    Ok(())
}

#[test]
fn the_format_link_and_the_search_light_their_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("link:time.format")?;
    bed.shows(|s| s["page"] == "appearance" && s["lit"] == "locale.time")?;
    // "fuseau" found from anywhere: its row, lit.
    bed.type_text("fuseau")?;
    bed.shows(|s| s["results"].as_array().is_some_and(|r| !r.is_empty()))?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["page"] == "date-and-time" && s["lit"] == "time.zone")?;
    Ok(())
}

#[test]
fn at_rest_the_page_draws_once_a_minute_only() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(State::default()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.places_with("time-clock")?;
    bed.hp.pointer_move(5.0, 5.0);
    // Still once the window settled: the clock draws on the minute, so a
    // quiet few seconds draw nothing but perhaps that minute.
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 30 {
            break;
        }
    }
    anyhow::ensure!(still >= 30, "the window never came to rest");
    let before = bed.hp.window_commits();
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert!(
        bed.hp.window_commits() <= before + 1,
        "the page drew at rest: {} commits",
        bed.hp.window_commits() - before
    );
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(Some(State::default()), "2880x1800@2")? else {
        return Ok(());
    };
    let save = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        bed.hp.pointer_move(5.0, 5.0);
        let mut last = bed.hp.window_commits();
        let mut still = 0;
        for _ in 0..40 {
            if still >= 5 {
                break;
            }
            wait(&mut bed.hp, Duration::from_millis(100))?;
            let now = bed.hp.window_commits();
            still = if now == last { still + 1 } else { 0 };
            last = now;
        }
        let window = bed.window()?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let scale = 2;
        let image = image::imageops::crop_imm(
            &image,
            (window.x.max(0) * scale) as u32,
            (window.y.max(0) * scale) as u32,
            (window.width * scale) as u32,
            (window.height * scale) as u32,
        )
        .to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    bed.open()?;
    save(&mut bed, "time-page.png")?;
    bed.click("control:time.zone")?;
    bed.shows(|s| {
        s["time"]["picker"]["rows"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    })?;
    save(&mut bed, "time-picker.png")?;
    bed.type_text("mon")?;
    bed.shows(|s| s["time"]["picker"]["query"] == "mon")?;
    save(&mut bed, "time-picker-mon.png")?;
    bed.key(KEY_ENTER)?;
    bed.shows(|s| s["time"]["zone"] == "America/Toronto" && s["toast"].is_string())?;
    save(&mut bed, "time-zone-set.png")?;
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.click("control:time.auto")?;
    bed.shows(|s| s["time"]["fields"]["editable"] == true)?;
    bed.fill_field("box:time-time")?;
    bed.type_text("16:45")?;
    bed.shows(|s| s["time"]["fields"]["time"] == "16:45")?;
    bed.click("time-set")?;
    bed.shows(|s| s["toast"] == "Heure réglée : 16:45.")?;
    save(&mut bed, "time-manual.png")?;
    Ok(())
}
