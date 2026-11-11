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

use crate::common;
use crate::fakes;
use crate::nm;
use crate::rfkill;
use crate::settings_page;

use settings_page::{Floor, Wheel};
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
    shell_log: PathBuf,
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
        '.' => Some(52),
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
        Self::start_all(setup, radios, output, false)
    }

    /// The same with the shell (its bar, its Wi-Fi popover), which starts
    /// the Settings by their script.
    fn start_with_shell(setup: Setup, radios: &[(u8, bool)]) -> anyhow::Result<Option<Self>> {
        Self::start_with_shell_on(setup, radios, "1920x1080")
    }

    fn start_with_shell_on(
        setup: Setup,
        radios: &[(u8, bool)],
        output: &str,
    ) -> anyhow::Result<Option<Self>> {
        Self::start_all(Some(setup), radios, output, true)
    }

    fn start_all(
        setup: Option<Setup>,
        radios: &[(u8, bool)],
        output: &str,
        shell: bool,
    ) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        if !fakes::available() {
            common::missing("dbus-daemon, pipewire, pw-dump, wpctl and pw-metadata");
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
                 export DBUS_SYSTEM_BUS_ADDRESS='{}' DBUS_SESSION_BUS_ADDRESS='{}' HOCUS_RFKILL='{}' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug,hocus_network=info\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                fakes.address,
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
        // nmcli's import, as the fake NetworkManager's door does it.
        let nmcli = root.join("bin/nmcli");
        std::fs::write(
            &nmcli,
            "#!/bin/sh
# nmcli connection import type KIND file PATH
\
             exec dbus-send --system --print-reply=literal --dest=org.freedesktop.NetworkManager \
             /org/freedesktop/NetworkManager/Settings org.hocus.Test.Import \"string:$4\" \"string:$6\"\n",
        )?;
        std::fs::set_permissions(&nmcli, std::fs::Permissions::from_mode(0o755))?;
        let shell_log = root.join("shell.log");
        let shell_script = match shell.then(common::shell_program) {
            Some(program) => Some(fakes.shell_with(
                &program,
                "fr_FR.UTF-8",
                &[
                    ("HOME", at("home")),
                    ("XDG_CONFIG_HOME", at("config")),
                    ("XDG_CACHE_HOME", at("cache")),
                    ("XDG_STATE_HOME", at("state")),
                    ("XDG_DATA_HOME", at("data")),
                    ("ICED_BACKEND", "tiny-skia".to_owned()),
                    ("HOCUS_RFKILL", rfkill.path.display().to_string()),
                    ("PATH", format!("{}:/usr/bin:/bin", at("bin"))),
                    ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
                ],
                Some(&shell_log),
            )?),
            None => None,
        };
        let mut config = Config {
            plain_background: true,
            lang: hocus_words::Lang::French,
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.appearance.reduced_motion = true;
        let headless = HeadlessConfig {
            outputs: vec![output.parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        };
        let mut hp = match shell_script {
            Some(script) => {
                config.panel.form = hocus_compositor::FormChoice::Full;
                let address = fakes.address.clone();
                let mut hp = Headless::start_on_bus(headless, config, Some(script), Some(address))?;
                let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
                    !hp.connections_surfaces().is_empty()
                        && hp.connections_commits() > 0
                        && hp.levels_commits() > 0
                })?;
                anyhow::ensure!(drawn, "no connections' row");
                common::at_rest(&mut hp)?;
                hp
            }
            None => Headless::start_with(headless, config)?,
        };
        hp.pointer_move(5.0, 5.0);
        Ok(Some(Self {
            hp,
            fakes,
            nm,
            rfkill,
            log,
            script,
            state: root.join("state/hocuspanda/airplane"),
            shell_log,
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
        self.open_with(&["network"], ready)
    }

    /// The Settings launched as the desktop launches them, with `args`.
    fn launch(&mut self, args: &[&str]) {
        let mut argv = vec![self.script.display().to_string()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv,
        });
    }

    /// Launched with `args`, the window open with the keyboard and the
    /// page drawn once `ready` holds.
    fn open_with(
        &mut self,
        args: &[&str],
        ready: impl Fn(&Value) -> bool,
    ) -> anyhow::Result<Value> {
        self.launch(args);
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID))
        })?;
        anyhow::ensure!(
            open,
            "the window never opened: {:?}\n{}",
            self.hp.windows(),
            self.plain()
        );
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

    /// The page scrolled with the wheel until `id` is in the window, each
    /// wheel taken by the window before the next (#660).
    fn scroll_to_show(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 660.0,
            notches: 1,
            top: 50.0,
            floor: Floor::Margin(10.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
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
    bed.click("sheet:password")?;
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

// --- The second part: the sheets, the cable, `--join` -------------------------

/// The desktop portal's file chooser on the fakes' bus: it answers each
/// `OpenFile` with the next of `chosen`, a moment after, as a portal does
/// once the user chooses; it says what it was asked.
struct Chooser {
    chosen: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
    asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    async fn open_file(
        &self,
        _parent_window: String,
        title: String,
        options: std::collections::HashMap<String, zbus::OwnedVariant>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<zbus::OwnedObjectPath> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let token = options
            .get("handle_token")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        self.asked
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("poisoned".into()))?
            .push(title);
        let chosen = {
            let mut chosen = self
                .chosen
                .lock()
                .map_err(|_| zbus::fdo::Error::Failed("poisoned".into()))?;
            (!chosen.is_empty()).then(|| chosen.remove(0))
        };
        let uri = chosen.map(|c| format!("file://{}", c.display()));
        let connection = connection.clone();
        let answer = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let mut results: std::collections::HashMap<&str, zbus::Variant<'_>> =
                std::collections::HashMap::new();
            let code: u32 = match uri {
                Some(uri) => {
                    results.insert("uris", zbus::Variant::from(vec![uri]));
                    0
                }
                None => 1,
            };
            let _ = connection
                .emit_signal(
                    Some(sender.as_str()),
                    answer.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(code, results),
                )
                .await;
        });
        zbus::OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

impl Bed {
    /// The portal's file chooser served, answering with `chosen` in turn.
    fn chooser(
        &self,
        chosen: Vec<PathBuf>,
    ) -> anyhow::Result<(
        zbus::Connection,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    )> {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let connection = self.fakes.runtime().block_on(
            zbus::connection::Builder::address(self.fakes.address.as_str())
                .name("org.freedesktop.portal.Desktop")
                .serve_at(
                    "/org/freedesktop/portal/desktop",
                    Chooser {
                        chosen: std::sync::Arc::new(std::sync::Mutex::new(chosen)),
                        asked: asked.clone(),
                    },
                )
                .build(),
        )?;
        Ok((connection, asked))
    }

    /// The sheet's field `field` clicked, then `text` typed in it.
    fn fill(&mut self, field: &str, text: &str) -> anyhow::Result<()> {
        self.click(&format!("sheet:{field}"))?;
        let want = field.to_owned();
        self.shows(|s| {
            s["network"]["sheet"].is_object()
                && s["focus"]
                    .as_str()
                    .is_some_and(|f| f.starts_with("control:"))
                && !want.is_empty()
        })?;
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.type_text(text)?;
        let typed = field.to_owned();
        self.shows(|s| {
            s["network"]["sheet"]["typed"]
                .as_array()
                .is_some_and(|t| t.iter().any(|f| f == typed.as_str()))
        })?;
        Ok(())
    }

    /// A menu of the sheet opened, and its `index`th choice, `label`,
    /// clicked.
    fn pick(&mut self, menu: &str, label: &str, index: usize) -> anyhow::Result<()> {
        self.click(&format!("sheet:{menu}"))?;
        let want = menu.to_owned();
        self.shows(|s| s["network"]["sheet"]["menu"] == want.as_str())?;
        self.click(&format!("option:{index}"))?;
        self.shows(|s| s["network"]["sheet"]["menu"].is_null())
            .map_err(|e| e.context(format!("{label} not chosen")))?;
        Ok(())
    }
}

#[test]
fn a_hidden_network_joins_with_its_name_typed() -> anyhow::Result<()> {
    let setup = Setup {
        hidden: vec![FakeAp::new("labocache", 60, Security::Psk("cachecache"))],
        ..home()
    };
    let Some(mut bed) = Bed::start(Some(setup), &[(WLAN, false)])? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("network:hidden")?;
    let sheet = bed.shows(|s| s["network"]["sheet"]["hidden"] == true)?;
    assert_eq!(
        sheet["network"]["sheet"]["fields"],
        serde_json::json!(["name", "password"])
    );
    // The keyboard starts in its name.
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("labocache")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "labocache")?;
    bed.press(KEY_TAB);
    bed.shows(|s| s["focus"] == "control:1")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("cachecache")?;
    bed.shows(|s| s["network"]["sheet"]["filled"] == true)?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: "labocache".to_owned(),
        psk: Some("cachecache".to_owned()),
    })?;
    let added = bed.nm().added("labocache").unwrap_or_default();
    assert_eq!(
        added.get("802-11-wireless.hidden").map(String::as_str),
        Some("true"),
        "{added:?}"
    );
    assert_eq!(
        added
            .get("802-11-wireless-security.key-mgmt")
            .map(String::as_str),
        Some("wpa-psk")
    );
    bed.shows(|s| {
        s["network"]["sheet"].is_null() && s["network"]["current"]["name"] == "labocache"
    })?;
    Ok(())
}

