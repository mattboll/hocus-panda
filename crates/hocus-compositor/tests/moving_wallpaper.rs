//! The wallpapers that move (#433, docs/design/settings.md 6.3), on the
//! held clock, on GLES (the GPU `HOCUS_TEST_GPU` names, else Mesa's
//! software device; they fail without either) and on pixman: their pace,
//! when they stop, the glass over them, and what they cost.

use crate::common;

use std::time::Duration;

use common::{Command, Shows, open_showing, open_with};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OpenSize, OutputSpec, RendererChoice, WallpaperOnBattery,
    WindowRule,
};
use hocus_link::wallpaper::Picture;

fn allocations(run: impl FnOnce()) -> u64 {
    common::counting::allocations(run).1
}

const STEP: Duration = Duration::from_micros(16_667);
const OUTPUT: &str = "HEADLESS-1";

fn moving(picture: &str) -> Config {
    let mut config = Config::default();
    config.wallpaper.picture = Picture::of(picture).unwrap_or_default();
    config
}

fn sized(renderer: HeadlessConfig, sizes: &[(i32, i32)]) -> HeadlessConfig {
    HeadlessConfig {
        outputs: sizes.iter().map(|(w, h)| OutputSpec::new(*w, *h)).collect(),
        ..renderer
    }
}

/// A compositor on GLES showing `config`'s wallpaper, or `None` (said)
/// where there is no GLES.
fn gles(config: Config, sizes: &[(i32, i32)]) -> anyhow::Result<Option<Headless>> {
    let hp = Headless::start_with(sized(common::gles(), sizes), config)?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    Ok(Some(hp))
}

/// Steps the held clock `steps` refreshes at 60 Hz: the frames drawn on
/// output 0 meanwhile.
fn frames(hp: &mut Headless, steps: u32) -> anyhow::Result<u64> {
    let before = hp.frames_on(0);
    for _ in 0..steps {
        hp.step(STEP)?;
    }
    Ok(hp.frames_on(0) - before)
}

#[test]
fn the_rain_and_the_stars_run_at_their_own_pace() -> anyhow::Result<()> {
    for (picture, fps) in [("rain", 15u64), ("stars", 20), ("aurora", 10)] {
        let Some(mut hp) = gles(moving(picture), &[(640, 400)])? else {
            return Ok(());
        };
        hp.hold_clock();
        hp.step(STEP)?;
        assert!(hp.wall_running(OUTPUT), "{picture}");
        let a = hp.screenshot(0)?;
        let drawn = frames(&mut hp, 120)?;
        let b = hp.screenshot(0)?;
        // 2 s of the held clock.
        let expected = 2 * fps;
        assert!(
            drawn.abs_diff(expected) <= 1,
            "{picture}: {drawn} frames in 2 s"
        );
        assert_ne!(a.2, b.2, "{picture}: successive frames differ");
        // Its own colours: the rain's green over black, the stars' blue.
        let green =
            b.2.chunks(4)
                .filter(|p| p[1] > 150 && i32::from(p[1]) > i32::from(p[2]) + 60)
                .count();
        if picture == "rain" {
            assert!(green > 100, "{green} green pixels");
        }
    }
    Ok(())
}

/// Frames and the wallpaper's own frames and wakes over 60 s of the held
/// clock, stepped by `step`; and the allocations of a step that drew the
/// wallpaper over `rest`, those of a step that drew nothing: the most of
/// any (gated), and their means in the minute's second tenth and in its
/// last two (said only).
struct Minute {
    frames: u64,
    ticks: u64,
    wakes: u64,
    most: u64,
    means: (f64, f64),
}

