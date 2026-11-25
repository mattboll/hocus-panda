//! A window's frame and what it shows (docs/design/windows.md §3): a menu
//! the window opens is in front of all of the window, its rims, rings and
//! shadow included, where the menu reaches past the window's edge; a
//! snapped window's corners stay round whatever its client draws around
//! it, and so do a window's whose client draws its own shadow (#958).

use crate::common;

use common::{
    BTN_LEFT, Command, Event, KEY_LEFT, KEY_LEFTMETA, Shows, WAIT, WINDOW_COLOUR, chord,
    compositor, open_showing, open_with, wait_for_rect,
};
use hocus_compositor::Headless;
use hocus_compositor::model::Rect;
use std::time::{Duration, Instant};

/// The screen's pixel at `(x, y)`, as ARGB.
fn pixel(px: &[u8], w: i32, (x, y): (i32, i32)) -> u32 {
    #[allow(clippy::cast_sign_loss)]
    let i = ((y * w + x) * 4) as usize;
    u32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]])
}

/// A menu reaching past its window's right edge, as a browser's hidden
/// bookmarks' menu does: the window's rim does not cut across it.
#[test]
fn a_menu_past_its_windows_edge_is_over_the_windows_rim() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (_, window) = open_with(&mut hp, "test.menu", (600, 400), false)?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.menu"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    let rect = Rect::new(300, 200, 600, 400);
    hp.place_window(id, rect);
    wait_for_rect(&mut hp, "test.menu", rect)?;
    // 300 wide from 500 in: 200 past the window's right edge.
    window.commands.send(Command::Popup {
        anchor: (500, 100, 10, 10),
        size: (300, 150),
    })?;
    let mut at = None;
    hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = window.events.try_recv() {
            if let Event::PopupAt(x, y) = event {
                at = Some((x, y));
            }
        }
        at.is_some()
    })?;
    assert_eq!(at, Some((500, 110)), "under its anchor, past the edge");
    let edge = rect.x + rect.width;
    let menu_y = rect.y + 110 + 75;
    // The menu drawn, once its client has committed its first buffer.
    let until = Instant::now() + WAIT;
    loop {
        hp.dispatch(Duration::from_millis(10))?;
        let (w, _, px) = hp.screenshot(0)?;
        if pixel(&px, w, (edge + 100, menu_y)) == WINDOW_COLOUR {
            break;
        }
        anyhow::ensure!(Instant::now() < until, "the menu never showed");
    }
    common::at_rest(&mut hp)?;
    let (w, _, px) = hp.screenshot(0)?;
    // Across the window's right edge, where its rim is: the menu's own
    // colour (the test windows' and menus' blue) all the way.
    let across = |y: i32| (edge - 3..=edge + 3).map(move |x| (x, y));
    let cut: Vec<_> = across(menu_y)
        .filter(|&at| pixel(&px, w, at) != WINDOW_COLOUR)
        .map(|at| (at, format!("{:08x}", pixel(&px, w, at))))
        .collect();
    assert!(
        cut.is_empty(),
        "the menu is cut across where the window's rim runs: {cut:?}"
    );
    // Above the menu, the rim is there: what the menu covers.
    assert!(
        across(rect.y + 50).any(|at| pixel(&px, w, at) != WINDOW_COLOUR),
        "the window's frame shows past its edge above the menu"
    );
    Ok(())
}

/// A snapped window is told it is tiled: GTK then draws its edges square
/// and a dark 1 px line around its geometry, square at the corners
/// (Firefox's black top corners). What a tiled client draws outside its
/// geometry is not shown: its corners are the frame's, round, and around
/// it is what is around a window that draws nothing there.
#[test]
fn a_snapped_window_shows_nothing_its_client_draws_around_it() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // The cursor away from where the window goes.
    hp.pointer_move(1500.0, 900.0);
    let area = hp
        .work_area()
        .ok_or_else(|| anyhow::anyhow!("no work area"))?;
    let half = Rect::new(area.x, area.y, (area.width - 8) / 2, area.height);
    // What a window drawing nothing around it shows there, snapped.
    let (_, plain) = open_with(&mut hp, "test.plain", (600, 400), false)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.plain", half)?;
    common::at_rest(&mut hp)?;
    let (w, _, before) = hp.screenshot(0)?;
    plain.commands.send(Command::Close)?;
    let gone = hp.dispatch_until(WAIT, |hp| id_of(hp, "test.plain").is_err())?;
    anyhow::ensure!(gone, "the plain window never closed");
    // A client drawing a dark band 30 px wide around its geometry.
    let (_, _window) = open_showing(
        &mut hp,
        "test.square",
        Shows::Shadowed {
            app_id: "test.square",
            size: (600, 400),
            margin: 30,
        },
    )?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut hp, "test.square", half)?;
    common::at_rest(&mut hp)?;
    let (_, _, after) = hp.screenshot(0)?;
    // Outside the window, past its rim: its top-left corner, diagonally,
    // and beside its left edge. As around the plain window.
    let (x, y) = (half.x, half.y);
    for at in [(x - 3, y - 3), (x - 2, y - 2), (x - 4, y + half.height / 2)] {
        assert_eq!(
            format!("{:08x}", pixel(&after, w, at)),
            format!("{:08x}", pixel(&before, w, at)),
            "at {at:?}, outside the snapped window: what its client draws around it shows"
        );
    }
    Ok(())
}

