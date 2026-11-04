//! Google Calendar through GNOME Online Accounts (#567): the day's workers
//! against an Online Accounts of the test's own, on a private bus, and a
//! Calendar API of its own on 127.0.0.1. Never the network, never the
//! session's bus. The clock is 4 November 2026, 14:00 UTC.
//!
//! One test per point of the acceptance.

mod fakes;

use std::{path::Path, sync::mpsc, time::Duration};

use fakes::{FakeApi, FakeGoa, PrivateBus, Sync, calendar, event};
use hocus_day::{
    Clock, Day, Env, State, Update, Workers,
    model::{Answer, Colour, FreeBusy, Status},
    secrets::Secrets,
};
use serde_json::json;

const ALEX: &str = "google-calendars-alex@example.org";

/// What a test runs against: a bus with Online Accounts on it, the API,
/// and the workers.
struct Bed {
    dir: tempfile::TempDir,
    // Dropped in this order: the workers, Online Accounts, its bus.
    workers: Workers,
    day: Day,
    updates: mpsc::Receiver<Update>,
    goa: FakeGoa,
    api: FakeApi,
    _bus: PrivateBus,
}

/// The fakes, Online Accounts with `accounts` (id, provider, identity),
/// the API set by `set`, then the workers of `config` (`[day]`).
fn start(
    accounts: &[(&str, &str, &str)],
    set: impl FnOnce(&mut fakes::Api),
    config: &str,
) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let bus = PrivateBus::start(dir.path())?;
    let goa = FakeGoa::start(&bus.address)?;
    for (id, provider, identity) in accounts {
        goa.add(id, provider, identity)?;
    }
    let api = FakeApi::start()?;
    api.with(set);
    let config = hocus_day::config::from_toml(config).map_err(anyhow::Error::msg)?;
    let env = Env {
        clock: Clock::new(now),
        secrets: Secrets::default(),
        state_dir: Some(dir.path().join("state")),
        cache_dir: Some(dir.path().join("cache")),
        config_dir: Some(dir.path().to_path_buf()),
        session_bus: Some(bus.address.clone()),
        google_api: hocus_day::google::api(Some(&api.address)),
    };
    let (out, updates) = mpsc::channel();
    let (workers, day) = Workers::start(&config, &env, &out);
    Ok(Bed {
        dir,
        workers,
        day,
        updates,
        goa,
        api,
        _bus: bus,
    })
}

