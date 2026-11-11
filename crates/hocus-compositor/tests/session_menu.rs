//! The session menu (7.4, issue #415, variant A): the footer's user's
//! popover grows into it. The real shell on the CPU, its user named Alex,
//! its home, configuration, cache and state directories the test's own, no
//! session bus; the compositor's system bus a private `dbus-daemon` with a
//! fake logind (and polkit) that records what it is asked and powers
//! nothing off. Never the machine's logind.
//!
//! One scenario per point of the approved spec: the popover's rows; lock;
//! suspend after the lock; the 10 s countdown and logind's `PowerOff` at
//! its end; "Annuler"; "Éteindre maintenant"; an inhibitor's reason, and
//! the countdown again once it lets go; a window that does not close,
//! listed and closed all the same; `Super+Shift+E`; logging out ending the
//! compositor once the windows close; the keyboard on "Annuler"; the
//! password asked first when logind says `challenge`; and at rest, nothing
//! drawn. Times are the motions' clock, held and stepped, where they count.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use common::{BTN_LEFT, Command, Event, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB, WAIT, wait};
use hocus_compositor::{Config, Ending, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{HolderKind, LeaveAction, LeaveStep};
use zbus::{Connection, OwnedVariant, connection::Builder};

const KEY_E: u32 = 18;
const KEY_L: u32 = 38;
const KEY_ENTER: u32 = 28;

// --- The fake logind and polkit --------------------------------------------------

/// What the fake logind says, and what it was asked.
#[derive(Debug, Clone)]
struct Logind {
    can_suspend: &'static str,
    can_reboot: &'static str,
    can_power_off: &'static str,
    /// `what`, `who`, `why`, `mode`, `uid`, `pid`.
    inhibitors: Vec<(String, String, String, String, u32, u32)>,
    /// The power calls, and their `interactive`.
    calls: Vec<(&'static str, bool)>,
    /// polkit's `CheckAuthorization`: the action and its flags.
    authorizations: Vec<(String, u32)>,
}

impl Default for Logind {
    fn default() -> Self {
        Self {
            can_suspend: "yes",
            can_reboot: "yes",
            can_power_off: "yes",
            inhibitors: Vec::new(),
            calls: Vec::new(),
            authorizations: Vec::new(),
        }
    }
}

type Shared = Arc<Mutex<Logind>>;

fn logind(shared: &Shared) -> std::sync::MutexGuard<'_, Logind> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Manager(Shared);

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn can_suspend(&self) -> String {
        logind(&self.0).can_suspend.to_owned()
    }

    fn can_reboot(&self) -> String {
        logind(&self.0).can_reboot.to_owned()
    }

    fn can_power_off(&self) -> String {
        logind(&self.0).can_power_off.to_owned()
    }

    fn list_inhibitors(&self) -> Vec<(String, String, String, String, u32, u32)> {
        logind(&self.0).inhibitors.clone()
    }

    fn power_off(&self, interactive: bool) {
        logind(&self.0).calls.push(("PowerOff", interactive));
    }

    fn reboot(&self, interactive: bool) {
        logind(&self.0).calls.push(("Reboot", interactive));
    }

    fn suspend(&self, interactive: bool) {
        logind(&self.0).calls.push(("Suspend", interactive));
    }
}

struct Authority(Shared);

#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Authority {
    fn check_authorization(
        &self,
        _subject: (String, HashMap<String, OwnedVariant>),
        action_id: String,
        _details: HashMap<String, String>,
        flags: u32,
        _cancellation_id: String,
    ) -> (bool, bool, HashMap<String, String>) {
        logind(&self.0).authorizations.push((action_id, flags));
        (true, false, HashMap::new())
    }
}

/// The private system bus and its fakes, stopped when dropped.
struct Fake {
    runtime: tokio::runtime::Runtime,
    bus: Option<Child>,
    address: String,
    shared: Shared,
    connections: Vec<Connection>,
}

