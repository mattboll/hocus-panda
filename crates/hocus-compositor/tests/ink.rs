//! Drawing on the screen (issue #564, docs/design/ink.md), headless: one
//! test per line of the approved spec's "Measured headless". Pixman, a
//! flat background, the motions' clock held where a time is measured (the
//! 3 s hold, the 600 ms fade, the laser's 450 ms trail). Pixels are read
//! from what the screen shows, and from a capture's PNG; a share's frames
//! are read in `sharing.rs`, which has the share's PipeWire.

use crate::common;

use std::{path::PathBuf, sync::mpsc, time::Duration};

use anyhow::Context;
use common::{
    BTN_LEFT, ClientHandle, Event, KEY_ESC, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT, Pointed,
    Shows, WAIT, WINDOW_COLOUR, chord, open, open_showing, rect_of, wait, wait_for_rect,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};

/// Print (`KEY_SYSRQ`).
const KEY_PRINT: u32 = 99;

/// A compositor on one 1920 × 1080 screen, its pictures' folder the
/// test's.
struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
}

impl Bed {
    fn start() -> anyhow::Result<Self> {
        Self::start_on(true)
    }

    fn start_on(plain: bool) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = plain;
        config.pictures = Some(dir.path().join("Pictures"));
        config.state_dir = Some(dir.path().join("state"));
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec![OutputSpec::new(1920, 1080)],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
        anyhow::ensure!(drawn, "nothing drawn");
        Ok(Self { hp, _dir: dir })
    }

    /// The pixel at (`x`, `y`) of the screen: red, green, blue.
    fn shown(&mut self, (x, y): (f64, f64)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y as i32 * width + x as i32)? * 4;
        let p = bytes.get(at..at + 4).context("off the screen")?;
        Ok([p[2], p[1], p[0]])
    }

    /// A drag of the left button along `path` with `keys` held, in steps
    /// of at most 4 px; the pointer then goes out of the way (the
    /// compositor draws the cursor).
    fn draw(&mut self, keys: &[u32], path: &[(f64, f64)]) {
        for key in keys {
            self.hp.key(*key, true);
        }
        self.hp.pointer_move(path[0].0, path[0].1);
        self.hp.button(BTN_LEFT, true);
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = ((b.0 - a.0).hypot(b.1 - a.1) / 4.0).ceil().max(1.0);
            for i in 1..=steps as i32 {
                let t = f64::from(i) / steps;
                self.hp
                    .pointer_move(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
        self.hp.button(BTN_LEFT, false);
        for key in keys.iter().rev() {
            self.hp.key(*key, false);
        }
        self.hp.pointer_move(1880.0, 1060.0);
    }

    /// The `n`th capture made, read back.
    fn taken(&mut self, n: u64) -> anyhow::Result<image::RgbaImage> {
        let done = self.hp.dispatch_until(WAIT, |hp| hp.captures().0 >= n)?;
        anyhow::ensure!(done, "no capture {n} made");
        let file = self.hp.captures().1.context("no file written")?;
        Ok(image::open(&file)?.to_rgba8())
    }
}

/// The ink's red, `#F2545B`, or near it.
fn red([r, g, b]: [u8; 3]) -> bool {
    r > 200 && g < 140 && b < 150 && r > g + 80
}

/// The background with nothing on it, or the test window's blue.
fn clear(p: [u8; 3], under: [u8; 3]) -> bool {
    p.iter().zip(under).all(|(a, b)| a.abs_diff(b) <= 2)
}

fn window_blue() -> [u8; 3] {
    [
        (WINDOW_COLOUR >> 16) as u8,
        (WINDOW_COLOUR >> 8) as u8,
        WINDOW_COLOUR as u8,
    ]
}

/// The point `t` of the way from `a` to `b`.
fn along(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// What a pointing client's pointer got since the last call, until `last`
/// came (in order: everything before it has come too).
fn pointed_until(
    hp: &mut Headless,
    client: &ClientHandle,
    last: Pointed,
) -> anyhow::Result<Vec<Pointed>> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = client.events.try_recv() {
            if let Event::Pointer(p) = event {
                seen.push(p);
            }
        }
        seen.last() == Some(&last)
    })?;
    anyhow::ensure!(came, "{last:?} never came: {seen:?}");
    Ok(seen)
}