/// The test's clock: 4 November 2026, 14:00 UTC.
fn now() -> jiff::Zoned {
    jiff::civil::date(2026, 11, 4)
        .at(14, 0, 0, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap_or_else(|_| jiff::Zoned::now())
}

const GOOGLE: &str = "[[day.calendars]]\nkind = \"google\"\n";

impl Bed {
    /// Waits for what the workers send until `ok` says the day is as
    /// expected.
    fn until(&mut self, what: &str, ok: impl Fn(&Day) -> bool) -> anyhow::Result<()> {
        while !ok(&self.day) {
            match self.updates.recv_timeout(Duration::from_secs(20)) {
                Ok(update) => {
                    self.day.apply(update);
                }
                Err(_) => anyhow::bail!("{what}: never so; the day: {:#?}", self.day),
            }
        }
        Ok(())
    }

    fn summary(&self) -> String {
        self.day.summary(&now())
    }

    fn account(&self, id: &str) -> Option<&hocus_day::Snapshot> {
        self.day
            .accounts()
            .iter()
            .find(|s| s.account.id.as_str() == id)
    }

    fn fresh(&self, id: &str) -> bool {
        self.account(id)
            .is_some_and(|s| matches!(s.state, State::Fresh { .. }))
    }

    fn cache(&self, id: &str) -> std::path::PathBuf {
        self.dir.path().join("cache").join(format!("{id}.json"))
    }
}

/// The two calendars of most tests: Travail, Alex's own; Fériés, read
/// only and not selected.
fn two_calendars(api: &mut fakes::Api) {
    api.calendars = vec![
        calendar(
            "alex@example.org",
            "alex@example.org",
            "#6CC287",
            "owner",
            true,
        ),
        calendar(
            "fr.french#holiday@group.v.calendar.google.com",
            "Jours fériés",
            "#F5A35B",
            "reader",
            false,
        ),
    ];
    api.calendars[0]["summaryOverride"] = json!("Travail");
}

#[test]
fn the_calendars_and_todays_events_reach_the_snapshot() -> anyhow::Result<()> {
    let mut bed = start(
        &[
            ("account_1", "google", "alex@example.org"),
            // Not Google: left out.
            ("account_2", "ms365", "alex@example.com"),
        ],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (
                    vec![
                        event("e1", "Point produit", "14:25", "14:55"),
                        event("e2", "Revue de sprint", "16:00", "17:00"),
                        // A working location: not an event of the day.
                        json!({
                            "id": "w1", "eventType": "workingLocation", "summary": "Home",
                            "start": {"date": "2026-11-04"}, "end": {"date": "2026-11-05"},
                        }),
                        // Tomorrow's: not today's.
                        json!({
                            "id": "e3", "summary": "Congés de Lou",
                            "start": {"date": "2026-11-05"}, "end": {"date": "2026-11-06"},
                        }),
                    ],
                    "s1".to_owned(),
                ),
            );
        },
        GOOGLE,
    )?;
    bed.until("Alex's account read", |d| {
        d.accounts()
            .iter()
            .any(|s| s.account.id.as_str() == ALEX && matches!(s.state, State::Fresh { .. }))
    })?;
    let ids: Vec<&str> = bed
        .day
        .accounts()
        .iter()
        .map(|s| s.account.id.as_str())
        .collect();
    assert_eq!(ids, [ALEX, "local"]);
    let alex = bed.account(ALEX).expect("Alex's account");
    assert_eq!(alex.account.name, "alex@example.org");
    let calendars: Vec<(&str, Colour, bool, bool)> = alex
        .calendars
        .iter()
        .map(|c| (c.name.as_str(), c.colour, c.read_only, c.shown))
        .collect();
    assert_eq!(
        calendars,
        [
            (
                "Travail",
                "#6CC287".parse().map_err(anyhow::Error::msg)?,
                false,
                true
            ),
            (
                "Jours fériés",
                "#F5A35B".parse().map_err(anyhow::Error::msg)?,
                true,
                false
            ),
        ]
    );
    let summary = bed.summary();
    assert!(
        summary.contains(
            "today=2=[14:25 «Point produit» (Travail); 16:00 «Revue de sprint» (Travail)]"
        ),
        "{summary}"
    );
    // The event's page, and its alarms: the calendar's own reminders.
    let first = &alex.events[0];
    assert_eq!(
        first.page.as_deref(),
        Some("https://calendar.google.com/event?eid=e1")
    );
    assert_eq!(first.alarms, [10]);
    // The hidden calendar's events are not asked for; the window is, its
    // occurrences expanded, with Online Accounts' token.
    let asked = bed.api.asked();
    let listing = asked
        .iter()
        .find(|a| a.path == "/calendars/alex@example.org/events")
        .expect("Travail listed");
    assert_eq!(
        listing.query.get("singleEvents").map(String::as_str),
        Some("true")
    );
    assert!(listing.query.contains_key("timeMin") && listing.query.contains_key("timeMax"));
    assert_eq!(listing.token, "tok-1");
    assert!(
        !asked.iter().any(|a| a.path.contains("holiday")),
        "{asked:?}"
    );
    assert!(bed.cache(ALEX).exists());
    Ok(())
}