impl Fake {
    fn start(dir: &Path, logind: Logind) -> anyhow::Result<Self> {
        let dir = dir.join("system-bus");
        std::fs::create_dir_all(&dir)?;
        let (bus, address) = common::private_bus(&dir)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let shared: Shared = Arc::new(Mutex::new(logind));
        let (login, polkit) = runtime.block_on({
            let (address, shared) = (address.clone(), shared.clone());
            async move {
                let login = Builder::address(address.as_str())
                    .name("org.freedesktop.login1")
                    .serve_at("/org/freedesktop/login1", Manager(shared.clone()))
                    .build()
                    .await?;
                let polkit = Builder::address(address.as_str())
                    .name("org.freedesktop.PolicyKit1")
                    .serve_at("/org/freedesktop/PolicyKit1/Authority", Authority(shared))
                    .build()
                    .await?;
                anyhow::Ok((login, polkit))
            }
        })?;
        Ok(Self {
            runtime,
            bus: Some(bus),
            address,
            shared,
            connections: vec![login, polkit],
        })
    }

    fn calls(&self) -> Vec<(&'static str, bool)> {
        logind(&self.shared).calls.clone()
    }

    fn change(&self, change: impl FnOnce(&mut Logind)) {
        change(&mut logind(&self.shared));
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let connections = std::mem::take(&mut self.connections);
        self.runtime.block_on(async move { drop(connections) });
        // The daemon this test started, by its own handle.
        if let Some(mut bus) = self.bus.take() {
            let _ = bus.kill();
            let _ = bus.wait();
        }
    }
}

// --- The bed ------------------------------------------------------------------------

struct Bed {
    hp: Headless,
    // Dropped after the compositor: its logind outlives it.
    fake: Fake,
    dir: tempfile::TempDir,
    log: PathBuf,
}

