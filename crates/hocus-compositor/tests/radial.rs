//! A window's radial menu (#723, docs/design/windows.md §10b): `Super` and
//! a right click that does not move opens it on the pointer, a drag still
//! resizes; crossing a sector's rim picks it; Close only after a rest and
//! a click; inert sectors; pushed in near a corner; the keys; the grip's
//! right click; nothing over a shell surface. And the two window states it
//! brings: always on top, on all desktops.

use crate::common;

use std::time::Duration;

use common::{
    BTN_LEFT, ClientHandle, Event, KEY_2, KEY_DOWN, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP,
    Shows, WAIT, chord, compositor, open, open_showing, open_with, rect_of, two_screens,
    wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{Mode, Rect, WindowId, Zone},
    radial::{CLOSE_REST, REACH, RING, Sector},
};

const BTN_RIGHT: u32 = 0x111;
const KEY_ENTER: u32 = 28;
/// The work area of the headless 1920×1080 output, the bar's space kept.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<WindowId> {
    hp.all_windows()
        .into_iter()
        .find(|(_, app, _, _)| app.as_deref() == Some(app_id))
        .map(|(id, _, _, _)| id)
        .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))
}

fn middle(rect: Rect) -> (f64, f64) {
    (
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + f64::from(rect.height) / 2.0,
    )
}

/// `Super` and a right click at `at`, without moving.
fn super_click(hp: &mut Headless, at: (f64, f64)) {
    hp.pointer_move(at.0, at.1);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_RIGHT, true);
    hp.button(BTN_RIGHT, false);
    hp.key(KEY_LEFTMETA, false);
}

/// The pointer from `from` toward `(dx, dy)`, in steps of `step` pixels.
fn glide(hp: &mut Headless, from: (f64, f64), (dx, dy): (f64, f64), steps: u32) {
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        hp.pointer_move(from.0 + dx * t, from.1 + dy * t);
    }
}

/// A point `r` from the centre toward sector `sector`.
fn toward(centre: (f64, f64), sector: Sector, r: f64) -> (f64, f64) {
    let a = (sector.index() as f64 * 45.0 - 90.0).to_radians();
    (centre.0 + r * a.cos(), centre.1 + r * a.sin())
}

/// Opens a window's menu at its middle; its centre.
fn opened(hp: &mut Headless, app_id: &str) -> anyhow::Result<(f64, f64)> {
    let rect = rect_of(hp, app_id).ok_or_else(|| anyhow::anyhow!("no {app_id}"))?;
    let at = middle(rect);
    super_click(hp, at);
    let seen = hp
        .radial()
        .ok_or_else(|| anyhow::anyhow!("no menu after Super and a right click"))?;
    anyhow::ensure!(seen.window == id_of(hp, app_id)?, "another window's menu");
    Ok(seen.centre)
}

#[test]
fn super_and_a_right_click_opens_it_on_the_pointer_and_a_drag_still_resizes() -> anyhow::Result<()>
{
    let mut hp = compositor()?;
    let (rect, client) = open_showing(
        &mut hp,
        "test.radial",
        Shows::Pointing {
            app_id: "test.radial",
            size: (800, 600),
        },
    )?;
    let at = middle(rect);
    super_click(&mut hp, at);
    let seen = hp.radial().expect("the menu opens");
    assert_eq!(seen.centre, at, "centred on the pointer");
    assert_eq!(seen.lit, None);
    // It stays open once the button is up; the window saw no press.
    hp.dispatch(Duration::from_millis(50))?;
    assert!(hp.radial().is_some());
    let pressed = client
        .events
        .try_iter()
        .any(|e| e == Event::Pointer(common::Pointed::Press));
    assert!(!pressed, "the window saw the press");
    // `Esc` closes it.
    chord(&mut hp, &[KEY_ESC]);
    assert!(hp.radial().is_none());
    // A drag resizes, from the nearest corner, as it always did.
    let (x, y) = (f64::from(rect.x + 50), f64::from(rect.y + 40));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_RIGHT, true);
    hp.pointer_move(x - 20.0, y - 15.0);
    hp.pointer_move(x - 40.0, y - 30.0);
    hp.button(BTN_RIGHT, false);
    hp.key(KEY_LEFTMETA, false);
    assert!(hp.radial().is_none(), "a drag opens nothing");
    wait_for_rect(
        &mut hp,
        "test.radial",
        Rect::new(rect.x - 40, rect.y - 30, rect.width + 40, rect.height + 30),
    )?;
    Ok(())
}

