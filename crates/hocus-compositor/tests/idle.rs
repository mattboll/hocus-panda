//! When the session is idle (7.3, docs/design/idle.md), headless, the
//! motions' clock held and stepped (`Headless::step`) so that minutes pass
//! in no time: the delay reached, input starting it again, what holds it
//! off (a shown surface's inhibitor, an `org.freedesktop.ScreenSaver`
//! `Inhibit` on a private bus, the focused window in full screen), the
//! countdown from zero once the last hold goes, what `hocus msg idle`
//! says, the clients' idle notifications, and at rest a single timer that
//! does not wake.

mod common;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use common::{
    Command, Event, KEY_H, KEY_LEFTMETA, Shows, WAIT, chord, client, open_showing, open_with,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, IdleSettings, RendererChoice,
    idle::{Holder, Phase},
};

const DELAY: Duration = Duration::from_secs(300);
const SECOND: Duration = Duration::from_secs(1);
const SCREENSAVER: &str = "org.freedesktop.ScreenSaver";

fn settings() -> Config {
    Config {
        idle: IdleSettings { delay: Some(DELAY) },
        ..Config::default()
    }
}

fn headless() -> HeadlessConfig {
    HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

/// A compositor with a 300 s delay, its clock held, the countdown just
/// started by the user's input.
fn start() -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(headless(), settings())?;
    begin(&mut hp);
    Ok(hp)
}

