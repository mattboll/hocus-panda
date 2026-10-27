//! A large wallpaper is read, decoded and scaled off the loop: no frame
//! waits for it.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::gate::Gate;

/// Given a large picture on a disk slow to answer (held back until the
/// test lets it), when it is made the wallpaper, then frames are drawn and
/// requests answered while it is read, and it comes in once read.
#[test]
fn a_large_image_is_read_and_decoded_off_the_loop() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("large.png");
    // 6000×4000, a gradient: large to decode, quick to make.
    let (w, h) = (6000_u32, 4000_u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&[(x * 255 / w) as u8, 120, (y * 255 / h) as u8]);
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_compression(png::Compression::Fastest);
    encoder.write_header()?.write_image_data(&data)?;
    let gate = Gate::new();
    gate.fifo(&path, png)?;

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
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.reached() {
        anyhow::ensure!(Instant::now() < deadline, "the picture was never read");
        compositor.dispatch(Duration::from_millis(5))?;
    }
    // Frames go on meanwhile.
    compositor.advance(FRAME * 10)?;
    let early = accent(&mut compositor)?;
    anyhow::ensure!(gate.open(), "the loop waited for the picture to be read");
    assert_eq!(early, before, "the picture came before it was read");
    let deadline = Instant::now() + Duration::from_secs(30);
    while accent(&mut compositor)? == before {
        anyhow::ensure!(Instant::now() < deadline, "the wallpaper never came");
        compositor.advance(FRAME)?;
        compositor.dispatch(Duration::from_millis(5))?;
    }
    compositor.advance(FRAME * 4)?;
    hocus_test::report_held(&compositor.stats());
    Ok(())
}

fn accent(compositor: &mut Headless) -> anyhow::Result<Option<String>> {
    match compositor.request(Request::Config)? {
        Reply::Config(info) => Ok(info.accent),
        other => anyhow::bail!("expected the configuration, got {other:?}"),
    }
}
