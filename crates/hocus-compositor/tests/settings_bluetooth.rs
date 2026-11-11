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

use crate::bluez;
use crate::common;
use crate::fakes;
use crate::settings_page;

use settings_page::{Floor, Wheel};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use bluez::{Answer, Bluez, Call, FakeDevice, Incoming, Pairing, Setup};
use common::{BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFTCTRL, KEY_TAB, WAIT, chord, wait};
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

/// The paired devices of an adapter that is off: none connected.
fn mine_off() -> Vec<FakeDevice> {
    mine()
        .into_iter()
        .map(|d| FakeDevice {
            connected: false,
            ..d
        })
        .collect()
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
    shell_log: PathBuf,
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
        Self::start_on(setup, "1920x1080", 2, false)
    }

    /// The same on a screen `output` (`WxH[@SCALE]`), the search's first
    /// pass, a connection and a pairing given `timeout` seconds at most;
    /// the shell too (`shell`), its Bluetooth on the same BlueZ, the
    /// Settings it opens this script.
    fn start_on(
        setup: Option<Setup>,
        output: &str,
        timeout: u32,
        shell: bool,
    ) -> anyhow::Result<Option<Self>> {
        let shell = shell.then(common::shell_program);
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
        let headless = HeadlessConfig {
            outputs: vec![output.parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        };
        let shell_log = root.join("shell.log");
        let hp = match shell {
            None => Headless::start_with(headless, config)?,
            Some(shell) => {
                let env = [
                    ("HOME", at("home")),
                    ("XDG_CONFIG_HOME", at("config")),
                    ("XDG_CACHE_HOME", at("cache")),
                    ("XDG_STATE_HOME", at("state")),
                    ("XDG_DATA_HOME", at("data")),
                    ("ICED_BACKEND", "tiny-skia".to_owned()),
                    ("PIPEWIRE_REMOTE", "/nonexistent".to_owned()),
                    ("PATH", format!("{}:/usr/bin:/bin", at("bin"))),
                    ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
                ];
                let script = fakes.shell_with(&shell, "fr_FR.UTF-8", &env, Some(&shell_log))?;
                let mut hp = Headless::start_with_shell(headless, config, Some(script))?;
                let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
                    hp.connections_surfaces().len() == 1 && hp.connections_commits() > 0
                })?;
                anyhow::ensure!(drawn, "no connections: {:?}", hp.connections_surfaces());
                hp
            }
        };
        Ok(Some(Self {
            hp,
            bluez,
            log,
            shell_log,
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

    /// `id` scrolled into the window with the wheel over the page.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 500.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
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

/// The seconds the window gives a connection or a pairing while the test
/// holds BlueZ's answer: as long as the test waits for what it shows.
const HELD: u32 = WAIT.as_secs() as u32;

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
    // The window gives a connection as long as the test may hold it.
    let setup = Setup {
        devices,
        ..Setup::default()
    };
    let Some(mut bed) = Bed::start_on(Some(setup), "1920x1080", HELD, false)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "mine").len() == 4)?;
    // The keyboard connects from its row: "Connexion…", then connected,
    // the last connected first. Its answer held until the window said
    // "Connexion…": a window slow to draw went straight to connected.
    bed.bluez().hold();
    bed.click(&format!("bt:act:{K380}"))?;
    bed.asked(&Call::Connect(K380.to_owned()))?;
    bed.shows(|s| device(s, "mine", K380)["line"] == "Connexion…")?;
    bed.bluez().release();
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
    // The window gives a pairing as long as the test may hold it.
    let Some(mut bed) = Bed::start_on(Some(setup()), "1920x1080", HELD, false)? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "nearby").len() == 4)?;
    // Its answer held until the window says it pairs.
    bed.bluez().hold();
    bed.click(&format!("bt:pair:{BUDS}"))?;
    bed.asked(&Call::Pair(BUDS.to_owned()))?;
    // Pairing: its row says so, the others are inert.
    let shown = bed.shows(|s| device(s, "nearby", BUDS)["line"] == "Association…")?;
    assert_eq!(device(&shown, "nearby", FLIP)["inert"], true);
    bed.bluez().release();
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
        devices: mine_off(),
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
    // The whole word: a window slow to read the keys says the results of
    // « ju » first, where the pairing comes third.
    let shown = bed.shows(|s| {
        s["query"] == "jumeler"
            && s["results"]
                .as_array()
                .is_some_and(|r| r.iter().any(|r| r["target"] == "bluetooth.pair"))
    })?;
    assert_eq!(
        shown["results"][0]["name"], "Associer un appareil",
        "{shown}"
    );
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