#[test]
fn an_enterprise_network_sends_its_8021x_settings() -> anyhow::Result<()> {
    let setup = Setup {
        aps: {
            let mut aps = around();
            aps.push(FakeAp::new("Labo-TLS", 50, Security::Enterprise));
            aps
        },
        ..home()
    };
    let Some(mut bed) = Bed::start(Some(setup), &[(WLAN, false)])? else {
        return Ok(());
    };
    let dir = bed.fakes.dir.path().join("home");
    let (ca, certificate, key) = (
        dir.join("ca.pem"),
        dir.join("alex.crt"),
        dir.join("alex.key"),
    );
    for file in [&ca, &certificate, &key] {
        std::fs::write(file, "-----BEGIN-----\n")?;
    }
    let (_portal, asked) = bed.chooser(vec![certificate.clone(), key.clone(), ca.clone()])?;
    bed.open()?;
    // PEAP: MSCHAPv2, the system's authorities.
    bed.click("connect:eduroam")?;
    let sheet = bed.shows(|s| s["network"]["sheet"]["name"] == "eduroam")?;
    assert_eq!(sheet["network"]["sheet"]["security"], "Enterprise");
    assert_eq!(sheet["network"]["sheet"]["method"], "PEAP");
    assert_eq!(sheet["network"]["sheet"]["inner"], "MSCHAPv2");
    assert_eq!(
        sheet["network"]["sheet"]["fields"],
        serde_json::json!(["identity", "password", "domain"])
    );
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("alex")?;
    bed.fill("password", "motdepasse")?;
    bed.fill("domain", "radius.univ.fr")?;
    bed.shows(|s| s["network"]["sheet"]["filled"] == true)?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: "eduroam".to_owned(),
        psk: None,
    })?;
    let added = bed.nm().added("eduroam").unwrap_or_default();
    let get = |k: &str| added.get(k).cloned().unwrap_or_default();
    assert_eq!(get("802-11-wireless-security.key-mgmt"), "wpa-eap");
    assert_eq!(get("802-1x.eap"), "peap");
    assert_eq!(get("802-1x.phase2-auth"), "mschapv2");
    assert_eq!(get("802-1x.identity"), "alex");
    assert_eq!(get("802-1x.password"), "motdepasse");
    assert_eq!(get("802-1x.password-flags"), "0");
    assert_eq!(get("802-1x.system-ca-certs"), "true");
    assert_eq!(get("802-1x.domain-suffix-match"), "radius.univ.fr");
    bed.shows(|s| s["network"]["sheet"].is_null() && s["network"]["current"]["name"] == "eduroam")?;
    assert!(
        !bed.plain().contains("motdepasse"),
        "the password was logged"
    );
    // TLS: the certificate and the key from the file chooser, the
    // authority's from a file.
    bed.click("connect:Labo-TLS")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "Labo-TLS")?;
    bed.pick("method", "TLS", 2)?;
    bed.shows(|s| s["network"]["sheet"]["method"] == "TLS")?;
    wait(&mut bed.hp, Duration::from_millis(60))?;
    bed.fill("identity", "alex")?;
    bed.click("sheet:certificate")?;
    bed.shows(|s| s["network"]["sheet"]["certificate"] == true)?;
    bed.click("sheet:key")?;
    bed.shows(|s| s["network"]["sheet"]["key"] == true)?;
    bed.fill("key-password", "clesecrete")?;
    bed.pick("authority", "Choisir un fichier…", 1)?;
    bed.shows(|s| {
        s["network"]["sheet"]["authority"] == "File" && s["network"]["sheet"]["filled"] == true
    })?;
    bed.click("sheet:connect")?;
    bed.asked(&Call::AddAndActivate {
        ssid: "Labo-TLS".to_owned(),
        psk: None,
    })?;
    let added = bed.nm().added("Labo-TLS").unwrap_or_default();
    let get = |k: &str| added.get(k).cloned().unwrap_or_default();
    assert_eq!(get("802-1x.eap"), "tls");
    assert!(!added.contains_key("802-1x.phase2-auth"), "{added:?}");
    assert_eq!(
        get("802-1x.client-cert"),
        format!("file://{}", certificate.display())
    );
    assert_eq!(
        get("802-1x.private-key"),
        format!("file://{}", key.display())
    );
    assert_eq!(get("802-1x.private-key-password"), "clesecrete");
    assert_eq!(get("802-1x.ca-cert"), format!("file://{}", ca.display()));
    assert!(!added.contains_key("802-1x.system-ca-certs"), "{added:?}");
    let titles = asked.lock().map(|a| a.clone()).unwrap_or_default();
    assert_eq!(
        titles,
        [
            "Certificat personnel",
            "Clé privée",
            "Certificat de l’autorité"
        ]
    );
    Ok(())
}

