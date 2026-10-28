//! A seam's handle, headless (docs/design/windows.md §8): two windows
//! brought together by hand form a pair whose handle resizes both, floating
//! or snapped; windows not at the gap form none; and the handle shows and
//! takes presses only where the seam itself shows.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, KEY_LEFT, KEY_LEFTMETA, KEY_RIGHT, WINDOW_COLOUR, chord, compositor, open, rect_of,
    wait, wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{GAP, Mode, Rect, WindowId, Zone},
};
use smithay::input::pointer::CursorIcon;

/// The work area of the headless 1920×1080 output.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

/// Moves the window at `rect` with `Super`, held at its middle, so that
/// its corner would be at (`x`, `y`) without the magnet, and lets it go.
fn drag_to(hp: &mut Headless, rect: Rect, x: i32, y: i32) {
    let at = rect.centre();
    hp.pointer_move(at.0, at.1);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(at.0 + f64::from(x - rect.x), at.1 + f64::from(y - rect.y));
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
}

/// Presses at (`x`, `y`), drags by `dx` across, and lets go.
fn drag_by(hp: &mut Headless, (x, y): (f64, f64), dx: f64) {
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + dx, y);
    hp.button(BTN_LEFT, false);
}

fn id_of(hp: &Headless, app: &str) -> WindowId {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app))
        .map(|w| w.id)
        .expect("no such window")
}

/// The pixel at (`x`, `y`) of a screenshot, ARGB.
fn pixel((width, _, bytes): &(i32, i32, Vec<u8>), x: f64, y: f64) -> Option<u32> {
    let at = usize::try_from((y as i32) * width + x as i32).ok()? * 4;
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[test]
fn two_floating_windows_stuck_at_the_gap_form_a_pair_whose_handle_resizes_both()
-> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.left", (400, 300))?;
    let b = open(&mut hp, "test.right", (400, 300))?;
    // The one on top aside, then the other brought next to it by hand,
    // 6 px short of the gap: the magnet holds it there.
    drag_to(&mut hp, b, 1100, 450);
    let b = Rect::new(1100, 450, 400, 300);
    wait_for_rect(&mut hp, "test.right", b)?;
    drag_to(&mut hp, a, b.x - GAP - 6 - a.width, 300);
    let a = Rect::new(b.x - GAP - a.width, 300, 400, 300);
    wait_for_rect(&mut hp, "test.left", a)?;
    // The handle shows at the middle of what they share along the seam.
    let (x, y) = (
        f64::from(a.right() + GAP / 2),
        f64::from(b.y + a.bottom()) / 2.0,
    );
    hp.pointer_move(x, y);
    let (seam, _) = hp.state().seam_handle.expect("the handle shows");
    assert!(seam.vertical);
    assert_eq!(seam.centre(), (x, y));
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::EwResize));
    // It resizes both, and both still float.
    drag_by(&mut hp, (x, y), 60.0);
    let wider = Rect::new(a.x, a.y, a.width + 60, a.height);
    wait_for_rect(&mut hp, "test.left", wider)?;
    let narrower = Rect::new(b.x + 60, b.y, b.width - 60, b.height);
    wait_for_rect(&mut hp, "test.right", narrower)?;
    assert_eq!(hp.mode(id_of(&hp, "test.left")), Mode::Floating);
    assert_eq!(hp.mode(id_of(&hp, "test.right")), Mode::Floating);
    // The shared edge itself still resizes only the window grabbed.
    let edge = (f64::from(wider.right() + 2), f64::from(b.y + 40));
    drag_by(&mut hp, edge, -30.0);
    wait_for_rect(
        &mut hp,
        "test.left",
        Rect::new(wider.x, wider.y, wider.width - 30, wider.height),
    )?;
    assert_eq!(rect_of(&hp, "test.right"), Some(narrower));
    Ok(())
}