// --- Part 2: the menu, the sheets, the bar's hand-off ---------------------------

const PHONE: &str = "Pixel 7a";
const LEGACY: &str = "Vieille enceinte";
const KEY_F2: u32 = 60;
const KEY_DELETE: u32 = 111;
const KEY_Z: u32 = 44;

/// Devices around that pair with a code: a phone comparing one, a
/// keyboard typing one, a speaker asking a PIN.
fn with_codes() -> Setup {
    Setup {
        devices: mine(),
        later: vec![
            FakeDevice::around(PHONE, "phone", -50).pairing(Pairing::Confirm(482_913)),
            FakeDevice::around(K380_NEW, "input-keyboard", -55).pairing(Pairing::Display {
                passkey: 305_718,
                typed: 3,
            }),
            FakeDevice::around(LEGACY, "audio-card", -60).pairing(Pairing::Pin("0000")),
        ],
        ..Setup::default()
    }
}

const K380_NEW: &str = "Clavier K780";

/// The key typing `c` (lower-case letters and digits).
fn key_of(c: char) -> Option<u32> {
    let letters = "qwertyuiop";
    let middle = "asdfghjkl";
    let bottom = "zxcvbnm";
    if let Some(i) = letters.find(c) {
        return Some(16 + i as u32);
    }
    if let Some(i) = middle.find(c) {
        return Some(30 + i as u32);
    }
    if let Some(i) = bottom.find(c) {
        return Some(44 + i as u32);
    }
    match c {
        '0' => Some(11),
        '1'..='9' => Some(c as u32 - '1' as u32 + 2),
        ' ' => Some(57),
        _ => None,
    }
}

impl Bed {
    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let code = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c}"))?;
            self.key(code)?;
        }
        Ok(())
    }

    /// The page open, the devices around found.
    fn open_around(&mut self, count: usize) -> anyhow::Result<Value> {
        self.open()?;
        self.shows(|s| names(s, "nearby").len() == count)
    }

    /// A paired device's menu, then its item `i`.
    fn menu_item(&mut self, name: &str, i: usize) -> anyhow::Result<()> {
        self.click(&format!("bt:more:{name}"))?;
        self.shows(|s| s["bluetooth"]["menu"] == name)?;
        self.click(&format!("bt:item:{i}"))
    }

    fn shell_said(&self) -> String {
        common::logged(&self.shell_log)
    }
}

fn sheet(shown: &Value) -> &Value {
    &shown["bluetooth"]["sheet"]
}

#[test]
fn pairing_a_phone_shows_the_code_and_confirms_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    // The page's agent, BlueZ's default while it shows.
    bed.asked(&Call::RegisterAgent("KeyboardDisplay".to_owned()))?;
    bed.asked(&Call::DefaultAgent)?;
    bed.click(&format!("bt:pair:{PHONE}"))?;
    let shown = bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    assert_eq!(sheet(&shown)["code"], "482 913");
    assert_eq!(sheet(&shown)["name"], PHONE);
    assert_eq!(sheet(&shown)["ours"], true);
    // A code shows: the pairing has a minute, not the 2 s of the others.
    let left = sheet(&shown)["left"].as_u64().unwrap_or_default();
    assert!((50..=60).contains(&left), "{left} s left");
    bed.places_with("sheet")?;
    bed.click("sheet:ok")?;
    bed.shows(|s| sheet(s).is_null())?;
    bed.asked(&Call::Trust(PHONE.to_owned()))?;
    let shown = bed.shows(|s| device(s, "mine", PHONE)["connected"] == true)?;
    assert_eq!(names(&shown, "mine")[0], PHONE);
    Ok(())
}

