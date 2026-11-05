//! The Settings' Réseau page (6.6, issue #406, variant A): the real
//! `hocus-settings` window, a client of a headless compositor, against a
//! NetworkManager of the test's own on a private system bus
//! (`levels/nm.rs`) and radios of the test's own (`levels/rfkill.rs`),
//! never the machine's. Its configuration, state, home and runtime
//! directories are the test's own; no session bus; it draws with
//! tiny-skia.
//!
//! One test per point of the approved spec: the network in use and the
//! ones around, a scan asked as the page shows; an open network joined
//! from its row; the Wi-Fi's switch and its "Annuler"; the saved
//! networks most recent first, a password read with the eye and changed,
//! "Se connecter seul"; a network forgotten and given back with its
//! password by "Annuler", one in use joined again; polkit's refusal; a
//! wrong password in the sheet said and the field kept, the right one
//! joining; airplane mode blocking Wi-Fi and Bluetooth and giving back
//! only what was on; without NetworkManager, the page read-only until it
//! comes; at rest, the window commits nothing. What the window shows is
//! read from its log, never on a time.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;
#[path = "levels/nm.rs"]
mod nm;
#[path = "levels/rfkill.rs"]
mod rfkill;

use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use common::{BTN_LEFT, KEY_ESC, WAIT, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use nm::{Call, FakeAp, Nm, Security, Setup};
use rfkill::{BLUETOOTH, Rfkill, WLAN, WWAN};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_ENTER: u32 = 28;
const KEY_TAB: u32 = 15;
const KEY_DELETE: u32 = 111;
const KEY_END: u32 = 107;
const KEY_BACKSPACE: u32 = 14;

/// The networks around the mockup's "Maison".
fn around() -> Vec<FakeAp> {
    vec![
        FakeAp::new("Maison", 82, Security::Psk("maison-pass")),
        FakeAp::new("Livebox-8F21", 70, Security::Psk("motdepasse")),
        FakeAp::new("Bureau-Invites", 64, Security::Open),
        FakeAp::new("eduroam", 55, Security::Enterprise),
        FakeAp::new("Bureau", 40, Security::Psk("bureaupass")),
    ]
}

/// Connected to "Maison"; "Bureau" used 2 h ago, "TGV INOUI" 12 days ago.
fn home() -> Setup {
    Setup {
        aps: around(),
        saved: vec![
            ("Maison", Some("maison-pass")),
            ("Bureau", Some("bureaupass")),
            ("TGV INOUI", None),
        ],
        current: Some("Maison"),
        used: vec![("Bureau", 2 * 3600 + 60), ("TGV INOUI", 12 * 86_400 + 60)],
        ..Setup::default()
    }
}

struct Bed {
    hp: Headless,
    fakes: Fakes,
    nm: Option<Nm>,
    rfkill: Rfkill,
    log: PathBuf,
    script: PathBuf,
    state: PathBuf,
}

/// The evdev code of a letter or a digit, on a US keyboard.
fn key_of(c: char) -> Option<u32> {
    const LETTERS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    match c {
        'a'..='z' => LETTERS.get(c as usize - 'a' as usize).copied(),
        '1'..='9' => Some(2 + (c as u32 - '1' as u32)),
        '0' => Some(11),
        '-' => Some(12),
        _ => None,
    }
}

impl Bed {
    /// The compositor, the fakes (NetworkManager as `setup` says, if
    /// any) and the Settings' script; `None` when the Settings are not
    /// built or D-Bus is missing.
    fn start(setup: Option<Setup>, radios: &[(u8, bool)]) -> anyhow::Result<Option<Self>> {
        Self::start_on(setup, radios, "1920x1080")
    }

    fn start_on(
        setup: Option<Setup>,
        radios: &[(u8, bool)],
        output: &str,
    ) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        if !fakes::available() {
            eprintln!("dbus-daemon or PipeWire's tools missing: skipped");
            return Ok(None);
        }
        let mut fakes = Fakes::empty()?;
        fakes.start_bus()?;
        let nm = match setup {
            Some(setup) => Some(Nm::start(fakes.runtime(), &fakes.address, setup)?),
            None => None,
        };
        let root = fakes.dir.path().to_owned();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let rfkill = Rfkill::start(&root, radios)?;
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, "# Mine.\n[keyboard]\nlayouts = [\"us\"]\n")?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{}' HOCUS_RFKILL='{}' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug,hocus_network=info\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                fakes.address,
                rfkill.path.display(),
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
        let mut config = Config {
            plain_background: true,
            french: true,
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
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
            fakes,
            nm,
            rfkill,
            log,
            script,
            state: root.join("state/hocuspanda/airplane"),
        }))
    }

    fn nm(&self) -> &Nm {
        self.nm
            .as_ref()
            .unwrap_or_else(|| unreachable!("no NetworkManager"))
    }

    /// The Settings launched on Réseau, their window open with the
    /// keyboard and the page drawn once `ready` holds.
    fn open_until(&mut self, ready: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "network".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        let shown = self.shows(|s| s["page"] == "network" && ready(s))?;
        self.places_with("row:network.airplane")?;
        Ok(shown)
    }

    /// Open, NetworkManager read, its networks around listed.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.open_until(|s| {
            s["network"]["manager"] == true
                && s["network"]["radios"] == true
                && s["network"]["around"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
        })
    }

    fn plain(&self) -> String {
        common::logged(&self.log)
    }

    fn said(&self) -> String {
        self.plain()
            .lines()
            .filter(|l| !l.contains(" shows {") && !l.contains(" places {"))
            .collect::<Vec<_>>()
            .join("\n")
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
                "the window never showed it; it shows {}\nit said:\n{}",
                common::last_json(&log, " shows {")
                    .map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                self.said()
            )
        })
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

    /// A click in the middle of `id`, scrolled into the window first,
    /// once it stays in place (a list that just changed is laid out).
    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        self.scroll_to_show(id)?;
        let mut places = self.places_with(id)?;
        for _ in 0..50 {
            wait(&mut self.hp, Duration::from_millis(60))?;
            let again = self.places_with(id)?;
            if again[id] == places[id] {
                break;
            }
            places = again;
        }
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
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

    /// The page scrolled with the wheel until `id` is in the window.
    fn scroll_to_show(&mut self, id: &str) -> anyhow::Result<()> {
        let window = self.window()?;
        let mut place = self.places_with(id)?[id].clone();
        for _ in 0..60 {
            let n = |i: usize| place[i].as_f64().unwrap_or_default();
            if n(1) >= 50.0 && n(1) + n(3) <= f64::from(window.height) - 10.0 {
                return Ok(());
            }
            let middle = (
                f64::from(window.x) + f64::from(window.width) * 0.6,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            self.hp.pointer_move(middle.0 - 1.0, middle.1);
            self.hp.pointer_move(middle.0, middle.1);
            self.hp.wheel(if n(1) < 50.0 { -1 } else { 1 });
            let log = self.log.clone();
            let mut moved = None;
            self.hp.dispatch_until(WAIT, |_| {
                moved = common::last_json(&log, " places {")
                    .and_then(|p| p.get(id).cloned())
                    .filter(|p| *p != place);
                moved.is_some()
            })?;
            place = moved.ok_or_else(|| anyhow::anyhow!("{id} never came into view: {place}"))?;
        }
        anyhow::bail!("{id} never came into view")
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// `text` typed, each key taken before the next.
    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let key = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c:?}"))?;
            self.press(key);
            wait(&mut self.hp, Duration::from_millis(15))?;
        }
        Ok(())
    }

    /// Waits until NetworkManager has been asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let Some(nm) = self.nm.as_ref() else {
            anyhow::bail!("no NetworkManager");
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| nm.calls().contains(call))?;
        anyhow::ensure!(
            done,
            "not asked {call:?}: {:?}\nthe window said:\n{}",
            nm.calls(),
            self.said()
        );
        Ok(())
    }

    /// Waits until `done` holds of the fakes.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < WAIT {
            if done(self) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("never {what}; the window said:\n{}", self.said())
    }

    /// Waits until the window commits nothing for a second.
    fn settled(&mut self) -> anyhow::Result<u64> {
        let mut last = self.hp.window_commits();
        let mut still = 0;
        for _ in 0..200 {
            wait(&mut self.hp, Duration::from_millis(100))?;
            let now = self.hp.window_commits();
            still = if now == last { still + 1 } else { 0 };
            last = now;
            if still >= 10 {
                return Ok(last);
            }
        }
        anyhow::bail!("the window never came to rest")
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["name"].as_str().map(str::to_owned))
        .collect()
}

