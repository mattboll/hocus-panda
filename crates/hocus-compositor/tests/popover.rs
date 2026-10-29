//! The sound's popover beside the bar (docs/design/bar.md §9; issue #361):
//! the real shell, against the private fakes of its services
//! (`levels/fakes.rs`), with three outputs. A click on the sound's row opens
//! it and it stays; a click outside, a second click or `Esc` closes it; in
//! the mini form, hovering the gauge opens it and leaving closes it; the
//! wheel, the slider, the mute button and the outputs change the fake
//! PipeWire; and nothing draws at rest, a change in it drawing it alone.
//! 3.4b's (issue #380): the microphones, their mute, volume and choice;
//! the applications playing, their volume and mute, a stream starting or
//! ending while it is open.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.
//! The timings of hovering, on the controller's own clock, are the shell's
//! unit tests (`popover::tests`); here they are checked within their
//! bounds, in real time.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::time::{Duration, Instant};

use common::{BTN_LEFT, KEY_ESC, KEY_LEFTMETA, KEY_TAB, WAIT, chord, shell_program, wait};
use fakes::{FakeSink, Fakes};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_B: u32 = 48;
const KEY_ENTER: u32 = 28;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;

const SPEAKERS: &str = "hocus-test-speakers";
const HEADSET: &str = "hocus-test-headset";
const SCREEN: &str = "hocus-test-hdmi";

/// The outputs: the laptop's speakers (the default), a headset, a screen.
const OUTPUTS: [FakeSink; 3] = [
    FakeSink::new(SPEAKERS, "Haut-parleurs du portable"),
    FakeSink {
        name: HEADSET,
        description: "Casque Bluetooth",
        form_factor: Some("headset"),
    },
    FakeSink::new(SCREEN, "Écran externe (HDMI)"),
];

/// On a 1920×1080 screen, the full bar 196 px wide: the levels' surface,
/// and the sound's row's middle (three rows and a one-line status line).
const LEVELS: Rect = Rect::new(1920 - 8 - 1 - 194, 1080 - 8 - 41 - 175, 194, 175);
const ROW: (f64, f64) = (1800.0, (LEVELS.y + 57) as f64);
/// The popover with three outputs, no microphone and no application
/// playing: 280 × 332, 8 px left of the bar, its bottom on the bar's.
const POPOVER: Rect = popover(332);
/// Its mute button, its slider's ends (the knob's centre at 0 and 100 %),
/// and its outputs' rows, in it.
const MUTE: (i32, i32) = (31, 69);
const SLIDER: (i32, i32, i32) = (63, 213, 69);
const OUTPUT_Y: [i32; 3] = [152, 190, 228];
/// The mini bar's gauge, and its popover.
const MINI_GAUGE: (f64, f64) = (1920.0 - 8.0 - 1.0 - 27.0, (1080 - 8 - 41 - 97 + 40) as f64);
const MINI_POPOVER: Rect = Rect::new(1920 - 8 - 56 - 8 - 280, 1080 - 8 - 332, 280, 332);

/// The full bar's popover, `height` tall.
const fn popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 196 - 8 - 280, 1080 - 8 - height, 280, height)
}

/// The fakes there, or why the test cannot run.
fn ready() -> Option<std::path::PathBuf> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !fakes::available() {
        eprintln!("dbus-daemon, pipewire, pw-dump, wpctl or pw-metadata missing: skipped");
        return None;
    }
    Some(shell)
}

fn start(fakes: &Fakes, shell: &std::path::Path) -> anyhow::Result<Headless> {
    start_on(fakes, shell, 1080)
}

/// On a screen 1920 px wide and `height` tall.
fn start_on(fakes: &Fakes, shell: &std::path::Path, height: i32) -> anyhow::Result<Headless> {
    let levels = Rect::new(
        LEVELS.x,
        LEVELS.y + height - 1080,
        LEVELS.width,
        LEVELS.height,
    );
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("1920x{height}").parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(fakes.shell(shell, "fr_FR.UTF-8")?),
    )?;
    let placed = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.levels_surfaces() == vec![("HEADLESS-1".to_owned(), levels)] && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(placed, "no levels: {:?}", hp.levels_surfaces());
    // The sound's row, there and blue.
    until(&mut hp, (levels.x + 40, levels.y + 57), blue)?;
    Ok(hp)
}