#[test]
fn windows_nine_pixels_apart_form_no_pair() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.left", (400, 300))?;
    let b = open(&mut hp, "test.right", (400, 300))?;
    // The magnet would hold a window 9 px away at the gap; one flush with
    // the work area's right edge stays there, the nearer pull.
    let right = AREA.right() - b.width;
    drag_to(&mut hp, b, right, 750);
    wait_for_rect(&mut hp, "test.right", Rect::new(right, 750, 400, 300))?;
    // The other's right edge, 9 px short of the first's left once it is
    // up beside it.
    let at = (f64::from(a.right() + 3), f64::from(a.y + 100));
    drag_by(&mut hp, at, f64::from(right - 9 - a.right()));
    let a = Rect::new(a.x, a.y, right - 9 - a.x, a.height);
    wait_for_rect(&mut hp, "test.left", a)?;
    drag_to(&mut hp, Rect::new(right, 750, 400, 300), right, 450);
    wait_for_rect(&mut hp, "test.right", Rect::new(right, 450, 400, 300))?;
    hp.pointer_move(f64::from(a.right()) + 4.5, 550.0);
    assert!(hp.state().seam_handle.is_none(), "no pair, no handle");
    Ok(())
}

/// Two windows snapped left and right.
fn snapped_pair(hp: &mut Headless) -> anyhow::Result<()> {
    open(hp, "test.left", (400, 300))?;
    chord(hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(hp, "test.left", Zone::Left.rect(AREA))?;
    open(hp, "test.right", (400, 300))?;
    chord(hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait_for_rect(hp, "test.right", Zone::Right.rect(AREA))?;
    Ok(())
}

/// The middle of the gap between the left and right halves.
fn seam_centre() -> (f64, f64) {
    let left = Zone::Left.rect(AREA);
    (
        f64::from(left.right() + GAP / 2),
        f64::from(left.y) + f64::from(left.height) / 2.0,
    )
}

#[test]
fn a_floating_window_over_the_seam_hides_its_handle_and_takes_the_press() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    snapped_pair(&mut hp)?;
    // Opened in the middle, over the seam's.
    let over = open(&mut hp, "test.over", (400, 300))?;
    let (x, y) = seam_centre();
    assert!(over.contains(x, y), "{over:?} covers the handle's place");
    // Near the seam above it: the handle shows, under the window.
    hp.pointer_move(x, f64::from(over.y - 100));
    assert!(hp.state().seam_handle.is_some(), "the handle shows");
    wait(&mut hp, Duration::from_millis(200))?;
    let shot = hp.screenshot(0)?;
    assert_eq!(
        pixel(&shot, x, y),
        Some(WINDOW_COLOUR),
        "the window, not the handle"
    );
    // A press there is the window's.
    hp.pointer_move(x, y);
    assert_eq!(hp.state().frame_cursor, None, "the client's cursor");
    drag_by(&mut hp, (x, y), 60.0);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(rect_of(&hp, "test.left"), Some(Zone::Left.rect(AREA)));
    assert_eq!(rect_of(&hp, "test.right"), Some(Zone::Right.rect(AREA)));
    assert_eq!(rect_of(&hp, "test.over"), Some(over));
    Ok(())
}

#[test]
fn a_window_behind_the_pair_leaves_its_handle() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // In the middle, then the pair over it: it shows through the gap.
    let behind = open(&mut hp, "test.behind", (400, 300))?;
    snapped_pair(&mut hp)?;
    let (x, y) = seam_centre();
    assert!(behind.contains(x, y));
    hp.pointer_move(x, y);
    assert!(hp.state().seam_handle.is_some(), "the handle shows");
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::EwResize));
    drag_by(&mut hp, (x, y), 100.0);
    let left = Zone::Left.rect(AREA);
    wait_for_rect(
        &mut hp,
        "test.left",
        Rect::new(left.x, left.y, left.width + 100, left.height),
    )?;
    assert_eq!(rect_of(&hp, "test.behind"), Some(behind));
    Ok(())
}
