//! A window asked to come forward (xdg-activation), headless
//! (docs/design/windows.md §4, issue #966): with a token made from the
//! user's click in another application (a link opened in the browser),
//! it comes forward even on another desktop or another screen, its
//! desktop switched to; without one, its rim pulses and the focus stays.
//! While what the click opened has not shown, the pointer is busy: until
//! a window appears or comes forward, 5 s at most.

use crate::common;

use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, ClientHandle, Command, Event, KEY_1, KEY_2, KEY_LEFTMETA, Shows, WAIT, chord,
    open_showing, open_with,
};
use hocus_compositor::{
    Config, DesktopSwitching, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};

const CHAT: &str = "test.chat";
const BROWSER: &str = "test.browser";
const SECOND: &str = "HEADLESS-2";

/// A window's desktop (from 0) and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(usize, bool)> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| (w.2, w.3))
}

fn has_keyboard(hp: &Headless, app_id: &str) -> bool {
    hp.windows()
        .iter()
        .any(|w| w.app_id.as_deref() == Some(app_id) && w.focused && w.keyboard)
}

/// The chat's window, where a click on a link asks for a token.
fn chat(hp: &mut Headless) -> anyhow::Result<ClientHandle> {
    let (_, handle) = open_showing(
        hp,
        CHAT,
        Shows::Linking {
            app_id: CHAT,
            size: (600, 400),
        },
    )?;
    Ok(handle)
}

/// A click on the chat's link, where no other window covers it: the
/// token it got for it.
fn click_a_link(hp: &mut Headless, events: &Receiver<Event>) -> anyhow::Result<String> {
    let windows = hp.windows();
    let rect = windows
        .iter()
        .find(|w| w.app_id.as_deref() == Some(CHAT))
        .map(|w| w.rect)
        .ok_or_else(|| anyhow::anyhow!("no chat: {windows:?}"))?;
    let covered = |(x, y): (i32, i32)| {
        windows.iter().any(|w| {
            let r = w.rect;
            w.app_id.as_deref() != Some(CHAT)
                && x >= r.x
                && x < r.x + r.width
                && y >= r.y
                && y < r.y + r.height
        })
    };
    let at = [
        (rect.x + rect.width / 2, rect.y + rect.height / 2),
        (rect.x + 20, rect.y + 20),
        (rect.x + rect.width - 20, rect.y + rect.height - 20),
        (rect.x + 20, rect.y + rect.height - 20),
        (rect.x + rect.width - 20, rect.y + 20),
    ]
    .into_iter()
    .find(|p| !covered(*p))
    .ok_or_else(|| anyhow::anyhow!("the chat is covered: {windows:?}"))?;
    hp.pointer_move(f64::from(at.0), f64::from(at.1));
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        hp.dispatch_until(Duration::from_millis(20), |_| false)?;
        if let Some(token) = events.try_iter().find_map(|e| match e {
            Event::Token(token) => Some(token),
            _ => None,
        }) {
            return Ok(token);
        }
    }
    anyhow::bail!("no token for the click")
}

/// The browser, already open on the first screen's second desktop; the
/// chat on its first, which shows.
fn browser_on_another_desktop(hp: &mut Headless) -> anyhow::Result<(ClientHandle, ClientHandle)> {
    hp.pointer_move(960.0, 540.0);
    chord(hp, &[KEY_LEFTMETA, KEY_2]);
    let (_, browser) = open_with(hp, BROWSER, (800, 600), false)?;
    chord(hp, &[KEY_LEFTMETA, KEY_1]);
    let chat = chat(hp)?;
    anyhow::ensure!(
        where_is(hp, BROWSER) == Some((1, false)),
        "{:?}",
        hp.all_windows()
    );
    Ok((browser, chat))
}