fn popovers(hp: &Headless) -> Vec<Rect> {
    hp.popover_surfaces().into_iter().map(|(_, r)| r).collect()
}

/// Waits until the popover is at `rect`, or gone (`None`).
fn popover_is(hp: &mut Headless, rect: Option<Rect>) -> anyhow::Result<()> {
    let want: Vec<Rect> = rect.into_iter().collect();
    let done = hp.dispatch_until(WAIT, |hp| popovers(hp) == want)?;
    anyhow::ensure!(done, "popovers {:?}, not {want:?}", popovers(hp));
    Ok(())
}

fn click(hp: &mut Headless, (x, y): (f64, f64)) {
    // Twice: the first move may only enter the surface.
    hp.pointer_move(x - 1.0, y);
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
}

/// A point of the popover at `rect`.
fn at(rect: Rect, (x, y): (i32, i32)) -> (f64, f64) {
    (f64::from(rect.x + x), f64::from(rect.y + y))
}

fn press(hp: &mut Headless, key: u32) {
    hp.key(key, true);
    hp.key(key, false);
}

fn rgb(hp: &mut Headless, (x, y): (i32, i32)) -> anyhow::Result<(u8, u8, u8)> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = usize::try_from(y * width + x)? * 4;
    let [b, g, r, _] = bytes
        .get(at..at + 4)
        .and_then(|p| <[u8; 4]>::try_from(p).ok())
        .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
    Ok((r, g, b))
}

fn blue((r, g, b): (u8, u8, u8)) -> bool {
    b > r.saturating_add(60) && b > g
}

fn dark((r, g, b): (u8, u8, u8)) -> bool {
    r < 70 && g < 70 && b < 90
}

fn until(hp: &mut Headless, at: (i32, i32), like: fn((u8, u8, u8)) -> bool) -> anyhow::Result<()> {
    let mut seen = (0, 0, 0);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        seen = rgb(hp, at)?;
        if like(seen) {
            return Ok(());
        }
        wait(hp, Duration::from_millis(50))?;
    }
    anyhow::bail!("{at:?} shows {seen:?}")
}

/// Waits until the fake's sink `name` is at `want` (volume, muted).
fn volume(hp: &mut Headless, fakes: &Fakes, name: &str, want: (f32, bool)) -> anyhow::Result<()> {
    let mut seen = None;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let now = fakes.volume(name)?;
        if (now.0 - want.0).abs() < 0.005 && now.1 == want.1 {
            return Ok(());
        }
        seen = Some(now);
        wait(hp, Duration::from_millis(50))?;
    }
    anyhow::bail!("{name}: volume {seen:?}, not {want:?}")
}

