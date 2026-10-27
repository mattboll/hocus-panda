//! System services (#30): the status row follows a battery served on a
//! private bus, and a service slow to answer never makes the loop wait.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::{
    bus::{PrivateBus, installed},
    gate::Gate,
};
use zbus::object_server::SignalEmitter;

const PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

/// A UPower display device; a `gate` holds back whether it is present
/// until it opens.
struct Device {
    percent: f64,
    gate: Option<Gate>,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Device {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        true
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.percent
    }

    /// Discharging.
    #[zbus(property)]
    fn state(&self) -> u32 {
        2
    }

    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        3 * 3600
    }
}

fn serve(bus: &PrivateBus, percent: f64, gate: Option<Gate>) -> anyhow::Result<zbus::Connection> {
    Ok(zbus::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .serve_at(PATH, Device { percent, gate })
            .name("org.freedesktop.UPower")
            .build(),
    )?)
}

/// The column, which these tests read (#263: the strip by default).
const COLUMN: &str = "[panel]\nform = \"column\"\n";

fn compositor(bus: &PrivateBus) -> anyhow::Result<Headless> {
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: Some(bus.address.clone()),
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })?;
    compositor.write_config(COLUMN)?;
    Ok(compositor)
}

fn shows(compositor: &mut Headless, label: &str) -> anyhow::Result<bool> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    Ok(nodes.iter().any(|n| n.label.as_deref() == Some(label)))
}

/// Turns the loop, frame after frame, until `label` shows.
fn wait_for(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shows(compositor, label)? {
        anyhow::ensure!(Instant::now() < deadline, "{label} never showed");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    Ok(())
}

#[test]
fn the_status_row_follows_the_battery() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let service = serve(&bus, 64.0, None)?;
    let mut compositor = compositor(&bus)?;
    wait_for(&mut compositor, "Battery 64%")?;
    zbus::block_on(async {
        let iface = service.object_server().interface::<_, Device>(PATH).await?;
        iface.get_mut().await.percent = 63.0;
        let emitter: &SignalEmitter<'_> = iface.signal_emitter();
        iface.get().await.percentage_changed(emitter).await
    })?;
    wait_for(&mut compositor, "Battery 63%")?;
    Ok(())
}

/// Given a UPower that answers only once the test lets it, then frames are
/// drawn and requests answered while it is asked, and the battery shows
/// once it answers.
#[test]
fn a_slow_service_never_makes_the_loop_wait() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let gate = Gate::new();
    let _service = serve(&bus, 50.0, Some(gate.clone()))?;
    let mut compositor = compositor(&bus)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.reached() {
        anyhow::ensure!(Instant::now() < deadline, "the battery was never asked for");
        compositor.dispatch(Duration::from_millis(10))?;
    }
    // Frames go on while the service takes its time.
    for _ in 0..10 {
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    let early = shows(&mut compositor, "Battery 50%")?;
    anyhow::ensure!(gate.open(), "the loop waited for the service");
    assert!(!early, "the battery showed before the service answered");
    // And the answer arrives in the end.
    wait_for(&mut compositor, "Battery 50%")?;
    hocus_test::report_held(&compositor.stats());
    Ok(())
}
