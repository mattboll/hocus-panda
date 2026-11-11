//! The desktop acting on the Énergie page (6.10, issue #410, the approved
//! spec's "What the desktop does"): the dim, the lock and the screens off
//! with each power source's delays, sleeping after a while, the lid, the
//! power button and the automatic power saver, every `[power]` key
//! followed live. One test per point, headless, the motions' clock held
//! and stepped so that minutes pass in no time.
//!
//! The system bus is a private `dbus-daemon` of the test's own with a fake
//! logind (its inhibitors, `Suspend`, `PowerOff`, `ListInhibitors`), UPower
//! (`OnBattery`, its display device's level) and power-profiles-daemon
//! (`ActiveProfile`); the session bus, where one is needed, another with a
//! fake notifications server: never the machine's.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use common::{WAIT, open_with};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, Lid, LockState, OutputSpec, PowerButton,
    PowerDelay, RendererChoice,
    away::Shown,
    away::power::{LidRead, Saver},
};

const CURTAIN: Duration = Duration::from_secs(10);
const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
/// evdev's `KEY_POWER`.
const KEY_POWER: u32 = 116;

fn config() -> Config {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.plain_background = true;
    config.lock_auth = Authenticator::test("panda");
    config.french = true;
    config
}

/// The password typed, until the session opens.
fn unlock(hp: &mut Headless) -> anyhow::Result<()> {
    // p a n d a, Enter
    for key in [25, 30, 49, 32, 30, 28] {
        common::chord(hp, &[key]);
    }
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    anyhow::ensure!(open, "the lock did not open");
    Ok(())
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

fn after(seconds: u64) -> PowerDelay {
    PowerDelay::After(Duration::from_secs(seconds))
}

/// The held clock moved on by `by`, in steps of at most a minute.
fn pass(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while !left.is_zero() {
        let step = left.min(MINUTE);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

/// The clock held, the countdowns just started by the user's input.
fn begin(hp: &mut Headless) {
    hp.hold_clock();
    hp.pointer_move(200.0, 200.0);
}

// --- The delays -----------------------------------------------------------------------

/// The dim at the source's delay, the lock as it ends, the screens off the
/// source's `screen-off-after` later: on AC `[idle] delay` and
/// `[power.ac]`, on battery `[power.battery]`.
#[test]
fn the_dim_lock_and_screen_off_follow_the_delays_of_the_power_source() -> anyhow::Result<()> {
    let mut set = config();
    set.idle.delay = Some(2 * MINUTE);
    set.power.ac.screen_off_after = after(60);
    set.power.battery.idle = after(60);
    set.power.battery.screen_off_after = after(0);
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, set.clone())?;
    begin(&mut hp);
    pass(&mut hp, 2 * MINUTE - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    pass(&mut hp, CURTAIN)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    pass(&mut hp, MINUTE - SECOND)?;
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    pass(&mut hp, SECOND)?;
    assert!(hp.screens_on().is_empty());
    drop(hp);

    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let mut hp = system.compositor(&["1920x1080"], set, None)?;
    let told = hp.dispatch_until(WAIT, Headless::on_battery)?;
    anyhow::ensure!(told, "UPower's OnBattery never came");
    begin(&mut hp);
    pass(&mut hp, MINUTE - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    pass(&mut hp, CURTAIN)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    // "Aussitôt": off as it locks.
    pass(&mut hp, SECOND)?;
    assert!(hp.screens_on().is_empty(), "{:?}", hp.screens_on());
    Ok(())
}

/// A countdown under way keeps the time already spent when the source
/// changes: 4 min of AC's 5 spent, unplugged, the battery's 3 are over
/// (dimmed at once); 2 min of the battery's 3 spent, plugged in, AC's 5
/// end 3 min later.
#[test]
fn plugging_in_switches_the_delays_keeping_the_time_spent() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0).on_ac())? else {
        return Ok(());
    };
    let mut hp = system.compositor(&["1920x1080"], config(), None)?;
    let up = hp.dispatch_until(WAIT, |hp| hp.state().power_profile_now().is_some())?;
    anyhow::ensure!(up, "the system bus was never read");
    begin(&mut hp);
    pass(&mut hp, 4 * MINUTE)?;
    assert_eq!(hp.away(), Shown::Here);
    system.change(|s| s.on_battery = true)?;
    let dimmed = hp.dispatch_until(WAIT, |hp| hp.away() == Shown::Curtain { locks: true })?;
    anyhow::ensure!(dimmed, "not dimmed on battery: {:?}", hp.idle());
    hp.pointer_move(600.0, 600.0);
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, 2 * MINUTE)?;
    system.change(|s| s.on_battery = false)?;
    let ac = hp.dispatch_until(WAIT, |hp| !hp.on_battery())?;
    anyhow::ensure!(ac, "back on AC never came");
    pass(&mut hp, 3 * MINUTE - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    Ok(())
}

/// `lock-on-idle = false`: the curtain has no pill, stays dimmed once it
/// ended, and the screens go off `screen-off-after` later, unlocked; a
/// move turns them on, unlocked, and goes no further.
#[test]
fn locking_off_turns_the_screen_off_without_locking() -> anyhow::Result<()> {
    let mut set = config();
    set.power.lock_on_idle = false;
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, set)?;
    begin(&mut hp);
    pass(&mut hp, 5 * MINUTE)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: false });
    pass(&mut hp, CURTAIN)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: false });
    assert_eq!(hp.curtain_dim(), Some(0.74));
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    pass(&mut hp, Duration::from_secs(30) - SECOND)?;
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Dark);
    assert!(hp.screens_on().is_empty());
    pass(&mut hp, 60 * MINUTE)?;
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.pointer_move(600.0, 600.0);
    assert_eq!(hp.away(), Shown::Here);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    Ok(())
}