#[test]
fn a_click_opens_it_and_it_stays_until_a_click_outside_a_second_click_or_esc() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS)?;
    let mut hp = start(&fakes, &shell)?;
    // Two windows side by side, the second focused.
    common::open(&mut hp, "first", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    common::open(&mut hp, "second", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait(&mut hp, Duration::from_millis(500))?;
    let focused = |hp: &Headless| {
        hp.windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    };
    assert_eq!(focused(&hp).as_deref(), Some("second"));
    let first = common::rect_of(&hp, "first").expect("first");

    // A click on the row: it opens beside the bar, and takes the keyboard.
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(POPOVER))?;
    let held = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    assert!(held, "the keyboard is on {:?}", hp.keyboard_layer());
    // The pointer goes away: it stays.
    hp.pointer_move(600.0, 500.0);
    hp.pointer_move(601.0, 500.0);
    wait(&mut hp, Duration::from_millis(700))?;
    assert_eq!(popovers(&hp), vec![POPOVER]);

    // A click on a window closes it, and goes no further: a menu's.
    click(&mut hp, (f64::from(first.x + 50), f64::from(first.y + 50)));
    popover_is(&mut hp, None)?;
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(focused(&hp).as_deref(), Some("second"));
    // The windows have the keyboard back.
    let back = hp.dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(back, "the keyboard is on {:?}", hp.keyboard_layer());

    // A click whose release comes once the popover holds the keyboard, as
    // a person's does: the keys still reach it.
    hp.pointer_move(ROW.0 - 1.0, ROW.1);
    hp.pointer_move(ROW.0, ROW.1);
    hp.button(BTN_LEFT, true);
    let held = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    assert!(held, "the keyboard is on {:?}", hp.keyboard_layer());
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(100))?;
    press(&mut hp, KEY_ESC);
    popover_is(&mut hp, None)?;
    let row = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-levels")
    })?;
    assert!(row, "the keyboard is on {:?}", hp.keyboard_layer());
    press(&mut hp, KEY_ESC);
    let gone = hp.dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "the keyboard is on {:?}", hp.keyboard_layer());

    // A second click on the row closes it.
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(POPOVER))?;
    click(&mut hp, ROW);
    popover_is(&mut hp, None)?;

    // Esc closes it, and the row holds the keyboard; Enter opens it again;
    // Esc twice lets the keyboard go. (The click, and a move away that the
    // shell reads with it, open it all the same.)
    click(&mut hp, ROW);
    hp.pointer_move(600.0, 500.0);
    popover_is(&mut hp, Some(POPOVER))?;
    hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    press(&mut hp, KEY_ESC);
    popover_is(&mut hp, None)?;
    let row = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-levels")
    })?;
    assert!(row, "the keyboard is on {:?}", hp.keyboard_layer());
    press(&mut hp, KEY_ENTER);
    popover_is(&mut hp, Some(POPOVER))?;
    hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    press(&mut hp, KEY_ESC);
    popover_is(&mut hp, None)?;
    hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-levels")
    })?;
    press(&mut hp, KEY_ESC);
    let gone = hp.dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "the keyboard is on {:?}", hp.keyboard_layer());
    Ok(())
}

#[test]
fn in_the_mini_form_hovering_the_gauge_opens_it_and_leaving_closes_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS)?;
    let mut hp = start(&fakes, &shell)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    let mini = hp.dispatch_until(WAIT, |hp| {
        hp.levels_surfaces()
            .first()
            .is_some_and(|(_, r)| r.width == 54)
    })?;
    assert!(mini, "{:?}", hp.levels_surfaces());
    wait(&mut hp, Duration::from_millis(500))?;

    // Passing over the gauge opens nothing.
    hp.pointer_move(MINI_GAUGE.0 - 1.0, MINI_GAUGE.1);
    hp.pointer_move(MINI_GAUGE.0, MINI_GAUGE.1);
    hp.dispatch(Duration::from_millis(1))?;
    hp.pointer_move(1000.0, 500.0);
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(popovers(&hp), vec![]);
    // Resting on it: open after 150 ms, not before 100.
    hp.pointer_move(MINI_GAUGE.0 - 1.0, MINI_GAUGE.1);
    hp.pointer_move(MINI_GAUGE.0, MINI_GAUGE.1);
    let rested = Instant::now();
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(popovers(&hp), vec![], "open before 150 ms");
    popover_is(&mut hp, Some(MINI_POPOVER))?;
    eprintln!("opened {:?} after resting", rested.elapsed());
    // It does not take the keyboard.
    assert_eq!(hp.keyboard_layer(), None);
    // The pointer goes into it: it stays.
    let inside = at(MINI_POPOVER, (140, 20));
    hp.pointer_move(inside.0 - 1.0, inside.1);
    hp.pointer_move(inside.0, inside.1);
    wait(&mut hp, Duration::from_millis(600))?;
    assert_eq!(popovers(&hp), vec![MINI_POPOVER]);
    // It leaves for the windows: closed after 300 ms, not before 150.
    hp.pointer_move(800.0, 300.0);
    let left = Instant::now();
    wait(&mut hp, Duration::from_millis(150))?;
    assert_eq!(popovers(&hp), vec![MINI_POPOVER], "closed before 300 ms");
    popover_is(&mut hp, None)?;
    eprintln!("closed {:?} after leaving", left.elapsed());

    // A click on the gauge opens it as in the full form: it stays.
    click(&mut hp, MINI_GAUGE);
    popover_is(&mut hp, Some(MINI_POPOVER))?;
    hp.pointer_move(800.0, 300.0);
    wait(&mut hp, Duration::from_millis(700))?;
    assert_eq!(popovers(&hp), vec![MINI_POPOVER]);
    click(&mut hp, (800.0, 300.0));
    popover_is(&mut hp, None)?;

    // Opened by hovering, then a click on the gauge: it stays, and takes
    // the keyboard, which Esc gives to the gauge.
    hp.pointer_move(MINI_GAUGE.0 - 1.0, MINI_GAUGE.1);
    hp.pointer_move(MINI_GAUGE.0, MINI_GAUGE.1);
    popover_is(&mut hp, Some(MINI_POPOVER))?;
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let held = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    assert!(held, "the keyboard is on {:?}", hp.keyboard_layer());
    hp.pointer_move(800.0, 300.0);
    wait(&mut hp, Duration::from_millis(700))?;
    assert_eq!(popovers(&hp), vec![MINI_POPOVER]);
    press(&mut hp, KEY_ESC);
    popover_is(&mut hp, None)?;
    let row = hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-levels")
    })?;
    assert!(row, "the keyboard is on {:?}", hp.keyboard_layer());
    Ok(())
}

