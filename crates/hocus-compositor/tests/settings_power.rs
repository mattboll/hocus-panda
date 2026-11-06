//! The Settings' Énergie page (6.10, issue #410, variant A): the real
//! `hocus-settings` window, a client of a headless compositor, its file,
//! state, home and runtime directories the test's own; its system bus a
//! private `dbus-daemon` with a fake UPower (the display device, the
//! battery's health, `OnBattery`, `LidIsPresent`), power-profiles-daemon
//! and logind: never the machine's.
//!
//! One test per point of the approved spec: the page in 7.2's order with
//! its timeline following the delays and the segmented control; the
//! battery's card; locking off changing the screen's line, the dim never
//! leaving the screen's row inert; the power mode chosen, toasted and
//! undone; without power-profiles-daemon, the modes inert; without a
//! battery, one set of delays and no lid; without logind, the system
//! decides; a file 6.1 wrote showing its battery's dim; at rest, nothing
//! drawn. What the window shows is read from its log ("shows" lines).

mod common;
mod settings_page;

use settings_page::{Floor, Wheel};
use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use common::{BTN_LEFT, KEY_LEFTCTRL, WAIT, chord, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const KEY_Z: u32 = 44;

// --- The fakes ------------------------------------------------------------------------

/// What the fake services say.
#[derive(Debug, Clone)]
struct Services {
    on_battery: bool,
    lid: bool,
    /// The display device: present, percent, UPower's state, seconds to
    /// empty, to full.
    battery: Option<(f64, u32, i64, i64)>,
    capacity: f64,
    /// `None`: no power-profiles-daemon.
    profiles: Option<(String, Vec<&'static str>, String)>,
    logind: bool,
}

impl Services {
    /// A laptop on battery at 82 %, 3 h 40 left, 91 % of its capacity.
    fn laptop() -> Self {
        Self {
            on_battery: true,
            lid: true,
            battery: Some((82.0, 2, 3 * 3600 + 40 * 60, 0)),
            capacity: 91.0,
            profiles: Some((
                "balanced".into(),
                vec!["power-saver", "balanced", "performance"],
                String::new(),
            )),
            logind: true,
        }
    }

    /// A desktop: no battery, no lid.
    fn desktop() -> Self {
        Self {
            on_battery: false,
            lid: false,
            battery: None,
            ..Self::laptop()
        }
    }
}

#[derive(Debug)]
struct Held {
    services: Services,
    modes_set: Vec<String>,
}

type Shared = Arc<Mutex<Held>>;

fn held(shared: &Shared) -> std::sync::MutexGuard<'_, Held> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

struct UPower(Shared);

#[zbus::interface(name = "org.freedesktop.UPower")]
impl UPower {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        held(&self.0).services.on_battery
    }

    #[zbus(property)]
    fn lid_is_present(&self) -> bool {
        held(&self.0).services.lid
    }

    fn enumerate_devices(&self) -> Vec<zbus::OwnedObjectPath> {
        if held(&self.0).services.battery.is_none() {
            return Vec::new();
        }
        zbus::OwnedObjectPath::try_from("/org/freedesktop/UPower/devices/battery_BAT0")
            .into_iter()
            .collect()
    }
}

struct Display(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Display {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        held(&self.0).services.battery.is_some()
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        held(&self.0).services.battery.map_or(0.0, |b| b.0)
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        held(&self.0).services.battery.map_or(0, |b| b.1)
    }

    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        held(&self.0).services.battery.map_or(0, |b| b.2)
    }

    #[zbus(property)]
    fn time_to_full(&self) -> i64 {
        held(&self.0).services.battery.map_or(0, |b| b.3)
    }
}

