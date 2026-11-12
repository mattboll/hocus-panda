//! The desktop's effects (#793) and closing a window in flames (#794,
//! docs/design/effects.md), on the held clock: a closed window burns on
//! GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names), its
//! shader compiled and drawing flames over and above it, then goes and
//! nothing more is drawn; on the CPU, with reduced motion or the switch
//! off, it closes as standard.

use crate::common;

use std::time::Duration;

use common::{WINDOW_COLOUR, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);

fn flaming() -> Config {
    let mut config = Config::default();
    config.effects.flames = true;
    config
}

fn on(renderer: HeadlessConfig) -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![OutputSpec::new(1280, 800)],
        ..renderer
    }
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

/// Over `rect` (clear of its rim), the share of pixels in the test
/// window's colour.
fn window_share(shot: &(i32, i32, Vec<u8>), rect: Rect) -> f64 {
    let [b0, g0, r0] = [
        WINDOW_COLOUR & 0xff,
        (WINDOW_COLOUR >> 8) & 0xff,
        (WINDOW_COLOUR >> 16) & 0xff,
    ]
    .map(|v| v as i32);
    let (mut seen, mut all) = (0u64, 0u64);
    for y in (rect.y + 12..rect.y + rect.height - 12).step_by(3) {
        for x in (rect.x + 12..rect.x + rect.width - 12).step_by(3) {
            let i = ((y * shot.0 + x) * 4) as usize;
            let p = &shot.2[i..i + 3];
            let (b, g, r) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            all += 1;
            seen += u64::from((b - b0).abs() <= 3 && (g - g0).abs() <= 3 && (r - r0).abs() <= 3);
        }
    }
    seen as f64 / all.max(1) as f64
}

/// Pixels of fire (bright orange to yellow) in `rect`, clipped to the
/// screen.
fn fire(shot: &(i32, i32, Vec<u8>), rect: Rect) -> usize {
    let (w, h) = (shot.0, shot.2.len() as i32 / 4 / shot.0);
    let mut n = 0;
    for y in rect.y.max(0)..(rect.y + rect.height).min(h) {
        for x in rect.x.max(0)..(rect.x + rect.width).min(w) {
            let i = ((y * w + x) * 4) as usize;
            let (b, g, r) = (shot.2[i], shot.2[i + 1], shot.2[i + 2]);
            n += usize::from(r > 200 && g > 60 && b < 110 && r > g.saturating_add(40));
        }
    }
    n
}

#[test]
fn a_closed_window_burns_its_flames_rising_above_it_then_goes() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles()), flaming())?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (rect, client) = open_with(&mut hp, "test.burn", (600, 400), false)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.windows().is_empty())?;
    // Gone from the desktop (input never reaches it): its picture burns.
    assert!(hp.windows().is_empty());
    assert_eq!(hp.burning(), 1, "it burns");
    hp.step(Duration::ZERO)?;
    let whole = window_share(&hp.screenshot(0)?, rect);
    // Halfway: the paper half gone, flames over it and above the window.
    run(&mut hp, Duration::from_millis(850))?;
    let shot = hp.screenshot(0)?;
    let half = window_share(&shot, rect);
    let over = fire(&shot, rect);
    let above = fire(
        &shot,
        Rect {
            x: rect.x,
            y: rect.y - 130,
            width: rect.width,
            height: 130,
        },
    );
    eprintln!("whole {whole}, halfway {half}, fire over {over}, above {above}");
    assert!(whole > 0.9, "whole as it starts: {whole}");
    assert!(half > 0.05 && half < 0.8, "half burnt: {half}");
    assert!(over > 500, "flames over it: {over}");
    // By 1.7 s the paper is gone; its sparks drift on a while.
    run(&mut hp, Duration::from_millis(900))?;
    let gone = window_share(&hp.screenshot(0)?, rect);
    assert!(gone < 0.01, "burnt: {gone}");
    assert_eq!(hp.burning(), 1, "its sparks drift on");
    // All gone within 5 s, and then nothing is drawn.
    let mut waited = Duration::ZERO;
    while hp.burning() > 0 && waited < Duration::from_secs(4) {
        run(&mut hp, Duration::from_millis(100))?;
        waited += Duration::from_millis(100);
    }
    assert_eq!(hp.burning(), 0, "over within 5 s");
    assert_eq!(hp.leaving(), 0);
    run(&mut hp, Duration::from_millis(100))?;
    let last = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(100))?;
    assert_eq!(hp.frames_on(0), last, "nothing drawn once it is over");
    Ok(())
}

#[test]
fn on_the_cpu_with_reduced_motion_or_the_switch_off_it_closes_as_standard() -> anyhow::Result<()> {
    let pixman = HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    };
    let mut cases = vec![(on(pixman), flaming(), "pixman")];
    let mut reduced = flaming();
    reduced.appearance.reduced_motion = true;
    cases.push((on(common::gles()), reduced, "reduced motion"));
    cases.push((on(common::gles()), Config::default(), "off"));
    for (renderer, config, case) in cases {
        let mut hp = Headless::start_with(renderer, config)?;
        common::at_rest(&mut hp)?;
        let (_, client) = open_with(&mut hp, "test.plain", (600, 400), false)?;
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        let _ = client.commands.send(common::Command::Close);
        hp.dispatch_until(common::WAIT, |hp| hp.windows().is_empty())?;
        assert_eq!(hp.burning(), 0, "{case}: no flames");
        assert_eq!(hp.leaving(), 1, "{case}: it closes as standard");
        // The standard close (220 ms), or reduced motion's fade.
        run(&mut hp, Duration::from_millis(400))?;
        assert_eq!(hp.leaving(), 0, "{case}: gone as standard");
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

/// The burn for the maintainer, written to `$HOCUS_SHOTS` when it names a
/// folder, on GLES: a document window beside another, as it closes, then
/// at 30 %, 60 % and 85 % of its burn. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut config = flaming();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        config,
    )?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    common::at_rest(&mut hp)?;
    let beside = common::Shows::Document {
        app_id: "test.beside",
        size: (420, 520),
    };
    let (_, _beside) = common::open_showing(&mut hp, "test.beside", beside)?;
    let shows = common::Shows::Document {
        app_id: "test.shot",
        size: (560, 420),
    };
    let (rect, client) = common::open_showing(&mut hp, "test.shot", shows)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let x = (rect.x - 160).max(0);
    let y = (rect.y - 170).max(0);
    let crop = (
        x as u32,
        y as u32,
        ((rect.x + rect.width + 160).min(1280) - x) as u32,
        ((rect.y + rect.height + 120).min(800) - y) as u32,
    );
    save(&mut hp, &out, "flames-before", crop)?;
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.burning() == 1)?;
    hp.step(Duration::ZERO)?;
    let mut at = Duration::ZERO;
    for percent in [30u64, 60, 85] {
        let to = Duration::from_millis(1700 * percent / 100);
        run(&mut hp, to - at)?;
        at = to;
        save(&mut hp, &out, &format!("flames-{percent:02}"), crop)?;
    }
    run(&mut hp, Duration::from_millis(1700 + 400) - at)?;
    save(&mut hp, &out, "flames-after", crop)?;
    Ok(())
}
