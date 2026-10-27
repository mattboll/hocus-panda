//! What the loop asks of the file system runs off it (#280): a folder
//! slow to answer (a network home, a disk waking up) holds back no frame
//! when a recording begins, when the record picker looks for the camera,
//! or when the settings open.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use hocus_api::{Button, Event, Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::bus::installed;

/// How long the stand-in's read takes unless the test lets it end.
const SLOW: Duration = Duration::from_secs(5);

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

/// `user-dirs.dirs` as a FIFO: reading it waits until `slow_read` writes.
fn fifo(dir: &Path) -> anyhow::Result<PathBuf> {
    let path = dir.join("user-dirs.dirs");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &path,
        rustix::fs::Mode::from_raw_mode(0o600),
    )?;
    Ok(path)
}

/// A read of `path` that takes [`SLOW`], or ends when told: the videos
/// folder, then the end of the file. `done` is set once it is written.
fn slow_read(path: PathBuf, videos: &Path) -> (mpsc::Sender<()>, Arc<AtomicBool>) {
    let (go, wait) = mpsc::channel::<()>();
    let done = Arc::new(AtomicBool::new(false));
    let written = done.clone();
    let line = format!("XDG_VIDEOS_DIR=\"{}\"\n", videos.display());
    std::thread::spawn(move || {
        let _ = wait.recv_timeout(SLOW);
        // Opening blocks until the compositor's reader opens its end.
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
        written.store(true, Ordering::SeqCst);
    });
    (go, done)
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

fn held(compositor: &mut Headless) -> anyhow::Result<Vec<hocus_api::SlowHandler>> {
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("expected stats");
    };
    Ok(hocus_test::held_the_loop(&stats)
        .into_iter()
        .cloned()
        .collect())
}

/// Given a `user-dirs.dirs` whose read takes 5 s and no recordings folder
/// configured, when a region is recorded, then the recording begins and
/// frames are drawn without waiting for the file; the video goes to the
/// folder it names once read. Given a camera device, the picker offers
/// it once a worker found it. Opening the settings then holds nothing
/// either.
#[test]
fn slow_folders_hold_back_no_frame_when_recording_or_opening_the_settings() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let empty = dir.path().join("bin");
    std::fs::create_dir(&empty)?;
    let camera = dir.path().join("video0");
    std::fs::write(&camera, b"")?;
    let videos = dir.path().join("Videos");
    let user_dirs = fifo(dir.path())?;

    let mut compositor = compositor(&empty)?;
    compositor.set_user_dirs(&user_dirs);
    compositor.request(Request::Subscribe)?;
    compositor.write_config(&format!(
        "[recording]\nhardware = false\ncountdown = 1\ncamera_device = {:?}\n",
        camera.display().to_string()
    ))?;
    let (go, done) = slow_read(user_dirs, &videos);

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
    assert!(
        !done.load(Ordering::SeqCst),
        "the recording waited for the user's folders to be read"
    );
    assert!(
        labels(&mut compositor)?
            .iter()
            .any(|l| l == "Stop the recording"),
        "the recording did not begin"
    );
    let _ = go.send(());
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
    let held = held(&mut compositor)?;
    assert!(held.is_empty(), "held the loop: {held:?}");
    Ok(())
}