#[test]
fn the_wheel_the_slider_the_mute_button_and_the_outputs_change_the_sound() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS)?;
    let mut hp = start(&fakes, &shell)?;
    let headset = fakes.sink(HEADSET)?.to_string();
    fakes.wpctl(&["set-volume", &headset, "0.30"])?;

    // The wheel over the row: 5 % a notch, without opening anything.
    hp.pointer_move(ROW.0 - 1.0, ROW.1);
    hp.pointer_move(ROW.0, ROW.1);
    hp.wheel(-1);
    volume(&mut hp, &fakes, SPEAKERS, (0.67, false))?;
    hp.wheel(1);
    hp.wheel(1);
    volume(&mut hp, &fakes, SPEAKERS, (0.57, false))?;
    // A touchpad: 15 px a notch.
    hp.finger_scroll(-10.0);
    hp.finger_scroll(-10.0);
    volume(&mut hp, &fakes, SPEAKERS, (0.62, false))?;
    // Up to 100 % and no further; a muted sound comes back.
    let speakers = fakes.sink(SPEAKERS)?.to_string();
    fakes.wpctl(&["set-volume", &speakers, "0.98"])?;
    fakes.wpctl(&["set-mute", &speakers, "1"])?;
    wait(&mut hp, Duration::from_millis(300))?;
    hp.wheel(-1);
    volume(&mut hp, &fakes, SPEAKERS, (1.0, false))?;
    hp.wheel(-1);
    wait(&mut hp, Duration::from_millis(400))?;
    volume(&mut hp, &fakes, SPEAKERS, (1.0, false))?;
    assert_eq!(popovers(&hp), vec![]);

    // The slider: a click at a third, then a drag to the end and back to
    // the middle; the bar's gauge follows.
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(POPOVER))?;
    let (from, to, y) = SLIDER;
    let x = |share: f64| f64::from(from) + f64::from(to - from) * share;
    let point = |share: f64| at(POPOVER, (0, y)).0 + x(share);
    let row_y = f64::from(POPOVER.y + y);
    hp.pointer_move(point(0.3) - 1.0, row_y);
    hp.pointer_move(point(0.3), row_y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    volume(&mut hp, &fakes, SPEAKERS, (0.30, false))?;
    let gauge_past_a_third = (LEVELS.x + 37 + 55, LEVELS.y + 57);
    until(&mut hp, gauge_past_a_third, dark)?;
    hp.button(BTN_LEFT, true);
    for step in 0..=20 {
        hp.pointer_move(point(0.3 + 0.035 * f64::from(step)), row_y);
        hp.dispatch(Duration::from_millis(5))?;
    }
    for step in 0..=10 {
        hp.pointer_move(point(1.0 - 0.05 * f64::from(step)), row_y);
        hp.dispatch(Duration::from_millis(5))?;
    }
    hp.button(BTN_LEFT, false);
    volume(&mut hp, &fakes, SPEAKERS, (0.50, false))?;
    until(&mut hp, (LEVELS.x + 37 + 45, LEVELS.y + 57), blue)?;
    // The keys: Tab to the slider, the arrows 5 % a press.
    press(&mut hp, KEY_TAB);
    press(&mut hp, KEY_TAB);
    press(&mut hp, KEY_RIGHT);
    volume(&mut hp, &fakes, SPEAKERS, (0.55, false))?;
    press(&mut hp, KEY_LEFT);
    press(&mut hp, KEY_LEFT);
    volume(&mut hp, &fakes, SPEAKERS, (0.45, false))?;

    // The mute button.
    click(&mut hp, at(POPOVER, MUTE));
    volume(&mut hp, &fakes, SPEAKERS, (0.45, true))?;
    click(&mut hp, at(POPOVER, MUTE));
    volume(&mut hp, &fakes, SPEAKERS, (0.45, false))?;

    // Another output: the headset is the default, at its own 30 %, which
    // the slider then shows; the speakers keep theirs.
    // Between where the knob's edge is at 30 % and at 45 %: filled now,
    // the track at 30 %.
    let slider_at_40 = (POPOVER.x + 119, POPOVER.y + y);
    until(&mut hp, slider_at_40, blue)?;
    click(&mut hp, at(POPOVER, (140, OUTPUT_Y[1])));
    let chosen = hp.dispatch_until(WAIT, |_| {
        fakes.default_sink_name().ok().as_deref() == Some(HEADSET)
    })?;
    assert!(chosen, "default: {:?}", fakes.default_sink_name());
    until(&mut hp, slider_at_40, dark)?;
    volume(&mut hp, &fakes, HEADSET, (0.30, false))?;
    volume(&mut hp, &fakes, SPEAKERS, (0.45, false))?;
    // The bar's gauge is the headset's now.
    until(&mut hp, (LEVELS.x + 37 + 55, LEVELS.y + 57), dark)?;
    Ok(())
}

