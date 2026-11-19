//! The screen light (#619, lot 1, docs/design/camera.md): while an
//! application uses the camera, the edges of its window's screen glow.
//!
//! One test per point of the spec: the real shell against a PipeWire of
//! the test's own (`levels/fakes.rs`), a camera's node and an
//! application's stream linked to it as a session manager would: the
//! light on the screen of that application's window, and gone with the
//! stream; never in a capture (`Print`); off in the configuration, none,
//! turned on live, it comes; with reduced motion, at once. Around the
//! call (2026-11-18): the other window and the wallpaper lit, not the
//! call's window nor the bar's column, the light following the window,
//! never in a capture. A share's
//! frames: `sharing::the_screen_light_is_never_in_a_share`; the Settings'
//! rows: `settings_appearance::the_camera_page_writes_the_screen_light_s_keys`.
//! The cameras' devices are files of the test's own, never `/dev/video*`.
//!
//! Pixman, a flat background: the light is told by its pixels.

use crate::common;
use crate::fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use common::{WAIT, chord, wait};
use fakes::Fakes;
use hocus_compositor::{
    Config, Headless, HeadlessConfig, LightShape, LightStrength, OutputSpec, RendererChoice,
    model::Rect,
};
use hocus_link::{CameraUser, CaptureAsk, CaptureMode};

const KEY_PRINT: u32 = 99;
const CALL: &str = "org.example.visio";
const SCREEN: (i32, i32) = (1920, 1080);

fn visio() -> Vec<CameraUser> {
    vec![CameraUser {
        app: "Visio".to_owned(),
        ids: vec!["visio".to_owned()],
    }]
}

/// The red of pixel (`x`, `y`) of screen `index` as it shows.
fn red(hp: &mut Headless, index: usize, (x, y): (i32, i32)) -> anyhow::Result<u8> {
    let (w, _, px) = hp.screenshot(index)?;
    let at = usize::try_from(y * w + x)? * 4;
    px.get(at + 2).copied().context("off the screen")
}

/// The colour of pixel (`x`, `y`) of screen `index` as it shows.
fn rgb(hp: &mut Headless, index: usize, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
    let (w, _, px) = hp.screenshot(index)?;
    let at = usize::try_from(y * w + x)? * 4;
    let p = px.get(at..at + 4).context("off the screen")?;
    Ok([p[2], p[1], p[0]])
}

/// 10 px in from the left edge (the bar is on the right), half way down:
/// in the soft light.
const EDGE: (i32, i32) = (10, SCREEN.1 / 2);

/// Runs until the light is full on screen `name` alone.
fn lit_on(hp: &mut Headless, name: &str) -> anyhow::Result<()> {
    let lit = hp.dispatch_until(WAIT, |hp| {
        let light = hp.screen_light();
        light.len() == 1 && light[0].0 == name && light[0].1 >= 1.0
    })?;
    anyhow::ensure!(lit, "not lit on {name}: {:?}", hp.screen_light());
    // A frame drawn as it is.
    wait(hp, Duration::from_millis(50))
}

/// A compositor without a shell on `outputs`, its pictures' folder in
/// `dir`.
fn alone(
    outputs: &[&str],
    dir: &Path,
    change: impl FnOnce(&mut Config),
) -> anyhow::Result<Headless> {
    let mut config = Config {
        plain_background: true,
        pictures: Some(dir.join("Pictures")),
        state_dir: Some(dir.join("state")),
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    change(&mut config);
    let outputs = outputs
        .iter()
        .map(|o| o.parse())
        .collect::<Result<Vec<OutputSpec>, _>>()?;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs,
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    anyhow::ensure!(drawn, "nothing drawn");
    Ok(hp)
}

/// The call's window on the second screen.
fn call_on_the_second(hp: &mut Headless) -> anyhow::Result<()> {
    common::open(hp, CALL, (800, 600))?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(CALL))
        .context("no call")?
        .id;
    let rect = Rect {
        x: SCREEN.0 + 400,
        y: 200,
        width: 800,
        height: 600,
    };
    hp.place_window(id, rect);
    common::wait_for_rect(hp, CALL, rect)
}