fn a_minute(hp: &mut Headless, step: Duration, rest: u64) -> anyhow::Result<Minute> {
    let (frames, ticks, wakes) = (hp.frames_on(0), hp.wall_ticks(OUTPUT), hp.wall_wakes());
    let steps = (Duration::from_secs(60).as_nanos() / step.as_nanos()) as u32;
    let mut drawing = Vec::new();
    for k in 0..steps {
        let before = hp.frames_on(0);
        let mut result = Ok(());
        let n = allocations(|| result = hp.step(step));
        result?;
        if hp.frames_on(0) > before {
            drawing.push((k, n));
        }
    }
    let over = |n: &u64| n.saturating_sub(rest);
    let most = drawing.iter().map(|(_, n)| over(n)).max().unwrap_or(0);
    let picked = |from: u32, to: u32| -> Vec<u64> {
        drawing
            .iter()
            .filter(|(k, _)| (from..to).contains(k))
            .map(|(_, n)| over(n))
            .collect()
    };
    let mean = |p: Vec<u64>| p.iter().sum::<u64>() as f64 / p.len().max(1) as f64;
    let tenth = steps / 10;
    let (early, late) = ((tenth, 2 * tenth), (8 * tenth, steps));
    Ok(Minute {
        frames: hp.frames_on(0) - frames,
        ticks: hp.wall_ticks(OUTPUT) - ticks,
        wakes: hp.wall_wakes() - wakes,
        most,
        means: (mean(picked(early.0, early.1)), mean(picked(late.0, late.1))),
    })
}

/// A minute of `picture`, on battery or not: at most `most` frames, and
/// at most 30 allocations over a turn of the loop at rest on every frame
/// of the minute. Stepped a frame of its own at a time.
///
/// Not gated: allocations growing from the minute's start to its end.
/// A step takes a turn of the loop more or fewer as the real clock goes
/// on a loaded machine, some ten allocations each way: on CI a drawn
/// frame's fewest came out at 0 or 3 early against 13 late, and a mean
/// over 30 frames moved by 2, the pace exact. Comparing a few allocations
/// early and late reads the machine's load, not the wallpaper. What grows
/// with its frames still fails the cap: 30 over rest on every one of its
/// 300 to 1 200 frames, the most seen 24 or 25.
fn a_minute_of(picture: &str, battery: bool, most: u64) -> anyhow::Result<()> {
    let Some(mut hp) = gles(moving(picture), &[(320, 200)])? else {
        return Ok(());
    };
    hp.set_on_battery(battery);
    hp.hold_clock();
    let fps: u32 = match picture {
        "rain" => 15,
        "aurora" => 10,
        _ => 20,
    };
    let step = Duration::from_secs(1) / if battery { fps } else { 2 * fps };
    // A turn at rest: the same steps, the wallpaper still.
    let mut still = moving(picture);
    still.wallpaper.animate = false;
    hp.reconfigure(still);
    hp.step(step)?;
    let mut resting: Vec<u64> = (0..20)
        .map(|_| {
            let mut result = Ok(());
            let n = allocations(|| result = hp.step(step));
            result.map(|()| n)
        })
        .collect::<anyhow::Result<_>>()?;
    resting.sort_unstable();
    let rest = resting[resting.len() / 2];
    hp.reconfigure(moving(picture));
    hp.step(step)?;
    let minute = a_minute(&mut hp, step, rest)?;
    eprintln!(
        "{picture}, battery {battery}: {} frames, {} of its own, {} wakes; \
         allocations of a frame over a step at rest ({rest}): {} at most, means {:?}",
        minute.frames, minute.ticks, minute.wakes, minute.most, minute.means
    );
    assert!(
        minute.frames <= most,
        "{picture} {battery}: {}",
        minute.frames
    );
    assert!(minute.frames >= most * 9 / 10, "it ran: {}", minute.frames);
    assert!(
        minute.most <= 30,
        "{}, means {:?}",
        minute.most,
        minute.means
    );
    Ok(())
}

#[test]
fn a_minute_of_rain_costs_902_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("rain", false, 902)
}

#[test]
fn a_minute_of_stars_costs_1202_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("stars", false, 1202)
}

