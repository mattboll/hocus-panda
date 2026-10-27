//! The saved session is read off the loop (#277): a home directory slow
//! to read (a network one, a disk waking up) holds back neither the start
//! nor a frame, and the welcome-back card offers the saved windows once
//! the file is read.

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

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};

/// How long the stand-in's reads take unless the test lets them end.
const SLOW: Duration = Duration::from_secs(2);

const SAVED: &str = r#"
[[window]]
app_id = "foot"
title = "~"
command = ["/bin/true"]
output = "HEADLESS-1"
desktop = 1
geometry = [100, 100, 600, 400]
state = "normal"
"#;

fn compositor() -> anyhow::Result<Headless> {
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
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
    })
}

/// The session file as a FIFO: reading it waits until `slow_read` writes.
fn fifo(state_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join("session.toml");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &path,
        rustix::fs::Mode::from_raw_mode(0o600),
    )?;
    Ok(path)
}

/// A read of `path` that takes [`SLOW`], or ends when told: the saved
/// session, then the end of the file. `done` is set once it is written.
fn slow_read(path: PathBuf) -> (mpsc::Sender<()>, Arc<AtomicBool>) {
    let (go, wait) = mpsc::channel::<()>();
    let done = Arc::new(AtomicBool::new(false));
    let written = done.clone();
    std::thread::spawn(move || {
        let _ = wait.recv_timeout(SLOW);
        // Opening blocks until the compositor's reader opens its end.
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.write_all(SAVED.as_bytes());
        }
        written.store(true, Ordering::SeqCst);
    });
    (go, done)
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

fn click(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    let node = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
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
    compositor.advance(FRAME)
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

/// Given a session file whose read takes 2 s, when the compositor starts,
/// then it starts and draws without waiting for it, and the card offers
/// the saved windows once it is read; asked again later, the same.
#[test]
fn a_slow_session_file_holds_back_neither_the_start_nor_a_frame() -> anyhow::Result<()> {
    let mut compositor = compositor()?;
    let path = fifo(&compositor.state_dir())?;
    let (go, done) = slow_read(path.clone());

    // The session ends as a crash would; the next one starts on its state.
    compositor.restart()?;
    assert!(
        !done.load(Ordering::SeqCst),
        "the start waited for the saved session to be read"
    );
    compositor.advance(FRAME * 3)?;
    let shown = labels(&mut compositor)?;
    assert!(
        !shown.iter().any(|l| l == "Welcome back"),
        "the card showed before the file was read: {shown:?}"
    );
    let _ = go.send(());
    wait_for(&mut compositor, "Welcome back")?;
    wait_for(&mut compositor, "1 window was open.")?;
    click(&mut compositor, "Start fresh")?;

    // Asked again a second later (as tests do): the loop goes on meanwhile.
    compositor.advance(Duration::from_secs(1))?;
    let (go, done) = slow_read(path);
    compositor.request(Request::OfferSession)?;
    assert!(
        !done.load(Ordering::SeqCst),
        "the request waited for the saved session to be read"
    );
    compositor.advance(FRAME * 3)?;
    let _ = go.send(());
    wait_for(&mut compositor, "Welcome back")?;
    let held = held(&mut compositor)?;
    assert!(held.is_empty(), "held the loop: {held:?}");
    Ok(())
}