#[test]
fn annuler_on_a_code_refuses_it_and_says_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.click(&format!("bt:pair:{PHONE}"))?;
    bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    // Esc is the sheet's Annuler.
    bed.key(KEY_ESC)?;
    let shown =
        bed.shows(|s| sheet(s).is_null() && device(s, "nearby", PHONE)["line"] != "Association…")?;
    assert!(
        device(&shown, "nearby", PHONE)["failure"].is_null(),
        "{shown}"
    );
    assert_eq!(bed.bluez().paired(PHONE), (false, false));
    Ok(())
}

#[test]
fn pairing_a_keyboard_lights_the_digits_typed() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.click(&format!("bt:pair:{K380_NEW}"))?;
    let shown = bed.shows(|s| sheet(s)["kind"] == "display" && sheet(s)["typed"] == 3)?;
    assert_eq!(sheet(&shown)["code"], "305718");
    bed.places_with("sheet:cells")?;
    // Annuler gives the pairing up.
    bed.click("sheet:cancel")?;
    bed.asked(&Call::CancelPairing(K380_NEW.to_owned()))?;
    let shown =
        bed.shows(|s| sheet(s).is_null() && device(s, "nearby", K380_NEW)["line"] == "Clavier")?;
    assert!(
        device(&shown, "nearby", K380_NEW)["failure"].is_null(),
        "{shown}"
    );
    Ok(())
}

#[test]
fn a_legacy_device_asks_for_a_pin() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.click(&format!("bt:pair:{LEGACY}"))?;
    bed.shows(|s| sheet(s)["kind"] == "pin")?;
    // Its field has the keyboard: the PIN typed, Enter gives it.
    bed.type_text("0000")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Code("0000".to_owned()))?;
    bed.shows(|s| device(s, "mine", LEGACY)["connected"] == true)?;
    Ok(())
}

#[test]
fn a_phone_that_starts_the_pairing_reaches_the_page_while_it_is_open() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_on("home")?;
    // Not shown: no agent of the page's, nobody to ask.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(!bed.bluez().default_agent());
    bed.click("card:bluetooth")?;
    bed.shows(|s| names(s, "nearby").len() == 3)?;
    bed.bluez_is("the page's agent the default", Bluez::default_agent)?;
    // The phone asks to pair, comparing a code: the sheet, accepted.
    bed.bluez().incoming(PHONE, Incoming::Confirm(123_456))?;
    let shown = bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    assert_eq!(sheet(&shown)["code"], "123 456");
    assert_eq!(sheet(&shown)["ours"], false);
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Incoming(PHONE.to_owned(), true))?;
    bed.shows(|s| names(s, "mine").contains(&PHONE.to_owned()))?;
    // Earbuds pairing by themselves: "veut s'associer", refused.
    bed.bluez().incoming(LEGACY, Incoming::Authorize)?;
    bed.shows(|s| sheet(s)["kind"] == "authorize")?;
    bed.click("sheet:cancel")?;
    bed.asked(&Call::Incoming(LEGACY.to_owned(), false))?;
    // The sheet gone, its scrim with it: the sidebar takes the click.
    bed.shows(|s| sheet(s).is_null())?;
    // The page left: the agent goes.
    bed.click("nav:home")?;
    bed.asked(&Call::UnregisterAgent)?;
    bed.bluez_is("no default agent", |b| !b.default_agent())?;
    Ok(())
}

#[test]
fn the_device_giving_up_closes_the_sheet_and_says_so() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.click(&format!("bt:pair:{PHONE}"))?;
    bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    bed.bluez().give_up(PHONE)?;
    let shown =
        bed.shows(|s| sheet(s).is_null() && !device(s, "nearby", PHONE)["failure"].is_null())?;
    assert_eq!(
        device(&shown, "nearby", PHONE)["failure"],
        "L’appareil a annulé l’association."
    );
    Ok(())
}