#[test]
fn a_minute_of_rain_on_battery_costs_482_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("rain", true, 482)
}

#[test]
fn a_minute_of_aurora_costs_602_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("aurora", false, 602)
}

#[test]
fn a_minute_of_aurora_on_battery_costs_302_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("aurora", true, 302)
}

#[test]
fn a_minute_of_stars_on_battery_costs_602_frames_at_most() -> anyhow::Result<()> {
    a_minute_of("stars", true, 602)
}

#[test]
fn covered_it_stops_and_costs_nothing() -> anyhow::Result<()> {
    let mut config = moving("rain");
    config.windows.rules.push(WindowRule {
        app_id: "test.big".into(),
        desktop: None,
        size: OpenSize::Maximized,
        above: false,
    });
    let Some(mut hp) = gles(config, &[(1280, 800)])? else {
        return Ok(());
    };
    let (_, client) = open_with(&mut hp, "test.big", (400, 300), false)?;
    hp.hold_clock();
    for _ in 0..30 {
        hp.step(STEP)?;
    }
    let covered = hp.covered(OUTPUT);
    assert!(covered >= 0.9, "{covered}");
    assert!(!hp.wall_running(OUTPUT));
    let minute = a_minute(&mut hp, Duration::from_millis(50), 0)?;
    assert_eq!(
        (minute.frames, minute.ticks, minute.wakes),
        (0, 0, 0),
        "covered for 60 s"
    );
    // Uncovered, it goes on within a frame of its own.
    let _ = client.commands.send(Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.windows().is_empty())?;
    let drawn = frames(&mut hp, 60)?;
    assert!(hp.wall_running(OUTPUT));
    assert!(drawn >= 14, "{drawn} frames in 1 s");
    Ok(())
}

#[test]
fn it_stops_when_nobody_sees_it_and_goes_on_from_there() -> anyhow::Result<()> {
    let Some(mut hp) = gles(moving("rain"), &[(640, 400)])? else {
        return Ok(());
    };
    hp.hold_clock();
    hp.step(STEP)?;
    assert!(hp.wall_running(OUTPUT));
    // Each pause, a few frames into it: nothing moves; then back.
    let check = |hp: &mut Headless, what: &str, on: bool| -> anyhow::Result<()> {
        let ticks = hp.wall_ticks(OUTPUT);
        for _ in 0..30 {
            hp.step(STEP)?;
        }
        let moved = hp.wall_ticks(OUTPUT) > ticks;
        assert_eq!(moved, on, "{what}");
        assert_eq!(hp.wall_running(OUTPUT), on, "{what}");
        Ok(())
    };
    // The power saver.
    hp.set_power_profile(Some("power-saver"));
    check(&mut hp, "power saver", false)?;
    hp.set_power_profile(Some("balanced"));
    check(&mut hp, "balanced", true)?;
    // On battery, still.
    let mut still = moving("rain");
    still.wallpaper.on_battery = WallpaperOnBattery::Still;
    hp.reconfigure(still.clone());
    hp.set_on_battery(true);
    check(&mut hp, "on battery, still", false)?;
    hp.set_on_battery(false);
    check(&mut hp, "on mains", true)?;
    // Reduced motion; animate = false.
    let mut reduced = still.clone();
    reduced.appearance.reduced_motion = true;
    hp.reconfigure(reduced);
    check(&mut hp, "reduced motion", false)?;
    let mut off = still.clone();
    off.wallpaper.animate = false;
    hp.reconfigure(off);
    check(&mut hp, "animate = false", false)?;
    hp.reconfigure(still);
    check(&mut hp, "animate", true)?;
    // Two minutes without input, until the next.
    hp.step(Duration::from_secs(121))?;
    check(&mut hp, "2 min without input", false)?;
    hp.pointer_move(100.0, 100.0);
    check(&mut hp, "input", true)?;
    // A fullscreen window.
    let shows = Shows::Asking {
        app_id: "test.full",
        size: (400, 300),
        fullscreen: true,
    };
    let (_, client) = open_showing(&mut hp, "test.full", shows)?;
    check(&mut hp, "fullscreen", false)?;
    let _ = client.commands.send(Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.windows().is_empty())?;
    check(&mut hp, "fullscreen gone", true)?;
    // Locked.
    hp.lock();
    check(&mut hp, "locked", false)?;
    Ok(())
}

