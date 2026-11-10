//! A meeting's reminder (Day.6, issue #430, variant B): a tab that slides
//! out of the bar beside the meeting's card, with Join (or Open), Later
//! and ✕ ([`draw`]).
//!
//! - **When**: for each event of the featured calendars
//!   ([`hocus_day::Day::featured`]: `[[day.calendars]] featured`, every
//!   shown calendar without it) that takes a slot of the day (timed,
//!   neither cancelled, declined nor free), at its own earliest alarm,
//!   else `[calendar] remind` minutes before its start (default 5;
//!   [`hocus_day::reminders`]). It stays until 10 min after the start, then
//!   goes to the notifications' history ("… · a commencé à 10:00"), its
//!   link its action while the meeting runs. One that came while the
//!   machine slept shows on resume if the meeting has not been running for
//!   10 min. Two meetings get a tab each, 6 px apart, the earliest on top.
//! - **The times** are whole minutes, as the meeting's card has them
//!   ([`crate::meeting`]): its start to the nearest, the clock's minute as
//!   it shows it.
//! - **Off the loop**: a thread of its own (`reminders`) computes them from
//!   the day ([`crate::day::snapshot`]) as it changes, and waits for the
//!   next one on the monotonic clock: the time to show is read from the
//!   wall clock once, as the next reminder is known, and again after a
//!   resume, a clock set (a `CLOCK_REALTIME` timer that the kernel cancels
//!   when the clock is set) or a time zone changed. Nothing wakes the
//!   shell's loop until a tab comes, changes or goes; while one shows, its
//!   line draws again at most once a minute.
//! - **Do not disturb** holds the bubbles, never a reminder: a meeting is
//!   not a notification that can wait.
//! - **A screen shared** (5.11) shows its tab with "Réunion" for a title.
//! - **A fullscreen window** covers the bar: its screen's tab shows over
//!   it, 8 px from the screen's edge.
//! - **The halo** (§6) is the meeting card's: joining, opening or
//!   dismissing a reminder ends it ([`crate::meeting::acted_on`]); Later
//!   leaves it on.
//! - `Super+J` ([`join_key`]) joins the meeting whose tab shows, else the
//!   one starting next within 10 min: of the featured calendars too.

pub mod draw;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Mutex, OnceLock, PoisonError, mpsc},
    time::{Duration, Instant},
};

use hocus_day::{
    Day,
    reminders::{self, Settings},
};
use hocus_link::{BUBBLE_OUT, EDGE, Form, ToCompositor};
use iced::{Element, Length, Rectangle, widget::canvas, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use jiff::{SignedDuration, Zoned};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{Message, clock::time::Lang, meeting};

/// How long a tab stays after its meeting's start, in minutes.
pub const STAY: i64 = 10;
/// `Super+J` joins a meeting starting within this many minutes.
const JOIN_WITHIN: i64 = 10;
/// The tab: its size, the gap between two, the gap to the bar.
pub const WIDTH: f32 = 340.0;
pub const HEIGHT: f32 = 44.0;
const BETWEEN: f32 = 6.0;
const GAP: i32 = 8;
/// Past the minute's start: a timer may fire a little early, and a look
/// just before the minute would show the one before.
const PAST_THE_MINUTE: i64 = 5;
/// How far back the day is read: a tab still showing, after a long sleep.
const BACK: SignedDuration = SignedDuration::from_hours(24);
/// How far ahead: an alarm may come days before its event.
const AHEAD: SignedDuration = SignedDuration::from_hours(24 * 400);

// --- The meetings, and their reminders ------------------------------------------

/// A meeting a reminder may come for, its times whole minutes since the
/// epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    /// Its reminder's minutes before its start.
    pub lead: i64,
    /// Its start, `HH:MM` in the system's zone.
    pub hhmm: String,
    pub video: Option<String>,
    pub page: Option<String>,
    pub place: Option<String>,
}

/// The meetings of `day` a reminder may come for (and `Super+J` join),
/// around `now`, in order: the featured calendars'.
pub fn meetings(day: &Day, now: &Zoned, settings: Settings) -> Vec<Meeting> {
    let zone = now.time_zone();
    let at = now.timestamp();
    let (Ok(from), Ok(to)) = (at.checked_sub(BACK), at.checked_add(AHEAD)) else {
        return Vec::new();
    };
    let floor = meeting::clock_minute(now) - BACK.as_mins();
    day.featured(from, to, zone)
        .into_iter()
        .filter_map(|shown| {
            let event = shown.event;
            let lead = reminders::lead(event, settings)?;
            let start = event.when.start(zone);
            let start_m = meeting::nearest_minute(start);
            (start_m >= floor).then(|| Meeting {
                id: event.id.as_str().to_owned(),
                title: event.title.clone(),
                start: start_m,
                end: meeting::nearest_minute(event.when.end(zone)).max(start_m),
                lead: i64::from(lead),
                hhmm: meeting::hhmm(start, now),
                video: event.video.clone().filter(|v| !v.is_empty()),
                page: event.page.clone().filter(|p| !p.is_empty()),
                place: event.place.clone().filter(|p| !p.is_empty()),
            })
        })
        .collect()
}

/// A tab, as it shows at a minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// Its surfaces' number ([`hocus_link::reminder_namespace`]), its
    /// note's in the history.
    pub serial: u64,
    pub meeting: Meeting,
    /// Minutes to its start: 0 at it, less once it started.
    pub left: i64,
}

/// Where a reminder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Shown,
    /// Taken away by Later: back at that minute.
    Later {
        back: i64,
    },
    /// Joined, opened, dismissed, or gone to the history.
    Done,
}

#[derive(Debug, Clone)]
struct Tracked {
    meeting: Meeting,
    stage: Stage,
    serial: u64,
}

/// What [`Reminders::look`] found.
#[derive(Debug, Default, PartialEq)]
pub struct Looked {
    /// The tabs to show, earliest first.
    pub tabs: Vec<Tab>,
    /// Meetings whose tab stayed its time: to the history, with their
    /// link while they run.
    pub kept: Vec<(u64, Meeting)>,
    /// Meetings of the history that ended: their link goes.
    pub ended: Vec<(u64, Meeting)>,
    /// The minute to look again at, if any.
    pub wake: Option<i64>,
}