/// Two screens switching their desktops alone (#420).
fn per_screen() -> anyhow::Result<Headless> {
    let mut config = Config::default();
    config.desktops.switching = DesktopSwitching::PerScreen;
    Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

#[test]
fn a_click_brings_the_browser_from_another_desktop() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig::default())?;
    let (browser, chat) = browser_on_another_desktop(&mut hp)?;
    // Another window has the focus: the click on the link gives the chat
    // the keyboard, and its token is the user's still.
    let _other = open_with(&mut hp, "test.other", (200, 150), false)?;
    anyhow::ensure!(!has_keyboard(&hp, CHAT), "{:?}", hp.windows());
    let token = click_a_link(&mut hp, &chat.events)?;
    let _ = browser.commands.send(Command::Activate(Some(token)));
    let came = hp.dispatch_until(WAIT, |hp| has_keyboard(hp, BROWSER))?;
    anyhow::ensure!(came, "{:?} {:?}", hp.all_windows(), hp.windows());
    assert_eq!(where_is(&hp, BROWSER), Some((1, true)));
    assert_eq!(hp.desktop().0, 1);
    Ok(())
}

#[test]
fn a_click_brings_the_browser_from_another_screen() -> anyhow::Result<()> {
    let mut hp = per_screen()?;
    // The browser on the second screen's second desktop, which shows its
    // first; the chat on the first screen.
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let (_, browser) = open_with(&mut hp, BROWSER, (800, 600), false)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    hp.pointer_move(960.0, 540.0);
    let chat = chat(&mut hp)?;
    anyhow::ensure!(
        where_is(&hp, BROWSER) == Some((1, false)),
        "{:?}",
        hp.all_windows()
    );
    let token = click_a_link(&mut hp, &chat.events)?;
    let _ = browser.commands.send(Command::Activate(Some(token)));
    let came = hp.dispatch_until(WAIT, |hp| has_keyboard(hp, BROWSER))?;
    anyhow::ensure!(came, "{:?} {:?}", hp.all_windows(), hp.windows());
    assert_eq!(where_is(&hp, BROWSER), Some((1, true)));
    let second = hp
        .strips()
        .into_iter()
        .find(|(name, ..)| name == SECOND)
        .map(|(_, _, current)| current);
    assert_eq!(second, Some(1), "{:?}", hp.strips());
    Ok(())
}

#[test]
fn without_a_click_the_window_pulses_and_the_focus_stays() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig::default())?;
    hp.pointer_move(960.0, 540.0);
    let (_, browser) = open_with(&mut hp, BROWSER, (800, 600), false)?;
    let _chat = chat(&mut hp)?;
    anyhow::ensure!(has_keyboard(&hp, CHAT), "{:?}", hp.windows());
    // A token of its own, made from no input.
    let _ = browser.commands.send(Command::Activate(None));
    let pulsed = hp.dispatch_until(WAIT, |hp| hp.pulse(BROWSER).is_some())?;
    anyhow::ensure!(pulsed, "no pulse: {:?}", hp.windows());
    assert!(has_keyboard(&hp, CHAT), "{:?}", hp.windows());
    Ok(())
}

#[test]
fn the_pointer_is_busy_until_the_browser_comes() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig::default())?;
    let (browser, chat) = browser_on_another_desktop(&mut hp)?;
    assert!(!hp.busy_cursor());
    let token = click_a_link(&mut hp, &chat.events)?;
    assert!(hp.busy_cursor(), "not busy after the click");
    // A moment later, still nothing: still busy.
    hp.dispatch_until(Duration::from_millis(500), |_| false)?;
    assert!(hp.busy_cursor(), "no longer busy before the browser came");
    let _ = browser.commands.send(Command::Activate(Some(token)));
    let came = hp.dispatch_until(WAIT, |hp| has_keyboard(hp, BROWSER))?;
    anyhow::ensure!(came, "{:?}", hp.windows());
    assert!(!hp.busy_cursor(), "still busy with the browser come");
    Ok(())
}

#[test]
fn the_pointer_is_busy_5_s_at_most() -> anyhow::Result<()> {
    let mut hp = Headless::start(HeadlessConfig::default())?;
    hp.pointer_move(960.0, 540.0);
    let chat = chat(&mut hp)?;
    let clicked = Instant::now();
    click_a_link(&mut hp, &chat.events)?;
    assert!(hp.busy_cursor(), "not busy after the click");
    let cleared = hp.dispatch_until(Duration::from_secs(8), |hp| !hp.busy_cursor())?;
    anyhow::ensure!(cleared, "still busy after 8 s");
    let after = clicked.elapsed();
    assert!(
        (Duration::from_millis(4500)..Duration::from_secs(7)).contains(&after),
        "cleared after {after:?}"
    );
    Ok(())
}