/// The keys a typing client got, until `code`'s press came.
fn keys_until(
    hp: &mut Headless,
    events: &mpsc::Receiver<Event>,
    code: u32,
) -> anyhow::Result<Vec<(u32, bool)>> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = events.try_recv() {
            if let Event::Key(k, pressed) = event {
                seen.push((k, pressed));
            }
        }
        seen.contains(&(code, true))
    })?;
    anyhow::ensure!(came, "key {code} never came: {seen:?}");
    Ok(seen)
}

fn pointing(bed: &mut Bed) -> anyhow::Result<(Rect, ClientHandle)> {
    let app_id = "test.pointing";
    open_showing(
        &mut bed.hp,
        app_id,
        Shows::Pointing {
            app_id,
            size: (1000, 700),
        },
    )
}

#[test]
fn super_drag_draws_red_along_the_path_and_the_window_under_gets_no_event() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (rect, client) = pointing(&mut bed)?;
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    // The pointer in the window, as a user's before drawing.
    common::point(&mut bed.hp, (x + 50.0, y + 50.0))?;
    pointed_until(&mut bed.hp, &client, Pointed::Motion)?;
    let from = (x + 100.0, y + 100.0);
    let corner = (x + 500.0, y + 300.0);
    let to = (x + 800.0, y + 300.0);
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.pointer_move(from.0, from.1);
    pointed_until(&mut bed.hp, &client, Pointed::Motion)?;
    bed.hp.button(BTN_LEFT, true);
    // Grabbed by the compositor, a crosshair showing.
    assert!(bed.hp.pointer_grabbed());
    assert_eq!(bed.hp.frame_cursor().as_deref(), Some("crosshair"));
    for (a, b) in [(from, corner), (corner, to)] {
        for i in 1..=50 {
            let p = along(a, b, f64::from(i) / 50.0);
            bed.hp.pointer_move(p.0, p.1);
        }
    }
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    assert_eq!(bed.hp.frame_cursor(), None);
    // The pointer on in the window: what it got since the press.
    bed.hp.pointer_move(x + 60.0, y + 600.0);
    bed.hp.pointer_move(x + 61.0, y + 600.0);
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Motion)?;
    assert!(
        !seen.contains(&Pointed::Press) && !seen.contains(&Pointed::Release),
        "{seen:?}"
    );
    let left = seen.iter().position(|p| *p == Pointed::Leave);
    let back = seen.iter().position(|p| *p == Pointed::Enter);
    let (Some(left), Some(back)) = (left, back) else {
        anyhow::bail!("the pointer never left the window, or never came back: {seen:?}");
    };
    assert!(left < back, "{seen:?}");
    assert_eq!(left, 0, "{seen:?}");
    assert!(!seen[left..back].contains(&Pointed::Motion), "{seen:?}");
    common::at_rest(&mut bed.hp)?;
    assert_eq!(bed.hp.ink(), (true, false));
    for t in [0.2, 0.5, 0.8] {
        let p = along(from, corner, t);
        assert!(red(bed.shown(p)?), "{p:?}: {:?}", bed.shown(p)?);
        let p = along(corner, to, t);
        assert!(red(bed.shown(p)?), "{p:?}: {:?}", bed.shown(p)?);
    }
    // Away from the path, the window as it was.
    let off = (x + 300.0, y + 400.0);
    assert!(
        clear(bed.shown(off)?, window_blue()),
        "{:?}",
        bed.shown(off)?
    );
    Ok(())
}

