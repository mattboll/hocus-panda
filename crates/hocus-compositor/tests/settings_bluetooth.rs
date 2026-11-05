//! The Settings' Bluetooth page (6.7, issue #407): the real
//! `hocus-settings` window, a client of a headless compositor, against a
//! BlueZ faked on a private bus (`levels/bluez.rs`, as the bar's tests),
//! never the machine's. Its configuration, state, home and runtime
//! directories are the test's own; no session bus, no PipeWire; it draws
//! with tiny-skia.
//!
//! One test per point of the approved spec: the paired devices (connected
//! first, their batteries, the low one in orange, the input device that
//! comes back by itself) and those around (named ones only, the strongest
//! first, a code to check for a phone, a tablet or a computer); a device
//! connected and disconnected from its row, one that does not answer
//! saying so; a device paired without a code reaching the paired devices,
//! connected; the search and the computer visible while the page shows,
//! stopped and hidden as it is left or the window closes; off, blocked and
//! without an adapter, and the switch's toast with "Annuler"; the
//! keyboard; the home's card and the search; at rest, nothing drawn. What
//! the window shows is read from its log, never on a time.

#[path = "levels/bluez.rs"]
mod bluez;
mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use bluez::{Answer, Bluez, Call, FakeDevice, Setup};
use common::{BTN_LEFT, KEY_DOWN, KEY_TAB, WAIT, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;

const SONY: &str = "Casque WH-1000XM4";
const MX: &str = "MX Master 3S";
const PIXEL: &str = "Pixel 8";
const K380: &str = "Clavier K380";
const BUDS: &str = "Galaxy Buds2";
const MACBOOK: &str = "MacBook de Léa";
const FLIP: &str = "Flip 6";
const TAB: &str = "Galaxy Tab S9";

/// The mockup's paired devices: the mouse and the headphones connected
/// (the mouse last), the phone and the keyboard not.
fn mine() -> Vec<FakeDevice> {
    vec![
        FakeDevice::paired(SONY, "audio-headphones", Some(82)).connected(),
        FakeDevice::paired(MX, "input-mouse", Some(12)).connected(),
        FakeDevice::paired(PIXEL, "phone", None),
        FakeDevice::paired(K380, "input-keyboard", Some(64)),
    ]
}

/// The mockup's devices around, found once the search starts, and one
/// that never said its name.
fn around() -> Vec<FakeDevice> {
    vec![
        FakeDevice::around(FLIP, "audio-card", -65),
        FakeDevice::around(BUDS, "audio-headphones", -50).appearing(0x0941),
        FakeDevice::around(TAB, "computer", -70).appearing(0x0087),
        FakeDevice::around(MACBOOK, "computer", -58),
        FakeDevice {
            name: None,
            ..FakeDevice::around("unnamed", "", -40)
        },
    ]
}

fn setup() -> Setup {
    Setup {
        devices: mine(),
        later: around(),
        ..Setup::default()
    }
}

struct Bed {
    hp: Headless,
    bluez: Option<Bluez>,
    log: PathBuf,
    script: PathBuf,
    // Dropped after BlueZ: its connection goes before the bus.
    _fakes: Fakes,
    _dir: tempfile::TempDir,
}

impl Bed {
    /// The compositor, a bus of its own with BlueZ as `setup` says (none:
    /// not running), and the Settings' script; `None` when the Settings
    /// or `dbus-daemon` are not there.
    fn start(setup: Option<Setup>) -> anyhow::Result<Option<Self>> {
        Self::start_on(setup, "1920x1080", 2)
    }

    /// The same on a screen `output` (`WxH[@SCALE]`), the search's first
    /// pass, a connection and a pairing given `timeout` seconds at most.
    fn start_on(setup: Option<Setup>, output: &str, timeout: u32) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let bus = Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !bus {
            eprintln!("dbus-daemon missing: skipped");
            return Ok(None);
        }
        let mut fakes = Fakes::empty()?;
        fakes.start_bus()?;
        let bluez = setup
            .map(|setup| Bluez::start(fakes.runtime(), &fakes.address, setup))
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
                 export DBUS_SYSTEM_BUS_ADDRESS='{}' HOCUS_BLUETOOTH_TIMEOUT={timeout} \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug,hocus_bluetooth=debug\n\
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
            bluez,
            log,
            script,
            _fakes: fakes,
            _dir: dir,
        }))
    }

    fn bluez(&self) -> &Bluez {
        self.bluez.as_ref().expect("BlueZ")
    }

    /// The Settings launched on `section`, their window open with the
    /// keyboard, and BlueZ heard.
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

    /// The Settings on Bluetooth, BlueZ heard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.open_on("bluetooth")?;
        self.shows(|s| s["bluetooth"]["heard"] == true)
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
                        || l.contains("hocus_bluetooth")
                })
                .collect();
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                trouble.join("\n")
            )
        })
    }

    /// Waits until BlueZ was asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let done = {
            let bluez = self.bluez.as_ref().expect("BlueZ");
            self.hp
                .dispatch_until(WAIT, |_| bluez.calls().contains(call))?
        };
        anyhow::ensure!(
            done,
            "BlueZ was never asked {call:?}: {:?}",
            self.bluez().calls()
        );
        Ok(())
    }

    fn times(&self, call: &Call) -> usize {
        self.bluez().calls().iter().filter(|c| *c == call).count()
    }

    /// Waits until BlueZ's state says `done`.
    fn bluez_is(&mut self, what: &str, done: impl Fn(&Bluez) -> bool) -> anyhow::Result<()> {
        let held = {
            let bluez = self.bluez.as_ref().expect("BlueZ");
            self.hp.dispatch_until(WAIT, |_| done(bluez))?
        };
        anyhow::ensure!(held, "BlueZ never said {what}: {:?}", self.bluez().calls());
        Ok(())
    }

    /// The window's places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = common::last_json(&log, " places {").filter(|p| p.get(id).is_some());
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            anyhow::anyhow!(
                "{id} is nowhere: {:?}",
                common::last_json(&log, " places {")
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

    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    /// `id` scrolled into the window with the wheel over the page.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        for _ in 0..40 {
            let [_, y, _, h] = self.place_of(id)?;
            let window = self.window()?;
            let notches = if y + h > f64::from(window.height) - 90.0 {
                2
            } else if y < 56.0 {
                -2
            } else {
                return Ok(());
            };
            self.hp.pointer_move(
                f64::from(window.x) + 500.0,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            self.hp.wheel(notches);
            let log = self.log.clone();
            let key = id.to_owned();
            let before = [y, h];
            let moved = self.hp.dispatch_until(Duration::from_secs(2), |_| {
                read_place(&log, &key).is_some_and(|p| [p[1], p[3]] != before)
            })?;
            if !moved {
                return Ok(());
            }
        }
        anyhow::bail!("{id} never came into view")
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("toast-") {
            self.reveal(id)?;
        }
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

    /// The pointer away from the window.
    fn away(&mut self) -> anyhow::Result<()> {
        let window = self.window()?;
        self.hp.pointer_move(
            f64::from(window.x + window.width) + 40.0,
            f64::from(window.y) + 10.0,
        );
        wait(&mut self.hp, Duration::from_millis(30))
    }

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }
}

fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let place = places.get(id)?;
    let n = |i: usize| place[i].as_f64();
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

/// A row of the page's list `list` (`mine`, `nearby`), by its name.
fn device<'a>(shown: &'a Value, list: &str, name: &str) -> &'a Value {
    shown["bluetooth"][list]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["name"] == name))
        .unwrap_or(&Value::Null)
}

fn names(shown: &Value, list: &str) -> Vec<String> {
    shown["bluetooth"][list]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_bluetooth_page_lists_paired_devices_and_those_around() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    // The search found them all; the one that never said its name is
    // left out.
    let shown = bed.shows(|s| names(s, "nearby").len() == 4)?;
    // Connected first, the last connected first; then by name.
    assert_eq!(names(&shown, "mine"), [MX, SONY, K380, PIXEL]);
    let mx = device(&shown, "mine", MX);
    assert_eq!(mx["line"], "Connectée · batterie\u{a0}12\u{a0}%");
    assert_eq!(mx["low"], true);
    assert_eq!(mx["button"], "Déconnecter");
    let sony = device(&shown, "mine", SONY);
    assert_eq!(sony["line"], "Connecté · batterie\u{a0}82\u{a0}%");
    assert_eq!(sony["low"], false);
    // A keyboard trusted comes back by itself; a phone does not say it.
    assert_eq!(
        device(&shown, "mine", K380)["line"],
        "Pas connecté · se connecte seul quand il s’allume"
    );
    assert_eq!(device(&shown, "mine", PIXEL)["line"], "Pas connecté");
    assert_eq!(device(&shown, "mine", PIXEL)["button"], "Connecter");
    // The strongest first; a code to check for a computer or a tablet.
    assert_eq!(names(&shown, "nearby"), [BUDS, MACBOOK, FLIP, TAB]);
    assert_eq!(device(&shown, "nearby", BUDS)["line"], "Écouteurs");
    assert_eq!(
        device(&shown, "nearby", MACBOOK)["line"],
        "Ordinateur · un code sera à vérifier"
    );
    assert_eq!(device(&shown, "nearby", FLIP)["line"], "Enceinte");
    assert_eq!(
        device(&shown, "nearby", TAB)["line"],
        "Tablette · un code sera à vérifier"
    );
    assert_eq!(shown["bluetooth"]["powered"], true);
    // Drawn: the switch's card, both groups and their rows.
    for id in [
        "row:bluetooth.power",
        "bt:switch",
        "group:bluetooth.mine",
        "group:bluetooth.nearby",
        "group:bluetooth.later",
        "bt:act:Pixel 8",
        "bt:pair:Flip 6",
    ] {
        bed.places_with(id)?;
    }
    Ok(())
}

