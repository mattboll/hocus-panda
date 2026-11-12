//! Windows in jelly (#797), docs/design/effects.md, on the held clock and
//! GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names): a
//! dragged window bends like jelly and lies straight again, pixel for
//! pixel; off, with reduced motion or on the CPU, a drag is rigid.

use crate::common;

use std::time::Duration;

use common::{BTN_LEFT, KEY_LEFTALT, WINDOW_COLOUR, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};

fn on(renderer: HeadlessConfig) -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![OutputSpec::new(1280, 800)],
        ..renderer
    }
}

/// The jelly on; the standard wobble off, so that any bend is the
/// jelly's.
fn both() -> Config {
    let mut config = Config::default();
    config.effects.jelly = true;
    config.windows.wobble = false;
    config
}

/// Steps the held clock by `by`, a frame at a time.
fn run(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while left > Duration::ZERO {
        let step = left.min(common::STEP);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

/// Over `rect` (clear of its rim), the share of pixels in the test
/// window's colour.
fn window_share(shot: &(i32, i32, Vec<u8>), rect: Rect) -> f64 {
    let [b0, g0, r0] = [
        WINDOW_COLOUR & 0xff,
        (WINDOW_COLOUR >> 8) & 0xff,
        (WINDOW_COLOUR >> 16) & 0xff,
    ]
    .map(|v| v as i32);
    let h = shot.2.len() as i32 / 4 / shot.0;
    let (mut seen, mut all) = (0u64, 0u64);
    for y in (rect.y + 12..rect.y + rect.height - 12).step_by(3) {
        for x in (rect.x + 12..rect.x + rect.width - 12).step_by(3) {
            if x < 0 || y < 0 || x >= shot.0 || y >= h {
                continue;
            }
            let i = ((y * shot.0 + x) * 4) as usize;
            let p = &shot.2[i..i + 3];
            let (b, g, r) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            all += 1;
            seen += u64::from((b - b0).abs() <= 3 && (g - g0).abs() <= 3 && (r - r0).abs() <= 3);
        }
    }
    seen as f64 / all.max(1) as f64
}

/// Pixels of `a` and `b` that differ.
fn differ(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>)) -> usize {
    a.2.as_chunks::<4>()
        .0
        .iter()
        .zip(b.2.as_chunks::<4>().0)
        .filter(|(p, q)| p[..3] != q[..3])
        .count()
}

/// Holds the window at `at` with `Alt`.
fn grab(hp: &mut Headless, at: (f64, f64)) {
    hp.pointer_move(at.0, at.1);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
}

fn release(hp: &mut Headless) {
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
}

/// Drags the pointer held at `from` by `by`, in ten moves a frame apart.
fn drag(hp: &mut Headless, from: (f64, f64), by: (f64, f64)) -> anyhow::Result<()> {
    for k in 1..=10 {
        let k = f64::from(k) / 10.0;
        hp.pointer_move(from.0 + by.0 * k, from.1 + by.1 * k);
        hp.step(common::STEP)?;
    }
    Ok(())
}

#[test]
fn a_dragged_window_bends_like_jelly_then_lies_straight() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles()), both())?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    let (rect, _client) = open_with(&mut hp, "test.jelly", (600, 400), false)?;
    let at = (f64::from(rect.x + 120), f64::from(rect.y + 14));
    hp.pointer_move(at.0, at.1);
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    hp.step(common::STEP)?;
    let before = hp.screenshot(0)?;
    // Held on its title near the left, thrown 250 px right in 160 ms: the
    // point held follows, the far side lags.
    grab(&mut hp, at);
    drag(&mut hp, at, (250.0, 0.0))?;
    let bend = hp.bend().unwrap_or(0.0);
    let moved = Rect::new(rect.x + 250, rect.y, rect.width, rect.height);
    let covered = window_share(&hp.screenshot(0)?, moved);
    eprintln!("bent {bend:.1} px, the window where it went {covered:.3}");
    assert!(bend > 20.0, "it bends like jelly: {bend}");
    assert!(covered < 0.97, "its far side lags: {covered}");
    // Back where it was, let go: it swings, then lies still within 3 s.
    drag(&mut hp, (at.0 + 250.0, at.1), (-250.0, 0.0))?;
    release(&mut hp);
    let mut waited = Duration::ZERO;
    while hp.bend().is_some() && waited < Duration::from_secs(3) {
        run(&mut hp, Duration::from_millis(100))?;
        waited += Duration::from_millis(100);
    }
    assert!(hp.bend().is_none(), "settled within 3 s");
    // Straight: as it was before the drag, pixel for pixel, and nothing
    // drawn frame after frame.
    run(&mut hp, Duration::from_millis(100))?;
    let after = hp.screenshot(0)?;
    assert_eq!(differ(&before, &after), 0, "straight again");
    let last = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.frames_on(0), last, "nothing drawn once still");
    Ok(())
}

