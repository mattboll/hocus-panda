//! The curtain's screenshots for its pull request (7.2, issue #413): the
//! real shell on the CPU, a window and the bar under the curtain, 3 s and
//! 7 s into it (the panda awake, then asleep). Written to `$HOCUS_SHOTS`
//! when it names a folder; run by hand (`--run-ignored only`). The clock
//! runs: the shell counts down on its own.

use crate::common;

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use common::WAIT;
use hocus_compositor::{Config, Headless, HeadlessConfig, IdleSettings, RendererChoice};

fn shoot(hp: &mut Headless, out: &std::path::Path, name: &str) -> anyhow::Result<()> {
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
    // The pill, ×2.
    let pill = hp
        .curtain_surfaces()
        .first()
        .map(|(_, r)| *r)
        .context("no pill")?;
    // Logical pixels, the screen at 1.5.
    let at = |v: i32| (f64::from(v) * 1.5).round() as u32;
    let (x, y) = (at(pill.x - 40), at(pill.y - 40));
    let (cw, ch) = (at(380), at(144));
    let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let zoomed = image::imageops::resize(
        &crop,
        cw * 4 / 3,
        ch * 4 / 3,
        image::imageops::FilterType::CatmullRom,
    );
    let file = out.join(format!("{name}-zoom.png"));
    zoomed.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let shell = common::shell_program();
    std::fs::create_dir_all(&out)?;
    let home = tempfile::tempdir()?;
    let at = |sub: &str| home.path().join(sub).display().to_string();
    for sub in ["home", "config", "cache", "state", "data"] {
        std::fs::create_dir_all(home.path().join(sub))?;
    }
    let script = common::shell_script(
        home.path(),
        &shell,
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("HOCUS_USER_NAME", "Alex".to_owned()),
        ],
    )?;
    let mut config = Config {
        idle: IdleSettings {
            delay: Some(Duration::from_secs(4)),
        },
        ..Config::default()
    };
    config.keyboard.layouts = vec!["fr".to_owned()];
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.5".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    let (_, _window) = common::open_with(&mut hp, "test.window", (1100, 700), false)?;
    hp.pointer_move(1300.0, 500.0);
    let came = hp.dispatch_until(WAIT, |hp| {
        !hp.drawn_layers(hocus_link::CURTAIN_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(came, "no curtain");
    common::wait(&mut hp, Duration::from_millis(2500))?;
    shoot(&mut hp, &out, "curtain")?;
    common::wait(&mut hp, Duration::from_millis(4500))?;
    shoot(&mut hp, &out, "curtain-asleep")?;
    Ok(())
}