#[test]
fn a_wep_network_sends_its_key() -> anyhow::Result<()> {
    let setup = Setup {
        aps: {
            let mut aps = around();
            aps.push(FakeAp::new("Imprimante-3A", 45, Security::Wep("12345")));
            aps
        },
        ..home()
    };
    let Some(mut bed) = Bed::start(Some(setup), &[(WLAN, false)])? else {
        return Ok(());
    };
    let shown = bed.open()?;
    let printer = find(&shown["network"]["around"], "Imprimante-3A");
    assert_eq!(
        printer["line"],
        "WEP · une protection ancienne, facile à casser"
    );
    assert_eq!(printer["warn"], true);
    bed.click("connect:Imprimante-3A")?;
    bed.shows(|s| s["network"]["sheet"]["security"] == "Wep")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("12345")?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: "Imprimante-3A".to_owned(),
        psk: None,
    })?;
    let added = bed.nm().added("Imprimante-3A").unwrap_or_default();
    let get = |k: &str| added.get(k).cloned().unwrap_or_default();
    assert_eq!(get("802-11-wireless-security.key-mgmt"), "none");
    assert_eq!(get("802-11-wireless-security.wep-key0"), "12345");
    assert_eq!(get("802-11-wireless-security.wep-key-type"), "1");
    bed.shows(|s| s["network"]["current"]["name"] == "Imprimante-3A")?;
    Ok(())
}