#[test]
fn super_shift_drag_draws_a_straight_arrow_its_head_at_the_end() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let background = bed.shown((400.0, 300.0))?;
    let from = (400.0, 500.0);
    let detour = (700.0, 300.0);
    let to = (1000.0, 500.0);
    bed.draw(&[KEY_LEFTMETA, KEY_LEFTSHIFT], &[from, detour, to]);
    common::at_rest(&mut bed.hp)?;
    // Straight from the press to the release, not where the pointer went.
    for t in [0.1, 0.4, 0.7, 0.9] {
        let p = along(from, to, t);
        assert!(red(bed.shown(p)?), "{p:?}: {:?}", bed.shown(p)?);
    }
    assert!(clear(bed.shown(detour)?, background));
    assert!(clear(bed.shown(along(from, detour, 0.5))?, background));
    // The head: 22 px back from the tip, 13.2 px either side of the shaft.
    for side in [-1.0, 1.0] {
        let p = (to.0 - 0.7 * 22.0, to.1 + side * 0.7 * 13.2);
        assert!(red(bed.shown(p)?), "{p:?}: {:?}", bed.shown(p)?);
    }
    // Nothing past the tip, nor a head at the start.
    assert!(!red(bed.shown((to.0 + 12.0, to.1))?));
    assert!(!red(bed.shown((from.0 + 15.4, from.1 + 9.2))?));
    Ok(())
}

#[test]
fn a_segment_damages_its_bounds_and_the_fade_the_drawings() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.pointer_move(600.0, 500.0);
    bed.hp.button(BTN_LEFT, true);
    for i in 1..=50 {
        bed.hp.pointer_move(600.0 + f64::from(i) * 4.0, 500.0);
    }
    common::at_rest(&mut bed.hp)?;
    bed.hp.take_damage(0);
    // One more segment of 6 px: its ink and its glow, and the cursor
    // moved with it, no more.
    bed.hp.pointer_move(806.0, 500.0);
    common::at_rest(&mut bed.hp)?;
    let damage = bed.hp.take_damage(0).context("nothing drawn")?;
    assert!(damage.width <= 60 && damage.height <= 60, "{damage:?}");
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    common::at_rest(&mut bed.hp)?;
    bed.hp.take_damage(0);
    // The fade: the drawing's tiles (120 px), not the screen.
    bed.hp.hold_clock();
    bed.hp.step(Duration::from_millis(3300))?;
    let damage = bed.hp.take_damage(0).context("nothing drawn")?;
    assert!(
        damage.width <= 3 * 120 && damage.height <= 2 * 120,
        "{damage:?}"
    );
    assert!(damage.x <= 600 && damage.right() >= 806, "{damage:?}");
    Ok(())
}

#[test]
fn the_strokes_are_still_there_at_2_9_s_and_gone_by_3_6_s() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let p = (900.0, 400.0);
    let background = bed.shown(p)?;
    bed.hp.hold_clock();
    bed.draw(&[KEY_LEFTMETA], &[(800.0, 400.0), (1000.0, 400.0)]);
    bed.hp.step(Duration::from_millis(2900))?;
    assert!(red(bed.shown(p)?), "{:?}", bed.shown(p)?);
    // Fading, ease-in: under way at 3.3 s.
    bed.hp.step(Duration::from_millis(400))?;
    let fading = bed.shown(p)?;
    assert!(!red(fading) && !clear(fading, background), "{fading:?}");
    bed.hp.step(Duration::from_millis(300))?;
    assert!(clear(bed.shown(p)?, background), "{:?}", bed.shown(p)?);
    bed.hp.step(Duration::from_millis(16))?;
    assert_eq!(bed.hp.ink(), (false, false));
    Ok(())
}

#[test]
fn a_second_stroke_before_3_s_keeps_the_first() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let first = (900.0, 300.0);
    let second = (900.0, 600.0);
    let background = bed.shown(first)?;
    bed.hp.hold_clock();
    bed.draw(&[KEY_LEFTMETA], &[(800.0, 300.0), (1000.0, 300.0)]);
    bed.hp.step(Duration::from_secs(2))?;
    bed.draw(&[KEY_LEFTMETA], &[(800.0, 600.0), (1000.0, 600.0)]);
    // 4.9 s after the first's release, 2.9 s after the second's: both.
    bed.hp.step(Duration::from_millis(2900))?;
    assert!(red(bed.shown(first)?), "{:?}", bed.shown(first)?);
    assert!(red(bed.shown(second)?));
    // And both go together.
    bed.hp.step(Duration::from_millis(700))?;
    assert!(clear(bed.shown(first)?, background));
    assert!(clear(bed.shown(second)?, background));
    Ok(())
}