#[test]
fn each_screen_decides_alone() -> anyhow::Result<()> {
    let mut config = moving("stars");
    config.windows.rules.push(WindowRule {
        app_id: "test.big".into(),
        desktop: None,
        size: OpenSize::Maximized,
        above: false,
    });
    let Some(mut hp) = gles(config, &[(1280, 800), (640, 400)])? else {
        return Ok(());
    };
    hp.pointer_move(400.0, 300.0);
    let (_, _client) = open_with(&mut hp, "test.big", (400, 300), false)?;
    hp.hold_clock();
    // Its opening over.
    for _ in 0..30 {
        hp.step(STEP)?;
    }
    let before = (hp.frames_on(0), hp.frames_on(1));
    for _ in 0..60 {
        hp.step(STEP)?;
    }
    assert!(!hp.wall_running("HEADLESS-1"));
    assert!(hp.wall_running("HEADLESS-2"));
    let drawn = (hp.frames_on(0) - before.0, hp.frames_on(1) - before.1);
    assert_eq!(drawn.0, 0, "{drawn:?}");
    assert!(drawn.1 >= 19, "{drawn:?}");
    Ok(())
}

/// The bar's glass over a moving wallpaper blurs a still copy of it, taken
/// as the bar comes and again every 2 minutes while it moves: at most one
/// blur in a minute, five in ten, and at least one more in ten, so that the
/// glass never drifts far from what moves beside it.
#[test]
fn the_glass_over_it_is_blurred_every_two_minutes_at_most() -> anyhow::Result<()> {
    let mut config = moving("rain");
    config.panel.form = hocus_compositor::FormChoice::Full;
    let Some(mut hp) = gles(config, &[(1280, 800)])? else {
        return Ok(());
    };
    std::mem::forget(common::client(hp.socket(), Some(Shows::ClearBar))?);
    let barred = hp.dispatch_until(common::WAIT, |hp| {
        hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
    })?;
    anyhow::ensure!(barred, "no bar surface");
    hp.hold_clock();
    for _ in 0..30 {
        hp.step(STEP)?;
    }
    assert!(hp.glass_on());
    let blurs = hp.glass_blurs();
    assert!(blurs >= 1, "the glass was blurred");
    let before = hp.frames_on(0);
    for _ in 0..1200 {
        hp.step(Duration::from_millis(50))?;
    }
    let drawn = hp.frames_on(0) - before;
    assert!(drawn >= 850, "it ran: {drawn}");
    let minute = hp.glass_blurs() - blurs;
    assert!(minute <= 1, "{minute} blurs in 60 s");
    // Nine minutes more, a frame of its own at a time; a move of the
    // pointer each minute, or it rests after 2 min without input.
    for k in 0..9 * 60 * 15 / 4 {
        if k % 225 == 0 {
            hp.pointer_move(100.0 + f64::from(k % 7), 100.0);
        }
        hp.step(Duration::from_millis(267))?;
    }
    let ten = hp.glass_blurs() - blurs;
    eprintln!("the glass blurred {minute} times in 60 s, {ten} in 10 min");
    assert!((1..=5).contains(&ten), "{ten} blurs in 10 min");
    Ok(())
}

