//! A tray application slow to answer: the tray waits for it off the loop.
//! Needs `dbus-daemon`; skipped without it.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::{
    bus::{PrivateBus, installed},
    tray::TrayApp,
};

#[test]
fn a_slow_tray_application_never_holds_the_loop() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: Some(bus.address.clone()),
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
    })?;
    // Wait for the watcher, then register an application that takes
    // 300 ms for each of its properties (a second or more for all).
    let deadline = Instant::now() + Duration::from_secs(5);
    let app = loop {
        match TrayApp::start(
            &bus.address,
            "Slow",
            [200, 80, 80],
            &[],
            Duration::from_millis(300),
        ) {
            Ok(app) => break app,
            Err(_) if Instant::now() < deadline => {
                compositor.dispatch(Duration::from_millis(10))?;
            }
            Err(err) => return Err(err),
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut longest = Duration::ZERO;
    loop {
        let start = Instant::now();
        compositor.dispatch(Duration::from_millis(5))?;
        longest = longest.max(start.elapsed());
        let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
            anyhow::bail!("expected the interface");
        };
        if nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Tray: 1 application"))
        {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "the application never showed");
    }
    assert!(
        longest < Duration::from_millis(100),
        "a dispatch took {longest:?}"
    );
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("expected stats");
    };
    let held = hocus_test::held_the_loop(&stats);
    assert!(held.is_empty(), "the loop was held: {held:?}");
    drop(app);
    Ok(())
}
