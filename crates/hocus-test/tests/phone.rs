//! The phone (#85) without KDE Connect: the session says it is absent and
//! logs nothing about it above debug; with a stand-in, files reach it
//! through the control API. Needs `dbus-daemon`; skipped without it.

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use hocus_api::{PhoneInfo, Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::{
    bus::{PrivateBus, installed},
    phone::FakePhone,
};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut log) = self.0.lock() {
            log.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn compositor(bus: &PrivateBus) -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
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
    })
}

/// The phone as the compositor says it, once `done` holds.
fn wait_phone(
    compositor: &mut Headless,
    done: impl Fn(&PhoneInfo) -> bool,
) -> anyhow::Result<PhoneInfo> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        compositor.dispatch(Duration::from_millis(10))?;
        let Reply::Phone(info) = compositor.request(Request::Phone)? else {
            anyhow::bail!("expected the phone");
        };
        if done(&info) {
            return Ok(info);
        }
        anyhow::ensure!(Instant::now() < deadline, "the phone stayed {info:?}");
    }
}

#[test]
fn without_kde_connect_nothing_shows_and_nothing_is_logged() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    // Global: the phone's own threads log too.
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let bus = PrivateBus::start()?;
    let mut compositor = compositor(&bus)?;
    let info = wait_phone(&mut compositor, |i| i.state == "absent")?;
    assert!(info.phones.is_empty());
    // Asked to send anyway, it says there is no phone.
    let refused = compositor.request(Request::SendToPhone {
        paths: vec!["/etc/hostname".into()],
    });
    assert!(refused.is_err(), "{refused:?}");

    let log = String::from_utf8_lossy(&captured.0.lock().map(|l| l.clone()).unwrap_or_default())
        .into_owned();
    let said: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("phone") || l.contains("kdeconnect") || l.contains("KDE Connect"))
        .collect();
    assert!(said.is_empty(), "the phone was logged about: {said:#?}");

    // KDE Connect started later is found then.
    let files = tempfile::tempdir()?;
    let phone = FakePhone::start(&bus.address, "Pixel 8", 82, false, files.path().into())?;
    let info = wait_phone(&mut compositor, |i| {
        i.state == "running" && !i.phones.is_empty()
    })?;
    assert_eq!(info.phones[0].name, "Pixel 8");
    assert_eq!(info.phones[0].battery, Some(82));

    let photo = files.path().join("Été 1.jpg");
    std::fs::write(&photo, b"jpeg")?;
    compositor.request(Request::SendToPhone {
        paths: vec![photo.to_string_lossy().into_owned()],
    })?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let wanted = format!("share {}", hocus_platform::phone::file_url(&photo));
    while !phone.asked().contains(&wanted) {
        anyhow::ensure!(
            Instant::now() < deadline,
            "the phone was asked {:?}",
            phone.asked()
        );
        compositor.dispatch(Duration::from_millis(10))?;
    }
    // Gone, it leaves the column.
    drop(phone);
    wait_phone(&mut compositor, |i| {
        i.state == "stopped" && i.phones.is_empty()
    })?;
    Ok(())
}
