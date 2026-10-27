//! A flood of notifications from one application: the server answers it
//! off the loop, and the rate limit holds it off the loop: what it drops
//! never wakes it. Needs `dbus-daemon`; skipped without it.

use std::time::{Duration, Instant};

use hocus_api::{Event, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::bus::{Note, PrivateBus, installed, notify_on};

#[test]
fn a_flood_from_one_app_is_answered_and_rate_limited_off_the_loop() -> anyhow::Result<()> {
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
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })?;
    compositor.request(Request::Subscribe)?;

    let address = bus.address.clone();
    let flood = std::thread::spawn(move || -> anyhow::Result<()> {
        let connection =
            zbus::block_on(zbus::connection::Builder::address(address.as_str()).build())?;
        for i in 0..100 {
            let summary = format!("message {i}");
            notify_on(
                &connection,
                &Note {
                    app: "flood",
                    replaces: 0,
                    summary: &summary,
                    body: "",
                    actions: &[],
                    urgency: 1,
                    timeout_ms: -1,
                    hints: &[],
                },
            )?;
        }
        Ok(())
    });
    // Answered without the loop: it does not turn until the flood is over.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !flood.is_finished() {
        anyhow::ensure!(
            Instant::now() < deadline,
            "the flood waited for the loop to answer it"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    flood
        .join()
        .map_err(|_| anyhow::anyhow!("the flood thread panicked"))??;
    // What was let through reaches the loop, and frames are drawn.
    compositor.dispatch(Duration::from_millis(100))?;
    compositor.advance(hocus_compositor::FRAME * 30)?;

    // Events are read off the socket along with the next reply.
    compositor.request(Request::Version)?;
    let shown = compositor
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, Event::Notification { .. }))
        .count();
    assert_eq!(shown, 5, "the burst passes, then one a second");
    // The rest never reached the loop: its handler ran once for each.
    let stats = compositor.stats();
    let ran = stats
        .handlers
        .iter()
        .find(|h| h.name == "notification")
        .map_or(0, |h| h.runs);
    assert_eq!(ran, 5, "the loop handled what the rate limit dropped");
    hocus_test::report_held(&stats);
    Ok(())
}
