//! The lock screen (7.1, issue #412, docs/design/lock.md): one test per
//! point of the approved spec, the session locked with
//! [`Headless::lock`] as `Super+L` and the idle will, the password
//! checked by the tests' own authenticator (`Authenticator::test`, built
//! for these tests only: never PAM, never anyone's password).
//!
//! Without the shell, a locked screen is black: what is checked there is
//! what the compositor guarantees alone (nothing drawn, no input to any
//! client, captures of the lock). With the real shell (from beside the
//! test, drawing on the CPU), what it draws, as it logs it: the form on
//! the primary screen, the clock alone on the others, Caps Lock, the
//! wrong password, the wait, its crash, the unlock, and nothing drawn at
//! rest but the minute.
//!
//! Pixman here, so the shell draws on the CPU: nothing draws on a GPU.

use crate::common;

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use common::{
    BTN_LEFT, ClientHandle, Event, KEY_ESC, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB,
    Pointed, Shows, WAIT, WINDOW_COLOUR, chord, open_showing, shell_program, wait,
};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, LockState, OutputSpec, RendererChoice,
};
use hocus_link::{LockFocus, LockPhase};

const KEY_ENTER: u32 = 28;
const KEY_CAPSLOCK: u32 = 58;
const KEY_PRINT: u32 = 99;
const KEY_SPACE: u32 = 57;
const KEY_RETURN: u32 = KEY_ENTER;
/// The tests' password, and a wrong one.
const RIGHT: &str = "panda";
const WRONG: &str = "koala";

/// A US keyboard's lower-case letters, by their evdev codes.
fn letter(c: char) -> u32 {
    match c {
        'a' => 30,
        'b' => 48,
        'd' => 32,
        'k' => 37,
        'l' => 38,
        'n' => 49,
        'o' => 24,
        'p' => 25,
        'x' => 45,
        other => panic!("no key for {other:?} here"),
    }
}

fn type_text(hp: &mut Headless, text: &str) {
    for c in text.chars() {
        chord(hp, &[letter(c)]);
    }
}

/// A compositor alone (no shell) with the tests' authenticator, its
/// pictures in `dir`.
fn alone(outputs: &[&str], dir: &Path) -> anyhow::Result<Headless> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.plain_background = true;
    config.lock_auth = Authenticator::test(RIGHT);
    config.pictures = Some(dir.join("Pictures"));
    let outputs = outputs
        .iter()
        .map(|o| o.parse())
        .collect::<Result<Vec<OutputSpec>, _>>()?;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs,
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    anyhow::ensure!(drawn, "nothing drawn");
    Ok(hp)
}

/// Whether output `index`'s screen shows a pixel of `rgb`.
fn shows(hp: &mut Headless, index: usize, rgb: [u8; 3]) -> anyhow::Result<bool> {
    let (_, _, px) = hp.screenshot(index)?;
    Ok(px
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| [p[2], p[1], p[0]] == rgb))
}

/// Whether output `index`'s screen is black throughout, but for the
/// cursor (the compositor draws it, 64 px around the pointer at most).
fn black(hp: &mut Headless, index: usize) -> anyhow::Result<bool> {
    let origin = hp
        .outputs()
        .get(index)
        .map(|(_, r)| (r.x, r.y))
        .unwrap_or((0, 0));
    let (px, py) = hp.pointer();
    let (cx, cy) = (px as i32 - origin.0, py as i32 - origin.1);
    let (width, _, pixels) = hp.screenshot(index)?;
    Ok(pixels.as_chunks::<4>().0.iter().enumerate().all(|(i, p)| {
        let (x, y) = (i as i32 % width, i as i32 / width);
        let cursor = (cx - 8..cx + 64).contains(&x) && (cy - 8..cy + 64).contains(&y);
        cursor || p[..3] == [0, 0, 0]
    }))
}

const WINDOW_RGB: [u8; 3] = [
    (WINDOW_COLOUR >> 16) as u8,
    (WINDOW_COLOUR >> 8) as u8,
    WINDOW_COLOUR as u8,
];

