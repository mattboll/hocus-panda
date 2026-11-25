//! What a frame costs on a big screen, on GLES (Mesa's software device, or
//! the GPU `HOCUS_TEST_GPU` names): the effects that play on their own at
//! 30 frames a second (the garland across the top, the creatures, the
//! fireflies) step
//! together, on one clock, so that a 50 Hz screen draws at most 30 frames
//! a second for them, not one for each; what a frame of a 3440 × 1440
//! screen with the bar's glass and the effects costs the loop; and what a
//! laptop panel's scene gives the damage tracker (#948).

use crate::common;

use std::{path::Path, process::Command, time::Duration};

use common::{Shows, WAIT, open_showing};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

/// A frame of a 50 Hz screen.
const AT_50_HZ: Duration = Duration::from_millis(20);

/// `hocus agent NAME STATE` on `hp`'s control socket, from a child of the
/// test's process (whose windows are the agent's).
fn hocus(hp: &Headless, args: &[&str]) -> anyhow::Result<()> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let status = Command::new(common::hocus_program())
        .arg("--socket")
        .arg(socket)
        .args(args)
        .status()?;
    anyhow::ensure!(status.success(), "hocus {args:?}: {status}");
    Ok(())
}

/// The garland across the top and the creatures, and `more`.
fn slow_effects(more: impl FnOnce(&mut Config)) -> Config {
    let mut config = Config::default();
    config.effects.garland_top = true;
    config.effects.creatures = true;
    more(&mut config);
    config
}

/// Two windows of text at `rects`, an agent working in the first, its
/// panda sitting on it.
fn windows(hp: &mut Headless, rects: [Rect; 2]) -> anyhow::Result<Vec<common::ClientHandle>> {
    let mut clients = Vec::new();
    for (app_id, rect) in [("test.notes", rects[0]), ("test.page", rects[1])] {
        let (_, client) = open_showing(
            hp,
            app_id,
            Shows::Document {
                app_id,
                size: (rect.width as u32, rect.height as u32),
            },
        )?;
        clients.push(client);
        let id = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id))
            .map(|w| w.id)
            .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))?;
        hp.place_window(id, rect);
    }
    hocus(hp, &["agent", "build", "working"])?;
    let sits = hp.dispatch_until(WAIT, |hp| !hp.creatures_sitting().is_empty())?;
    anyhow::ensure!(sits, "no creature");
    Ok(clients)
}

/// The garland across the top and a panda, on one 1280 × 800 screen
/// drawing at 50 Hz: they step on one clock, at most 30 times a second,
/// a frame drawn for each step of both, not one for each of theirs.
#[test]
fn the_garland_and_the_creatures_step_together() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        slow_effects(|_| {}),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    let _clients = windows(
        &mut hp,
        [Rect::new(200, 300, 500, 300), Rect::new(760, 360, 420, 300)],
    )?;
    anyhow::ensure!(hp.garland_tops().len() == 1, "no garland across the top");
    // On the machine's clock: the loop wakes for their steps itself, ten
    // frames' worth however long a loaded machine takes, and never more
    // than 30 a second of the time it took (a count over a second of the
    // wall clock failed on a machine too busy to draw ten).
    common::wait(&mut hp, Duration::from_millis(500))?;
    let frames = hp.frames_on(0);
    let started = std::time::Instant::now();
    let woke = hp.dispatch_until(WAIT, |hp| hp.frames_on(0) >= frames + 10)?;
    let took = started.elapsed();
    let real = hp.frames_on(0) - frames;
    anyhow::ensure!(woke, "{real} frames in {took:?}");
    // On the held clock, a 50 Hz screen's frames.
    hp.hold_clock();
    for _ in 0..25 {
        hp.step(AT_50_HZ)?;
    }
    let frames = hp.frames_on(0);
    for _ in 0..50 {
        hp.step(AT_50_HZ)?;
    }
    let held = hp.frames_on(0) - frames;
    eprintln!("{real} frames in {took:?} on the machine's clock, {held} a second at 50 Hz");
    assert!(
        real as f64 <= took.as_secs_f64() * 30.0 + 1.0,
        "{real} frames in {took:?}"
    );
    assert!((10..=31).contains(&held), "{held} frames at 50 Hz");
    Ok(())
}

