//! The screenshots of 8.4 and 8.5 B for the maintainer (issues #420,
//! #490): the real shell on the CPU, away from the machine's services, a
//! 16:10 screen with the bar and a 21:9 one beside it. The bar's two rows
//! of tiles; then a window dragged over the wide screen's third tile.
//! Written to `$HOCUS_SHOTS` when it names a folder; run by hand
//! (`--run-ignored only`).

use crate::common;

use std::{path::Path, time::Duration};

use anyhow::Context;
use common::{
    BTN_LEFT, KEY_1, KEY_2, KEY_3, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, WAIT, chord, wait,
};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice};

/// The first screen, and its bar's desktops' corner ×2 beside it.
fn shoot(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    wait(hp, Duration::from_millis(400))?;
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    let (_, tiles) = hp
        .desktops_surfaces()
        .into_iter()
        .next()
        .context("no desktops' surface")?;
    let (x, y) = ((tiles.x - 300).max(0) as u32, (tiles.y - 120).max(0) as u32);
    let (cw, ch) = ((w as u32 - x), (h as u32 - y));
    let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let zoomed = image::imageops::resize(
        &crop,
        cw * 2,
        ch * 2,
        image::imageops::FilterType::CatmullRom,
    );
    let file = out.join(format!("{name}-zoom.png"));
    zoomed.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

#[test]
#[ignore = "screenshots for the maintainer, by hand"]
fn the_bars_rows_for_two_screens_and_a_window_over_a_tile() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS") else {
        eprintln!("HOCUS_SHOTS names no folder");
        return Ok(());
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let at = |sub: &str| -> anyhow::Result<String> {
        let path = dir.path().join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path.display().to_string())
    };
    let env = [
        ("HOME", at("home")?),
        ("XDG_CONFIG_HOME", at("config")?),
        ("XDG_CACHE_HOME", at("cache")?),
        ("XDG_STATE_HOME", at("state")?),
        ("XDG_DATA_HOME", at("data")?),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
        ("HOCUS_USER_NAME", "Alex".to_owned()),
    ];
    let script = common::shell_script(dir.path(), &shell, &env)?;
    let mut config = Config::default();
    config.panel.form = FormChoice::Full;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1200".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(up, "no desktops' tiles");
    // The laptop: a window on the left half, another floating; the wide
    // screen: one on its first desktop, two on its second, which it shows.
    hp.pointer_move(960.0, 600.0);
    common::open(&mut hp, "test.editor", (900, 700))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
    let (floating, _) = common::open_with(&mut hp, "test.terminal", (700, 480), false)?;
    hp.pointer_move(1920.0 + 1720.0, 720.0);
    common::open(&mut hp, "test.browser", (1600, 1000))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    common::open(&mut hp, "test.chat", (1000, 900))?;
    common::open(&mut hp, "test.notes", (700, 600))?;
    hp.pointer_move(960.0, 600.0);
    let told = hp.dispatch_until(WAIT, |hp| hp.desk_tile_middles().len() == 8)?;
    anyhow::ensure!(told, "tiles: {:?}", hp.desk_tile_middles());
    shoot(&mut hp, &out, "two-rows")?;
    // The terminal dragged over the wide screen's third tile.
    let to = hp
        .desk_tile_middles()
        .into_iter()
        .find(|(_, output, desktop, _)| output == "HEADLESS-2" && *desktop == 2)
        .map(|(.., middle)| middle)
        .context("no third tile")?;
    // Held near its right edge: small, it shows left of the bar.
    let (x, y) = (
        f64::from(floating.x + floating.width - 60),
        f64::from(floating.y + 40),
    );
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTALT, true);
    hp.button(BTN_LEFT, true);
    for step in 1..=24 {
        let t = f64::from(step) / 24.0;
        hp.pointer_move(x + (to.0 - x) * t, y + (to.1 - y) * t);
        hp.dispatch(Duration::from_millis(16))?;
    }
    let over = hp.dispatch_until(WAIT, |hp| hp.drop_over().is_some())?;
    anyhow::ensure!(over, "never over the tile");
    shoot(&mut hp, &out, "drag-over-tile")?;
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTALT, false);
    shoot(&mut hp, &out, "dropped")?;
    Ok(())
}

/// #947's screenshots: the laptop's panel (2560 × 1600 at 1.25) with the
/// bar, the 34-inch screen on its right; linked, a click on the 34-inch's
/// row's third tile puts both on their desktop 3, both rows' rings
/// moving. Written to `$HOCUS_SHOTS`; run by hand.
#[test]
#[ignore = "screenshots for the maintainer, by hand"]
fn the_bars_rows_after_a_switch_of_linked_screens() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS") else {
        eprintln!("HOCUS_SHOTS names no folder");
        return Ok(());
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let at = |sub: &str| -> anyhow::Result<String> {
        let path = dir.path().join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path.display().to_string())
    };
    let env = [
        ("HOME", at("home")?),
        ("XDG_CONFIG_HOME", at("config")?),
        ("XDG_CACHE_HOME", at("cache")?),
        ("XDG_STATE_HOME", at("state")?),
        ("XDG_DATA_HOME", at("data")?),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
        ("HOCUS_USER_NAME", "Alex".to_owned()),
    ];
    let script = common::shell_script(dir.path(), &shell, &env)?;
    let mut config = Config::default();
    config.panel.form = FormChoice::Full;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.25".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(up, "no desktops' tiles");
    // Desktop 1: a terminal on the laptop, a browser on the 34-inch;
    // desktop 3: an editor and a chat on the laptop, two windows there.
    let (laptop, wide) = ((1024.0, 640.0), (2048.0 + 1720.0, 720.0));
    hp.pointer_move(laptop.0, laptop.1);
    common::open(&mut hp, "test.terminal", (900, 600))?;
    hp.pointer_move(wide.0, wide.1);
    common::open(&mut hp, "test.browser", (2000, 1100))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    common::open(&mut hp, "test.notes", (1200, 900))?;
    common::open(&mut hp, "test.docs", (1000, 800))?;
    hp.pointer_move(laptop.0, laptop.1);
    common::open(&mut hp, "test.editor", (1400, 900))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    let told = hp.dispatch_until(WAIT, |hp| hp.desk_tile_middles().len() == 8)?;
    anyhow::ensure!(told, "tiles: {:?}", hp.desk_tile_middles());
    shoot(&mut hp, &out, "linked-before")?;
    let to = hp
        .desk_tile_middles()
        .into_iter()
        .find(|(_, output, desktop, _)| output == "HEADLESS-2" && *desktop == 2)
        .map(|(.., middle)| middle)
        .context("no third tile")?;
    common::click_on(&mut hp, to, hocus_link::DESKTOPS_NAMESPACE)?;
    let both = hp.dispatch_until(WAIT, |hp| {
        hp.strips().iter().all(|(_, _, current)| *current == 2)
    })?;
    anyhow::ensure!(both, "{:?}", hp.strips());
    hp.pointer_move(laptop.0, laptop.1);
    shoot(&mut hp, &out, "linked-after")?;
    let (w, h, px) = hp.screenshot(1)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let file = out.join("linked-after-wide.png");
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}
