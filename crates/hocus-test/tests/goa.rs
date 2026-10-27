//! Google calendars through GNOME Online Accounts (#137). Without GOA the
//! session shows nothing of it and logs nothing about it above debug; GOA
//! started later is found then. A token Google refuses is renewed through
//! GOA once, never more. Google Tasks (#257) are read and written with the
//! same token; one without their scope leaves the calendars be. Following GOA, or not, never keeps the loop from
//! sleeping. Stand-ins on a private bus and 127.0.0.1 only:
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
use hocus_platform::{
    calendar::{
        caldav::FetchError,
        goa::{self, GoogleAccount},
        gtasks,
    },
    tasks::Task,
};
use hocus_test::{
    bus::{PrivateBus, installed},
    goa::{ACCOUNT_ID, FakeGoa},
};
use jiff::Timestamp;

const ALEX: &str = "alex.panda@example.com";

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
    start(Some(bus), Some(google))
}

fn start(bus: Option<&PrivateBus>, google: Option<&str>) -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: bus.map(|b| b.address.clone()),
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: google.map(str::to_owned),
        google_tasks: None,
    })
}

/// How many times the loop woke while nothing happened for `real`, each
/// dispatch waiting 20 ms at most.
fn wakes(compositor: &mut Headless, real: Duration) -> anyhow::Result<u32> {
    let until = Instant::now() + real;
    let mut n = 0;
    while Instant::now() < until {
        compositor.dispatch(Duration::from_millis(20))?;
        n += 1;
    }
    Ok(n)
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

/// Whether a line of the log is about GOA: it names it, Google, or
/// GNOME's online accounts, as words. Not a part of a word: the
/// compositor's private directory, logged at start, has a random name, and
/// one in some thousands holds "goa" (`/tmp/hocus-ggOAlK`, #292).
fn about_goa(line: &str) -> bool {
    let line = line.to_lowercase();
    line.contains("online accounts")
        || line
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "goa" || word == "google")
}

