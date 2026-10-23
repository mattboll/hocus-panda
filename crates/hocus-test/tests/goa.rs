//! Google calendars through GNOME Online Accounts (#137). Without GOA the
//! session shows nothing of it and logs nothing about it above debug; GOA
//! started later is found then. A token Google refuses is renewed through
//! GOA once, never more. Stand-ins on a private bus and 127.0.0.1 only:
//! never the session's bus, never Google. Needs `dbus-daemon`; skipped
//! without it.

use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use hocus_api::{CalendarsInfo, Reply, Request};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_platform::calendar::{
    caldav::FetchError,
    goa::{self, GoogleAccount},
};
use hocus_test::{
    bus::{PrivateBus, installed},
    goa::{ACCOUNT_ID, FakeGoa},
};
use jiff::Timestamp;

const ALEX: &str = "alex.panda@gmail.com";

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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/scenarios/calendar/google")
}

fn compositor(bus: &PrivateBus, google: &str) -> anyhow::Result<Headless> {
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
        google_caldav: Some(google.to_owned()),
    })
}

/// The calendars as the compositor says them, once `done` holds.
fn wait_calendars(
    compositor: &mut Headless,
    done: impl Fn(&CalendarsInfo) -> bool,
) -> anyhow::Result<CalendarsInfo> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        compositor.dispatch(Duration::from_millis(10))?;
        let Reply::Calendars(info) = compositor.request(Request::Calendars)? else {
            anyhow::bail!("expected the calendars");
        };
        if done(&info) {
            return Ok(info);
        }
        anyhow::ensure!(Instant::now() < deadline, "the calendars stayed {info:#?}");
    }
}

#[test]
fn without_goa_nothing_shows_and_nothing_is_logged() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    // Global: GOA is followed and synced on threads of their own.
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let bus = PrivateBus::start()?;
    // Google's stand-in is up, GOA's name not taken: GOA is not there.
    let goa = FakeGoa::start(&bus.address, ALEX, fixture(), false)?;
    let mut compositor = compositor(&bus, goa.google())?;
    let info = wait_calendars(&mut compositor, |i| i.online.state == "absent")?;
    assert!(info.online.accounts.is_empty() && info.events.is_empty());
    // A moment more, for anything that would come late.
    compositor.advance(Duration::from_secs(3))?;
    compositor.dispatch(Duration::from_millis(100))?;
    assert!(goa.asked().is_empty(), "{:?}", goa.asked());

    let log = String::from_utf8_lossy(&captured.0.lock().map(|l| l.clone()).unwrap_or_default())
        .into_owned();
    let said: Vec<&str> = log
        .lines()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("online accounts") || l.contains("goa") || l.contains("google")
        })
        .collect();
    assert!(said.is_empty(), "GOA was logged about: {said:#?}");

    // GOA started later is found then, and its calendar synced.
    goa.run()?;
    let info = wait_calendars(&mut compositor, |i| {
        i.online.state == "installed" && i.events.iter().any(|e| e.summary == "Family dinner")
    })?;
    assert_eq!(info.online.accounts.len(), 1);
    assert_eq!(info.online.accounts[0].name, ALEX);
    assert_eq!(info.online.accounts[0].id, format!("goa:{ACCOUNT_ID}"));
    assert!(info.online.accounts[0].synced);
    Ok(())
}

fn count(asked: &[String], what: &str) -> usize {
    asked.iter().filter(|l| l.starts_with(what)).count()
}

#[test]
fn a_refused_token_is_renewed_once() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let goa = FakeGoa::start(&bus.address, ALEX, fixture(), true)?;
    let account = GoogleAccount {
        path: format!("/org/gnome/OnlineAccounts/Accounts/{ACCOUNT_ID}"),
        id: ACCOUNT_ID.into(),
        identity: ALEX.into(),
        name: ALEX.into(),
    };
    let from: Timestamp = "2026-10-01T00:00:00Z".parse()?;
    let to: Timestamp = "2026-12-01T00:00:00Z".parse()?;
    let fetch = || goa::fetch(&bus.address, &account, goa.google(), from, to);

    let calendars = fetch()?;
    assert_eq!(calendars.len(), 1);
    assert_eq!(calendars[0].calendar, ALEX);
    assert!(calendars[0].ics.iter().any(|i| i.contains("Family dinner")));
    assert_eq!(count(&goa.asked(), "EnsureCredentials"), 0);

    // The token went stale at Google: renewed once, and it reads.
    goa.revoke();
    let calendars = fetch()?;
    assert!(calendars[0].ics.iter().any(|i| i.contains("Dentist")));
    let asked = goa.asked();
    assert_eq!(count(&asked, "EnsureCredentials"), 1, "{asked:#?}");
    assert_eq!(count(&asked, "GetAccessToken"), 3, "{asked:#?}");

    // Google refuses every token: one renewal, then a refusal.
    goa.break_tokens();
    let refused = fetch();
    assert!(
        matches!(refused, Err(FetchError::Refused(_))),
        "{refused:?}"
    );
    let asked = goa.asked();
    assert_eq!(count(&asked, "EnsureCredentials"), 2, "{asked:#?}");
    assert_eq!(count(&asked, "GetAccessToken"), 5, "{asked:#?}");
    // The refusal says nothing of the token.
    let said = format!("{refused:?}");
    assert!(!said.contains("ya29"), "{said}");

    // Without its account in GOA, the token is refused by GOA itself.
    goa.remove()?;
    let gone = fetch();
    assert!(gone.is_err(), "{gone:?}");
    Ok(())
}