impl Bed {
    /// The compositor on a 1920 × 1080 screen, its shell's user named
    /// Alex, logind saying `logind`.
    fn start(logind: Logind) -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let fake = Fake::start(root, logind)?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let at = |sub: &str| root.join(sub).display().to_string();
        // The shell's own system bus is none: no AccountsService (the
        // panda), no polkit agent.
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_on_buses(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
            None,
            Some(fake.address.clone()),
        )?;
        let mut bed = Self { hp, fake, dir, log };
        let up = bed.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::BELL_NAMESPACE).len() == 1
        })?;
        anyhow::ensure!(up, "no footer");
        bed.hp.pointer_move(400.0, 300.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        plain(&self.log)
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for the shell to log `text` after `from`: the line.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        let after = plain(&log).get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(done, "the shell never logged {text:?}; it logged:\n{after}");
        Ok(after
            .lines()
            .rfind(|l| l.contains(text))
            .unwrap_or_default()
            .to_owned())
    }

    /// The footer's user's button, its centre.
    fn avatar(&self) -> (f64, f64) {
        let f = self.hp.drawn_layers(hocus_link::BELL_NAMESPACE)[0].1;
        (f64::from(f.x + 11 + 44 * 3 + 20), f64::from(f.y + 6 + 17))
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        common::point(&mut self.hp, (x, y))?;
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// The menu opened by a click on the avatar, once drawn: its rect.
    fn open_menu(&mut self) -> anyhow::Result<Rect> {
        let mark = self.mark();
        let avatar = self.avatar();
        self.click(avatar)?;
        let popover = self.popover()?;
        self.logged_after(mark, "shows=rows")?;
        Ok(popover)
    }

    /// The popover open, once it is there and drawn.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(open, "no popover: {:?}", self.hp.popover_surfaces());
        common::at_rest(&mut self.hp)?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// The popover's height, once it is `height` (it changes as the menu
    /// shows a confirmation).
    fn popover_of(&mut self, height: i32) -> anyhow::Result<Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            common::drawn_popovers(hp)
                .first()
                .is_some_and(|r| r.height == height)
        })?;
        anyhow::ensure!(done, "the popover: {:?}", self.hp.popover_surfaces());
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// The centre of the row at `top` (its 44 px from there) of the menu
    /// at `menu`.
    fn row(menu: Rect, top: i32) -> (f64, f64) {
        (f64::from(menu.x + 9 + 131), f64::from(menu.y + top + 22))
    }

    /// The confirmation's button at `top` (36 px high) in `menu`.
    fn button(menu: Rect, top: i32) -> (f64, f64) {
        (f64::from(menu.x + 15 + 126), f64::from(menu.y + top + 18))
    }

    /// Waits until logind was called.
    fn called(&mut self, call: (&'static str, bool)) -> anyhow::Result<()> {
        let shared = self.fake.shared.clone();
        let done = self
            .hp
            .dispatch_until(WAIT, |_| logind(&shared).calls.contains(&call))?;
        anyhow::ensure!(
            done,
            "logind was not called {call:?}: {:?}",
            self.fake.calls()
        );
        Ok(())
    }

    fn step(&mut self, by: Duration) -> anyhow::Result<()> {
        // A second at a time at most: the countdown is told each.
        let mut left = by;
        while !left.is_zero() {
            let one = left.min(Duration::from_millis(500));
            self.hp.step(one)?;
            left -= one;
        }
        Ok(())
    }

    /// The pixel at (`x`, `y`): red, green, blue.
    fn pixel(&mut self, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The menu's rows' tops in the popover (its layout's): Réglages; a
/// divider; Verrouiller, Mettre en veille; a divider; Se déconnecter,
/// Redémarrer, Éteindre.
const SETTINGS: i32 = 59;
const LOCK: i32 = SETTINGS + 44 + 13;
const SUSPEND: i32 = LOCK + 44;
const LOG_OUT: i32 = SUSPEND + 44 + 13;
const RESTART: i32 = LOG_OUT + 44;
const SHUT_DOWN: i32 = RESTART + 44;
const MENU_HEIGHT: i32 = SHUT_DOWN + 44 + 15;
/// The confirmation's: the bar's top, "… maintenant", "Annuler"; its
/// height.
const BAR: i32 = 131;
const NOW: i32 = 145;
const CANCEL: i32 = NOW + 36 + 6;
const CONFIRM_HEIGHT: i32 = CANCEL + 36 + 15;

fn amber([r, g, b]: [u8; 3]) -> bool {
    r > 200 && g > 150 && b < 150
}

#[test]
fn the_popover_shows_the_menus_rows_under_the_avatar_and_the_name() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    assert_eq!(menu.width, 280);
    assert_eq!(menu.height, MENU_HEIGHT);
    assert_eq!(menu.y + menu.height, 1080 - 8, "its bottom on the bar's");
    let line = bed.logged_after(0, "shows=rows")?;
    assert!(line.contains("name=\"Alex\""), "{line}");
    assert!(
        line.contains(
            "[\"Réglages\", \"Verrouiller\", \"Mettre en veille\", \"Se déconnecter\", \
             \"Redémarrer\", \"Éteindre\"]"
        ),
        "{line}"
    );
    // The avatar: the panda on the accent's gradient (no AccountsService),
    // 32 px at the header's left.
    let [r, g, b] = bed.pixel((menu.x + 15 + 2, menu.y + 13 + 20))?;
    assert!(g > 120 && b > 100 && r < 140, "the avatar: {:?}", [r, g, b]);
    // logind says it cannot suspend nor power off: suspend goes, restart
    // and shut down are greyed.
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the popover stayed");
    bed.fake.change(|l| {
        l.can_suspend = "na";
        l.can_reboot = "no";
        l.can_power_off = "no";
    });
    let mark = bed.mark();
    let avatar = bed.avatar();
    bed.click(avatar)?;
    // Read again as it opens; logging out is always allowed.
    let line = bed.logged_after(
        mark,
        "[\"Réglages\", \"Verrouiller\", \"Se déconnecter\", \"(Redémarrer)\", \"(Éteindre)\"]",
    )?;
    assert!(!line.contains("Mettre en veille"), "{line}");
    Ok(())
}

#[test]
fn verrouiller_locks() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.click(Bed::row(menu, LOCK))?;
    let locked = bed.hp.dispatch_until(WAIT, |hp| {
        hp.lock_state() == hocus_compositor::LockState::Locked
    })?;
    anyhow::ensure!(locked, "Verrouiller did not lock");
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the menu stayed");
    assert!(bed.fake.calls().is_empty());
    Ok(())
}

#[test]
fn super_l_locks() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    common::chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_L]);
    let locked = bed.hp.dispatch_until(WAIT, |hp| {
        hp.lock_state() == hocus_compositor::LockState::Locked
    })?;
    anyhow::ensure!(locked, "Super+L did not lock");
    Ok(())
}

