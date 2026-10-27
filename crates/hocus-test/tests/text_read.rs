//! A screenshot's text is read once (#302): one run of the reader per
//! click, and none more when the indexer reads the same picture, before
//! the click, while the click's read runs, or after it.

use std::{
    path::Path,
    time::{Duration, Instant},
};

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
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })
}

fn buttons(compositor: &mut Headless, label: &str) -> anyhow::Result<Vec<hocus_api::UiNode>> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    Ok(nodes
        .into_iter()
        .filter(|n| n.role == "button" && n.label.as_deref() == Some(label))
        .collect())
}

fn click(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let node = buttons(compositor, label)?
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no button {label:?}"))?;
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

/// Print, and the path of the screenshot saved once its toast is in.
fn screenshot(compositor: &mut Headless) -> anyhow::Result<String> {
    for pressed in [true, false] {
        compositor.request(Request::Key { code: 99, pressed })?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let path = loop {
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.request(Request::Version)?;
        let saved = compositor.take_events().into_iter().find_map(|e| match e {
            Event::Screenshot {
                path: Some(path), ..
            } => Some(path),
            _ => None,
        });
        if let Some(path) = saved {
            break path;
        }
        anyhow::ensure!(Instant::now() < deadline, "no screenshot");
    };
    compositor.advance(Duration::from_millis(600))?;
    Ok(path)
}

/// The text the stand-in reads in `path`.
fn text_of(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("Text of {name}")
}

/// The reader's runs so far, each with the picture it read.
fn runs(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn search(compositor: &mut Headless) -> anyhow::Result<hocus_api::SearchInfo> {
    let Reply::Search(info) = compositor.request(Request::Search)? else {
        anyhow::bail!("expected the indexer's state");
    };
    Ok(info)
}

#[test]
fn a_screenshot_s_text_is_read_once_per_click_and_once_with_the_indexer() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (log, go) = (dir.path().join("runs"), dir.path().join("go"));
    // tesseract, counting its runs: it reads nothing until the test lets
    // it, and goes on by itself after ten seconds.
    let ocr = dir.path().join("ocr");
    std::fs::write(
        &ocr,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{}'\nn=0\nwhile [ ! -e '{}' ] && [ $n -lt 1000 ]; do sleep 0.01; n=$((n + 1)); done\necho \"Text of $(basename \"$1\")\"\n",
            log.display(),
            go.display()
        ),
    )?;
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

    // The indexer reads the screenshots folder, the new picture in it:
    // the reader runs, held back.
    let first = screenshot(&mut compositor)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while runs(&log).is_empty() {
        anyhow::ensure!(
            Instant::now() < deadline,
            "the indexer never read the picture"
        );
        compositor.request(Request::SearchAgain)?;
        compositor.dispatch(Duration::from_millis(10))?;
    }
    // Its toast's "Copy the text", while the indexer's read runs: it
    // waits for that read, and the toast goes, so it is not clicked twice.
    click(&mut compositor, "Copy the text")?;
    anyhow::ensure!(
        buttons(&mut compositor, "Copy the text")?.is_empty(),
        "the toast stays to be clicked again"
    );
    std::fs::write(&go, "")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while reader.clipboard() != Some(text_of(&first)) {
        anyhow::ensure!(Instant::now() < deadline, "the text never came");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    assert_eq!(
        runs(&log),
        std::slice::from_ref(&first),
        "one read for the click and the indexer"
    );

    // Another screenshot, the indexer done: one click, one read.
    let second = screenshot(&mut compositor)?;
    assert_ne!(second, first);
    click(&mut compositor, "Copy the text")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while reader.clipboard() != Some(text_of(&second)) {
        anyhow::ensure!(Instant::now() < deadline, "the second text never came");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    assert_eq!(runs(&log), [first.clone(), second.clone()]);

    // The indexer, looking again, finds the second picture: it keeps the
    // text the click read.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = search(&mut compositor)?;
        if info.state == "ready" && info.documents == 2 {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "the indexer never had both: {info:?}"
        );
        compositor.request(Request::SearchAgain)?;
        compositor.dispatch(Duration::from_millis(10))?;
    }
    assert_eq!(runs(&log), [first, second], "the indexer read it again");
    Ok(())
}
