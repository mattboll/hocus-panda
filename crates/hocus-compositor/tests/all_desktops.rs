//! Every desktop at once (docs/design/windows.md §12c, issue #944,
//! variant C), headless, the compositor alone: #947's two screens, a
//! laptop's panel (2560 × 1600 at 1.25, 2048 × 1280 logical) and a
//! 34-inch screen on its right (3440 × 1440), linked.
//!
//! `Super+Shift+W`, three fingers down and the top-right corner open it,
//! from the desktop or the overview; again, `Esc`, `Super+W` or three
//! fingers up close it; a binding the file gives keeps its gesture. Each
//! screen's desktops in a grid, at one scale; a click on a desktop or a
//! window goes there on every screen (per screen, that screen alone); a
//! window carried to another desktop goes there, of that screen, in its
//! zone or where let go; the arrows, `Enter`, `Tab` and `Super+Shift+N`;
//! the labels, a project's, the agents' badges, the empty desktops'
//! pandas; reduced motion; the lock and a screen unplugged close it; at
//! rest nothing draws. The Settings' row and gestures' line are tested
//! beside theirs (`settings_keyboard`, `settings_mouse`), the bar's tiles
//! with the real shell (`bar_desktops`). With `HOCUS_SHOTS` naming a
//! folder, the tests save the screens as `all-desktops-*.png`.

use crate::common;

use std::{io::Write, os::unix::net::UnixStream, path::Path, time::Duration};

use common::{
    BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT, KEY_TAB, KEY_UP,
    WAIT, chord, open, open_with, wait,
};
use hocus_compositor::{
    Config, DesktopSwitching, GridLabel, GridSelection, Headless, HeadlessConfig, LockState,
    RendererChoice, hot_corner,
};

const LAPTOP: &str = "HEADLESS-1";
const WIDE: &str = "HEADLESS-2";
const ON_LAPTOP: (f64, f64) = (1024.0, 640.0);
const ON_WIDE: (f64, f64) = (2048.0 + 1720.0, 720.0);
const KEY_W: u32 = 17;
const KEY_ENTER: u32 = 28;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;
const KEY_4: u32 = 5;
/// The 34-inch's top-right corner, and a point 40 px in from it.
const WIDE_CORNER: (f64, f64) = (2048.0 + 3439.0, 0.0);
const NEAR_WIDE_CORNER: (f64, f64) = (2048.0 + 3400.0, 30.0);

fn screens_with(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.25".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(ON_LAPTOP.0, ON_LAPTOP.1);
    Ok(hp)
}

/// The two screens, linked, `config` read from `toml`, with windows: the
/// laptop's terminal on 1, `test.two` on 2; the 34-inch's left half on 1,
/// a floating window on 2, the mail on 3; 4 empty on both.
fn start_with(toml: &str) -> anyhow::Result<Headless> {
    let config: Config = toml::from_str(toml)?;
    let mut hp = screens_with(config)?;
    open(&mut hp, "test.term", (900, 600))?;
    open(&mut hp, "test.two", (800, 500))?;
    send(&mut hp, KEY_2)?;
    hp.pointer_move(ON_WIDE.0, ON_WIDE.1);
    open(&mut hp, "test.half", (1200, 800))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    open(&mut hp, "test.float", (1000, 700))?;
    send(&mut hp, KEY_2)?;
    open(&mut hp, "test.mail", (1000, 700))?;
    send(&mut hp, KEY_3)?;
    hp.pointer_move(ON_LAPTOP.0, ON_LAPTOP.1);
    wait(&mut hp, Duration::from_millis(400))?;
    Ok(hp)
}

fn start() -> anyhow::Result<Headless> {
    start_with("")
}

/// `Super+Shift+N`: the focused window to desktop N.
fn send(hp: &mut Headless, digit: u32) -> anyhow::Result<()> {
    chord(hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, digit]);
    wait(hp, Duration::from_millis(100))
}

fn super_shift_w(hp: &mut Headless) {
    chord(hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_W]);
}

fn press(hp: &mut Headless, key: u32) {
    hp.key(key, true);
    hp.key(key, false);
}

/// Opens every desktop with the keys and lets it zoom out.
fn opened(hp: &mut Headless) -> anyhow::Result<()> {
    super_shift_w(hp);
    open_now(hp)?;
    wait(hp, Duration::from_millis(400))
}