#[test]
fn mettre_en_veille_locks_then_suspends() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.click(Bed::row(menu, SUSPEND))?;
    // Locked at once; logind asked to suspend only once every screen
    // shows the lock.
    let shared = bed.fake.shared.clone();
    let mut drawn_before = None;
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        let called = logind(&shared).calls.contains(&("Suspend", true));
        if called && drawn_before.is_none() {
            drawn_before = Some(hp.lock_drawn());
        }
        called
    })?;
    anyhow::ensure!(
        done,
        "logind was never asked to suspend: {:?}",
        bed.fake.calls()
    );
    assert_eq!(bed.hp.lock_state(), hocus_compositor::LockState::Locked);
    assert_eq!(
        drawn_before,
        Some(true),
        "it suspended before the lock showed"
    );
    assert_eq!(bed.fake.calls(), [("Suspend", true)]);
    assert_eq!(bed.hp.ending(), None);
    Ok(())
}

#[test]
fn eteindre_counts_ten_seconds_down_then_powers_off() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    let mark = bed.mark();
    bed.click(Bed::row(menu, SHUT_DOWN))?;
    bed.logged_after(mark, "Éteindre ? « L’ordinateur s’éteint dans 10 s. »")?;
    let shown = bed.hp.leave_shown().expect("a confirmation");
    assert_eq!(shown.action, LeaveAction::ShutDown);
    assert_eq!(shown.step, LeaveStep::Counting { left_ms: 10_000 });
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    assert_eq!(confirm.y + confirm.height, 1080 - 8, "it grew upward");
    common::at_rest(&mut bed.hp)?;
    // The bar full: amber at its far end.
    let bar = |part: f64| (confirm.x + 15 + (250.0 * part) as i32, confirm.y + BAR + 2);
    assert!(amber(bed.pixel(bar(0.95))?), "{:?}", bed.pixel(bar(0.95))?);
    // Six seconds on: "4 s", the bar drained past its middle.
    let mark = bed.mark();
    bed.step(Duration::from_secs(6))?;
    bed.logged_after(mark, "« L’ordinateur s’éteint dans 4 s. »")?;
    common::at_rest(&mut bed.hp)?;
    assert!(!amber(bed.pixel(bar(0.6))?), "{:?}", bed.pixel(bar(0.6))?);
    assert!(amber(bed.pixel(bar(0.2))?), "{:?}", bed.pixel(bar(0.2))?);
    assert!(
        bed.fake.calls().is_empty(),
        "too early: {:?}",
        bed.fake.calls()
    );
    // At 10 s: no window to close, logind powers off, the session ends.
    bed.step(Duration::from_millis(3900))?;
    assert!(
        bed.fake.calls().is_empty(),
        "too early: {:?}",
        bed.fake.calls()
    );
    bed.step(Duration::from_millis(200))?;
    bed.called(("PowerOff", true))?;
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::PoweredOff));
    assert_eq!(bed.fake.calls(), [("PowerOff", true)]);
    Ok(())
}