// --- Sleeping ------------------------------------------------------------------------

/// On battery, 15 min without activity: logind's `Suspend(false)`; its
/// `PrepareForSleep` locks the session before the machine sleeps. With
/// `lock-before-sleep = false`, it sleeps unlocked.
#[test]
fn suspend_after_calls_logind_and_locks_first() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let mut set = config();
    set.power.lock_on_idle = false;
    let mut hp = system.compositor(&["1920x1080"], set.clone(), None)?;
    system.wait_ready(&mut hp)?;
    begin(&mut hp);
    pass(&mut hp, 15 * MINUTE - SECOND)?;
    assert!(system.calls().is_empty(), "{:?}", system.calls());
    pass(&mut hp, SECOND)?;
    system.wait_call(&mut hp, "Suspend false")?;
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "not locked before sleeping");
    let free = hp.dispatch_until(WAIT, |hp| !hp.sleep_inhibited())?;
    anyhow::ensure!(free, "the sleep inhibitor was never let go");
    // Asked once, until the next activity.
    pass(&mut hp, 30 * MINUTE)?;
    assert_eq!(system.calls(), ["Suspend false"]);
    drop(hp);

    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    set.power.lock_before_sleep = false;
    let mut hp = system.compositor(&["1920x1080"], set, None)?;
    system.wait_ready(&mut hp)?;
    begin(&mut hp);
    pass(&mut hp, 15 * MINUTE)?;
    system.wait_call(&mut hp, "Suspend false")?;
    system.prepare_for_sleep(true)?;
    let free = hp.dispatch_until(WAIT, |hp| !hp.sleep_inhibited())?;
    anyhow::ensure!(free, "the sleep inhibitor was never let go");
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    Ok(())
}

/// A video holding idle off (its inhibitor) holds the sleep off too; so
/// does logind's own `idle` inhibitor (the countdown starts again then);
/// once neither holds, the machine sleeps.
#[test]
fn a_video_holding_idle_holds_the_suspend_too() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let mut hp = system.compositor(&["1920x1080"], config(), None)?;
    system.wait_ready(&mut hp)?;
    begin(&mut hp);
    let (_, video) = open_with(&mut hp, "test.video", (640, 360), false)?;
    video.commands.send(common::Command::Inhibit(true))?;
    let held = hp.dispatch_until(WAIT, |hp| !hp.idle().holders.is_empty())?;
    anyhow::ensure!(held, "{:?}", hp.idle());
    pass(&mut hp, 40 * MINUTE)?;
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert!(system.calls().is_empty(), "{:?}", system.calls());
    // logind's own: asked, it says no, and 15 min later again.
    system.change(|s| {
        s.idle_inhibitors = vec![(
            "idle".into(),
            "Player".into(),
            "Playing".into(),
            "block".into(),
            1000,
            42,
        )];
    })?;
    video.commands.send(common::Command::Inhibit(false))?;
    let free = hp.dispatch_until(WAIT, |hp| hp.idle().holders.is_empty())?;
    anyhow::ensure!(free, "{:?}", hp.idle());
    pass(&mut hp, 15 * MINUTE)?;
    let listed = hp.dispatch_until(WAIT, |_| system.listed() > 0)?;
    anyhow::ensure!(listed, "logind's inhibitors were never read");
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert!(system.calls().is_empty(), "{:?}", system.calls());
    system.change(|s| s.idle_inhibitors.clear())?;
    pass(&mut hp, 15 * MINUTE)?;
    system.wait_call(&mut hp, "Suspend false")?;
    Ok(())
}