#[test]
fn the_cable_says_unplugged_then_connected() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(
        shown["network"]["wired"]["line"],
        "Adaptateur Ethernet USB · débranché"
    );
    assert_eq!(shown["network"]["wired"]["value"], "Débranché");
    bed.places_with("row:network.cable")?;
    bed.nm().set_wired(true)?;
    let plugged = bed.shows(|s| s["network"]["wired"]["connected"] == true)?;
    assert_eq!(
        plugged["network"]["wired"]["value"],
        "Connecté · 1 Gb/s · 192.168.1.30"
    );
    assert_eq!(plugged["network"]["card"], "Câble");
    Ok(())
}

#[test]
fn launched_with_join_opens_that_networks_sheet() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    // As the Wi-Fi popover's "dans les Réglages" row launches them.
    bed.open_with(&["network", "--join", "eduroam"], |s| {
        s["network"]["sheet"]["name"] == "eduroam"
    })?;
    // Launched again for another: the window comes forward on its sheet.
    bed.launch(&["network", "--join", "Livebox-8F21"]);
    let again = bed.shows(|s| s["network"]["sheet"]["name"] == "Livebox-8F21")?;
    assert_eq!(again["network"]["sheet"]["security"], "Psk { sae: false }");
    let windows = bed
        .hp
        .windows()
        .iter()
        .filter(|w| w.app_id.as_deref() == Some(APP_ID))
        .count();
    assert_eq!(windows, 1);
    Ok(())
}

