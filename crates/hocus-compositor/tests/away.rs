//! Locking on idle and before sleep (7.2, issue #413, docs/design/idle.md
//! and lock.md): one test per point of the approved spec, headless, the
//! motions' clock held and stepped (`Headless::step`) so that minutes pass
//! in no time.
//!
//! The system bus is a private `dbus-daemon` of the test's own, with a
//! fake logind (its `Inhibit`, whose file descriptor's other end the test
//! keeps to see it closed, and `PrepareForSleep`) and a fake UPower
//! (`OnBattery`): never the machine's. The pill is drawn by the real shell
//! on the CPU where a test needs it.

use crate::common;

use std::{
    io::Read,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use anyhow::Context;
use common::{BTN_LEFT, Command, Event, Pointed, Shows, WAIT, chord, open_showing, open_with};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, LockState, OutputSpec, RendererChoice,
    away::Shown, idle::Phase,
};

/// Five minutes on AC, three on battery; the curtain's 10 s; the screens
/// off 30 s after the lock.
const AC: Duration = Duration::from_secs(300);
const BATTERY: Duration = Duration::from_secs(180);
const CURTAIN: Duration = Duration::from_secs(10);
const SCREEN_OFF: Duration = Duration::from_secs(30);
const SECOND: Duration = Duration::from_secs(1);
const RIGHT: &str = "panda";
const KEY_A: u32 = 30;
const KEY_ENTER: u32 = 28;

fn config() -> Config {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.plain_background = true;
    config.lock_auth = Authenticator::test(RIGHT);
    config
}