fn open_now(hp: &mut Headless) -> anyhow::Result<()> {
    let open = hp.dispatch_until(WAIT, |hp| hp.all_desktops() == Some(true))?;
    anyhow::ensure!(open, "every desktop never showed");
    Ok(())
}

/// Waits until every desktop is gone.
fn closed(hp: &mut Headless) -> anyhow::Result<()> {
    let gone = hp.dispatch_until(WAIT, |hp| hp.all_desktops().is_none())?;
    anyhow::ensure!(gone, "every desktop stays");
    anyhow::ensure!(hp.overview().is_none(), "the overview shows");
    Ok(())
}

/// The desktop each screen shows (from 0), the laptop's first.
fn shown(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// Screen `output`'s desktop `desktop` (from 0): its picture.
fn desktop(hp: &Headless, output: &str, desktop: usize) -> anyhow::Result<(f64, f64, f64, f64)> {
    hp.grid_desktops()
        .into_iter()
        .find(|(o, d, _)| o == output && *d == desktop)
        .map(|(_, _, a)| a)
        .ok_or_else(|| anyhow::anyhow!("no desktop {} on {output}", desktop + 1))
}

fn middle((x, y, w, h): (f64, f64, f64, f64)) -> (f64, f64) {
    (x + w / 2.0, y + h / 2.0)
}

/// A point of desktop `d`'s background, below its windows' pictures: its
/// bottom-left, in a little.
fn background(hp: &Headless, output: &str, d: usize) -> anyhow::Result<(f64, f64)> {
    let (x, y, _, h) = desktop(hp, output, d)?;
    Ok((x + 6.0, y + h - 6.0))
}

/// `app_id`'s picture in the grid.
fn picture(hp: &Headless, app_id: &str) -> anyhow::Result<(f64, f64, f64, f64)> {
    hp.grid_windows()
        .into_iter()
        .find(|w| w.0.as_deref() == Some(app_id))
        .map(|w| w.3)
        .ok_or_else(|| anyhow::anyhow!("{app_id} not in the grid: {:?}", hp.grid_windows()))
}

fn click(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<()> {
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    wait(hp, Duration::from_millis(50))
}

/// Presses on `from`, moves to `to` in steps, holding the button.
fn carry(hp: &mut Headless, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
    hp.pointer_move(from.0, from.1);
    hp.button(BTN_LEFT, true);
    for step in 1..=16 {
        let t = f64::from(step) / 16.0;
        hp.pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
    }
    wait(hp, Duration::from_millis(150))
}

fn let_go(hp: &mut Headless) -> anyhow::Result<()> {
    hp.button(BTN_LEFT, false);
    wait(hp, Duration::from_millis(300))
}

/// A window's screen, its desktop (from 0), its place.
fn where_is(hp: &Headless, app_id: &str) -> Option<(String, usize, hocus_compositor::model::Rect)> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .and_then(|p| Some((p.output?, p.desktop, p.rect)))
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused && w.keyboard)
        .and_then(|w| w.app_id)
}

/// With `HOCUS_SHOTS` naming a folder: both screens side by side, at
/// rest, as `all-desktops-WHAT.png`.
fn shot(hp: &mut Headless, what: &str) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    if hp.overview_badges().iter().any(|b| b.1 == "waiting") {
        // A waiting agent's panda waves on: a frame of its own.
        hp.hold_clock();
        hp.step(Duration::from_millis(500))?;
        hp.release_clock();
    } else if hp.grid_carry().is_some() {
        wait(hp, Duration::from_millis(300))?;
    } else {
        common::at_rest(hp)?;
    }
    let screens = [hp.screenshot(0)?, hp.screenshot(1)?];
    let width: i32 = screens.iter().map(|s| s.0).sum();
    let height = screens.iter().map(|s| s.1).max().unwrap_or(0);
    let mut image =
        image::RgbaImage::from_pixel(width as u32, height as u32, image::Rgba([14, 15, 18, 255]));
    let mut x0 = 0;
    for (w, h, bytes) in &screens {
        for y in 0..*h {
            for x in 0..*w {
                let at = usize::try_from(y * w + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(
                    (x0 + x) as u32,
                    y as u32,
                    image::Rgba([p[2], p[1], p[0], 255]),
                );
            }
        }
        x0 += w;
    }
    image.save(Path::new(&folder).join(format!("all-desktops-{what}.png")))?;
    Ok(())
}