// --- The third part: the VPNs, the popover's links, the bar's airplane -----

impl Bed {
    /// Waits until the shell logs `what`.
    fn shell_logged(&mut self, what: &str) -> anyhow::Result<()> {
        let log = self.shell_log.clone();
        let done = self
            .hp
            .dispatch_until(WAIT, |_| common::logged(&log).contains(what))?;
        anyhow::ensure!(done, "the shell never logged {what:?}");
        Ok(())
    }

    /// The Wi-Fi's popover opened from the bar's row: where it is.
    fn wifi_popover(&mut self) -> anyhow::Result<Rect> {
        // The connections' surface placed and sized (under the levels).
        let placed = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.connections_surfaces()
                .first()
                .is_some_and(|(_, r)| r.width > 0 && r.height >= 36)
        })?;
        anyhow::ensure!(
            placed,
            "no connections: {:?}",
            self.hp.connections_surfaces()
        );
        common::at_rest(&mut self.hp)?;
        let surface = self
            .hp
            .connections_surfaces()
            .first()
            .map(|(_, r)| *r)
            .ok_or_else(|| anyhow::anyhow!("no connections"))?;
        let item = (
            f64::from(surface.x + surface.width / 2),
            f64::from(surface.y) + 19.5,
        );
        // On the row once nothing else of the bar's is over it (#692): a
        // bar opened before the network was read has its desktops'
        // surface where the row comes, until the shell moves it up.
        common::click_on(&mut self.hp, item, hocus_link::CONNECTIONS_NAMESPACE)?;
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(
            open,
            "no popover from {item:?} on {surface:?}; the shell said:\n{}",
            common::logged(&self.shell_log)
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .join("\n")
        );
        common::at_rest(&mut self.hp)?;
        Ok(common::drawn_popovers(&self.hp)[0])
    }
}

/// The popover's row `i` under the network in use (3.5's sizes, at ×1):
/// its header, the network's 48 px block, the divider, the label.
fn popover_row(popover: Rect, i: usize) -> (f64, f64) {
    let top = 45.0 + 6.0 + 48.0 + 6.0 + 9.0 + 6.0 + (4.0 + 12.0 * 1.366) + 6.0;
    (
        f64::from(popover.x) + 120.0,
        f64::from(popover.y) + top + 38.0 * i as f64 + 18.0,
    )
}

#[test]
fn importing_a_wireguard_file_adds_a_vpn_that_is_off() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Some(home()), &[(WLAN, false)])? else {
        return Ok(());
    };
    let home_dir = bed.fakes.dir.path().join("home");
    let (travail, bureau) = (home_dir.join("Travail.conf"), home_dir.join("Bureau.ovpn"));
    std::fs::write(&travail, "[Interface]\nAddress = 10.8.0.6/24\n")?;
    std::fs::write(
        &bureau,
        "client\nremote vpn.example.org 1194\nauth-user-pass\n",
    )?;
    let (_portal, asked) = bed.chooser(vec![travail, bureau])?;
    bed.open()?;
    bed.click("network:import")?;
    bed.asked(&Call::Update2 {
        ssid: "Travail".to_owned(),
        psk: None,
        autoconnect: Some(false),
    })?;
    // Said once NetworkManager lists it.
    let added = bed.shows(|s| {
        s["toast"] == "VPN « Travail » ajouté."
            && find(&s["network"]["vpns"], "Travail")["line"] == "WireGuard · coupé"
    })?;
    assert_eq!(added["toast_action"], "undo");
    let travail = find(&added["network"]["vpns"], "Travail");
    assert_eq!(travail["line"], "WireGuard · coupé");
    assert_eq!(travail["on"], false);
    assert_eq!(
        asked.lock().map(|a| a.clone()).unwrap_or_default(),
        ["Importer un VPN"]
    );
    // "Annuler": the VPN goes.
    bed.click("toast-action")?;
    bed.asked(&Call::Delete {
        ssid: "Travail".to_owned(),
    })?;
    bed.shows(|s| {
        s["toast"] == "Annulé : VPN « Travail » retiré."
            && find(&s["network"]["vpns"], "Travail").is_null()
    })?;
    // An OpenVPN file that asks for a user name and a password: asked
    // first, then kept with it.
    bed.click("network:import")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "Bureau")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("alex")?;
    bed.press(KEY_TAB);
    bed.shows(|s| s["focus"] == "control:1")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("motdepasse")?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::VpnCredentials {
        name: "Bureau".to_owned(),
        user: "alex".to_owned(),
        password: "motdepasse".to_owned(),
    })?;
    let bureau = bed.shows(|s| find(&s["network"]["vpns"], "Bureau").is_object())?;
    assert_eq!(
        find(&bureau["network"]["vpns"], "Bureau")["line"],
        "OpenVPN · coupé"
    );
    assert!(
        !bed.plain().contains("motdepasse"),
        "the password was logged"
    );
    Ok(())
}

