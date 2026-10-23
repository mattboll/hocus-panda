//! Calendars (#75): CalDAV accounts and subscriptions synced on worker
//! threads every few minutes, kept in a sealed cache for when the network
//! is down, expanded into the next weeks' events. The column shows the next
//! meeting, with Join when it has a call; reminders come ten minutes and
//! one minute before. An account taken out of the configuration takes its
//! cached events and its password with it.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

use hocus_api::{CalendarStatus, CalendarsInfo, Event, EventInfo};
use hocus_core::config::CalendarAccount;
use hocus_platform::{
    calendar::{
        caldav::{self, FetchError, Source},
        ics::{self, Occurrence},
    },
    sealed::Sealed,
    secrets::Secrets,
};
use hocus_shell::column::{Agenda, Meeting};
use jiff::{SignedDuration, Timestamp, ToSpan, civil::Date, tz::TimeZone};
use smithay::reexports::calloop::{
    LoopHandle, channel,
    timer::{TimeoutAction, Timer},
};

use crate::{App, State};

/// Between syncs.
const SYNC_EVERY: Duration = Duration::from_secs(600);
/// The first one, once the session has settled.
const FIRST_SYNC: Duration = Duration::from_secs(2);
/// Reminders, in minutes before.
const REMIND: [i64; 2] = [10, 1];
/// Join shows this long before.
const JOIN_BEFORE: i64 = 10 * 60;
/// The events kept: from yesterday to this many days ahead.
const AHEAD_DAYS: i64 = 42;

#[derive(Debug)]
enum News {
    /// The cache, read at start.
    Cached(HashMap<String, Vec<String>>),
    /// An account's events, from the sync of this generation.
    Fetched(String, u64, Result<Vec<String>, FetchError>),
    /// The secrets kept now; a password was just stored, so sync again.
    Secrets { count: usize, stored: bool },
}

#[derive(Debug, Default)]
pub struct Calendar {
    /// Each account's events as iCalendar texts, as last fetched.
    texts: HashMap<String, Vec<String>>,
    /// The coming weeks' events, sorted, with their account.
    events: Vec<(String, Occurrence)>,
    errors: HashMap<String, FetchError>,
    synced: HashSet<String>,
    /// The accounts configured, to see one come or go; unknown until the
    /// configuration is read, and nothing is forgotten before.
    known: Option<Vec<String>>,
    next_sync: Option<Duration>,
    /// Bumped at each sync: what an older one brings back is stale.
    generation: u64,
    /// Reminders shown: event, start, minutes before.
    reminded: HashSet<(String, Timestamp, i64)>,
    /// Reminder toasts with Join, and their links.
    joins: HashMap<u32, String>,
    news: Option<channel::Sender<News>>,
    cache: Option<PathBuf>,
    /// The cache was read: saving before would lose it.
    cache_read: bool,
    secrets: Option<Secrets>,
    secrets_kept: usize,
    /// The day the events were expanded on.
    expanded_on: Option<Date>,
}

pub fn start(
    handle: &LoopHandle<'static, App>,
    state: &mut State,
    dir: Option<PathBuf>,
    session_bus: Option<&str>,
) {
    let (news, received) = channel::channel::<News>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("calendar", |app| app.state.calendar_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no calendars");
        return;
    }
    state.calendar.news = Some(news.clone());
    state.calendar.next_sync = Some(state.clock.now() + FIRST_SYNC);
    // Wakes the loop: syncs and reminders fall due on the clock.
    let ticking = handle.insert_source(Timer::from_duration(FIRST_SYNC), |_, _, app| {
        app.state.needs_redraw = true;
        TimeoutAction::ToDuration(Duration::from_secs(30))
    });
    if let Err(err) = ticking {
        tracing::warn!(%err.error, "reminders will come only when something happens");
    }
    let Some(dir) = dir else {
        return;
    };
    let cache = dir.join("calendar-cache");
    state.calendar.cache = Some(cache.clone());
    let secrets = Secrets {
        // The session's own keyring only: nested, the developer's is not ours.
        bus: state
            .is_session
            .then(|| session_bus.map(str::to_owned))
            .flatten(),
        fallback: dir.join("calendar-secrets"),
        attribute: "calendar",
    };
    state.calendar.secrets = Some(secrets.clone());
    spawn("calendar cache", move || {
        let cached = match Sealed::new(&cache).read() {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Ok(None) => HashMap::new(),
            Err(err) => {
                tracing::warn!(err, "the calendar cache could not be read");
                HashMap::new()
            }
        };
        let _ = news.send(News::Cached(cached));
    });
}