fn begin(hp: &mut Headless) {
    hp.hold_clock();
    hp.pointer_move(200.0, 200.0);
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

fn phase(hp: &Headless) -> Phase {
    hp.idle().phase
}

/// Waits until what holds it off is as `held` says, and the countdown
/// has seen it (once a turn of the loop).
fn wait_held(hp: &mut Headless, held: impl Fn(&[Holder]) -> bool) -> anyhow::Result<()> {
    let seen = hp.dispatch_until(WAIT, |hp| {
        let said = hp.idle();
        held(&said.holders) && said.holders.is_empty() == (said.phase != Phase::Held)
    })?;
    anyhow::ensure!(seen, "held by {:?}", hp.idle().holders);
    Ok(())
}

/// The countdown has `left` to run, give or take a millisecond.
fn assert_left(hp: &Headless, left: Duration) {
    let said = hp.idle();
    assert_eq!(said.phase, Phase::Counting, "{said:?}");
    let shown = said.left.unwrap_or_default();
    assert!(
        shown.abs_diff(left) < Duration::from_millis(1),
        "{shown:?} left, not {left:?}"
    );
}

/// Idle comes once the delay passes without input, and only then.
#[test]
fn idle_is_reached_after_the_delay() -> anyhow::Result<()> {
    let mut hp = start()?;
    assert_left(&hp, DELAY);
    pass(&mut hp, DELAY - SECOND)?;
    assert_left(&hp, SECOND);
    assert_eq!(hp.idle_counts().reached, 0);
    pass(&mut hp, SECOND)?;
    assert_eq!(phase(&hp), Phase::Reached);
    assert_eq!(hp.idle_counts().reached, 1);
    // Reached once, however long it stays idle.
    pass(&mut hp, DELAY)?;
    assert_eq!(hp.idle_counts().reached, 1);
    Ok(())
}

/// Input starts the countdown again; after idle, it is counting again.
#[test]
fn input_starts_the_countdown_again() -> anyhow::Result<()> {
    let mut hp = start()?;
    pass(&mut hp, Duration::from_secs(200))?;
    hp.key(KEY_H, true);
    hp.key(KEY_H, false);
    pass(&mut hp, Duration::from_secs(200))?;
    assert_left(&hp, Duration::from_secs(100));
    pass(&mut hp, Duration::from_secs(100))?;
    assert_eq!(phase(&hp), Phase::Reached);
    hp.pointer_move(300.0, 300.0);
    assert_left(&hp, DELAY);
    pass(&mut hp, DELAY)?;
    assert_eq!(hp.idle_counts().reached, 2);
    Ok(())
}

/// Five minutes by default (7.2); `"off"`: never idle, and no timer for
/// it.
#[test]
fn five_minutes_by_default_and_never_when_off() -> anyhow::Result<()> {
    let hp = Headless::start(headless())?;
    assert_eq!(hp.idle().delay, Some(DELAY));
    drop(hp);
    let off = Config {
        idle: IdleSettings { delay: None },
        ..Config::default()
    };
    let mut hp = Headless::start_with(headless(), off)?;
    begin(&mut hp);
    pass(&mut hp, Duration::from_secs(3600))?;
    let said = hp.idle();
    assert_eq!(
        (said.phase, said.delay, said.left),
        (Phase::Off, None, None)
    );
    assert_eq!(hp.idle_counts().reached, 0);
    assert!(!hp.idle_counts().armed);
    Ok(())
}

/// A window's idle inhibitor holds it while the window shows; minimised,
/// it no longer does, and the countdown runs.
#[test]
fn a_shown_surfaces_inhibitor_holds_it_and_a_hidden_ones_does_not() -> anyhow::Result<()> {
    let mut hp = start()?;
    let (_, video) = open_with(&mut hp, "test.video", (640, 360), false)?;
    video.commands.send(Command::Inhibit(true))?;
    wait_held(&mut hp, |h| h == [Holder::Inhibitor("test.video".into())])?;
    assert_eq!(phase(&hp), Phase::Held);
    pass(&mut hp, DELAY * 2)?;
    assert_eq!(phase(&hp), Phase::Held);
    assert_eq!(hp.idle_counts().reached, 0);
    // Super+H minimises the focused window: input, then the hold goes.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_H]);
    wait_held(&mut hp, <[Holder]>::is_empty)?;
    assert_left(&hp, DELAY);
    pass(&mut hp, DELAY)?;
    assert_eq!(phase(&hp), Phase::Reached);
    Ok(())
}

/// An inhibitor destroyed lets go: the countdown starts again from zero,
/// however long ago the last input was.
#[test]
fn the_countdown_starts_from_zero_when_the_last_hold_goes() -> anyhow::Result<()> {
    let mut hp = start()?;
    let (_, video) = open_with(&mut hp, "test.video", (640, 360), false)?;
    video.commands.send(Command::Inhibit(true))?;
    wait_held(&mut hp, |h| h.len() == 1)?;
    pass(&mut hp, DELAY + Duration::from_secs(100))?;
    video.commands.send(Command::Inhibit(false))?;
    wait_held(&mut hp, <[Holder]>::is_empty)?;
    assert_left(&hp, DELAY);
    pass(&mut hp, DELAY - SECOND)?;
    assert_eq!(phase(&hp), Phase::Counting);
    pass(&mut hp, SECOND)?;
    assert_eq!(phase(&hp), Phase::Reached);
    Ok(())
}

/// The focused window in full screen holds it; out of full screen, the
/// countdown starts again from zero.
#[test]
fn the_focused_fullscreen_window_holds_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    let (_, film) = open_showing(
        &mut hp,
        "test.film",
        Shows::Asking {
            app_id: "test.film",
            size: (640, 360),
            fullscreen: true,
        },
    )?;
    wait_held(&mut hp, |h| h == [Holder::Fullscreen("test.film".into())])?;
    pass(&mut hp, DELAY * 2)?;
    assert_eq!(phase(&hp), Phase::Held);
    film.commands.send(Command::Fullscreen(false))?;
    wait_held(&mut hp, <[Holder]>::is_empty)?;
    assert_left(&hp, DELAY);
    pass(&mut hp, DELAY)?;
    assert_eq!(phase(&hp), Phase::Reached);
    Ok(())
}

/// A compositor on a private bus, and a caller of its own there.
struct Bus {
    hp: Headless,
    daemon: std::process::Child,
    address: String,
    runtime: tokio::runtime::Runtime,
    _dir: tempfile::TempDir,
}