#[test]
fn an_openvpn_file_without_its_plugin_says_so() -> anyhow::Result<()> {
    let setup = Setup {
        no_openvpn: true,
        ..home()
    };
    let Some(mut bed) = Bed::start(Some(setup), &[(WLAN, false)])? else {
        return Ok(());
    };
    let file = bed.fakes.dir.path().join("home/Maison.ovpn");
    std::fs::write(&file, "client\nremote vpn.example.org 1194\n")?;
    let (_portal, _) = bed.chooser(vec![file])?;
    bed.open()?;
    bed.click("network:import")?;
    bed.shows(|s| {
        s["toast"]
            == "Le module OpenVPN de NetworkManager n’est pas installé (network-manager-openvpn)."
    })?;
    Ok(())
}

#[test]
fn a_vpn_switch_activates_and_deactivates_it() -> anyhow::Result<()> {
    let setup = Setup {
        vpns: vec![("Travail", "wireguard", false), ("Panne", "openvpn", true)],
        ..home()
    };
    let Some(mut bed) = Bed::start(Some(setup), &[(WLAN, false)])? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(names(&shown["network"]["vpns"]), ["Panne", "Travail"]);
    bed.click("vpn-switch:Travail")?;
    bed.asked(&Call::Activate {
        ssid: "Travail".to_owned(),
    })?;
    let on = bed.shows(|s| find(&s["network"]["vpns"], "Travail")["on"] == true)?;
    let line = find(&on["network"]["vpns"], "Travail")["line"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        line.starts_with("Connecté depuis ") && line.ends_with(" · WireGuard · 10.8.0.6"),
        "{line}"
    );
    bed.click("vpn-switch:Travail")?;
    bed.asked(&Call::Deactivate)?;
    bed.shows(|s| find(&s["network"]["vpns"], "Travail")["line"] == "WireGuard · coupé")?;
    // One that fails says why.
    bed.click("vpn-switch:Panne")?;
    bed.shows(|s| {
        find(&s["network"]["vpns"], "Panne")["line"]
            == "La connexion a échoué : le serveur ne répond pas"
    })?;
    // Its menu: renamed in place.
    bed.click("vpn-menu:Travail")?;
    bed.shows(|s| s["network"]["vpn_menu"] == true)?;
    bed.click("option:0")?;
    bed.shows(|s| s["network"]["renaming"] == "Travail")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("boulot")?;
    bed.press(KEY_ENTER);
    bed.shows(|s| find(&s["network"]["vpns"], "boulot").is_object())?;
    // Deleted, and back with "Annuler".
    bed.click("vpn-menu:boulot")?;
    bed.shows(|s| s["network"]["vpn_menu"] == true)?;
    bed.click("option:2")?;
    bed.asked(&Call::Delete {
        ssid: "boulot".to_owned(),
    })?;
    bed.shows(|s| s["toast"] == "VPN « boulot » supprimé.")?;
    bed.click("toast-action")?;
    bed.shows(|s| {
        s["toast"] == "Annulé : VPN « boulot » revient."
            && find(&s["network"]["vpns"], "boulot").is_object()
    })?;
    Ok(())
}