// --- Opening and closing ------------------------------------------------------

/// Scenario 1: Both screens show their four desktops in a 2 × 2 grid, 3
/// under 1, each window at its place, desktop 1 ringed on both.
#[test]
fn super_shift_w_shows_each_screen_s_desktops_in_a_grid() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    for output in [LAPTOP, WIDE] {
        let places: Vec<_> = (0..4)
            .map(|d| desktop(&hp, output, d))
            .collect::<anyhow::Result<_>>()?;
        // 2 right of 1, 3 under 1, 4 right of 3.
        assert!(
            places[1].0 > places[0].0 && places[1].1 == places[0].1,
            "{places:?}"
        );
        assert!(
            places[2].0 == places[0].0 && places[2].1 > places[0].1,
            "{places:?}"
        );
        assert!(
            places[3].0 == places[1].0 && places[3].1 == places[2].1,
            "{places:?}"
        );
    }
    // Each window inside its desktop's picture, where it is on its screen.
    let term = picture(&hp, "test.term")?;
    let one = desktop(&hp, LAPTOP, 0)?;
    let k = one.2 / 2048.0;
    let at = where_is(&hp, "test.term")
        .map(|w| w.2)
        .expect("the terminal");
    assert!(
        (term.0 - (one.0 + f64::from(at.x) * k)).abs() < 1e-6,
        "{term:?}"
    );
    assert!((term.2 - f64::from(at.width) * k).abs() < 1e-6, "{term:?}");
    let mail = picture(&hp, "test.mail")?;
    let three = desktop(&hp, WIDE, 2)?;
    assert!(
        mail.0 >= three.0 && mail.1 >= three.1 && mail.0 + mail.2 <= three.0 + three.2,
        "{mail:?} not in {three:?}"
    );
    // The desktop shown, 1, on both.
    assert_eq!(shown(&hp), [0, 0]);
    shot(&mut hp, "two-screens")?;
    Ok(())
}

/// Scenario 2: `Super+Shift+W`, `Esc` or `Super+W`: desktop 1 on both, as
/// before, the focus unchanged.
#[test]
fn the_keys_close_it_as_it_was() -> anyhow::Result<()> {
    let mut hp = start()?;
    let before = focused(&hp);
    assert!(before.is_some());
    for close in [
        &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_W][..],
        &[KEY_ESC][..],
        &[KEY_LEFTMETA, KEY_W][..],
    ] {
        opened(&mut hp)?;
        chord(&mut hp, close);
        closed(&mut hp)?;
        assert_eq!(shown(&hp), [0, 0], "after {close:?}");
        assert_eq!(focused(&hp), before, "after {close:?}");
    }
    Ok(())
}

/// Scenario 3: Three fingers down open it; no swipe reached a client.
#[test]
fn three_fingers_down_open_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    hp.swipe(3, &[(0.0, 30.0); 6], 10);
    open_now(&mut hp)?;
    assert_eq!(hp.swipes_given(), (0, 0), "a client had the swipe");
    Ok(())
}

/// Scenario 4: From the overview, three fingers down.
#[test]
fn three_fingers_down_from_the_overview() -> anyhow::Result<()> {
    let mut hp = start()?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    let open = hp.dispatch_until(WAIT, |hp| hp.overview() == Some(true))?;
    anyhow::ensure!(open, "no overview");
    wait(&mut hp, Duration::from_millis(300))?;
    hp.swipe(3, &[(0.0, 30.0); 6], 10);
    open_now(&mut hp)?;
    Ok(())
}

/// Scenario 5: Three fingers up close it whatever they are bound to;
/// closed, they are the binding's again.
#[test]
fn three_fingers_up_close_it_before_their_binding() -> anyhow::Result<()> {
    let mut hp = start_with("[shortcuts]\nlock = [\"Super+L\", \"swipe:3:up\"]\n")?;
    opened(&mut hp)?;
    hp.swipe(3, &[(0.0, -30.0); 6], 10);
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [0, 0]);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.lock_state(), LockState::Unlocked, "locked");
    hp.swipe(3, &[(0.0, -30.0); 6], 10);
    let locked = hp.dispatch_until(WAIT, |hp| hp.lock_state() != LockState::Unlocked)?;
    anyhow::ensure!(locked, "three fingers up never locked");
    Ok(())
}