impl State {
    /// The user's time zone; UTC on the virtual clock, unless the
    /// configuration chose one.
    fn wall_tz(&self) -> TimeZone {
        self.wall.zone()
    }

    /// Now, by the user's clock (virtual in tests).
    fn wall_now(&self) -> Timestamp {
        self.wall_civil()
            .to_zoned(self.wall_tz())
            .map_or_else(|_| Timestamp::now(), |z| z.timestamp())
    }

    fn calendar_news(&mut self, news: News) {
        match news {
            News::Cached(cached) => {
                // What a sync brought meanwhile stays, and accounts gone
                // stay gone.
                let mut dropped = false;
                for (id, texts) in cached {
                    if self.calendar.known.as_ref().is_none_or(|k| k.contains(&id)) {
                        self.calendar.texts.entry(id).or_insert(texts);
                    } else {
                        dropped = true;
                    }
                }
                self.calendar.cache_read = true;
                if dropped {
                    self.save_calendar_cache();
                }
            }
            News::Fetched(id, generation, _)
                if generation != self.calendar.generation
                    || !self
                        .calendar
                        .known
                        .as_ref()
                        .is_some_and(|k| k.contains(&id)) =>
            {
                return;
            }
            News::Fetched(id, _, Ok(texts)) => {
                self.calendar.errors.remove(&id);
                self.calendar.synced.insert(id.clone());
                self.calendar.texts.insert(id, texts);
                self.save_calendar_cache();
            }
            News::Fetched(id, _, Err(err)) => {
                tracing::warn!(id, %err, "a calendar did not sync; its cache stays");
                self.calendar.errors.insert(id, err);
            }
            News::Secrets { count, stored } => {
                self.calendar.secrets_kept = count;
                if stored {
                    self.calendar.next_sync = Some(self.clock.now());
                }
            }
        }
        self.expand_calendar();
        self.emit(|_| Event::Calendars);
    }

    fn save_calendar_cache(&self) {
        let Some(path) = self
            .calendar
            .cache
            .clone()
            .filter(|_| self.calendar.cache_read)
        else {
            return;
        };
        let texts = self.calendar.texts.clone();
        spawn("calendar cache", move || {
            let saved = serde_json::to_vec(&texts)
                .map_err(|e| e.to_string())
                .and_then(|bytes| Sealed::new(&path).write(&bytes));
            if let Err(err) = saved {
                tracing::warn!(err, "the calendar cache was not saved");
            }
        });
    }

    /// The coming weeks' events, from every account's texts.
    fn expand_calendar(&mut self) {
        let tz = self.wall_tz();
        let today = self.wall_now().to_zoned(tz.clone()).date();
        let day = |d: Date| d.to_zoned(tz.clone()).map(|z| z.timestamp());
        let (Ok(from), Ok(to)) = (
            day(today.first_of_month()),
            day(today.saturating_add(AHEAD_DAYS.days())),
        ) else {
            return;
        };
        let mut events = Vec::new();
        for (id, texts) in &self.calendar.texts {
            let parsed: Vec<ics::VEvent> = texts.iter().flat_map(|t| ics::parse(t)).collect();
            events.extend(
                ics::expand(&parsed, from, to, &tz)
                    .into_iter()
                    .map(|o| (id.clone(), o)),
            );
        }
        events.sort_by(|(_, a), (_, b)| (a.start, &a.uid).cmp(&(b.start, &b.uid)));
        // The same event in two accounts (shared calendars): once.
        events.dedup_by(|(_, a), (_, b)| a.uid == b.uid && a.start == b.start);
        self.calendar.events = events;
        self.calendar.expanded_on = Some(today);
        self.needs_redraw = true;
    }

    /// After each dispatch: accounts that came or went, syncs and
    /// reminders that fall due, a new day.
    pub fn refresh_calendar(&mut self) {
        // Until the configuration is read, its accounts are not known.
        if self.config_path.is_some() && !self.config_read {
            return;
        }
        let ids: Vec<String> = self
            .config
            .calendars
            .iter()
            .map(CalendarAccount::id)
            .collect();
        let now = self.clock.now();
        if self.calendar.known.as_ref() != Some(&ids) {
            self.calendar_accounts_changed(ids);
            self.calendar.next_sync = Some(now);
        }
        let due = self.calendar.next_sync.is_some_and(|t| now >= t);
        if due && !self.config.calendars.is_empty() {
            self.calendar.next_sync = Some(now + SYNC_EVERY);
            self.sync_calendars();
        }
        let today = self.wall_now().to_zoned(self.wall_tz()).date();
        if self.calendar.expanded_on.is_some_and(|d| d != today) {
            self.expand_calendar();
        }
        self.remind();
    }