fn headless(outputs: &[&str]) -> anyhow::Result<HeadlessConfig> {
    Ok(HeadlessConfig {
        outputs: outputs
            .iter()
            .map(|o| o.parse())
            .collect::<Result<Vec<OutputSpec>, _>>()?,
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

/// The clock held, the countdown just started by the user's input.
fn begin(hp: &mut Headless) {
    hp.hold_clock();
    hp.pointer_move(200.0, 200.0);
}

fn start(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, config)?;
    begin(&mut hp);
    Ok(hp)
}

/// The held clock moved on by `by`, in steps of at most a minute.
fn pass(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while !left.is_zero() {
        let step = left.min(Duration::from_secs(60));
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

fn type_panda(hp: &mut Headless) {
    // p a n d a
    for key in [25, 30, 49, 32, 30] {
        chord(hp, &[key]);
    }
}

/// What a pointing client's pointer got so far.
fn pointed(hp: &mut Headless, client: &common::ClientHandle) -> anyhow::Result<Vec<Pointed>> {
    common::wait(hp, Duration::from_millis(100))?;
    let mut seen = Vec::new();
    while let Ok(event) = client.events.try_recv() {
        if let Event::Pointer(p) = event {
            seen.push(p);
        }
    }
    Ok(seen)
}

// --- The timeline ------------------------------------------------------------------

/// Idle: the curtain at the delay, dimming over 10 s; locked at its end;
/// the screens off 30 s after; a key turns them on, still locked, and goes
/// to the lock's field; off again 30 s after it; the password opens.
#[test]
fn idle_brings_the_curtain_then_the_lock_then_the_screens_off() -> anyhow::Result<()> {
    let mut hp = start(config())?;
    pass(&mut hp, AC - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    assert_eq!(hp.curtain_dim(), Some(0.0));
    hp.step(CURTAIN / 2)?;
    let half = hp.curtain_dim().context("no dim")?;
    assert!(
        (f64::from(half) - 0.74 * 0.5f64.powf(1.15)).abs() < 1e-3,
        "{half}"
    );
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.step(CURTAIN / 2)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(hp.away(), Shown::Here);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    pass(&mut hp, SCREEN_OFF - SECOND)?;
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    pass(&mut hp, SECOND)?;
    assert!(hp.screens_on().is_empty(), "{:?}", hp.screens_on());
    // A key: the screens on, still locked, the key in the field.
    chord(&mut hp, &[KEY_A]);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(hp.lock_typed(), Some(1));
    pass(&mut hp, SCREEN_OFF)?;
    assert!(hp.screens_on().is_empty());
    // The password opens it; the screens on with the first key.
    hp.key(14, true); // Backspace
    hp.key(14, false);
    type_panda(&mut hp);
    chord(&mut hp, &[KEY_ENTER]);
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    anyhow::ensure!(open, "the lock did not open: {:?}", hp.lock_shown());
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.away_counts().curtains, 1);
    Ok(())
}

// --- What lifts it -------------------------------------------------------------------

/// Any input lifts the curtain and goes no further: a click on a window
/// reaches nothing (its release neither), a move does not move over the
/// window, a key types nothing. The next click goes as usual; a move of
/// less than 4 px is no one's.
#[test]
fn what_lifts_the_curtain_is_swallowed() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, config())?;
    let app = "test.pointing";
    let (rect, client) = open_showing(
        &mut hp,
        app,
        Shows::Pointing {
            app_id: app,
            size: (800, 600),
        },
    )?;
    let (x, y) = (
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    );
    hp.hold_clock();
    hp.pointer_move(x, y);
    pointed(&mut hp, &client)?;
    // A click.
    pass(&mut hp, AC)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    // Less than 4 px: not the user's.
    hp.pointer_move(x + 2.0, y + 1.0);
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    hp.button(BTN_LEFT, true);
    assert_eq!(hp.away(), Shown::Here);
    hp.button(BTN_LEFT, false);
    let seen = pointed(&mut hp, &client)?;
    assert!(
        !seen.contains(&Pointed::Press) && !seen.contains(&Pointed::Release),
        "{seen:?}"
    );
    // A move.
    pass(&mut hp, AC)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    hp.pointer_move(x + 40.0, y + 30.0);
    assert_eq!(hp.away(), Shown::Here);
    let seen = pointed(&mut hp, &client)?;
    assert!(!seen.contains(&Pointed::Motion), "{seen:?}");
    // Then a click goes as usual.
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let seen = pointed(&mut hp, &client)?;
    assert!(
        seen.contains(&Pointed::Press) && seen.contains(&Pointed::Release),
        "{seen:?}"
    );
    // A key.
    let typing = "test.typing";
    let (_, keys) = open_showing(
        &mut hp,
        typing,
        Shows::Typing {
            app_id: typing,
            size: (400, 300),
        },
    )?;
    common::wait(&mut hp, Duration::from_millis(100))?;
    while keys.events.try_recv().is_ok() {}
    pass(&mut hp, AC)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    chord(&mut hp, &[KEY_A]);
    assert_eq!(hp.away(), Shown::Here);
    common::wait(&mut hp, Duration::from_millis(100))?;
    let typed: Vec<Event> = keys
        .events
        .try_iter()
        .filter(|e| matches!(e, Event::Key(..)))
        .collect();
    assert!(typed.is_empty(), "{typed:?}");
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    assert!(hp.away_counts().swallowed >= 3);
    Ok(())
}

// --- The delays ----------------------------------------------------------------------

/// On battery (UPower's `OnBattery`), the battery's delay; back on AC,
/// AC's.
#[test]
fn on_battery_its_own_delay() -> anyhow::Result<()> {
    let Some(mut system) = System::start(true)? else {
        return Ok(());
    };
    let mut hp = system.compositor(config(), None)?;
    let told = hp.dispatch_until(WAIT, Headless::on_battery)?;
    anyhow::ensure!(told, "UPower's OnBattery never came");
    begin(&mut hp);
    pass(&mut hp, BATTERY - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    hp.pointer_move(500.0, 500.0);
    assert_eq!(hp.away(), Shown::Here);
    system.set_on_battery(false)?;
    let told = hp.dispatch_until(WAIT, |hp| !hp.on_battery())?;
    anyhow::ensure!(told, "back on AC never came");
    pass(&mut hp, BATTERY)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, AC - BATTERY)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    Ok(())
}

// --- What holds it off ---------------------------------------------------------------

/// A shown window's idle inhibitor, then the focused window in full
/// screen: no curtain, no lock, no screen off while they hold.
#[test]
fn an_inhibitor_or_a_focused_fullscreen_window_holds_it_all_off() -> anyhow::Result<()> {
    let mut hp = start(config())?;
    let (_, video) = open_with(&mut hp, "test.video", (640, 360), false)?;
    video.commands.send(Command::Inhibit(true))?;
    let held = hp.dispatch_until(WAIT, |hp| hp.idle().phase == Phase::Held)?;
    anyhow::ensure!(held, "{:?}", hp.idle());
    pass(&mut hp, Duration::from_secs(20 * 60))?;
    assert_eq!(hp.away(), Shown::Here);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    video.commands.send(Command::Inhibit(false))?;
    let (_, _film) = open_showing(
        &mut hp,
        "test.film",
        Shows::Asking {
            app_id: "test.film",
            size: (640, 360),
            fullscreen: true,
        },
    )?;
    let held = hp.dispatch_until(WAIT, |hp| {
        hp.idle().holders
            == [hocus_compositor::idle::Holder::Fullscreen(
                "test.film".into(),
            )]
    })?;
    anyhow::ensure!(held, "{:?}", hp.idle());
    pass(&mut hp, Duration::from_secs(20 * 60))?;
    assert_eq!(hp.away(), Shown::Here);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.away_counts().curtains, 0);
    Ok(())
}

// --- Before sleep --------------------------------------------------------------------

/// The inhibitor held from the start; `PrepareForSleep(true)` locks at
/// once (no curtain, no fade), and the inhibitor goes once the lock is on
/// screen; the wake-up takes a new one, the session still locked.
#[test]
fn prepare_for_sleep_locks_and_lets_go_once_the_lock_shows() -> anyhow::Result<()> {
    let Some(mut system) = System::start(false)? else {
        return Ok(());
    };
    let mut hp = system.compositor(config(), None)?;
    system.wait_inhibited(&mut hp, 1)?;
    hp.pointer_move(300.0, 300.0);
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "never locked");
    assert_eq!(hp.away_counts().curtains, 0);
    let free = hp.dispatch_until(WAIT, |hp| !hp.sleep_inhibited())?;
    anyhow::ensure!(free, "the inhibitor was never let go");
    assert!(hp.lock_drawn());
    system.wait_released(&mut hp)?;
    let counts = hp.away_counts();
    assert_eq!((counts.slept, counts.slept_late), (1, 0), "{counts:?}");
    system.prepare_for_sleep(false)?;
    system.wait_inhibited(&mut hp, 2)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    Ok(())
}