/// The keys a typing client got until `code`'s press came.
fn keys_until(
    hp: &mut Headless,
    events: &mpsc::Receiver<Event>,
    code: u32,
) -> anyhow::Result<Vec<(u32, bool)>> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = events.try_recv() {
            if let Event::Key(k, pressed) = event {
                seen.push((k, pressed));
            }
        }
        seen.contains(&(code, true))
    })?;
    anyhow::ensure!(came, "key {code} never came: {seen:?}");
    Ok(seen)
}

/// What a pointing client's pointer got until `last` came.
fn pointed_until(
    hp: &mut Headless,
    client: &ClientHandle,
    last: Pointed,
) -> anyhow::Result<Vec<Pointed>> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = client.events.try_recv() {
            if let Event::Pointer(p) = event {
                seen.push(p);
            }
        }
        seen.last() == Some(&last)
    })?;
    anyhow::ensure!(came, "{last:?} never came: {seen:?}");
    Ok(seen)
}

/// Types the password and `Enter`, then waits for the lock to open.
fn unlock(hp: &mut Headless) -> anyhow::Result<()> {
    type_text(hp, RIGHT);
    chord(hp, &[KEY_ENTER]);
    let open = hp.dispatch_until(WAIT, |hp| hp.lock_state() == LockState::Unlocked)?;
    anyhow::ensure!(open, "the lock did not open: {:?}", hp.lock_shown());
    Ok(())
}

/// Waits until the lock says `phase`.
fn phase(hp: &mut Headless, phase: LockPhase) -> anyhow::Result<()> {
    let came = hp.dispatch_until(WAIT, |hp| hp.lock_shown().is_some_and(|s| s.phase == phase))?;
    anyhow::ensure!(came, "never {phase:?}: {:?}", hp.lock_shown());
    Ok(())
}

// --- With the real shell ---------------------------------------------------------

/// The real shell, away from the machine's services, its log in `log`.
struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
}

impl Bed {
    fn start(outputs: &[&str], clock_at: Option<&str>) -> anyhow::Result<Option<Self>> {
        let shell = shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let log = root.join("shell.log");
        let at = |sub: &str| root.join(sub).display().to_string();
        let mut env = vec![
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("HOCUS_USER_NAME", "Alex".to_owned()),
            ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ];
        if let Some(at) = clock_at {
            env.push(("HOCUS_CLOCK_AT", at.to_owned()));
        }
        let script = common::shell_script(root, &shell, &env)?;
        // Its log beside it.
        let text = std::fs::read_to_string(&script)?;
        let text = text.replace("\"$@\"\n", &format!("\"$@\" 2>>'{}'\n", log.display()));
        std::fs::write(&script, text)?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.lock_auth = Authenticator::test(RIGHT);
        let outputs = outputs
            .iter()
            .map(|o| o.parse())
            .collect::<Result<Vec<OutputSpec>, _>>()?;
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self { hp, _dir: dir, log };
        let up = bed
            .hp
            .dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default())
            .chars()
            .filter(|c| *c != '\u{1b}')
            .collect()
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for a line of the shell's log after `from` holding every one
    /// of `texts`.
    fn logged_after(&mut self, from: usize, texts: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let read = |log: &Path| {
            String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default())
                .chars()
                .filter(|c| *c != '\u{1b}')
                .collect::<String>()
        };
        let found = |all: &str| {
            all.get(from..)
                .unwrap_or_default()
                .lines()
                .find(|l| texts.iter().all(|t| l.contains(t)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&read(&log)).is_some())?;
        anyhow::ensure!(
            done,
            "the shell never logged {texts:?}; it said:\n{}",
            read(&log).get(from..).unwrap_or_default()
        );
        found(&read(&log)).context("gone")
    }

