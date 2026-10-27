//! A large wallpaper is decoded and scaled off the loop: no frame waits for
//! it.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};

#[test]
fn a_large_image_keeps_the_loop_within_its_budget() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("large.png");
    // 6000×4000, a gradient: large to decode, quick to write.
    let (w, h) = (6000_u32, 4000_u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&[(x * 255 / w) as u8, 120, (y * 255 / h) as u8]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_compression(png::Compression::Fastest);
    encoder.write_header()?.write_image_data(&data)?;

    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })?;
    // Its accent tells when the picture has been made.
    let before = accent(&mut compositor)?;
    compositor.write_config(&format!(
        "[appearance]\naccent = \"wallpaper\"\n[wallpaper]\nkind = \"image\"\nimage = \"{}\"\n",
        path.display()
    ))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while accent(&mut compositor)? == before {
        anyhow::ensure!(Instant::now() < deadline, "the wallpaper never came");
        // Frames go on meanwhile.
        compositor.advance(hocus_compositor::FRAME)?;
        compositor.dispatch(Duration::from_millis(5))?;
    }
    compositor.advance(hocus_compositor::FRAME * 4)?;
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("expected stats");
    };
    let held = hocus_test::held_the_loop(&stats);
    assert!(held.is_empty(), "the loop was held: {held:?}");
    Ok(())
}

fn accent(compositor: &mut Headless) -> anyhow::Result<Option<String>> {
    match compositor.request(Request::Config)? {
        Reply::Config(info) => Ok(info.accent),
        other => anyhow::bail!("expected the configuration, got {other:?}"),
    }
}