/// With the shell, the machine waits for its lock to be on screen; a
/// shell that does not draw it (held) keeps it waiting 2 s at most.
#[test]
fn before_sleep_the_shells_lock_is_waited_for_two_seconds_at_most() -> anyhow::Result<()> {
    let Some(mut system) = System::start(false)? else {
        return Ok(());
    };
    let Some(shell) = Shell::alone()? else {
        return Ok(());
    };
    let mut hp = system.compositor(config(), Some(shell.script.clone()))?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    system.wait_inhibited(&mut hp, 1)?;
    system.prepare_for_sleep(true)?;
    let free = hp.dispatch_until(WAIT, |hp| !hp.sleep_inhibited())?;
    anyhow::ensure!(free, "the inhibitor was never let go");
    assert_eq!(
        hp.lock_drawn_on(),
        ["HEADLESS-1"],
        "its lock on screen first"
    );
    assert_eq!(hp.away_counts().slept_late, 0);
    system.wait_released(&mut hp)?;
    system.prepare_for_sleep(false)?;
    system.wait_inhibited(&mut hp, 2)?;
    // Unlocked, then the shell held: its lock never comes.
    type_panda(&mut hp);
    chord(&mut hp, &[KEY_ENTER]);
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    anyhow::ensure!(open, "the lock did not open");
    common::at_rest(&mut hp)?;
    hp.hold_shell(true)?;
    hp.hold_clock();
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "never locked");
    pass(&mut hp, Duration::from_millis(1900))?;
    assert!(hp.sleep_inhibited(), "let go before the 2 s");
    pass(&mut hp, Duration::from_millis(100))?;
    assert!(!hp.sleep_inhibited(), "still waiting after 2 s");
    assert_eq!(hp.away_counts().slept_late, 1);
    hp.hold_shell(false)?;
    Ok(())
}

// --- The lid -------------------------------------------------------------------------