#[test]
fn a_paired_device_connects_one_that_does_not_answer_says_so_and_disconnect() -> anyhow::Result<()>
{
    let mut devices = mine();
    devices[2] = FakeDevice::paired(PIXEL, "phone", None).answering(Answer::Refuses);
    let Some(mut bed) = Bed::start(Some(Setup {
        devices,
        ..Setup::default()
    }))?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "mine").len() == 4)?;
    // The keyboard connects from its row: "Connexion…", then connected,
    // the last connected first.
    bed.click(&format!("bt:act:{K380}"))?;
    bed.asked(&Call::Connect(K380.to_owned()))?;
    bed.shows(|s| device(s, "mine", K380)["line"] == "Connexion…")?;
    let shown = bed.shows(|s| device(s, "mine", K380)["connected"] == true)?;
    assert_eq!(names(&shown, "mine")[0], K380);
    assert_eq!(
        device(&shown, "mine", K380)["line"],
        "Connecté · batterie\u{a0}64\u{a0}%"
    );
    // The phone does not answer: said under its row.
    bed.click(&format!("bt:act:{PIXEL}"))?;
    bed.asked(&Call::Connect(PIXEL.to_owned()))?;
    let shown = bed.shows(|s| !device(s, "mine", PIXEL)["failure"].is_null())?;
    assert_eq!(
        device(&shown, "mine", PIXEL)["failure"],
        "Il ne répond pas. Vérifie qu’il est allumé et à portée."
    );
    // Disconnect.
    bed.click(&format!("bt:act:{SONY}"))?;
    bed.asked(&Call::Disconnect(SONY.to_owned()))?;
    bed.shows(|s| {
        device(s, "mine", SONY)["connected"] == false
            && device(s, "mine", SONY)["button"] == "Connecter"
    })?;
    assert!(!bed.bluez().connected().contains(&SONY.to_owned()));
    Ok(())
}

#[test]
fn pairing_without_a_code_moves_the_device_to_my_devices_connected() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "nearby").len() == 4)?;
    bed.click(&format!("bt:pair:{BUDS}"))?;
    bed.asked(&Call::Pair(BUDS.to_owned()))?;
    // Pairing: its row says so, the others are inert.
    let shown = bed.shows(|s| device(s, "nearby", BUDS)["line"] == "Association…")?;
    assert_eq!(device(&shown, "nearby", FLIP)["inert"], true);
    // Then trusted, connected, and among the paired devices, first.
    bed.asked(&Call::Trust(BUDS.to_owned()))?;
    bed.asked(&Call::Connect(BUDS.to_owned()))?;
    let shown = bed.shows(|s| device(s, "mine", BUDS)["connected"] == true)?;
    assert_eq!(names(&shown, "mine")[0], BUDS);
    assert!(!names(&shown, "nearby").contains(&BUDS.to_owned()));
    assert_eq!(bed.bluez().paired(BUDS), (true, true));
    Ok(())
}