/// Scenario 6: Closed, with nothing bound to three fingers up, the client
/// under the pointer gets the whole swipe.
#[test]
fn three_fingers_up_closed_are_the_client_s() -> anyhow::Result<()> {
    let mut hp = start()?;
    hp.swipe(3, &[(0.0, -30.0); 5], 10);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.all_desktops(), None);
    assert_eq!(hp.swipes_given(), (1, 5), "the client has it whole");
    Ok(())
}

/// Scenario 7: The 34-inch's top-right corner, held 150 ms, opens it;
/// rested there again without leaving, nothing; 8 px away and back, it
/// closes.
#[test]
fn the_top_right_corner_opens_it_and_closes_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    hp.pointer_move(NEAR_WIDE_CORNER.0, NEAR_WIDE_CORNER.1);
    let entered = std::time::Instant::now();
    hp.pointer_move(WIDE_CORNER.0, WIDE_CORNER.1);
    open_now(&mut hp)?;
    assert!(entered.elapsed() >= hot_corner::DWELL, "open at once");
    // Staying, or back from 5 px away: still open.
    wait(&mut hp, Duration::from_millis(400))?;
    hp.pointer_move(WIDE_CORNER.0 - 5.0, 5.0);
    hp.pointer_move(WIDE_CORNER.0, WIDE_CORNER.1);
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.all_desktops(), Some(true));
    // Away, and in again: it closes.
    hp.pointer_move(NEAR_WIDE_CORNER.0, NEAR_WIDE_CORNER.1);
    hp.pointer_move(WIDE_CORNER.0, WIDE_CORNER.1);
    wait(&mut hp, Duration::from_millis(200))?;
    closed(&mut hp)?;
    Ok(())
}

/// Scenario 8: Passing through the corner in under 150 ms opens nothing.
#[test]
fn passing_through_the_corner_opens_nothing() -> anyhow::Result<()> {
    let mut hp = start()?;
    hp.pointer_move(NEAR_WIDE_CORNER.0, NEAR_WIDE_CORNER.1);
    let entered = std::time::Instant::now();
    hp.pointer_move(WIDE_CORNER.0, WIDE_CORNER.1);
    wait(&mut hp, Duration::from_millis(40))?;
    let passed = entered.elapsed() < hot_corner::DWELL;
    hp.pointer_move(NEAR_WIDE_CORNER.0, NEAR_WIDE_CORNER.1);
    wait(&mut hp, Duration::from_millis(300))?;
    // Unless the machine held the test longer than the rest, which proves
    // nothing either way.
    if passed {
        assert_eq!(hp.all_desktops(), None, "open as the pointer passed");
    }
    Ok(())
}

/// The laptop alone, `toml` its configuration, a window on it.
fn laptop_alone(toml: &str) -> anyhow::Result<Headless> {
    let config: Config = toml::from_str(toml)?;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.25".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(ON_LAPTOP.0, ON_LAPTOP.1);
    open(&mut hp, "test.term", (900, 600))?;
    wait(&mut hp, Duration::from_millis(200))?;
    Ok(hp)
}

/// Scenario 9: On the bar's screen, the bar on its right: the pointer
/// pushed to the very corner, above the bar, opens it and does not bring
/// the bar in front; 8 px away and back, it closes. Pushed against the edge
/// level with the bar, the bar comes in front and nothing opens.
#[test]
fn the_top_right_corner_acts_above_the_bar() -> anyhow::Result<()> {
    let mut hp = laptop_alone("")?;
    hp.pointer_move(2000.0, 30.0);
    hp.pointer_move(2047.0, 0.0);
    open_now(&mut hp)?;
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.bar_pushed(), None, "the bar's edge took the corner");
    hp.pointer_move(2030.0, 20.0);
    hp.pointer_move(2047.0, 0.0);
    wait(&mut hp, Duration::from_millis(200))?;
    closed(&mut hp)?;
    // Level with the bar: the bar's.
    hp.pointer_move(2000.0, 640.0);
    hp.pointer_move(2047.0, 640.0);
    let front = hp.dispatch_until(WAIT, |hp| hp.bar_in_front().is_some())?;
    anyhow::ensure!(front, "the bar never came in front");
    assert_eq!(hp.bar_pushed().as_deref(), Some(LAPTOP));
    wait(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.all_desktops(), None);
    Ok(())
}