/// In the evening, the fireflies over a 3440 × 1440 screen, with the
/// garland across the top and a panda: all three step on one clock, at
/// most 30 times a second, so that a 50 Hz screen draws at most 30 frames
/// a second for them, not one each turn; and the fireflies' glows, moving
/// all over its lower part, tell the box around them as changed, not
/// hundreds of rectangles the frame draws each element in one by one.
#[test]
fn the_fireflies_step_with_the_garland_and_the_creatures() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(3440, 1440)],
            ..common::gles()
        },
        slow_effects(|c| c.effects.fireflies = true),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    hp.hold_evening(true);
    let _clients = windows(
        &mut hp,
        [
            Rect::new(400, 300, 1200, 800),
            Rect::new(1800, 250, 1100, 900),
        ],
    )?;
    // Out over 3 s, on the machine's clock.
    common::wait(&mut hp, Duration::from_secs(3))?;
    let flies = hp.fireflies("HEADLESS-1").map_or(0, |f| f.len());
    anyhow::ensure!(flies > 300, "{flies} fireflies");
    anyhow::ensure!(hp.garland_tops().len() == 1, "no garland across the top");
    hp.hold_clock();
    for _ in 0..50 {
        hp.step(AT_50_HZ)?;
    }
    hp.take_frame_cost(0);
    hp.take_damage_area(0);
    hp.take_damage_rects(0);
    let frames = hp.frames_on(0);
    for _ in 0..100 {
        hp.step(AT_50_HZ)?;
    }
    let frames = hp.frames_on(0) - frames;
    let cost = hp.take_frame_cost(0);
    let area = hp.take_damage_area(0);
    let rects = hp.take_damage_rects(0);
    let screen = 3440 * 1440;
    let each = area / frames.max(1);
    // What the loop's thread spent on the screen's scenes, for the record
    // (on Mesa's software device, the draws' vertices are its work too).
    eprintln!(
        "in the evening, 2 s at 50 Hz: {frames} frames, {flies} fireflies; a frame: {} \
         rectangles, {each} pixels ({} %); {} ms building the scenes, {} ms rendering",
        rects / frames.max(1),
        each * 100 / screen,
        cost.build_ns / 1_000_000,
        cost.render_ns / 1_000_000,
    );
    // 60 of the 100 turns at most, a few more for the windows' own.
    assert!((20..=63).contains(&frames), "{frames} frames in 2 s");
    assert!(
        rects / frames.max(1) <= 8,
        "{rects} rectangles in {frames} frames"
    );
    Ok(())
}

