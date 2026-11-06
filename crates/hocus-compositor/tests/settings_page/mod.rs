//! The Settings' page scrolled with the wheel until what a test clicks is
//! in view (#660), for every test file of the Settings that clicks.
//!
//! Where to click is read from places that follow what the window last
//! showed (`shown`, the serial of its last "shows" line), and each wheel
//! is waited for until the window says it has taken it: its places carry
//! how many wheels they follow (`wheels`). Without that, a
//! wheel the window had not read yet (a loaded machine) scrolled the page
//! after the test had read where to click, and the click landed on
//! nothing; a page that "kept still for a moment" said nothing of a wheel
//! still on its way.

// Each test file uses a part of it.
#![allow(dead_code)]

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
