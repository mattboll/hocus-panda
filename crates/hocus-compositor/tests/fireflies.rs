//! Evening fireflies (#804, docs/design/effects.md), on the held clock and
//! on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names):
//! in the evening they glow on the wallpaper, moving, and the window's
//! pixels stay as they were; by day, or off, nothing; still with reduced
//! motion, nothing drawn frame after frame.

use crate::common;

use std::{path::Path, time::Duration};

use common::open_with;
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);
/// Across the screen's lower half, where they roam.
const WINDOW: Rect = Rect {
    x: 100,
    y: 380,
    width: 1000,
    height: 220,
};

fn fireflies(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.fireflies = on;
    config.appearance.reduced_motion = reduced_motion;
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

/// A GLES compositor, a window of the test's colour at [`WINDOW`], at
/// rest, the fireflies off, evening or not; the clock held.
fn bed(evening: bool) -> anyhow::Result<(Headless, common::ClientHandle)> {
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
    hp.hold_evening(evening);
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(
        &mut hp,
        "test.dusk",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.dusk"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, WINDOW);
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    run(&mut hp, STEP * 2)?;
    Ok((hp, client))
}

/// Over `rect`, the pixels the same in `a` and `b`, of how many.
fn same(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>), rect: Rect) -> (usize, usize) {
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

/// The wallpaper where they roam, around the window.
const WALL: [Rect; 2] = [
    Rect {
        x: 0,
        y: 240,
        width: 1100,
        height: 130,
    },
    Rect {
        x: 0,
        y: 610,
        width: 1100,
        height: 180,
    },
];

/// The wallpaper's pixels lit (changed) between `a` and `b`.
fn lit(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>)) -> usize {
    WALL.iter()
        .map(|r| {
            let (kept, all) = same(a, b, *r);
            all - kept
        })
        .sum()
}

#[test]
fn in_the_evening_they_glow_on_the_wallpaper_never_on_a_window() -> anyhow::Result<()> {
    let (mut hp, _client) = bed(true)?;
    let before = hp.screenshot(0)?;
    hp.reconfigure(fireflies(true, false));
    run(&mut hp, Duration::from_secs(4))?;
    let flies = hp
        .fireflies("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("no fireflies"))?;
    assert!(flies.len() > 60, "{}", flies.len());
    let shot = hp.screenshot(0)?;
    assert!(lit(&before, &shot) > 2000, "glows on the wallpaper");
    let (kept, all) = same(&before, &shot, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(500))?;
    assert!(hp.frames_on(0) > frames + 10, "they move");
    let moved = hp
        .fireflies("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("gone"))?;
    assert!(moved.iter().zip(&flies).any(|(a, b)| a != b));
    let later = hp.screenshot(0)?;
    let (kept, all) = same(&before, &later, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    Ok(())
}

#[test]
fn by_day_or_off_none_and_still_with_reduced_motion() -> anyhow::Result<()> {
    for (evening, config, case) in [
        (false, fireflies(true, false), "by day"),
        (true, fireflies(false, false), "off"),
        (true, fireflies(true, true), "reduced motion"),
    ] {
        let (mut hp, _client) = bed(evening)?;
        let before = hp.screenshot(0)?;
        hp.reconfigure(config);
        run(&mut hp, Duration::from_millis(100))?;
        let still = case == "reduced motion";
        assert_eq!(hp.fireflies("HEADLESS-1").is_some(), still, "{case}");
        let shot = hp.screenshot(0)?;
        assert_eq!(lit(&before, &shot) > 0, still, "{case}: drawn");
        let last = hp.frames_on(0);
        run(&mut hp, Duration::from_millis(300))?;
        assert_eq!(hp.frames_on(0), last, "{case}: nothing drawn per frame");
    }
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

/// The fireflies for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar and two windows, in the
/// evening: drifting; then gathered round the pointer resting on the
/// wallpaper. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    hp.hold_evening(true);
    config.effects.fireflies = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_secs(6))?;
    save(&mut hp, &out, "fireflies")?;
    hp.pointer_move(420.0, 640.0);
    run(&mut hp, Duration::from_secs(14))?;
    save(&mut hp, &out, "fireflies-round-the-pointer")?;
    Ok(())
}