#[test]
fn leaving_the_page_stops_the_search_and_hides_the_computer() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    // Shown: the search runs and the computer is visible, pairable.
    bed.asked(&Call::StartDiscovery)?;
    bed.asked(&Call::Discoverable(true))?;
    bed.asked(&Call::Pairable(true))?;
    bed.bluez_is("visible", |b| b.discoverable() && b.discovering())?;
    let shown = bed.shows(|s| s["bluetooth"]["visible"] == true)?;
    assert_eq!(shown["bluetooth"]["powered"], true);
    // BlueZ's timeout hides it: visible again while the page shows.
    bed.bluez().discoverable_times_out()?;
    bed.bluez_is("visible again", |b| {
        b.calls()
            .iter()
            .filter(|c| **c == Call::Discoverable(true))
            .count()
            == 2
    })?;
    bed.bluez_is("visible", Bluez::discoverable)?;
    // The home: the search stops, the computer is hidden.
    bed.click("nav:home")?;
    bed.shows(|s| s["page"] == "home")?;
    bed.asked(&Call::StopDiscovery)?;
    bed.asked(&Call::Discoverable(false))?;
    bed.bluez_is("hidden", |b| !b.discoverable() && !b.discovering())?;
    // Back on the page, then the window closed: hidden again before the
    // program goes.
    bed.click("card:bluetooth")?;
    bed.shows(|s| s["page"] == "bluetooth")?;
    bed.bluez_is("visible", |b| b.discoverable() && b.discovering())?;
    let stops = bed.times(&Call::StopDiscovery);
    bed.hp.key(common::KEY_ESC, true);
    bed.hp.key(common::KEY_ESC, false);
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID))
    })?;
    anyhow::ensure!(gone, "the window never closed");
    bed.bluez_is("hidden", |b| !b.discoverable() && !b.discovering())?;
    assert_eq!(bed.times(&Call::StopDiscovery), stops + 1);
    Ok(())
}

#[test]
fn off_blocked_and_without_adapter_say_so() -> anyhow::Result<()> {
    // On, then turned off with the switch: the toast, "Annuler".
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bluetooth"]["powered"] == true)?;
    bed.click("bt:switch")?;
    bed.asked(&Call::Power(false))?;
    let shown = bed.shows(|s| s["bluetooth"]["powered"] == false)?;
    assert_eq!(shown["toast"], "Bluetooth coupé");
    assert_eq!(shown["toast_action"], "undo");
    // Off: the paired devices stay, the devices around are not drawn.
    assert_eq!(names(&shown, "mine").len(), 4);
    assert!(shown["bluetooth"]["nearby"].is_null());
    assert_eq!(shown["bluetooth"]["blocked"], false);
    bed.away()?;
    bed.click("toast-action")?;
    bed.asked(&Call::Power(true))?;
    let shown = bed.shows(|s| s["bluetooth"]["powered"] == true)?;
    assert_eq!(shown["toast"], "Annulé : le Bluetooth est rallumé.");
    drop(bed);

    // Blocked by rfkill: said; the switch asks, BlueZ refuses, it stays
    // off.
    let Some(mut bed) = Bed::start(Some(Setup {
        blocked: true,
        devices: mine(),
        ..Setup::default()
    }))?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["bluetooth"]["blocked"] == true)?;
    assert_eq!(shown["bluetooth"]["powered"], false);
    bed.click("bt:switch")?;
    bed.asked(&Call::Power(true))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let shown = bed.shows(|s| s["bluetooth"]["heard"] == true)?;
    assert_eq!(shown["bluetooth"]["powered"], false);
    assert!(shown["toast"].is_null(), "{shown}");
    drop(bed);

    // Without an adapter: the info box, nothing else; then BlueZ not
    // running at all, the same.
    for setup in [
        Some(Setup {
            adapter: false,
            ..Setup::default()
        }),
        None,
    ] {
        let Some(mut bed) = Bed::start(setup)? else {
            return Ok(());
        };
        bed.open()?;
        let shown = bed.shows(|s| s["bluetooth"]["adapter"] == false)?;
        assert!(
            shown["bluetooth"]["mine"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        bed.places_with("bt:no-adapter")?;
        let places = common::last_json(&bed.log, " places {").unwrap_or_default();
        assert!(places.get("bt:switch").is_none(), "{places}");
    }
    Ok(())
}

#[test]
fn the_keyboard_goes_through_the_lists() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| names(s, "nearby").len() == 4)?;
    // Tab past the search and the sidebar's rows to the switch, then the
    // paired devices' list: one stop.
    let mut focus = shown["focus"].clone();
    for _ in 0..30 {
        bed.key(KEY_TAB)?;
        focus = bed.shows(|s| s["focus"] != focus)?["focus"].clone();
        if focus == "control:1" {
            break;
        }
    }
    anyhow::ensure!(focus == "control:1", "{focus}");
    // ↓ twice: the keyboard (third), Enter connects it.
    bed.key(KEY_DOWN)?;
    bed.key(KEY_DOWN)?;
    bed.shows(|s| s["bluetooth"]["cursor"][0] == 2)?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Connect(K380.to_owned()))?;
    bed.shows(|s| device(s, "mine", K380)["connected"] == true)?;
    // The next stop: the devices around; Enter pairs the first.
    bed.key(KEY_TAB)?;
    bed.shows(|s| s["focus"] == "control:2")?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Pair(BUDS.to_owned()))?;
    Ok(())
}