    /// Locks, and waits until the shell's lock shows on every screen.
    fn lock(&mut self) -> anyhow::Result<()> {
        let screens = self.hp.outputs().len();
        let from = self.mark();
        self.hp.lock();
        let shown = self
            .hp
            .dispatch_until(WAIT, |hp| hp.lock_drawn_on().len() == screens)?;
        anyhow::ensure!(shown, "the shell's lock never showed");
        self.logged_after(from, &["the lock is drawn"])?;
        common::at_rest(&mut self.hp)
    }

    fn typing(&mut self) -> anyhow::Result<ClientHandle> {
        let app_id = "test.typing";
        let (_, client) = open_showing(
            &mut self.hp,
            app_id,
            Shows::Typing {
                app_id,
                size: (800, 600),
            },
        )?;
        Ok(client)
    }
}

// --- The tests ---------------------------------------------------------------------

/// Locked, no window is drawn, not even under the lock, nor a client's
/// surface claiming to be a lock; no key, click or motion reaches a
/// client; the desktop's shortcuts do nothing. Unlocked, they do again.
#[test]
fn locked_no_window_is_drawn_and_no_input_reaches_a_client() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = alone(&["1920x1080"], dir.path())?;
    let pointing_id = "test.pointing";
    let (rect, pointing) = open_showing(
        &mut hp,
        pointing_id,
        Shows::Pointing {
            app_id: pointing_id,
            size: (600, 400),
        },
    )?;
    // Opened last, it has the keyboard.
    let typing_id = "test.typing";
    let (_, typing) = open_showing(
        &mut hp,
        typing_id,
        Shows::Typing {
            app_id: typing_id,
            size: (500, 300),
        },
    )?;
    // A client's own surface in the lock's namespace: never the lock.
    let fake = common::client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: hocus_link::LOCK_NAMESPACE,
            size: (300, 300),
            margin: (0, 0),
            colour: 0xff00_ff00,
        }),
    )?;
    common::at_rest(&mut hp)?;
    assert!(shows(&mut hp, 0, WINDOW_RGB)?, "the windows show before");
    // Over the pointing window, clear of the typing one.
    let (x, y) = (f64::from(rect.x + 20), f64::from(rect.y + rect.height - 20));
    hp.pointer_move(x, y);
    pointed_until(&mut hp, &pointing, Pointed::Enter)?;

    hp.lock();
    assert!(hp.state().locked(), "locked at once");
    pointed_until(&mut hp, &pointing, Pointed::Leave)?;
    common::at_rest(&mut hp)?;
    assert!(black(&mut hp, 0)?, "only black: no window, no fake lock");
    assert!(
        hp.windows().iter().all(|w| !w.keyboard),
        "nobody has the keyboard"
    );
    // Keys, the pointer and its buttons reach nobody.
    chord(&mut hp, &[letter('x')]);
    hp.pointer_move(x + 5.0, y - 5.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert_eq!(hp.pointer_window(), None);
    // The desktop's shortcuts: Alt+Tab, Super+Space, Print, Super+Return.
    chord(&mut hp, &[KEY_LEFTALT, KEY_TAB]);
    assert!(hp.switcher().is_none(), "no Alt+Tab");
    chord(&mut hp, &[KEY_LEFTMETA, KEY_SPACE]);
    chord(&mut hp, &[KEY_PRINT]);
    assert_eq!(hp.capture(), hocus_compositor::CaptureShown::Closed);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RETURN]);
    assert!(hp.state().locked());
    // The x went into the lock's own field; locking again keeps it.
    hp.lock();
    assert_eq!(hp.lock_typed(), Some(1));
    chord(&mut hp, &[KEY_ESC]);
    assert_eq!(hp.lock_typed(), Some(0), "Esc empties the field");
    assert!(black(&mut hp, 0)?);

    unlock(&mut hp)?;
    common::at_rest(&mut hp)?;
    assert!(shows(&mut hp, 0, WINDOW_RGB)?, "the windows show again");
    // The typing window has the keyboard again: its next key is the first
    // it gets since the lock.
    chord(&mut hp, &[letter('b')]);
    let keys = keys_until(&mut hp, &typing.events, letter('b'))?;
    for c in "xpanda".chars() {
        assert!(
            !keys.contains(&(letter(c), true)),
            "{c} reached the window: {keys:?}"
        );
    }
    assert!(!keys.contains(&(KEY_ESC, true)) && !keys.contains(&(KEY_ENTER, true)));
    hp.pointer_move(x + 2.0, y);
    let seen = pointed_until(&mut hp, &pointing, Pointed::Motion)?;
    assert!(
        !seen.contains(&Pointed::Press),
        "no press while locked: {seen:?}"
    );
    drop(fake);
    Ok(())
}

