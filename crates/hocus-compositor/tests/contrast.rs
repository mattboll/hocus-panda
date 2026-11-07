//! High contrast for what the compositor draws itself (#423, bar.md §13
//! "High contrast"): the radial menu's disc and the overview's thumbnails
//! take the contrast table's opaque background (near black on dark, white
//! on light), their faint fills and tints gone; without it, their own dark
//! colours. Pixels of a headless screen, on the CPU.

mod common;

use std::time::Duration;

use common::{KEY_LEFTMETA, Shows, chord, open_showing, wait};
use hocus_compositor::{Appearance, Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::theme::Scheme;

const BTN_RIGHT: u32 = 0x111;
const KEY_W: u32 = 17;
/// The disc's band between the sectors' ring and its rim: its background
/// alone (`hocus_compositor::radial`).
const BAND: f64 = 116.0 + 3.0;

fn start(contrast: Option<Scheme>) -> anyhow::Result<Headless> {
    let config = Config {
        appearance: Appearance {
            high_contrast: contrast.is_some(),
            theme: contrast.unwrap_or(Scheme::Dark),
            reduced_motion: true,
            ..Appearance::default()
        },
        ..Config::default()
    };
    Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

fn pixel(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<[u8; 3]> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = usize::try_from((y as i32) * width + x as i32)? * 4;
    let [b, g, r, _] = bytes
        .get(at..at + 4)
        .and_then(|p| <[u8; 4]>::try_from(p).ok())
        .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
    Ok([r, g, b])
}

/// With `HOCUS_SHOTS` set, the screen saved there as `name.ppm`, to look
/// at.
fn shot(hp: &mut Headless, name: &str) -> anyhow::Result<()> {
    let Some(dir) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    let (w, h, bytes) = hp.screenshot(0)?;
    let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
    for p in bytes.chunks(4) {
        ppm.extend([p[2], p[1], p[0]]);
    }
    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ppm")), ppm)?;
    Ok(())
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

/// The radial menu on a window, open: a point of its disc's band.
fn radial_band(hp: &mut Headless) -> anyhow::Result<(f64, f64)> {
    let (rect, _client) = open_showing(
        hp,
        "test.contrast",
        Shows::Pointing {
            app_id: "test.contrast",
            size: (800, 600),
        },
    )?;
    let at = middle(rect);
    hp.pointer_move(at.0, at.1);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_RIGHT, true);
    hp.button(BTN_RIGHT, false);
    hp.key(KEY_LEFTMETA, false);
    anyhow::ensure!(hp.radial().is_some(), "no radial menu");
    wait(hp, Duration::from_millis(400))?;
    // Between the up and the up-right sectors' labels, in the band.
    let a = (-90.0f64 + 22.5).to_radians();
    Ok((at.0 + BAND * a.cos(), at.1 + BAND * a.sin()))
}

fn middle(rect: Rect) -> (f64, f64) {
    (
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + f64::from(rect.height) / 2.0,
    )
}

#[test]
fn the_radial_menu_follows_high_contrast() -> anyhow::Result<()> {
    let mut plain = start(None)?;
    let at = radial_band(&mut plain)?;
    let own = pixel(&mut plain, at)?;
    assert!(
        own.iter().all(|c| *c < 40) && !near(own, [0x05, 0x07, 0x0C]),
        "its own dark disc: {own:?}"
    );

    let mut dark = start(Some(Scheme::Dark))?;
    let at = radial_band(&mut dark)?;
    let seen = pixel(&mut dark, at)?;
    shot(&mut dark, "radial-dark-contrast")?;
    assert!(
        near(seen, [0x05, 0x07, 0x0C]),
        "dark and contrast: {seen:?}"
    );

    let mut light = start(Some(Scheme::Light))?;
    let at = radial_band(&mut light)?;
    let seen = pixel(&mut light, at)?;
    shot(&mut light, "radial-light-contrast")?;
    assert!(
        near(seen, [0xFF, 0xFF, 0xFF]),
        "light and contrast: {seen:?}"
    );
    Ok(())
}

/// The overview open: the middle of the second desktop's thumbnail.
fn overview_thumb(hp: &mut Headless) -> anyhow::Result<(f64, f64)> {
    hp.pointer_move(960.0, 540.0);
    chord(hp, &[KEY_LEFTMETA, KEY_W]);
    anyhow::ensure!(hp.overview() == Some(true), "no overview");
    wait(hp, Duration::from_millis(400))?;
    hp.overview_thumbs()
        .into_iter()
        .find(|(_, desktop, _)| *desktop == 1)
        .map(|(_, _, at)| at)
        .ok_or_else(|| anyhow::anyhow!("no second desktop's thumbnail"))
}

/// The pixels of the row through the thumbnail's lower part, from its
/// middle out past its left edge: its rim is one of them.
fn left_of(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<Vec<[u8; 3]>> {
    (60..160)
        .map(|dx| pixel(hp, (x - f64::from(dx), y + 30.0)))
        .collect()
}

#[test]
fn the_overviews_thumbnails_have_solid_rims_under_high_contrast() -> anyhow::Result<()> {
    // The cards' rim of each contrast table.
    let (dark_rim, light_rim) = ([0x8E, 0x98, 0xAB], [0x4A, 0x54, 0x68]);
    let mut plain = start(None)?;
    let at = overview_thumb(&mut plain)?;
    let own = left_of(&mut plain, at)?;
    assert!(
        !own.iter()
            .any(|p| near(*p, dark_rim) || near(*p, light_rim)),
        "a faint rim of its own: {own:?}"
    );
    // On the dimmed wallpaper, dark whatever the theme: the dark table's
    // rim in both.
    for (scheme, rim) in [(Scheme::Dark, dark_rim), (Scheme::Light, dark_rim)] {
        let mut hp = start(Some(scheme))?;
        let at = overview_thumb(&mut hp)?;
        let seen = left_of(&mut hp, at)?;
        shot(&mut hp, &format!("overview-{scheme:?}-contrast"))?;
        assert!(
            seen.iter().any(|p| near(*p, rim)),
            "{scheme:?}: no rim {rim:?} in {seen:?}"
        );
    }
    Ok(())
}