/// Scenario 9b: The bar on the left: its top-left corner stays the bar's;
/// the top-right one opens every desktop.
#[test]
fn the_top_left_corner_stays_the_bar_s_on_the_left() -> anyhow::Result<()> {
    let mut hp = laptop_alone("[panel]\nside = \"left\"\n")?;
    hp.pointer_move(40.0, 30.0);
    hp.pointer_move(0.0, 0.0);
    wait(&mut hp, Duration::from_millis(400))?;
    assert_eq!(hp.overview(), None, "the bar's corner opened the overview");
    hp.pointer_move(2000.0, 30.0);
    hp.pointer_move(2047.0, 0.0);
    open_now(&mut hp)?;
    Ok(())
}

/// Scenario 10: A file giving three fingers down to another action keeps
/// them: the focused window is asked to close, nothing opens.
#[test]
fn a_binding_the_file_gives_keeps_three_fingers_down() -> anyhow::Result<()> {
    let config: Config = toml::from_str("[shortcuts]\nclose = [\"Super+Q\", \"swipe:3:down\"]\n")?;
    let mut hp = screens_with(config)?;
    let (_, client) = open_with(&mut hp, "test.term", (900, 600), false)?;
    hp.swipe(3, &[(0.0, 30.0); 8], 10);
    let mut asked = false;
    hp.dispatch_until(WAIT, |_| {
        asked |= client
            .events
            .try_iter()
            .any(|e| e == common::Event::CloseRequested);
        asked
    })?;
    anyhow::ensure!(asked, "the window was never asked to close");
    assert_eq!(hp.all_desktops(), None);
    Ok(())
}

// --- The pointer --------------------------------------------------------------

/// Scenario 13: A click on the 34-inch's desktop 3: both screens show 3,
/// closed.
#[test]
fn a_click_on_a_desktop_goes_there_on_every_screen() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let at = background(&hp, WIDE, 2)?;
    // The mail is there: its picture, then the desktop's background.
    click(&mut hp, at)?;
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [2, 2]);
    Ok(())
}

/// Scenario 14: A click on the laptop's desktop 2 window: both screens show
/// 2, and that window has the keyboard.
#[test]
fn a_click_on_a_window_goes_there_and_focuses_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let at = middle(picture(&hp, "test.two")?);
    click(&mut hp, at)?;
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [1, 1]);
    assert_eq!(focused(&hp).as_deref(), Some("test.two"));
    Ok(())
}

/// Scenario 16: The pointer on the laptop's desktop 2 lights desktop 2, on
/// both screens (linked).
#[test]
fn the_pointer_lights_a_desktop() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let at = background(&hp, LAPTOP, 1)?;
    hp.pointer_move(at.0, at.1);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.grid_hover(), Some((LAPTOP.to_owned(), 1)));
    // Outside every desktop, none; a click there closes it at once.
    hp.pointer_move(ON_LAPTOP.0, 4.0);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.grid_hover(), None);
    click(&mut hp, (ON_LAPTOP.0, 4.0))?;
    assert_eq!(hp.all_desktops(), None, "not at once");
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

// --- Moving a window ------------------------------------------------------------

/// Scenario 17: The 34-inch's left-half window carried to its desktop 3:
/// the left half of the 34-inch's desktop 3; both still on 1; the view
/// open.
#[test]
fn a_snapped_window_carried_keeps_its_zone() -> anyhow::Result<()> {
    let mut hp = start()?;
    let before = where_is(&hp, "test.half").map(|w| w.2);
    opened(&mut hp)?;
    let from = middle(picture(&hp, "test.half")?);
    let to = background(&hp, WIDE, 2)?;
    carry(&mut hp, from, to)?;
    assert_eq!(
        hp.grid_carry(),
        Some((Some("test.half".to_owned()), Some((WIDE.to_owned(), 2))))
    );
    shot(&mut hp, "carried")?;
    let_go(&mut hp)?;
    let (output, desktop, rect) = where_is(&hp, "test.half").expect("the half");
    assert_eq!((output.as_str(), desktop), (WIDE, 2));
    assert_eq!(Some(rect), before, "not in the same zone");
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.all_desktops(), Some(true));
    Ok(())
}