/// A capture taken as the session locks, of the screen or of a window,
/// shows the lock (black without the shell), never the desktop; while
/// locked, `Print` takes none.
#[test]
fn a_capture_while_locked_shows_no_desktop() -> anyhow::Result<()> {
    for window in [false, true] {
        let dir = tempfile::tempdir()?;
        let mut hp = alone(&["1280x720"], dir.path())?;
        let app_id = "test.window";
        let (_, client) = open_showing(
            &mut hp,
            app_id,
            Shows::Window {
                app_id,
                size: (600, 400),
                popup: false,
            },
        )?;
        common::at_rest(&mut hp)?;
        hp.pointer_move(200.0, 200.0);
        // Asked, then locked before it is drawn.
        let modifier = if window { KEY_LEFTALT } else { KEY_LEFTSHIFT };
        chord(&mut hp, &[modifier, KEY_PRINT]);
        hp.lock();
        let taken = hp.dispatch_until(WAIT, |hp| hp.captures().0 >= 1)?;
        anyhow::ensure!(taken, "no capture made");
        let file = hp.captures().1.context("no file")?;
        let image = image::open(&file)?.to_rgba8();
        assert!(
            image.pixels().all(|p| p.0[..3] == [0, 0, 0]),
            "the capture shows something else than the black lock (window: {window})"
        );
        // Locked, `Print` and its siblings take nothing.
        chord(&mut hp, &[KEY_LEFTSHIFT, KEY_PRINT]);
        chord(&mut hp, &[KEY_PRINT]);
        wait(&mut hp, Duration::from_millis(300))?;
        assert_eq!(hp.captures().0, 1, "no capture while locked");
        drop(client);
    }
    Ok(())
}

/// A wrong password: checked off the loop, the field empty again, shaken
/// (on a GPU), its border and its line say so; typing again brings the
/// field back. The right one opens the lock. No key reaches the window.
#[test]
fn a_wrong_password_then_the_right_one() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080"], None)? else {
        return Ok(());
    };
    let typing = bed.typing()?;
    bed.lock()?;
    let from = bed.mark();
    type_text(&mut bed.hp, WRONG);
    assert_eq!(bed.hp.lock_typed(), Some(5));
    bed.logged_after(from, &["the lock is drawn", "Typing", "chars: 5"])?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_ENTER]);
    phase(&mut bed.hp, LockPhase::Wrong)?;
    let shown = bed.hp.lock_shown().context("no lock")?;
    assert_eq!((shown.chars, shown.shake), (0, 1), "emptied, shaken once");
    assert_eq!(bed.hp.lock_typed(), Some(0));
    bed.logged_after(from, &["the lock is drawn", "Wrong", "chars: 0"])?;
    assert_eq!(bed.hp.lock_checks(), 1);
    // Typing again: the field again.
    type_text(&mut bed.hp, "p");
    phase(&mut bed.hp, LockPhase::Typing)?;
    chord(&mut bed.hp, &[14]);
    phase(&mut bed.hp, LockPhase::Idle)?;
    unlock(&mut bed.hp)?;
    assert_eq!(bed.hp.lock_checks(), 2);
    // The window's first key since: the one after the unlock.
    chord(&mut bed.hp, &[letter('b')]);
    let keys = keys_until(&mut bed.hp, &typing.events, letter('b'))?;
    for c in "koalapnd".chars() {
        assert!(
            !keys.contains(&(letter(c), true)),
            "{c} reached the window: {keys:?}"
        );
    }
    Ok(())
}

