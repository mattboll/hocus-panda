//! The screenshots of 8.4 and 8.5 B for the maintainer (issues #420,
//! #490): the real shell on the CPU, away from the machine's services, a
//! 16:10 screen with the bar and a 21:9 one beside it. The bar's two rows
//! of tiles; then a window dragged over the wide screen's third tile.
//! Written to `$HOCUS_SHOTS` when it names a folder; run by hand
//! (`--run-ignored only`).

use crate::common;

use std::{path::Path, time::Duration};

use anyhow::Context;
use common::{BTN_LEFT, KEY_2, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, WAIT, chord, wait};
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