#[test]
fn off_with_reduced_motion_or_on_the_cpu_a_drag_is_rigid() -> anyhow::Result<()> {
    let pixman = HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    };
    let mut reduced = both();
    reduced.appearance.reduced_motion = true;
    let mut off = Config::default();
    off.windows.wobble = false;
    for (renderer, config, case) in [
        (on(pixman), both(), "pixman"),
        (on(common::gles()), reduced, "reduced motion"),
        (on(common::gles()), off, "off"),
    ] {
        let mut hp = Headless::start_with(renderer, config)?;
        common::at_rest(&mut hp)?;
        let (rect, _client) = open_with(&mut hp, "test.plain", (600, 400), false)?;
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        let at = (f64::from(rect.x + 120), f64::from(rect.y + 14));
        grab(&mut hp, at);
        drag(&mut hp, at, (250.0, 0.0))?;
        assert_eq!(hp.bend(), None, "{case}: rigid");
        release(&mut hp);
    }
    Ok(())
}

/// A screenshot of output 0 saved as `name`.png in `out`, cut to `crop`.
fn save(
    hp: &mut Headless,
    out: &std::path::Path,
    name: &str,
    crop: (u32, u32, u32, u32),
) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let (x, y, cw, ch) = crop;
    let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The jelly for the maintainer, written to `$HOCUS_SHOTS` when it names
/// a folder, on GLES: a document window shaken as the board's demo shakes
/// it. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut config = both();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with(on(common::gles()), config)?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    common::at_rest(&mut hp)?;
    let shows = common::Shows::Document {
        app_id: "test.shot",
        size: (560, 400),
    };
    let (rect, _client) = common::open_showing(&mut hp, "test.shot", shows)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let crop = (0, 0, 1280, 800);
    save(&mut hp, &out, "jelly-before", crop)?;
    // The board's demo: held on the title 120 px in, swung sideways.
    let at = (f64::from(rect.x + 120), f64::from(rect.y + 14));
    grab(&mut hp, at);
    let start = hp.now();
    let mut shot = 0;
    for frame in 1..=90u32 {
        let k = (hp.now() - start).as_secs_f64();
        let x = (k * 3.2).sin() * 230.0 * (k * 1.5).min(1.0);
        let y = (k * 6.4).sin() * 50.0 + k * 20.0;
        hp.pointer_move(at.0 + x, at.1 + y);
        hp.step(common::STEP)?;
        if [24, 40, 62].contains(&frame) {
            shot += 1;
            save(&mut hp, &out, &format!("jelly-drag-{shot}"), crop)?;
        }
    }
    release(&mut hp);
    run(&mut hp, Duration::from_millis(250))?;
    save(&mut hp, &out, "jelly-let-go", crop)?;
    run(&mut hp, Duration::from_secs(3))?;
    save(&mut hp, &out, "jelly-settled", crop)?;
    Ok(())
}
