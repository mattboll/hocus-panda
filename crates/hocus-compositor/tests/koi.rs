//! The koi pond (#803, docs/design/effects.md), on the held clock and on
//! GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names): the
//! wallpaper turns to a pond whose koi swim, and the window's pixels stay
//! as they were; off, nothing; still with reduced motion, nothing drawn
//! frame after frame.

use crate::common;

use std::{path::Path, time::Duration};

use common::open_with;
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);
const WINDOW: Rect = Rect {
    x: 300,
    y: 200,
    width: 500,
    height: 320,
};
/// Wallpaper only: left of the window, under the bar's height.
const WALL: Rect = Rect {
    x: 20,
    y: 100,
    width: 240,
    height: 600,
};

fn koi(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.koi = on;
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

/// Steps until the pond shows (its floor drawn by a worker, on the wall
/// clock), at most a few seconds.
fn until_pond(hp: &mut Headless) -> anyhow::Result<Vec<(f64, f64)>> {
    for _ in 0..500 {
        if let Some(koi) = hp.koi("HEADLESS-1") {
            return Ok(koi);
        }
        std::thread::sleep(Duration::from_millis(10));
        hp.step(STEP)?;
    }
    anyhow::bail!("no pond")
}

/// A GLES compositor, a window of the test's colour at [`WINDOW`], at
/// rest, the koi off; the clock held.
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
        "test.koi",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.koi"))
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

#[test]
fn the_wallpaper_turns_to_a_pond_and_the_window_stays_as_it_was() -> anyhow::Result<()> {
    let (mut hp, _client) = bed()?;
    let before = hp.screenshot(0)?;
    hp.reconfigure(koi(true, false));
    let koi = until_pond(&mut hp)?;
    // The board's seven over its 1200 × 720, as many per pixel.
    assert!((7..=10).contains(&koi.len()), "{}", koi.len());
    run(&mut hp, Duration::from_millis(100))?;
    let pond = hp.screenshot(0)?;
    let (kept, all) = same(&before, &pond, WALL);
    assert!(
        kept < all / 10,
        "the wallpaper is a pond: {kept} of {all} kept"
    );
    let (kept, all) = same(&before, &pond, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    // The koi swim and the water moves, frame after frame.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_secs(1))?;
    assert!(hp.frames_on(0) > frames + 30, "it moves");
    let later = hp.screenshot(0)?;
    let (kept, all) = same(&pond, &later, WALL);
    assert!(kept < all, "the water moved: {kept} of {all} kept");
    let moved = hp
        .koi("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("no koi"))?;
    assert!(
        koi.iter()
            .zip(&moved)
            .all(|(a, b)| (a.0 - b.0).hypot(a.1 - b.1) > 20.0),
        "each swam: {koi:?} → {moved:?}"
    );
    let (kept, all) = same(&before, &later, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    Ok(())
}

#[test]
fn off_nothing_and_still_with_reduced_motion() -> anyhow::Result<()> {
    for (config, case) in [
        (koi(false, false), "off"),
        (koi(true, true), "reduced motion"),
    ] {
        let (mut hp, _client) = bed()?;
        let before = hp.screenshot(0)?;
        hp.reconfigure(config);
        let still = case == "reduced motion";
        if still {
            until_pond(&mut hp)?;
        }
        run(&mut hp, Duration::from_millis(100))?;
        assert_eq!(hp.koi("HEADLESS-1").is_some(), still, "{case}");
        let shot = hp.screenshot(0)?;
        let (kept, all) = same(&before, &shot, WALL);
        assert_eq!(kept < all, still, "{case}: drawn: {kept} of {all} kept");
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

/// The pond for the maintainer, written to `$HOCUS_SHOTS` when it names a
/// folder, on GLES, with the shell's bar and two windows: swimming; the
/// pointer resting on the water, the koi come to it; a click there. Run
/// by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    config.effects.koi = true;
    hp.reconfigure(config);
    until_pond(&mut hp)?;
    run(&mut hp, Duration::from_secs(3))?;
    save(&mut hp, &out, "koi")?;
    hp.pointer_move(330.0, 640.0);
    run(&mut hp, Duration::from_secs(10))?;
    save(&mut hp, &out, "koi-round-the-pointer")?;
    hp.button(0x110, true);
    hp.button(0x110, false);
    run(&mut hp, Duration::from_millis(400))?;
    save(&mut hp, &out, "koi-a-click")?;
    Ok(())
}