/// What the user does with a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Rejoindre: the video call.
    Join,
    /// Voir, or a click on its text: the event, in the calendar (Day.5).
    Open,
    /// Later: back 1 min before, or at the start.
    Later,
    /// ✕.
    Dismiss,
}

/// `w`, or `at` if sooner.
fn wake(at: i64, w: &mut Option<i64>) {
    *w = Some(w.map_or(at, |w| w.min(at)));
}

/// The reminders and what the user did with them.
#[derive(Debug, Default)]
pub struct Reminders {
    tracked: HashMap<String, Tracked>,
    /// The meetings gone to the history with their link, by id.
    in_history: HashMap<String, (u64, Meeting)>,
    serials: u64,
    /// The meetings as last looked at, for `Super+J`.
    meetings: Vec<Meeting>,
}

impl Reminders {
    /// The reminders at clock minute `minute`, of `meetings`.
    pub fn look(&mut self, meetings: Vec<Meeting>, minute: i64) -> Looked {
        let mut looked = Looked::default();
        let ids: HashSet<&str> = meetings.iter().map(|m| m.id.as_str()).collect();
        // Cancelled, declined, gone: their tab goes.
        self.tracked.retain(|id, _| ids.contains(id.as_str()));
        for m in &meetings {
            let remind = m.start - m.lead;
            let stay = m.start + STAY;
            // Moved: its reminder anew, unless its tab shows and still
            // should (it updates in place).
            if let Some(t) = self.tracked.get(&m.id)
                && t.meeting.start != m.start
                && !(t.stage == Stage::Shown && (remind..stay).contains(&minute))
            {
                self.tracked.remove(&m.id);
            }
            if minute >= stay {
                if let Some(t) = self.tracked.get_mut(&m.id) {
                    if t.stage == Stage::Shown {
                        looked.kept.push((t.serial, m.clone()));
                    }
                    t.stage = Stage::Done;
                }
                continue;
            }
            let stage = self.tracked.get(&m.id).map(|t| t.stage);
            match stage {
                None if minute < remind => {
                    wake(remind, &mut looked.wake);
                    continue;
                }
                None => {
                    self.serials += 1;
                    self.tracked.insert(
                        m.id.clone(),
                        Tracked {
                            meeting: m.clone(),
                            stage: Stage::Shown,
                            serial: self.serials,
                        },
                    );
                }
                Some(Stage::Later { back }) if minute < back => {
                    wake(back, &mut looked.wake);
                    continue;
                }
                Some(Stage::Done) => continue,
                Some(Stage::Later { .. } | Stage::Shown) => {}
            }
            let Some(t) = self.tracked.get_mut(&m.id) else {
                continue;
            };
            t.stage = Stage::Shown;
            t.meeting = m.clone();
            looked.tabs.push(Tab {
                serial: t.serial,
                meeting: m.clone(),
                left: m.start - minute,
            });
            // Its line changes each minute, and it goes at its stay's end.
            wake(minute + 1, &mut looked.wake);
        }
        for (serial, m) in &looked.kept {
            if m.video.is_some() && m.end > minute {
                self.in_history.insert(m.id.clone(), (*serial, m.clone()));
            }
        }
        let ended: Vec<String> = self
            .in_history
            .iter()
            .filter(|(_, (_, m))| m.end <= minute)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ended {
            if let Some(gone) = self.in_history.remove(&id) {
                looked.ended.push(gone);
            }
        }
        for (_, m) in self.in_history.values() {
            wake(m.end, &mut looked.wake);
        }
        looked.tabs.sort_by(|a, b| {
            (a.meeting.start, &a.meeting.id).cmp(&(b.meeting.start, &b.meeting.id))
        });
        self.meetings = meetings;
        looked
    }

    /// The tab of `serial`, as last looked at.
    fn tracked_of(&mut self, serial: u64) -> Option<&mut Tracked> {
        self.tracked.values_mut().find(|t| t.serial == serial)
    }

    /// `act` on tab `serial` at minute `minute`: what to open, if anything,
    /// and the meeting acted on (its halo ends) unless it was Later.
    pub fn act(&mut self, serial: u64, act: Act, minute: i64) -> Option<Acted> {
        let t = self.tracked_of(serial)?;
        if t.stage != Stage::Shown {
            return None;
        }
        let m = t.meeting.clone();
        match act {
            Act::Join => {
                t.stage = Stage::Done;
                Some(Acted::Opens {
                    url: m.video.clone().or(m.page.clone()),
                    meeting: m,
                })
            }
            Act::Open => {
                t.stage = Stage::Done;
                Some(Acted::InCalendar(m))
            }
            Act::Later => {
                let left = m.start - minute;
                if left <= 0 {
                    return None;
                }
                let back = if left > 1 { m.start - 1 } else { m.start };
                t.stage = Stage::Later { back };
                Some(Acted::Later { back, meeting: m })
            }
            Act::Dismiss => {
                t.stage = Stage::Done;
                Some(Acted::Dismissed(m))
            }
        }
    }

    /// `Super+J` at `minute`: the meeting whose tab shows (the first with a
    /// link), else the one with a link starting next within 10 min; its
    /// reminder done.
    pub fn join_key(&mut self, minute: i64, tabs: &[Tab]) -> Option<Meeting> {
        let shown = tabs
            .iter()
            .find(|t| t.meeting.video.is_some())
            .map(|t| t.meeting.clone());
        let next = || {
            self.meetings
                .iter()
                .filter(|m| m.video.is_some())
                .filter(|m| (1..=JOIN_WITHIN).contains(&(m.start - minute)))
                .min_by_key(|m| m.start)
                .cloned()
        };
        let m = shown.or_else(next)?;
        let serial = match self.tracked.get(&m.id) {
            Some(t) => t.serial,
            None => {
                self.serials += 1;
                self.serials
            }
        };
        self.tracked.insert(
            m.id.clone(),
            Tracked {
                meeting: m.clone(),
                stage: Stage::Done,
                serial,
            },
        );
        Some(m)
    }

