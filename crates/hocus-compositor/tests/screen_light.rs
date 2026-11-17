//! The screen light (#619, lot 1, docs/design/camera.md): while an
//! application uses the camera, the edges of its window's screen glow.
//!
//! One test per point of the spec: the real shell against a PipeWire of
//! the test's own (`levels/fakes.rs`), a camera's node and an
//! application's stream linked to it as a session manager would: the
//! light on the screen of that application's window, and gone with the
//! stream; never in a capture (`Print`); off in the configuration, none,
//! turned on live, it comes; with reduced motion, at once. A share's
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
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};
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

/// 10 px in from the right edge, half way down: in the soft light, off
/// the bar.
const EDGE: (i32, i32) = (SCREEN.0 - 10, SCREEN.1 / 2);

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
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
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
