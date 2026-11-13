//! What the effects that play on their own cost (#815,
//! docs/design/effects.md), on the held clock and on GLES (Mesa's
//! software device, or the GPU `HOCUS_TEST_GPU` names): they move on at
//! most 60 times a second whatever the screen's refresh, and the snow
//! falling tells the screen its flakes' places alone, not the screen; the
//! koi's pond and the rain on the glass the same.

use crate::common;

use std::time::Duration;

use common::open_with;
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

const SCREEN: (i32, i32) = (1280, 800);

/// A GLES compositor, a window at rest in the middle, the clock held,
/// then `config`'s effects on for half a second of 60 Hz frames.
fn bed(config: Config) -> anyhow::Result<(Headless, common::ClientHandle)> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(SCREEN.0, SCREEN.1)],
            ..common::gles()
        },
        Config::default(),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(&mut hp, "test.still", (600, 360), false)?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.still"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, Rect::new(340, 220, 600, 360));
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    hp.reconfigure(config);
    for _ in 0..30 {
        hp.step(Duration::from_micros(16_667))?;
    }
    Ok((hp, client))
}

fn snow_and_garland() -> Config {
    let mut config = Config::default();
    config.effects.snow = true;
    config.effects.garland = true;
    config
}

#[test]
fn on_a_165_hz_screen_they_move_on_60_times_a_second() -> anyhow::Result<()> {
    let (mut hp, _client) = bed(snow_and_garland())?;
    anyhow::ensure!(hp.snowing() && hp.garland().is_some(), "no effects");
    // A second of a 165 Hz screen's frames.
    let (moved, drawn) = (hp.effect_frames(), hp.frames_on(0));
    for _ in 0..165 {
        hp.step(Duration::from_nanos(6_060_606))?;
    }
    let moved = hp.effect_frames() - moved;
    let drawn = hp.frames_on(0) - drawn;
    eprintln!("a second at 165 Hz: {moved} effect frames, {drawn} drawn");
    assert!((57..=61).contains(&moved), "{moved} effect frames");
    // Nothing else moves: a frame drawn for each of theirs alone.
    assert!(drawn <= 62, "{drawn} frames drawn");
    // At 60 Hz, one each refresh.
    let moved = hp.effect_frames();
    for _ in 0..60 {
        hp.step(Duration::from_micros(16_667))?;
    }
    assert_eq!(hp.effect_frames() - moved, 60);
    Ok(())
}

#[test]
fn snow_falling_redraws_its_flakes_not_the_screen() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.effects.snow = true;
    let (mut hp, _client) = bed(config)?;
    anyhow::ensure!(hp.snowing(), "no snow");
    hp.take_damage_area(0);
    let drawn = hp.frames_on(0);
    for _ in 0..60 {
        hp.step(Duration::from_micros(16_667))?;
    }
    let drawn = hp.frames_on(0) - drawn;
    let screen = (SCREEN.0 * SCREEN.1) as u64;
    let each = hp.take_damage_area(0) / drawn.max(1);
    eprintln!(
        "{drawn} frames, {each} pixels drawn over each ({} %)",
        each * 100 / screen
    );
    assert!(drawn >= 55, "it falls: {drawn} frames");
    assert!(each < screen / 4, "{each} pixels a frame of {screen}");
    Ok(())
}

/// A second at 165 Hz: the effects' frames.
fn at_165_hz(hp: &mut Headless) -> anyhow::Result<u64> {
    let moved = hp.effect_frames();
    for _ in 0..165 {
        hp.step(Duration::from_nanos(6_060_606))?;
    }
    Ok(hp.effect_frames() - moved)
}

#[test]
fn the_koi_and_the_rain_on_the_glass_move_on_60_times_a_second_too() -> anyhow::Result<()> {
    let screen = (SCREEN.0 * SCREEN.1) as u64;
    for name in ["koi", "rain"] {
        let mut config = Config::default();
        config.effects.koi = name == "koi";
        config.effects.rain_on_glass = name == "rain";
        let (mut hp, _client) = bed(config)?;
        // The pond's floor is drawn by a worker.
        let shows =
            |hp: &Headless| hp.koi("HEADLESS-1").is_some() || hp.raindrops("HEADLESS-1").is_some();
        for _ in 0..500 {
            if shows(&hp) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            hp.step(Duration::from_micros(16_667))?;
        }
        anyhow::ensure!(shows(&hp), "no {name}");
        let moved = at_165_hz(&mut hp)?;
        assert!((57..=61).contains(&moved), "{name}: {moved} effect frames");
        // At 60 Hz: the rain tells its drops' places alone. The koi's
        // water ripples all over as they swim: all of it changes.
        hp.take_damage_area(0);
        let drawn = hp.frames_on(0);
        for _ in 0..60 {
            hp.step(Duration::from_micros(16_667))?;
        }
        let drawn = hp.frames_on(0) - drawn;
        let each = hp.take_damage_area(0) / drawn.max(1);
        eprintln!(
            "{name}: {moved} frames a second at 165 Hz; {drawn} at 60 Hz, {each} pixels \
             drawn over each ({} %)",
            each * 100 / screen
        );
        if name == "rain" {
            assert!(
                each < screen / 4,
                "{name}: {each} pixels a frame of {screen}"
            );
        }
    }
    Ok(())
}