fn find<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .and_then(|l| l.iter().find(|n| n["name"] == name))
        .unwrap_or(&Value::Null)
}

#[test]
fn the_network_page_shows_the_connected_network_and_the_ones_around() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false), (BLUETOOTH, false)])? else {
        return Ok(());
    };
    let shown = bed.open()?;
    // A scan asked as the page shows.
    bed.asked(&Call::Scan)?;
    let network = &shown["network"];
    assert_eq!(network["airplane"], false);
    assert_eq!(network["wifi"]["on"], true);
    assert_eq!(network["current"]["name"], "Maison");
    assert_eq!(
        network["current"]["line"],
        "Connecté · WPA2 · 5 GHz · signal fort"
    );
    assert_eq!(
        network["current"]["address"],
        "Adresse 192.168.1.24 · par la box 192.168.1.1"
    );
    assert_eq!(network["saved_count"], 3);
    // The others, strongest first, saying what they ask.
    let around = &network["around"];
    assert_eq!(
        names(around),
        ["Livebox-8F21", "Bureau-Invites", "eduroam", "Bureau"]
    );
    assert_eq!(find(around, "Livebox-8F21")["line"], "WPA2");
    assert_eq!(find(around, "Livebox-8F21")["action"], "sheet");
    assert_eq!(
        find(around, "Bureau-Invites")["line"],
        "Ouvert · une page de connexion peut s’afficher"
    );
    assert_eq!(find(around, "Bureau-Invites")["action"], "connect");
    assert_eq!(
        find(around, "eduroam")["line"],
        "Entreprise (802.1X) · identifiant et mot de passe"
    );
    assert_eq!(find(around, "Bureau")["line"], "WPA2 · enregistré");
    assert_eq!(find(around, "Bureau")["action"], "connect");
    // The saved networks: the one in use, then the most recent first.
    bed.click("row:network.saved")?;
    let sub = bed.shows(|s| s["network"]["sub"] == true)?;
    let saved = &sub["network"]["saved"];
    assert_eq!(names(saved), ["Maison", "Bureau", "TGV INOUI"]);
    assert_eq!(find(saved, "Maison")["line"], "Connecté maintenant · WPA2");
    assert_eq!(find(saved, "Bureau")["line"], "Il y a 2 h · WPA2");
    assert_eq!(find(saved, "TGV INOUI")["line"], "Il y a 12 jours · ouvert");
    // The home's card says the network in use.
    bed.click("network:back")?;
    bed.shows(|s| s["network"]["sub"] == false)?;
    bed.click("nav:home")?;
    let home = bed.shows(|s| s["page"] == "home")?;
    let card = home["cards"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c[0] == "Réseau"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(card[1], "Wi-Fi · Maison");
    Ok(())
}

