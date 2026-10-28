//! Magnetic edges, headless (docs/design/windows.md §6, §8): a window
//! moved near another sticks at the gap and comes free dragged on, sticks
//! flush to the work area's edges, a resized edge sticks too, and windows
//! that do not show do not pull.

mod common;

use std::time::Duration;

use anyhow::Context;

use common::{
    BTN_LEFT, KEY_2, KEY_DOWN, KEY_LEFTMETA, chord, compositor, open, wait, wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{GAP, Rect},
};

/// The work area of the headless 1920×1080 output.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

/// Starts moving the window at `rect` with `Super`, held at its middle;
/// the pointer's place then.
fn grab(hp: &mut Headless, rect: Rect) -> (f64, f64) {
    let at = rect.centre();
    hp.pointer_move(at.0, at.1);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    at
}

fn release(hp: &mut Headless) {
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
}

/// Moves the window at `rect` so that its corner would be at (`x`, `y`)
/// without the magnet, and lets it go.
fn drag_to(hp: &mut Headless, rect: Rect, x: i32, y: i32) {
    let at = grab(hp, rect);
    hp.pointer_move(at.0 + f64::from(x - rect.x), at.1 + f64::from(y - rect.y));
    release(hp);
}

#[test]
fn a_window_moved_near_another_sticks_at_the_gap_and_comes_free() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.still", (400, 300))?;
    let b = open(&mut hp, "test.moved", (400, 300))?;
    // 10 px short of the gap right of the other: at the gap.
    let at = grab(&mut hp, b);
    let dx = f64::from(a.right() + GAP + 10 - b.x);
    hp.pointer_move(at.0 + dx, at.1 + 3.0);
    let stuck = Rect::new(a.right() + GAP, a.y, 400, 300);
    wait_for_rect(&mut hp, "test.moved", stuck)?;
    // 13 px away: free, under the pointer.
    hp.pointer_move(at.0 + dx + 3.0, at.1 + 30.0);
    release(&mut hp);
    let free = Rect::new(a.right() + GAP + 13, a.y + 30, 400, 300);
    wait_for_rect(&mut hp, "test.moved", free)?;
    Ok(())
}

#[test]
fn near_the_work_areas_edge_a_window_sticks_flush() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.edge", (400, 300))?;
    // 9 px in from the left and 11 px up from the bottom, the pointer far
    // from the edges where it would snap.
    drag_to(&mut hp, start, AREA.x + 9, AREA.bottom() - 300 - 11);
    let flush = Rect::new(AREA.x, AREA.bottom() - 300, 400, 300);
    wait_for_rect(&mut hp, "test.edge", flush)?;
    Ok(())
}

#[test]
fn a_resized_edge_sticks_at_the_gap() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.resized", (400, 300))?;
    let b = open(&mut hp, "test.right", (400, 300))?;
    drag_to(&mut hp, b, 1250, b.y);
    let b = Rect::new(1250, b.y, 400, 300);
    wait_for_rect(&mut hp, "test.right", b)?;
    // The right edge, dragged to 7 px short of the gap.
    let (x, y) = (f64::from(a.right() + 3), f64::from(a.y + 100));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + f64::from(b.x - GAP - 7 - a.right()), y);
    hp.button(BTN_LEFT, false);
    let grown = Rect::new(a.x, a.y, b.x - GAP - a.x, 300);
    wait_for_rect(&mut hp, "test.resized", grown)?;
    Ok(())
}

#[test]
fn windows_minimised_or_on_another_desktop_do_not_pull() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // Minimised.
    let hidden = open(&mut hp, "test.minimised", (400, 300))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_DOWN]);
    assert_eq!(hp.minimized(), 1);
    let b = open(&mut hp, "test.alone", (400, 300))?;
    let near = (hidden.right() + GAP + 10, hidden.y + 40);
    drag_to(&mut hp, b, near.0, near.1);
    wait_for_rect(&mut hp, "test.alone", Rect::new(near.0, near.1, 400, 300))?;
    // On the first desktop, seen from the second: 10 px short of the gap
    // left of the window just moved.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let c = open(&mut hp, "test.second", (400, 300))?;
    let near = (near.0 - GAP - 10 - 400, c.y + 80);
    drag_to(&mut hp, c, near.0, near.1);
    wait_for_rect(&mut hp, "test.second", Rect::new(near.0, near.1, 400, 300))?;
    Ok(())
}

#[test]
fn sticking_does_not_make_the_jelly_jump() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.still", (400, 300))?;
    let b = open(&mut hp, "test.jelly", (400, 300))?;
    // 20 px short of the gap, then still until the jelly rests.
    let at = grab(&mut hp, b);
    let dx = f64::from(a.right() + GAP + 20 - b.x);
    hp.pointer_move(at.0 + dx, at.1 + 50.0);
    wait(&mut hp, Duration::from_millis(800))?;
    let rest = hp.bend().unwrap_or(0.0);
    assert!(rest < 1.0, "at rest: {rest}");
    // 8 px on: it jumps the last 12 px, and bends only by the 8.
    hp.pointer_move(at.0 + dx - 8.0, at.1 + 50.0);
    wait_for_rect(
        &mut hp,
        "test.jelly",
        Rect::new(a.right() + GAP, b.y + 50, 400, 300),
    )?;
    let bend = hp.bend().context("it wobbles")?;
    assert!(bend <= 8.5, "the jelly bent by {bend} px");
    // Held still by the magnet, it does not bend.
    wait(&mut hp, Duration::from_millis(800))?;
    hp.pointer_move(at.0 + dx - 14.0, at.1 + 50.0);
    let bend = hp.bend().unwrap_or(0.0);
    assert!(bend < 1.0, "the jelly bent by {bend} px");
    release(&mut hp);
    Ok(())
}
