//! The live wallpaper on the lock screen and in the overview (#807,
//! docs/design/effects.md), on the held clock and on GLES (Mesa's software
//! device, or the GPU `HOCUS_TEST_GPU` names): the snow falls behind the
//! lock and in the overview behind its dim; neither shows what is bound to
//! a window (the snow's caps on the windows' edges).
//!
//! The lock without the shell: the shell of the tests draws on the CPU,
//! whose surfaces are opaque, and the compositor then leaves the lock's
//! background to it. What the compositor draws under the lock is checked
//! alone; the screenshots run the shell on Mesa's software Vulkan.

use crate::common;

use std::{path::Path, time::Duration};

use anyhow::Context;
use common::{KEY_LEFTMETA, Shows, chord, open_showing, open_with, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);
const KEY_W: u32 = 17;

/// The windows (`common::effects_desk`'s): the snow's caps lie on their
/// top edges.
const NOTES: Rect = Rect {
    x: 110,
    y: 150,
    width: 430,
    height: 300,
};
const PAGE: Rect = Rect {
    x: 620,
    y: 300,
    width: 520,
    height: 380,
};

type Shot = (i32, i32, Vec<u8>);

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

/// How many pixels of `rect` differ between `a` and `b`.
fn changed(a: &Shot, b: &Shot, rect: Rect) -> usize {
    let mut n = 0;
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * a.0 + x) * 4) as usize;
            n += usize::from(a.2.get(i..i + 3) != b.2.get(i..i + 3));
        }
    }
    n
}

/// The share of `rect`'s pixels brighter than `level` (each channel).
fn bright(shot: &Shot, rect: Rect, level: u8) -> f64 {
    let mut n = 0;
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * shot.0 + x) * 4) as usize;
            n += usize::from(shot.2[i..i + 3].iter().all(|c| *c > level));
        }
    }
    n as f64 / f64::from(rect.width * rect.height)
}

/// The band along a window's top edge where its cap lies.
fn cap_band(window: Rect) -> Rect {
    Rect {
        x: window.x + 20,
        y: window.y - 14,
        width: window.width - 40,
        height: 14,
    }
}

/// On the left, away from the lock's form and the overview's windows.
const SIDE: Rect = Rect {
    x: 20,
    y: 40,
    width: 80,
    height: 500,
};

/// Snow for 4 s: caps on both windows.
fn snow_for_a_while(hp: &mut Headless, config: &mut Config) -> anyhow::Result<()> {
    config.effects.snow = true;
    hp.reconfigure(config.clone());
    run(hp, Duration::from_secs(4))?;
    let shot = hp.screenshot(0)?;
    for window in [NOTES, PAGE] {
        let caps = bright(&shot, cap_band(window), 200);
        anyhow::ensure!(caps > 0.2, "no cap on {window:?}: {caps}");
    }
    Ok(())
}

/// Locks, and steps the held clock until the lock shows, its fade over.
fn locked(hp: &mut Headless) -> anyhow::Result<()> {
    hp.lock();
    for _ in 0..600 {
        if hp.lock_drawn() {
            return Ok(());
        }
        hp.step(STEP)?;
    }
    anyhow::bail!("the lock never drew")
}

#[test]
fn the_snow_falls_behind_the_lock_and_no_cap_shows() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1440, 900)],
            ..common::gles()
        },
        config.clone(),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    let mut clients = Vec::new();
    for (app_id, rect) in [("test.notes", NOTES), ("test.page", PAGE)] {
        let size = (rect.width as u32, rect.height as u32);
        let (_, client) = open_with(&mut hp, app_id, size, false)?;
        clients.push(client);
        let id = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id))
            .map(|w| w.id)
            .context("no such window")?;
        hp.place_window(id, rect);
    }
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    snow_for_a_while(&mut hp, &mut config)?;
    locked(&mut hp)?;
    run(&mut hp, Duration::from_millis(300))?;
    let a = hp.screenshot(0)?;
    run(&mut hp, Duration::from_millis(300))?;
    let b = hp.screenshot(0)?;
    assert!(changed(&a, &b, SIDE) > 30, "the snow falls behind the lock");
    // The ground's pile at the foot.
    let foot = Rect {
        x: 400,
        y: 884,
        width: 600,
        height: 12,
    };
    assert!(bright(&b, foot, 180) > 0.5, "the ground's snow");
    // No cap where the windows' edges were: they are not shown.
    for window in [NOTES, PAGE] {
        let caps = bright(&b, cap_band(window), 200);
        assert!(caps < 0.05, "no cap on {window:?} behind the lock: {caps}");
    }
    Ok(())
}