#[test]
fn the_home_card_and_the_search_lead_to_the_page() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open_on("home")?;
    let shown = bed.shows(|s| {
        s["cards"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c[1] == "2 appareils connectés"))
    })?;
    assert_eq!(shown["page"], "home");
    // Not shown: no search, not visible.
    assert!(!bed.bluez().discovering() && !bed.bluez().discoverable());
    // The search finds "jumeler": Associer un appareil, on the page.
    for c in ["j", "u", "m", "e", "l", "e", "r"] {
        let code = match c {
            "j" => 36,
            "u" => 22,
            "m" => 50,
            "e" => 18,
            "l" => 38,
            _ => 19,
        };
        bed.key(code)?;
    }
    let shown = bed.shows(|s| {
        s["results"]
            .as_array()
            .is_some_and(|r| r.iter().any(|r| r["target"] == "bluetooth.pair"))
    })?;
    assert_eq!(shown["results"][0]["name"], "Associer un appareil");
    bed.key(KEY_ENTER)?;
    let shown = bed.shows(|s| s["page"] == "bluetooth" && s["lit"] == "bluetooth.pair")?;
    assert_eq!(shown["bluetooth"]["heard"], true);
    bed.asked(&Call::StartDiscovery)?;
    Ok(())
}

#[test]
fn at_rest_the_page_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    // The search's first pass over: its spinner gone.
    bed.shows(|s| names(s, "nearby").len() == 4 && s["bluetooth"]["searching"] == false)?;
    bed.away()?;
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

/// The pull request's pictures, at ×2: `HOCUS_SHOTS=DIR`.
#[test]
#[ignore = "pictures for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        bed.away()?;
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
    // The mockup's state: two connected, the mouse's battery low, the
    // search under way.
    let Some(mut bed) = Bed::start_on(Some(setup()), "2880x1800@2", 20)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "nearby").len() == 4)?;
    shoot(&mut bed, "bluetooth.png")?;
    // A device around pairing, the others inert.
    bed.bluez().set_takes(Duration::from_secs(30));
    bed.click(&format!("bt:pair:{FLIP}"))?;
    bed.shows(|s| device(s, "nearby", FLIP)["line"] == "Association…")?;
    shoot(&mut bed, "bluetooth-pairing.png")?;
    drop(bed);
    // A phone that does not answer.
    let mut devices = mine();
    devices[2] = FakeDevice::paired(PIXEL, "phone", None).answering(Answer::Refuses);
    let Some(mut bed) = Bed::start_on(
        Some(Setup {
            devices,
            ..Setup::default()
        }),
        "2880x1800@2",
        2,
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bluetooth"]["searching"] == false)?;
    bed.click(&format!("bt:act:{PIXEL}"))?;
    bed.shows(|s| !device(s, "mine", PIXEL)["failure"].is_null())?;
    shoot(&mut bed, "bluetooth-no-answer.png")?;
    // Turned off: the paired devices at 45 %, the toast.
    bed.click("bt:switch")?;
    bed.shows(|s| s["bluetooth"]["powered"] == false && s["toast"] == "Bluetooth coupé")?;
    shoot(&mut bed, "bluetooth-off.png")?;
    drop(bed);
    // Blocked by rfkill.
    let Some(mut bed) = Bed::start_on(
        Some(Setup {
            blocked: true,
            devices: mine(),
            ..Setup::default()
        }),
        "2880x1800@2",
        2,
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bluetooth"]["blocked"] == true)?;
    shoot(&mut bed, "bluetooth-blocked.png")?;
    drop(bed);
    // No adapter.
    let Some(mut bed) = Bed::start_on(
        Some(Setup {
            adapter: false,
            ..Setup::default()
        }),
        "2880x1800@2",
        2,
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bluetooth"]["adapter"] == false)?;
    shoot(&mut bed, "bluetooth-no-adapter.png")?;
    Ok(())
}