#[test]
fn annuler_stops_the_countdown() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    bed.click(Bed::row(menu, SHUT_DOWN))?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    bed.step(Duration::from_secs(2))?;
    let mark = bed.mark();
    bed.click(Bed::button(confirm, CANCEL))?;
    bed.logged_after(mark, "shows=rows")?;
    assert_eq!(bed.hp.leave_shown(), None);
    // The menu's rows again, open.
    bed.popover_of(MENU_HEIGHT)?;
    bed.step(Duration::from_secs(12))?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert!(bed.fake.calls().is_empty(), "{:?}", bed.fake.calls());
    assert_eq!(bed.hp.ending(), None);
    Ok(())
}

#[test]
fn eteindre_maintenant_acts_at_once() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    bed.click(Bed::row(menu, SHUT_DOWN))?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    bed.click(Bed::button(confirm, NOW))?;
    // The clock held: nothing counted.
    bed.called(("PowerOff", true))?;
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::PoweredOff));
    Ok(())
}

#[test]
fn redemarrer_maintenant_reboots() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.click(Bed::row(menu, RESTART))?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    bed.click(Bed::button(confirm, NOW))?;
    bed.called(("Reboot", true))?;
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::Restarted));
    assert_eq!(bed.fake.calls(), [("Reboot", true)]);
    Ok(())
}

#[test]
fn an_inhibitors_reason_shows_and_holds_the_countdown() -> anyhow::Result<()> {
    let held = Logind {
        inhibitors: vec![
            (
                "shutdown:sleep".into(),
                "Firefox".into(),
                "Un téléchargement est en cours".into(),
                "block".into(),
                1000,
                4242,
            ),
            // Those logind waits for itself are not listed.
            (
                "shutdown".into(),
                "Mises à jour".into(),
                "Installation".into(),
                "delay".into(),
                0,
                1,
            ),
        ],
        ..Logind::default()
    };
    let Some(mut bed) = Bed::start(held)? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    let mark = bed.mark();
    bed.click(Bed::row(menu, SHUT_DOWN))?;
    let line = bed.logged_after(mark, "Firefox retient l’arrêt")?;
    assert!(
        line.contains("Firefox: Un téléchargement est en cours"),
        "{line}"
    );
    assert!(!line.contains("Mises à jour"), "{line}");
    let shown = bed.hp.leave_shown().expect("a confirmation");
    let LeaveStep::Held(holders) = &shown.step else {
        panic!("not held: {shown:?}");
    };
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].kind, HolderKind::Inhibitor { pid: 4242 });
    // Nothing counts while it holds.
    bed.step(Duration::from_secs(12))?;
    assert!(bed.fake.calls().is_empty(), "{:?}", bed.fake.calls());
    // It lets go: the countdown from 10 s.
    let mark = bed.mark();
    bed.fake.change(|l| l.inhibitors.clear());
    bed.logged_after(mark, "« L’ordinateur s’éteint dans 10 s. »")?;
    assert!(matches!(
        bed.hp.leave_shown().map(|s| s.step),
        Some(LeaveStep::Counting { left_ms }) if left_ms > 9_000
    ));
    Ok(())
}

#[test]
fn a_window_that_does_not_close_is_listed_and_fermer_quand_meme_closes_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    // A window that stays when asked to close, as one asking to save.
    let (_, client) = common::open_with(&mut bed.hp, "org.example.Notes", (300, 200), false)?;
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    bed.click(Bed::row(menu, LOG_OUT))?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    bed.click(Bed::button(confirm, NOW))?;
    let asked = bed.hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::CloseRequested)
    })?;
    anyhow::ensure!(asked, "the window was never asked to close");
    // 5 s on, it is listed.
    let mark = bed.mark();
    bed.step(Duration::from_millis(4900))?;
    assert!(matches!(
        bed.hp.leave_shown().map(|s| s.step),
        Some(LeaveStep::Closing)
    ));
    bed.step(Duration::from_millis(200))?;
    let line = bed.logged_after(mark, "Notes retient la déconnexion")?;
    assert!(
        line.contains("Notes: Sa fenêtre ne s’est pas fermée."),
        "{line}"
    );
    assert_eq!(bed.hp.ending(), None);
    // The keyboard on "Annuler"; round to "Revenir à Notes", then
    // "Fermer quand même".
    let mark = bed.mark();
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.logged_after(mark, "\"focus\":\"session:Force(0)\"")?;
    bed.press(KEY_ENTER);
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::LoggedOut));
    assert!(bed.hp.windows().is_empty(), "{:?}", bed.hp.windows());
    Ok(())
}