/// Scenario 18: A floating window of the 34-inch's desktop 2 carried onto
/// the laptop's desktop 4: there, where let go, no larger than the laptop's
/// work area.
#[test]
fn a_floating_window_carried_to_the_other_screen_lands_where_let_go() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let from = middle(picture(&hp, "test.float")?);
    let four = desktop(&hp, LAPTOP, 3)?;
    let to = (four.0 + four.2 * 0.4, four.1 + four.3 * 0.4);
    carry(&mut hp, from, to)?;
    let_go(&mut hp)?;
    let (output, desktop, rect) = where_is(&hp, "test.float").expect("the window");
    assert_eq!((output.as_str(), desktop), (LAPTOP, 3));
    let area = hp.work_area_of(LAPTOP).expect("the laptop's work area");
    assert!(
        rect.width <= area.width && rect.height <= area.height,
        "{rect:?}"
    );
    assert!(
        rect.x >= area.x
            && rect.y >= area.y
            && rect.x + rect.width <= area.x + area.width
            && rect.y + rect.height <= area.y + area.height,
        "{rect:?} out of {area:?}"
    );
    // Where its picture was let go: its middle under the pointer, give or
    // take the lift's 4 %, inside the work area.
    let k = four.2 / 2048.0;
    let pointer = ((to.0 - four.0) / k, (to.1 - four.1) / k);
    let middle = (
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + f64::from(rect.height) / 2.0,
    );
    assert!(
        (middle.0 - pointer.0).abs() < 60.0 && (middle.1 - pointer.1).abs() < 60.0,
        "{middle:?} far from {pointer:?}"
    );
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.all_desktops(), Some(true));
    Ok(())
}

/// Scenario 19: Let go on its own desktop or outside every desktop, a
/// window is where it was.
#[test]
fn a_window_let_go_on_its_desktop_or_nowhere_springs_back() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let before = where_is(&hp, "test.term");
    let from = middle(picture(&hp, "test.term")?);
    let own = background(&hp, LAPTOP, 0)?;
    carry(&mut hp, from, (own.0 + 20.0, own.1 - 20.0))?;
    let_go(&mut hp)?;
    assert_eq!(where_is(&hp, "test.term"), before);
    carry(&mut hp, from, (ON_LAPTOP.0, 4.0))?;
    let_go(&mut hp)?;
    assert_eq!(where_is(&hp, "test.term"), before);
    assert_eq!(hp.all_desktops(), Some(true));
    Ok(())
}

/// Scenario 20: A window carried over the laptop's desktop 3 lights it, its
/// label saying what letting go does.
#[test]
fn a_desktop_under_a_carried_window_says_drop_here() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let from = middle(picture(&hp, "test.term")?);
    let to = background(&hp, LAPTOP, 2)?;
    carry(&mut hp, from, to)?;
    assert_eq!(
        hp.grid_carry(),
        Some((Some("test.term".to_owned()), Some((LAPTOP.to_owned(), 2))))
    );
    match hp.grid_label(LAPTOP, 2) {
        Some(GridLabel::Drop(text)) => {
            assert!(
                text == "Lâcher : bureau 3" || text == "Drop: desktop 3",
                "{text}"
            );
        }
        other => anyhow::bail!("{other:?}"),
    }
    assert!(matches!(
        hp.grid_label(LAPTOP, 1),
        Some(GridLabel::Number(_))
    ));
    // On that screen alone: the window goes to the laptop's.
    assert!(matches!(hp.grid_label(WIDE, 2), Some(GridLabel::Number(_))));
    let_go(&mut hp)?;
    Ok(())
}

// --- The keyboard ---------------------------------------------------------------

/// Scenario 21: `↓ Enter`: both on 3; again on 3, `↑ →` then `Enter`: both
/// on 2; `Tab` then `Super+Shift+4`: the selected window on desktop 4 of
/// its screen, the view open.
#[test]
fn the_arrows_enter_tab_and_super_shift_n() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    press(&mut hp, KEY_DOWN);
    assert_eq!(
        hp.grid_selection(),
        Some(GridSelection::Desktop(LAPTOP.to_owned(), 2))
    );
    shot(&mut hp, "selected")?;
    press(&mut hp, KEY_ENTER);
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [2, 2]);
    opened(&mut hp)?;
    press(&mut hp, KEY_UP);
    press(&mut hp, KEY_RIGHT);
    press(&mut hp, KEY_ENTER);
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [1, 1]);
    opened(&mut hp)?;
    press(&mut hp, KEY_TAB);
    let Some(GridSelection::Window(_, 1, _)) = hp.grid_selection() else {
        anyhow::bail!("no window selected: {:?}", hp.grid_selection());
    };
    send(&mut hp, KEY_4)?;
    let (output, desktop, _) = where_is(&hp, "test.two").expect("the window");
    assert_eq!((output.as_str(), desktop), (LAPTOP, 3));
    assert_eq!(hp.all_desktops(), Some(true));
    Ok(())
}