#[test]
fn an_open_network_joins_from_its_row_and_disconnect_leaves() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("network:disconnect")?;
    bed.asked(&Call::Disconnect)?;
    bed.shows(|s| s["network"]["current"].is_null())?;
    bed.click("connect:Bureau-Invites")?;
    bed.asked(&Call::AddAndActivate {
        ssid: "Bureau-Invites".to_owned(),
        psk: None,
    })?;
    bed.shows(|s| s["network"]["current"]["name"] == "Bureau-Invites")?;
    // A saved one joins with its saved connection.
    bed.click("connect:Bureau")?;
    bed.asked(&Call::Activate {
        ssid: "Bureau".to_owned(),
    })?;
    bed.shows(|s| s["network"]["current"]["name"] == "Bureau")?;
    Ok(())
}

#[test]
fn the_wifi_switch_toasts_and_annuler_turns_it_back_on() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("switch:network.wifi")?;
    bed.asked(&Call::Enable(false))?;
    let off = bed.shows(|s| s["network"]["wifi"]["on"] == false && s["toast"] == "Wi-Fi coupé")?;
    // Off: no network around listed.
    assert!(off["network"]["around"].is_null(), "{off}");
    bed.click("toast-action")?;
    bed.asked(&Call::Enable(true))?;
    bed.shows(|s| s["network"]["wifi"]["on"] == true && s["toast"] == "Annulé : Wi-Fi allumé.")?;
    Ok(())
}