/// Caps Lock: on as it locks, the lock says so at once; switched as the
/// user types, at once too.
#[test]
fn caps_lock_shows_as_it_locks_and_from_the_first_key() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080"], None)? else {
        return Ok(());
    };
    chord(&mut bed.hp, &[KEY_CAPSLOCK]);
    let from = bed.mark();
    bed.lock()?;
    assert!(
        bed.hp.lock_shown().is_some_and(|s| s.caps),
        "on as it locks"
    );
    bed.logged_after(from, &["the lock is drawn", "caps: true"])?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_CAPSLOCK]);
    assert!(bed.hp.lock_shown().is_some_and(|s| !s.caps), "off at once");
    bed.logged_after(from, &["the lock is drawn", "caps: false"])?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_CAPSLOCK]);
    type_text(&mut bed.hp, "p");
    assert!(bed.hp.lock_shown().is_some_and(|s| s.caps && s.chars == 1));
    bed.logged_after(from, &["the lock is drawn", "caps: true", "chars: 1"])?;
    // Caps Lock off, the password opens it.
    chord(&mut bed.hp, &[KEY_CAPSLOCK]);
    chord(&mut bed.hp, &[KEY_ESC]);
    unlock(&mut bed.hp)
}

/// PAM's refusal after three wrong passwords (the tests' faillock): the
/// field waits 30 s, counting down, taking nothing, then takes a password
/// again.
#[test]
fn too_many_attempts_wait_30_seconds_then_the_field_again() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080"], None)? else {
        return Ok(());
    };
    bed.lock()?;
    for n in 1..=3 {
        type_text(&mut bed.hp, WRONG);
        chord(&mut bed.hp, &[KEY_ENTER]);
        let wrong = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.lock_shown().is_some_and(|s| s.shake == n))?;
        anyhow::ensure!(wrong, "attempt {n} never wrong");
    }
    let from = bed.mark();
    type_text(&mut bed.hp, RIGHT);
    chord(&mut bed.hp, &[KEY_ENTER]);
    phase(&mut bed.hp, LockPhase::Waiting(30))?;
    bed.logged_after(from, &["the lock is drawn", "Waiting(30)"])?;
    // Nothing is typed meanwhile.
    type_text(&mut bed.hp, RIGHT);
    assert_eq!(bed.hp.lock_typed(), Some(0));
    // The wait counts down on the compositor's clock, held here.
    bed.hp.step(Duration::from_secs(10))?;
    phase(&mut bed.hp, LockPhase::Waiting(20))?;
    bed.hp.step(Duration::from_secs(20))?;
    phase(&mut bed.hp, LockPhase::Idle)?;
    bed.hp.release_clock();
    unlock(&mut bed.hp)
}

/// Two screens: the form on the bar's, the clock alone on the other.
#[test]
fn a_second_screen_shows_the_clock_alone() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080", "2560x1440"], None)? else {
        return Ok(());
    };
    let outputs: Vec<String> = bed.hp.outputs().into_iter().map(|(n, _)| n).collect();
    let primary = bed
        .hp
        .bars()
        .first()
        .map(|(n, _, _)| n.clone())
        .context("no bar")?;
    let other = outputs
        .iter()
        .find(|n| **n != primary)
        .context("one screen")?
        .clone();
    let from = bed.mark();
    bed.lock()?;
    assert_eq!(bed.hp.lock_shown().map(|s| s.output), Some(primary.clone()));
    bed.logged_after(
        from,
        &[
            "the lock is drawn",
            &format!("output: \"{primary}\""),
            "form: true",
        ],
    )?;
    bed.logged_after(
        from,
        &[
            "the lock is drawn",
            &format!("output: \"{other}\""),
            "form: false",
        ],
    )?;
    // Both drawn, neither black.
    for index in 0..2 {
        assert!(!black(&mut bed.hp, index)?, "screen {index} shows the lock");
    }
    unlock(&mut bed.hp)
}