// --- The lid -------------------------------------------------------------------------

/// `[power] lid`: `lock` locks and turns the panel off, `screen-off` only
/// the panel, `nothing` nothing, `suspend` asks logind; docked on AC with
/// another screen on, only the panel, whatever `lid` says. Opened, the
/// panel comes back.
#[test]
fn the_lid_does_what_power_lid_says_and_docked_only_turns_the_panel_off() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let file_dir = tempfile::tempdir()?;
    let file = file_dir.path().join("config.toml");
    let mut set = config();
    set.file = Some(file.clone());
    rewrite(&file, "[power]\nlid = \"lock\"\n")?;
    set.power.lid = Lid::Lock;
    let mut hp = system.compositor(&["1920x1200", "2560x1440"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    assert!(hp.state().lid_handled());
    // On battery: the external screen does not count.
    hp.lid(true);
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    hp.lid(false);
    assert_eq!(hp.screens_on(), ["HEADLESS-1", "HEADLESS-2"]);
    unlock(&mut hp)?;

    follow(&mut hp, &file, "[power]\nlid = \"screen-off\"\n", |c| {
        c.power.lid == Lid::ScreenOff
    })?;
    hp.lid(true);
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.lid(false);

    follow(&mut hp, &file, "[power]\nlid = \"nothing\"\n", |c| {
        c.power.lid == Lid::Nothing
    })?;
    hp.lid(true);
    assert_eq!(hp.screens_on(), ["HEADLESS-1", "HEADLESS-2"]);
    hp.lid(false);

    follow(&mut hp, &file, "[power]\nlid = \"suspend\"\n", |c| {
        c.power.lid == Lid::Suspend
    })?;
    hp.lid(true);
    system.wait_call(&mut hp, "Suspend false")?;
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    hp.lid(false);

    // Docked: on AC, the external screen on.
    system.change(|s| s.on_battery = false)?;
    let ac = hp.dispatch_until(WAIT, |hp| !hp.on_battery())?;
    anyhow::ensure!(ac, "AC never came");
    hp.lid(true);
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    assert_eq!(system.calls(), ["Suspend false"], "docked: no sleep");
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.lid(false);
    // `lid-ignored-when-docked = false`: the lid's choice, docked or not.
    follow(
        &mut hp,
        &file,
        "[power]\nlid = \"suspend\"\nlid-ignored-when-docked = false\n",
        |c| !c.power.lid_ignored_when_docked,
    )?;
    hp.lid(true);
    let two = hp.dispatch_until(WAIT, |_| system.calls().len() == 2)?;
    anyhow::ensure!(two, "{:?}", system.calls());
    Ok(())
}

/// The laptop alone, `lid = "suspend"`: closed, the panel off and logind
/// asked to sleep. The lid opened while asleep, its event never came
/// (#730): as the machine wakes the lid is read again (UPower's
/// `LidIsClosed`, no signal), the panel on, still locked, no new sleep.
#[test]
fn the_lid_opened_while_asleep_is_read_again_as_the_machine_wakes() -> anyhow::Result<()> {
    let Some(system) = System::start(Services {
        lid: Some(false),
        ..Services::battery(80.0)
    })?
    else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::Suspend;
    let mut hp = system.compositor(&["1920x1200"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    system.change_quietly(|s| s.lid = Some(true));
    hp.lid(true);
    system.wait_call(&mut hp, "Suspend false")?;
    assert!(hp.screens_on().is_empty());
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "never locked");
    system.change_quietly(|s| s.lid = Some(false));
    system.prepare_for_sleep(false)?;
    let read = hp.dispatch_until(WAIT, |hp| {
        hp.state().lid_read_state() == LidRead::Known && !hp.state().lid_closed()
    })?;
    anyhow::ensure!(read, "the lid not read again: {}", hp.state().away_said());
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(system.calls(), ["Suspend false"]);
    Ok(())
}

/// Woken with the lid still closed (UPower says so as it is read again):
/// the panel off again, nothing else asked (no new sleep). UPower then
/// says it opened: the panel on, whether libinput's event comes or not.
#[test]
fn woken_with_the_lid_closed_the_panel_waits_for_it_to_open() -> anyhow::Result<()> {
    let Some(system) = System::start(Services {
        lid: Some(false),
        ..Services::battery(80.0)
    })?
    else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::Suspend;
    let mut hp = system.compositor(&["1920x1200"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    system.change_quietly(|s| s.lid = Some(true));
    hp.lid(true);
    system.wait_call(&mut hp, "Suspend false")?;
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "never locked");
    system.prepare_for_sleep(false)?;
    let read = hp.dispatch_until(WAIT, |hp| hp.state().lid_read_state() == LidRead::Known)?;
    anyhow::ensure!(read, "the lid not read again: {}", hp.state().away_said());
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert!(hp.state().lid_closed());
    assert!(hp.screens_on().is_empty());
    assert_eq!(
        system.calls(),
        ["Suspend false"],
        "no sleep again at waking"
    );
    system.change(|s| s.lid = Some(false))?;
    let open = hp.dispatch_until(WAIT, |hp| hp.screens_on() == ["HEADLESS-1"])?;
    anyhow::ensure!(open, "UPower's opening not taken");
    assert!(!hp.state().lid_closed());
    Ok(())
}

/// Nothing can say the lid's state after waking (no `LidIsClosed`, no
/// ACPI on a test's bus): the laptop's screen, the only one, is lit all
/// the same rather than left black on a state from before the sleep.
#[test]
fn unsure_of_the_lid_after_waking_the_only_screen_is_lit() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::Suspend;
    let mut hp = system.compositor(&["1920x1200"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    hp.lid(true);
    system.wait_call(&mut hp, "Suspend false")?;
    assert!(hp.screens_on().is_empty());
    system.prepare_for_sleep(true)?;
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "never locked");
    system.prepare_for_sleep(false)?;
    let lit = hp.dispatch_until(WAIT, |hp| hp.screens_on() == ["HEADLESS-1"])?;
    anyhow::ensure!(lit, "the only screen left off: {}", hp.state().away_said());
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    assert_eq!(hp.state().lid_read_state(), LidRead::Asked);
    assert_eq!(system.calls(), ["Suspend false"]);
    Ok(())
}

/// Back from another terminal (the tty's `ActivateSession`), the lid is
/// read again: opened meanwhile without its event, the panel comes on.
/// Docked, the laptop's screen is not lit before the answer.
#[test]
fn back_from_another_terminal_the_lid_is_read_again() -> anyhow::Result<()> {
    let Some(system) = System::start(Services {
        lid: Some(false),
        ..Services::battery(80.0)
    })?
    else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::ScreenOff;
    let mut hp = system.compositor(&["1920x1200"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    system.change_quietly(|s| s.lid = Some(true));
    hp.lid(true);
    assert!(hp.screens_on().is_empty());
    system.change_quietly(|s| s.lid = Some(false));
    hp.session_back();
    let open = hp.dispatch_until(WAIT, |hp| {
        !hp.state().lid_closed() && hp.screens_on() == ["HEADLESS-1"]
    })?;
    anyhow::ensure!(open, "the lid not read again: {}", hp.state().away_said());
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    Ok(())
}

/// With another screen on, a lid not yet read again keeps the laptop's
/// screen off (it is not the only one): read closed, it stays off.
#[test]
fn docked_the_lid_read_again_closed_keeps_the_panel_off() -> anyhow::Result<()> {
    let Some(system) = System::start(Services {
        lid: Some(true),
        ..Services::battery(80.0).on_ac()
    })?
    else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::Suspend;
    let mut hp = system.compositor(&["1920x1200", "2560x1440"], set, None)?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    hp.lid(true);
    common::wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    hp.session_back();
    assert_eq!(hp.screens_on(), ["HEADLESS-2"], "lit before the answer");
    let read = hp.dispatch_until(WAIT, |hp| hp.state().lid_read_state() == LidRead::Known)?;
    anyhow::ensure!(read, "the lid not read again: {}", hp.state().away_said());
    assert_eq!(hp.screens_on(), ["HEADLESS-2"]);
    assert!(system.calls().is_empty(), "{:?}", system.calls());
    Ok(())
}

/// Without logind's inhibitor (refused), the lid and the button are the
/// system's: the panel goes off, nothing else; the button's key goes on.
#[test]
fn without_loginds_inhibitor_the_system_decides() -> anyhow::Result<()> {
    let Some(system) = System::start(Services {
        refuse_handles: true,
        ..Services::battery(80.0)
    })?
    else {
        return Ok(());
    };
    let mut set = config();
    set.power.lid = Lid::Lock;
    set.power.power_button = PowerButton::Suspend;
    let mut hp = system.compositor(&["1920x1200"], set, None)?;
    hp.set_internal("HEADLESS-1");
    let up = hp.dispatch_until(WAIT, |hp| hp.sleep_inhibited() && hp.on_battery())?;
    anyhow::ensure!(up, "the system bus was never read");
    common::wait(&mut hp, Duration::from_millis(200))?;
    assert!(!hp.state().lid_handled());
    assert!(
        hp.state().away_said().contains("lid open system"),
        "{}",
        hp.state().away_said()
    );
    hp.lid(true);
    assert!(hp.screens_on().is_empty());
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    hp.lid(false);
    hp.key(KEY_POWER, true);
    hp.key(KEY_POWER, false);
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert!(system.calls().is_empty(), "{:?}", system.calls());
    assert_eq!(hp.lock_state(), LockState::Unlocked);
    Ok(())
}

// --- The power button ------------------------------------------------------------------

/// `ask` (the default): the session menu opens on the bar, as a click on
/// the footer's user opens it. `suspend`: locked, then logind's
/// `Suspend`; `power-off`: every window asked to close, then logind's
/// `PowerOff`.
#[test]
fn the_power_button_ask_opens_the_session_menu() -> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let Some(shell) = Shell::alone()? else {
        return Ok(());
    };
    let file_dir = tempfile::tempdir()?;
    let file = file_dir.path().join("config.toml");
    rewrite(&file, "")?;
    let mut set = config();
    set.file = Some(file.clone());
    let mut hp = system.compositor(&["1920x1080"], set, Some(shell.script.clone()))?;
    system.wait_ready(&mut hp)?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    assert!(hp.popover_surfaces().is_empty());
    hp.key(KEY_POWER, true);
    hp.key(KEY_POWER, false);
    // Drawn, not only placed: the menu's surface has its place, and the
    // keyboard, from its first commit, before the shell has its window;
    // a key that comes then is lost (#686). Esc is pressed on the menu
    // shown, as a user would.
    let open = hp.dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(open, "the session menu did not open");
    hp.key(1, true); // Esc
    hp.key(1, false);
    let closed = hp.dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the menu stayed");

    follow(
        &mut hp,
        &file,
        "[power]\npower-button = \"suspend\"\n",
        |c| c.power.power_button == PowerButton::Suspend,
    )?;
    hp.key(KEY_POWER, true);
    hp.key(KEY_POWER, false);
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Locked)?;
    anyhow::ensure!(locked, "not locked first");
    system.wait_call(&mut hp, "Suspend true")?;
    unlock(&mut hp)?;

    follow(
        &mut hp,
        &file,
        "[power]\npower-button = \"power-off\"\n",
        |c| c.power.power_button == PowerButton::PowerOff,
    )?;
    hp.key(KEY_POWER, true);
    hp.key(KEY_POWER, false);
    system.wait_call(&mut hp, "PowerOff true")?;
    Ok(())
}

// --- The automatic power saver ----------------------------------------------------------

/// On battery, at 20 % or under: power-profiles-daemon's power saver,
/// with one notification; back on AC, the profile it replaced. A user who
/// turns it off is not overruled before the next time on AC; a profile
/// chosen meanwhile is kept on AC.
#[test]
fn the_automatic_saver_turns_on_under_the_level_and_gives_back_the_profile_on_ac()
-> anyhow::Result<()> {
    let Some(system) = System::start(Services::battery(35.0))? else {
        return Ok(());
    };
    let mut hp = system.compositor(&["1920x1080"], config(), None)?;
    system.wait_ready(&mut hp)?;
    assert_eq!(hp.state().power_profile_now(), Some("balanced"));
    system.change(|s| s.level = 21.0)?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(system.profile(), "balanced");
    system.change(|s| s.level = 20.0)?;
    let saver = hp.dispatch_until(WAIT, |_| system.profile() == "power-saver")?;
    anyhow::ensure!(saver, "the power saver did not come: {}", system.profile());
    let told = hp.dispatch_until(WAIT, |_| !system.notifications().is_empty())?;
    anyhow::ensure!(told, "no notification");
    assert_eq!(
        system.notifications(),
        ["Économie d’énergie activée : batterie à 20 %."]
    );
    // Turned off by the user: not again on this discharge.
    let seen = hp.dispatch_until(WAIT, |hp| {
        hp.state().power_profile_now() == Some("power-saver")
    })?;
    anyhow::ensure!(seen, "the profile change never came back");
    system.change(|s| s.profile = "performance".into())?;
    let spent = hp.dispatch_until(WAIT, |hp| *hp.state().power_saver() == Saver::Spent)?;
    anyhow::ensure!(spent, "{:?}", hp.state().power_saver());
    system.change(|s| s.level = 15.0)?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(system.profile(), "performance");
    assert_eq!(system.notifications().len(), 1);
    // On AC: the profile chosen stays; ready again for the next discharge.
    system.change(|s| s.on_battery = false)?;
    let ready = hp.dispatch_until(WAIT, |hp| *hp.state().power_saver() == Saver::Ready)?;
    anyhow::ensure!(ready, "{:?}", hp.state().power_saver());
    assert_eq!(system.profile(), "performance");
    // The next discharge: on again, and back on AC, its profile back.
    system.change(|s| s.on_battery = true)?;
    let saver = hp.dispatch_until(WAIT, |_| system.profile() == "power-saver")?;
    anyhow::ensure!(saver, "not on again: {}", system.profile());
    let seen = hp.dispatch_until(WAIT, |hp| {
        hp.state().power_profile_now() == Some("power-saver")
    })?;
    anyhow::ensure!(seen, "the profile change never came back");
    system.change(|s| s.on_battery = false)?;
    let back = hp.dispatch_until(WAIT, |_| system.profile() == "performance")?;
    anyhow::ensure!(back, "the profile did not come back: {}", system.profile());
    Ok(())
}

// --- Live -------------------------------------------------------------------------------

/// A delay changed on disk moves only the countdown's end, from the same
/// activity: 2 min spent of 5, the dim set to 3 min, it comes 1 min later;
/// the screens' delay changed while locked takes effect at once.
#[test]
fn a_delay_changed_on_disk_moves_only_the_countdowns_end() -> anyhow::Result<()> {
    let file_dir = tempfile::tempdir()?;
    let file = file_dir.path().join("config.toml");
    rewrite(&file, "[idle]\ndelay = 300\n")?;
    let mut set = config();
    set.file = Some(file.clone());
    let mut hp = Headless::start_with(headless(&["1920x1080"])?, set)?;
    begin(&mut hp);
    pass(&mut hp, 2 * MINUTE)?;
    follow(&mut hp, &file, "[idle]\ndelay = 180\n", |c| {
        c.idle.delay == Some(3 * MINUTE)
    })?;
    pass(&mut hp, MINUTE - SECOND)?;
    assert_eq!(hp.away(), Shown::Here);
    pass(&mut hp, SECOND)?;
    assert_eq!(hp.away(), Shown::Curtain { locks: true });
    pass(&mut hp, CURTAIN)?;
    assert_eq!(hp.lock_state(), LockState::Locked);
    assert_eq!(hp.screens_on(), ["HEADLESS-1"]);
    follow(
        &mut hp,
        &file,
        "[idle]\ndelay = 180\n[power.ac]\nscreen-off-after = \"0s\"\n",
        |c| c.power.ac.screen_off_after == after(0),
    )?;
    hp.step(Duration::from_millis(100))?;
    assert!(hp.screens_on().is_empty());
    Ok(())
}

// --- Screenshots ----------------------------------------------------------------------

fn shoot(hp: &mut Headless, index: usize, out: &Path, name: &str) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(index)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The pull request's screenshots, to `$HOCUS_SHOTS` when it names a
/// folder; run by hand (`--run-ignored only`): the power button opening
/// the session menu; the dim held, unlocked, with locking off; the lid
/// closed (`lock`) with an external screen, on battery.
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(system) = System::start(Services::battery(80.0))? else {
        return Ok(());
    };
    let Some(shell) = Shell::alone()? else {
        return Ok(());
    };
    let mut set = config();
    set.plain_background = false;
    set.power.lock_on_idle = false;
    let mut hp = system.compositor(&["1920x1080"], set.clone(), Some(shell.script.clone()))?;
    system.wait_ready(&mut hp)?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    let (_, _window) = open_with(&mut hp, "test.window", (900, 600), false)?;
    common::wait(&mut hp, Duration::from_millis(500))?;
    hp.key(KEY_POWER, true);
    hp.key(KEY_POWER, false);
    let open = hp.dispatch_until(WAIT, |hp| !hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(open, "no menu");
    common::wait(&mut hp, Duration::from_millis(800))?;
    shoot(&mut hp, 0, &out, "power-button-session-menu")?;
    hp.key(1, true);
    hp.key(1, false);
    common::wait(&mut hp, Duration::from_millis(500))?;
    begin(&mut hp);
    pass(&mut hp, 3 * MINUTE + CURTAIN + 3 * SECOND)?;
    anyhow::ensure!(
        hp.away() == Shown::Curtain { locks: false },
        "{:?}",
        hp.away()
    );
    common::wait(&mut hp, Duration::from_millis(300))?;
    hp.step(Duration::from_millis(100))?;
    shoot(&mut hp, 0, &out, "locking-off-dim-held")?;
    drop(hp);

    set.power.lid = Lid::Lock;
    set.power.lock_on_idle = true;
    let mut hp = system.compositor(&["1920x1200", "2560x1440"], set, Some(shell.script.clone()))?;
    hp.set_internal("HEADLESS-1");
    system.wait_ready(&mut hp)?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.lid(true);
    let drawn = hp.dispatch_until(Duration::from_secs(10), Headless::lock_drawn)?;
    anyhow::ensure!(drawn, "no lock");
    common::wait(&mut hp, Duration::from_millis(800))?;
    shoot(&mut hp, 0, &out, "lid-lock-laptop-panel-off")?;
    shoot(&mut hp, 1, &out, "lid-lock-external-screen")?;
    Ok(())
}

// --- Helpers ------------------------------------------------------------------------------

/// `text` written over `file` as the Settings write it.
fn rewrite(file: &Path, text: &str) -> anyhow::Result<()> {
    let new = file.with_extension("new");
    std::fs::write(&new, text)?;
    std::fs::rename(&new, file)?;
    Ok(())
}

/// `text` written over `file`, until the compositor follows it.
fn follow(
    hp: &mut Headless,
    file: &Path,
    text: &str,
    done: impl Fn(&Config) -> bool,
) -> anyhow::Result<()> {
    rewrite(file, text)?;
    let followed = hp.dispatch_until(WAIT, |hp| done(&hp.state().config))?;
    anyhow::ensure!(followed, "the file was not followed: {text}");
    Ok(())
}

// --- The fakes -----------------------------------------------------------------------------

/// What the fake services say and were asked.
#[derive(Debug, Clone)]
struct Services {
    on_battery: bool,
    level: f64,
    profile: String,
    /// logind refuses the lid's and the button's inhibitor.
    refuse_handles: bool,
    /// logind's inhibitors listed.
    idle_inhibitors: Vec<(String, String, String, String, u32, u32)>,
    /// UPower's `LidIsClosed`; `None`: it cannot say (an error).
    lid: Option<bool>,
}

impl Services {
    fn battery(level: f64) -> Self {
        Self {
            on_battery: true,
            level,
            profile: "balanced".into(),
            refuse_handles: false,
            idle_inhibitors: Vec::new(),
            lid: None,
        }
    }

    fn on_ac(mut self) -> Self {
        self.on_battery = false;
        self
    }
}

#[derive(Debug)]
struct Held {
    services: Services,
    /// The other ends of the inhibitors given.
    inhibitors: Vec<UnixStream>,
    calls: Vec<String>,
    listed: usize,
    notifications: Vec<String>,
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
        let mut held = held(&self.0);
        let fine = match (what.as_str(), mode.as_str()) {
            ("sleep", "delay") => true,
            ("handle-lid-switch:handle-power-key", "block") => !held.services.refuse_handles,
            _ => false,
        };
        if !fine {
            return Err(zbus::fdo::Error::AccessDenied(format!("{what} {mode}")));
        }
        let (ours, theirs) =
            UnixStream::pair().map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        held.inhibitors.push(ours);
        Ok(zbus::OwnedFd::from(std::os::fd::OwnedFd::from(theirs)))
    }

    fn can_suspend(&self) -> String {
        "yes".into()
    }

    fn can_reboot(&self) -> String {
        "yes".into()
    }

    fn can_power_off(&self) -> String {
        "yes".into()
    }

    fn list_inhibitors(&self) -> Vec<(String, String, String, String, u32, u32)> {
        let mut held = held(&self.0);
        held.listed += 1;
        held.services.idle_inhibitors.clone()
    }

    fn suspend(&self, interactive: bool) {
        held(&self.0).calls.push(format!("Suspend {interactive}"));
    }

    fn power_off(&self, interactive: bool) {
        held(&self.0).calls.push(format!("PowerOff {interactive}"));
    }
}

struct UPower(Shared);

#[zbus::interface(name = "org.freedesktop.UPower")]
impl UPower {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        held(&self.0).services.on_battery
    }

    #[zbus(property)]
    fn lid_is_closed(&self) -> zbus::fdo::Result<bool> {
        held(&self.0)
            .services
            .lid
            .ok_or_else(|| zbus::fdo::Error::Failed("no lid known".into()))
    }
}

struct Device(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Device {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        held(&self.0).services.level
    }
}

struct Profiles(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        held(&self.0).services.profile.clone()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) {
        held(&self.0).services.profile = profile;
    }
}