#[test]
fn a_random_name_is_not_about_goa() {
    assert!(!about_goa(
        "INFO hocus_compositor: compositor ready socket=/tmp/hocus-ggOAlK/hocus-0 \
         control=/tmp/hocus-ggOAlK/hocus-0.api permission=Test"
    ));
    assert!(!about_goa("socket=/tmp/hocus-xGooGLe/hocus-0"));
    assert!(about_goa(
        "DEBUG hocus_platform::calendar::goa: GNOME Online Accounts is not followed"
    ));
    assert!(about_goa(
        "WARN hocus_compositor::calendar: Google calendars will not be followed"
    ));
    assert!(about_goa(
        "WARN a calendar did not sync id=\"goa:account_1\""
    ));
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
    let said: Vec<&str> = log.lines().filter(|l| about_goa(l)).collect();
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

/// The loop sleeps when nothing happens, GOA looked for or not: what
/// follows it is never touched on each dispatch (a clone of a channel's
/// sender, dropped, wakes the loop at once, which then spins).
#[test]
fn the_loop_sleeps_with_or_without_goa() -> anyhow::Result<()> {
    // No session bus: GOA is not looked for.
    let mut alone = start(None, None)?;
    alone.dispatch(Duration::from_millis(100))?;
    let n = wakes(&mut alone, Duration::from_millis(400))?;
    assert!(
        n <= 40,
        "without a session bus the loop woke {n} times in 400 ms"
    );
    drop(alone);
    if !installed("dbus-daemon") {
        eprintln!("the rest skipped: no dbus-daemon");
        return Ok(());
    }
    // GOA looked for and absent.
    let bus = PrivateBus::start()?;
    let mut compositor = start(Some(&bus), None)?;
    wait_calendars(&mut compositor, |i| i.online.state == "absent")?;
    let n = wakes(&mut compositor, Duration::from_millis(400))?;
    assert!(n <= 40, "without GOA the loop woke {n} times in 400 ms");
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

fn alex() -> GoogleAccount {
    GoogleAccount {
        path: format!("/org/gnome/OnlineAccounts/Accounts/{ACCOUNT_ID}"),
        id: ACCOUNT_ID.into(),
        identity: ALEX.into(),
        name: ALEX.into(),
    }
}

fn tasks_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/scenarios/calendar/google-tasks.json")
}

#[test]
fn google_tasks_are_read_and_written_with_goa_s_token() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let goa = FakeGoa::start(&bus.address, ALEX, fixture(), true)?;
    goa.load_tasks(&tasks_fixture())?;
    let account = alex();
    let range = (
        "2026-10-01T00:00:00Z".parse::<Timestamp>()?,
        "2026-12-01T00:00:00Z".parse::<Timestamp>()?,
    );
    let id = format!("goa:{ACCOUNT_ID}");
    let sync = || {
        goa::sync(
            &bus.address,
            &account,
            goa.google(),
            Some(goa.tasks()),
            &id,
            range,
        )
    };

    // One token for the calendar and the tasks.
    let (calendars, lists) = sync()?;
    assert!(calendars[0].ics.iter().any(|i| i.contains("Family dinner")));
    let lists = lists.expect("asked").expect("read");
    let names: Vec<_> = lists
        .iter()
        .map(|l| (l.name.as_str(), l.tasks.len()))
        .collect();
    assert_eq!(names, [("My Tasks", 3), ("Groceries", 1)]);
    assert_eq!(count(&goa.asked(), "GetAccessToken"), 1);

    // Written: added, changed, deleted.
    let now: Timestamp = "2026-10-21T14:37:00Z".parse()?;
    let new = Task::new_google(&id, "Z3JvY2VyaWVz", "Groceries", "Oat milk", None, now)?;
    let added = goa::with_tasks(&bus.address, &account, |token| {
        gtasks::insert(
            goa.tasks(),
            token,
            "Z3JvY2VyaWVz",
            &gtasks::body(&new, None),
        )
    })?;
    let added_id = gtasks::id_of(&added).expect("an id");
    let mut done = gtasks::parse(&added, &id, "Z3JvY2VyaWVz", "Groceries").expect("a task");
    let before = done.clone();
    done.set_done(true, now);
    let patched = goa::with_tasks(&bus.address, &account, |token| {
        gtasks::patch(
            goa.tasks(),
            token,
            "Z3JvY2VyaWVz",
            &added_id,
            &gtasks::body(&done, Some(&before)),
        )
    })?;
    assert!(patched.is_some_and(|t| t.contains("\"status\":\"completed\"")));
    let kept = goa.google_task("Oat milk").expect("kept");
    assert!(
        kept.contains("\"completed\":\"2026-10-21T14:37:00.000Z\""),
        "{kept}"
    );
    goa::with_tasks(&bus.address, &account, |token| {
        gtasks::delete(goa.tasks(), token, "Z3JvY2VyaWVz", &added_id)
    })?;
    assert_eq!(goa.google_task("Oat milk"), None);
    // Gone already: gone.
    goa::with_tasks(&bus.address, &account, |token| {
        gtasks::delete(goa.tasks(), token, "Z3JvY2VyaWVz", &added_id)
    })?;

    // A stale token is renewed once, for the tasks too.
    goa.revoke();
    let read = goa::with_tasks(&bus.address, &account, |token| {
        gtasks::lists(goa.tasks(), token, &id)
    })?;
    assert_eq!(read.len(), 2);
    assert_eq!(count(&goa.asked(), "EnsureCredentials"), 1);

    // Without the tasks scope: the calendars still read, the tasks say so.
    goa.share_tasks(false);
    let (calendars, lists) = sync()?;
    assert!(!calendars.is_empty());
    let refused = lists.expect("asked");
    assert!(
        matches!(refused, Err(gtasks::Error::Unshared(_))),
        "{refused:?}"
    );
    // No renewal for that: the token is good, its scope is not.
    assert_eq!(count(&goa.asked(), "EnsureCredentials"), 1);
    assert!(!format!("{refused:?}").contains("ya29"));
    Ok(())
}