#[test]
fn the_popovers_dans_les_reglages_row_opens_the_sheet_for_that_network() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_with_shell(home(), &[(WLAN, false)])? else {
        return Ok(());
    };
    let popover = bed.wifi_popover()?;
    // eduroam, third under Maison: 802.1X, "dans les Réglages".
    common::click_on(
        &mut bed.hp,
        popover_row(popover, 2),
        hocus_link::POPOVER_NAMESPACE,
    )?;
    bed.shell_logged("the Settings open on a network's sheet")?;
    let open = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
    })?;
    anyhow::ensure!(open, "the Settings never opened: {}", bed.plain());
    bed.shows(|s| s["page"] == "network" && s["network"]["sheet"]["name"] == "eduroam")?;
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the popover stayed open");
    // "Réglages du Wi-Fi…", the popover's last row.
    bed.press(KEY_ESC);
    bed.shows(|s| s["network"]["sheet"].is_null())?;
    bed.hp.pointer_move(5.0, 5.0);
    let popover = bed.wifi_popover()?;
    let link = (
        f64::from(popover.x + popover.width / 2),
        f64::from(popover.y + popover.height - 15 - 18),
    );
    common::click_on(&mut bed.hp, link, hocus_link::POPOVER_NAMESPACE)?;
    bed.shell_logged("section=\"network\"")?;
    Ok(())
}

#[test]
fn in_airplane_mode_the_bar_says_it_and_its_switch_turns_the_wifi_on_alone() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start_with_shell(home(), &[(WLAN, true), (BLUETOOTH, true)])? else {
        return Ok(());
    };
    // Every radio blocked: the row says "Mode avion".
    bed.shell_logged("link=Airplane")?;
    let popover = bed.wifi_popover()?;
    let _ = popover;
    // The switch (the first stop): the Wi-Fi's radio alone unblocked.
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.until("the Wi-Fi unblocked", |bed| {
        bed.rfkill.soft(WLAN) == Some(false)
    })?;
    assert_eq!(bed.rfkill.soft(BLUETOOTH), Some(true));
    assert_eq!(bed.rfkill.asked(), [(WLAN, false)]);
    Ok(())
}

/// The second part's pictures, at ×2: `HOCUS_SHOTS=DIR`.
#[test]
#[ignore = "pictures for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_sheets() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start_on(
        Some(home()),
        &[(WLAN, false), (BLUETOOTH, false)],
        "2880x1800@2",
    )?
    else {
        return Ok(());
    };
    let dir = bed.fakes.dir.path().join("home");
    let (certificate, key) = (dir.join("alex.crt"), dir.join("alex.key"));
    for file in [&certificate, &key] {
        std::fs::write(file, "-----BEGIN-----\n")?;
    }
    let (_portal, _) = bed.chooser(vec![certificate, key])?;
    bed.nm().set_wired(true)?;
    bed.open()?;
    bed.shows(|s| s["network"]["scanning"] == false && s["network"]["wired"]["connected"] == true)?;
    bed.scroll_to_show("row:network.cable")?;
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-cable.png")?;
    // eduroam's sheet, filled in.
    bed.click("connect:eduroam")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "eduroam")?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("alex.martin")?;
    for (at, field, text) in [
        ("control:1", "password", "motdepasse"),
        ("control:2", "domain", "radius.univ.fr"),
    ] {
        bed.press(KEY_TAB);
        bed.shows(|s| s["focus"] == at)?;
        wait(&mut bed.hp, Duration::from_millis(100))?;
        bed.type_text(text)?;
        bed.shows(|s| {
            s["network"]["sheet"]["typed"]
                .as_array()
                .is_some_and(|t| t.iter().any(|f| f == field))
        })?;
    }
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-eduroam.png")?;
    // TLS, its files chosen, the authority not checked.
    bed.pick("method", "TLS", 2)?;
    bed.shows(|s| s["network"]["sheet"]["method"] == "TLS")?;
    bed.click("sheet:certificate")?;
    bed.shows(|s| s["network"]["sheet"]["certificate"] == true)?;
    bed.click("sheet:key")?;
    bed.shows(|s| s["network"]["sheet"]["key"] == true)?;
    bed.pick("authority", "Ne pas vérifier", 2)?;
    bed.shows(|s| s["network"]["sheet"]["authority"] == "Unchecked")?;
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-tls.png")?;
    bed.press(KEY_ESC);
    bed.shows(|s| s["network"]["sheet"].is_null())?;
    // A hidden network's sheet.
    bed.click("network:hidden")?;
    bed.shows(|s| s["network"]["sheet"]["hidden"] == true)?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    bed.type_text("labocache")?;
    bed.shows(|s| s["network"]["sheet"]["name"] == "labocache")?;
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-hidden.png")?;
    Ok(())
}

