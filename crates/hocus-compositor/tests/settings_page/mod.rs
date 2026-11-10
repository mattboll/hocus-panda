//! The Settings' page scrolled with the wheel until what a test clicks is
//! in view (#660), and where to click read from the window's current
//! places (#684), for every test file of the Settings that clicks.
//!
//! Where to click is read from places that follow what the window last
//! showed (`shown`, the serial of its last "shows" line), and each wheel
//! is waited for until the window says it has taken it: its places carry
//! how many wheels they follow (`wheels`). Without that, a
//! wheel the window had not read yet (a loaded machine) scrolled the page
//! after the test had read where to click, and the click landed on
//! nothing; a page that "kept still for a moment" said nothing of a wheel
//! still on its way.

use std::{path::Path, time::Duration};

use hocus_compositor::{Headless, model::Rect};
use serde_json::Value;

use crate::common::{self, WAIT};

/// How long a wheel may take to be taken: longer than the other waits,
/// for each one scrolls and redraws the whole window, which a window
/// sharing a loaded CPU (one of a dozen) has taken up to 7 s to do.
const WHEEL: Duration = Duration::from_secs(30);

/// How a file's page is scrolled.
pub struct Wheel {
    /// Where the wheel turns, from the window's left edge: over the page.
    pub x: f64,
    /// The notches of each wheel.
    pub notches: i32,
    /// The highest the top of what is clicked may be: below the header.
    pub top: f64,
    /// The lowest its bottom may be.
    pub floor: Floor,
}

/// The lowest a place's bottom may be.
pub enum Floor {
    /// This far above the window's bottom (a toast's room).
    Margin(f64),
    /// Above the toast while there is one (its "Annuler" would take the
    /// click), else this far above the window's bottom.
    AboveToast(f64),
}

/// The last places the window said.
fn said(log: &Path) -> Option<Value> {
    common::last_json(log, " places {")
}

/// How many wheels the window had taken when it said `places`.
fn wheels(places: &Value) -> u64 {
    places["wheels"].as_u64().unwrap_or(0)
}

/// Whether `places` follow the last "shows" line: the window is laid out
/// as it last said, not as it was a moment before (its places are said
/// after what it shows, and once settled).
fn fresh(log: &Path, places: &Value) -> bool {
    let serial = common::last_json(log, " shows {")
        .and_then(|s| s["serial"].as_u64())
        .unwrap_or(0);
    places["shown"].as_u64().unwrap_or(0) >= serial
}

/// The window's places once they hold `id` and follow its last "shows"
/// line: where `id` is in what the window shows now (#684). The last
/// places said, read alone, may be from before its last change (a page
/// gone back to, a list still filling), and a click there lands on what
/// was there.
pub fn places_with(hp: &mut Headless, log: &Path, id: &str) -> anyhow::Result<Value> {
    // What held, not what is read again after.
    let mut seen = None;
    hp.dispatch_until(WAIT, |_| {
        seen = said(log).filter(|p| p.get(id).is_some() && fresh(log, p));
        seen.is_some()
    })?;
    seen.ok_or_else(|| {
        anyhow::anyhow!(
            "{id} is nowhere in places as recent as what the window shows: {:?}",
            said(log)
        )
    })
}

