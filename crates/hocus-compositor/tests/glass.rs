//! The bar's glass, headless on GLES (docs/design/bar.md, 3.1), on the GPU
//! `HOCUS_TEST_GPU` names or else on Mesa's software device: what shows
//! through it is what is under it, in the same frame, while a window is
//! dragged under the bar (issue #517). A clear surface in the bar's
//! namespace and place stands for the shell's, so that the screen there
//! shows the glass alone.

use crate::common;

use std::time::Duration;

use common::{BTN_LEFT, KEY_LEFTALT, Shows, WAIT, client, open, wait};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, model::Rect};

/// One pixel of a screenshot (B, G, R, A in memory), as R, G, B.
fn pixel(shot: &(i32, i32, Vec<u8>), x: i32, y: i32) -> [i32; 3] {
    let i = ((y * shot.0 + x) * 4) as usize;
    let p = &shot.2[i..i + 4];
    [i32::from(p[2]), i32::from(p[1]), i32::from(p[0])]
}

/// How far a point is outside `rect`: negative inside, by how far from
/// its nearest edge.
fn outside(rect: Rect, x: i32, y: i32) -> i32 {
    let dx = (rect.x - x).max(x - rect.right());
    let dy = (rect.y - y).max(y - rect.bottom());
    if dx <= 0 && dy <= 0 {
        dx.max(dy)
    } else {
        dx.max(0).max(dy.max(0))
    }
}

/// A window nearly as wide as the screen, dragged step by step from the
/// left edge until its right edge is at the middle of the bar, wobbling:
/// at every step the glass shows it where it is under the bar, and
/// nowhere else. It once showed nothing of it under the bar, and some of
/// it where it was not yet: the glass drew the bent window where it is on
/// the screen, not where the glass's pane is. At scale 1 and 2.
#[test]
fn the_glass_shows_a_dragged_window_where_it_is() -> anyhow::Result<()> {
    let mut config = Config::default();
    // The form the clear surface stands for, at both scales.
    config.panel.form = FormChoice::Full;
    for scale in [1, 2] {
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec![format!("2560x1600@{scale}").parse()?],
                ..common::gles()
            },
            config.clone(),
        )?;
        if hp.renderer() != "gles" {
            eprintln!("no GLES here: no glass to see");
            return Ok(());
        }
        std::mem::forget(client(hp.socket(), Some(Shows::ClearBar))?);
        let barred = hp.dispatch_until(WAIT, |hp| {
            hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
        })?;
        anyhow::ensure!(barred, "no bar surface");
        let bar = hp.bar_surfaces()[0].1;
        let width = 2560 / scale - 360;
        let start = open(&mut hp, "test.glass", (width as u32, 400))?;
        wait(&mut hp, Duration::from_millis(400))?;
        // The glass with nothing but the background under it.
        let empty = hp.screenshot(0)?;
        // Held by its middle: to the left edge, then right in steps.
        let held = (width / 2, 200);
        let (y, mut x) = (start.y, start.x);
        hp.pointer_move(f64::from(x + held.0), f64::from(y + held.1));
        hp.key(KEY_LEFTALT, true);
        hp.button(BTN_LEFT, true);
        let first = 40;
        while x > first {
            x = (x - 40).max(first);
            hp.pointer_move(f64::from(x + held.0), f64::from(y + held.1));
            wait(&mut hp, Duration::from_millis(16))?;
        }
        let last = bar.x + bar.width / 2 - width;
        let mut steps: Vec<i32> = (first..last).step_by(20).collect();
        steps.push(last);
        let mut wrong = Vec::new();
        for (n, at) in steps.iter().enumerate() {
            hp.pointer_move(f64::from(at + held.0), f64::from(y + held.1));
            // Held still at the end: the jelly settles, and the window is
            // drawn bent all the same while it is held.
            let still = if n + 1 == steps.len() { 1000 } else { 30 };
            wait(&mut hp, Duration::from_millis(still))?;
            let shot = hp.screenshot(0)?;
            let window = hp
                .drawn_rect("test.glass")
                .unwrap_or(Rect::new(*at, y, width, 400));
            // How far the bent window lags behind or runs ahead of it.
            let bend = hp.bend().unwrap_or(0.0).ceil() as i32;
            let row = window.y + window.height / 2;
            for px in (bar.x + 4..bar.right() - 4).step_by(8) {
                let d = outside(window, px, row);
                let got = pixel(&shot, px * scale, row * scale);
                let bare = pixel(&empty, px * scale, row * scale);
                // Inside the window past the blur's reach (52 px): its
                // blue. Outside past its shadow too: the background, as
                // before it came.
                if d < -60 - bend {
                    if got[2] < 180 || got[2] - bare[2] < 60 {
                        wrong.push(format!(
                            "x{scale}, step {n}, window at {}: at {px},{row}, {} px inside \
                             it, the glass shows {got:?}, the background is {bare:?}",
                            window.x, -d
                        ));
                    }
                } else if d > 200 + bend && got.iter().zip(bare).any(|(a, b)| (a - b).abs() > 8) {
                    wrong.push(format!(
                        "x{scale}, step {n}, window at {}: at {px},{row}, {d} px from it, \
                         the glass shows {got:?}, not the background's {bare:?}",
                        window.x
                    ));
                }
            }
        }
        hp.button(BTN_LEFT, false);
        hp.key(KEY_LEFTALT, false);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
    Ok(())
}
