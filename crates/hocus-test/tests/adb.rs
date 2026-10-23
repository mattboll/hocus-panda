//! Wireless adb (#153) without adb installed: the session says it is
//! absent and logs nothing about it above debug, nor runs anything.

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use hocus_api::{Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};

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

#[test]
fn without_adb_nothing_shows_and_nothing_is_logged() -> anyhow::Result<()> {
    // Global: adb's worker logs on a thread of its own.
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // A PATH without adb, whatever the machine has.
    let empty = tempfile::tempdir()?;
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
        system_bus: None,
        data_dirs: None,
        child_env: Some(vec![("PATH".into(), empty.path().as_os_str().to_owned())]),
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
    })?;
    let phone = |compositor: &mut Headless| -> anyhow::Result<hocus_api::PhoneInfo> {
        let Reply::Phone(info) = compositor.request(Request::Phone)? else {
            anyhow::bail!("expected the phone");
        };
        Ok(info)
    };
    assert_eq!(phone(&mut compositor)?.adb, "off", "off by default");
    compositor.write_config("[phone]\nadb = true\n")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = phone(&mut compositor)?;
        if info.adb == "absent" {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "adb stayed {:?}", info.adb);
        compositor.dispatch(Duration::from_millis(10))?;
    }
    // No line in the column for it.
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    assert!(
        !nodes.iter().any(|n| n.label.as_deref() == Some("Phone")),
        "a phone line without adb"
    );

    let log = String::from_utf8_lossy(&captured.0.lock().map(|l| l.clone()).unwrap_or_default())
        .into_owned();
    let said: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("adb") || l.contains("debugging") || l.contains("phone"))
        .collect();
    assert!(said.is_empty(), "adb was logged about: {said:#?}");
    Ok(())
}