/// The lid closed with another screen there (docked, on AC): the laptop's
/// screen goes off, nothing locks; opened, it is on again.
#[test]
fn the_lid_closed_docked_on_ac_does_not_lock() -> anyhow::Result<()> {
    let Some(system) = System::start(false)? else {
        return Ok(());
    };
    let mut hp = Headless::start_on_buses(
        headless(&["1920x1200", "2560x1440"])?,
        config(),
        None,
        None,
        Some(system.address.clone()),
    )?;
    hp.set_internal("HEADLESS-1");
    begin(&mut hp);
    hp.lid(true);
    common::wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    assert!(!hp.on_battery());
    pass(&mut hp, Duration::from_secs(60))?;
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.lid(false);
    assert_eq!(hp.screens_on(), ["HEADLESS-1", "HEADLESS-2"]);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    Ok(())
}

// --- At rest -------------------------------------------------------------------------

/// At rest only the idle countdown's timer is armed, and nothing wakes;
/// the curtain draws a frame a tenth of a second, nothing between; locked
/// with the screens off, nothing is armed and nothing draws.
#[test]
fn at_rest_no_wakeup_beyond_the_idle_timer() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, config())?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    let away = hp.away_counts();
    assert_eq!(
        (away.armed, away.timers, away.wakeups),
        (false, 0, 0),
        "{away:?}"
    );
    assert!(hp.idle_counts().armed);
    begin(&mut hp);
    pass(&mut hp, AC)?;
    let start = hp.now();
    hp.step(Duration::from_millis(330))?;
    // A frame asked for before this step has come and gone.
    common::wait(&mut hp, Duration::from_millis(200))?;
    common::at_rest(&mut hp)?;
    assert_eq!(hp.wake(), Some(start + Duration::from_millis(400)));
    pass(&mut hp, CURTAIN - Duration::from_millis(330))?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    pass(&mut hp, SCREEN_OFF)?;
    assert!(hp.screens_on().is_empty());
    common::wait(&mut hp, Duration::from_millis(300))?;
    let frames = hp.frames();
    common::wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.frames(), frames);
    assert!(!hp.away_counts().armed, "{:?}", hp.away_counts());
    assert!(!hp.idle_counts().armed, "{:?}", hp.idle_counts());
    Ok(())
}

// --- The pill ------------------------------------------------------------------------