#[test]
fn moving_up_past_the_rim_maximises_without_a_click() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.up", (600, 400))?;
    let centre = opened(&mut hp, "test.up")?;
    glide(&mut hp, centre, (0.0, -(HUB_IN)), 3);
    assert_eq!(hp.radial().map(|r| r.lit), Some(Some(Sector::Up)));
    glide(
        &mut hp,
        (centre.0, centre.1 - HUB_IN),
        (0.0, -(RING + 10.0 - HUB_IN)),
        4,
    );
    assert!(hp.radial().is_none(), "crossing the rim picks");
    let id = id_of(&hp, "test.up")?;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Maximized));
    wait_for_rect(&mut hp, "test.up", AREA)?;
    Ok(())
}

/// Inside the ring, past the hub.
const HUB_IN: f64 = 70.0;

#[test]
fn moving_right_past_the_rim_sends_it_to_the_next_desktop_and_the_screen_stays()
-> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.right", (600, 400))?;
    let centre = opened(&mut hp, "test.right")?;
    // A quick flick, one move from the hub to past the rim.
    hp.pointer_move(centre.0 + RING + 30.0, centre.1 + 5.0);
    assert!(hp.radial().is_none());
    let id = id_of(&hp, "test.right")?;
    let done = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(w, _, desktop, shown)| *w == id && *desktop == 1 && !shown)
    })?;
    assert!(done, "not on desktop 2: {:?}", hp.all_windows());
    assert_eq!(hp.desktop().0, 0, "the screen stays on its desktop");
    Ok(())
}

#[test]
fn crossing_closes_rim_does_nothing_and_a_click_after_a_rest_asks_it_to_close() -> anyhow::Result<()>
{
    let mut hp = compositor()?;
    let (_, client) = open_with(&mut hp, "test.close", (600, 400), false)?;
    hp.hold_clock();
    let centre = opened(&mut hp, "test.close")?;
    glide(
        &mut hp,
        centre,
        {
            let (x, y) = toward(centre, Sector::DownLeft, RING + 20.0);
            (x - centre.0, y - centre.1)
        },
        6,
    );
    let seen = hp.radial().expect("crossing Close's rim picks nothing");
    assert_eq!(seen.lit, Some(Sector::DownLeft));
    assert!(!seen.armed);
    // A click before the rest is ignored.
    hp.step(Duration::from_millis(150))?;
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert!(hp.radial().is_some(), "a click too soon closes nothing");
    hp.step(CLOSE_REST)?;
    assert!(hp.radial().is_some_and(|r| r.armed), "armed after the rest");
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert!(hp.radial().is_none());
    hp.release_clock();
    let asked = asked_to_close(&mut hp, &client)?;
    assert!(asked, "the window was not asked to close");
    // Asked, it decides: it stays.
    assert!(rect_of(&hp, "test.close").is_some());
    Ok(())
}

fn asked_to_close(hp: &mut Headless, client: &ClientHandle) -> anyhow::Result<bool> {
    let mut asked = false;
    hp.dispatch_until(Duration::from_secs(5), |_| {
        asked |= client.events.try_iter().any(|e| e == Event::CloseRequested);
        asked
    })?;
    Ok(asked)
}

#[test]
fn on_the_first_desktop_previous_is_inert_and_with_one_screen_other_screen_too()
-> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.inert", (600, 400))?;
    let centre = opened(&mut hp, "test.inert")?;
    let seen = hp.radial().expect("open");
    assert!(!seen.enabled[Sector::Left.index()]);
    assert!(!seen.enabled[Sector::DownRight.index()]);
    assert!(seen.enabled[Sector::Right.index()]);
    // Crossing an inert sector's rim does nothing.
    hp.pointer_move(centre.0 - RING - 30.0, centre.1);
    assert!(hp.radial().is_some_and(|r| r.lit.is_none()));
    let id = id_of(&hp, "test.inert")?;
    assert!(
        hp.all_windows()
            .iter()
            .any(|(w, _, d, _)| *w == id && *d == 0)
    );
    Ok(())
}

#[test]
fn with_two_screens_other_screen_sends_it_there_onto_the_desktop_shown() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    open(&mut hp, "test.other", (600, 400))?;
    let centre = opened(&mut hp, "test.other")?;
    assert!(
        hp.radial()
            .is_some_and(|r| r.enabled[Sector::DownRight.index()])
    );
    let (x, y) = toward(centre, Sector::DownRight, RING + 25.0);
    hp.pointer_move(x, y);
    assert!(hp.radial().is_none());
    let moved = hp.dispatch_until(WAIT, |hp| {
        rect_of(hp, "test.other").is_some_and(|r| r.x >= 1920)
    })?;
    assert!(moved, "still at {:?}", rect_of(&hp, "test.other"));
    let id = id_of(&hp, "test.other")?;
    assert!(hp.windows().iter().any(|w| w.id == id && w.focused));
    Ok(())
}