#[test]
fn a_saved_networks_password_shows_with_the_eye_and_changes_with_enter() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("row:network.saved")?;
    bed.shows(|s| s["network"]["sub"] == true)?;
    bed.click("saved:Bureau")?;
    let open = bed.shows(|s| s["network"]["unfolded"].is_string())?;
    let uuid = open["network"]["unfolded"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    // Never read before the eye is pressed.
    assert!(
        !bed.nm()
            .calls()
            .iter()
            .any(|c| matches!(c, Call::GetSecrets { .. })),
        "{:?}",
        bed.nm().calls()
    );
    bed.click("network:eye")?;
    bed.asked(&Call::GetSecrets {
        ssid: "Bureau".to_owned(),
    })?;
    bed.shows(|s| {
        s["network"]["password"]["shown"] == true && s["network"]["password"]["read"] == true
    })?;
    // Not in the log, even shown.
    assert!(
        !bed.plain().contains("bureaupass"),
        "the password was logged"
    );
    // Changed: typed, Enter saves it to disk.
    bed.click("network:change")?;
    bed.shows(|s| s["network"]["password"]["editing"] == true)?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("nouveaumotdepasse")?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::Update2 {
        ssid: "Bureau".to_owned(),
        psk: Some("nouveaumotdepasse".to_owned()),
        autoconnect: Some(true),
    })?;
    let saved = bed.nm().saved();
    assert!(
        saved.contains(&("Bureau".to_owned(), Some("nouveaumotdepasse".to_owned()))),
        "{saved:?}"
    );
    // "Se connecter seul" off.
    bed.shows(|s| s["network"]["password"].is_null())?;
    bed.click("switch:network.autoconnect")?;
    bed.asked(&Call::Update2 {
        ssid: "Bureau".to_owned(),
        psk: None,
        autoconnect: Some(false),
    })?;
    bed.shows(|s| find(&s["network"]["saved"], "Bureau")["autoconnect"] == false)?;
    // NetworkManager kept the password the update left out.
    assert!(
        bed.nm()
            .saved()
            .contains(&("Bureau".to_owned(), Some("nouveaumotdepasse".to_owned())))
    );
    assert!(
        bed.nm()
            .saved_uuids()
            .iter()
            .any(|(u, s, a)| *u == uuid && s == "Bureau" && !a)
    );
    Ok(())
}

#[test]
fn forgetting_a_network_deletes_it_and_annuler_brings_it_back_with_its_password()
-> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("row:network.saved")?;
    bed.click("saved:Bureau")?;
    let open = bed.shows(|s| s["network"]["unfolded"].is_string())?;
    let uuid = open["network"]["unfolded"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    bed.click("network:forget")?;
    bed.asked(&Call::Delete {
        ssid: "Bureau".to_owned(),
    })?;
    let gone = bed.shows(|s| {
        s["toast"] == "« Bureau » oublié."
            && s["toast_action"] == "undo"
            && names(&s["network"]["saved"]) == ["Maison", "TGV INOUI"]
    })?;
    assert!(gone["network"]["unfolded"].is_null());
    bed.click("toast-action")?;
    bed.asked(&Call::AddConnection {
        ssid: "Bureau".to_owned(),
        uuid: uuid.clone(),
        psk: Some("bureaupass".to_owned()),
    })?;
    bed.shows(|s| {
        s["toast"] == "Annulé : « Bureau » revient."
            && names(&s["network"]["saved"]).contains(&"Bureau".to_owned())
    })?;
    // The one in use, forgotten with Delete on the keyboard, comes back
    // joined.
    bed.click("network:back")?;
    bed.click("row:network.saved")?;
    bed.shows(|s| s["network"]["sub"] == true)?;
    // `Tab` to the list (after the search field, the sidebar, "‹ Réseau"),
    // its first row the one in use.
    let mut focused = bed.shows(|_| true)?["focus"].clone();
    for _ in 0..40 {
        if focused == "control:1" {
            break;
        }
        bed.press(KEY_TAB);
        let before = focused.clone();
        focused = bed.shows(|s| s["focus"] != before)?["focus"].clone();
    }
    assert_eq!(focused, "control:1");
    bed.press(KEY_DELETE);
    bed.asked(&Call::Delete {
        ssid: "Maison".to_owned(),
    })?;
    bed.shows(|s| s["toast"] == "« Maison » oublié.")?;
    bed.until("disconnected", |bed| bed.nm().connected().is_none())?;
    bed.click("toast-action")?;
    bed.until("Maison back", |bed| {
        bed.nm()
            .calls()
            .iter()
            .any(|c| matches!(c, Call::AddConnection { ssid, .. } if ssid == "Maison"))
    })?;
    bed.asked(&Call::Activate {
        ssid: "Maison".to_owned(),
    })?;
    bed.until("joined again", |bed| {
        bed.nm().connected().as_deref() == Some("Maison")
    })?;
    Ok(())
}