/// The window at ×2, once settled (or a caret blinking), into `out`.
fn shot(bed: &mut Bed, out: &std::path::Path, name: &str) -> anyhow::Result<()> {
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

/// The third part's pictures, at ×2: `HOCUS_SHOTS=DIR`.
#[test]
#[ignore = "pictures for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_vpns_and_the_bar() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let setup = Setup {
        vpns: vec![
            ("Travail", "wireguard", false),
            ("Serveur perso", "openvpn", false),
            ("Panne", "openvpn", true),
        ],
        ..home()
    };
    let Some(mut bed) =
        Bed::start_with_shell_on(setup, &[(WLAN, false), (BLUETOOTH, false)], "3840x2160@2")?
    else {
        return Ok(());
    };
    // The bar's popover: "dans les Réglages" and "Réglages du Wi-Fi…".
    let popover = bed.wifi_popover()?;
    bed.hp.pointer_move(5.0, 5.0);
    common::at_rest(&mut bed.hp)?;
    let around = |bed: &mut Bed, rect: Rect, name: &str| -> anyhow::Result<()> {
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y) = (
            (rect.x * 2 - 24).max(0) as u32,
            (rect.y * 2 - 24).max(0) as u32,
        );
        let image = image::imageops::crop_imm(
            &image,
            x,
            y,
            ((rect.width * 2 + 48) as u32).min(w as u32 - x),
            ((rect.height * 2 + 48) as u32).min(h as u32 - y),
        )
        .to_image();
        image.save(out.join(name))?;
        eprintln!("{}", out.join(name).display());
        Ok(())
    };
    around(&mut bed, popover, "popover-wifi.png")?;
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the popover stayed open");
    // The VPNs: one on, one failed.
    bed.open()?;
    bed.click("vpn-switch:Travail")?;
    bed.shows(|s| find(&s["network"]["vpns"], "Travail")["on"] == true)?;
    bed.click("vpn-switch:Panne")?;
    bed.shows(|s| {
        find(&s["network"]["vpns"], "Panne")["line"]
            .as_str()
            .is_some_and(|l| l.starts_with("La connexion a échoué"))
    })?;
    bed.scroll_to_show("network:import")?;
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-vpns.png")?;
    bed.click("vpn-menu:Serveur perso")?;
    bed.shows(|s| s["network"]["vpn_menu"] == true)?;
    bed.hp.pointer_move(5.0, 5.0);
    shot(&mut bed, &out, "network-vpn-menu.png")?;
    bed.press(KEY_ESC);
    bed.shows(|s| s["network"]["vpn_menu"] == false)?;
    // Airplane mode: the bar's row says it.
    bed.scroll_to_show("switch:network.airplane")?;
    bed.click("switch:network.airplane")?;
    bed.shell_logged("link=Airplane")?;
    bed.hp.pointer_move(5.0, 5.0);
    common::at_rest(&mut bed.hp)?;
    let row = bed
        .hp
        .connections_surfaces()
        .first()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow::anyhow!("no connections"))?;
    let popover = bed.wifi_popover()?;
    bed.hp.pointer_move(5.0, 5.0);
    common::at_rest(&mut bed.hp)?;
    let both = Rect::new(
        popover.x.min(row.x),
        popover.y.min(row.y),
        (popover.x + popover.width).max(row.x + row.width) - popover.x.min(row.x),
        (popover.y + popover.height).max(row.y + row.height) - popover.y.min(row.y),
    );
    around(&mut bed, both, "bar-airplane.png")?;
    Ok(())
}