#[test]
fn a_declined_event_a_free_one_and_a_cancelled_one_are_marked_as_such() -> anyhow::Result<()> {
    let mut declined = event("d", "Comité", "09:00", "10:00");
    declined["attendees"] = json!([
        {"email": "lou@example.org", "organizer": true, "responseStatus": "accepted"},
        {"email": "alex@example.org", "self": true, "responseStatus": "declined"},
    ]);
    let mut free = event("f", "Déjeuner", "12:00", "13:00");
    free["transparency"] = json!("transparent");
    let mut cancelled = event("c", "Point annulé", "15:00", "15:30");
    cancelled["status"] = json!("cancelled");
    let mut accepted = event("a", "Atelier", "16:00", "17:00");
    accepted["attendees"] =
        json!([{"email": "alex@example.org", "self": true, "responseStatus": "accepted"}]);
    accepted["location"] = json!("Salle Jupiter");
    accepted["reminders"] = json!({"useDefault": false, "overrides": [
        {"method": "popup", "minutes": 5}, {"method": "email", "minutes": 60}, {"method": "popup", "minutes": 30},
    ]});
    let mut bed = start(
        &[("account_1", "google", "alex@example.org")],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (vec![declined, free, cancelled, accepted], "s1".to_owned()),
            );
        },
        GOOGLE,
    )?;
    bed.until("Alex's account read", |d| {
        d.accounts().iter().any(|s| s.events.len() == 4)
    })?;
    let alex = bed.account(ALEX).expect("Alex's account");
    let find = |id: &str| alex.events.iter().find(|e| e.id.as_str() == id).expect(id);
    assert_eq!(find("d").answer, Answer::Declined);
    assert_eq!(find("f").free_busy, FreeBusy::Free);
    assert_eq!(find("c").status, Status::Cancelled);
    let a = find("a");
    assert_eq!(
        (a.status, a.answer, a.free_busy),
        (Status::Confirmed, Answer::Accepted, FreeBusy::Busy)
    );
    assert_eq!(a.place.as_deref(), Some("Salle Jupiter"));
    // Its own reminders, the screen's only, earliest first.
    assert_eq!(a.alarms, [30, 5]);
    // Of the four, only the accepted one takes time in the bar.
    let taking: Vec<&str> = alex
        .events
        .iter()
        .filter(|e| e.takes_time())
        .map(|e| e.title.as_str())
        .collect();
    assert_eq!(taking, ["Atelier"]);
    Ok(())
}

#[test]
fn the_video_link_comes_from_hangout_link_or_conference_data() -> anyhow::Result<()> {
    let mut meet = event("m", "Point produit", "14:25", "14:55");
    meet["hangoutLink"] = json!("https://meet.google.com/abc-defg-hij");
    let mut zoom = event("z", "Client", "15:00", "16:00");
    zoom["conferenceData"] = json!({"entryPoints": [
        {"entryPointType": "phone", "uri": "tel:+33-1-23-45-67-89"},
        {"entryPointType": "video", "uri": "https://acme.zoom.us/j/123"},
    ]});
    let mut described = event("t", "Revue", "16:00", "17:00");
    described["description"] = json!(
        "Ordre du jour : https://docs.example.org/a\n<a href=\"https://teams.microsoft.com/l/meetup-join/x\">Rejoindre</a>"
    );
    let plain = event("p", "Piscine", "18:00", "19:00");
    let mut bed = start(
        &[("account_1", "google", "alex@example.org")],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (vec![meet, zoom, described, plain], "s1".to_owned()),
            );
        },
        GOOGLE,
    )?;
    bed.until("Alex's account read", |d| {
        d.accounts().iter().any(|s| s.events.len() == 4)
    })?;
    let alex = bed.account(ALEX).expect("Alex's account");
    let videos: Vec<(&str, Option<&str>)> = alex
        .events
        .iter()
        .map(|e| (e.id.as_str(), e.video.as_deref()))
        .collect();
    assert_eq!(
        videos,
        [
            ("m", Some("https://meet.google.com/abc-defg-hij")),
            ("z", Some("https://acme.zoom.us/j/123")),
            ("t", Some("https://teams.microsoft.com/l/meetup-join/x")),
            ("p", None),
        ]
    );
    Ok(())
}

