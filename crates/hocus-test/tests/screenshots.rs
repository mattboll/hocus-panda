//! The screenshot tool (#69): saved where the configuration says, and its
//! text read off the loop.

use std::time::{Duration, Instant};

use hocus_api::{Event, Reply, Request};
use hocus_compositor::{
    FRAME, HarnessOptions, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};
use hocus_test::client::SolidWindow;

fn compositor() -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            renderer: RendererChoice::Pixman,
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
    })
}

fn click(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    let node = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
        .ok_or_else(|| anyhow::anyhow!("no button {label:?} in {nodes:?}"))?;
    let r = node.rect;
    compositor.request(Request::PointerMove {
        x: f64::from(r.x) + f64::from(r.width) / 2.0,
        y: f64::from(r.y) + f64::from(r.height) / 2.0,
    })?;
    for pressed in [true, false] {
        compositor.request(Request::PointerButton {
            button: hocus_api::Button::Left,
            pressed,
        })?;
    }
    Ok(())
}

#[test]
fn a_screenshot_is_saved_and_its_text_read_off_the_loop() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    // tesseract, slowly: a second to read "the picture".
    let ocr = dir.path().join("ocr");
    std::fs::write(&ocr, "#!/bin/sh\nsleep 1\necho 'Hello from the picture'\n")?;
    std::fs::set_permissions(&ocr, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let folder = dir.path().join("shots");
    let mut compositor = compositor()?;
    compositor.write_config(&format!(
        "[screenshots]\nfolder = \"{}\"\nocr = \"{}\"\n",
        folder.display(),
        ocr.display()
    ))?;
    let reader = SolidWindow {
        app_id: "reader".into(),
        width: 300,
        height: 200,
        argb: 0xff_33_66_cc,
    }
    .spawn(compositor.socket_path())?;
    compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
    compositor.request(Request::Subscribe)?;

    // Print: the screen under the pointer.
    for pressed in [true, false] {
        compositor.request(Request::Key { code: 99, pressed })?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let (path, size) = loop {
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.request(Request::Version)?;
        let found = compositor.take_events().into_iter().find_map(|e| match e {
            Event::Screenshot {
                path: Some(path),
                width,
                height,
            } => Some((path, (width, height))),
            _ => None,
        });
        if let Some(found) = found {
            break found;
        }
        anyhow::ensure!(Instant::now() < deadline, "no screenshot");
    };
    assert_eq!(size, (1280, 800));
    assert!(path.starts_with(&folder.display().to_string()), "{path}");
    let png = std::fs::read(&path)?;
    assert_eq!(&png[1..4], b"PNG");
    // The toast comes in.
    compositor.advance(Duration::from_millis(600))?;

    // Its toast copies its text; the loop never waits for it.
    click(&mut compositor, "Copy the text")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while reader.clipboard().as_deref() != Some("Hello from the picture") {
        anyhow::ensure!(Instant::now() < deadline, "the text never came");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("no stats");
    };
    let slow: Vec<_> = stats
        .slow_handlers
        .iter()
        .filter(|h| h.ms >= 200.0)
        .collect();
    assert!(slow.is_empty(), "the loop waited: {slow:?}");
    Ok(())
}