struct Notifications(Shared);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app: String,
        _replaces: u32,
        _icon: String,
        summary: String,
        _body: String,
        _actions: Vec<String>,
        _hints: HashMap<String, zbus::OwnedVariant>,
        _timeout: i32,
    ) -> u32 {
        let mut held = held(&self.0);
        held.notifications.push(summary);
        held.notifications.len() as u32
    }
}

/// A system bus and a session bus of the test's own, the fakes on them.
struct System {
    daemons: Vec<std::process::Child>,
    address: String,
    session: String,
    runtime: tokio::runtime::Runtime,
    shared: Shared,
    upower: zbus::Connection,
    login: zbus::Connection,
    _connections: Vec<zbus::Connection>,
    _dir: tempfile::TempDir,
}

impl System {
    fn start(services: Services) -> anyhow::Result<Option<Self>> {
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            common::missing("dbus-daemon");
        }
        let dir = tempfile::tempdir()?;
        let system_dir = dir.path().join("system");
        let session_dir = dir.path().join("session");
        std::fs::create_dir_all(&system_dir)?;
        std::fs::create_dir_all(&session_dir)?;
        let (system, address) = common::private_bus(&system_dir)?;
        let (session_daemon, session) = common::private_bus(&session_dir)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let shared: Shared = Arc::new(Mutex::new(Held {
            services,
            inhibitors: Vec::new(),
            calls: Vec::new(),
            listed: 0,
            notifications: Vec::new(),
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
                .serve_at(
                    "/org/freedesktop/UPower/devices/DisplayDevice",
                    Device(shared.clone()),
                )
                .build(),
        )?;
        let profiles = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.UPower.PowerProfiles")
                .serve_at(
                    "/org/freedesktop/UPower/PowerProfiles",
                    Profiles(shared.clone()),
                )
                .build(),
        )?;
        let notifications = runtime.block_on(
            zbus::connection::Builder::address(session.as_str())
                .name("org.freedesktop.Notifications")
                .serve_at(
                    "/org/freedesktop/Notifications",
                    Notifications(shared.clone()),
                )
                .build(),
        )?;
        Ok(Some(Self {
            daemons: vec![system, session_daemon],
            address,
            session,
            runtime,
            shared,
            upower,
            login,
            _connections: vec![profiles, notifications],
            _dir: dir,
        }))
    }