#[test]
fn escape_clears_the_strokes_and_otherwise_goes_to_the_window() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let app_id = "test.typing";
    let (rect, client) = open_showing(
        &mut bed.hp,
        app_id,
        Shows::Typing {
            app_id,
            size: (600, 400),
        },
    )?;
    let focused = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.windows().iter().any(|w| w.keyboard))?;
    anyhow::ensure!(focused, "the window never took the keyboard");
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    let p = (x + 300.0, y + 200.0);
    bed.draw(
        &[KEY_LEFTMETA],
        &[(x + 100.0, y + 200.0), (x + 500.0, y + 200.0)],
    );
    common::at_rest(&mut bed.hp)?;
    assert!(red(bed.shown(p)?));
    chord(&mut bed.hp, &[KEY_ESC]);
    assert_eq!(bed.hp.ink(), (false, false));
    common::at_rest(&mut bed.hp)?;
    assert!(clear(bed.shown(p)?, window_blue()), "{:?}", bed.shown(p)?);
    // No stroke: `Esc` is the window's. The first never reached it.
    chord(&mut bed.hp, &[KEY_ESC]);
    let keys = keys_until(&mut bed.hp, &client.events, KEY_ESC)?;
    let escapes = keys.iter().filter(|k| **k == (KEY_ESC, true)).count();
    assert_eq!(escapes, 1, "{keys:?}");
    Ok(())
}

#[test]
fn super_alt_shows_the_laser_its_trail_gone_in_450_ms_and_a_click_reaches_the_window()
-> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (rect, client) = pointing(&mut bed)?;
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    common::point(&mut bed.hp, (x + 100.0, y + 300.0))?;
    pointed_until(&mut bed.hp, &client, Pointed::Motion)?;
    // The window opened, its motion over.
    common::at_rest(&mut bed.hp)?;
    bed.hp.hold_clock();
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.key(KEY_LEFTALT, true);
    assert_eq!(bed.hp.ink(), (false, true));
    // Across the window, 10 px every 4 ms, a frame at each.
    for i in 1..=40 {
        bed.hp
            .pointer_move(x + 100.0 + f64::from(i) * 10.0, y + 300.0);
        bed.hp.step(Duration::from_millis(4))?;
    }
    let head = (x + 500.0, y + 300.0);
    // A frame of the trail fading redraws the trail, not the screen.
    bed.hp.take_damage(0);
    bed.hp.step(Duration::from_millis(4))?;
    let damage = bed.hp.take_damage(0).context("nothing drawn")?;
    assert!(damage.width <= 500 && damage.height <= 80, "{damage:?}");
    // The dot under the pointer (up and left of it: the cursor is drawn
    // down and right of its tip).
    let under = (head.0 - 3.0, head.1 - 3.0);
    assert!(red(bed.shown(under)?), "{:?}", bed.shown(under)?);
    // Its trail behind it, 40 ms old.
    let behind = (head.0 - 100.0, head.1);
    let trail = bed.shown(behind)?;
    assert!(trail[0] > trail[2] + 60, "{trail:?}");
    // A click meanwhile goes to the window.
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    // Still: 450 ms on, the trail is gone, the dot is still there.
    bed.hp.step(Duration::from_millis(450))?;
    bed.hp.step(Duration::from_millis(16))?;
    assert!(
        clear(bed.shown(behind)?, window_blue()),
        "{:?}",
        bed.shown(behind)?
    );
    assert!(red(bed.shown(under)?));
    // Nothing moves: nothing is drawn.
    common::at_rest(&mut bed.hp)?;
    let frames = bed.hp.frames();
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.frames(), frames, "a frame with the laser still");
    // Either key let go: the laser goes.
    bed.hp.key(KEY_LEFTALT, false);
    assert_eq!(bed.hp.ink(), (false, false));
    bed.hp.step(Duration::from_millis(16))?;
    assert!(
        clear(bed.shown(under)?, window_blue()),
        "{:?}",
        bed.shown(under)?
    );
    bed.hp.key(KEY_LEFTMETA, false);
    Ok(())
}