// --- The layout -----------------------------------------------------------------

/// Scenario 22: Six desktops: 3 × 2; eight: 3 × 3, its last place empty.
#[test]
fn the_grid_follows_the_number_of_desktops() -> anyhow::Result<()> {
    for (count, cols, rows) in [(6, 3, 2), (8, 3, 3)] {
        let mut hp = screens_with(toml::from_str(&format!("[desktops]\ncount = {count}\n"))?)?;
        opened(&mut hp)?;
        for output in [LAPTOP, WIDE] {
            let places: Vec<_> = hp
                .grid_desktops()
                .into_iter()
                .filter(|(o, _, _)| o == output)
                .map(|(_, _, a)| a)
                .collect();
            assert_eq!(places.len(), count);
            let xs: std::collections::BTreeSet<i64> =
                places.iter().map(|a| a.0.round() as i64).collect();
            let ys: std::collections::BTreeSet<i64> =
                places.iter().map(|a| a.1.round() as i64).collect();
            assert_eq!((xs.len(), ys.len()), (cols, rows), "{count}: {places:?}");
        }
    }
    Ok(())
}

/// Scenario 23: A window's picture is at the same scale on both screens,
/// 0.39.
#[test]
fn one_scale_on_both_screens() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let laptop = desktop(&hp, LAPTOP, 0)?;
    let wide = desktop(&hp, WIDE, 0)?;
    let (k1, k2) = (laptop.2 / 2048.0, wide.2 / 3440.0);
    assert!((k1 - k2).abs() * 3440.0 < 1.0, "{k1} and {k2}");
    assert!((k1 - 0.39).abs() < 0.005, "{k1}");
    // A window's picture too.
    let half = picture(&hp, "test.half")?;
    let rect = where_is(&hp, "test.half").map(|w| w.2).expect("the half");
    assert!(
        (half.2 - f64::from(rect.width) * k1).abs() < 1.0,
        "{half:?}"
    );
    Ok(())
}

/// Scenario 24: Desktop 2 the project "hocus-site": its label on both
/// screens has the monogram, "2 · hocus-site" and the folder.
#[test]
fn a_project_s_desktop_is_labelled_as_its_pill() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    let folder = home.join("github/hocus-site");
    std::fs::create_dir_all(folder.join(".git"))?;
    let config = Config {
        state_dir: Some(dir.path().join("state/hocuspanda")),
        home: Some(home),
        ..Config::default()
    };
    let mut hp = screens_with(config)?;
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(b"msg screens\n")?;
    stream.write_all(
        format!(
            "project-make\t{LAPTOP}\t2\thocus-site\t{}\n",
            folder.display()
        )
        .as_bytes(),
    )?;
    let made = hp.dispatch_until(WAIT, |hp| !hp.projects().is_empty())?;
    anyhow::ensure!(made, "no project");
    opened(&mut hp)?;
    for output in [LAPTOP, WIDE] {
        assert_eq!(
            hp.grid_label(output, 1),
            Some(GridLabel::Project {
                mono: "HS".to_owned(),
                title: "2 · hocus-site".to_owned(),
                folder: "~/github/hocus-site".to_owned(),
            }),
            "on {output}"
        );
    }
    assert!(matches!(hp.grid_label(LAPTOP, 0), Some(GridLabel::Number(n)) if n == "1"));
    shot(&mut hp, "project")?;
    Ok(())
}

