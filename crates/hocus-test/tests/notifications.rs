//! A flood of notifications from one application: the rate limit holds it
//! off the loop. Needs `dbus-daemon`; skipped without it.

use std::time::{Duration, Instant};

use hocus_api::{Event, Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::bus::{Note, PrivateBus, installed, notify_on};

#[test]
fn a_flood_from_one_app_is_rate_limited_and_the_loop_keeps_its_budget() -> anyhow::Result<()> {
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
    })?;
    compositor.request(Request::Subscribe)?;

    let address = bus.address.clone();
    let flood = std::thread::spawn(move || -> anyhow::Result<Duration> {
        let connection =
            zbus::block_on(zbus::connection::Builder::address(address.as_str()).build())?;
        let start = Instant::now();
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
                },
            )?;
        }
        Ok(start.elapsed())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !flood.is_finished() && Instant::now() < deadline {
        compositor.dispatch(Duration::from_millis(5))?;
    }
    let took = flood
        .join()
        .map_err(|_| anyhow::anyhow!("the flood thread panicked"))??;
    assert!(took < Duration::from_secs(1), "the flood took {took:?}");
    // What was let through has reached the loop, and frames are drawn.
    compositor.dispatch(Duration::from_millis(100))?;
    compositor.advance(hocus_compositor::FRAME * 30)?;

    // Events are read off the socket along with the next reply.
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("expected stats");
    };
    let shown = compositor
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, Event::Notification { .. }))
        .count();
    assert_eq!(shown, 5, "the burst passes, then one a second");
    let held = hocus_test::held_the_loop(&stats);
    assert!(held.is_empty(), "the loop was held: {held:?}");
    Ok(())
}