    /// The meeting of history note `serial`, while it runs.
    fn in_history(&self, serial: u64) -> Option<&Meeting> {
        self.in_history
            .values()
            .find(|(s, _)| *s == serial)
            .map(|(_, m)| m)
    }
}

/// What acting on a tab did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acted {
    /// Joined: `url` opens (none: nothing to open).
    Opens {
        url: Option<String>,
        meeting: Meeting,
    },
    /// Opened: the calendar (Day.5) on its day, the meeting chosen.
    InCalendar(Meeting),
    Later {
        back: i64,
        meeting: Meeting,
    },
    Dismissed(Meeting),
}

// --- Words ------------------------------------------------------------------------

/// The tab's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    pub fn now() -> Self {
        Self::new(Lang::of_messages())
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    /// "dans 5 min", "maintenant", "a commencé il y a 4 min".
    pub fn when(self, left: i64) -> String {
        let minutes = |m: i64| meeting::Words::minutes(m);
        match (left, self.fr) {
            (1.., true) => format!("dans {}", minutes(left)),
            (1.., false) => format!("in {}", minutes(left)),
            (0, _) => self.pick("maintenant", "now").to_owned(),
            (_, true) => format!("a commencé il y a {}", minutes(-left)),
            (_, false) => format!("started {} ago", minutes(-left)),
        }
    }

    /// What the line says of where: "visio", else its place.
    fn place(self, m: &Meeting) -> Option<String> {
        match (&m.video, &m.place) {
            (Some(_), _) => Some(self.pick("visio", "video").to_owned()),
            (None, Some(place)) => Some(place.clone()),
            (None, None) => None,
        }
    }

    /// "10:00 · dans 5 min · visio".
    pub fn line(self, tab: &Tab) -> String {
        let mut line = format!("{} · {}", tab.meeting.hhmm, self.when(tab.left));
        if let Some(place) = self.place(&tab.meeting) {
            line.push_str(" · ");
            line.push_str(&place);
        }
        line
    }

    /// The title, or "Réunion" on a screen shared.
    pub fn title(self, tab: &Tab, shared: bool) -> String {
        if shared {
            self.pick("Réunion", "Meeting").to_owned()
        } else {
            tab.meeting.title.clone()
        }
    }

    pub fn join(self) -> &'static str {
        self.pick("Rejoindre", "Join")
    }

    pub fn open(self) -> &'static str {
        self.pick("Voir", "Open")
    }

    /// Later's name, `left` minutes before the start.
    pub fn later(self, left: i64) -> &'static str {
        if left > 1 {
            self.pick("Me le redire 1 min avant", "Remind me 1 min before")
        } else {
            self.pick("Me le redire à l'heure", "Remind me when it starts")
        }
    }

    pub fn dismiss(self) -> &'static str {
        self.pick("Ignorer ce rappel", "Dismiss this reminder")
    }

    /// For screen readers, said once as it comes: "Revue de sprint, dans 5
    /// minutes, visio".
    pub fn name(self, tab: &Tab, shared: bool) -> String {
        let long = |m: i64| {
            if m == 1 {
                "1 minute".to_owned()
            } else if m >= 60 {
                let (h, m) = (m / 60, m % 60);
                if m == 0 {
                    format!("{h} h")
                } else {
                    format!("{h} h {m:02}")
                }
            } else {
                format!("{m} minutes")
            }
        };
        let when = match (tab.left, self.fr) {
            (1.., true) => format!("dans {}", long(tab.left)),
            (1.., false) => format!("in {}", long(tab.left)),
            (0, _) => self.pick("maintenant", "now").to_owned(),
            (_, true) => format!("a commencé il y a {}", long(-tab.left)),
            (_, false) => format!("started {} ago", long(-tab.left)),
        };
        let mut name = format!("{}, {when}", self.title(tab, shared));
        match (&tab.meeting.video, &tab.meeting.place) {
            (Some(_), _) => name.push_str(self.pick(", visio", ", video call")),
            (None, Some(place)) => {
                name.push_str(", ");
                name.push_str(place);
            }
            (None, None) => {}
        }
        name
    }

    /// The history's line: "Revue de sprint · a commencé à 10:00".
    pub fn started(self, m: &Meeting) -> String {
        if self.fr {
            format!("{} · a commencé à {}", m.title, m.hhmm)
        } else {
            format!("{} · started at {}", m.title, m.hhmm)
        }
    }

    /// The history's application.
    pub fn calendar(self) -> &'static str {
        self.pick("Agenda", "Calendar")
    }
}

// --- The thread -------------------------------------------------------------------

/// What wakes the reminders' thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Come {
    /// The day changed.
    Day,
    /// The machine resumed, the clock was set, or the time zone changed.
    Clock(&'static str),
    Act(u64, Act),
    /// `Super+J`.
    JoinKey,
    /// The history's action on note `serial`.
    FromHistory(u64),
    /// Where the tabs show changed: a bar, a form, a share, full screen.
    Places,
}

/// The tabs shown, the places they show at, and their surfaces.
#[derive(Debug, Default)]
struct Shown {
    tabs: Vec<Tab>,
    places: Places,
    /// Each tab's surface on each bar, by bar and serial.
    opened: HashMap<(String, u64), Opened>,
    /// Surfaces going, removed once the compositor has faded them out.
    leaving: Vec<(window::Id, Instant)>,
    /// The tabs already announced (screen readers), by serial.
    announced: HashSet<u64>,
    /// What the log said of the tabs last.
    said: String,
}