/// The maintainer's desk: a 3440 × 1440 screen, the bar on its right in
/// Givre (the shell on wgpu, on Mesa's software Vulkan) over the aurora,
/// two floating windows, an agent's panda on one, and the effects on:
/// the garland across the top, the creatures, the fireflies, the
/// daylight, desktops in depth, the flames, the jelly. What a frame of it
/// costs the loop, as a 50 Hz screen draws.
#[test]
fn a_frame_of_a_big_screen() -> anyhow::Result<()> {
    let Some(manifest) = lavapipe() else {
        common::missing("Mesa's software Vulkan driver, lavapipe (mesa-vulkan-drivers)");
    };
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config/hocuspanda", "state", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/hocuspanda/config.toml");
    std::fs::write(
        &file,
        "[keyboard]\nlayouts = [\"us\"]\n\n[appearance]\ntheme = \"frost\"\naccent = \"ice\"\n\n\
         [wallpaper]\npicture = \"aurora\"\n\n[panel]\nside = \"right\"\n\n\
         [effects]\ngarland-top = true\ncreatures = true\nfireflies = true\ndaylight = true\n\
         depth = true\nflames = true\njelly = true\n",
    )?;
    let at = |sub: &str| root.join(sub).display().to_string();
    let script = common::shell_script(
        root,
        &common::shell_program(),
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("TZ", "UTC".to_owned()),
            ("ICED_BACKEND", "wgpu".to_owned()),
            ("WGPU_BACKEND", "vulkan".to_owned()),
            ("VK_DRIVER_FILES", manifest.to_owned()),
            ("VK_ICD_FILENAMES", manifest.to_owned()),
        ],
    )?;
    let mut config = Config::load(&file);
    config.file = Some(file.clone());
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(3440, 1440)],
            ..common::gles()
        },
        config,
        Some(script),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    // Midday, whatever the clock says: the fireflies' evening is measured
    // above, its bounds its own.
    hp.hold_evening(false);
    hp.hold_sun("2026-11-16T12:00:00Z".parse()?);
    let up = hp.dispatch_until(Duration::from_secs(60), |hp| {
        !hp.drawn_layers(hocus_link::NAMESPACE).is_empty() && !hp.shell_on_cpu()
    })?;
    anyhow::ensure!(up, "no bar on the GPU");
    let _clients = windows(
        &mut hp,
        [
            Rect::new(400, 300, 1200, 800),
            Rect::new(1800, 250, 1100, 900),
        ],
    )?;
    common::wait(&mut hp, Duration::from_secs(3))?;
    // The bar held still over what is counted, as drawn: it draws on its
    // own clock, the machine's, so the frames its commits brought grew
    // with the time a loaded machine took over the 100 turns (68, then 80
    // and more on CI). Its glass and its surfaces are drawn all the same.
    hp.hold_shell(true)?;
    let measured = measure_held(&mut hp);
    hp.hold_shell(false)?;
    let Held {
        frames,
        blurs,
        wall,
        cost,
        area,
        rects,
    } = measured?;
    let scenes = cost.scenes.max(1);
    let screen = 3440 * 1440;
    let each = area / frames.max(1);
    // What the loop's thread spent, for the record: on Mesa's software
    // device, the draws' vertices are its work too.
    eprintln!(
        "2 s at 50 Hz: {frames} frames drawn of {} scenes; a scene: {} µs building \
         ({} elements), {} µs rendering; a frame drawn: {} rectangles, {each} pixels \
         ({} %); the glass blurred {blurs} times; {} ms on the clock",
        cost.scenes,
        cost.build_ns / 1000 / scenes,
        cost.elements / scenes,
        cost.render_ns / 1000 / scenes,
        rects / frames.max(1),
        each * 100 / screen,
        wall.as_millis(),
    );
    anyhow::ensure!(hp.garland_tops().len() == 1, "no garland across the top");
    // The garland and the panda, 30 times a second at most: 60 frames of
    // the 100 turns, not every one, a few more for the windows' own.
    assert!((20..=63).contains(&frames), "{frames} frames in 2 s");
    // The garland's strip and the panda's box, not the screen.
    assert!(each * 100 / screen <= 15, "{each} pixels a frame");
    assert!(
        rects / frames.max(1) <= 8,
        "{rects} rectangles in {frames} frames"
    );
    // Nothing moves behind the bar: its glass stays as it was blurred.
    assert_eq!(blurs, 0, "the glass blurred again");
    Ok(())
}

/// A frame of a laptop's 165 Hz panel (#948).
const AT_165_HZ: Duration = Duration::from_micros(6061);