#[test]
fn the_strokes_are_in_a_screenshot() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    open(&mut bed.hp, "test.shot", (800, 600))?;
    let from = (300.0, 200.0);
    let to = (1500.0, 900.0);
    bed.draw(&[KEY_LEFTMETA], &[from, to]);
    // The screen at once (5.8): the ink in its PNG as on the screen.
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_PRINT]);
    let image = bed.taken(1)?;
    assert_eq!(image.dimensions(), (1920, 1080));
    for t in [0.1, 0.5, 0.9] {
        let (px, py) = along(from, to, t);
        let [r, g, b, _] = image.get_pixel(px as u32, py as u32).0;
        assert!(red([r, g, b]), "{:?}: {:?}", (px, py), [r, g, b]);
    }
    Ok(())
}

/// No red left along `y` from `x0` to `x1`, on the screen.
fn nothing_along(bed: &mut Bed, y: f64, (x0, x1): (f64, f64)) -> anyhow::Result<()> {
    // One frame read, its pixels looked at.
    let (width, _, bytes) = bed.hp.screenshot(0)?;
    let mut x = x0;
    while x <= x1 {
        for dy in [-2.0, 0.0, 2.0] {
            let at = usize::try_from((y + dy) as i32 * width + x as i32)? * 4;
            let q = bytes.get(at..at + 4).context("off the screen")?;
            let p = [q[2], q[1], q[0]];
            anyhow::ensure!(!red(p), "ink left at {:?}: {p:?}", (x, y + dy));
        }
        x += 3.0;
    }
    Ok(())
}

#[test]
fn a_stroke_cleared_by_escape_leaves_no_ink_when_another_is_drawn() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    // Held: a stroke stays its 3 s however slow the machine is, and each
    // frame is waited for, not a time (#589).
    bed.hp.hold_clock();
    // The second stroke in the same tiles as the first, apart from it,
    // drawn before any frame shows the first gone.
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 400.0), (1000.0, 400.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    assert!(red(bed.shown((800.0, 400.0))?));
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 440.0), (1000.0, 440.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    assert!(red(bed.shown((800.0, 440.0))?));
    nothing_along(&mut bed, 400.0, (590.0, 1010.0))?;
    // The next frame too (the pointer moved), and the screen's capture.
    bed.hp.pointer_move(1870.0, 1050.0);
    bed.hp.step(Duration::from_millis(16))?;
    nothing_along(&mut bed, 400.0, (590.0, 1010.0))?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_PRINT]);
    let image = bed.taken(1)?;
    for x in (590..=1010).step_by(3) {
        let [r, g, b, _] = image.get_pixel(x, 400).0;
        assert!(
            !red([r, g, b]),
            "ink left in the capture at {x}: {:?}",
            [r, g, b]
        );
    }
    let [r, g, b, _] = image.get_pixel(800, 440).0;
    assert!(red([r, g, b]));
    // Past the capture's flash (260 ms), the stroke still held.
    bed.hp.step(Duration::from_millis(300))?;
    // And with a frame drawn between `Esc` and the next stroke.
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.hp.step(Duration::from_millis(16))?;
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 400.0), (1000.0, 400.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    nothing_along(&mut bed, 440.0, (590.0, 1010.0))?;
    let p = bed.shown((800.0, 400.0))?;
    assert!(red(p), "{p:?}");
    Ok(())
}

#[test]
fn a_stroke_gone_by_its_fade_leaves_no_ink_when_another_is_drawn() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.hp.hold_clock();
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 400.0), (1000.0, 400.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    assert!(red(bed.shown((800.0, 400.0))?));
    // Through the fade, frame by frame, to its end.
    for _ in 0..(3700 / 50) {
        bed.hp.step(Duration::from_millis(50))?;
    }
    bed.hp.step(Duration::from_millis(16))?;
    assert_eq!(bed.hp.ink(), (false, false));
    nothing_along(&mut bed, 400.0, (590.0, 1010.0))?;
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 440.0), (1000.0, 440.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    assert!(red(bed.shown((800.0, 440.0))?));
    nothing_along(&mut bed, 400.0, (590.0, 1010.0))?;
    bed.hp.step(Duration::from_millis(16))?;
    nothing_along(&mut bed, 400.0, (590.0, 1010.0))?;
    // Gone in one step too, past the fade with no frame in it.
    bed.hp.step(Duration::from_secs(4))?;
    bed.hp.step(Duration::from_millis(16))?;
    bed.draw(&[KEY_LEFTMETA], &[(600.0, 400.0), (1000.0, 400.0)]);
    bed.hp.step(Duration::from_millis(16))?;
    nothing_along(&mut bed, 440.0, (590.0, 1010.0))?;
    assert!(red(bed.shown((800.0, 400.0))?));
    Ok(())
}

