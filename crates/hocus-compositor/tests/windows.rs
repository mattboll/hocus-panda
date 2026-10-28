//! Windows on screen, headless (docs/design/windows.md §4–§10): snapping by
//! the keys and by dragging, resizing from the frame, a snapped pair
//! resizing together, fullscreen and back, minimising, menus that grab
//! the keyboard, and no frame drawn once nothing moves.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, Command, Event, KEY_DOWN, KEY_H, KEY_LEFT, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT,
    KEY_UP, WAIT, chord, compositor, open, open_with, rect_of, wait, wait_for_rect,
};
use hocus_compositor::model::{GAP, Mode, Rect, Zone};

/// The work area of the headless 1920×1080 output: 8 px in, and the full
/// bar's place kept on the right.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

#[test]
fn the_keys_snap_to_halves_quarters_maximise_restore_and_minimise() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let floating = open(&mut hp, "test.keys", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::Left.rect(AREA))?;
    // Again: the other side.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::Right.rect(AREA))?;
    // A quarter keeps the side it is on.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_UP]);
    wait_for_rect(&mut hp, "test.keys", Zone::TopRight.rect(AREA))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.keys", Zone::TopLeft.rect(AREA))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    wait_for_rect(&mut hp, "test.keys", AREA)?;
    // Restored where it floated, then minimised.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_DOWN]);
    wait_for_rect(&mut hp, "test.keys", floating)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_DOWN]);
    assert_eq!(hp.minimized(), 1);
    assert!(rect_of(&hp, "test.keys").is_none(), "not shown");
    // Super+Shift+H brings it back, focused.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_H]);
    assert_eq!(hp.minimized(), 0);
    let window = &hp.windows()[0];
    assert_eq!(window.rect, floating);
    assert!(window.focused && window.keyboard);
    Ok(())
}

#[test]
fn dragging_to_an_edge_snaps_once_the_preview_shows() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.drag", (400, 300))?;
    let (x, y) = (f64::from(start.x + 200), f64::from(start.y + 150));
    // Across the left edge and back, quickly: nothing.
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(1.0, y);
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(250))?;
    assert_eq!(rect_of(&hp, "test.drag"), Some(start));
    // Held at the edge past 80 ms: the left half.
    hp.button(BTN_LEFT, true);
    hp.pointer_move(1.0, y);
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(&mut hp, "test.drag", Zone::Left.rect(AREA))?;
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Snapped(Zone::Left));
    // A top corner: a quarter.
    hp.key(KEY_LEFTMETA, true);
    hp.pointer_move(400.0, 500.0);
    hp.button(BTN_LEFT, true);
    // Pulled away, it floats again at its size, under the pointer.
    hp.pointer_move(420.0, 520.0);
    // Held at 392 of its 846 px across and 492 of its 1064 px down.
    wait_for_rect(&mut hp, "test.drag", Rect::new(235, 381, 400, 300))?;
    hp.pointer_move(1700.0, 2.0);
    wait(&mut hp, Duration::from_millis(120))?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(&mut hp, "test.drag", Zone::TopRight.rect(AREA))?;
    Ok(())
}

#[test]
fn the_frame_resizes_from_a_corner_and_an_edge() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.resize", (400, 300))?;
    // The band outside the bottom-right corner.
    let (x, y) = (f64::from(start.right() + 3), f64::from(start.bottom() + 3));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 50.0, y + 40.0);
    hp.button(BTN_LEFT, false);
    let grown = Rect::new(start.x, start.y, 450, 340);
    wait_for_rect(&mut hp, "test.resize", grown)?;
    // The left edge: the right one stays.
    let (x, y) = (f64::from(grown.x - 4), f64::from(grown.y + 100));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 30.0, y);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.resize",
        Rect::new(grown.x + 30, grown.y, 420, 340),
    )?;
    Ok(())
}

#[test]
fn a_snapped_pair_resizes_together() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.left", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.left", Zone::Left.rect(AREA))?;
    open(&mut hp, "test.right", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(&mut hp, "test.right", Zone::Right.rect(AREA))?;
    // The left one's right edge, in the gap between them.
    let left = Zone::Left.rect(AREA);
    let (x, y) = (f64::from(left.right() + 2), 500.0);
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 100.0, y);
    hp.button(BTN_LEFT, false);
    let wider = Rect::new(left.x, left.y, left.width + 100, left.height);
    wait_for_rect(&mut hp, "test.left", wider)?;
    let right = Zone::Right.rect(AREA);
    wait_for_rect(
        &mut hp,
        "test.right",
        Rect::new(
            wider.right() + GAP,
            right.y,
            right.right() - wider.right() - GAP,
            right.height,
        ),
    )?;
    Ok(())
}

#[test]
fn fullscreen_covers_the_output_and_comes_back() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (start, client) = open_with(&mut hp, "test.full", (400, 300), false)?;
    client.commands.send(Command::Fullscreen(true))?;
    wait_for_rect(&mut hp, "test.full", Rect::new(0, 0, 1920, 1080))?;
    let id = hp.windows()[0].id;
    assert_eq!(hp.mode(id), Mode::Fullscreen);
    client.commands.send(Command::Fullscreen(false))?;
    wait_for_rect(&mut hp, "test.full", start)?;
    assert_eq!(hp.mode(id), Mode::Floating);
    Ok(())
}

#[test]
fn a_menu_takes_the_keyboard_and_a_press_elsewhere_closes_it() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (rect, client) = open_with(&mut hp, "test.menu", (400, 300), true)?;
    hp.pointer_move(f64::from(rect.x + 20), f64::from(rect.y + 20));
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let mut shown = false;
    hp.dispatch_until(WAIT, |hp| {
        shown |= client.events.try_recv().ok() == Some(Event::PopupShown);
        shown && hp.keyboard_on_popup()
    })?;
    assert!(shown && hp.keyboard_on_popup(), "the menu has the keyboard");
    assert!(!hp.windows()[0].keyboard);
    // A press where nothing of its application is.
    hp.pointer_move(2.0, 2.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    let mut done = false;
    hp.dispatch_until(WAIT, |hp| {
        done |= client.events.try_recv().ok() == Some(Event::PopupDone);
        done && !hp.keyboard_on_popup()
    })?;
    assert!(done, "the menu was told to close");
    let window = &hp.windows()[0];
    assert!(window.keyboard && window.focused, "the keyboard is back");
    Ok(())
}

#[test]
fn nothing_is_drawn_once_the_motions_end() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    open(&mut hp, "test.rest", (400, 300))?;
    // The opening (220 ms) and the focus rim (120 ms) run, then stop.
    wait(&mut hp, Duration::from_millis(500))?;
    let frames = hp.frames();
    assert!(frames > 2, "the opening was drawn: {frames} frames");
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames(), frames, "no frame at rest");
    Ok(())
}

#[test]
fn super_and_right_drag_resizes_from_the_nearest_corner() -> anyhow::Result<()> {
    const BTN_RIGHT: u32 = 0x111;
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.corner", (400, 300))?;
    // Nearer the top-left corner: it moves, the bottom-right one stays.
    let (x, y) = (f64::from(start.x + 50), f64::from(start.y + 40));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_RIGHT, true);
    hp.pointer_move(x - 40.0, y - 30.0);
    hp.button(BTN_RIGHT, false);
    hp.key(KEY_LEFTMETA, false);
    wait_for_rect(
        &mut hp,
        "test.corner",
        Rect::new(start.x - 40, start.y - 30, 440, 330),
    )?;
    Ok(())
}
