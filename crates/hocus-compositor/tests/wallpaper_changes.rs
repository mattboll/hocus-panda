//! A wallpaper that changes by itself (#894): a folder's pictures in
//! turn, on pixman (at once, no fade), its interval made short by the
//! test (`Config::wallpaper_every`), the folder the test's own with
//! pictures of a colour each. What shows is read from the screen.

use std::{path::Path, time::Duration};

use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, WallpaperOrder,
};
use hocus_link::wallpaper::Picture;

const RED: [u8; 3] = [220, 40, 40];
const GREEN: [u8; 3] = [40, 200, 60];
const BLUE: [u8; 3] = [40, 60, 220];
const YELLOW: [u8; 3] = [230, 210, 40];

fn picture(path: &Path, [r, g, b]: [u8; 3]) -> anyhow::Result<()> {
    image::RgbaImage::from_pixel(32, 20, image::Rgba([r, g, b, 255])).save(path)?;
    Ok(())
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 12)
}

/// The colour in the middle of the screen.
fn middle(hp: &mut Headless) -> anyhow::Result<[u8; 3]> {
    let (w, h, bytes) = hp.screenshot(0)?;
    let at = usize::try_from((h / 2) * w + w / 2)? * 4;
    let [b, g, r, _] = bytes
        .get(at..at + 4)
        .and_then(|p| <[u8; 4]>::try_from(p).ok())
        .ok_or_else(|| anyhow::anyhow!("no middle"))?;
    Ok([r, g, b])
}

/// The colours the screen shows one after the other, read until `n` of
/// them are seen (each named once while it shows).
fn seen(hp: &mut Headless, n: usize) -> anyhow::Result<Vec<&'static str>> {
    let names = [
        ("red", RED),
        ("green", GREEN),
        ("blue", BLUE),
        ("yellow", YELLOW),
    ];
    let mut out: Vec<&'static str> = Vec::new();
    for _ in 0..800 {
        let now = middle(hp)?;
        if let Some((name, _)) = names.iter().find(|(_, c)| near(now, *c))
            && out.last() != Some(name)
        {
            out.push(name);
            if out.len() == n {
                return Ok(out);
            }
        }
        hp.dispatch(Duration::from_millis(20))?;
    }
    anyhow::bail!("the screen showed {out:?} only")
}

#[test]
fn a_folders_pictures_come_in_turn_and_the_folder_is_followed() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let folder = dir.path().join("Fonds");
    std::fs::create_dir_all(folder.join("sub"))?;
    picture(&folder.join("a.png"), RED)?;
    picture(&folder.join("b.png"), GREEN)?;
    // A subfolder's too; a hidden picture and another file are not.
    picture(&folder.join("sub/c.png"), BLUE)?;
    picture(&folder.join(".hidden.png"), YELLOW)?;
    std::fs::write(folder.join("notes.txt"), "not a picture")?;
    let mut config = Config::default();
    config.wallpaper.picture = Picture::Folder;
    config.wallpaper.folder = folder.display().to_string();
    config.wallpaper.order = WallpaperOrder::Name;
    config.wallpaper_every = Some(Duration::from_millis(700));
    config.state_dir = Some(dir.path().join("state"));
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(320, 200)],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
    )?;
    // By name, then again from the first.
    assert_eq!(seen(&mut hp, 4)?, ["red", "green", "blue", "red"]);
    // A picture added joins the round in its place.
    picture(&folder.join("b2.png"), YELLOW)?;
    assert_eq!(seen(&mut hp, 5)?, ["red", "green", "yellow", "blue", "red"]);
    // One removed leaves it.
    std::fs::remove_file(folder.join("b.png"))?;
    let after = seen(&mut hp, 5)?;
    assert!(!after.contains(&"green"), "{after:?}");
    // The round kept for the next session.
    let kept = std::fs::read_to_string(dir.path().join("state/wallpaper.json"))?;
    assert!(
        kept.contains("b2.png") && !kept.contains("b.png\""),
        "{kept}"
    );
    Ok(())
}

/// On the GPU (GLES on Mesa's software device), the next picture fades in
/// over 2 s on the held clock: halfway, neither the one nor the other.
#[test]
fn on_the_gpu_the_next_picture_fades_in() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let folder = dir.path().join("Fonds");
    std::fs::create_dir_all(&folder)?;
    picture(&folder.join("a.png"), RED)?;
    picture(&folder.join("b.png"), BLUE)?;
    let mut config = Config::default();
    config.wallpaper.picture = Picture::Folder;
    config.wallpaper.folder = folder.display().to_string();
    config.wallpaper.order = WallpaperOrder::Name;
    config.wallpaper_every = Some(Duration::from_millis(1500));
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(320, 200)],
            ..crate::common::gles()
        },
        config,
    )?;
    if hp.renderer() != "gles" {
        crate::common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    assert_eq!(seen(&mut hp, 1)?, ["red"]);
    hp.hold_clock();
    // The change comes on the real clock; its fade on the held one.
    let fading = hp.dispatch_until(Duration::from_secs(5), |hp| {
        !hp.state().graphics.backgrounds.borrow().fading.is_empty()
    })?;
    anyhow::ensure!(fading, "no fade began");
    hp.step(Duration::from_millis(1000))?;
    let half = middle(&mut hp)?;
    assert!(
        !near(half, RED) && !near(half, BLUE) && half[0] > 60 && half[2] > 60,
        "{half:?}"
    );
    hp.step(Duration::from_millis(1100))?;
    hp.step(Duration::from_millis(20))?;
    let end = middle(&mut hp)?;
    assert!(near(end, BLUE), "{end:?}");
    Ok(())
}
