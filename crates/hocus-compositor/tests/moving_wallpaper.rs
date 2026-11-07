//! The wallpapers that move (#433, docs/design/settings.md 6.3), on the
//! held clock, on GLES (the GPU `HOCUS_TEST_GPU` names, else Mesa's
//! software device; skipped without either) and on pixman: their pace,
//! when they stop, the glass over them, and what they cost.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Duration,
};

use common::{Command, Shows, open_showing, open_with};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OpenSize, OutputSpec, RendererChoice, WallpaperOnBattery,
    WindowRule,
};
use hocus_link::wallpaper::Picture;

/// Counts the allocations of the thread that asks, while it asks: the
/// loop runs on the test's thread, the clients on theirs.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNTED: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: every call goes to the system's allocator; counting touches
// thread-locals of plain integers only, which never allocate.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNTING.try_with(|on| {
            if on.get() {
                let _ = COUNTED.try_with(|n| n.set(n.get() + 1));
            }
        });
        // SAFETY: as the caller promised us.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller promised us.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations(run: impl FnOnce()) -> u64 {
    COUNTED.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    run();
    COUNTING.with(|on| on.set(false));
    COUNTED.with(Cell::get)
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
        eprintln!("no GLES here: the wallpapers do not move");
        return Ok(None);
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
    for (picture, fps) in [("rain", 15u64), ("stars", 20)] {
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
/// clock, stepped by `step`; and the most allocations of a step that drew
/// the wallpaper over `rest`, those of a step that drew nothing, early in
/// the minute and late.
struct Minute {
    frames: u64,
    ticks: u64,
    wakes: u64,
    over_rest: (u64, u64),
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
    let most = |from: u32, to: u32| {
        drawing
            .iter()
            .filter(|(k, _)| (from..to).contains(k))
            .map(|(_, n)| n.saturating_sub(rest))
            .max()
            .unwrap_or(0)
    };
    let tenth = steps / 10;
    Ok(Minute {
        frames: hp.frames_on(0) - frames,
        ticks: hp.wall_ticks(OUTPUT) - ticks,
        wakes: hp.wall_wakes() - wakes,
        over_rest: (most(tenth, 2 * tenth), most(8 * tenth, steps)),
    })
}

/// A minute of `picture`, on battery or not: at most `most` frames, and
/// at most 30 allocations a frame over a turn of the loop at rest, none
/// growing over the minute. Stepped a frame of its own at a time.
fn a_minute_of(picture: &str, battery: bool, most: u64) -> anyhow::Result<()> {
    let Some(mut hp) = gles(moving(picture), &[(320, 200)])? else {
        return Ok(());
    };
    hp.set_on_battery(battery);
    hp.hold_clock();
    let fps: u32 = if picture == "rain" { 15 } else { 20 };
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
         allocations of a frame over a step at rest ({rest}): {:?}",
        minute.frames, minute.ticks, minute.wakes, minute.over_rest
    );
    assert!(
        minute.frames <= most,
        "{picture} {battery}: {}",
        minute.frames
    );
    assert!(minute.frames >= most * 9 / 10, "it ran: {}", minute.frames);
    assert!(minute.over_rest.0 <= 30, "{:?}", minute.over_rest);
    assert!(
        minute.over_rest.1 <= minute.over_rest.0 + 2,
        "{:?}",
        minute.over_rest
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

/// The bar's glass over a moving wallpaper blurs a still picture of it:
/// once, then never again for its frames.
#[test]
fn the_glass_over_it_is_blurred_once() -> anyhow::Result<()> {
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
    assert_eq!(hp.glass_blurs(), blurs, "no blur again in 60 s");
    Ok(())
}

/// On pixman: the still picture, drawn once, and nothing after.
#[test]
fn without_gles_the_still_picture_is_drawn_once() -> anyhow::Result<()> {
    for picture in ["rain", "stars"] {
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
    for picture in ["rain", "stars"] {
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
    for picture in ["rain", "stars"] {
        let Some(mut hp) = gles(moving(picture), &[(3440, 1440)])? else {
            return Ok(());
        };
        hp.hold_clock();
        let fps: u32 = if picture == "rain" { 15 } else { 20 };
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