#[test]
fn the_light_is_on_the_call_s_screen_while_its_camera_is_used() -> anyhow::Result<()> {
    let shell = common::shell_program();
    if !fakes::available() {
        common::missing("dbus-daemon, pipewire, pw-dump, wpctl and pw-metadata");
    }
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config", "cache", "state", "data", "devices"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let mut fakes = Fakes::start()?;
    let at = |sub: &str| root.join(sub).display().to_string();
    let log = root.join("shell.log");
    let script = fakes.shell_with(
        &shell,
        "fr_FR.UTF-8",
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("XDG_DATA_DIRS", at("data")),
            ("HOCUS_VIDEO_DEVICES", at("devices")),
            ("ICED_BACKEND", "tiny-skia".to_owned()),
            ("TZ", "UTC".to_owned()),
            ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ],
        Some(&log),
    )?;
    let mut config = Config {
        plain_background: true,
        pipewire_remote: Some(fakes.pw_socket()),
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
        Some(fakes.address.clone()),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    call_on_the_second(&mut hp)?;
    // The pointer and the focus on the first screen: the call's window
    // chooses.
    hp.pointer_move(400.0, 600.0);
    common::open(&mut hp, "org.example.notes", (400, 300))?;
    let dark = red(&mut hp, 1, EDGE)?;
    // A camera, and the call's stream reading it.
    fakes.node(
        "hocus-test-camera",
        "Video/Source",
        "node.description = \"Caméra\"",
        true,
    )?;
    fakes.capture(
        "visio-camera",
        "Stream/Input/Video",
        "application.name = \"visio\"",
        "hocus-test-camera",
    )?;
    lit_on(&mut hp, "HEADLESS-2")?;
    let lit = red(&mut hp, 1, EDGE)?;
    let other = red(&mut hp, 0, EDGE)?;
    assert!(
        lit > dark + 50 && other < dark + 5,
        "the call's screen {lit} (dark {dark}), the other {other}\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    // The call stops its camera: the light goes.
    fakes.stop("visio-camera")?;
    let gone = hp.dispatch_until(WAIT, |hp| hp.screen_light().is_empty())?;
    anyhow::ensure!(gone, "the light stays: {:?}", hp.screen_light());
    wait(&mut hp, Duration::from_millis(50))?;
    assert!(red(&mut hp, 1, EDGE)? < dark + 5);
    Ok(())
}

#[test]
fn a_capture_of_the_lit_screen_has_no_light() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = alone(&["1920x1080"], dir.path(), |_| {})?;
    hp.camera_users(visio());
    lit_on(&mut hp, "HEADLESS-1")?;
    let shown = red(&mut hp, 0, EDGE)?;
    chord(&mut hp, &[KEY_PRINT]);
    hp.capture_ask(CaptureAsk::Mode(CaptureMode::Screen));
    hp.capture_ask(CaptureAsk::Shoot);
    let done = hp.dispatch_until(WAIT, |hp| hp.captures().0 >= 1)?;
    anyhow::ensure!(done, "no capture made");
    let file = hp.captures().1.context("no file written")?;
    let image = image::open(&file)?.to_rgba8();
    let [r, ..] = image.get_pixel(EDGE.0 as u32, EDGE.1 as u32).0;
    assert!(shown > 60 && r < 40, "the screen {shown}, the capture {r}");
    Ok(())
}

/// Window `app` opened and placed at `rect`; its id.
fn placed(hp: &mut Headless, app: &'static str, rect: Rect) -> anyhow::Result<()> {
    common::open(hp, app, (rect.width as u32, rect.height as u32))?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app))
        .context("no window")?
        .id;
    hp.place_window(id, rect);
    common::wait_for_rect(hp, app, rect)
}

/// Whether `lit` is `under` seen through the neutral light at strong's
/// alpha, within a few levels.
fn is_light(lit: [u8; 3], under: [u8; 3]) -> bool {
    let light = hocus_compositor::LightWarmth::Neutral.rgb();
    (0..3).all(|i| {
        let want = 0.97 * f64::from(light[i]) + 0.03 * f64::from(under[i]);
        (f64::from(lit[i]) - want).abs() <= 3.0
    })
}

fn same(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= 2)
}