#[test]
fn a_change_polkit_refuses_is_said_and_nothing_changes() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.nm().refuse(true);
    bed.click("row:network.saved")?;
    bed.click("saved:Bureau")?;
    bed.shows(|s| s["network"]["unfolded"].is_string())?;
    bed.click("network:forget")?;
    let refused = bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("Le système a refusé : "))
    })?;
    assert_eq!(
        names(&refused["network"]["saved"]),
        ["Maison", "Bureau", "TGV INOUI"]
    );
    assert!(bed.nm().saved().iter().any(|(s, _)| s == "Bureau"));
    Ok(())
}

#[test]
fn a_wrong_password_in_the_sheet_says_so_and_keeps_the_field() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    // Esc closes the sheet, what was typed forgotten.
    bed.click("connect:Livebox-8F21")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "Livebox-8F21")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("abc")?;
    bed.press(KEY_ESC);
    bed.shows(|s| s["network"]["sheet"].is_null())?;
    bed.click("connect:Livebox-8F21")?;
    bed.shows(|s| {
        s["network"]["sheet"]["name"] == "Livebox-8F21" && s["network"]["sheet"]["filled"] == false
    })?;
    // The keyboard starts in its field.
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("mauvaispass")?;
    bed.shows(|s| s["network"]["sheet"]["filled"] == true)?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: "Livebox-8F21".to_owned(),
        psk: Some("mauvaispass".to_owned()),
    })?;
    let wrong = bed.shows(|s| s["network"]["sheet"]["error"] == "Secret")?;
    assert_eq!(wrong["network"]["sheet"]["filled"], true, "the field kept");
    // The connection that failed is not kept.
    bed.asked(&Call::Delete {
        ssid: "Livebox-8F21".to_owned(),
    })?;
    // The right one, after the wrong one taken away: joined, the sheet
    // closed.
    bed.click("sheet:field")?;
    bed.press(KEY_END);
    for _ in 0.."mauvaispass".len() {
        bed.press(KEY_BACKSPACE);
    }
    bed.shows(|s| s["network"]["sheet"]["filled"] == false)?;
    bed.type_text("motdepasse")?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: "Livebox-8F21".to_owned(),
        psk: Some("motdepasse".to_owned()),
    })?;
    bed.shows(|s| {
        s["network"]["sheet"].is_null() && s["network"]["current"]["name"] == "Livebox-8F21"
    })?;
    assert!(
        !bed.plain().contains("motdepasse"),
        "the password was logged"
    );
    Ok(())
}

#[test]
fn airplane_mode_blocks_wifi_and_bluetooth_and_restores_what_was_on() -> anyhow::Result<()> {
    // Bluetooth off before: it stays off after.
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false), (BLUETOOTH, true)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("switch:network.airplane")?;
    bed.until("blocked", |bed| {
        bed.rfkill.soft(WLAN) == Some(true) && bed.rfkill.soft(BLUETOOTH) == Some(true)
    })?;
    let on =
        bed.shows(|s| s["network"]["airplane"] == true && s["toast"] == "Mode avion activé")?;
    assert_eq!(
        bed.rfkill.asked(),
        [(WLAN, true), (BLUETOOTH, true), (WWAN, true)]
    );
    assert_eq!(on["network"]["wifi"]["on"], false);
    assert!(on["network"]["around"].is_null(), "{on}");
    // What was on is kept.
    assert_eq!(std::fs::read_to_string(&bed.state)?, "wlan\n");
    // Off: the Wi-Fi alone comes back.
    bed.click("switch:network.airplane")?;
    bed.until("unblocked", |bed| bed.rfkill.soft(WLAN) == Some(false))?;
    bed.shows(|s| s["network"]["airplane"] == false && s["network"]["wifi"]["on"] == true)?;
    assert_eq!(bed.rfkill.soft(BLUETOOTH), Some(true), "Bluetooth was off");
    assert_eq!(bed.rfkill.asked()[3..], [(WLAN, false)]);
    assert!(!bed.state.exists());
    // On again, then the Wi-Fi's switch turns the Wi-Fi on alone.
    bed.click("switch:network.airplane")?;
    bed.shows(|s| s["network"]["airplane"] == true)?;
    bed.click("switch:network.wifi")?;
    bed.until("Wi-Fi alone", |bed| {
        bed.rfkill.soft(WLAN) == Some(false) && bed.rfkill.soft(BLUETOOTH) == Some(true)
    })?;
    bed.shows(|s| s["network"]["airplane"] == false && s["network"]["wifi"]["on"] == true)?;
    // A switch blocks the radio: said, and the switch inert.
    bed.rfkill.set_hard(WLAN, true);
    let blocked = bed.shows(|s| s["network"]["wifi"]["block"] == "hard")?;
    assert_eq!(blocked["network"]["wifi"]["on"], false);
    Ok(())
}