#[test]
fn super_shift_e_opens_the_log_out_confirmation() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let mark = bed.mark();
    common::chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_E]);
    bed.logged_after(mark, "Se déconnecter ? « La session se ferme dans 10 s. »")?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    assert_eq!(confirm.y + confirm.height, 1080 - 8);
    assert_eq!(bed.hp.ending(), None);
    // Esc cancels, Esc again closes.
    let mark = bed.mark();
    bed.press(KEY_ESC);
    bed.logged_after(mark, "shows=rows")?;
    assert_eq!(bed.hp.leave_shown(), None);
    assert_eq!(bed.hp.popover_surfaces().len(), 1);
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the popover stayed");
    assert_eq!(bed.hp.ending(), None);
    Ok(())
}

#[test]
fn logging_out_ends_the_compositor_once_the_windows_close() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let (_, client) = common::open_with(&mut bed.hp, "org.example.Editor", (300, 200), false)?;
    let menu = bed.open_menu()?;
    bed.hp.hold_clock();
    bed.click(Bed::row(menu, LOG_OUT))?;
    let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
    bed.click(Bed::button(confirm, NOW))?;
    // Asked to close, it does, as an application quitting.
    let asked = bed.hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::CloseRequested)
    })?;
    anyhow::ensure!(asked, "the window was never asked to close");
    assert_eq!(bed.hp.ending(), None, "before its window closed");
    client.commands.send(Command::Close)?;
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::LoggedOut));
    // Without waiting the 5 s, nor asking logind.
    assert!(bed.fake.calls().is_empty());
    Ok(())
}

#[test]
fn the_keyboard_lands_on_maintenant() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let mark = bed.mark();
    common::chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_E]);
    bed.logged_after(mark, "Se déconnecter ?")?;
    bed.logged_after(mark, "\"focus\":\"session:Now\"")?;
    bed.popover_of(CONFIRM_HEIGHT)?;
    // Enter logs out at once, without waiting out the count.
    bed.press(KEY_ENTER);
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "Enter did not log out");
    assert_eq!(bed.hp.ending(), Some(Ending::LoggedOut));
    Ok(())
}

#[test]
fn a_restart_that_needs_the_password_asks_polkit_first() -> anyhow::Result<()> {
    let challenge = Logind {
        can_reboot: "challenge",
        ..Logind::default()
    };
    let Some(mut bed) = Bed::start(challenge)? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    let mark = bed.mark();
    bed.click(Bed::row(menu, RESTART))?;
    // polkit asked, the user interacting (the desktop's dialog, 7.5), for
    // logind's action; granted, the countdown.
    bed.logged_after(mark, "Redémarrer ? « L’ordinateur redémarre dans")?;
    let asked = logind(&bed.fake.shared).authorizations.clone();
    assert_eq!(asked, [("org.freedesktop.login1.reboot".to_owned(), 1)]);
    assert!(bed.fake.calls().is_empty());
    Ok(())
}

#[test]
fn at_rest_the_menu_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    let menu = bed.open_menu()?;
    // The pointer away from its rows.
    common::point(
        &mut bed.hp,
        (f64::from(menu.x + 140), f64::from(menu.y + 4)),
    )?;
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let (popover, footer) = (bed.hp.popover_commits(), bed.hp.bar_commits());
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.popover_commits(), popover, "the menu drew at rest");
    assert_eq!(bed.hp.bar_commits(), footer, "the bar drew at rest");
    let _ = &bed.dir;
    Ok(())
}