#[test]
fn forgetting_asks_then_removes_the_device() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "mine").len() == 4)?;
    bed.menu_item(PIXEL, 2)?;
    let shown = bed.shows(|s| s["bluetooth"]["forgetting"] == PIXEL)?;
    assert!(shown["bluetooth"]["menu"].is_null());
    bed.places_with("sheet:ok")?;
    // Esc: nothing forgotten.
    bed.key(KEY_ESC)?;
    bed.shows(|s| s["bluetooth"]["forgetting"].is_null())?;
    assert!(bed.bluez().known(PIXEL));
    // Asked again, confirmed: the row goes, the toast has no "Annuler".
    bed.menu_item(PIXEL, 2)?;
    bed.shows(|s| s["bluetooth"]["forgetting"] == PIXEL)?;
    bed.click("sheet:ok")?;
    bed.asked(&Call::Remove(PIXEL.to_owned()))?;
    let shown = bed.shows(|s| !names(s, "mine").contains(&PIXEL.to_owned()))?;
    assert_eq!(shown["toast"], "« Pixel 8 » oublié.");
    assert!(shown["toast_action"].is_null());
    assert_eq!(shown["undo"], 0);
    // Ctrl+Z skips it.
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(!bed.bluez().known(PIXEL));
    Ok(())
}

#[test]
fn renaming_sets_the_alias_and_annuler_takes_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| names(s, "mine").len() == 4)?;
    bed.menu_item(PIXEL, 0)?;
    bed.shows(|s| s["bluetooth"]["renaming"] == PIXEL)?;
    bed.places_with("bt:rename")?;
    // The name selected: what is typed takes its place.
    bed.type_text("tel alex")?;
    bed.shows(|s| s["bluetooth"]["renaming"] == "tel alex")?;
    bed.key(KEY_ENTER)?;
    bed.asked(&Call::Alias(PIXEL.to_owned(), "tel alex".to_owned()))?;
    let shown = bed.shows(|s| names(s, "mine").contains(&"tel alex".to_owned()))?;
    assert_eq!(shown["toast"], "Renommé : « tel alex ».");
    assert_eq!(shown["toast_action"], "undo");
    bed.away()?;
    bed.click("toast-action")?;
    bed.asked(&Call::Alias(PIXEL.to_owned(), PIXEL.to_owned()))?;
    let shown = bed.shows(|s| names(s, "mine").contains(&PIXEL.to_owned()))?;
    assert_eq!(shown["toast"], "Annulé : le nom revient à « Pixel 8 ».");
    // F2 renames the row with the keyboard; Esc lets it go.
    let mut focus = shown["focus"].clone();
    for _ in 0..30 {
        bed.key(KEY_TAB)?;
        focus = bed.shows(|s| s["focus"] != focus)?["focus"].clone();
        if focus == "control:1" {
            break;
        }
    }
    bed.key(KEY_F2)?;
    bed.shows(|s| s["bluetooth"]["renaming"].is_string())?;
    bed.key(KEY_ESC)?;
    bed.shows(|s| s["bluetooth"]["renaming"].is_null())?;
    // Delete asks to forget it.
    bed.key(KEY_DELETE)?;
    bed.shows(|s| s["bluetooth"]["forgetting"].is_string())?;
    Ok(())
}

#[test]
fn connect_automatically_is_its_trust_and_annuler_takes_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(setup()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| device(s, "mine", K380)["trusted"] == true)?;
    bed.menu_item(K380, 1)?;
    bed.asked(&Call::Distrust(K380.to_owned()))?;
    let shown = bed.shows(|s| device(s, "mine", K380)["trusted"] == false)?;
    assert_eq!(shown["toast"], "« Clavier K380 » ne se connecte plus seul.");
    assert_eq!(device(&shown, "mine", K380)["line"], "Pas connecté");
    bed.away()?;
    bed.click("toast-action")?;
    bed.shows(|s| device(s, "mine", K380)["trusted"] == true)?;
    Ok(())
}

