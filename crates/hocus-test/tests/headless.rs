//! Acceptance scenarios of the headless backend and its virtual clock (#5).

use std::time::Duration;

use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice, RendererKind};
use hocus_test::client::SolidWindow;

/// Long enough for a client thread on a loaded CI machine.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// The two outputs of the scenarios: a laptop panel at scale 2 and a wide
/// screen to its right. Pixman, so that every machine draws the same pixels.
fn two_outputs() -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(3440, 1440),
        ],
        renderer: RendererChoice::Pixman,
    }
}

fn frames(compositor: &Headless) -> Vec<u64> {
    compositor.outputs().iter().map(|o| o.frames).collect()
}

#[test]
fn starts_with_pixman_and_reports_both_outputs() -> anyhow::Result<()> {
    let compositor = Headless::start(two_outputs())?;

    assert_eq!(compositor.renderer(), Some(RendererKind::Pixman));
    let outputs = compositor.outputs();
    let summary: Vec<_> = outputs
        .iter()
        .map(|o| {
            (
                o.name.as_str(),
                o.spec.width,
                o.spec.height,
                o.spec.scale,
                o.position,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("HEADLESS-1", 2560, 1600, 2.0, (0, 0)),
            ("HEADLESS-2", 3440, 1440, 1.0, (1280, 0)),
        ]
    );
    Ok(())
}

#[test]
fn runs_in_a_private_runtime_directory() -> anyhow::Result<()> {
    let compositor = Headless::start(two_outputs())?;
    let dir = compositor.runtime_dir().to_path_buf();

    assert!(compositor.socket_path().starts_with(&dir));
    assert_ne!(
        Some(dir.as_os_str()),
        std::env::var_os("XDG_RUNTIME_DIR").as_deref()
    );
    drop(compositor);
    assert!(!dir.exists(), "{} was left behind", dir.display());
    Ok(())
}

#[test]
fn draws_nothing_at_rest() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    // The first frame clears both outputs.
    compositor.advance(FRAME)?;
    assert_eq!(frames(&compositor), [1, 1]);

    compositor.advance(Duration::from_secs(10))?;

    assert_eq!(frames(&compositor), [1, 1]);
    Ok(())
}

#[test]
fn a_committed_buffer_draws_one_frame_on_its_output() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    compositor.advance(FRAME)?;
    let start = compositor.now();

    let window = SolidWindow {
        app_id: "hocus-test.solid".into(),
        width: 400,
        height: 300,
        argb: 0xff_33_99_ff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    assert_eq!(
        compositor.now(),
        start,
        "the clock moved without the harness"
    );
    assert_eq!(
        frames(&compositor),
        [1, 1],
        "a frame was drawn before its time"
    );

    compositor.advance(FRAME)?;

    // New windows open centred on the first output.
    assert_eq!(frames(&compositor), [2, 1]);
    Ok(())
}

/// GLES where there is a render node, pixman elsewhere: both must draw.
#[test]
fn the_automatic_renderer_draws() -> anyhow::Result<()> {
    let mut compositor = Headless::start(HeadlessConfig::default())?;
    let window = SolidWindow {
        app_id: "hocus-test.solid".into(),
        width: 200,
        height: 100,
        argb: 0xff_ff_66_00,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;

    compositor.advance(FRAME)?;

    assert_eq!(frames(&compositor), [1]);
    Ok(())
}