/// The shell ending while locked (a crash): the session stays locked,
/// the screens black, no key to a window; the shell comes back with the
/// lock, which opens with the password.
#[test]
fn a_shell_crash_keeps_the_session_locked() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080"], None)? else {
        return Ok(());
    };
    let typing = bed.typing()?;
    bed.lock()?;
    let first = bed.hp.shell_pid();
    bed.hp.kill_shell()?;
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.lock_surfaces().is_empty())?;
    assert!(gone, "its lock surfaces went with it");
    assert!(bed.hp.state().locked(), "still locked");
    common::at_rest(&mut bed.hp)?;
    assert!(black(&mut bed.hp, 0)?, "black, no window");
    chord(&mut bed.hp, &[letter('x')]);
    // Back, it draws the lock again.
    let back = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.shell_connected() && hp.shell_pid() != first && hp.lock_drawn_on().len() == 1
    })?;
    assert!(back, "the shell came back with the lock");
    assert!(bed.hp.state().locked());
    chord(&mut bed.hp, &[KEY_ESC]);
    unlock(&mut bed.hp)?;
    chord(&mut bed.hp, &[letter('b')]);
    let keys = keys_until(&mut bed.hp, &typing.events, letter('b'))?;
    assert!(
        !keys.contains(&(letter('x'), true)),
        "x reached the window: {keys:?}"
    );
    Ok(())
}

/// Unlocked: the shell is told, its lock surfaces go, the desktop shows
/// as it was, the focused window has the keyboard, the bar's clock draws
/// again.
#[test]
fn unlocking_gives_the_desktop_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(&["1920x1080"], None)? else {
        return Ok(());
    };
    let typing = bed.typing()?;
    common::at_rest(&mut bed.hp)?;
    assert!(shows(&mut bed.hp, 0, WINDOW_RGB)?);
    bed.lock()?;
    assert!(
        !shows(&mut bed.hp, 0, WINDOW_RGB)?,
        "the window does not show"
    );
    let clock = bed.hp.clock_commits();
    let from = bed.mark();
    unlock(&mut bed.hp)?;
    bed.logged_after(from, &["the session is unlocked"])?;
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.lock_surfaces().is_empty())?;
    assert!(gone, "the lock's surfaces went");
    common::at_rest(&mut bed.hp)?;
    assert!(shows(&mut bed.hp, 0, WINDOW_RGB)?, "the window shows again");
    assert!(
        bed.hp.windows().iter().any(|w| w.keyboard),
        "the window has the keyboard"
    );
    let ticks = bed
        .hp
        .dispatch_until(Duration::from_secs(3), |hp| hp.clock_commits() > clock)?;
    assert!(ticks, "the bar's clock draws again");
    chord(&mut bed.hp, &[letter('b')]);
    keys_until(&mut bed.hp, &typing.events, letter('b'))?;
    Ok(())
}