/// On pixman: the still picture, drawn once, and nothing after.
#[test]
fn without_gles_the_still_picture_is_drawn_once() -> anyhow::Result<()> {
    for picture in ["rain", "stars", "aurora"] {
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec![OutputSpec::new(640, 400)],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            moving(picture),
        )?;
        common::at_rest(&mut hp)?;
        // The thread's still picture shows.
        let drawn = hp.dispatch_until(common::WAIT, |hp| {
            hp.state()
                .backgrounds
                .borrow()
                .shown
                .values()
                .any(|s| s.picture == (640, 400))
        })?;
        assert!(drawn, "{picture}: its still picture");
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        let before = hp.frames_on(0);
        let draws = hp.state().backgrounds.borrow().draws;
        for _ in 0..1200 {
            hp.step(Duration::from_millis(50))?;
        }
        assert_eq!(hp.frames_on(0), before, "{picture}: nothing moves");
        assert_eq!(hp.state().backgrounds.borrow().draws, draws);
        assert_eq!(hp.wall_ticks(OUTPUT), 0);
        let shot = hp.screenshot(0)?;
        let lit = shot
            .2
            .chunks(4)
            .filter(|p| p[0].max(p[1]).max(p[2]) > 120)
            .count();
        assert!(lit > 20, "{picture}: {lit} lit pixels");
    }
    Ok(())
}