#[test]
fn an_incremental_sync_applies_its_changes_and_a_410_lists_again() -> anyhow::Result<()> {
    let mut bed = start(
        &[("account_1", "google", "alex@example.org")],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (
                    vec![
                        event("e1", "Point produit", "14:25", "14:55"),
                        event("e2", "Revue de sprint", "16:00", "17:00"),
                        event("e3", "Rétro", "17:00", "17:30"),
                    ],
                    "s1".to_owned(),
                ),
            );
            // From s1: e1 moved, e2 deleted (an id alone), e4 new.
            api.syncs.insert(
                "s1".to_owned(),
                Sync::Changes {
                    items: vec![
                        event("e1", "Point produit", "14:40", "15:10"),
                        json!({"id": "e2", "status": "cancelled"}),
                        event("e4", "Café", "15:30", "15:45"),
                    ],
                    next: "s2".to_owned(),
                },
            );
            api.syncs.insert("s2".to_owned(), Sync::Expired);
        },
        GOOGLE,
    )?;
    bed.until("listed", |d| {
        d.accounts().iter().any(|s| s.events.len() == 3)
    })?;
    bed.workers.refresh();
    bed.until("synced", |d| d.summary(&now()).contains("Café"))?;
    let summary = bed.summary();
    assert!(
        summary.contains("today=3=[14:40 «Point produit» (Travail); 15:30 «Café» (Travail); 17:00 «Rétro» (Travail)]"),
        "{summary}"
    );
    let synced = bed
        .api
        .asked()
        .iter()
        .filter(|a| a.query.get("syncToken").map(String::as_str) == Some("s1"))
        .map(|a| a.query.get("pageToken").cloned())
        .collect::<Vec<_>>();
    // Its two pages, the token said on each.
    assert_eq!(synced, [None, Some("1".to_owned())]);
    // From s2, Google says the token expired: the calendar is listed
    // again, its events as the listing says.
    bed.api.with(|api| {
        api.events.insert(
            "alex@example.org".to_owned(),
            (vec![event("e5", "Démo", "16:30", "17:00")], "s3".to_owned()),
        );
    });
    bed.workers.refresh();
    bed.until("listed again", |d| {
        d.accounts()
            .iter()
            .any(|s| s.events.len() == 1 && s.events[0].title == "Démo")
    })?;
    let asked = bed.api.asked();
    let last_two: Vec<Option<&str>> = asked
        .iter()
        .filter(|a| a.path.ends_with("/events"))
        .rev()
        .take(2)
        .map(|a| a.query.get("syncToken").map(String::as_str))
        .collect();
    assert_eq!(last_two, [None, Some("s2")]);
    Ok(())
}

#[test]
fn a_401_asks_online_accounts_again_and_its_refusal_means_signing_in_again() -> anyhow::Result<()> {
    let mut bed = start(
        &[("account_1", "google", "alex@example.org")],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (
                    vec![event("e1", "Point produit", "14:25", "14:55")],
                    "s1".to_owned(),
                ),
            );
            // The first token Online Accounts gives is refused, as one
            // revoked before it expired.
            api.accept.insert("tok-2".to_owned());
        },
        GOOGLE,
    )?;
    bed.until("read with a second token", |d| {
        d.accounts().iter().any(|s| s.events.len() == 1)
    })?;
    assert!(bed.fresh(ALEX));
    assert_eq!(bed.goa.given(), 2);
    let tokens: Vec<String> = bed.api.asked().iter().map(|a| a.token.clone()).collect();
    assert_eq!(tokens.first().map(String::as_str), Some("tok-1"));
    assert!(tokens.iter().skip(1).all(|t| t == "tok-2"), "{tokens:?}");
    // Google refuses the token again, and Online Accounts can give none:
    // the account needs signing in again, its events kept.
    bed.api.with(|api| {
        api.accept.clear();
        api.accept.insert("tok-none".to_owned());
    });
    bed.goa.refuse(true);
    bed.workers.refresh();
    bed.until("signing in again", |d| {
        d.accounts().iter().any(|s| {
            matches!(
                &s.state,
                State::Failed {
                    error: hocus_day::SourceError::SignIn(_),
                    fetched: Some(_)
                }
            )
        })
    })?;
    let summary = bed.summary();
    assert!(
        summary.contains(&format!("{ALEX} failed (sign in again:")),
        "{summary}"
    );
    assert!(summary.contains("«Point produit»"), "{summary}");
    Ok(())
}

