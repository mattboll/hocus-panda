//! Desktops in depth (#806, docs/design/effects.md), on the held clock and
//! on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names):
//! the wallpaper is the board's landscape; a switch of desktops moves the
//! camera through it in 1.5 s, pulling back halfway, each layer by its
//! share of the travel, the landscape drawn again at most 60 times a
//! second; the window's pixels stay as they were; at rest nothing is
//! drawn frame after frame.

use crate::common;

use std::{path::Path, time::Duration};

use common::{KEY_1, KEY_2, KEY_LEFTMETA, chord, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

/// A frame on a 165 Hz screen.
const STEP: Duration = Duration::from_micros(6_061);
const WINDOW: Rect = Rect {
    x: 300,
    y: 60,
    width: 500,
    height: 300,
};
/// One desktop along the strip: the screen's width and the gap.
const STRIDE: f64 = 1280.0 + 24.0;

fn depth(on: bool) -> Config {
    let mut config = Config::default();
    config.effects.depth = on;
    config
}

/// Steps the held clock by `by`, a frame at a time.
fn run(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while left > Duration::ZERO {
        let step = left.min(STEP);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

/// A GLES compositor, a window of the test's colour at [`WINDOW`] on the
/// first desktop, at rest; the clock held.
fn bed() -> anyhow::Result<(Headless, common::ClientHandle)> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        Config::default(),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(
        &mut hp,
        "test.depth",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.depth"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, WINDOW);
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    run(&mut hp, STEP * 2)?;
    Ok((hp, client))
}

type Shot = (i32, i32, Vec<u8>);

/// A layer's silhouette in `shot`: for each column, the first row in
/// `rows` whose red is under `nearer` (this layer, or one in front of
/// it), when it is this layer's (its red at least `own`).
fn outline(shot: &Shot, rows: std::ops::Range<i32>, nearer: u8, own: u8) -> Vec<Option<i32>> {
    (0..shot.0)
        .map(|x| {
            let red = |y: i32| shot.2[((y * shot.0 + x) * 4 + 2) as usize];
            let y = rows.clone().find(|y| red(*y) < nearer)?;
            (red(y) >= own).then_some(y)
        })
        .collect()
}

/// How far left `after` shows the silhouette `before` showed: the shift
/// that matches them best, within `range`.
fn shift(before: &[Option<i32>], after: &[Option<i32>], range: std::ops::Range<usize>) -> usize {
    range
        .filter_map(|s| {
            let pairs: Vec<i32> = (0..before.len().saturating_sub(s))
                .filter_map(|x| Some((before[x + s]? - after[x]?).abs()))
                .collect();
            (pairs.len() >= 100).then(|| {
                let off = pairs.iter().map(|d| f64::from(*d)).sum::<f64>() / pairs.len() as f64;
                (off, s)
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(0, |(_, s)| s)
}

/// Over `rect`, the pixels the same in `a` and `b`, of how many.
fn same(a: &Shot, b: &Shot, rect: Rect) -> (usize, usize) {
    let (mut same, mut all) = (0, 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * a.0 + x) * 4) as usize;
            all += 1;
            same += usize::from(a.2.get(i..i + 3) == b.2.get(i..i + 3));
        }
    }
    (same, all)
}

/// The window but its corners (round: the wallpaper shows there).
fn inside(rect: Rect) -> Rect {
    Rect {
        x: rect.x + 12,
        y: rect.y + 12,
        width: rect.width - 24,
        height: rect.height - 24,
    }
}

#[test]
fn a_switch_moves_each_layer_by_its_share_and_the_window_stays_as_it_was() -> anyhow::Result<()> {
    let (mut hp, _client) = bed()?;
    let before = hp.screenshot(0)?;
    hp.reconfigure(depth(true));
    run(&mut hp, Duration::from_millis(300))?;
    let (x, zoom, _) = hp.depth(0).ok_or_else(|| anyhow::anyhow!("no depth"))?;
    assert_eq!((x, zoom), (0.0, 1.0));
    let first = hp.screenshot(0)?;
    let below = Rect {
        x: 0,
        y: 420,
        width: 1280,
        height: 380,
    };
    let (kept, all) = same(&before, &first, below);
    assert!(kept < all / 10, "the landscape: {kept} of {all} kept");
    let (kept, all) = same(&before, &first, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    // At rest, nothing drawn per frame.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn per frame");

    // Super+2: the camera's move, 1.5 s, a frame each 165th of a second.
    let (_, _, drawn) = hp.depth(0).ok_or_else(|| anyhow::anyhow!("no depth"))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    run(&mut hp, Duration::from_millis(750))?;
    let (x, zoom, _) = hp.depth(0).ok_or_else(|| anyhow::anyhow!("no depth"))?;
    // Halfway, as drawn: at the last 60th of a second.
    assert!((x - STRIDE / 2.0).abs() < 40.0, "halfway: {x}");
    assert!((zoom - 0.9).abs() < 0.01, "pulled back: {zoom}");
    run(&mut hp, Duration::from_millis(800))?;
    let (x, zoom, now) = hp.depth(0).ok_or_else(|| anyhow::anyhow!("no depth"))?;
    assert_eq!((x, zoom), (STRIDE, 1.0), "there");
    let draws = now - drawn;
    assert!(
        (40..=95).contains(&draws),
        "drawn {draws} times in 1.55 s: at most 60 a second"
    );
    let second = hp.screenshot(0)?;
    // The hills (0.38) in front of the far mountains, then the forest
    // (0.72) in front of the hills: each moved left by its share.
    let hills = |shot: &Shot| outline(shot, 440..700, 75, 50);
    let moved = shift(&hills(&first), &hills(&second), 0..800);
    let want = (0.38 * STRIDE).round() as usize;
    assert!(
        moved.abs_diff(want) <= 2,
        "the hills moved {moved}, not {want}"
    );
    let forest = |shot: &Shot| outline(shot, 560..780, 48, 28);
    let moved = shift(&forest(&first), &forest(&second), 0..1100);
    let want = (0.72 * STRIDE).round() as usize;
    assert!(
        moved.abs_diff(want) <= 2,
        "the forest moved {moved}, not {want}"
    );
    // Once the desktops' dots are gone, nothing drawn per frame.
    run(&mut hp, Duration::from_millis(500))?;
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn per frame");

    // Back: the window as it was, pixel for pixel.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    run(&mut hp, Duration::from_millis(1600))?;
    let back = hp.screenshot(0)?;
    let (kept, all) = same(&before, &back, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    let (kept, all) = same(&first, &back, below);
    assert_eq!(kept, all, "the landscape where it was");
    Ok(())
}

// --- The screenshots ------------------------------------------------------------

/// A screenshot of output 0 saved as `name`.png in `out`.
fn save(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// Desktops in depth for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar and two windows on the
/// first desktop: at rest, then a switch to the second at a quarter, half
/// and three quarters of its way, and there. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    config.effects.depth = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_millis(200))?;
    save(&mut hp, &out, "depth-rest")?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    for (at, name) in [
        (375, "depth-switch-25"),
        (375, "depth-switch-50"),
        (375, "depth-switch-75"),
        (500, "depth-there"),
    ] {
        run(&mut hp, Duration::from_millis(at))?;
        save(&mut hp, &out, name)?;
    }
    Ok(())
}

/// The landscape with the daylight on (#822), for the maintainer, written
/// to `$HOCUS_SHOTS` as [`shots`] is: its sky at the board's 8, 13, 17:30
/// and 22 o'clock (21 June at 45° N, 0° E, the tests' place). Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn daylight_shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    let at = |s: &str| s.parse::<jiff::Timestamp>();
    hp.hold_sun(at("2026-06-21T06:32:00Z")?);
    config.effects.depth = true;
    config.effects.daylight = true;
    hp.reconfigure(config);
    for (time, name) in [
        ("2026-06-21T06:32:00Z", "depth-daylight-0800"),
        ("2026-06-21T12:02:00Z", "depth-daylight-1300"),
        ("2026-06-21T16:59:00Z", "depth-daylight-1730"),
        ("2026-06-21T21:28:00Z", "depth-daylight-2200"),
    ] {
        hp.hold_sun(at(time)?);
        run(&mut hp, Duration::from_millis(200))?;
        eprintln!("{name}: the board's {:?}", hp.daylight_hour());
        save(&mut hp, &out, name)?;
    }
    Ok(())
}