#[derive(Debug, Default, Clone)]
struct Places {
    /// The bars, by screen, and their forms.
    bars: HashMap<String, Form>,
    /// Screens whose front window is full screen, covering their bar.
    fullscreen: HashSet<String>,
    /// Screens shared (5.11).
    shared: HashSet<String>,
    /// The shell draws on the CPU: the surface is the tab alone.
    card: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Opened {
    id: window::Id,
    size: (u32, u32),
    margin: (i32, i32, i32, i32),
    /// The tab in the surface.
    tab: Rectangle,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn shown<T>(with: impl FnOnce(&mut Shown) -> T) -> T {
    let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
    with(shown.get_or_insert_with(Shown::default))
}

static WAKE: OnceLock<Mutex<mpsc::Sender<Come>>> = OnceLock::new();
static OUT: Mutex<Option<UnboundedSender<Vec<Message>>>> = Mutex::new(None);
static TAKEN: Mutex<Option<UnboundedReceiver<Vec<Message>>>> = Mutex::new(None);

fn come(what: Come) {
    if let Some(wake) = WAKE.get() {
        let _ = wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(what);
    }
}

fn out(messages: Vec<Message>) {
    if messages.is_empty() {
        return;
    }
    if let Some(out) = OUT.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let _ = out.send(messages);
    }
}

/// Starts the reminders' thread, and follows the day: after the day's
/// start, and the notifications' wall clock. `software`: the shell draws on
/// the CPU.
pub fn start(software: bool) {
    shown(|s| s.places.card = software);
    let (messages, taken) = unbounded_channel();
    *OUT.lock().unwrap_or_else(PoisonError::into_inner) = Some(messages);
    *TAKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(taken);
    let (wake, woken) = mpsc::channel();
    if WAKE.set(Mutex::new(wake)).is_err() {
        return;
    }
    let started = std::thread::Builder::new()
        .name("reminders".to_owned())
        .spawn(move || run(&woken, settings()));
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the meetings' reminders");
        return;
    }
    crate::day::on_change(|| {
        come(Come::Day);
        true
    });
    watch_clock_sets();
}

/// `[calendar]` of the configuration file the compositor names.
fn settings() -> Settings {
    let file = match std::env::var_os(hocus_link::CONFIG_ENV) {
        Some(file) if file.is_empty() => None,
        Some(file) => Some(PathBuf::from(file)),
        None => None,
    };
    let Some(file) = file else {
        return Settings::default();
    };
    reminders::read(&file).unwrap_or_else(|why| {
        tracing::warn!("[calendar] refused, the default reminders: {why}");
        Settings::default()
    })
}