/// Scenario 25: An agent waiting in the laptop's desktop 2 terminal: its
/// window carries the badge.
#[test]
fn a_waiting_agent_s_window_carries_its_badge() -> anyhow::Result<()> {
    let mut hp = start()?;
    // The agents' window: this process's last, as a terminal's is its
    // agents' ancestor.
    open(&mut hp, "test.host", (900, 600))?;
    send(&mut hp, KEY_2)?;
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let pid = std::process::id();
    let mut stream = UnixStream::connect(&socket)?;
    writeln!(stream, "agent command migration waiting {pid} migration")?;
    let until = std::time::Instant::now() + WAIT;
    while hp.agents().is_empty() && std::time::Instant::now() < until {
        hp.dispatch(Duration::from_millis(10))?;
    }
    anyhow::ensure!(hp.agents().len() == 1, "the agents: {:?}", hp.agents());
    opened(&mut hp)?;
    assert_eq!(
        hp.overview_badges(),
        [(Some("test.host".to_owned()), "waiting".to_owned(), 1)]
    );
    let (output, desktop, _) = where_is(&hp, "test.host").expect("the host");
    assert_eq!((output.as_str(), desktop), (LAPTOP, 1));
    // Its panda waves on: the view moves.
    wait(&mut hp, Duration::from_millis(200))?;
    assert!(!hp.still());
    shot(&mut hp, "agent")?;
    Ok(())
}

/// Scenario 26: Desktop 4, empty on both screens, shows the resting panda
/// on each; desktop 3, the mail on the 34-inch only, on the laptop alone.
#[test]
fn an_empty_desktop_shows_the_resting_panda() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    let mut asleep = hp.overview_asleep();
    asleep.sort();
    assert_eq!(
        asleep,
        [
            (LAPTOP.to_owned(), 2),
            (LAPTOP.to_owned(), 3),
            (WIDE.to_owned(), 3)
        ]
    );
    Ok(())
}

// --- Several screens ------------------------------------------------------------

/// Scenario 27: Per screen: a click on the laptop's desktop 3 shows 3
/// there, the 34-inch still on 1, the view closed on both.
#[test]
fn per_screen_a_click_goes_there_on_that_screen_alone() -> anyhow::Result<()> {
    let mut hp = start_with("[desktops]\nper-screen = true\n")?;
    assert_eq!(
        hp.state().config.desktops.switching,
        DesktopSwitching::PerScreen
    );
    opened(&mut hp)?;
    let at = background(&hp, LAPTOP, 2)?;
    click(&mut hp, at)?;
    closed(&mut hp)?;
    assert_eq!(shown(&hp), [2, 0]);
    Ok(())
}

// --- Motion, the lock, the screens ------------------------------------------------

/// Scenario 28: With reduced motion it fades in, 120 ms, nothing moving.
#[test]
fn with_reduced_motion_it_fades_in() -> anyhow::Result<()> {
    let mut hp = start_with("[appearance]\nreduced-motion = true\n")?;
    hp.hold_clock();
    super_shift_w(&mut hp);
    assert_eq!(hp.all_desktops(), Some(true));
    hp.step(Duration::from_millis(60))?;
    assert!(!hp.still(), "no fade");
    hp.step(Duration::from_millis(80))?;
    hp.step(common::STEP)?;
    let still = hp.still();
    hp.release_clock();
    assert!(still, "it still moves past 120 ms");
    Ok(())
}

/// Scenario 29: The session locking closes it.
#[test]
fn the_lock_closes_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    hp.lock();
    assert_eq!(hp.all_desktops(), None);
    Ok(())
}

/// Scenario 30: The 34-inch unplugged closes it.
#[test]
fn a_screen_unplugged_closes_it() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    hp.unplug(WIDE)?;
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.all_desktops(), None);
    Ok(())
}

/// Scenario 31: Open, no agent waiting, at rest: no screen draws a frame in
/// half a second (the resting pandas breathe as it opens, then lie still).
#[test]
fn at_rest_it_draws_nothing() -> anyhow::Result<()> {
    let mut hp = start()?;
    opened(&mut hp)?;
    hp.hold_clock();
    hp.step(Duration::from_secs(4))?;
    hp.step(Duration::from_millis(4400))?;
    hp.step(common::STEP)?;
    let still = hp.still();
    hp.release_clock();
    assert!(still, "something still moves");
    common::at_rest(&mut hp)?;
    let frames = (hp.frames_on(0), hp.frames_on(1));
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(
        (hp.frames_on(0), hp.frames_on(1)),
        frames,
        "frames drawn at rest"
    );
    assert_eq!(hp.all_desktops(), Some(true));
    Ok(())
}
