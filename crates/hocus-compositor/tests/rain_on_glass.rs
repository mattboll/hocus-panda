//! Rain on the glass (#802, docs/design/effects.md), on the held clock and
//! on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names):
//! the wallpaper turns to wet glass, its drops sliding, and the window's
//! pixels stay as they were; off, nothing; still with reduced motion,
//! nothing drawn frame after frame.

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

fn rain(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.rain_on_glass = on;
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
/// rest, the rain off; the clock held.
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
        "test.rain",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.rain"))
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

/// Wallpaper only: left of the window, under the bar's height.
const WALL: Rect = Rect {
    x: 20,
    y: 100,
    width: 240,
    height: 600,
};

#[test]
fn the_wallpaper_turns_to_wet_glass_and_the_window_stays_as_it_was() -> anyhow::Result<()> {
    let (mut hp, _client) = bed()?;
    let before = hp.screenshot(0)?;
    hp.reconfigure(rain(true, false));
    run(&mut hp, Duration::from_millis(200))?;
    let (stills, rolling) = hp
        .raindrops("HEADLESS-1")
        .ok_or_else(|| anyhow::anyhow!("no rain"))?;
    assert!(stills > 500 && rolling > 3, "{stills} still, {rolling} big");
    let wet = hp.screenshot(0)?;
    let (kept, all) = same(&before, &wet, WALL);
    assert!(kept < all / 2, "the wallpaper fogged: {kept} of {all} kept");
    let (kept, all) = same(&before, &wet, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    // Its drops slide: drawn again where they moved, frame after frame.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(1500))?;
    assert!(hp.frames_on(0) > frames + 30, "it moves");
    let later = hp.screenshot(0)?;
    let (kept, all) = same(&wet, &later, WALL);
    assert!(kept < all, "drops moved: {kept} of {all} kept");
    let (kept, all) = same(&before, &later, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    Ok(())
}

#[test]
fn off_nothing_and_still_with_reduced_motion() -> anyhow::Result<()> {
    for (config, case) in [
        (rain(false, false), "off"),
        (rain(true, true), "reduced motion"),
    ] {
        let (mut hp, _client) = bed()?;
        let before = hp.screenshot(0)?;
        hp.reconfigure(config);
        run(&mut hp, Duration::from_millis(100))?;
        let still = case == "reduced motion";
        assert_eq!(hp.raindrops("HEADLESS-1").is_some(), still, "{case}");
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

/// The rain for the maintainer, written to `$HOCUS_SHOTS` when it names a
/// folder, on GLES, with the shell's bar and two windows: over the "Nuit"
/// wallpaper, then a second later. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    config.effects.rain_on_glass = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_secs(2))?;
    save(&mut hp, &out, "rain")?;
    run(&mut hp, Duration::from_secs(1))?;
    save(&mut hp, &out, "rain-a-second-later")?;
    Ok(())
}