/// A laptop's desk (#948): its 2560 × 1600 panel at scale 2, a 3440 × 1440
/// screen beside it, three opaque windows of text on the panel. What a
/// scene of the panel gives the damage tracker, which subtracts every
/// opaque rectangle in front of an element from it, for each element,
/// every frame: a few rectangles a rounded window, its corners' squares
/// left out whole, not one a pixel row of each corner (thirty a window
/// at this scale, and the loop's largest cost of its own).
#[test]
fn a_scene_gives_the_damage_tracker_a_few_rectangles_a_window() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![
                OutputSpec::new(2560, 1600).scale(2.0),
                OutputSpec::new(3440, 1440),
            ],
            ..common::gles()
        },
        Config::default(),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    let mut clients = Vec::new();
    for (app_id, rect) in [
        ("test.terminal", Rect::new(60, 80, 640, 480)),
        ("test.notes", Rect::new(560, 160, 600, 420)),
        ("test.page", Rect::new(240, 320, 700, 400)),
    ] {
        let (_, client) = open_showing(
            &mut hp,
            app_id,
            Shows::Document {
                app_id,
                size: (rect.width as u32, rect.height as u32),
            },
        )?;
        client.commands.send(common::Command::Opaque)?;
        clients.push(client);
        let id = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id))
            .map(|w| w.id)
            .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))?;
        hp.place_window(id, rect);
    }
    // Until every window's opaque region is in the scene: three windows'
    // worth of opaque rectangles over the wallpaper's.
    let mut opaque = 0;
    for _ in 0..200 {
        hp.step(AT_165_HZ)?;
        let cost = hp.take_frame_cost(0);
        opaque = cost.opaque / cost.scenes.max(1);
        if opaque >= 4 {
            break;
        }
    }
    anyhow::ensure!(opaque >= 4, "the windows never said they were opaque");
    for _ in 0..10 {
        hp.step(AT_165_HZ)?;
    }
    hp.take_frame_cost(0);
    for _ in 0..60 {
        hp.step(AT_165_HZ)?;
    }
    let cost = hp.take_frame_cost(0);
    let scenes = cost.scenes.max(1);
    let (elements, opaque) = (cost.elements / scenes, cost.opaque / scenes);
    eprintln!(
        "{} scenes of the panel: {elements} elements, {opaque} opaque rectangles a scene",
        cost.scenes
    );
    anyhow::ensure!(cost.scenes >= 60, "{} scenes in 60 steps", cost.scenes);
    // Five a window at most (its opaque region less its corners'
    // squares), and the wallpaper's.
    assert!(opaque <= 3 * 5 + 1, "{opaque} opaque rectangles a scene");
    Ok(())
}

/// What 2 s of a 50 Hz screen on the held clock drew, after 1 s of it.
struct Held {
    frames: u64,
    blurs: u64,
    /// The time they took on the machine's clock, for the record.
    wall: Duration,
    cost: hocus_compositor::FrameCost,
    area: u64,
    rects: u64,
}

fn measure_held(hp: &mut Headless) -> anyhow::Result<Held> {
    hp.hold_clock();
    for _ in 0..50 {
        hp.step(AT_50_HZ)?;
    }
    hp.take_frame_cost(0);
    hp.take_damage_area(0);
    hp.take_damage_rects(0);
    let frames = hp.frames_on(0);
    let blurs = hp.glass_blurs();
    let wall = std::time::Instant::now();
    for _ in 0..100 {
        hp.step(AT_50_HZ)?;
    }
    Ok(Held {
        frames: hp.frames_on(0) - frames,
        blurs: hp.glass_blurs() - blurs,
        wall: wall.elapsed(),
        cost: hp.take_frame_cost(0),
        area: hp.take_damage_area(0),
        rects: hp.take_damage_rects(0),
    })
}

/// Mesa's software Vulkan driver's manifest, where Debian puts it.
fn lavapipe() -> Option<&'static str> {
    [
        "/usr/share/vulkan/icd.d/lvp_icd.json",
        "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json",
        "/usr/share/vulkan/icd.d/lvp_icd.aarch64.json",
    ]
    .into_iter()
    .find(|p| Path::new(p).exists())
}
