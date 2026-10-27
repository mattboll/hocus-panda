//! What the loop asks of the file system runs off it (#280, #285): a
//! folder slow to answer (a network home, a disk waking up) holds back no
//! frame when a recording begins, when the record picker looks for the
//! camera, when the settings open, or when a program starts.

use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_api::{Button, Event, Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::{bus::installed, gate::Gate};

const LEFT_META: u32 = 125;
const LEFT_SHIFT: u32 = 42;
const KEY_R: u32 = 19;
const KEY_F9: u32 = 67;

/// With the shell, and a `PATH` where nothing is found: the settings'
/// default command starts nothing.
fn compositor(empty: &Path) -> anyhow::Result<Headless> {
    compositor_with(vec![("PATH".into(), empty.as_os_str().to_owned())])
}

/// With the shell, and `env` for what it starts.
fn compositor_with(env: Vec<(OsString, OsString)>) -> anyhow::Result<Headless> {
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
        child_env: Some(env),
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

/// Given a `PATH` whose first directory is slow to answer (the thread
/// that starts each program waits until the test lets it go: no
/// directory can be made slow without FUSE), when a shortcut starts a
/// program, then the key is handled at once and the loop draws on while
/// the program is starting; once let go, the program starts with the
/// session's environment and an activation token of the shell's in place
/// of the session's own, and no handler held the loop.
#[test]
fn a_slow_path_holds_back_no_frame_when_a_program_starts() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let slow = dir.path().join("slow");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&slow)?;
    std::fs::create_dir(&bin)?;
    // What it was started with, written whole.
    let probe = bin.join("probe");
    std::fs::write(
        &probe,
        "#!/bin/sh\nexport -p > \"$XDG_RUNTIME_DIR/probe.new\"\n\
         mv \"$XDG_RUNTIME_DIR/probe.new\" \"$XDG_RUNTIME_DIR/probe-env\"\n",
    )?;
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755))?;
    let path = std::env::join_paths([
        slow.as_path(),
        &bin,
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])?;
    let mut compositor = compositor_with(vec![
        ("PATH".into(), path),
        ("HOCUS_SESSION_SAYS".into(), "kept".into()),
        ("XDG_ACTIVATION_TOKEN".into(), "used-already".into()),
        ("LD_PRELOAD".into(), "".into()),
    ])?;
    let gate = Gate::new();
    let held = gate.clone();
    let reached = Arc::new(AtomicUsize::new(0));
    let at_gate = reached.clone();
    compositor.set_program_gate(move |_| {
        at_gate.fetch_add(1, Ordering::SeqCst);
        held.wait();
    });
    compositor.write_config("[shortcuts]\n\"Super+F9\" = \"spawn probe\"\n")?;

    keys(&mut compositor, &[LEFT_META, KEY_F9])?;
    // Answered at once: the program is starting, on its own thread.
    assert_eq!(compositor.programs_starting(), 1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while reached.load(Ordering::SeqCst) == 0 {
        anyhow::ensure!(Instant::now() < deadline, "the program was never started");
        compositor.dispatch(Duration::from_millis(10))?;
    }
    run(&mut compositor, Duration::from_millis(1500))?;
    assert_eq!(
        compositor.programs_starting(),
        1,
        "it started before let go"
    );
    let env = compositor.runtime_dir().join("probe-env");
    assert!(!env.exists(), "it ran before let go");

    anyhow::ensure!(gate.open(), "the loop waited for the program to start");
    let deadline = Instant::now() + Duration::from_secs(10);
    while compositor.programs_starting() > 0 || !env.exists() {
        anyhow::ensure!(Instant::now() < deadline, "the program never started");
        compositor.dispatch(Duration::from_millis(10))?;
    }
    let env = std::fs::read_to_string(&env)?;
    let token = env
        .lines()
        .find_map(|l| l.strip_prefix("export XDG_ACTIVATION_TOKEN="))
        .map(|t| t.trim_matches(['\'', '"']));
    assert!(
        token.is_some_and(|t| !t.is_empty() && t != "used-already"),
        "no token of the shell's: {env}"
    );
    for line in [
        "HOCUS_SESSION_SAYS=",
        "DESKTOP_STARTUP_ID=",
        "WAYLAND_DISPLAY=",
        "XDG_CURRENT_DESKTOP=",
        "XDG_SESSION_TYPE=",
    ] {
        assert!(env.contains(line), "no {line} in {env}");
    }
    assert!(!env.contains("LD_PRELOAD"), "{env}");
    assert_eq!(reached.load(Ordering::SeqCst), 1);
    hocus_test::report_held(&compositor.stats());
    Ok(())
}