/// The shell draws the pill on the screen under the pointer, centred
/// across it, its top at 64 % of its height; lifted, it goes.
#[test]
fn the_shell_draws_the_pill_on_the_screen_under_the_pointer() -> anyhow::Result<()> {
    let Some(shell) = Shell::alone()? else {
        return Ok(());
    };
    let mut hp = Headless::start_with_shell(
        headless(&["1920x1080", "2560x1600@2"])?,
        config(),
        Some(shell.script.clone()),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.hold_clock();
    hp.pointer_move(2400.0, 300.0);
    pass(&mut hp, AC)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    let drawn = hp.dispatch_until(WAIT, |hp| {
        !hp.curtain_surfaces().is_empty()
            && !hp.drawn_layers(hocus_link::CURTAIN_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(drawn, "no pill");
    let (output, rect) = hp.curtain_surfaces().remove(0);
    assert_eq!(output, "HEADLESS-2");
    // 1280 × 800 logical, from x 1920: centred, its top at 512; 80 high
    // for its 64 px panda (#649).
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (1920 + (1280 - 300) / 2, 512, 300, 80)
    );
    hp.pointer_move(2000.0, 400.0);
    assert_eq!(hp.away(), Shown::Here);
    hp.step(Duration::from_millis(200))?;
    let gone = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::CURTAIN_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "the pill stayed");
    Ok(())
}

// --- The fakes -----------------------------------------------------------------------

/// What the fake logind and UPower hold.
#[derive(Debug, Default)]
struct Held {
    /// The other ends of the inhibitors given.
    inhibitors: Vec<UnixStream>,
    on_battery: bool,
}

type Shared = Arc<Mutex<Held>>;

fn held(shared: &Shared) -> std::sync::MutexGuard<'_, Held> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Login(Shared);

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Login {
    fn inhibit(
        &self,
        what: String,
        _who: String,
        _why: String,
        mode: String,
    ) -> zbus::fdo::Result<zbus::OwnedFd> {
        if what != "sleep" || mode != "delay" {
            return Err(zbus::fdo::Error::InvalidArgs(format!("{what} {mode}")));
        }
        let (ours, theirs) =
            UnixStream::pair().map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        ours.set_nonblocking(true)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        held(&self.0).inhibitors.push(ours);
        Ok(zbus::OwnedFd::from(std::os::fd::OwnedFd::from(theirs)))
    }
}

struct UPower(Shared);

#[zbus::interface(name = "org.freedesktop.UPower")]
impl UPower {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        held(&self.0).on_battery
    }
}

/// A system bus of the test's own, with logind and UPower on it.
struct System {
    daemon: std::process::Child,
    address: String,
    runtime: tokio::runtime::Runtime,
    shared: Shared,
    login: zbus::Connection,
    upower: zbus::Connection,
    dir: tempfile::TempDir,
}

impl System {
    fn start(on_battery: bool) -> anyhow::Result<Option<Self>> {
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let (daemon, address) = common::private_bus(dir.path())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let shared: Shared = Arc::new(Mutex::new(Held {
            inhibitors: Vec::new(),
            on_battery,
        }));
        let login = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.login1")
                .serve_at("/org/freedesktop/login1", Login(shared.clone()))
                .build(),
        )?;
        let upower = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.UPower")
                .serve_at("/org/freedesktop/UPower", UPower(shared.clone()))
                .build(),
        )?;
        Ok(Some(Self {
            daemon,
            address,
            runtime,
            shared,
            login,
            upower,
            dir,
        }))
    }

    fn compositor(&mut self, config: Config, shell: Option<PathBuf>) -> anyhow::Result<Headless> {
        Headless::start_on_buses(
            headless(&["1920x1080"])?,
            config,
            shell,
            None,
            Some(self.address.clone()),
        )
    }

    /// Until `count` inhibitors were given, the last held by the
    /// compositor.
    fn wait_inhibited(&mut self, hp: &mut Headless, count: usize) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let came = hp.dispatch_until(WAIT, |hp| {
            held(&shared).inhibitors.len() == count && hp.sleep_inhibited()
        })?;
        anyhow::ensure!(
            came,
            "{} inhibitors given, not {count}",
            held(&shared).inhibitors.len()
        );
        Ok(())
    }

    /// Until the last inhibitor given is closed at the compositor's end.
    fn wait_released(&mut self, hp: &mut Headless) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let closed = hp.dispatch_until(WAIT, |_| {
            let mut held = held(&shared);
            held.inhibitors
                .last_mut()
                .is_some_and(|end| matches!(end.read(&mut [0u8; 1]), Ok(0)))
        })?;
        anyhow::ensure!(closed, "the inhibitor's file descriptor is still open");
        Ok(())
    }

    fn prepare_for_sleep(&self, sleeping: bool) -> anyhow::Result<()> {
        let login = self.login.clone();
        self.runtime.block_on(async move {
            login
                .emit_signal(
                    None::<&str>,
                    "/org/freedesktop/login1",
                    "org.freedesktop.login1.Manager",
                    "PrepareForSleep",
                    &(sleeping,),
                )
                .await
        })?;
        Ok(())
    }

    fn set_on_battery(&self, on_battery: bool) -> anyhow::Result<()> {
        held(&self.shared).on_battery = on_battery;
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
                        std::collections::HashMap::<&str, zbus::Variant<'_>>::from([(
                            "OnBattery",
                            zbus::Variant::from(on_battery),
                        )]),
                        Vec::<&str>::new(),
                    ),
                )
                .await
        })?;
        Ok(())
    }
}

impl Drop for System {
    fn drop(&mut self) {
        // The daemon this test started, by its own PID.
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = &self.dir;
    }
}

/// The real shell, away from the machine's services, in directories of
/// its own.
struct Shell {
    script: PathBuf,
    _dir: tempfile::TempDir,
}

impl Shell {
    fn alone() -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let at = |sub: &str| root.join(sub).display().to_string();
        let env = vec![
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("HOCUS_USER_NAME", "Alex".to_owned()),
        ];
        let script = common::shell_script(root, &shell, &env)?;
        Ok(Some(Self { script, _dir: dir }))
    }
}