struct Cell(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Cell {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        2
    }

    #[zbus(property)]
    fn power_supply(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn capacity(&self) -> f64 {
        held(&self.0).services.capacity
    }
}

struct Profiles(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        held(&self.0)
            .services
            .profiles
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) {
        let mut held = held(&self.0);
        held.modes_set.push(profile.clone());
        if let Some(p) = held.services.profiles.as_mut() {
            p.0 = profile;
        }
    }

    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, zbus::OwnedVariant>> {
        held(&self.0)
            .services
            .profiles
            .as_ref()
            .map(|p| {
                p.1.iter()
                    .filter_map(|name| {
                        let value =
                            zbus::OwnedVariant::try_from(zbus::Variant::from(*name)).ok()?;
                        Some(HashMap::from([("Profile".to_owned(), value)]))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn performance_degraded(&self) -> String {
        held(&self.0)
            .services
            .profiles
            .as_ref()
            .map(|p| p.2.clone())
            .unwrap_or_default()
    }
}

struct Login;

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Login {
    fn can_suspend(&self) -> String {
        "yes".into()
    }
}

/// A system bus of the test's own, the fakes on it.
struct System {
    daemon: std::process::Child,
    address: String,
    runtime: tokio::runtime::Runtime,
    shared: Shared,
    upower: zbus::Connection,
    profiles: Option<zbus::Connection>,
    _others: Vec<zbus::Connection>,
}

impl System {
    fn start(dir: &Path, services: Services) -> anyhow::Result<Self> {
        let dir = dir.join("system-bus");
        std::fs::create_dir_all(&dir)?;
        let (daemon, address) = common::private_bus(&dir)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let with_profiles = services.profiles.is_some();
        let logind = services.logind;
        let shared: Shared = Arc::new(Mutex::new(Held {
            services,
            modes_set: Vec::new(),
        }));
        let builder = || zbus::connection::Builder::address(address.as_str());
        let upower = runtime.block_on(
            builder()
                .name("org.freedesktop.UPower")
                .serve_at("/org/freedesktop/UPower", UPower(shared.clone()))
                .serve_at(
                    "/org/freedesktop/UPower/devices/DisplayDevice",
                    Display(shared.clone()),
                )
                .serve_at(
                    "/org/freedesktop/UPower/devices/battery_BAT0",
                    Cell(shared.clone()),
                )
                .build(),
        )?;
        let profiles = if with_profiles {
            Some(
                runtime.block_on(
                    builder()
                        .name("org.freedesktop.UPower.PowerProfiles")
                        .serve_at(
                            "/org/freedesktop/UPower/PowerProfiles",
                            Profiles(shared.clone()),
                        )
                        .build(),
                )?,
            )
        } else {
            None
        };
        let mut others = Vec::new();
        if logind {
            others.push(
                runtime.block_on(
                    builder()
                        .name("org.freedesktop.login1")
                        .serve_at("/org/freedesktop/login1", Login)
                        .build(),
                )?,
            );
        }
        Ok(Self {
            daemon,
            address,
            runtime,
            shared,
            upower,
            profiles,
            _others: others,
        })
    }

    /// What the services say, changed, and a signal that it did.
    fn change(&self, change: impl FnOnce(&mut Services)) -> anyhow::Result<()> {
        change(&mut held(&self.shared).services);
        let upower = self.upower.clone();
        self.runtime.block_on(async move {
            upower
                .emit_signal(
                    None::<&str>,
                    "/org/freedesktop/UPower",
                    "org.freedesktop.DBus.Properties",
                    "PropertiesChanged",
                    &(
                        "org.freedesktop.UPower",
                        HashMap::<&str, zbus::Variant<'_>>::new(),
                        Vec::<&str>::new(),
                    ),
                )
                .await
        })?;
        Ok(())
    }

    fn modes_set(&self) -> Vec<String> {
        held(&self.shared).modes_set.clone()
    }
}

impl Drop for System {
    fn drop(&mut self) {
        drop(self.profiles.take());
        // The daemon this test started, by its own PID.
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

// --- The bed --------------------------------------------------------------------------

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
    system: Option<System>,
}

impl Bed {
    /// A compositor, the file holding `file`, the system bus's services
    /// as `services` say (`None`: no system bus at all).
    fn start(file: &str, services: Option<Services>) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        let system = services.map(|s| System::start(root, s)).transpose()?;
        let bus = system.as_ref().map_or_else(
            || "unix:path=/nonexistent".to_owned(),
            |s| s.address.clone(),
        );
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{bus}' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
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
                outputs: vec!["1920x1080".parse()?],
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
            system,
        }))
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("config/hocuspanda/config.toml")
    }

    fn plain(&self) -> String {
        common::logged(&self.log)
    }

    /// Opened on the Énergie page, the system bus heard.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "power".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        self.shows(|s| s["page"] == "power" && s["file"] == "saved" && !s["power"].is_null())
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
                "the window never showed it; it shows {}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string())
            )
        })
    }

    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " places {") };
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|p| p.get(id).is_some());
            seen.is_some()
        })?;
        seen.ok_or_else(|| anyhow::anyhow!("{id} is nowhere: {:?}", read(&log)))
    }

    /// `id` scrolled into view, clear of the toast, by the wheel.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let wheel = Wheel {
            x: 600.0,
            notches: 2,
            top: 56.0,
            floor: Floor::Margin(90.0),
        };
        settings_page::reveal(&mut self.hp, &self.log, APP_ID, id, &wheel)
    }

    fn window(&self) -> anyhow::Result<hocus_compositor::model::Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("option:") {
            self.reveal(id)?;
        }
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self
            .hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))?;
        let (x, y) = (
            f64::from(window.x) + n(0) + n(2) / 2.0,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        );
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str().is_some_and(|m| m.starts_with(setting)))?;
        self.click(&format!("option:{n}"))?;
        Ok(())
    }

    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.file();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(seen, "the file never held it:\n{text}");
        Ok(text)
    }
}