#[test]
fn nothing_draws_at_rest_and_a_change_draws_the_popover_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS)?;
    let mut hp = start(&fakes, &shell)?;
    let rest = |hp: &mut Headless| -> anyhow::Result<(u64, u64, u64)> {
        // Once every service has answered.
        let start = Instant::now();
        loop {
            let commits = (hp.levels_commits(), hp.popover_commits());
            wait(hp, Duration::from_millis(700))?;
            if (hp.levels_commits(), hp.popover_commits()) == commits || start.elapsed() > WAIT {
                break;
            }
        }
        let counts = (hp.bar_commits(), hp.levels_commits(), hp.popover_commits());
        wait(hp, Duration::from_secs(2))?;
        Ok((
            hp.bar_commits() - counts.0,
            hp.levels_commits() - counts.1,
            hp.popover_commits() - counts.2,
        ))
    };
    // Closed: nothing draws but the clock.
    hp.pointer_move(600.0, 500.0);
    assert_eq!(rest(&mut hp)?, (0, 0, 0));
    // Open, the pointer in it and still: nothing draws.
    let bars = hp.bar_commits();
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(POPOVER))?;
    let inside = at(POPOVER, (140, 20));
    hp.pointer_move(inside.0 - 1.0, inside.1);
    hp.pointer_move(inside.0, inside.1);
    assert_eq!(rest(&mut hp)?, (0, 0, 0));
    // Opening it drew neither the bar nor anything else of the shell's but
    // the row's surface and the popover.
    assert_eq!(hp.bar_commits(), bars, "the bar was drawn again");

    // An output's row under the pointer: the popover alone draws, and only
    // its rectangle changes on the screen.
    for attempt in 0..5 {
        // The clock's count first: a tick committed before it is drawn
        // during the wait, and its damage taken away with the rest.
        let clocks = hp.clock_commits();
        wait(&mut hp, Duration::from_millis(100))?;
        hp.take_damage(0);
        let (popover, levels) = (hp.popover_commits(), hp.levels_commits());
        let row = at(POPOVER, (140, OUTPUT_Y[attempt % 3]));
        hp.pointer_move(row.0, row.1);
        let drawn = hp.dispatch_until(WAIT, |hp| hp.popover_commits() > popover)?;
        anyhow::ensure!(drawn, "the popover did not draw the hover");
        wait(&mut hp, Duration::from_millis(100))?;
        assert_eq!(hp.levels_commits(), levels, "the levels were drawn again");
        assert_eq!(hp.bar_commits(), bars, "the bar was drawn again");
        if hp.clock_commits() == clocks {
            assert_eq!(hp.take_damage(0), Some(POPOVER));
            // A volume set elsewhere: the popover and the levels draw it,
            // nothing else.
            hp.take_damage(0);
            let speakers = fakes.sink(SPEAKERS)?.to_string();
            fakes.wpctl(&["set-volume", &speakers, "0.40"])?;
            let both = hp.dispatch_until(WAIT, |hp| {
                hp.popover_commits() > popover + 1 && hp.levels_commits() > levels
            })?;
            assert!(both, "the change was not drawn");
            assert_eq!(hp.bar_commits(), bars, "the bar was drawn again");
            return Ok(());
        }
    }
    anyhow::bail!("the clock ticked through every try")
}

