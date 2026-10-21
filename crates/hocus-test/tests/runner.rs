//! Acceptance scenarios of the scenario runner itself (#7).

use std::{path::Path, thread};

use hocus_compositor::{Headless, HeadlessConfig, RendererChoice};
use hocus_test::{
    client::SolidWindow,
    image::Image,
    scenario::{RunOptions, Scenario},
};

fn options(artifacts: &Path) -> RunOptions {
    RunOptions {
        artifacts: artifacts.to_path_buf(),
        bless: false,
    }
}

/// Writes `text` as `<dir>/<name>.toml` and loads it.
fn scenario(dir: &Path, name: &str, text: &str) -> anyhow::Result<Scenario> {
    let path = dir.join(format!("{name}.toml"));
    std::fs::write(&path, text)?;
    Scenario::load(&path)
}

#[test]
fn a_wrong_position_fails_with_expected_and_actual_values() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let scenario = scenario(
        dir.path(),
        "wrong-position",
        r##"
        description = "expects the window in the corner"
        [[step]]
        open = { app_id = "a", size = [400, 300], color = "#3399ff" }
        [[step]]
        expect_window = { app_id = "a", geometry = [0, 0, 400, 300] }
        "##,
    )?;

    let error = scenario
        .run(&options(dir.path()))
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap_or_default();

    assert!(error.contains("step 2"), "{error}");
    assert!(
        error.contains("expected [0, 0, 400, 300], got [440, 250, 400, 300]"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_screenshot_beyond_tolerance_leaves_actual_expected_and_diff() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let artifacts = dir.path().join("artifacts");
    // A reference with the window in another colour.
    let mut reference = Image {
        width: 1280,
        height: 800,
        rgba: [14, 15, 18, 255].repeat(1280 * 800),
    };
    for y in 250..550 {
        for x in 440..840 {
            let i = (y * 1280 + x) * 4;
            reference.rgba[i..i + 4].copy_from_slice(&[0, 200, 0, 255]);
        }
    }
    reference.save(&dir.path().join("green.png"))?;
    let scenario = scenario(
        dir.path(),
        "colour",
        r##"
        description = "the window is blue, the reference green"
        [[step]]
        open = { app_id = "a", size = [400, 300], color = "#3399ff" }
        [[step]]
        expect_screenshot = { output = "HEADLESS-1", reference = "green.png" }
        "##,
    )?;

    let error = scenario
        .run(&options(&artifacts))
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap_or_default();

    // The window's 400 × 300, its rounded corners, ring and shadow.
    let differing: usize = error
        .split(" of 1024000 pixels differ")
        .next()
        .and_then(|head| head.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_default();
    assert!((115_000..=140_000).contains(&differing), "{error}");
    for file in ["green.actual.png", "green.expected.png", "green.diff.png"] {
        let path = artifacts.join("colour").join(file);
        assert!(path.exists(), "{} is missing", path.display());
    }
    let diff = Image::load(&artifacts.join("colour/green.diff.png"))?;
    assert_eq!(diff.pixel(640, 400), Some([255, 0, 0, 255]));
    Ok(())
}

#[test]
fn scenarios_in_parallel_do_not_see_each_other() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let runs: Vec<_> = ["left", "right"]
        .into_iter()
        .map(|app_id| -> anyhow::Result<_> {
            let scenario = scenario(
                dir.path(),
                app_id,
                &format!(
                    r##"
                    description = "only its own window"
                    [[step]]
                    open = {{ app_id = "{app_id}", size = [100, 100], color = "#ffffff" }}
                    [[step]]
                    advance_ms = 200
                    [[step]]
                    expect_stack = ["{app_id}"]
                    "##
                ),
            )?;
            let options = options(dir.path());
            Ok(thread::spawn(move || scenario.run(&options)))
        })
        .collect::<Result<_, _>>()?;
    for run in runs {
        run.join()
            .map_err(|_| anyhow::anyhow!("a scenario thread panicked"))??;
    }

    // And their sockets are their own.
    let config = || HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..Default::default()
    };
    let mut first = Headless::start(config())?;
    let second = Headless::start(config())?;
    assert_ne!(first.socket_path(), second.socket_path());
    assert_ne!(first.api_socket(), second.api_socket());
    let window = SolidWindow {
        app_id: "only-in-first".into(),
        width: 100,
        height: 100,
        argb: 0xffff_ffff,
    };
    let _client = window.spawn(first.socket_path())?;
    first.dispatch_until(std::time::Duration::from_secs(10), |c| {
        c.window_count() == 1
    })?;
    assert_eq!(second.window_count(), 0);
    Ok(())
}