#[test]
fn the_popovers_dans_les_reglages_row_opens_the_page_and_pairs() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_on(
        Some(Setup {
            later: vec![FakeDevice::around(PHONE, "phone", -50).pairing(Pairing::Confirm(482_913))],
            ..Setup::default()
        }),
        "1920x1080",
        2,
        true,
    )?
    else {
        return Ok(());
    };
    // The bar's Bluetooth row, once shown, opens its popover; the phone
    // is found.
    let log = bed.shell_log.clone();
    let shown = bed
        .hp
        .dispatch_until(WAIT, |_| common::logged(&log).contains("row=Nothing"))?;
    anyhow::ensure!(shown, "no Bluetooth row:\n{}", bed.shell_said());
    wait(&mut bed.hp, Duration::from_millis(600))?;
    let s = bed
        .hp
        .connections_surfaces()
        .first()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow::anyhow!("no connections"))?;
    let item = (
        f64::from(s.x + s.width / 2),
        f64::from(s.y + s.height) - 5.5 - 15.0,
    );
    common::click(&mut bed.hp, item)?;
    let opened = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(opened, "no popover:\n{}", bed.shell_said());
    let found = bed.hp.dispatch_until(WAIT, |_| {
        common::logged(&log).contains(&format!("{PHONE} (code)"))
    })?;
    anyhow::ensure!(
        found,
        "the popover never listed the phone:\n{}",
        bed.shell_said()
    );
    // Its row, by the keyboard: the switch, the phone; Enter.
    bed.key(KEY_TAB)?;
    bed.key(KEY_TAB)?;
    bed.key(KEY_ENTER)?;
    // The Settings open on Bluetooth and pair it: the code's sheet.
    let open = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID))
    })?;
    anyhow::ensure!(open, "the Settings never opened:\n{}", bed.shell_said());
    bed.hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    let shown = bed.shows(|s| s["page"] == "bluetooth" && sheet(s)["kind"] == "confirm")?;
    assert_eq!(sheet(&shown)["name"], PHONE);
    assert_eq!(sheet(&shown)["ours"], true);
    bed.click("sheet:ok")?;
    bed.shows(|s| device(s, "mine", PHONE)["connected"] == true)?;
    Ok(())
}

#[test]
fn at_rest_with_a_sheet_closed_the_page_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(with_codes()))? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.shows(|s| s["bluetooth"]["searching"] == false)?;
    // The countdown draws each second while its sheet shows.
    bed.click(&format!("bt:pair:{PHONE}"))?;
    bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    bed.away()?;
    let before = bed.hp.window_commits();
    wait(&mut bed.hp, Duration::from_millis(2500))?;
    assert!(bed.hp.window_commits() > before, "the countdown never drew");
    // Closed: nothing more.
    bed.key(KEY_ESC)?;
    bed.shows(|s| sheet(s).is_null() && device(s, "nearby", PHONE)["line"] != "Association…")?;
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
    let Some(mut bed) = Bed::start_on(Some(setup()), "2880x1800@2", 20, false)? else {
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
        false,
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
            devices: mine_off(),
            ..Setup::default()
        }),
        "2880x1800@2",
        2,
        false,
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
        false,
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["bluetooth"]["adapter"] == false)?;
    shoot(&mut bed, "bluetooth-no-adapter.png")?;
    Ok(())
}

/// What is on screen in `rect` (×2), or the Settings' window, saved as
/// `name` in `out`, once the window has settled.
fn picture(bed: &mut Bed, out: &Path, name: &str, rect: Option<Rect>) -> anyhow::Result<()> {
    use anyhow::Context as _;
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
    let rect = match rect {
        Some(rect) => rect,
        None => bed.window()?,
    };
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let pad = 24;
    let x = (rect.x * 2 - pad).max(0) as u32;
    let y = (rect.y * 2 - pad).max(0) as u32;
    let cw = ((rect.width * 2 + 2 * pad) as u32).min(w as u32 - x);
    let ch = ((rect.height * 2 + 2 * pad) as u32).min(h as u32 - y);
    let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let path = out.join(name);
    image.save(&path)?;
    eprintln!("{}", path.display());
    Ok(())
}