#[test]
fn with_one_output_it_is_shorter_and_on_a_taller_screen_still_at_the_bottom() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with(&OUTPUTS[..1])?;
    // The bar's height is the screen's less 16 px: its row, 120 px lower.
    let mut hp = start_on(&fakes, &shell, 1200)?;
    click(&mut hp, (ROW.0, ROW.1 + 120.0));
    let rect = Rect::new(POPOVER.x, 1200 - 8 - 256, 280, 256);
    popover_is(&mut hp, Some(rect))?;
    Ok(())
}

// --- 3.4b: the microphone and the applications ---------------------------------

const MIC: &str = "hocus-test-mic";
const HEADSET_MIC: &str = "hocus-test-headset-mic";

/// The microphones: the laptop's (the default) and the headset's.
const MICROPHONES: [FakeSink; 2] = [
    FakeSink::new(MIC, "Micro du portable"),
    FakeSink::new(HEADSET_MIC, "Micro du casque"),
];

/// In a popover with three outputs and microphones: their row's middle,
/// and the list's rows'.
const MIC_ROW: i32 = 310;
const MIC_Y: [i32; 2] = [352, 390];
/// In a popover with three outputs and no microphone: the applications'
/// rows' tops; in one, the tile's middle and the slider's.
const APP_TOP: [i32; 3] = [292, 344, 396];
const TILE_AT: (i32, i32) = (31, 23);
const APP_SLIDER_Y: i32 = 33;

/// The default source's node name, as PipeWire's metadata says.
fn default_source(fakes: &Fakes) -> Option<String> {
    let out = std::process::Command::new("pw-metadata")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PIPEWIRE_REMOTE", fakes.pw_socket())
        .args(["-n", "default", "0", "default.audio.source"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    [MIC, HEADSET_MIC]
        .into_iter()
        .find(|name| text.contains(&format!("\"{name}\"")))
        .map(str::to_owned)
}

#[test]
fn the_microphone_its_mute_its_volume_and_which_one() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with_sources(&OUTPUTS, &MICROPHONES)?;
    fakes.wpctl(&["set-volume", &fakes.sink(MIC)?.to_string(), "0.70"])?;
    let mut hp = start(&fakes, &shell)?;
    // Its section: its row, and the two to choose from.
    let rect = popover(495);
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(rect))?;
    // Muted, then unmuted by its slider, as the master's.
    click(&mut hp, at(rect, (31, MIC_ROW)));
    volume(&mut hp, &fakes, MIC, (0.70, true))?;
    // Once the shell has read it too.
    wait(&mut hp, Duration::from_millis(300))?;
    let (left, right, _) = SLIDER;
    click(&mut hp, at(rect, ((left + right) / 2, MIC_ROW)));
    volume(&mut hp, &fakes, MIC, (0.50, false))?;
    // The keyboard: Tab to its slider (the mute button, the master's
    // slider, the three outputs, its mute button, it), Right: 5 % up.
    for _ in 0..7 {
        press(&mut hp, KEY_TAB);
    }
    press(&mut hp, KEY_RIGHT);
    volume(&mut hp, &fakes, MIC, (0.55, false))?;
    // The headset's: the default now, its own volume kept.
    click(&mut hp, at(rect, (140, MIC_Y[1])));
    let chosen = hp.dispatch_until(WAIT, |_| {
        default_source(&fakes).as_deref() == Some(HEADSET_MIC)
    })?;
    assert!(chosen, "default source: {:?}", default_source(&fakes));
    volume(&mut hp, &fakes, MIC, (0.55, false))?;
    // The master's sound untouched.
    volume(&mut hp, &fakes, SPEAKERS, (0.62, false))?;
    Ok(())
}