#[test]
fn the_snow_falls_in_the_overview_and_no_cap_shows() -> anyhow::Result<()> {
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    snow_for_a_while(&mut hp, &mut config)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    anyhow::ensure!(hp.overview() == Some(true), "no overview");
    run(&mut hp, Duration::from_millis(500))?;
    let a = hp.screenshot(0)?;
    run(&mut hp, Duration::from_millis(300))?;
    let b = hp.screenshot(0)?;
    assert!(changed(&a, &b, SIDE) > 30, "the snow falls in the overview");
    // The windows are laid out elsewhere: no cap where their edges were.
    for window in [NOTES, PAGE] {
        let caps = bright(&b, cap_band(window), 200);
        assert!(caps < 0.05, "no cap on {window:?} in the overview: {caps}");
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

/// Mesa's software Vulkan alone: no GPU is looked at.
const LAVAPIPE: &str = "/usr/share/vulkan/icd.d/lvp_icd.json";

/// `common::effects_desk`, the shell drawing with wgpu on lavapipe, as on
/// a GPU: its surfaces transparent, so that the lock shows the wallpaper
/// live.
fn desk_on_lavapipe() -> anyhow::Result<common::Desk> {
    let home = tempfile::tempdir()?;
    let at = |sub: &str| home.path().join(sub).display().to_string();
    for sub in ["home", "config", "cache", "state", "data"] {
        std::fs::create_dir_all(home.path().join(sub))?;
    }
    let script = common::shell_script(
        home.path(),
        &common::shell_program(),
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("ICED_BACKEND", "wgpu".to_owned()),
            ("WGPU_BACKEND", "vulkan".to_owned()),
            ("VK_ICD_FILENAMES", LAVAPIPE.to_owned()),
            ("VK_DRIVER_FILES", LAVAPIPE.to_owned()),
        ],
    )?;
    let mut config = Config::default();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1440, 900)],
            ..common::gles()
        },
        config.clone(),
        Some(script),
    )?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    let up = hp.dispatch_until(Duration::from_secs(30), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    anyhow::ensure!(!hp.shell_on_cpu(), "the shell draws on the CPU");
    let mut clients = Vec::new();
    for (app_id, rect) in [("test.notes", NOTES), ("test.page", PAGE)] {
        let shows = Shows::Document {
            app_id,
            size: (rect.width as u32, rect.height as u32),
        };
        let (_, client) = open_showing(&mut hp, app_id, shows)?;
        clients.push(client);
        let id = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id))
            .map(|w| w.id)
            .context("no such window")?;
        hp.place_window(id, rect);
    }
    wait(&mut hp, Duration::from_secs(2))?;
    hp.hold_clock();
    Ok((hp, config, clients, home))
}

/// The live wallpaper for the maintainer, written to `$HOCUS_SHOTS` when
/// it names a folder, on GLES, with the shell (wgpu on lavapipe): the
/// overview with the snow, then with the koi; the lock with the snow,
/// then with the landscape in depth too. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = desk_on_lavapipe()?;
    snow_for_a_while(&mut hp, &mut config)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    run(&mut hp, Duration::from_millis(600))?;
    save(&mut hp, &out, "overview-snow")?;
    config.effects.snow = false;
    config.effects.koi = true;
    hp.reconfigure(config.clone());
    run(&mut hp, Duration::from_millis(1500))?;
    save(&mut hp, &out, "overview-koi")?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    run(&mut hp, Duration::from_millis(600))?;
    config.effects.koi = false;
    config.effects.snow = true;
    hp.reconfigure(config.clone());
    run(&mut hp, Duration::from_secs(2))?;
    locked(&mut hp)?;
    // The shell's form, drawn a while after on lavapipe.
    wait(&mut hp, Duration::from_secs(2))?;
    run(&mut hp, Duration::from_millis(600))?;
    save(&mut hp, &out, "lock-snow")?;
    config.effects.depth = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_millis(600))?;
    save(&mut hp, &out, "lock-snow-depth")?;
    Ok(())
}
