//! A window's frame and what it shows (docs/design/windows.md §3): a menu
//! the window opens is in front of all of the window, its rims, rings and
//! shadow included, where the menu reaches past the window's edge.

use crate::common;

use common::{Command, Event, WAIT, WINDOW_COLOUR, compositor, open_with, wait_for_rect};
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