/// At rest while locked, nothing draws, neither the compositor nor the
/// shell (its bar's clock waits too), until the minute changes: the
/// lock's clock then draws once.
#[test]
fn at_rest_while_locked_nothing_wakes_but_the_minute() -> anyhow::Result<()> {
    // Eighteen seconds before a minute changes, as the shell starts.
    let Some(mut bed) = Bed::start(&["1920x1080"], Some("10:00:42"))? else {
        return Ok(());
    };
    bed.lock()?;
    let counts = |hp: &Headless| {
        (
            hp.frames(),
            hp.lock_commits(),
            hp.clock_commits(),
            hp.bar_commits(),
            hp.levels_commits(),
            hp.agents_commits(),
        )
    };
    let minute = |bed: &Bed, from: usize| {
        bed.plain()
            .get(from..)
            .unwrap_or_default()
            .contains("hm: \"10:01\"")
    };
    // Settled: the shell's last frames of the lock's start (on the CPU a
    // buffer comes a while after it is drawn) have come, nothing for a
    // second.
    let mut settled = false;
    for _ in 0..10 {
        let commits = counts(&bed.hp);
        wait(&mut bed.hp, Duration::from_secs(1))?;
        if counts(&bed.hp) == commits {
            settled = true;
            break;
        }
    }
    anyhow::ensure!(settled, "the lock never came to rest");
    let from = bed.mark();
    let before = counts(&bed.hp);
    wait(&mut bed.hp, Duration::from_secs(2))?;
    // Unless the minute came meanwhile (a slow machine), nothing drew.
    if !minute(&bed, from) {
        assert_eq!(
            counts(&bed.hp),
            before,
            "something drew at rest while locked"
        );
    }
    // The minute: the lock's clock draws, once.
    let came = {
        let log = bed.log.clone();
        bed.hp.dispatch_until(Duration::from_secs(70), |_| {
            String::from_utf8_lossy(&std::fs::read(&log).unwrap_or_default())
                .get(from..)
                .is_some_and(|l| l.contains("hm: \"10:01\""))
        })?
    };
    assert!(came, "the minute never drew");
    // The shell logs a draw before its buffer comes: waited for.
    let drew = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.lock_commits() > before.1)?;
    assert!(drew, "the lock drew at the minute");
    common::at_rest(&mut bed.hp)?;
    let at_minute = counts(&bed.hp);
    wait(&mut bed.hp, Duration::from_secs(2))?;
    let after = counts(&bed.hp);
    assert_eq!(
        after.1, at_minute.1,
        "the lock drew again within the minute"
    );
    assert_eq!(after.2, before.2, "the bar's clock drew while locked");
    assert_eq!(after.0, at_minute.0, "a frame within the minute");
    unlock(&mut bed.hp)
}

/// `hocus msg lock` on the control socket locks, and says so once it has.
#[test]
fn hocus_msg_lock_locks() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir()?;
    let mut hp = alone(&["1280x720"], dir.path())?;
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            let mut stream = std::os::unix::net::UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(b"msg lock\n")?;
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
    assert_eq!(answer.context("no answer")??, "locked\n");
    assert_eq!(hp.lock_state(), LockState::Locked);
    unlock(&mut hp)
}

/// `Tab` goes from the field to the button, then to the layout's button
/// (with two layouts): `Enter` there changes the layout, not the field.
#[test]
fn tab_goes_round_and_the_layout_switches_while_locked() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned(), "fr".to_owned()];
    config.plain_background = true;
    config.lock_auth = Authenticator::test(RIGHT);
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1280x720".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.lock();
    let focus = |hp: &Headless| hp.lock_shown().map(|s| s.focus);
    assert_eq!(focus(&hp), Some(LockFocus::Field));
    chord(&mut hp, &[KEY_TAB]);
    assert_eq!(focus(&hp), Some(LockFocus::Button));
    chord(&mut hp, &[KEY_TAB]);
    assert_eq!(focus(&hp), Some(LockFocus::Layout));
    let layout = hp.layout();
    chord(&mut hp, &[KEY_ENTER]);
    assert_ne!(
        hp.layout(),
        layout,
        "Enter on the layout's button changes it"
    );
    assert_eq!(hp.lock_typed(), Some(0));
    chord(&mut hp, &[KEY_TAB]);
    assert_eq!(focus(&hp), Some(LockFocus::Field));
    chord(&mut hp, &[KEY_LEFTSHIFT, KEY_TAB]);
    assert_eq!(focus(&hp), Some(LockFocus::Layout));
    // A letter goes to the field, whatever has the keyboard.
    chord(&mut hp, &[letter('p')]);
    assert_eq!(focus(&hp), Some(LockFocus::Field));
    assert_eq!(hp.lock_typed(), Some(1));
    assert!(hp.state().locked());
    Ok(())
}