/// The id of the window of `app_id`.
fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<hocus_compositor::model::WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))
}

/// Where a window goes, over the pale page of a document behind it.
const OVER_PAGE: Rect = Rect::new(300, 200, 600, 400);

/// The pixels around `rect`'s bottom corners, its shadow's reach below
/// and beside them, and their corners' edge pixels.
fn bottom_corners(px: &[u8], w: i32, rect: Rect) -> (Vec<u32>, [u32; 2]) {
    let bottom = rect.y + rect.height;
    let mut around = Vec::new();
    for x0 in [rect.x - 60, rect.right() - 30] {
        for y in bottom - 30..bottom + 60 {
            around.extend((x0..x0 + 90).map(|x| pixel(px, w, (x, y))));
        }
    }
    let edges = [
        pixel(px, w, (rect.x, bottom - 1)),
        pixel(px, w, (rect.right() - 1, bottom - 1)),
    ];
    (around, edges)
}

/// A client drawing its own shadow around square corners, as Chrome
/// does, over a pale window (#958): its shadow is cropped and the
/// desktop's drawn, its corners round, the same pixels as a window that
/// draws none; after it draws all of its window again, and after it
/// draws its lower part alone. The band outside its edges resizes it.
#[test]
fn a_window_drawing_its_own_shadow_has_the_desktops_and_round_corners() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    hp.pointer_move(1500.0, 900.0);
    open_showing(
        &mut hp,
        "test.page",
        Shows::Document {
            app_id: "test.page",
            size: (1100, 750),
        },
    )?;
    let page = Rect::new(100, 100, 1100, 750);
    hp.place_window(id_of(&hp, "test.page")?, page);
    wait_for_rect(&mut hp, "test.page", page)?;
    // What every other window looks like there: one drawing no shadow.
    let (_, plain) = open_with(&mut hp, "test.plain", (600, 400), false)?;
    hp.place_window(id_of(&hp, "test.plain")?, OVER_PAGE);
    wait_for_rect(&mut hp, "test.plain", OVER_PAGE)?;
    plain.commands.send(Command::Opaque)?;
    common::at_rest(&mut hp)?;
    let (w, _, px) = hp.screenshot(0)?;
    let (expected, edges) = bottom_corners(&px, w, OVER_PAGE);
    for edge in edges {
        assert_ne!(
            edge, WINDOW_COLOUR,
            "a plain window's bottom corner is square"
        );
    }
    plain.commands.send(Command::Close)?;
    let gone = hp.dispatch_until(WAIT, |hp| id_of(hp, "test.plain").is_err())?;
    anyhow::ensure!(gone, "the plain window never closed");
    // A client drawing a shadow 30 px wide around its geometry, its
    // window opaque, as Chrome's.
    let margin = 30;
    let (_, csd) = open_showing(
        &mut hp,
        "test.csd",
        Shows::Shadowed {
            app_id: "test.csd",
            size: (600, 400),
            margin,
        },
    )?;
    hp.place_window(id_of(&hp, "test.csd")?, OVER_PAGE);
    wait_for_rect(&mut hp, "test.csd", OVER_PAGE)?;
    csd.commands.send(Command::Opaque)?;
    common::at_rest(&mut hp)?;
    let check = |hp: &mut Headless, when: &str| -> anyhow::Result<()> {
        let (w, _, px) = hp.screenshot(0)?;
        let (around, edges) = bottom_corners(&px, w, OVER_PAGE);
        for edge in edges {
            assert_ne!(
                format!("{edge:08x}"),
                format!("{WINDOW_COLOUR:08x}"),
                "{when}: a bottom corner's edge pixel is the window's, square"
            );
        }
        let differ = around.iter().zip(&expected).filter(|(a, b)| a != b).count();
        assert_eq!(
            differ, 0,
            "{when}: {differ} pixels around the bottom corners are not a plain window's \
             (its client's shadow shows, not the desktop's)"
        );
        Ok(())
    };
    check(&mut hp, "drawn whole")?;
    // Its lower part alone drawn again: its bottom corners and the
    // shadow under them, in its buffer.
    let (m, bw) = (margin as i32, 600 + 2 * margin as i32);
    csd.commands
        .send(Command::Redraw(0, m + 400 - 60, bw, 60 + m))?;
    // Its commit in, then drawn.
    common::wait(&mut hp, Duration::from_millis(200))?;
    common::at_rest(&mut hp)?;
    check(&mut hp, "its lower part drawn again")?;
    // Its client's shadow, no longer drawn, still takes the pointer: the
    // band outside its edges resizes it all the same.
    let start = OVER_PAGE;
    let (x, y) = (f64::from(start.right() + 3), f64::from(start.bottom() + 3));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 50.0, y + 40.0);
    hp.button(BTN_LEFT, false);
    wait_for_rect(
        &mut hp,
        "test.csd",
        Rect::new(start.x, start.y, start.width + 50, start.height + 40),
    )?;
    Ok(())
}