    /// Accounts gone take their events, their cache and their password;
    /// the first time, those taken out while the session was off.
    fn calendar_accounts_changed(&mut self, ids: Vec<String>) {
        let first = self.calendar.known.is_none();
        let mut gone: Vec<String> = self
            .calendar
            .known
            .iter()
            .flatten()
            .chain(self.calendar.texts.keys())
            .filter(|k| !ids.contains(k))
            .cloned()
            .collect();
        gone.sort();
        gone.dedup();
        self.calendar.known = Some(ids.clone());
        if gone.is_empty() && !first {
            return;
        }
        for id in &gone {
            self.calendar.texts.remove(id);
            self.calendar.errors.remove(id);
            self.calendar.synced.remove(id);
        }
        self.save_calendar_cache();
        self.expand_calendar();
        self.emit(|_| Event::Calendars);
        let (Some(secrets), Some(news)) =
            (self.calendar.secrets.clone(), self.calendar.news.clone())
        else {
            return;
        };
        spawn("calendar secrets", move || {
            if let Err(err) = secrets.keep_only(&ids) {
                tracing::warn!(err, "old calendar passwords were not forgotten");
            }
            if let Ok(count) = secrets.count() {
                let _ = news.send(News::Secrets {
                    count,
                    stored: false,
                });
            }
        });
    }

    fn sync_calendars(&mut self) {
        let Some(news) = self.calendar.news.clone() else {
            return;
        };
        self.calendar.generation += 1;
        let generation = self.calendar.generation;
        let now = self.wall_now();
        let day = SignedDuration::from_hours(24);
        let from = now.checked_sub(day * 31).unwrap_or(now);
        let to = now.checked_add(day * (AHEAD_DAYS as i32)).unwrap_or(now);
        for account in self.config.calendars.clone() {
            let (news, secrets) = (news.clone(), self.calendar.secrets.clone());
            spawn("calendar sync", move || {
                let id = account.id();
                let source = if account.kind == "subscription" {
                    Source::Subscription { url: account.url }
                } else {
                    let password = secrets
                        .and_then(|s| s.lookup(&id).ok().flatten())
                        .unwrap_or_default();
                    Source::CalDav {
                        url: account.url,
                        user: account.user,
                        password,
                    }
                };
                let fetched = caldav::fetch(&source, from, to)
                    .map(|calendars| calendars.into_iter().flat_map(|c| c.ics).collect());
                let _ = news.send(News::Fetched(id, generation, fetched));
            });
        }
    }

    /// Ten minutes and one minute before: a toast, with Join when there is
    /// a call to join.
    fn remind(&mut self) {
        let now = self.wall_now();
        let mut due = Vec::new();
        for (_, o) in &self.calendar.events {
            if o.all_day {
                continue;
            }
            let left = o.start.duration_since(now).as_secs();
            for minutes in REMIND {
                let mark = minutes * 60;
                // Within the minute before the mark, once.
                let key = (o.uid.clone(), o.start, minutes);
                if (mark - 59..=mark).contains(&left) && !self.calendar.reminded.contains(&key) {
                    due.push((o.clone(), minutes));
                }
            }
        }
        let lang = self.shell_lang();
        for (o, minutes) in due {
            self.calendar
                .reminded
                .insert((o.uid.clone(), o.start, minutes));
            let actions = o
                .link
                .iter()
                .map(|_| ("join".to_owned(), lang.join().to_owned()))
                .collect();
            let summary = lang.starts_in(&o.summary, minutes);
            if let Some(id) = self.shell_toast_with(&summary, &o.location, actions)
                && let Some(link) = o.link
            {
                self.calendar.joins.insert(id, link);
            }
        }
    }

    /// A reminder's Join; whether the toast was one.
    pub fn calendar_action(&mut self, id: u32, key: &str) -> bool {
        let Some(link) = self.calendar.joins.get(&id).cloned() else {
            return false;
        };
        if key == "join" {
            self.launch("xdg-open", &[link]);
        }
        true
    }

    /// The column's Join: the next meeting's call.
    pub fn join_meeting(&mut self) {
        if let Some(link) = self.next_meeting().and_then(|o| o.link.clone()) {
            self.launch("xdg-open", &[link]);
        }
    }

    /// The meeting going on, else the next one, today or tomorrow.
    fn next_meeting(&self) -> Option<&Occurrence> {
        let now = self.wall_now();
        let tz = self.wall_tz();
        let tomorrow = now.to_zoned(tz.clone()).date().tomorrow().ok()?;
        self.calendar
            .events
            .iter()
            .map(|(_, o)| o)
            .filter(|o| !o.all_day && o.end > now)
            .find(|o| o.start.to_zoned(tz.clone()).date() <= tomorrow)
    }