/// The second pull request's pictures, at ×2: `HOCUS_SHOTS=DIR`.
#[test]
#[ignore = "pictures for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_codes_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(Some(with_codes()), "2880x1800@2", 2, false)? else {
        return Ok(());
    };
    bed.open_around(3)?;
    bed.shows(|s| s["bluetooth"]["searching"] == false)?;
    // A paired device's menu.
    bed.click(&format!("bt:more:{PIXEL}"))?;
    bed.shows(|s| s["bluetooth"]["menu"] == PIXEL)?;
    bed.hp.pointer_move(5.0, 5.0);
    picture(&mut bed, &out, "bluetooth-menu.png", None)?;
    // Forgetting asks first.
    bed.click("bt:item:2")?;
    bed.shows(|s| s["bluetooth"]["forgetting"] == PIXEL)?;
    bed.away()?;
    picture(&mut bed, &out, "bluetooth-forget.png", None)?;
    bed.key(KEY_ESC)?;
    // Renaming: the field.
    bed.menu_item(PIXEL, 0)?;
    bed.shows(|s| s["bluetooth"]["renaming"] == PIXEL)?;
    bed.type_text("tel alex")?;
    bed.shows(|s| s["bluetooth"]["renaming"] == "tel alex")?;
    bed.away()?;
    picture(&mut bed, &out, "bluetooth-rename.png", None)?;
    bed.key(KEY_ESC)?;
    // A code to compare.
    bed.click(&format!("bt:pair:{PHONE}"))?;
    bed.shows(|s| sheet(s)["kind"] == "confirm")?;
    bed.away()?;
    picture(&mut bed, &out, "bluetooth-code.png", None)?;
    bed.key(KEY_ESC)?;
    bed.shows(|s| sheet(s).is_null() && device(s, "nearby", PHONE)["line"] != "Association…")?;
    // A code typed on a keyboard.
    bed.click(&format!("bt:pair:{K380_NEW}"))?;
    bed.shows(|s| sheet(s)["typed"] == 3)?;
    bed.away()?;
    picture(&mut bed, &out, "bluetooth-keyboard-code.png", None)?;
    bed.click("sheet:cancel")?;
    bed.shows(|s| sheet(s).is_null() && device(s, "nearby", K380_NEW)["line"] == "Clavier")?;
    // A PIN.
    bed.click(&format!("bt:pair:{LEGACY}"))?;
    bed.shows(|s| sheet(s)["kind"] == "pin")?;
    bed.type_text("0000")?;
    bed.away()?;
    picture(&mut bed, &out, "bluetooth-pin.png", None)?;
    drop(bed);
    // The bar's popover: the phone "dans les Réglages", the link.
    let Some(mut bed) = Bed::start_on(
        Some(Setup {
            devices: vec![FakeDevice::paired(SONY, "audio-headphones", Some(82)).connected()],
            later: vec![
                FakeDevice::around(BUDS, "audio-headphones", -45),
                FakeDevice::around(PHONE, "phone", -50).pairing(Pairing::Confirm(482_913)),
            ],
            ..Setup::default()
        }),
        "2880x1800@2",
        2,
        true,
    )?
    else {
        return Ok(());
    };
    let log = bed.shell_log.clone();
    bed.hp
        .dispatch_until(WAIT, |_| common::logged(&log).contains("row=Connected"))?;
    wait(&mut bed.hp, Duration::from_millis(600))?;
    let s = bed
        .hp
        .connections_surfaces()
        .first()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow::anyhow!("no connections"))?;
    let item = (
        f64::from(s.x + s.width / 2),
        f64::from(s.y + s.height) - 5.5 - 15.0,
    );
    common::click(&mut bed.hp, item)?;
    bed.hp.dispatch_until(WAIT, |_| {
        common::logged(&log).contains(&format!("{PHONE} (code)"))
    })?;
    bed.hp.pointer_move(5.0, 5.0);
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    let rect = bed
        .hp
        .popover_surfaces()
        .first()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow::anyhow!("no popover"))?;
    picture(&mut bed, &out, "popover.png", Some(rect))?;
    Ok(())
}
