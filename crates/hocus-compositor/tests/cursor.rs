//! Cursors from a cursor theme, at each screen's scale; the compositor's
//! own drawn ones without a theme.

mod common;

use std::time::{Duration, Instant};

use anyhow::Context;
use common::{WAIT, open};
use hocus_compositor::{
    Appearance, Config, CursorEnv, Headless, HeadlessConfig, OutputSpec, RendererChoice,
    encode_xcursor,
};
use smithay::input::pointer::CursorIcon;

/// Opaque red and green, premultiplied ARGB.
const RED: u32 = 0xffff_0000;
const GREEN: u32 = 0xff00_ff00;

/// A theme, `test`, whose `ew-resize` is a red square at 24 px and a green
/// one at 48 px, each with its hotspot in the middle.
fn test_theme() -> anyhow::Result<tempfile::TempDir> {
    theme_with(&[
        (24, 24, 24, (12, 12), 0, RED),
        (48, 48, 48, (24, 24), 0, GREEN),
    ])
}

fn theme_with(ew_resize: &[hocus_compositor::TestImage]) -> anyhow::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    let cursors = dir.path().join("test/cursors");
    std::fs::create_dir_all(&cursors)?;
    std::fs::write(dir.path().join("test/index.theme"), "[Icon Theme]\n")?;
    std::fs::write(cursors.join("ew-resize"), encode_xcursor(ew_resize))?;
    Ok(dir)
}

/// One 1920×1080 screen at `scale`, its cursors from `theme` when given.
fn compositor(scale: f64, theme: Option<&tempfile::TempDir>) -> anyhow::Result<Headless> {
    let config = Config {
        appearance: Appearance {
            cursor_theme: Some("test".into()),
            ..Appearance::default()
        },
        cursor_env: CursorEnv {
            search: theme.map(|t| t.path().to_owned()).into_iter().collect(),
            ..CursorEnv::default()
        },
        ..Config::default()
    };
    Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080).scale(scale)],
            renderer: RendererChoice::Pixman,
        },
        config,
    )
}

/// The pixel at physical (`x`, `y`), as premultiplied ARGB.
fn pixel(hp: &mut Headless, x: i32, y: i32) -> anyhow::Result<u32> {
    let (width, _, pixels) = hp.screenshot(0)?;
    let i = ((y * width + x) * 4) as usize;
    let bgra: [u8; 4] = pixels
        .get(i..i + 4)
        .context("outside the screen")?
        .try_into()?;
    Ok(u32::from_le_bytes(bgra))
}

/// Points at a window's right edge on a screen at `scale`; the pixel
/// `offset` physical pixels right of and below the pointer, once it is
/// `wanted` (or the last one seen after a while).
fn at_an_edge(hp: &mut Headless, scale: i32, offset: i32, wanted: u32) -> anyhow::Result<u32> {
    let rect = open(hp, "test.cursor", (300, 200))?;
    let (x, y) = (
        f64::from(rect.x + rect.width) + 3.0,
        f64::from(rect.y) + 100.0,
    );
    hp.pointer_move(x, y);
    assert_eq!(hp.state().frame_cursor, Some(CursorIcon::EwResize));
    let (px, py) = (x as i32 * scale + offset, y as i32 * scale + offset);
    let until = Instant::now() + WAIT;
    loop {
        hp.dispatch(Duration::from_millis(5))?;
        let seen = pixel(hp, px, py)?;
        if seen == wanted || Instant::now() > until {
            return Ok(seen);
        }
    }
}

#[test]
fn a_resize_edge_shows_the_themes_cursor_at_the_screens_scale() -> anyhow::Result<()> {
    let theme = test_theme()?;
    // At 1, the 24 px image; at 2, the 48 px one, pixel for pixel: 20 px
    // from the pointer is inside it.
    let mut hp = compositor(1.0, Some(&theme))?;
    assert_eq!(at_an_edge(&mut hp, 1, 8, RED)?, RED);
    let mut hp = compositor(2.0, Some(&theme))?;
    assert_eq!(at_an_edge(&mut hp, 2, 20, GREEN)?, GREEN);
    // Once the window has opened, nothing is drawn while the cursor stays
    // the same.
    hp.dispatch_until(Duration::from_secs(1), |_| false)?;
    let frames = hp.frames();
    hp.dispatch_until(Duration::from_millis(300), |_| false)?;
    assert_eq!(hp.frames(), frames);
    Ok(())
}

#[test]
fn an_animated_cursor_is_drawn_when_its_frame_changes_only() -> anyhow::Result<()> {
    // Red, then green, 100 ms each.
    let theme = theme_with(&[
        (24, 24, 24, (12, 12), 100, RED),
        (24, 24, 24, (12, 12), 100, GREEN),
    ])?;
    let mut hp = compositor(1.0, Some(&theme))?;
    at_an_edge(&mut hp, 1, 8, RED)?;
    hp.dispatch_until(Duration::from_secs(1), |_| false)?;
    let frames = hp.frames();
    let (mut red, mut green) = (false, false);
    let until = Instant::now() + Duration::from_millis(600);
    while Instant::now() < until {
        hp.dispatch(Duration::from_millis(5))?;
        let rect = hp.windows()[0].rect;
        let seen = pixel(&mut hp, rect.x + rect.width + 3 + 8, rect.y + 100 + 8)?;
        red |= seen == RED;
        green |= seen == GREEN;
    }
    assert!(red && green, "red {red}, green {green}");
    // About one frame per 100 ms, not one per turn of the loop.
    let drawn = hp.frames() - frames;
    assert!((2..=12).contains(&drawn), "{drawn} frames");
    Ok(())
}

#[test]
fn without_a_theme_the_compositor_draws_its_cursors() -> anyhow::Result<()> {
    let mut hp = compositor(1.0, None)?;
    // The middle of the drawn double arrow: white.
    let seen = at_an_edge(&mut hp, 1, 0, 0xffff_ffff)?;
    assert_eq!(seen, 0xffff_ffff, "{seen:#x}");
    Ok(())
}