/// The thread: looks at the day as it is told, and when the next reminder
/// (or a tab's next minute) is due, on the monotonic clock.
fn run(woken: &mpsc::Receiver<Come>, settings: Settings) {
    tracing::info!(remind = settings.remind, "the meetings' reminders start");
    let mut reminders = Reminders::default();
    let mut due: Option<Instant> = None;
    let mut why = Some(Come::Day);
    loop {
        if let Some(come) = why.take() {
            due = handle(&mut reminders, come, settings);
        }
        let leaving = shown(|s| s.leaving.iter().map(|(_, at)| *at).min());
        let next = match (due, leaving) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let got = match next {
            Some(at) => woken.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => woken
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(come) => why = Some(come),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if due.is_some_and(|d| d <= now) {
                    why = Some(Come::Clock("due"));
                }
                remove_left(now);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// What came, carried out; the reminders looked at again: when to look
/// next, on the monotonic clock.
fn handle(reminders: &mut Reminders, come: Come, settings: Settings) -> Option<Instant> {
    let now = crate::notifications::wall();
    let minute = meeting::clock_minute(&now);
    tracing::debug!(why = ?come, "the reminders look again");
    match come {
        Come::Act(serial, act) => acted(reminders.act(serial, act, minute)),
        Come::JoinKey => {
            let tabs = shown(|s| s.tabs.clone());
            match reminders.join_key(minute, &tabs) {
                Some(m) => {
                    tracing::info!(id = m.id, "Super+J joins the meeting");
                    acted(Some(Acted::Opens {
                        url: m.video.clone(),
                        meeting: m,
                    }));
                }
                None => tracing::info!("Super+J: no meeting to join"),
            }
        }
        Come::FromHistory(serial) => {
            if let Some(url) = reminders.in_history(serial).and_then(|m| m.video.clone()) {
                tracing::info!(serial, "the history joins the meeting");
                open(&url);
            }
        }
        Come::Day | Come::Clock(_) | Come::Places => {}
    }
    let looked = reminders.look(meetings(&crate::day::snapshot(), &now, settings), minute);
    let words = Words::now();
    for (serial, m) in &looked.kept {
        keep(
            *serial,
            m,
            words,
            m.video.is_some() && m.end > minute,
            false,
        );
    }
    for (serial, m) in &looked.ended {
        keep(*serial, m, words, false, true);
    }
    show(looked.tabs);
    let wake = looked.wake?;
    // The wall clock read once: how long until that minute, on the
    // monotonic clock.
    let until = wake * 60_000 + PAST_THE_MINUTE - now.timestamp().as_millisecond();
    tracing::debug!(
        at = %jiff::Timestamp::from_millisecond(wake * 60_000)
            .map(|t| meeting::hhmm(t, &now))
            .unwrap_or_default(),
        in_s = until / 1000,
        "the reminders wait"
    );
    Some(Instant::now() + Duration::from_millis(until.max(0).unsigned_abs()))
}

/// What acting on a tab did, carried out.
fn acted(acted: Option<Acted>) {
    match acted {
        Some(Acted::Opens { url, meeting: m }) => {
            meeting::acted_on(&m.id);
            match url {
                Some(url) => {
                    tracing::info!(id = m.id, url, "the reminder opens its meeting");
                    open(&url);
                }
                None => tracing::info!(id = m.id, "the reminder's meeting has nothing to open"),
            }
        }
        Some(Acted::InCalendar(m)) => {
            meeting::acted_on(&m.id);
            let day = jiff::Timestamp::from_second(m.start * 60)
                .map(|at| {
                    at.to_zoned(crate::notifications::wall().time_zone().clone())
                        .date()
                })
                .unwrap_or_else(|_| crate::notifications::wall().date());
            tracing::info!(id = m.id, %day, "the reminder opens its meeting in the calendar");
            crate::popover::calendar::open_on("", day, Some(hocus_day::model::EventId::new(m.id)));
        }
        Some(Acted::Later { back, meeting: m }) => {
            tracing::info!(
                id = m.id,
                back_in = back - m.start,
                "the reminder comes back later"
            );
        }
        Some(Acted::Dismissed(m)) => {
            meeting::acted_on(&m.id);
            tracing::info!(id = m.id, "the reminder is dismissed");
        }
        None => {}
    }
}

/// `url` in its default handler.
fn open(url: &str) {
    crate::popover::send(crate::launcher::browse(url).into_iter().collect());
}

/// Meeting `m` in the notifications' history, as its tab went: its link
/// its action while `live`; `ended`: its meeting ended, its action goes.
fn keep(serial: u64, m: &Meeting, words: Words, live: bool, ended: bool) {
    tracing::debug!(id = m.id, live, ended, "the reminder in the history");
    let at = (m.start + STAY) * 60_000;
    if ended {
        crate::notifications::reminder_ended(serial, words.calendar(), words.started(m), at);
    } else {
        crate::notifications::keep_reminder(
            serial,
            words.calendar(),
            words.started(m),
            live.then(|| words.join()),
            at,
        );
    }
}

/// Acts on tab `serial` (its surface's click).
pub fn act(serial: u64, act: Act) {
    come(Come::Act(serial, act));
}

/// `Super+J`.
pub fn join_key() {
    come(Come::JoinKey);
}

/// The history's action on the note of `serial`: its meeting joined.
pub fn from_history(serial: u64) {
    come(Come::FromHistory(serial));
}

/// The machine resumed (logind): the wall clock read again.
pub fn resumed() {
    come(Come::Clock("resumed"));
}

/// The time zone changed.
pub fn zone_changed() {
    come(Come::Clock("time zone"));
}

/// Watches for the clock being set, on a thread of its own: a timer on
/// `CLOCK_REALTIME` far ahead, which the kernel cancels when the clock is
/// set (and on resume).
fn watch_clock_sets() {
    use rustix::time::{
        Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
        timerfd_settime,
    };
    let started = std::thread::Builder::new()
        .name("reminders: clock".to_owned())
        .spawn(|| {
            let fd = match timerfd_create(TimerfdClockId::Realtime, TimerfdFlags::CLOEXEC) {
                Ok(fd) => fd,
                Err(err) => {
                    tracing::info!(%err, "the clock's changes are not watched");
                    return;
                }
            };
            let far = Itimerspec {
                it_interval: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                it_value: Timespec {
                    tv_sec: i64::from(i32::MAX),
                    tv_nsec: 0,
                },
            };
            let flags = TimerfdTimerFlags::ABSTIME | TimerfdTimerFlags::CANCEL_ON_SET;
            loop {
                if let Err(err) = timerfd_settime(&fd, flags, &far) {
                    tracing::info!(%err, "the clock's changes are not watched");
                    return;
                }
                let mut ticks = [0u8; 8];
                match rustix::io::read(&fd, &mut ticks) {
                    Err(rustix::io::Errno::CANCELED) => come(Come::Clock("clock set")),
                    Err(rustix::io::Errno::INTR) | Ok(_) => {}
                    Err(err) => {
                        tracing::info!(%err, "the clock's changes are not watched");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch the clock");
    }
}

// --- The surfaces -----------------------------------------------------------------

/// Room around the tab on a GPU surface, for its shadow (`0 10px 30px`:
/// three deviations, the offset below) and its glow: left, top, right,
/// bottom. The bar's gap leaves 8 px on the right.
const ROOM: (f32, f32, f32, f32) = (45.0, 35.0, 8.0, 55.0);

/// Bar `bar`'s tab `index` (0 at the top): its surface's size, its margins
/// from the screen's top right (top, right, bottom, left; mirrored with the
/// bar on the left), the tab in it.
fn placed(places: &Places, bar: &str, form: Form, index: usize) -> Opened {
    let card_top = match form {
        // Its top on the meeting card's.
        Form::Full => EDGE as f32 + crate::column::TOP,
        // Centred on the mini form's mark.
        Form::Mini => {
            EDGE as f32 + crate::column::MINI_TOP + meeting::MARK_HEIGHT / 2.0 - HEIGHT / 2.0
        }
    };
    let top = card_top + index as f32 * (HEIGHT + BETWEEN);
    let right = if places.fullscreen.contains(bar) {
        EDGE
    } else {
        EDGE + form.width() + GAP
    } as f32;
    let (left, above, after, below) = if places.card {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        ROOM
    };
    Opened {
        id: window::Id::unique(),
        size: (
            (WIDTH + left + after).ceil() as u32,
            (HEIGHT + above + below).ceil() as u32,
        ),
        margin: (
            (top - above).round() as i32,
            (right - after).round() as i32,
            0,
            0,
        ),
        // The surface is mirrored on the left (`crate::side`): the room
        // toward the bar is then on the tab's left.
        tab: Rectangle::new(
            iced::Point::new(if crate::side::left() { after } else { left }, above),
            iced::Size::new(WIDTH, HEIGHT),
        ),
    }
}

/// The tabs to show now: their surfaces come, move, change or go.
fn show(tabs: Vec<Tab>) {
    let words = Words::now();
    let messages = shown(|s| {
        let changed: Vec<u64> = tabs
            .iter()
            .filter(|t| !s.tabs.iter().any(|b| b == *t))
            .map(|t| t.serial)
            .collect();
        s.tabs = tabs;
        let said = s
            .tabs
            .iter()
            .map(|t| format!("«{}» {}", t.meeting.title, words.line(t)))
            .collect::<Vec<_>>()
            .join("; ");
        if said != s.said {
            tracing::debug!(count = s.tabs.len(), tabs = said, "the reminders show");
            s.said = said;
        }
        for tab in &s.tabs {
            if s.announced.insert(tab.serial) {
                // Said once, politely, as it comes (§11), until iced has
                // accessibility.
                tracing::debug!(name = words.name(tab, false), "the reminder is announced");
            }
        }
        let live: HashSet<u64> = s.tabs.iter().map(|t| t.serial).collect();
        s.announced.retain(|serial| live.contains(serial));
        let mut messages = s.settle();
        for serial in changed {
            if s.opened.keys().any(|(_, sr)| *sr == serial) {
                crate::popover::tell(ToCompositor::Wake(hocus_link::reminder_namespace(serial)));
            }
        }
        messages.shrink_to_fit();
        messages
    });
    out(messages);
}

impl Shown {
    /// The surfaces as the tabs and the places say: opened, placed again,
    /// closing.
    fn settle(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut wanted: HashMap<(String, u64), Opened> = HashMap::new();
        for (bar, form) in &self.places.bars {
            for (index, tab) in self.tabs.iter().enumerate() {
                wanted.insert(
                    (bar.clone(), tab.serial),
                    placed(&self.places, bar, *form, index),
                );
            }
        }
        let gone: Vec<(String, u64)> = self
            .opened
            .keys()
            .filter(|k| !wanted.contains_key(*k))
            .cloned()
            .collect();
        let now = Instant::now();
        for key in gone {
            if let Some(opened) = self.opened.remove(&key) {
                let namespace = hocus_link::reminder_namespace(key.1);
                if self.places.bars.contains_key(&key.0) {
                    // Faded out by the compositor, then removed.
                    crate::popover::tell(ToCompositor::Closing(namespace));
                    self.leaving
                        .push((opened.id, now + BUBBLE_OUT + LEAVE_MARGIN));
                } else {
                    draw::forget(opened.id);
                    messages.push(Message::RemoveWindow(opened.id));
                }
            }
        }
        for (key, want) in wanted {
            match self.opened.get_mut(&key) {
                None => {
                    let namespace = hocus_link::reminder_namespace(key.1);
                    messages.push(Message::NewLayerShell {
                        settings: NewLayerShellSettings {
                            size: Some(want.size),
                            // Over a fullscreen window too.
                            layer: Layer::Overlay,
                            anchor: crate::side::anchor(Anchor::Right | Anchor::Top),
                            exclusive_zone: Some(0),
                            margin: Some(crate::side::margin(want.margin)),
                            keyboard_interactivity: KeyboardInteractivity::None,
                            output_option: if key.0 == crate::ALONE {
                                OutputOption::Active
                            } else {
                                OutputOption::OutputName(key.0.clone())
                            },
                            events_transparent: false,
                            namespace: Some(namespace),
                        },
                        id: want.id,
                    });
                    messages.push(crate::notifications::region(want.id, Some(want.tab)));
                    crate::side::opened(want.id);
                    self.opened.insert(key, want);
                }
                Some(opened) => {
                    if (opened.size, opened.margin, opened.tab)
                        != (want.size, want.margin, want.tab)
                    {
                        messages.push(Message::SizeChange {
                            id: opened.id,
                            size: want.size,
                        });
                        messages.push(Message::MarginChange {
                            id: opened.id,
                            margin: crate::side::moved(opened.id, want.margin),
                        });
                        messages.push(crate::notifications::region(opened.id, Some(want.tab)));
                        opened.size = want.size;
                        opened.margin = want.margin;
                        opened.tab = want.tab;
                    }
                }
            }
        }
        messages
    }
}

/// A little more than the compositor's fade out, before the surface goes.
const LEAVE_MARGIN: Duration = Duration::from_millis(40);

/// The surfaces faded out by `now`, removed.
fn remove_left(now: Instant) {
    let gone: Vec<Message> = shown(|s| {
        let (gone, kept): (Vec<_>, Vec<_>) = s.leaving.drain(..).partition(|(_, at)| *at <= now);
        s.leaving = kept;
        gone.into_iter()
            .map(|(id, _)| {
                draw::forget(id);
                Message::RemoveWindow(id)
            })
            .collect()
    });
    out(gone);
}

/// The places changed: the surfaces follow, and those whose title changes
/// (a screen shared) draw again.
fn places_changed(change: impl FnOnce(&mut Places)) {
    let (messages, woken) = shown(|s| {
        let before = s.places.shared.clone();
        change(&mut s.places);
        let woken: Vec<u64> = if before == s.places.shared {
            Vec::new()
        } else {
            s.tabs.iter().map(|t| t.serial).collect()
        };
        (s.settle(), woken)
    });
    out(messages);
    for serial in woken {
        crate::popover::tell(ToCompositor::Wake(hocus_link::reminder_namespace(serial)));
    }
    // The thread's leaving surfaces, timed.
    come(Come::Places);
}

/// Bar `bar` opened in `form`.
pub fn bar_opened(bar: &str, form: Form) {
    places_changed(|p| {
        p.bars.insert(bar.to_owned(), form);
    });
}

/// Bar `bar` in `form`.
pub fn form(bar: &str, form: Form) {
    let same = shown(|s| s.places.bars.get(bar) == Some(&form));
    if !same {
        places_changed(|p| {
            p.bars.insert(bar.to_owned(), form);
        });
    }
}

/// Bar `bar` went: its tabs too.
pub fn forget(bar: &str) {
    places_changed(|p| {
        p.bars.remove(bar);
    });
}

/// The screens whose front window is full screen.
pub fn set_fullscreen(screens: Vec<String>) {
    let screens: HashSet<String> = screens.into_iter().collect();
    if shown(|s| s.places.fullscreen == screens) {
        return;
    }
    tracing::debug!(?screens, "the reminders' screens in full screen");
    places_changed(|p| p.fullscreen = screens);
    crate::speak::poke();
}

/// Whether a screen's front window is full screen (her speak gate holds
/// her peek then).
pub fn fullscreen_any() -> bool {
    shown(|s| !s.places.fullscreen.is_empty())
}

/// The screens shared (5.11).
pub fn set_shared(screens: Vec<String>) {
    let screens: HashSet<String> = screens.into_iter().collect();
    if shown(|s| s.places.shared == screens) {
        return;
    }
    tracing::debug!(?screens, "the reminders' screens shared");
    places_changed(|p| p.shared = screens);
}

/// The surfaces' messages, for iced.
pub fn effects() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(16, async |mut output| {
        let Some(mut messages) = TAKEN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            std::future::pending::<()>().await;
            return;
        };
        while let Some(batch) = messages.recv().await {
            for message in batch {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

/// What a tab's surface draws: its tab, on its bar.
pub(crate) struct Seen {
    pub bar: String,
    pub tab: Tab,
    /// The tab in its surface.
    pub at: Rectangle,
    /// Its surface's margins from the screen's top right.
    pub margin: (i32, i32, i32, i32),
    /// Its bar's screen is shared: no title.
    pub shared: bool,
    /// The tab alone, on the CPU.
    pub card: bool,
}

/// What surface `id` shows, if it is a tab's.
pub(crate) fn seen(id: window::Id) -> Option<Seen> {
    shown(|s| {
        let ((bar, serial), opened) = s.opened.iter().find(|(_, o)| o.id == id)?;
        let tab = s.tabs.iter().find(|t| t.serial == *serial)?.clone();
        Some(Seen {
            bar: bar.clone(),
            shared: s.places.shared.contains(bar),
            at: opened.tab,
            margin: opened.margin,
            card: s.places.card,
            tab,
        })
    })
}

/// Whether surface `id` is a tab's.
fn is_tab(id: window::Id) -> bool {
    shown(|s| s.opened.values().any(|o| o.id == id))
}

/// The tab surface `id` shows, if it is one. `glass`: the compositor blurs
/// behind the shell.
pub fn view<'a>(id: window::Id, glass: bool) -> Option<Element<'a, Message>> {
    if !is_tab(id) {
        return None;
    }
    Some(
        canvas(draw::View {
            id,
            glass,
            words: Words::now(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 14:00 on 4 November 2026 (UTC), in minutes.
    const AT: i64 = 1_793_750_400 / 60 + 14 * 60;

    fn m(id: &str, start: i64, lead: i64) -> Meeting {
        Meeting {
            id: id.to_owned(),
            title: format!("Réunion {id}"),
            start,
            end: start + 30,
            lead,
            hhmm: "14:30".to_owned(),
            video: Some("https://meet.example.org/abc".to_owned()),
            page: Some("https://calendar.example.org/e".to_owned()),
            place: None,
        }
    }

    fn lefts(looked: &Looked) -> Vec<(String, i64)> {
        looked
            .tabs
            .iter()
            .map(|t| (t.meeting.id.clone(), t.left))
            .collect()
    }

    #[test]
    fn a_tab_comes_at_its_lead_stays_10_min_then_goes_to_the_history() {
        let mut r = Reminders::default();
        let meetings = vec![m("p", AT + 30, 5)];
        // Before: none, the next look at 14:25.
        let looked = r.look(meetings.clone(), AT);
        assert!(looked.tabs.is_empty());
        assert_eq!(looked.wake, Some(AT + 25));
        // At 14:25, "dans 5 min"; looked at again each minute.
        let looked = r.look(meetings.clone(), AT + 25);
        assert_eq!(lefts(&looked), [("p".to_owned(), 5)]);
        assert_eq!(looked.wake, Some(AT + 26));
        let serial = looked.tabs[0].serial;
        // 9 min after the start, still there.
        let looked = r.look(meetings.clone(), AT + 39);
        assert_eq!(lefts(&looked), [("p".to_owned(), -9)]);
        // 10 min after: to the history, its link kept until its end.
        let looked = r.look(meetings.clone(), AT + 40);
        assert!(looked.tabs.is_empty());
        assert_eq!(looked.kept, [(serial, meetings[0].clone())]);
        assert_eq!(looked.wake, Some(AT + 60));
        assert_eq!(r.in_history(serial), Some(&meetings[0]));
        let looked = r.look(meetings.clone(), AT + 60);
        assert_eq!(looked.ended, [(serial, meetings[0].clone())]);
        assert_eq!(looked.wake, None);
        assert_eq!(r.in_history(serial), None);
    }

    #[test]
    fn missed_while_asleep_it_shows_unless_the_meeting_ran_10_min() {
        let meetings = vec![m("p", AT + 30, 5)];
        let mut r = Reminders::default();
        r.look(meetings.clone(), AT);
        // Woken at 14:38: it shows, "started 8 min ago".
        assert_eq!(
            lefts(&r.look(meetings.clone(), AT + 38)),
            [("p".to_owned(), -8)]
        );
        let mut r = Reminders::default();
        r.look(meetings.clone(), AT);
        // At 14:40, too late: none, nor in the history.
        let looked = r.look(meetings, AT + 40);
        assert!(
            looked.tabs.is_empty() && looked.kept.is_empty(),
            "{looked:?}"
        );
    }

    #[test]
    fn later_takes_it_away_until_1_min_before_then_until_the_start() {
        let meetings = vec![m("p", AT + 30, 5)];
        let mut r = Reminders::default();
        let serial = r.look(meetings.clone(), AT + 25).tabs[0].serial;
        let acted = r.act(serial, Act::Later, AT + 25);
        assert!(matches!(acted, Some(Acted::Later { back, .. }) if back == AT + 29));
        let looked = r.look(meetings.clone(), AT + 26);
        assert!(looked.tabs.is_empty());
        assert_eq!(looked.wake, Some(AT + 29));
        // Back at 14:29, "dans 1 min": Later now brings it at the start.
        assert_eq!(
            lefts(&r.look(meetings.clone(), AT + 29)),
            [("p".to_owned(), 1)]
        );
        let acted = r.act(serial, Act::Later, AT + 29);
        assert!(matches!(acted, Some(Acted::Later { back, .. }) if back == AT + 30));
        assert_eq!(r.look(meetings.clone(), AT + 29).wake, Some(AT + 30));
        assert_eq!(
            lefts(&r.look(meetings.clone(), AT + 30)),
            [("p".to_owned(), 0)]
        );
        // Started: no Later.
        assert_eq!(r.act(serial, Act::Later, AT + 30), None);
    }

    #[test]
    fn joined_opened_or_dismissed_it_comes_no_more() {
        for act in [Act::Join, Act::Open, Act::Dismiss] {
            let meetings = vec![m("p", AT + 30, 5)];
            let mut r = Reminders::default();
            let serial = r.look(meetings.clone(), AT + 25).tabs[0].serial;
            let acted = r.act(serial, act, AT + 25).expect("acted");
            match (act, acted) {
                (Act::Join, Acted::Opens { url, .. }) => {
                    assert_eq!(url.as_deref(), Some("https://meet.example.org/abc"));
                }
                (Act::Open, Acted::InCalendar(m)) => assert_eq!(m.id, "p"),
                (Act::Dismiss, Acted::Dismissed(_)) => {}
                other => panic!("{other:?}"),
            }
            let looked = r.look(meetings.clone(), AT + 26);
            assert!(looked.tabs.is_empty(), "{act:?}");
            // Nor to the history at its stay's end.
            assert!(r.look(meetings, AT + 40).kept.is_empty(), "{act:?}");
        }
    }

    #[test]
    fn two_meetings_a_tab_each_the_earliest_first_a_change_in_place_a_cancel_away() {
        let mut r = Reminders::default();
        let (a, b) = (m("a", AT + 32, 5), m("b", AT + 30, 15));
        let looked = r.look(vec![a.clone(), b.clone()], AT + 28);
        assert_eq!(lefts(&looked), [("b".to_owned(), 2), ("a".to_owned(), 4)]);
        let serial = looked.tabs[1].serial;
        // Renamed: in place, the same tab.
        let renamed = Meeting {
            title: "Démo".to_owned(),
            ..a.clone()
        };
        let looked = r.look(vec![renamed, b.clone()], AT + 28);
        assert_eq!(looked.tabs[1].serial, serial);
        assert_eq!(looked.tabs[1].meeting.title, "Démo");
        // Cancelled (no longer a meeting): its tab goes.
        assert_eq!(lefts(&r.look(vec![b], AT + 28)), [("b".to_owned(), 2)]);
    }

    #[test]
    fn super_j_joins_the_tab_shown_else_the_next_within_10_min() {
        let mut r = Reminders::default();
        let soon = m("s", AT + 8, 5);
        let mut far = m("f", AT + 11, 5);
        far.video = Some("https://meet.example.org/far".to_owned());
        r.look(vec![soon.clone(), far.clone()], AT);
        assert_eq!(r.join_key(AT, &[]).map(|m| m.id), Some("s".to_owned()));
        // Joined: its reminder will not show.
        assert!(r.look(vec![soon, far.clone()], AT + 3).tabs.is_empty());
        // 11 min ahead: nothing to join.
        let mut r = Reminders::default();
        r.look(vec![far.clone()], AT);
        assert_eq!(r.join_key(AT, &[]), None);
        // A tab shown goes first.
        let looked = r.look(vec![far], AT + 6);
        assert_eq!(
            r.join_key(AT + 6, &looked.tabs).map(|m| m.id),
            Some("f".to_owned())
        );
    }

    #[test]
    fn the_tabs_words_in_both_languages() {
        let (fr, en) = (Words::new(Lang::French), Words::new(Lang::English));
        let tab = |left| Tab {
            serial: 1,
            meeting: Meeting {
                hhmm: "10:00".to_owned(),
                title: "Revue de sprint".to_owned(),
                ..m("r", AT, 5)
            },
            left,
        };
        assert_eq!(fr.line(&tab(5)), "10:00 · dans 5\u{a0}min · visio");
        assert_eq!(en.line(&tab(5)), "10:00 · in 5\u{a0}min · video");
        assert_eq!(fr.line(&tab(1)), "10:00 · dans 1\u{a0}min · visio");
        assert_eq!(fr.line(&tab(0)), "10:00 · maintenant · visio");
        assert_eq!(en.line(&tab(0)), "10:00 · now · video");
        assert_eq!(
            fr.line(&tab(-4)),
            "10:00 · a commencé il y a 4\u{a0}min · visio"
        );
        assert_eq!(en.line(&tab(-4)), "10:00 · started 4\u{a0}min ago · video");
        let mut room = tab(5);
        room.meeting.video = None;
        room.meeting.place = Some("Salle Jupiter · 3e étage".to_owned());
        assert_eq!(
            fr.line(&room),
            "10:00 · dans 5\u{a0}min · Salle Jupiter · 3e étage"
        );
        assert_eq!(
            fr.name(&tab(5), false),
            "Revue de sprint, dans 5 minutes, visio"
        );
        assert_eq!(
            en.name(&tab(5), false),
            "Revue de sprint, in 5 minutes, video call"
        );
        assert_eq!(fr.title(&tab(5), true), "Réunion");
        assert_eq!(en.title(&tab(5), true), "Meeting");
        assert_eq!((fr.join(), en.join()), ("Rejoindre", "Join"));
        assert_eq!((fr.open(), en.open()), ("Voir", "Open"));
        assert_eq!(fr.later(5), "Me le redire 1 min avant");
        assert_eq!(en.later(1), "Remind me when it starts");
        assert_eq!(fr.dismiss(), "Ignorer ce rappel");
        assert_eq!(en.dismiss(), "Dismiss this reminder");
        assert_eq!(
            fr.started(&tab(0).meeting),
            "Revue de sprint · a commencé à 10:00"
        );
        assert_eq!(
            en.started(&tab(0).meeting),
            "Revue de sprint · started at 10:00"
        );
    }

    #[test]
    fn the_tab_sits_beside_the_bar_at_the_cards_height_or_at_the_edge_in_full_screen() {
        let mut places = Places {
            card: true,
            ..Places::default()
        };
        // Full form: its top on the card's (108), 8 px left of the bar.
        let o = placed(&places, "eDP-1", Form::Full, 0);
        assert_eq!((o.size, o.margin), ((340, 44), (108, 8 + 196 + 8, 0, 0)));
        // The second, 6 px under.
        assert_eq!(placed(&places, "eDP-1", Form::Full, 1).margin.0, 108 + 50);
        // Mini: centred on the mark (8 + 68 + 8), beside the mini bar.
        let o = placed(&places, "eDP-1", Form::Mini, 0);
        assert_eq!(o.margin, (84 - 22, 8 + 56 + 8, 0, 0));
        // Full screen: 8 px from the screen's edge, at the same height.
        places.fullscreen.insert("eDP-1".to_owned());
        assert_eq!(
            placed(&places, "eDP-1", Form::Full, 0).margin,
            (108, 8, 0, 0)
        );
        // On a GPU, room for its shadow, the tab where it was.
        places.card = false;
        let o = placed(&places, "eDP-1", Form::Full, 0);
        assert_eq!(o.size, (340 + 45 + 8, 44 + 35 + 55));
        assert_eq!(o.margin, (108 - 35, 0, 0, 0));
        assert_eq!((o.tab.x, o.tab.y), (45.0, 35.0));
    }
}
