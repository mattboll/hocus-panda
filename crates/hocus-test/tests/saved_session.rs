//! The saved session is read off the loop (#277): a home directory slow
//! to read (a network one, a disk waking up) holds back neither the start
//! nor a frame, and the welcome-back card offers the saved windows once
//! the file is read.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::gate::Gate;

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
        google_tasks: None,
    })
}

/// The session file, read from a disk slow to answer: held back until
/// the gate opens.
fn slow_read(compositor: &Headless) -> anyhow::Result<Gate> {
    let dir = compositor.state_dir();
    std::fs::create_dir_all(&dir)?;
    let gate = Gate::new();
    gate.fifo(&dir.join("session.toml"), SAVED)?;
    Ok(gate)
}

/// Turns the loop until the session file is being read.
fn until_reached(compositor: &mut Headless, gate: &Gate) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.reached() {
        anyhow::ensure!(Instant::now() < deadline, "the session was never read");
        compositor.dispatch(Duration::from_millis(5))?;
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

/// Given a session file slow to read, when the compositor starts, then
/// it starts and draws without waiting for it, and the card offers the
/// saved windows once it is read; asked again later, the same.
#[test]
fn a_slow_session_file_holds_back_neither_the_start_nor_a_frame() -> anyhow::Result<()> {
    let mut compositor = compositor()?;
    let gate = slow_read(&compositor)?;

    // The session ends as a crash would; the next one starts on its state.
    compositor.restart()?;
    until_reached(&mut compositor, &gate)?;
    compositor.advance(FRAME * 3)?;
    let shown = labels(&mut compositor)?;
    anyhow::ensure!(
        gate.open(),
        "the start waited for the saved session to be read"
    );
    assert!(
        !shown.iter().any(|l| l == "Welcome back"),
        "the card showed before the file was read: {shown:?}"
    );
    wait_for(&mut compositor, "Welcome back")?;
    wait_for(&mut compositor, "1 window was open.")?;
    click(&mut compositor, "Start fresh")?;

    // Asked again a second later (as tests do): the loop goes on meanwhile.
    compositor.advance(Duration::from_secs(1))?;
    let gate = slow_read(&compositor)?;
    compositor.request(Request::OfferSession)?;
    until_reached(&mut compositor, &gate)?;
    compositor.advance(FRAME * 3)?;
    let shown = labels(&mut compositor)?;
    anyhow::ensure!(
        gate.open(),
        "the request waited for the saved session to be read"
    );
    assert!(
        !shown.iter().any(|l| l == "Welcome back"),
        "the card showed again before the file was read: {shown:?}"
    );
    wait_for(&mut compositor, "Welcome back")?;
    hocus_test::report_held(&compositor.stats());
    Ok(())
}