#[test]
fn without_networkmanager_the_page_is_read_only() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(None, &[(WLAN, false)])? else {
        return Ok(());
    };
    let shown = bed.open_until(|s| s["network"]["manager"] == false)?;
    bed.places_with("banner:network")?;
    assert_eq!(shown["network"]["card"], "Inconnu");
    // Nothing takes the keyboard on the page, the switches are inert.
    bed.click("switch:network.airplane")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(bed.rfkill.asked().is_empty());
    // NetworkManager comes: the page fills in.
    let nm = Nm::start(bed.fakes.runtime(), &bed.fakes.address, home())?;
    bed.nm = Some(nm);
    bed.shows(|s| s["network"]["manager"] == true && s["network"]["current"]["name"] == "Maison")?;
    Ok(())
}

#[test]
fn at_rest_the_page_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.asked(&Call::Scan)?;
    // The scan over: nothing moves.
    bed.shows(|s| s["network"]["scanning"] == false)?;
    bed.hp.pointer_move(5.0, 5.0);
    let last = bed.settled()?;
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
        // Settled, or a field's caret blinking.
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
    let Some(mut bed) = Bed::start_on(
        Some(home()),
        &[(WLAN, false), (BLUETOOTH, false)],
        "2880x1800@2",
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["network"]["scanning"] == false)?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "network.png")?;
    // The saved networks, one unfolded, its password shown.
    bed.click("network:details")?;
    bed.shows(|s| s["network"]["sub"] == true && s["network"]["unfolded"].is_string())?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "network-saved.png")?;
    // Another forgotten: its toast.
    bed.click("saved:Bureau")?;
    bed.shows(|s| s["network"]["unfolded"] == find(&s["network"]["saved"], "Bureau")["uuid"])?;
    bed.click("network:forget")?;
    bed.shows(|s| s["toast"] == "« Bureau » oublié.")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "network-forgotten.png")?;
    bed.click("toast-action")?;
    bed.shows(|s| s["toast"] == "Annulé : « Bureau » revient.")?;
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    // The sheet, a wrong password said.
    bed.click("network:back")?;
    bed.shows(|s| s["network"]["sub"] == false)?;
    bed.click("connect:Livebox-8F21")?;
    bed.shows(|s| s["network"]["sheet"].is_object())?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("mauvaispass")?;
    bed.press(KEY_ENTER);
    bed.shows(|s| s["network"]["sheet"]["error"] == "Secret")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "network-sheet.png")?;
    bed.press(KEY_ESC);
    bed.shows(|s| s["network"]["sheet"].is_null())?;
    // Airplane mode.
    bed.click("switch:network.airplane")?;
    bed.shows(|s| s["network"]["airplane"] == true)?;
    bed.click("toast-close")?;
    bed.shows(|s| s["toast"].is_null())?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "network-airplane.png")?;
    // Without NetworkManager.
    let Some(mut none) = Bed::start_on(None, &[(WLAN, false)], "2880x1800@2")? else {
        return Ok(());
    };
    none.open_until(|s| s["network"]["manager"] == false)?;
    none.hp.pointer_move(5.0, 5.0);
    shoot(&mut none, "network-without-nm.png")?;
    Ok(())
}