/// The wallpapers for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder: each as the compositor draws it on GLES (two frames a
/// second apart), and its still picture (pixman, the lock, the tiles).
/// Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots_of_the_wallpapers() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |hp: &mut Headless, name: &str| -> anyhow::Result<()> {
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
    };
    for picture in ["rain", "stars", "aurora"] {
        let mut config = moving(picture);
        config.panel.form = hocus_compositor::FormChoice::Full;
        let mut hp = Headless::start_with(sized(common::gles(), &[(1440, 900)]), config.clone())?;
        anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
        std::mem::forget(common::client(hp.socket(), Some(Shows::ClearBar))?);
        hp.dispatch_until(common::WAIT, |hp| {
            hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
        })?;
        hp.hold_clock();
        hp.step(STEP)?;
        save(&mut hp, &format!("{picture}-gles-a"))?;
        for _ in 0..60 {
            hp.step(STEP)?;
        }
        save(&mut hp, &format!("{picture}-gles-b"))?;
        let mut still = Headless::start_with(
            HeadlessConfig {
                outputs: vec![OutputSpec::new(1440, 900)],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        still.dispatch_until(common::WAIT, |hp| {
            hp.state()
                .backgrounds
                .borrow()
                .shown
                .values()
                .any(|s| s.picture == (1440, 900))
        })?;
        common::at_rest(&mut still)?;
        save(&mut still, &format!("{picture}-still"))?;
    }
    Ok(())
}

/// What a frame of each wallpaper takes on the GPU at 3440 × 1440, beside
/// a frame of the still one: reported, never a gate (wall-clock times
/// depend on the machine). On the GPU `HOCUS_TEST_GPU` names, else on
/// Mesa's software device. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "timings, by hand"]
fn timings_of_the_wallpapers() -> anyhow::Result<()> {
    for picture in ["rain", "stars", "aurora"] {
        let Some(mut hp) = gles(moving(picture), &[(3440, 1440)])? else {
            return Ok(());
        };
        hp.hold_clock();
        let fps = hocus_link::moving::Moving::of(picture).map_or(10, |m| m.fps());
        let step = Duration::from_secs(1) / fps;
        // Each frame read back, so that the GPU's work is in it.
        let time = |hp: &mut Headless, frames: u32| -> anyhow::Result<Duration> {
            let start = std::time::Instant::now();
            for _ in 0..frames {
                hp.step(step)?;
                hp.screenshot(0)?;
            }
            Ok(start.elapsed() / frames)
        };
        hp.step(step)?;
        let moving_frame = time(&mut hp, 30)?;
        let mut still = moving(picture);
        still.wallpaper.animate = false;
        hp.reconfigure(still);
        hp.step(step)?;
        let still_frame = time(&mut hp, 30)?;
        eprintln!(
            "{picture} at 3440 × 1440 on {}: {:.2} ms a frame, {:.2} ms still: {:.2} ms its own",
            if hocus_compositor::test_gpu::node().is_some() {
                "the GPU"
            } else {
                "llvmpipe"
            },
            moving_frame.as_secs_f64() * 1e3,
            still_frame.as_secs_f64() * 1e3,
            moving_frame.saturating_sub(still_frame).as_secs_f64() * 1e3,
        );
    }
    Ok(())
}

/// The day's recorder (#655) hears when a moving wallpaper starts moving
/// and stops: written with the next sample.
#[test]
fn the_days_record_says_when_it_moves_and_stops() -> anyhow::Result<()> {
    use hocus_perf::{Day, EventKind, files};
    let dir = tempfile::tempdir()?;
    let config = Config {
        state_dir: Some(dir.path().to_owned()),
        perf: hocus_compositor::PerfSettings {
            record: true,
            interval: Some(Duration::from_millis(50)),
        },
        ..moving("aurora")
    };
    let Some(mut hp) = gles(config, &[(320, 200)])? else {
        return Ok(());
    };
    let said = |dir: &std::path::Path| -> Vec<(EventKind, String)> {
        let dir = files::dir_in(dir);
        files::days(&dir)
            .into_iter()
            .filter_map(|date| Day::read(&dir, date).ok())
            .flat_map(|day| day.samples)
            .flat_map(|s| s.e)
            .map(|e| (e.kind, e.what))
            .collect()
    };
    let moving_said = hp.dispatch_until(common::WAIT, |_| {
        said(dir.path()).contains(&(EventKind::WallpaperMoving, "aurora".to_owned()))
    })?;
    assert!(moving_said, "{:?}", said(dir.path()));
    hp.set_power_profile(Some("power-saver"));
    let still_said = hp.dispatch_until(common::WAIT, |_| {
        said(dir.path()).contains(&(EventKind::WallpaperStill, "aurora".to_owned()))
    })?;
    assert!(still_said, "{:?}", said(dir.path()));
    Ok(())
}

/// The still copy of a moving wallpaper only feeds the glass: beside the
/// bar, in its gap to the screen's edges and outside its rounded corners,
/// the screen shows the moving wallpaper exactly as a screen without a bar
/// does at the same moment; under the bar, the glass's blur of the still
/// copy. Each wallpaper, two compositors stepped alike.
#[test]
fn the_still_copy_only_feeds_the_glass() -> anyhow::Result<()> {
    for picture in ["rain", "stars", "aurora"] {
        let mut config = moving(picture);
        config.panel.form = hocus_compositor::FormChoice::Full;
        let Some(mut barred) = gles(config.clone(), &[(1280, 800)])? else {
            return Ok(());
        };
        let Some(mut bare) = gles(config, &[(1280, 800)])? else {
            return Ok(());
        };
        barred.hold_clock();
        bare.hold_clock();
        std::mem::forget(common::client(barred.socket(), Some(Shows::ClearBar))?);
        let drawn = barred.dispatch_until(common::WAIT, |hp| {
            hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
        })?;
        anyhow::ensure!(drawn, "no bar surface");
        let bar = barred.bar_surfaces()[0].1;
        for _ in 0..90 {
            barred.step(STEP)?;
            bare.step(STEP)?;
        }
        // The same frame of the wallpaper on both: each began on its own
        // real clock before its clock was held, so on a loaded machine one
        // may be a frame behind; the one behind steps on until they agree.
        for _ in 0..20 {
            let (a, b) = (barred.wall_ticks(OUTPUT), bare.wall_ticks(OUTPUT));
            match a.cmp(&b) {
                std::cmp::Ordering::Less => barred.step(STEP)?,
                std::cmp::Ordering::Greater => bare.step(STEP)?,
                std::cmp::Ordering::Equal => break,
            }
        }
        assert_eq!(
            barred.wall_ticks(OUTPUT),
            bare.wall_ticks(OUTPUT),
            "{picture}: the same frame on both"
        );
        assert!(barred.glass_on(), "{picture}");
        let (a, b) = (barred.screenshot(0)?, bare.screenshot(0)?);
        let at = |shot: &(i32, i32, Vec<u8>), x: i32, y: i32| {
            let i = ((y * shot.0 + x) * 4) as usize;
            [shot.2[i], shot.2[i + 1], shot.2[i + 2]]
        };
        let (right, bottom) = (bar.x + bar.width, bar.y + bar.height);
        let mut beside = Vec::new();
        // Left of the bar, every fourth pixel.
        for y in (0..800).step_by(4) {
            for x in (0..bar.x - 1).step_by(4) {
                beside.push((x, y));
            }
        }
        // Its gaps: to the right edge, above and below it.
        for y in (0..800).step_by(2) {
            for x in right + 1..1280 {
                beside.push((x, y));
            }
        }
        for x in (bar.x..right).step_by(2) {
            for y in (0..bar.y - 1).chain(bottom + 1..800) {
                beside.push((x, y));
            }
        }
        // Outside its rounded corners.
        for (x, y) in [
            (bar.x, bar.y),
            (right - 1, bar.y),
            (bar.x, bottom - 1),
            (right - 1, bottom - 1),
        ] {
            beside.push((x, y));
        }
        let differ: Vec<_> = beside
            .iter()
            .filter(|(x, y)| at(&a, *x, *y) != at(&b, *x, *y))
            .take(5)
            .collect();
        assert!(differ.is_empty(), "{picture}, bar {bar:?}: {differ:?}");
        // Under the bar: the glass, not the moving wallpaper.
        let under = (bar.y + 40..bottom - 40)
            .step_by(4)
            .flat_map(|y| (bar.x + 20..right - 20).step_by(4).map(move |x| (x, y)))
            .filter(|(x, y)| at(&a, *x, *y) != at(&b, *x, *y))
            .count();
        assert!(
            under > 100,
            "{picture}: the glass shows ({under} pixels differ)"
        );
    }
    Ok(())
}

/// Between two of its frames, a moving wallpaper is drawn again only
/// where something over it changed, never over what did not: with the
/// desktops' dots changing at the top of the screen and the pointer
/// moving over the bar, the bar shows whole at every frame, the glass's
/// still copy taken again meanwhile (a popover comes). Drawn over
/// the bounds of those scattered places, the wallpaper covered the bar
/// between them until its next frame (the bar flickering, or gone a
/// moment, on a desktop switch or as a window passes).
#[test]
fn the_bar_shows_whole_between_the_wallpapers_frames() -> anyhow::Result<()> {
    const BAR: u32 = 0xffe0_a020;
    for picture in ["rain", "aurora"] {
        let Some(mut hp) = gles(moving(picture), &[(1280, 800)])? else {
            return Ok(());
        };
        std::mem::forget(common::client(
            hp.socket(),
            Some(Shows::Layer {
                namespace: hocus_link::NAMESPACE,
                size: (196, 0),
                margin: (8, 8),
                colour: BAR,
            }),
        )?);
        let placed = hp.dispatch_until(common::WAIT, |hp| {
            hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
        })?;
        anyhow::ensure!(placed, "no bar surface");
        common::wait(&mut hp, Duration::from_millis(200))?;
        let bar = hp.bar_surfaces()[0].1;
        hp.hold_clock();
        hp.pointer_move(400.0, 400.0);
        for _ in 0..10 {
            hp.step(STEP)?;
        }
        let ticks = hp.wall_ticks(OUTPUT);
        // To the next desktop: its dots show at the top of the screen.
        common::chord(
            &mut hp,
            &[
                common::KEY_LEFTMETA,
                common::KEY_LEFTCTRL,
                common::KEY_RIGHT,
            ],
        );
        let mut partial = 0;
        let blurs = hp.glass_blurs();
        for i in 0..60 {
            if i == 20 {
                std::mem::forget(common::client(
                    hp.socket(),
                    Some(Shows::Layer {
                        namespace: hocus_link::POPOVER_NAMESPACE,
                        size: (280, 200),
                        // Away from the bar: its shadow falls on it beside it.
                        margin: (300, 700),
                        colour: 0xff20_c060,
                    }),
                )?);
            }
            let pointer = (
                f64::from(bar.x + 30 + (i % 10) * 12),
                f64::from(200 + i * 8),
            );
            hp.pointer_move(pointer.0, pointer.1);
            hp.step(STEP)?;
            if hp
                .take_damage(0)
                .is_some_and(|d| d.width < 1280 || d.height < 800)
            {
                partial += 1;
            }
            let (width, _, shot) = hp.screenshot(0)?;
            let wrong: Vec<(i32, i32)> = (bar.y + 20..bar.y + bar.height - 20)
                .step_by(10)
                .flat_map(|y| {
                    (bar.x + 16..bar.x + bar.width - 16)
                        .step_by(10)
                        .map(move |x| (x, y))
                })
                // Away from the pointer.
                .filter(|(x, y)| {
                    (f64::from(*x) - pointer.0).abs() > 48.0
                        || (f64::from(*y) - pointer.1).abs() > 48.0
                })
                .filter(|(x, y)| {
                    let at = ((y * width + x) * 4) as usize;
                    let [b, g, r] = [shot[at], shot[at + 1], shot[at + 2]];
                    [r, g, b]
                        .iter()
                        .zip([(BAR >> 16) as u8, (BAR >> 8) as u8, BAR as u8])
                        .any(|(seen, want)| seen.abs_diff(want) > 3)
                })
                .take(5)
                .collect();
            assert!(
                wrong.is_empty(),
                "{picture}, frame {i}: the bar does not show at {wrong:?}"
            );
        }
        assert!(hp.wall_ticks(OUTPUT) > ticks, "{picture}: it moved");
        assert!(
            hp.glass_blurs() > blurs,
            "{picture}: the still copy was not taken again"
        );
        assert!(
            partial > 10,
            "{picture}: {partial} frames drew a part of the screen"
        );
    }
    Ok(())
}

/// The aurora for the maintainer, written to `$HOCUS_SHOTS`: without a
/// bar, a second apart; under the bar's glass (a clear surface stands for
/// the shell's bar, so the glass shows alone); its still picture. Run by
/// hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots_of_the_aurora() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |hp: &mut Headless, name: &str| -> anyhow::Result<()> {
        let (w, h, px) = hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        image.save(out.join(format!("{name}.png")))?;
        Ok(())
    };
    let mut config = moving("aurora");
    config.panel.form = hocus_compositor::FormChoice::Full;
    let mut hp = Headless::start_with(sized(common::gles(), &[(1440, 900)]), config.clone())?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    hp.hold_clock();
    hp.step(STEP)?;
    save(&mut hp, "aurora-v2-a")?;
    for _ in 0..60 {
        hp.step(STEP)?;
    }
    save(&mut hp, "aurora-v2-b")?;
    for _ in 0..600 {
        hp.step(Duration::from_millis(50))?;
    }
    save(&mut hp, "aurora-v2-c-30s-later")?;
    std::mem::forget(common::client(hp.socket(), Some(Shows::ClearBar))?);
    hp.dispatch_until(common::WAIT, |hp| {
        hp.bar_surfaces().first().is_some_and(|(_, r)| r.width > 0)
    })?;
    hp.step(STEP)?;
    save(&mut hp, "aurora-v2-under-the-glass")?;
    let mut still = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1440, 900)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    still.dispatch_until(common::WAIT, |hp| {
        hp.state()
            .backgrounds
            .borrow()
            .shown
            .values()
            .any(|s| s.picture == (1440, 900))
    })?;
    common::at_rest(&mut still)?;
    save(&mut still, "aurora-v2-still")
}