#[test]
fn an_account_added_in_online_accounts_starts_its_worker_and_one_removed_stops_it()
-> anyhow::Result<()> {
    let mut bed = start(
        &[("account_2", "google", "lou@example.org")],
        |api| {
            two_calendars(api);
            api.events.insert(
                "alex@example.org".to_owned(),
                (
                    vec![event("e1", "Point produit", "14:25", "14:55")],
                    "s1".to_owned(),
                ),
            );
        },
        // Only Alex's account: Lou's is left out.
        "[[day.calendars]]\nkind = \"google\"\naccount = \"alex@example.org\"\n\
         colors = { \"Travail\" = \"#C89BFF\" }\n",
    )?;
    let ids: Vec<&str> = bed
        .day
        .accounts()
        .iter()
        .map(|s| s.account.id.as_str())
        .collect();
    assert_eq!(ids, ["local"]);
    bed.goa.add("account_1", "google", "alex@example.org")?;
    bed.until("Alex's account started", |d| {
        d.accounts().iter().any(|s| s.events.len() == 1)
    })?;
    assert!(bed.fresh(ALEX));
    assert!(bed.account("google-calendars-lou@example.org").is_none());
    let colour: Colour = "#C89BFF".parse().map_err(anyhow::Error::msg)?;
    assert_eq!(
        bed.account(ALEX).map(|s| s.calendars[0].colour),
        Some(colour)
    );
    // One worker (a thread it starts without a name of its own, as the
    // HTTP client's name lookup, bears its name a moment).
    wait_for(|| threads("day: google") == 1)?;
    // Its calendar switched off: it stops; on: it starts again.
    bed.goa.set_calendar("account_1", false)?;
    bed.until("Alex's calendar off", |d| d.accounts().len() == 1)?;
    wait_for(|| threads("day: google") == 0)?;
    bed.goa.set_calendar("account_1", true)?;
    bed.until("Alex's calendar on", |d| {
        d.accounts().iter().any(|s| s.events.len() == 1)
    })?;
    // Removed: its worker stops, its cache is forgotten, and nothing more
    // is asked of Google for it.
    bed.goa.remove("account_1")?;
    bed.until("Alex's account removed", |d| d.accounts().len() == 1)?;
    wait_for(|| threads("day: google") == 0)?;
    assert!(!bed.cache(ALEX).exists());
    let asked = bed.api.asked().len();
    bed.workers.refresh();
    bed.workers.flush(Duration::from_secs(10));
    assert_eq!(bed.api.asked().len(), asked);
    Ok(())
}

#[test]
fn without_a_session_bus_there_is_no_google_source() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let config = hocus_day::config::from_toml(GOOGLE).map_err(anyhow::Error::msg)?;
    let env = Env {
        clock: Clock::system(),
        secrets: Secrets::default(),
        state_dir: Some(dir.path().join("state")),
        cache_dir: Some(dir.path().join("cache")),
        config_dir: None,
        session_bus: None,
        google_api: hocus_day::google::API.to_owned(),
    };
    let (out, _updates) = mpsc::channel();
    let (_workers, day) = Workers::start(&config, &env, &out);
    let ids: Vec<&str> = day
        .accounts()
        .iter()
        .map(|s| s.account.id.as_str())
        .collect();
    assert_eq!(ids, ["local"]);
    assert_eq!(threads("day: online"), 0);
    Ok(())
}

/// How many of this process's threads are named from `prefix` (the
/// kernel keeps 15 bytes of a name): each test is a process of its own
/// under nextest, as in CI.
fn threads(prefix: &str) -> usize {
    let prefix = &prefix[..prefix.len().min(15)];
    std::fs::read_dir(Path::new("/proc/self/task"))
        .map(|tasks| {
            tasks
                .flatten()
                .filter(|t| {
                    std::fs::read_to_string(t.path().join("comm"))
                        .is_ok_and(|name| name.starts_with(prefix))
                })
                .count()
        })
        .unwrap_or(0)
}

/// Waits until `ok`, a while at most.
fn wait_for(ok: impl Fn() -> bool) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    while !ok() {
        anyhow::ensure!(start.elapsed() < Duration::from_secs(20), "never so");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
