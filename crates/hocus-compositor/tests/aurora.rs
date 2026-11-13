//! Aurora following the music (#801, docs/design/effects.md), on the held
//! clock and on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU`
//! names): the wallpaper turns to the aurora, the window's pixels
//! untouched; still while no music plays, nothing drawn frame after
//! frame; while it plays, drawn again 60 times a second at most on a
//! 165 Hz screen; the music stopped, it calms to still within seconds.
//! That the shell says whether its player plays: `music::`.

use crate::common;

use std::{path::Path, time::Duration};

use common::{KEY_LEFTMETA, chord, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

const KEY_W: u32 = 17;
/// A frame on a 165 Hz screen.
const STEP: Duration = Duration::from_nanos(6_060_606);
const WINDOW: Rect = Rect {
    x: 260,
    y: 160,
    width: 420,
    height: 260,
};

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
/// rest, the aurora off; the clock held.
fn bed() -> anyhow::Result<(Headless, common::ClientHandle)> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(960, 600)],
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
        "test.aurora",
        (WINDOW.width as u32, WINDOW.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.aurora"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, WINDOW);
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    run(&mut hp, STEP * 2)?;
    Ok((hp, client))
}

type Shot = (i32, i32, Vec<u8>);

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

/// The sky, under the bar's height, right of the window.
const SKY: Rect = Rect {
    x: 700,
    y: 60,
    width: 220,
    height: 380,
};

#[test]
fn it_ripples_while_the_music_plays_at_60_frames_a_second_never_over_the_window()
-> anyhow::Result<()> {
    let (mut hp, _client) = bed()?;
    let before = hp.screenshot(0)?;
    let mut config = Config::default();
    config.effects.aurora = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_millis(100))?;
    let (_, pace, _) = hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?;
    assert_eq!(pace, 0.0, "still without music");
    let still = hp.screenshot(0)?;
    let (kept, all) = same(&before, &still, SKY);
    assert!(kept < all / 10, "the aurora: {kept} of {all} kept");
    let (kept, all) = same(&before, &still, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    // No music: nothing drawn per frame.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn per frame");

    // The music: a second of a 165 Hz screen.
    hp.set_music(true);
    let (moved, (_, _, drawn)) = (
        hp.effect_frames(),
        hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?,
    );
    run(&mut hp, Duration::from_secs(1))?;
    let moved = hp.effect_frames() - moved;
    let (_, pace, now) = hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?;
    let drawn = now - drawn;
    eprintln!("a second at 165 Hz: {moved} effect frames, the aurora drawn {drawn} times");
    assert!((57..=61).contains(&moved), "{moved} effect frames");
    assert!((55..=61).contains(&drawn), "drawn {drawn} times");
    assert!(pace > 0.9, "awake: {pace}");
    let playing = hp.screenshot(0)?;
    let (kept, all) = same(&still, &playing, SKY);
    assert!(kept < all / 2, "it moved: {kept} of {all} kept");
    let (kept, all) = same(&before, &playing, inside(WINDOW));
    assert_eq!(kept, all, "the window's pixels untouched");
    Ok(())
}

#[test]
fn the_music_stopped_it_calms_to_still() -> anyhow::Result<()> {
    let (mut hp, _client) = bed()?;
    let mut config = Config::default();
    config.effects.aurora = true;
    hp.reconfigure(config);
    hp.set_music(true);
    run(&mut hp, Duration::from_secs(1))?;
    hp.set_music(false);
    // A moment on, still calming.
    run(&mut hp, Duration::from_millis(500))?;
    let (_, pace, _) = hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?;
    assert!(pace > 0.2, "calming, not stopped: {pace}");
    run(&mut hp, Duration::from_millis(3500))?;
    let (time, pace, drawn) = hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?;
    assert_eq!(pace, 0.0, "calmed");
    let a = hp.screenshot(0)?;
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn per frame");
    let (later, _, again) = hp.aurora(0).ok_or_else(|| anyhow::anyhow!("no aurora"))?;
    assert_eq!((later, again), (time, drawn), "still");
    let b = hp.screenshot(0)?;
    let (kept, all) = same(&a, &b, SKY);
    assert_eq!(kept, all, "the aurora as it stood");
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

/// The aurora for the maintainer, written to `$HOCUS_SHOTS` when it names
/// a folder, on GLES, with the shell's bar and two windows: no music;
/// the music playing, at two moments; stopped and calmed; in the
/// overview, the music playing. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    config.effects.aurora = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_millis(200))?;
    save(&mut hp, &out, "aurora-no-music")?;
    hp.set_music(true);
    run(&mut hp, Duration::from_secs(2))?;
    save(&mut hp, &out, "aurora-music-playing")?;
    run(&mut hp, Duration::from_millis(330))?;
    save(&mut hp, &out, "aurora-music-playing-later")?;
    hp.set_music(false);
    run(&mut hp, Duration::from_secs(6))?;
    save(&mut hp, &out, "aurora-music-stopped")?;
    hp.set_music(true);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    run(&mut hp, Duration::from_millis(800))?;
    save(&mut hp, &out, "aurora-overview")?;
    Ok(())
}