    /// The column's meeting card.
    pub fn meeting_card(&self) -> Option<Meeting> {
        let lang = self.shell_lang();
        let now = self.wall_now();
        let tz = self.wall_tz();
        let o = self.next_meeting()?;
        let left = o.start.duration_since(now).as_secs();
        let at = o.start.to_zoned(tz.clone());
        let time = self
            .config
            .clock
            .format
            .time(at.hour() as u8, at.minute() as u8);
        let when = if left <= 0 {
            lang.meeting_now((o.end.duration_since(now).as_secs() + 59) / 60)
        } else if at.date() != now.to_zoned(tz).date() {
            lang.meeting_tomorrow(&time)
        } else if left < 3600 {
            lang.meeting_in((left + 59) / 60)
        } else {
            lang.meeting_at(&time)
        };
        // The ring fills over the hour before.
        let progress = (100 - left.clamp(0, 3600) * 100 / 3600) as u8;
        Some(Meeting {
            title: o.summary.clone(),
            when,
            progress,
            join: o.link.is_some() && left <= JOIN_BEFORE,
        })
    }

    /// The agenda: this month's days with events, and today's.
    pub fn agenda(&self) -> Agenda {
        let tz = self.wall_tz();
        let now = self.wall_now();
        let today = now.to_zoned(tz.clone()).date();
        let first = today.first_of_month();
        let days = today.days_in_month() as u8;
        let mut weeks = Vec::new();
        let mut monday =
            first.saturating_sub(i64::from(first.weekday().to_monday_zero_offset()).days());
        while monday <= today.last_of_month() {
            weeks.push(monday.iso_week_date().week() as u8);
            monday = monday.saturating_add(7.days());
        }
        let lang = self.shell_lang();
        let mut busy = Vec::new();
        let mut on_today = Vec::new();
        for (_, o) in &self.calendar.events {
            let start = o.start.to_zoned(tz.clone()).date();
            // An all-day event's end is the next day's start.
            let last = o
                .end
                .checked_sub(SignedDuration::from_secs(1))
                .unwrap_or(o.end)
                .to_zoned(tz.clone())
                .date()
                .max(start);
            let mut d = start.max(first);
            while d <= last && d.month() == today.month() && d.year() == today.year() {
                busy.push(d.day() as u8);
                d = d.saturating_add(1.days());
            }
            if start <= today && today <= last {
                let time = if o.all_day {
                    lang.all_day().to_owned()
                } else {
                    let at = o.start.to_zoned(tz.clone());
                    self.config
                        .clock
                        .format
                        .time(at.hour() as u8, at.minute() as u8)
                };
                on_today.push((time, o.summary.clone()));
            }
        }
        busy.sort_unstable();
        busy.dedup();
        Agenda {
            year: today.year(),
            first_weekday: first.weekday().to_monday_zero_offset() as u8,
            days,
            weeks,
            busy,
            today: on_today,
        }
    }

    /// Keeps a password typed in the settings, off the loop; a sync follows.
    pub fn calendar_secret(&mut self, id: String, secret: String) {
        let (Some(secrets), Some(news)) =
            (self.calendar.secrets.clone(), self.calendar.news.clone())
        else {
            return;
        };
        let label = format!("Hocuspanda calendar {id}");
        spawn("calendar secret", move || {
            if let Err(err) = secrets.store(&id, &label, &secret) {
                tracing::warn!(err, "the calendar password was not kept");
                return;
            }
            let count = secrets.count().unwrap_or_default();
            let _ = news.send(News::Secrets {
                count,
                stored: true,
            });
        });
    }

    pub fn calendars_info(&self) -> CalendarsInfo {
        let now = self.wall_now();
        let accounts = self
            .config
            .calendars
            .iter()
            .map(|a| {
                let id = a.id();
                CalendarStatus {
                    name: a.name.clone(),
                    events: self
                        .calendar
                        .events
                        .iter()
                        .filter(|(i, _)| *i == id)
                        .count(),
                    error: self.calendar.errors.get(&id).map(ToString::to_string),
                    refused: matches!(self.calendar.errors.get(&id), Some(FetchError::Refused(_))),
                    synced: self.calendar.synced.contains(&id),
                    id,
                }
            })
            .collect();
        CalendarsInfo {
            accounts,
            secrets: self.calendar.secrets_kept,
            events: self
                .calendar
                .events
                .iter()
                .filter(|(_, o)| o.end > now)
                .map(|(_, o)| EventInfo {
                    summary: o.summary.clone(),
                    start: o.start.to_string(),
                    link: o.link.clone(),
                })
                .collect(),
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}
