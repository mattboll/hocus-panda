//! Daylight (#805, docs/design/effects.md), on the held clock and on GLES
//! (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names), the sun
//! held at times of the test's (tests run in UTC: 45° N, 0° E): the
//! wallpaper is the sky of the hour, the window's shadow turns with the
//! sun and its pixels stay as they were; nothing is drawn frame after
//! frame; off, nothing.

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
/// Under the window and either side: where its shadow falls.
const BELOW: Rect = Rect {
    x: 200,
    y: 530,
    width: 700,
    height: 90,
};

fn at(s: &str) -> anyhow::Result<jiff::Timestamp> {
    Ok(s.parse()?)
}

fn daylight(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.daylight = on;
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
/// rest, the daylight off, the sun held at `sun`; the clock held.
fn bed(sun: &str) -> anyhow::Result<(Headless, common::ClientHandle)> {
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
    hp.hold_sun(at(sun)?);
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(
        &mut hp,
        "test.daylight",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.daylight"))
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
fn the_sky_follows_the_sun_and_the_window_stays_as_it_was() -> anyhow::Result<()> {
    // 21 June, 6:30 UTC: the board's 8 o'clock.
    let (mut hp, _client) = bed("2026-06-21T06:30:00Z")?;
    let before = hp.screenshot(0)?;
    hp.reconfigure(daylight(true, false));
    run(&mut hp, STEP * 3)?;
    let hour = hp
        .daylight_hour()
        .ok_or_else(|| anyhow::anyhow!("no daylight"))?;
    assert!((7.6..8.4).contains(&hour), "{hour}");
    let morning = hp.screenshot(0)?;
    let (kept, all) = same(&before, &morning, WALL);
    assert!(
        kept < all / 10,
        "the wallpaper is the sky: {kept} of {all} kept"
    );
    let (kept, all) = same(&before, &morning, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    // Nothing drawn frame after frame.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn per frame");
    // 18:07: the board's sunset, its sky and the shadow another.
    hp.hold_sun(at("2026-06-21T18:07:00Z")?);
    run(&mut hp, STEP * 3)?;
    let hour = hp
        .daylight_hour()
        .ok_or_else(|| anyhow::anyhow!("no daylight"))?;
    assert!((18.2..18.8).contains(&hour), "{hour}");
    let evening = hp.screenshot(0)?;
    let (kept, all) = same(&morning, &evening, WALL);
    assert!(kept < all / 10, "another sky: {kept} of {all} kept");
    let (kept, all) = same(&morning, &evening, BELOW);
    assert!(kept < all / 2, "the shadow turned: {kept} of {all} kept");
    let (kept, all) = same(&before, &evening, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    Ok(())
}

#[test]
fn off_nothing_and_with_reduced_motion_drawn_once() -> anyhow::Result<()> {
    for (config, case) in [
        (daylight(false, false), "off"),
        (daylight(true, true), "reduced motion"),
    ] {
        let (mut hp, _client) = bed("2026-06-21T12:00:00Z")?;
        let before = hp.screenshot(0)?;
        hp.reconfigure(config);
        run(&mut hp, Duration::from_millis(100))?;
        let shown = case == "reduced motion";
        assert_eq!(hp.daylight_hour().is_some(), shown, "{case}");
        let shot = hp.screenshot(0)?;
        let (kept, all) = same(&before, &shot, WALL);
        assert_eq!(kept < all, shown, "{case}: drawn: {kept} of {all} kept");
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

/// The daylight for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar and two windows, on 21
/// June at 45° N, 0° E: the morning (the board's 8 o'clock), noon (13),
/// the sunset (18:30) and the night (23). Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    hp.hold_sun(at("2026-06-21T06:30:00Z")?);
    config.effects.daylight = true;
    hp.reconfigure(config);
    for (time, name) in [
        ("2026-06-21T06:30:00Z", "daylight-morning"),
        ("2026-06-21T12:02:00Z", "daylight-noon"),
        ("2026-06-21T18:07:00Z", "daylight-sunset"),
        ("2026-06-21T22:30:00Z", "daylight-night"),
    ] {
        hp.hold_sun(at(time)?);
        run(&mut hp, Duration::from_millis(200))?;
        save(&mut hp, &out, name)?;
    }
    Ok(())
}
