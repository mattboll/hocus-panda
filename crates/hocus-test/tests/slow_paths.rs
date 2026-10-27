//! What the loop asks of the file system runs off it (#280): a folder
//! slow to answer (a network home, a disk waking up) holds back no frame
//! when a recording begins, when the record picker looks for the camera,
//! or when the settings open.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_api::{Button, Event, Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::{bus::installed, gate::Gate};

const LEFT_META: u32 = 125;
const LEFT_SHIFT: u32 = 42;
const KEY_R: u32 = 19;

/// With the shell, and a `PATH` where nothing is found: the settings'
/// default command starts nothing.
fn compositor(empty: &Path) -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
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
        child_env: Some(vec![("PATH".into(), empty.as_os_str().to_owned())]),
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })
}

/// `user-dirs.dirs` read from a disk slow to answer, held back until the
/// gate opens: the videos folder, then the end of the file.
fn slow_read(dir: &Path, videos: &Path) -> anyhow::Result<(PathBuf, Gate)> {
    let path = dir.join("user-dirs.dirs");
    let gate = Gate::new();
    gate.fifo(&path, format!("XDG_VIDEOS_DIR=\"{}\"\n", videos.display()))?;
    Ok((path, gate))
}

fn keys(compositor: &mut Headless, codes: &[u32]) -> anyhow::Result<()> {
    for (code, pressed) in codes
        .iter()
        .map(|c| (*c, true))
        .chain(codes.iter().rev().map(|c| (*c, false)))
    {
        compositor.request(Request::Key { code, pressed })?;
    }
    Ok(())
}

fn labels(compositor: &mut Headless) -> anyhow::Result<Vec<String>> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    Ok(nodes.into_iter().filter_map(|n| n.label).collect())
}

fn wait_for(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !labels(compositor)?.iter().any(|l| l == label) {
        anyhow::ensure!(Instant::now() < deadline, "{label} never showed");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    Ok(())
}

/// Moves the virtual clock by `by`, a tenth of a second at a time,
/// giving the workers real time too.
fn run(compositor: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let step = Duration::from_millis(100);
    let mut gone = Duration::ZERO;
    while gone < by {
        compositor.advance(step.min(by - gone))?;
        compositor.dispatch(Duration::from_millis(15))?;
        gone += step;
    }
    Ok(())
}

/// Given a `user-dirs.dirs` slow to read and no recordings folder
/// configured, when a region is recorded, then the recording begins and
/// frames are drawn without waiting for the file; the video goes to the
/// folder it names once read. Given a camera device, the picker offers
/// it once a worker found it. Opening the settings then looks at no file
/// (#280): what held the loop is only reported, its times being the
/// machine's as much as the code's.
#[test]
fn slow_folders_hold_back_no_frame_when_recording_or_opening_the_settings() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let empty = dir.path().join("bin");
    std::fs::create_dir(&empty)?;
    let camera = dir.path().join("video0");
    std::fs::write(&camera, b"")?;
    let videos = dir.path().join("Videos");
    let (user_dirs, gate) = slow_read(dir.path(), &videos)?;

    let mut compositor = compositor(&empty)?;
    compositor.set_user_dirs(&user_dirs);
    compositor.request(Request::Subscribe)?;
    compositor.write_config(&format!(
        "[recording]\nhardware = false\ncountdown = 1\ncamera_device = {:?}\n",
        camera.display().to_string()
    ))?;

    // Super+Shift+R: the camera is looked for on a worker.
    keys(&mut compositor, &[LEFT_META, LEFT_SHIFT, KEY_R])?;
    wait_for(&mut compositor, "Camera")?;
    // A region drawn: 320×240 at (100, 100), then the countdown.
    compositor.request(Request::PointerMove { x: 100.0, y: 100.0 })?;
    compositor.request(Request::PointerButton {
        button: Button::Left,
        pressed: true,
    })?;
    compositor.request(Request::PointerMove { x: 420.0, y: 340.0 })?;
    compositor.request(Request::PointerButton {
        button: Button::Left,
        pressed: false,
    })?;
    run(&mut compositor, Duration::from_millis(1500))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.reached() {
        anyhow::ensure!(
            Instant::now() < deadline,
            "the user's folders were never read"
        );
        compositor.dispatch(Duration::from_millis(5))?;
    }
    assert!(
        labels(&mut compositor)?
            .iter()
            .any(|l| l == "Stop the recording"),
        "the recording did not begin"
    );
    run(&mut compositor, Duration::from_millis(300))?;
    anyhow::ensure!(
        gate.open(),
        "the recording waited for the user's folders to be read"
    );
    run(&mut compositor, Duration::from_millis(500))?;
    keys(&mut compositor, &[LEFT_META, LEFT_SHIFT, KEY_R])?;

    if installed("ffmpeg") {
        let deadline = Instant::now() + Duration::from_secs(30);
        let path = loop {
            let saved = compositor.take_events().into_iter().find_map(|e| match e {
                Event::Recording { path, .. } => path,
                _ => None,
            });
            if let Some(path) = saved {
                break PathBuf::from(path);
            }
            anyhow::ensure!(Instant::now() < deadline, "the recording was never saved");
            compositor.dispatch(Duration::from_millis(20))?;
            // Events come in with a reply.
            compositor.request(Request::Stats)?;
        };
        assert_eq!(path.parent(), Some(videos.join("Screencasts").as_path()));
    }

    // The settings, with their default command: from beside the
    // compositor when built together, else from PATH (here, nothing).
    compositor.request(Request::OpenSettings { page: None })?;
    run(&mut compositor, Duration::from_millis(300))?;
    hocus_test::report_held(&compositor.stats());
    Ok(())
}
