//! Dropping a window on a desktop's tile in the bar, headless (docs/design/
//! windows.md §12, bar.md §4.4; 8.5 B, issue #490): while a window is
//! moved over the tiles the bar comes in front at once, the tile is the
//! one told, the window shrinks to 30 %; let go on a tile it goes to that
//! desktop of that tile's screen at its place before the drag, the
//! screens keeping the desktops they show; on its own desktop's tile it
//! goes back; `Esc` puts it back too.
//!
//! A plain layer surface of the test's stands for the shell's desktops'
//! surface, and the tiles are said as the shell says them.

mod common;

use std::time::Duration;

use common::{
    BTN_LEFT, KEY_ESC, KEY_LEFTALT, Shows, WAIT, client, open, rect_of, two_screens, wait,
    wait_for_rect,
};
use hocus_compositor::{
    Headless,
    model::{self, Rect},
};
use hocus_link::DeskTile;

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";

/// The tiles' surface: 194 × 70, 600 px from the top, 8 from the right.
const SURFACE: (u32, u32) = (194, 70);

/// The two screens, the desktops' surface on the first's bar, its tiles
/// said: the first screen's four 30 px high, the second's 20 px under
/// them. Where the surface is.
fn start() -> anyhow::Result<(Headless, Rect)> {
    let mut hp = two_screens()?;
    std::mem::forget(client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: hocus_link::DESKTOPS_NAMESPACE,
            size: SURFACE,
            margin: (600, 8),
            colour: 0xff30_3040,
        }),
    )?);
    let placed = hp.dispatch_until(WAIT, |hp| {
        !hp.layer_rects(hocus_link::DESKTOPS_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(placed, "no desktops' surface");
    let surface = hp.layer_rects(hocus_link::DESKTOPS_NAMESPACE)[0].1;
    let mut tiles = Vec::new();
    for (output, y, height) in [(FIRST, 5, 30), (SECOND, 45, 20)] {
        for desktop in 0..4u8 {
            tiles.push(DeskTile {
                output: output.into(),
                desktop,
                rect: [11 + i32::from(desktop) * 45, y, 39, height],
            });
        }
    }
    hp.desk_tiles(FIRST, tiles);
    Ok((hp, surface))
}

/// The middle of `output`'s tile `desktop`, global.
fn tile(surface: Rect, output: &str, desktop: i32) -> (f64, f64) {
    let (y, height) = if output == FIRST { (5, 30) } else { (45, 20) };
    (
        f64::from(surface.x + 11 + desktop * 45) + 19.5,
        f64::from(surface.y + y) + f64::from(height) / 2.0,
    )
}

/// A window's desktop (from 0) and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(usize, bool)> {
    hp.all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some(app_id))
        .map(|w| (w.2, w.3))
}

/// The desktop each screen shows.
fn shown(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// Presses `Alt` and the button on `window`'s middle and moves to `to`
/// in steps, holding them.
fn drag(hp: &mut Headless, window: Rect, to: (f64, f64)) -> anyhow::Result<()> {
    let (x, y) = window.centre();
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    for step in 1..=12 {
        let t = f64::from(step) / 12.0;
        hp.pointer_move(x + (to.0 - x) * t, y + (to.1 - y) * t);
    }
    wait(hp, Duration::from_millis(150))
}

fn release(hp: &mut Headless) -> anyhow::Result<()> {
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    wait(hp, Duration::from_millis(100))
}

#[test]
fn let_go_on_the_second_tile_the_window_goes_to_desktop_2_at_its_place() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.dropped", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 1))?;
    // Over the tile: told, the bar in front, the window small.
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), FIRST.to_owned(), 1))
    );
    assert_eq!(hp.bar_in_front().as_deref(), Some(FIRST));
    assert!(
        (hp.moved_scale() - 0.3).abs() < 1e-9,
        "{}",
        hp.moved_scale()
    );
    release(&mut hp)?;
    // On desktop 2, out of sight, where it was before the drag; the
    // screens show what they showed; the bar back under the windows.
    assert_eq!(where_is(&hp, "test.dropped"), Some((1, false)));
    assert_eq!(shown(&hp), [0, 0]);
    assert_eq!(hp.drop_over(), None);
    assert_eq!(hp.bar_in_front(), None);
    common::chord(&mut hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    wait_for_rect(&mut hp, "test.dropped", window)?;
    Ok(())
}

#[test]
fn let_go_on_its_own_desktops_tile_it_goes_back() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.back", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 0))?;
    release(&mut hp)?;
    wait_for_rect(&mut hp, "test.back", window)?;
    assert_eq!(where_is(&hp, "test.back"), Some((0, true)));
    Ok(())
}

#[test]
fn let_go_on_the_other_screens_tile_it_goes_to_that_screen() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    let areas = hp.work_areas();
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.across", (400, 300))?;
    // The second screen's first desktop, the one it shows: there, shown.
    drag(&mut hp, window, tile(surface, SECOND, 0))?;
    assert_eq!(
        hp.drop_over(),
        Some((FIRST.to_owned(), SECOND.to_owned(), 0))
    );
    release(&mut hp)?;
    let there = model::carried(window, areas[0], areas[1]);
    wait_for_rect(&mut hp, "test.across", there)?;
    assert_eq!(where_is(&hp, "test.across"), Some((0, true)));
    // Its third desktop: out of sight there.
    drag(&mut hp, there, tile(surface, SECOND, 2))?;
    release(&mut hp)?;
    assert_eq!(where_is(&hp, "test.across"), Some((2, false)));
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

#[test]
fn leaving_the_tiles_gives_back_its_size_and_esc_puts_it_back() -> anyhow::Result<()> {
    let (mut hp, surface) = start()?;
    hp.pointer_move(960.0, 540.0);
    let window = open(&mut hp, "test.esc", (400, 300))?;
    drag(&mut hp, window, tile(surface, FIRST, 2))?;
    assert!(hp.moved_scale() < 1.0);
    // Off the tiles, back in the screen's middle: full size, nothing told,
    // the bar still in front until the move ends.
    hp.pointer_move(700.0, 400.0);
    wait(&mut hp, Duration::from_millis(150))?;
    assert_eq!(hp.drop_over(), None);
    assert_eq!(hp.moved_scale(), 1.0);
    assert_eq!(hp.bar_in_front().as_deref(), Some(FIRST));
    assert_ne!(rect_of(&hp, "test.esc"), Some(window));
    // Esc: back where the drag began, the move over.
    hp.key(KEY_ESC, true);
    hp.key(KEY_ESC, false);
    wait_for_rect(&mut hp, "test.esc", window)?;
    assert_eq!(hp.bar_in_front(), None);
    release(&mut hp)?;
    assert_eq!(rect_of(&hp, "test.esc"), Some(window));
    assert_eq!(where_is(&hp, "test.esc"), Some((0, true)));
    Ok(())
}