#[test]
fn opened_near_the_corner_it_stays_whole_on_the_screen() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let rect = open(&mut hp, "test.corner", (600, 400))?;
    // Moved to the bottom right corner with Alt+drag would take long: the
    // window is big enough to reach 20 px from the corner once maximised.
    let id = id_of(&hp, "test.corner")?;
    hp.pointer_move(f64::from(rect.x + 10), f64::from(rect.y + 10));
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "test.corner", AREA)?;
    let at = (
        f64::from(AREA.x + AREA.width) - 20.0,
        f64::from(AREA.y + AREA.height) - 20.0,
    );
    super_click(&mut hp, at);
    let seen = hp.radial().expect("the menu opens");
    assert_eq!(seen.window, id);
    assert_eq!(seen.centre, (at.0.min(1920.0 - REACH), 1080.0 - REACH));
    assert!(seen.centre.0 + REACH <= 1920.0 && seen.centre.1 + REACH <= 1080.0);
    // Maximised: up restores.
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Maximized));
    Ok(())
}

#[test]
fn the_keys_light_a_sector_and_enter_picks_it() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.keys", (600, 400))?;
    opened(&mut hp, "test.keys")?;
    chord(&mut hp, &[KEY_DOWN]);
    assert_eq!(hp.radial().map(|r| r.lit), Some(Some(Sector::Down)));
    chord(&mut hp, &[KEY_ENTER]);
    assert!(hp.radial().is_none());
    assert_eq!(hp.minimized(), 1, "minimised");
    Ok(())
}

#[test]
fn a_right_click_on_the_grip_opens_it_too() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (rect, _client) = open_showing(
        &mut hp,
        "test.grip",
        Shows::KdeDecorated {
            app_id: "test.grip",
            size: (600, 400),
            server: true,
            header: None,
        },
    )?;
    let grip = (
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + 6.0,
    );
    hp.pointer_move(grip.0, grip.1 + 1.0);
    hp.pointer_move(grip.0, grip.1);
    hp.button(BTN_RIGHT, true);
    hp.button(BTN_RIGHT, false);
    let seen = hp.radial().expect("the grip's right click opens it");
    assert_eq!(seen.window, id_of(&hp, "test.grip")?);
    Ok(())
}

#[test]
fn over_a_shell_surface_super_and_a_right_click_open_nothing() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let rect = open(&mut hp, "test.under", (1200, 800))?;
    // A layer surface is no window: it shows once it has its place.
    let _layer = common::client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: "hocus-bar-popover",
            size: (300, 300),
            margin: (200, 500),
            colour: 0xff20_2020,
        }),
    )?;
    let shown = hp.dispatch_until(WAIT, |hp| !hp.layer_rects("hocus-bar-popover").is_empty())?;
    assert!(shown, "no layer surface");
    let (_, layer) = hp.layer_rects("hocus-bar-popover")[0].clone();
    let over = middle(layer);
    assert!(
        rect.contains(over.0, over.1),
        "the layer is over the window"
    );
    // Once it takes the pointer there.
    common::over(&mut hp, over, "hocus-bar-popover")?;
    super_click(&mut hp, over);
    assert!(hp.radial().is_none(), "opened over a shell surface");
    Ok(())
}

#[test]
fn always_on_top_keeps_it_above_a_window_focused_after() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.below", (900, 700))?;
    open(&mut hp, "test.above", (400, 300))?;
    let centre = opened(&mut hp, "test.above")?;
    let (x, y) = toward(centre, Sector::UpRight, RING + 25.0);
    hp.pointer_move(x, y);
    assert!(hp.radial().is_none());
    let above = id_of(&hp, "test.above")?;
    assert!(hp.above(above));
    // A click on the other window focuses it; the first stays above.
    let below = rect_of(&hp, "test.below").expect("below");
    hp.pointer_move(f64::from(below.x + 20), f64::from(below.y + 20));
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let below_id = id_of(&hp, "test.below")?;
    assert!(hp.windows().iter().any(|w| w.id == below_id && w.focused));
    assert_eq!(hp.stack().last(), Some(&above), "{:?}", hp.stack());
    // Again: it stacks as the others.
    let centre = opened(&mut hp, "test.above")?;
    let (x, y) = toward(centre, Sector::UpRight, RING + 25.0);
    hp.pointer_move(x, y);
    assert!(!hp.above(above));
    Ok(())
}