#[test]
fn alt_drag_moves_a_window_and_super_drag_no_longer_does() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let start = open(&mut bed.hp, "test.move", (400, 300))?;
    let (cx, cy) = start.centre();
    // `Super`+drag draws over it; it stays.
    bed.draw(&[KEY_LEFTMETA], &[(cx, cy), (cx + 100.0, cy + 50.0)]);
    common::at_rest(&mut bed.hp)?;
    assert_eq!(rect_of(&bed.hp, "test.move"), Some(start));
    assert!(bed.hp.ink().0);
    chord(&mut bed.hp, &[KEY_ESC]);
    // `Alt`+drag moves it.
    bed.draw(&[KEY_LEFTALT], &[(cx, cy), (cx + 100.0, cy + 50.0)]);
    let moved = Rect::new(start.x + 100, start.y + 50, start.width, start.height);
    wait_for_rect(&mut bed.hp, "test.move", moved)?;
    assert!(!bed.hp.ink().0);
    Ok(())
}

#[test]
fn at_rest_after_the_fade_no_frame_and_no_wakeup() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.draw(&[KEY_LEFTMETA], &[(800.0, 400.0), (1000.0, 500.0)]);
    // On the real clock: 3.6 s.
    let gone = bed
        .hp
        .dispatch_until(Duration::from_secs(20), |hp| hp.ink() == (false, false))?;
    anyhow::ensure!(gone, "the strokes never went");
    common::at_rest(&mut bed.hp)?;
    assert_eq!(bed.hp.wake(), None, "a wakeup is due");
    let frames = bed.hp.frames();
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.frames(), frames, "a frame at rest");
    assert_eq!(bed.hp.wake(), None, "a wakeup is due");
    Ok(())
}

/// The pull request's screenshots, at 1920 × 1080 at scale 1: a stroke,
/// an arrow and the laser with its trail, over a window, written to
/// `$HOCUS_SHOTS` when it names a folder. Run by hand (`--run-ignored
/// only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(u32::try_from(w)?, u32::try_from(h)?, rgba)
            .context("a screenshot of the wrong size")?;
        image.save(out.join(format!("{name}.png")))?;
        Ok(())
    };
    let mut bed = Bed::start_on(false)?;
    open(&mut bed.hp, "test.shots", (1100, 700))?;
    // A stroke: a loop around something, then under it.
    let mut path = Vec::new();
    for i in 0..=48 {
        let a = f64::from(i) / 48.0 * std::f64::consts::TAU * 1.05;
        path.push((900.0 + 160.0 * a.cos(), 480.0 + 90.0 * a.sin()));
    }
    path.extend([(1100.0, 600.0), (1180.0, 640.0), (1300.0, 650.0)]);
    bed.draw(&[KEY_LEFTMETA], &path);
    common::at_rest(&mut bed.hp)?;
    save(&mut bed, "stroke")?;
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.draw(
        &[KEY_LEFTMETA, KEY_LEFTSHIFT],
        &[(520.0, 760.0), (700.0, 700.0), (840.0, 560.0)],
    );
    common::at_rest(&mut bed.hp)?;
    save(&mut bed, "arrow")?;
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.hp.pointer_move(500.0, 600.0);
    common::at_rest(&mut bed.hp)?;
    bed.hp.hold_clock();
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.key(KEY_LEFTALT, true);
    for i in 1..=60 {
        let t = f64::from(i) / 60.0;
        bed.hp
            .pointer_move(500.0 + 500.0 * t, 600.0 - 200.0 * (t * 3.0).sin());
        bed.hp.step(Duration::from_millis(6))?;
    }
    save(&mut bed, "laser")?;
    bed.hp.key(KEY_LEFTALT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    Ok(())
}