    fn compositor(
        &self,
        outputs: &[&str],
        config: Config,
        shell: Option<PathBuf>,
    ) -> anyhow::Result<Headless> {
        // With a shell, the session bus is its notifications' own.
        let session = shell.is_none().then(|| self.session.clone());
        Headless::start_on_buses(
            headless(outputs)?,
            config,
            shell,
            session,
            Some(self.address.clone()),
        )
    }

    /// Until the compositor read the system bus and holds its inhibitors.
    fn wait_ready(&self, hp: &mut Headless) -> anyhow::Result<()> {
        let ready = hp.dispatch_until(WAIT, |hp| {
            hp.sleep_inhibited()
                && hp.state().lid_handled()
                && hp.state().power_profile_now().is_some()
        })?;
        anyhow::ensure!(
            ready,
            "the system bus was never read: {}",
            hp.state().away_said()
        );
        Ok(())
    }

    /// What the services say, changed, and their signal.
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

    /// What the services say, changed without a signal: as a lid opened
    /// while the machine slept may never be told.
    fn change_quietly(&self, change: impl FnOnce(&mut Services)) {
        change(&mut held(&self.shared).services);
    }

    fn calls(&self) -> Vec<String> {
        held(&self.shared).calls.clone()
    }

    fn listed(&self) -> usize {
        held(&self.shared).listed
    }

    fn profile(&self) -> String {
        held(&self.shared).services.profile.clone()
    }

    fn notifications(&self) -> Vec<String> {
        held(&self.shared).notifications.clone()
    }

    fn wait_call(&self, hp: &mut Headless, call: &str) -> anyhow::Result<()> {
        let came = hp.dispatch_until(WAIT, |_| self.calls().iter().any(|c| c == call))?;
        anyhow::ensure!(came, "no {call}: {:?}", self.calls());
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
}

impl Drop for System {
    fn drop(&mut self) {
        // The daemons this test started, by their own PIDs.
        for daemon in &mut self.daemons {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
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