#[test]
fn on_all_desktops_it_shows_on_each_and_again_on_the_current_one_only() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.every", (600, 400))?;
    let centre = opened(&mut hp, "test.every")?;
    let (x, y) = toward(centre, Sector::UpLeft, RING + 25.0);
    hp.pointer_move(x, y);
    assert!(hp.radial().is_none());
    let id = id_of(&hp, "test.every")?;
    assert_eq!(hp.everywhere(), vec![id]);
    // On desktop 2, it shows, where it was.
    let rect = rect_of(&hp, "test.every").expect("shown");
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let switched = hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 1)?;
    assert!(switched);
    assert_eq!(rect_of(&hp, "test.every"), Some(rect));
    assert!(
        hp.all_windows()
            .iter()
            .any(|(w, _, d, shown)| *w == id && *d == 1 && *shown)
    );
    // Again, from desktop 2: on that one only.
    let centre = opened(&mut hp, "test.every")?;
    let (x, y) = toward(centre, Sector::UpLeft, RING + 25.0);
    hp.pointer_move(x, y);
    assert!(hp.everywhere().is_empty());
    chord(&mut hp, &[KEY_LEFTMETA, common::KEY_1]);
    let back = hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 0)?;
    assert!(back);
    assert!(
        hp.all_windows()
            .iter()
            .any(|(w, _, d, shown)| *w == id && *d == 1 && !shown),
        "{:?}",
        hp.all_windows()
    );
    Ok(())
}

#[test]
fn sending_it_to_a_desktop_by_its_keys_takes_it_off_all_desktops() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.sent", (600, 400))?;
    let centre = opened(&mut hp, "test.sent")?;
    let (x, y) = toward(centre, Sector::UpLeft, RING + 25.0);
    hp.pointer_move(x, y);
    let id = id_of(&hp, "test.sent")?;
    assert_eq!(hp.everywhere(), vec![id]);
    let rect = rect_of(&hp, "test.sent").expect("shown");
    hp.pointer_move(f64::from(rect.x + 10), f64::from(rect.y + 10));
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    assert!(hp.everywhere().is_empty());
    let sent = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(w, _, d, shown)| *w == id && *d == 1 && !shown)
    })?;
    assert!(sent, "{:?}", hp.all_windows());
    Ok(())
}

/// A window sent to the next desktop (#728): it stays in front of the
/// window focused in its stead while it slides away, and lands on top of
/// the windows of the desktop it goes to; sent back by its keys
/// (`Super+Shift+1`), the same.
#[test]
fn a_window_sent_away_stays_in_front_as_it_leaves_and_lands_on_top() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let on = |hp: &mut Headless, desktop: usize| -> anyhow::Result<()> {
        let done = hp.dispatch_until(WAIT, |hp| hp.desktop().0 == desktop)?;
        anyhow::ensure!(done, "never on desktop {}", desktop + 1);
        Ok(())
    };
    let focused = |hp: &Headless| hp.windows().into_iter().find(|w| w.focused).map(|w| w.id);
    let index = |hp: &Headless, id: WindowId| hp.stack().iter().position(|w| *w == id);
    // Desktop 2 has a window already; desktop 1 two, the one sent on top.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    on(&mut hp, 1)?;
    open(&mut hp, "test.there", (900, 700))?;
    chord(&mut hp, &[KEY_LEFTMETA, common::KEY_1]);
    on(&mut hp, 0)?;
    open(&mut hp, "test.behind", (900, 700))?;
    open(&mut hp, "test.sent", (600, 400))?;
    let (sent, behind, there) = (
        id_of(&hp, "test.sent")?,
        id_of(&hp, "test.behind")?,
        id_of(&hp, "test.there")?,
    );
    let centre = opened(&mut hp, "test.sent")?;
    hp.pointer_move(centre.0 + RING + 30.0, centre.1 + 5.0);
    assert!(hp.radial().is_none());
    // As it leaves: the one behind focused, the one sent still drawn over
    // it, the stack's order the drawing's.
    assert_eq!(focused(&hp), Some(behind));
    assert_eq!(hp.stack().last(), Some(&sent), "{:?}", hp.stack());
    let gone = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(w, _, d, shown)| *w == sent && *d == 1 && !shown)
    })?;
    anyhow::ensure!(gone, "not sent: {:?}", hp.all_windows());
    // On desktop 2: over the window that was there.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    on(&mut hp, 1)?;
    assert!(index(&hp, sent) > index(&hp, there), "{:?}", hp.stack());
    assert_eq!(focused(&hp), Some(sent));
    // Back by its keys: in front still as it leaves, on top over there.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, common::KEY_1]);
    assert_eq!(focused(&hp), Some(there));
    assert_eq!(hp.stack().last(), Some(&sent), "{:?}", hp.stack());
    chord(&mut hp, &[KEY_LEFTMETA, common::KEY_1]);
    on(&mut hp, 0)?;
    assert!(index(&hp, sent) > index(&hp, behind), "{:?}", hp.stack());
    Ok(())
}