/// Around the call, strong: the other window and the wallpaper of the
/// call's screen are the light, the call's window is untouched, and so
/// is the bar's column; the light rises from the call's edge; it follows
/// the window as it moves; a `Print` capture has none of it.
#[test]
fn around_the_call_the_light_covers_all_but_the_call_s_window() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = alone(&["1920x1080"], dir.path(), |c| {
        c.camera.screen_light_shape = LightShape::AroundCall;
        c.camera.screen_light_strength = LightStrength::Strong;
    })?;
    placed(
        &mut hp,
        "org.example.notes",
        Rect {
            x: 1100,
            y: 300,
            width: 600,
            height: 400,
        },
    )?;
    let call = Rect {
        x: 200,
        y: 200,
        width: 700,
        height: 600,
    };
    placed(&mut hp, CALL, call)?;
    common::at_rest(&mut hp)?;
    let bar = hp.work_area().context("no work area")?;
    anyhow::ensure!(bar.right() < SCREEN.0 - 40, "no bar's room: {bar:?}");
    let notes = (1400, 500);
    let in_call = (550, 500);
    let wall = (100, 950);
    let column = (SCREEN.0 - 6, 540);
    let by_the_call = (call.x - 4, 500);
    let points = [notes, in_call, wall, column, by_the_call];
    let mut before = Vec::new();
    for p in points {
        before.push(rgb(&mut hp, 0, p)?);
    }
    hp.camera_users(visio());
    lit_on(&mut hp, "HEADLESS-1")?;
    let mut after = Vec::new();
    for p in points {
        after.push(rgb(&mut hp, 0, p)?);
    }
    assert!(
        is_light(after[0], before[0]),
        "the other window {after:?} {before:?}"
    );
    assert!(same(after[1], before[1]), "the call {after:?} {before:?}");
    assert!(
        is_light(after[2], before[2]),
        "the wallpaper {after:?} {before:?}"
    );
    assert!(
        same(after[3], before[3]),
        "the bar's column {after:?} {before:?}"
    );
    assert!(
        after[4][0] + 60 < after[2][0],
        "right by the call, the light rises: {after:?}"
    );
    // A capture: the screen as it is under the light.
    chord(&mut hp, &[KEY_PRINT]);
    hp.capture_ask(CaptureAsk::Mode(CaptureMode::Screen));
    hp.capture_ask(CaptureAsk::Shoot);
    let done = hp.dispatch_until(WAIT, |hp| hp.captures().0 >= 1)?;
    anyhow::ensure!(done, "no capture made");
    let file = hp.captures().1.context("no file written")?;
    let image = image::open(&file)?.to_rgba8();
    for (i, p) in [notes, wall].into_iter().enumerate() {
        let [r, g, b, _] = image.get_pixel(p.0 as u32, p.1 as u32).0;
        let want = before[if i == 0 { 0 } else { 2 }];
        assert!(
            same([r, g, b], want),
            "captured {p:?}: {:?} {want:?}",
            [r, g, b]
        );
    }
    common::at_rest(&mut hp)?;
    // The call moves over the other window: the light follows it.
    let moved = Rect { x: 700, ..call };
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(CALL))
        .context("no call")?
        .id;
    hp.place_window(id, moved);
    common::wait_for_rect(&mut hp, CALL, moved)?;
    common::at_rest(&mut hp)?;
    wait(&mut hp, Duration::from_millis(50))?;
    let left = rgb(&mut hp, 0, (300, 500))?;
    assert!(is_light(left, before[2]), "where the call was: {left:?}");
    let now_call = rgb(&mut hp, 0, (1200, 500))?;
    assert!(same(now_call, before[1]), "where it is: {now_call:?}");
    Ok(())
}

#[test]
fn off_there_is_none_turned_on_it_comes_and_reduced_motion_is_at_once() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = alone(&["1920x1080"], dir.path(), |c| {
        c.camera.screen_light = false;
    })?;
    hp.camera_users(visio());
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(hp.screen_light().is_empty(), "{:?}", hp.screen_light());
    let dark = red(&mut hp, 0, EDGE)?;
    // Turned on in the file: it fades in.
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.pictures = Some(dir.path().join("Pictures"));
    hp.reconfigure(config.clone());
    hp.dispatch(Duration::from_millis(10))?;
    let start = hp.screen_light();
    assert!(
        start.len() == 1 && start[0].1 < 1.0,
        "it should fade in: {start:?}"
    );
    lit_on(&mut hp, "HEADLESS-1")?;
    assert!(red(&mut hp, 0, EDGE)? > dark + 50);
    // With reduced motion, the camera stopped: gone at once.
    config.appearance.reduced_motion = true;
    hp.reconfigure(config);
    hp.camera_users(Vec::new());
    hp.dispatch(Duration::from_millis(10))?;
    let levels = hp.screen_light();
    assert!(levels.iter().all(|(_, level)| *level == 0.0), "{levels:?}");
    Ok(())
}