impl Bus {
    fn start() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let (daemon, address) = common::private_bus(dir.path())?;
        let mut hp = Headless::start_on_bus(headless(), settings(), None, Some(address.clone()))?;
        begin(&mut hp);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        Ok(Self {
            hp,
            daemon,
            address,
            runtime,
            _dir: dir,
        })
    }

    fn caller(&self) -> anyhow::Result<zbus::Connection> {
        let address = self.address.clone();
        Ok(self
            .runtime
            .block_on(zbus::connection::Builder::address(address.as_str()).build())?)
    }

    /// `Inhibit(app, reason)` at `path`, once the compositor serves it.
    fn inhibit(
        &self,
        caller: &zbus::Connection,
        path: &str,
        app: &str,
        reason: &str,
    ) -> anyhow::Result<u32> {
        let start = std::time::Instant::now();
        loop {
            let reply = self.runtime.block_on(caller.call_method(
                Some(SCREENSAVER),
                path,
                Some(SCREENSAVER),
                "Inhibit",
                &(app, reason),
            ));
            match reply {
                Ok(reply) => return Ok(reply.body().deserialize::<u32>()?),
                Err(err) if start.elapsed() < WAIT => {
                    eprintln!("not served yet: {err}");
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    fn un_inhibit(&self, caller: &zbus::Connection, cookie: u32) -> anyhow::Result<()> {
        self.runtime.block_on(caller.call_method(
            Some(SCREENSAVER),
            "/org/freedesktop/ScreenSaver",
            Some(SCREENSAVER),
            "UnInhibit",
            &cookie,
        ))?;
        Ok(())
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // The daemon this test started, by its own PID.
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn screensaver(app: &str, reason: &str) -> Holder {
    Holder::ScreenSaver {
        app: app.into(),
        reason: reason.into(),
    }
}

/// A browser's `Inhibit` for a video holds it until its `UnInhibit`, then
/// the countdown starts again from zero.
#[test]
fn a_screensaver_inhibit_holds_it_until_uninhibit() -> anyhow::Result<()> {
    let mut bus = Bus::start()?;
    let caller = bus.caller()?;
    let cookie = bus.inhibit(
        &caller,
        "/org/freedesktop/ScreenSaver",
        "Firefox",
        "video-playing",
    )?;
    assert_ne!(cookie, 0);
    wait_held(&mut bus.hp, |h| {
        h == [screensaver("Firefox", "video-playing")]
    })?;
    pass(&mut bus.hp, DELAY * 2)?;
    assert_eq!(phase(&bus.hp), Phase::Held);
    // Another caller's cookie does not release it.
    let other = bus.caller()?;
    bus.un_inhibit(&other, cookie)?;
    bus.un_inhibit(&caller, cookie)?;
    wait_held(&mut bus.hp, <[Holder]>::is_empty)?;
    assert_left(&bus.hp, DELAY);
    pass(&mut bus.hp, DELAY)?;
    assert_eq!(phase(&bus.hp), Phase::Reached);
    Ok(())
}

/// An application that leaves the bus without `UnInhibit` (it crashed, or
/// quit) lets go all the same; the other path is served too.
#[test]
fn a_screensaver_inhibit_ends_when_its_caller_leaves_the_bus() -> anyhow::Result<()> {
    let mut bus = Bus::start()?;
    let staying = bus.caller()?;
    bus.inhibit(
        &staying,
        "/ScreenSaver",
        "Chromium",
        "WebRTC has active PeerConnections",
    )?;
    let leaving = {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let caller =
            runtime.block_on(zbus::connection::Builder::address(bus.address.as_str()).build())?;
        let reply = runtime.block_on(caller.call_method(
            Some(SCREENSAVER),
            "/org/freedesktop/ScreenSaver",
            Some(SCREENSAVER),
            "Inhibit",
            &("mpv", "Playing"),
        ))?;
        let cookie = reply.body().deserialize::<u32>()?;
        (runtime, caller, cookie)
    };
    wait_held(&mut bus.hp, |h| h.len() == 2)?;
    let (runtime, caller, _) = leaving;
    drop(caller);
    drop(runtime);
    wait_held(&mut bus.hp, |h| {
        h == [screensaver("Chromium", "WebRTC has active PeerConnections")]
    })?;
    drop(staying);
    wait_held(&mut bus.hp, <[Holder]>::is_empty)?;
    assert_left(&bus.hp, DELAY);
    Ok(())
}

/// The control socket's question, from a thread as `hocus msg idle` asks
/// it, while the loop runs.
fn ask_idle(hp: &mut Headless) -> anyhow::Result<String> {
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(b"msg idle\n")?;
            let mut answer = String::new();
            stream.read_to_string(&mut answer)?;
            Ok(answer)
        })();
        let _ = said.send(asked);
    });
    let mut answer = None;
    hp.dispatch_until(WAIT, |_| {
        if answer.is_none() {
            answer = heard.try_recv().ok();
        }
        answer.is_some()
    })?;
    Ok(answer.context("no answer")??)
}

/// `hocus msg idle` says the state, the time left and who holds it.
#[test]
fn hocus_msg_idle_says_who_holds_it() -> anyhow::Result<()> {
    let mut bus = Bus::start()?;
    pass(&mut bus.hp, Duration::from_secs(60))?;
    assert_eq!(
        ask_idle(&mut bus.hp)?,
        "state counting\ndelay 300 s\nleft 240.0 s\nheld-by nothing\n\
         away here\nscreens on\npower ac\nsleep-inhibitor none\n"
    );
    let (_, video) = open_with(&mut bus.hp, "test.video", (640, 360), false)?;
    video.commands.send(Command::Inhibit(true))?;
    let caller = bus.caller()?;
    bus.inhibit(
        &caller,
        "/org/freedesktop/ScreenSaver",
        "Firefox",
        "video-playing",
    )?;
    wait_held(&mut bus.hp, |h| h.len() == 2)?;
    assert_eq!(
        ask_idle(&mut bus.hp)?,
        "state held\ndelay 300 s\nheld-by inhibitor test.video\n\
         held-by screensaver Firefox: video-playing\n\
         away here\nscreens on\npower ac\nsleep-inhibitor none\n"
    );
    Ok(())
}

/// Clients' idle notifications: one honours what holds idleness off, an
/// input one does not; input resumes both.
#[test]
fn idle_notifications_honour_holds_except_input_ones() -> anyhow::Result<()> {
    let mut hp = start()?;
    let (_, video) = open_with(&mut hp, "test.video", (640, 360), false)?;
    let locker = client(hp.socket(), None)?;
    locker.commands.send(Command::IdleNotification {
        timeout_ms: 10_000,
        input: false,
    })?;
    locker.commands.send(Command::IdleNotification {
        timeout_ms: 20_000,
        input: true,
    })?;
    let heard = |hp: &mut Headless, want: Event| -> anyhow::Result<()> {
        let mut seen = None;
        hp.dispatch_until(WAIT, |_| {
            if seen.is_none() {
                seen = locker
                    .events
                    .try_iter()
                    .find(|e| !matches!(e, Event::Globals(_)));
            }
            seen.is_some()
        })?;
        anyhow::ensure!(seen.as_ref() == Some(&want), "{seen:?}, not {want:?}");
        Ok(())
    };
    // Made: the compositor has both before the clock moves.
    let made = hp.dispatch_until(WAIT, |hp| hp.idle_counts().notifications == 2)?;
    anyhow::ensure!(made, "{:?}", hp.idle_counts());
    hp.pointer_move(210.0, 210.0);
    pass(&mut hp, Duration::from_secs(11))?;
    heard(&mut hp, Event::Idled { input: false })?;
    hp.pointer_move(220.0, 220.0);
    heard(&mut hp, Event::Resumed { input: false })?;
    video.commands.send(Command::Inhibit(true))?;
    wait_held(&mut hp, |h| h.len() == 1)?;
    pass(&mut hp, Duration::from_secs(21))?;
    heard(&mut hp, Event::Idled { input: true })?;
    pass(&mut hp, Duration::from_secs(60))?;
    hp.dispatch(Duration::from_millis(50))?;
    assert!(
        locker
            .events
            .try_iter()
            .all(|e| matches!(e, Event::Globals(_))),
        "the notification that honours holds became idle"
    );
    hp.pointer_move(230.0, 230.0);
    heard(&mut hp, Event::Resumed { input: true })?;
    Ok(())
}

/// At rest one timer waits for the delay; input moves no timer, puts none
/// in and wakes nothing; it fires once, at the end.
#[test]
fn at_rest_one_timer_and_no_wakeup_beyond_it() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(headless(), settings())?;
    common::wait(&mut hp, Duration::from_millis(300))?;
    let counts = hp.idle_counts();
    assert!(counts.armed, "{counts:?}");
    assert_eq!((counts.timers, counts.wakeups), (1, 0), "{counts:?}");
    for i in 0..200 {
        hp.pointer_move(100.0 + f64::from(i), 300.0);
        if i % 20 == 0 {
            hp.dispatch(Duration::from_millis(5))?;
        }
    }
    common::wait(&mut hp, Duration::from_millis(300))?;
    let counts = hp.idle_counts();
    assert!(counts.armed, "{counts:?}");
    assert_eq!((counts.timers, counts.wakeups), (1, 0), "{counts:?}");
    // The held clock past the deadline: reached, and no timer left.
    hp.hold_clock();
    pass(&mut hp, DELAY + SECOND)?;
    let counts = hp.idle_counts();
    assert_eq!((counts.armed, counts.reached), (false, 1), "{counts:?}");
    Ok(())
}