fn row<'a>(shown: &'a Value, id: &str) -> &'a Value {
    shown["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .unwrap_or(&Value::Null)
}

fn ids(shown: &Value) -> Vec<String> {
    shown["rows"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|r| r["id"].as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn timeline(shown: &Value) -> Vec<(String, String)> {
    shown["power"]["timeline"]
        .as_array()
        .map(|steps| {
            steps
                .iter()
                .map(|s| {
                    (
                        s["kind"].as_str().unwrap_or_default().to_owned(),
                        s["time"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

// --- The tests ------------------------------------------------------------------------

/// On battery: the card (82 %, its line, its health), the segmented
/// control on "Sur batterie", the rows in 7.2's order and the timeline
/// under them; a delay changed: written at once, the timeline follows;
/// "Sur secteur": AC's delays and their timeline.
#[test]
fn the_page_follows_7_2s_order_and_its_timeline_follows_the_delays() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("", Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| !s["power"]["battery"].is_null() && s["source"] == "battery")?;
    let battery = &shown["power"]["battery"];
    assert_eq!(battery["percent"], 82);
    assert_eq!(battery["line"], "Sur batterie · 3 h 40 restantes");
    assert_eq!(battery["health"], 91);
    assert_eq!(shown["power"]["segmented"], true);
    assert_eq!(
        ids(&shown),
        [
            "power.idle",
            "power.screen-off-after",
            "power.suspend-after",
            "power.lock-on-idle",
            "power.lock-before-sleep",
            "power.lid",
            "power.lid-ignored-when-docked",
            "power.power-button",
            "power.auto-saver",
            "power.auto-saver-below",
        ]
    );
    assert_eq!(row(&shown, "power.idle")["value"], "3 min");
    assert_eq!(row(&shown, "power.screen-off-after")["value"], "10 s après");
    assert_eq!(
        timeline(&shown),
        pairs(&[
            ("dernière activité", "0"),
            ("assombri", "3 min"),
            ("verrouillé", "+10 s"),
            ("écran éteint", "+10 s"),
            ("en veille", "15 min"),
        ])
    );
    let at: Vec<f64> = shown["power"]["timeline"]
        .as_array()
        .map(|s| s.iter().filter_map(|s| s["at"].as_f64()).collect())
        .unwrap_or_default();
    assert_eq!(at, [0.0, 20.0, 32.0, 44.0, 100.0]);
    // 5 min on battery: written in the battery's table, the timeline too.
    bed.choose("power.idle", 3)?;
    bed.written(|t| t.contains("[power.battery]") && t.contains("idle = \"5m\""))?;
    let shown = bed.shows(|s| timeline(s).get(1).is_some_and(|d| d.1 == "5 min"))?;
    assert_eq!(row(&shown, "power.idle")["value"], "5 min");
    // On AC: AC's delays, never asleep.
    bed.click("source:ac")?;
    let shown = bed.shows(|s| s["source"] == "ac")?;
    assert_eq!(
        timeline(&shown),
        pairs(&[
            ("dernière activité", "0"),
            ("assombri", "5 min"),
            ("verrouillé", "+10 s"),
            ("écran éteint", "+30 s"),
            ("pas de veille", "jamais"),
        ])
    );
    Ok(())
}

/// "Verrouiller après l'assombrissement" off: the screen's line says
/// after the dimming, the timeline has no lock; the dim "Jamais": the
/// screen's row inert, only the sleep on the timeline.
#[test]
fn locking_off_and_the_dim_never_change_the_screens_row() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("", Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["source"] == "battery")?;
    bed.click("control:power.lock-on-idle")?;
    bed.written(|t| t.contains("lock-on-idle = false"))?;
    let shown = bed.shows(|s| {
        s["power"]["lines"]
            .as_array()
            .is_some_and(|l| l.iter().any(|l| l[0] == "power.screen-off-after"))
    })?;
    let lines = shown["power"]["lines"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        lines.iter().any(
            |l| l[1] == "Après l’assombrissement, s’il ne se passe toujours rien." && l[2] == false
        ),
        "{lines:?}"
    );
    assert_eq!(
        timeline(&shown),
        pairs(&[
            ("dernière activité", "0"),
            ("assombri", "3 min"),
            ("écran éteint", "+20 s"),
            ("en veille", "15 min"),
        ])
    );
    bed.choose("power.idle", 7)?;
    let shown = bed.shows(|s| row(s, "power.idle")["value"] == "Jamais")?;
    assert_eq!(row(&shown, "power.screen-off-after")["inert"], true);
    assert_eq!(
        timeline(&shown),
        pairs(&[("dernière activité", "0"), ("en veille", "15 min")])
    );
    Ok(())
}

/// Waits until power-profiles-daemon was asked for `want`, in order: the
/// page shows a mode and its toast as the ask leaves, and the ask reaches
/// the daemon from the page's own thread a moment later.
fn modes_set_become(bed: &mut Bed, want: &[&str]) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let set = bed
            .system
            .as_ref()
            .map(System::modes_set)
            .unwrap_or_default();
        if set == want {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "power-profiles-daemon was asked {set:?}, not {want:?}"
        );
        wait(&mut bed.hp, Duration::from_millis(20))?;
    }
}

/// The power mode: the three modes, the active one chosen; a click asks
/// power-profiles-daemon and toasts "Mode d'énergie : Économie" with
/// "Annuler"; `Ctrl+Z` gives the mode back. Nothing goes in the file.
#[test]
fn the_power_mode_switches_with_its_toast_and_ctrl_z() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("", Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["power"]["modes"]["active"] == "balanced")?;
    assert_eq!(
        shown["power"]["modes"]["available"],
        serde_json::json!(["power-saver", "balanced", "performance"])
    );
    bed.click("mode:power-saver")?;
    let shown = bed.shows(|s| s["toast"] == "Mode d’énergie : Économie")?;
    assert_eq!(shown["toast_action"], "undo");
    modes_set_become(&mut bed, &["power-saver"])?;
    bed.shows(|s| s["power"]["modes"]["active"] == "power-saver")?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    bed.shows(|s| {
        s["power"]["modes"]["active"] == "balanced"
            && s["toast"] == "Annulé : Mode d’énergie : Équilibré."
    })?;
    modes_set_become(&mut bed, &["power-saver", "balanced"])?;
    assert!(!std::fs::read_to_string(bed.file())?.contains("profile"));
    // A mode the machine lacks: said, not offered; performance held back:
    // its reason.
    if let Some(system) = &bed.system {
        system.change(|s| {
            s.profiles = Some((
                "balanced".into(),
                vec!["power-saver", "balanced"],
                "high-operating-temperature".into(),
            ));
        })?;
    }
    bed.shows(|s| {
        s["power"]["modes"]["available"] == serde_json::json!(["power-saver", "balanced"])
    })?;
    Ok(())
}

/// Without power-profiles-daemon: the mode's line says it is missing,
/// the automatic saver and its slider inert, with their line.
#[test]
fn without_power_profiles_daemon_the_modes_are_inert() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(
        "",
        Some(Services {
            profiles: None,
            ..Services::laptop()
        }),
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| !s["power"]["battery"].is_null() && s["power"]["modes"].is_null())?;
    assert_eq!(row(&shown, "power.auto-saver")["inert"], true);
    assert_eq!(row(&shown, "power.auto-saver-below")["inert"], true);
    let lines = shown["power"]["lines"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        lines.iter().any(|l| l[0] == "power.auto-saver"
            && l[1] == "Elle passe par les modes : sans eux, rien à allumer."),
        "{lines:?}"
    );
    // Clicking a mode does nothing.
    bed.click("mode:power-saver")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let shown = bed.shows(|s| s["page"] == "power")?;
    assert!(shown["toast"].is_null(), "{shown}");
    Ok(())
}

/// A desktop: the card says "Sur secteur", no segmented control (AC's
/// delays), no lid's rows ("BOUTON" alone), only the mode in the power
/// saving group.
#[test]
fn the_page_without_a_battery_has_one_set_of_delays_and_no_lid() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("[idle]\ndelay = 600\n", Some(Services::desktop()))? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["power"]["lid"] == false && s["power"]["battery"].is_null())?;
    assert_eq!(shown["power"]["segmented"], false);
    assert_eq!(shown["source"], "ac");
    assert_eq!(
        ids(&shown),
        [
            "power.idle",
            "power.screen-off-after",
            "power.suspend-after",
            "power.lock-on-idle",
            "power.lock-before-sleep",
            "power.power-button",
        ]
    );
    assert_eq!(row(&shown, "power.idle")["value"], "10 min");
    bed.places_with("row:power.mode")?;
    Ok(())
}

/// Without logind, the lid's and the button's rows say the system
/// decides, in amber.
#[test]
fn without_logind_the_lid_and_the_button_say_the_system_decides() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(
        "",
        Some(Services {
            logind: false,
            ..Services::laptop()
        }),
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["power"]["logind"] == false && !s["power"]["battery"].is_null())?;
    let lines = shown["power"]["lines"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for id in ["power.lid", "power.power-button"] {
        assert!(
            lines.iter().any(|l| l[0] == id
                && l[1] == "Le système décide (logind indisponible)."
                && l[2] == true),
            "{id}: {lines:?}"
        );
    }
    Ok(())
}

/// A file 6.1 wrote: its battery's `screen-off` shows as the dim, its
/// `lock-after = "never"` everywhere as locking off; changing the dim
/// writes `idle`, the old key left alone.
#[test]
fn a_file_written_by_6_1_shows_its_battery_dim_and_its_lock() -> anyhow::Result<()> {
    let file = "[power.battery]\nscreen-off = \"2m\"\nlock-after = \"never\"\n\
                [power.ac]\nlock-after = \"never\"\n";
    let Some(mut bed) = Bed::start(file, Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    let shown =
        bed.shows(|s| s["source"] == "battery" && row(s, "power.idle")["value"] == "2 min")?;
    assert_eq!(row(&shown, "power.lock-on-idle")["value"], "non");
    bed.choose("power.idle", 0)?;
    let text = bed.written(|t| t.contains("idle = \"1m\""))?;
    assert!(text.contains("screen-off = \"2m\""), "{text}");
    Ok(())
}

/// At rest, with its services heard, the page draws nothing.
#[test]
fn at_rest_the_page_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start("", Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| !s["power"]["modes"].is_null() && !s["power"]["battery"].is_null())?;
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

/// The window, cropped from the screen, to `out/name.png`, once it is
/// still.
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
    // A laptop on battery: the top, then the bottom.
    let Some(mut bed) = Bed::start("", Some(Services::laptop()))? else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| !s["power"]["modes"].is_null() && s["source"] == "battery")?;
    bed.hp.pointer_move(1500.0, 900.0);
    shoot(&mut bed, &out, "power-battery-top")?;
    bed.reveal("row:power.charge-limit")?;
    shoot(&mut bed, &out, "power-battery-bottom")?;
    drop(bed);
    // Locking off, on AC.
    let Some(mut bed) = Bed::start(
        "[power]\nlock-on-idle = false\n",
        Some(Services {
            on_battery: false,
            battery: Some((64.0, 1, 0, 25 * 60)),
            ..Services::laptop()
        }),
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["source"] == "ac" && !s["power"]["battery"].is_null())?;
    bed.hp.pointer_move(1500.0, 900.0);
    shoot(&mut bed, &out, "power-ac-locking-off")?;
    drop(bed);
    // A desktop without power-profiles-daemon nor logind.
    let Some(mut bed) = Bed::start(
        "",
        Some(Services {
            profiles: None,
            logind: false,
            ..Services::desktop()
        }),
    )?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| s["power"]["logind"] == false)?;
    bed.hp.pointer_move(1500.0, 900.0);
    shoot(&mut bed, &out, "power-desktop-top")?;
    bed.reveal("row:power.charge-limit")?;
    shoot(&mut bed, &out, "power-desktop-bottom")?;
    Ok(())
}