/// The real shell for the maintainer's screenshots, in folders of
/// `root`'s, at a fixed time, without the session's buses or PipeWire.
fn shots_shell(root: &Path) -> anyhow::Result<PathBuf> {
    let shell = common::shell_program();
    for sub in ["home", "config", "cache", "state", "data"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let at = |sub: &str| root.join(sub).display().to_string();
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{}' XDG_CONFIG_HOME='{}' XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' \
             XDG_DATA_HOME='{}' TZ=UTC HOCUS_CLOCK_AT='2026-11-17T10:25:00' \
             HOCUS_USER_NAME=Alex LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             RUST_LOG=warn\nexec '{}' \"$@\"\n",
            at("home"),
            at("config"),
            at("cache"),
            at("state"),
            at("data"),
            shell.display(),
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

/// The maintainer's screenshots of the wide light (2026-11-18), into
/// `HOCUS_SHOTS`: one 1920×1080 screen, the real shell's bar, a call's
/// window and another one; the wide frame strong, around the call
/// strong, the wide frame soft, around the call soft.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_wide_light() -> anyhow::Result<()> {
    use hocus_compositor::Camera;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let dir = tempfile::tempdir()?;
    let script = shots_shell(dir.path())?;
    let base = |camera: Camera| {
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.camera = camera;
        config
    };
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        base(Camera::default()),
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    placed(
        &mut hp,
        "org.example.notes",
        Rect {
            x: 1000,
            y: 140,
            width: 600,
            height: 520,
        },
    )?;
    placed(
        &mut hp,
        CALL,
        Rect {
            x: 360,
            y: 240,
            width: 880,
            height: 620,
        },
    )?;
    hp.pointer_move(1000.0, 1000.0);
    hp.camera_users(visio());
    for (shape, strength, name) in [
        (LightShape::Frame, LightStrength::Strong, "a-frame-strong"),
        (
            LightShape::AroundCall,
            LightStrength::Strong,
            "b-around-call-strong",
        ),
        (LightShape::Frame, LightStrength::Soft, "c-frame-soft"),
        (
            LightShape::AroundCall,
            LightStrength::Soft,
            "d-around-call-soft",
        ),
    ] {
        hp.reconfigure(base(Camera {
            screen_light_shape: shape,
            screen_light_strength: strength,
            ..Camera::default()
        }));
        lit_on(&mut hp, "HEADLESS-1")?;
        common::at_rest(&mut hp)?;
        wait(&mut hp, Duration::from_millis(300))?;
        let (w, h, px) = hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("no image")?;
        let path = out.join(format!("screen-light-{name}.png"));
        image.save(&path)?;
        eprintln!("{}", path.display());
    }
    Ok(())
}

/// The maintainer's screenshots, into `HOCUS_SHOTS`: two screens, the
/// real shell's bar, a call's window on the second; the light at each
/// warmth and strength, and off.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_screen_light() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let dir = tempfile::tempdir()?;
    let script = shots_shell(dir.path())?;
    let base = |camera: hocus_compositor::Camera| {
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.camera = camera;
        config
    };
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        base(hocus_compositor::Camera::default()),
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    call_on_the_second(&mut hp)?;
    hp.pointer_move(3800.0, 1060.0);
    let save = |hp: &mut Headless, name: &str| -> anyhow::Result<()> {
        common::at_rest(hp)?;
        wait(hp, Duration::from_millis(300))?;
        let mut whole = image::RgbaImage::new(2 * SCREEN.0 as u32, SCREEN.1 as u32);
        for index in 0..2 {
            let (w, h, px) = hp.screenshot(index)?;
            let rgba: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect();
            let screen =
                image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("no image")?;
            image::imageops::replace(&mut whole, &screen, i64::from(SCREEN.0) * index as i64, 0);
        }
        let path = out.join(format!("{name}.png"));
        whole.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    save(&mut hp, "screen-light-none")?;
    hp.camera_users(visio());
    use hocus_compositor::{Camera, LightStrength as Strength, LightWarmth as Warmth};
    for (warmth, w) in [
        (Warmth::Cool, "cool"),
        (Warmth::Neutral, "neutral"),
        (Warmth::Warm, "warm"),
    ] {
        for (strength, s) in [
            (Strength::Soft, "soft"),
            (Strength::Medium, "medium"),
            (Strength::Strong, "strong"),
        ] {
            hp.reconfigure(base(Camera {
                screen_light: true,
                screen_light_warmth: warmth,
                screen_light_strength: strength,
                ..Camera::default()
            }));
            lit_on(&mut hp, "HEADLESS-2")?;
            save(&mut hp, &format!("screen-light-{w}-{s}"))?;
        }
    }
    // Turned off, the camera still used.
    hp.reconfigure(base(Camera {
        screen_light: false,
        ..Camera::default()
    }));
    let gone = hp.dispatch_until(WAIT, |hp| hp.screen_light().is_empty())?;
    anyhow::ensure!(gone, "the light stays");
    save(&mut hp, "screen-light-off")?;
    Ok(())
}