/// The screen's part round the popover at `menu`, ×2, as `name`.png in
/// `out`.
fn shoot(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    common::at_rest(&mut bed.hp)?;
    let menu = bed.hp.popover_surfaces()[0].1;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    // The popover and the bar beside it, 24 px round.
    let x = (menu.x - 24).max(0) as u32;
    let y = (menu.y - 24).max(0) as u32;
    let (cw, ch) = ((w as u32 - x).min(280 + 24 + 8 + 210), (h as u32 - y));
    let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let zoomed = image::imageops::resize(
        &crop,
        cw * 2,
        ch * 2,
        image::imageops::FilterType::CatmullRom,
    );
    let file = out.join(format!("{name}.png"));
    zoomed.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    // The menu, and its countdown.
    {
        let Some(mut bed) = Bed::start(Logind::default())? else {
            return Ok(());
        };
        let menu = bed.open_menu()?;
        bed.hp
            .pointer_move(f64::from(menu.x + 140), f64::from(menu.y + LOCK + 22));
        wait(&mut bed.hp, Duration::from_millis(300))?;
        shoot(&mut bed, &out, "menu")?;
        bed.hp.hold_clock();
        bed.click(Bed::row(menu, SHUT_DOWN))?;
        bed.popover_of(CONFIRM_HEIGHT)?;
        bed.step(Duration::from_secs(3))?;
        bed.hp.pointer_move(400.0, 300.0);
        shoot(&mut bed, &out, "countdown")?;
    }
    // An inhibitor holding the shutdown back.
    {
        let held = Logind {
            inhibitors: vec![(
                "shutdown:sleep".into(),
                "Firefox".into(),
                "Un téléchargement est en cours (2 fichiers)".into(),
                "block".into(),
                1000,
                4242,
            )],
            ..Logind::default()
        };
        let Some(mut bed) = Bed::start(held)? else {
            return Ok(());
        };
        let menu = bed.open_menu()?;
        let mark = bed.mark();
        bed.click(Bed::row(menu, SHUT_DOWN))?;
        bed.logged_after(mark, "Firefox retient l’arrêt")?;
        bed.hp.pointer_move(400.0, 300.0);
        shoot(&mut bed, &out, "held-inhibitor")?;
    }
    // A window that did not close within 5 s.
    {
        let Some(mut bed) = Bed::start(Logind::default())? else {
            return Ok(());
        };
        let (_, _client) = common::open_with(&mut bed.hp, "org.example.Notes", (300, 200), false)?;
        let menu = bed.open_menu()?;
        bed.hp.hold_clock();
        bed.click(Bed::row(menu, LOG_OUT))?;
        let confirm = bed.popover_of(CONFIRM_HEIGHT)?;
        bed.click(Bed::button(confirm, NOW))?;
        let closing = bed.hp.dispatch_until(WAIT, |hp| {
            matches!(hp.leave_shown().map(|s| s.step), Some(LeaveStep::Closing))
        })?;
        anyhow::ensure!(closing, "the windows were never asked to close");
        let mark = bed.mark();
        bed.step(Duration::from_millis(5100))?;
        bed.logged_after(mark, "Notes retient la déconnexion")?;
        bed.hp.pointer_move(400.0, 300.0);
        shoot(&mut bed, &out, "held-window")?;
    }
    Ok(())
}

/// The menu, then a confirmation, shot under `HOCUS_KIT_SHOTS` (#772's
/// side-by-sides; see `wifi::kit_shot`).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Logind::default())? else {
        return Ok(());
    };
    bed.open_menu()?;
    bed.hp.pointer_move(600.0, 500.0);
    common::popover_shot(&mut bed.hp, "session")
}
