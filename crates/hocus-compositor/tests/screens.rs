//! Two screens, headless (docs/design/windows.md §9): a snap, by dragging
//! or by the keys, lands on the screen it was made on, and stays there as
//! its work area changes, through fullscreen and back, and as a snapped
//! pair is resized.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, Command, KEY_LEFT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT, KEY_UP, Shows, WAIT,
    chord, client, open, open_with, rect_of, two_screens, wait, wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{GAP, Mode, Rect, Zone},
};

fn id_of(hp: &Headless, app_id: &str) -> Option<hocus_compositor::model::WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
}

/// The second screen's work area once it has one.
fn second_area(hp: &Headless) -> anyhow::Result<Rect> {
    let areas = hp.work_areas();
    anyhow::ensure!(areas.len() == 2, "two work areas: {areas:?}");
    Ok(areas[1])
}

/// A bar on the second screen: its work area shrinks; the new one.
fn bar_on_second(hp: &mut Headless, before: Rect) -> anyhow::Result<Rect> {
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let bar = client(hp.socket(), Some(Shows::Bar { height: 40 }))?;
    std::mem::forget(bar);
    let shrunk = hp.dispatch_until(WAIT, |hp| {
        hp.work_areas().get(1).is_some_and(|a| a.y == before.y + 40)
    })?;
    anyhow::ensure!(shrunk, "work areas: {:?}", hp.work_areas());
    second_area(hp)
}

#[test]
fn a_drag_snaps_on_the_screen_under_the_pointer_and_stays_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let area = second_area(&hp)?;
    // Opened on the first screen, held near its right edge.
    hp.pointer_move(960.0, 540.0);
    let (start, window) = open_with(&mut hp, "test.drag", (400, 300), false)?;
    assert!(start.right() < 1920, "on the first screen: {start:?}");
    let (x, y) = (f64::from(start.right() - 20), f64::from(start.y + 150));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    // The second screen's top-left corner: most of the window, and its
    // centre, still on the first.
    hp.pointer_move(1921.0, 2.0);
    let moved = rect_of(&hp, "test.drag").expect("mapped");
    assert!(
        moved.centre().0 < 1920.0,
        "its centre on the first: {moved:?}"
    );
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    let id = id_of(&hp, "test.drag").expect("mapped");
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::TopLeft));
    // Its screen's work area changes: it follows, on that screen.
    let area = bar_on_second(&mut hp, area)?;
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    // Fullscreen over its screen, and back in its quarter there.
    window.commands.send(Command::Fullscreen(true))?;
    wait_for_rect(&mut hp, "test.drag", Rect::new(1920, 0, 2560, 1440))?;
    window.commands.send(Command::Fullscreen(false))?;
    wait_for_rect(&mut hp, "test.drag", Zone::TopLeft.rect(area))?;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::TopLeft));
    Ok(())
}

#[test]
fn the_keys_snap_on_the_windows_screen_and_a_pair_stays_there() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let area = second_area(&hp)?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.left", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP]);
    wait_for_rect(&mut hp, "test.left", Zone::TopLeft.rect(area))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "test.left", area)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    open(&mut hp, "test.right", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(area))?;
    // The pair's handle, in the middle of the seam: both resize, and stay.
    let left = Zone::Left.rect(area);
    let (x, y) = (
        f64::from(left.right() + GAP / 2),
        f64::from(left.y) + f64::from(left.height) / 2.0,
    );
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x - 200.0, y);
    hp.button(BTN_LEFT, false);
    let narrower = Rect::new(left.x, left.y, left.width - 200, left.height);
    wait_for_rect(&mut hp, "test.left", narrower)?;
    let right = Zone::Right.rect(area);
    let wider = Rect::new(
        narrower.right() + GAP,
        right.y,
        right.right() - narrower.right() - GAP,
        right.height,
    );
    wait_for_rect(&mut hp, "test.right", wider)?;
    for app in ["test.left", "test.right"] {
        let id = id_of(&hp, app).expect("mapped");
        assert!(matches!(hp.mode(id), Mode::Snapped(_)), "{app} snapped");
    }
    // A re-layout of that screen puts them back in their halves there.
    let area = bar_on_second(&mut hp, area)?;
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(area))?;
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(area))?;
    Ok(())
}