/// `id` scrolled into the window (`app_id`'s, whose log is `log`), by
/// wheels over its page, each taken by the window before the next. At the
/// page's end, where a wheel moves nothing, as far as it goes.
pub fn reveal(
    hp: &mut Headless,
    log: &Path,
    app_id: &str,
    id: &str,
    wheel: &Wheel,
) -> anyhow::Result<()> {
    for _ in 0..60 {
        let mut places = None;
        hp.dispatch_until(WAIT, |_| {
            places = said(log).filter(|p| p.get(id).is_some() && fresh(log, p));
            places.is_some()
        })?;
        let places = places.ok_or_else(|| anyhow::anyhow!("{id} is nowhere: {:?}", said(log)))?;
        let n = |i: usize| places[id][i].as_f64().unwrap_or_default();
        let (top, bottom) = (n(1), n(1) + n(3));
        let window = window(hp, app_id)?;
        let height = f64::from(window.height);
        let floor = match wheel.floor {
            Floor::Margin(margin) => height - margin,
            Floor::AboveToast(margin) => places["toast"][1]
                .as_f64()
                .map_or(height - margin, |toast| toast - 8.0),
        };
        let notches = if bottom > floor {
            wheel.notches
        } else if top < wheel.top {
            -wheel.notches
        } else {
            return Ok(());
        };
        let taken = wheels(&places);
        let over = (
            f64::from(window.x) + wheel.x,
            f64::from(window.y) + height / 2.0,
        );
        hp.pointer_move(over.0 - 1.0, over.1);
        hp.pointer_move(over.0, over.1);
        hp.wheel(notches);
        let mut after = None;
        hp.dispatch_until(WHEEL, |_| {
            after = said(log).filter(|p| wheels(p) > taken && fresh(log, p));
            after.is_some()
        })?;
        let after = after
            .ok_or_else(|| anyhow::anyhow!("the window never took the wheel: {:?}", said(log)))?;
        // Taken, and nothing moved: the page's end.
        if after.get(id) == places.get(id) {
            return Ok(());
        }
    }
    anyhow::bail!("{id} never came into view")
}

fn window(hp: &Headless, app_id: &str) -> anyhow::Result<Rect> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.rect)
        .ok_or_else(|| anyhow::anyhow!("no settings window"))
}

/// The window's pixels at the screen's `scale`, `window` cut out of
/// screen `screen`.
pub fn window_pixels(
    hp: &mut Headless,
    screen: usize,
    scale: usize,
    window: Rect,
) -> anyhow::Result<Vec<u8>> {
    let (w, _, px) = hp.screenshot(screen)?;
    let at = |n: i32| usize::try_from(n).unwrap_or_default() * scale;
    let (x, y, cw, ch) = (
        at(window.x),
        at(window.y),
        at(window.width),
        at(window.height),
    );
    let mut out = Vec::with_capacity(cw * ch * 4);
    for row in y..y + ch {
        let start = (row * w as usize + x) * 4;
        out.extend_from_slice(&px[start..start + cw * 4]);
    }
    Ok(out)
}

/// How many pixels of `a` and `b` differ.
pub fn differ(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count()
        + a.len().abs_diff(b.len()) / 4
}

/// Waits until no window has committed for 500 ms (a caret blinks: 3 s at
/// most).
pub fn still(hp: &mut Headless) -> anyhow::Result<()> {
    let mut last = hp.window_commits();
    let mut still = 0;
    for _ in 0..60 {
        common::wait(hp, Duration::from_millis(100))?;
        let now = hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 5 {
            return Ok(());
        }
    }
    anyhow::bail!("the window never kept still")
}

/// The page of the window `app_id` (whose log is `log`) scrolled down
/// twice and back to its top by the wheel over it, each wheel taken and
/// drawn before the next; then the pointer out of the window.
pub fn scroll_round_trip(hp: &mut Headless, log: &Path, app_id: &str) -> anyhow::Result<()> {
    let window = window(hp, app_id)?;
    let over = (
        f64::from(window.x) + 600.0,
        f64::from(window.y) + f64::from(window.height) / 2.0,
    );
    let before = said(log).map_or(0, |p| wheels(&p));
    for (taken, notches) in (before + 1..).zip([3, 3, -40]) {
        hp.pointer_move(over.0 - 1.0, over.1);
        hp.pointer_move(over.0, over.1);
        hp.wheel(notches);
        let took = hp.dispatch_until(WHEEL, |_| said(log).is_some_and(|p| wheels(&p) >= taken))?;
        anyhow::ensure!(took, "the window never took the wheel: {:?}", said(log));
        still(hp)?;
    }
    hp.pointer_move(5.0, 5.0);
    Ok(())
}