#[test]
fn one_microphone_has_no_list_and_none_hides_its_section() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start_with_sources(&OUTPUTS, &MICROPHONES[..1])?;
    let mut hp = start(&fakes, &shell)?;
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(popover(415)))?;
    Ok(())
}

#[test]
fn the_applications_playing_their_volume_their_mute_and_streams_coming_and_going()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut fakes = Fakes::start_with(&OUTPUTS)?;
    // Firefox with two tabs playing, music, and a player paused.
    fakes.play("hocus-test-tab-1", "Firefox", None)?;
    fakes.play("hocus-test-tab-2", "Firefox", None)?;
    fakes.play("hocus-test-music", "Musique", None)?;
    fakes.play("hocus-test-paused", "Lecteur", None)?;
    fakes.pause("hocus-test-paused", true)?;
    let mut hp = start(&fakes, &shell)?;
    // Two rows: Firefox's (its two tabs), the music's; not the paused one.
    let two = popover(405);
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(two))?;

    // At rest with streams playing, nothing draws.
    let inside = at(two, (140, 20));
    hp.pointer_move(inside.0 - 1.0, inside.1);
    hp.pointer_move(inside.0, inside.1);
    wait(&mut hp, Duration::from_millis(700))?;
    let counts = (hp.bar_commits(), hp.levels_commits(), hp.popover_commits());
    wait(&mut hp, Duration::from_secs(2))?;
    assert_eq!(
        (hp.bar_commits(), hp.levels_commits(), hp.popover_commits()),
        counts,
        "drawn at rest"
    );

    // Firefox's tile mutes both its streams; again, unmutes them.
    let tile = |row: usize| at(two, (TILE_AT.0, APP_TOP[row] + TILE_AT.1));
    click(&mut hp, tile(0));
    for tab in ["hocus-test-tab-1", "hocus-test-tab-2"] {
        volume(&mut hp, &fakes, tab, (1.0, true))?;
    }
    // Once the shell has read it too.
    wait(&mut hp, Duration::from_millis(300))?;
    click(&mut hp, tile(0));
    for tab in ["hocus-test-tab-1", "hocus-test-tab-2"] {
        volume(&mut hp, &fakes, tab, (1.0, false))?;
    }
    // The music's slider, dragged to the middle: its stream alone.
    let (left, right, _) = SLIDER;
    let y = f64::from(two.y + APP_TOP[1] + APP_SLIDER_Y);
    let x = |px: i32| f64::from(two.x + px);
    hp.pointer_move(x(right) - 1.0, y);
    hp.pointer_move(x(right), y);
    hp.button(BTN_LEFT, true);
    for step in 1..=10 {
        let share = f64::from(step) / 10.0;
        hp.pointer_move(x(right) - (x(right) - x((left + right) / 2)) * share, y);
        wait(&mut hp, Duration::from_millis(16))?;
    }
    hp.button(BTN_LEFT, false);
    volume(&mut hp, &fakes, "hocus-test-music", (0.50, false))?;
    volume(&mut hp, &fakes, "hocus-test-tab-1", (1.0, false))?;
    volume(&mut hp, &fakes, SPEAKERS, (0.62, false))?;

    // The paused player plays again: its row comes, the popover grows.
    fakes.pause("hocus-test-paused", false)?;
    popover_is(&mut hp, Some(popover(457)))?;
    // A tab paused: Firefox keeps its row while the popover is open.
    fakes.pause("hocus-test-tab-1", true)?;
    fakes.pause("hocus-test-tab-2", true)?;
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(popovers(&hp), vec![popover(457)]);
    // Its streams gone, its row goes.
    fakes.stop("hocus-test-tab-1");
    fakes.stop("hocus-test-tab-2");
    popover_is(&mut hp, Some(two))?;
    // Opened again: the playing ones only.
    fakes.pause("hocus-test-music", true)?;
    click(&mut hp, ROW);
    popover_is(&mut hp, None)?;
    click(&mut hp, ROW);
    popover_is(&mut hp, Some(popover(353)))?;
    Ok(())
}
