//! The Date et heure page (6.11, settings.md "Date et heure"): the clock,
//! the automatic time, the time set by hand, the time zone and its
//! picker, the link to the format.
//!
//! systemd's timedated is followed and asked on a thread of its own
//! ([`crate::timedate`]), started the first time the page shows; nothing
//! is waited for on the window's thread, and nothing is written in the
//! file. A zone chosen toasts with "Annuler" and joins `Ctrl+Z`; the time
//! set by hand toasts without it; the automatic time is its switch. What
//! the system refuses (polkit) is said by a toast, and the page shows
//! what timedated still holds.
//!
//! At rest the page draws once a minute: its clock, from a one-shot timer
//! to the next minute while it shows.

mod view;

use std::time::Duration;

use iced::{
    Task,
    futures::channel::mpsc::UnboundedSender,
    keyboard::{Key, key::Named},
    widget::operation,
};

use super::{App, Doing, Focus, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{
    lang::{Lang, w},
    model::SectionId,
    time::{self as model, Found},
    timedate::{self, Ask, Asks, Heard, TimeDate},
};

/// The picker's search field, and the fields of the time set by hand.
pub(super) const ZONE_SEARCH: &str = "zone-search";
pub(super) const DATE_FIELD: &str = "time-date";
pub(super) const TIME_FIELD: &str = "time-time";

#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Heard),
    /// The automatic time's switch.
    Ntp,
    /// The date and the time typed.
    Date(String),
    Time(String),
    /// "Régler".
    Set,
    /// `↑` / `↓` in a field, which holds the keyboard or not.
    Step {
        date: bool,
        up: bool,
        focused: bool,
    },
    /// The picker opened or closed.
    Picker(bool),
    Query(String),
    /// A zone's row clicked: chosen.
    Pick(usize),
    /// The highlighted one chosen.
    Choose,
    /// The next minute.
    Tick,
    /// "Apparence et langue ›".
    Format,
}

/// A zone chosen, for its undo: the one before and the one after, as
/// their toasts say them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub before: String,
    pub after: String,
    pub before_said: String,
    pub after_said: String,
}

/// A control of the page that takes the keyboard, in `Tab`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Ntp,
    Set,
    Zone,
    Format,
}

/// The time zone picker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picker {
    pub query: String,
    pub selected: usize,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// Where the worker's messages go.
    to: Option<UnboundedSender<Message>>,
    asks: Option<Asks>,
    /// timedated as last heard: `None` until it is, `Some(None)` when it
    /// is not there.
    pub(super) heard: Option<Option<TimeDate>>,
    /// The automatic time asked, shown until timedated answers.
    ntp_asked: Option<bool>,
    /// A zone asked, why, and what its toast says.
    zone_asked: Option<(Choice, Doing)>,
    /// The time asked, as its toast says it.
    time_asked: Option<String>,
    /// The date and the time typed, while they are.
    pub(super) draft: Option<(String, String)>,
    pub(super) picker: Option<Picker>,
    /// The picker as last drawn: open, its query and its row.
    drawn: Option<(String, usize)>,
    /// The next minute's tick is asked.
    ticking: bool,
}

impl State {
    /// The page, its worker to start the first time it shows, telling the
    /// window through `to`.
    pub fn new(to: UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to),
            ..Self::default()
        }
    }

    /// timedated, when it is there.
    pub(super) fn td(&self) -> Option<&TimeDate> {
        self.heard.as_ref()?.as_ref()
    }

    fn ask(&self, ask: Ask) -> bool {
        let sent = self
            .asks
            .as_ref()
            .is_some_and(|a| a.unbounded_send(ask).is_ok());
        if !sent {
            tracing::warn!("timedated's thread has stopped: dropped");
        }
        sent
    }
}

impl App {
    fn time_shows(&self) -> bool {
        self.page == Page::Section(SectionId::DateTime)
    }

    /// The zone the page shows: the one asked while it runs, else
    /// timedated's.
    pub(super) fn time_zone(&self) -> Option<&str> {
        self.time
            .zone_asked
            .as_ref()
            .map(|(c, _)| c.after.as_str())
            .or_else(|| self.time.td().map(|t| t.zone.as_str()))
            .filter(|z| !z.is_empty())
    }

    /// The automatic time as the switch shows it.
    pub(super) fn time_ntp(&self) -> bool {
        self.time
            .ntp_asked
            .or_else(|| self.time.td().map(|t| t.ntp))
            .unwrap_or(false)
    }

    /// Whether the time can be set by hand now.
    pub(super) fn time_manual(&self) -> bool {
        self.time.td().is_some() && !self.time_ntp()
    }

    /// `[locale] time` is 12 hours.
    pub(super) fn time_twelve(&self) -> bool {
        crate::model::setting("locale.time")
            .map(|s| crate::store::Slot::of(s, self.source))
            .and_then(|s| self.value(s))
            == Some(crate::model::Value::Word("12h"))
    }

    /// `[locale] date`: "long", "short", "numeric" or "iso".
    pub(super) fn time_date_format(&self) -> &'static str {
        match crate::model::setting("locale.date")
            .map(|s| crate::store::Slot::of(s, self.source))
            .and_then(|s| self.value(s))
        {
            Some(crate::model::Value::Word(word)) => word,
            _ => "long",
        }
    }

    /// Now, in the zone the page shows.
    pub(super) fn time_now(&self) -> jiff::Zoned {
        model::now_in(self.time_zone(), jiff::Timestamp::now())
    }

    /// The fields' texts: as typed, else the clock's.
    pub(super) fn time_fields(&self) -> (String, String) {
        if let Some((date, time)) = &self.time.draft {
            return (date.clone(), time.clone());
        }
        let now = self.time_now();
        (
            model::say_numeric(now.date(), self.time_date_format() == "iso"),
            model::say_time(now.time(), self.time_twelve()),
        )
    }

    /// The time the fields hold, in microseconds since the epoch, if
    /// they read.
    pub(super) fn time_typed(&self) -> Option<i64> {
        let (date, time) = self.time_fields();
        let tz = self.time_zone().and_then(model::zone)?;
        model::usec_of(model::parse_date(&date)?, model::parse_time(&time)?, &tz)
    }

    /// The picker's rows now.
    pub(super) fn time_found(&self) -> Vec<Found> {
        let query = self.time.picker.as_ref().map_or("", |p| p.query.as_str());
        model::find(query, self.time_zone(), self.lang, jiff::Timestamp::now())
    }

    pub(super) fn time_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(heard) => self.time_heard(heard),
            Msg::Ntp => {
                let Some(td) = self.time.td() else {
                    return Task::none();
                };
                if !td.can_ntp || self.time.ntp_asked.is_some() {
                    return Task::none();
                }
                let on = !self.time_ntp();
                if self.time.ask(Ask::Ntp(on)) {
                    self.time.ntp_asked = Some(on);
                    self.knobs.insert(model::AUTO, std::time::Instant::now());
                }
                Task::none()
            }
            Msg::Date(date) => {
                let (_, time) = self.time_fields();
                self.time.draft = Some((date, time));
                Task::none()
            }
            Msg::Time(time) => {
                let (date, _) = self.time_fields();
                self.time.draft = Some((date, time));
                Task::none()
            }
            Msg::Set => {
                if !self.time_manual() || self.time.time_asked.is_some() {
                    return Task::none();
                }
                let Some(usec) = self.time_typed() else {
                    return Task::none();
                };
                let (_, said) = self.time_fields();
                if self.time.ask(Ask::Time(usec)) {
                    self.time.time_asked = Some(said);
                }
                Task::none()
            }
            Msg::Step { date, up, focused } => {
                if focused && self.time_manual() {
                    self.time_step(date, up);
                }
                Task::none()
            }
            Msg::Picker(open) => {
                if !open {
                    self.time.picker = None;
                    return Task::none();
                }
                if self.time.td().is_none() || self.time.zone_asked.is_some() {
                    return Task::none();
                }
                self.time.picker = Some(Picker::default());
                operation::focus(ZONE_SEARCH)
            }
            Msg::Query(query) => {
                if let Some(picker) = self.time.picker.as_mut() {
                    picker.query = query;
                    picker.selected = 0;
                }
                Task::none()
            }
            Msg::Pick(i) => {
                if let Some(picker) = self.time.picker.as_mut() {
                    picker.selected = i;
                }
                self.time_choose()
            }
            Msg::Choose => self.time_choose(),
            Msg::Tick => {
                self.time.ticking = false;
                Task::none()
            }
            Msg::Format => self.show_row("locale.time"),
        }
    }

    fn time_heard(&mut self, heard: Heard) -> Task<Message> {
        match heard {
            Heard::State(state) => {
                let ntp_before = self.time.td().map(|t| t.ntp);
                self.time.heard = Some(state);
                let ntp_now = self.time.td().map(|t| t.ntp);
                if self.time.ntp_asked.is_none() && ntp_before.is_some() && ntp_before != ntp_now {
                    self.knobs.insert(model::AUTO, std::time::Instant::now());
                }
                if self.time.td().is_none() {
                    self.time.picker = None;
                }
                Task::none()
            }
            Heard::Done(ask, result) => {
                let refused = result.is_err();
                let task = match ask {
                    Ask::Ntp(on) => {
                        self.time.ntp_asked = None;
                        if refused && self.time.td().is_some_and(|t| t.ntp != on) {
                            // The switch goes back.
                            self.knobs.insert(model::AUTO, std::time::Instant::now());
                        }
                        Task::none()
                    }
                    Ask::Time(_) => {
                        let said = self.time.time_asked.take().unwrap_or_default();
                        if refused {
                            Task::none()
                        } else {
                            self.time.draft = None;
                            let text = match self.lang {
                                Lang::French => format!("Heure réglée : {said}."),
                                Lang::English => format!("Time set: {said}."),
                            };
                            return self.toast(text, ToastAction::None, TOAST);
                        }
                    }
                    Ask::Zone(_) => {
                        let Some((choice, doing)) = self.time.zone_asked.take() else {
                            return Task::none();
                        };
                        if refused {
                            Task::none()
                        } else {
                            return self.time_zone_done(choice, doing);
                        }
                    }
                };
                if refused {
                    let text = self
                        .say(w("Le système a refusé.", "The system refused."))
                        .to_owned();
                    return self.toast(text, ToastAction::None, TOAST);
                }
                task
            }
        }
    }

    /// A zone set: kept for its undo, and said.
    fn time_zone_done(&mut self, choice: Choice, doing: Doing) -> Task<Message> {
        let lang = self.lang;
        match doing {
            Doing::New => {
                let text = match lang {
                    Lang::French => format!("Fuseau horaire : {}.", choice.after_said),
                    Lang::English => format!("Time zone: {}.", choice.after_said),
                };
                self.undo.push(super::Undo::Time(choice));
                self.redo.clear();
                self.toast(text, ToastAction::Undo, TOAST)
            }
            Doing::Undo => {
                // Back to `after`, which was the choice's `before`.
                let text = match lang {
                    Lang::French => format!(
                        "Annulé : le fuseau horaire revient à {}.",
                        choice.after_said
                    ),
                    Lang::English => {
                        format!("Undone: the time zone is back to {}.", choice.after_said)
                    }
                };
                self.redo.push(super::Undo::Time(reversed(choice)));
                self.light = Some((model::ZONE, std::time::Instant::now()));
                self.toast(text, ToastAction::None, UNDONE)
            }
            Doing::Redo => {
                let text = match lang {
                    Lang::French => format!("Rétabli : fuseau horaire : {}.", choice.after_said),
                    Lang::English => format!("Redone: time zone: {}.", choice.after_said),
                };
                self.undo.push(super::Undo::Time(choice));
                self.light = Some((model::ZONE, std::time::Instant::now()));
                self.toast(text, ToastAction::None, UNDONE)
            }
        }
    }

    /// `choice` undone (or redone): its zone asked back.
    pub(super) fn time_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let (choice, doing) = if redo {
            (choice, Doing::Redo)
        } else {
            (reversed(choice), Doing::Undo)
        };
        self.go(Page::Section(SectionId::DateTime));
        if self.time.ask(Ask::Zone(choice.after.clone())) {
            self.time.zone_asked = Some((choice, doing));
        }
        Task::none()
    }

    /// The highlighted zone chosen: asked of timedated, the picker closed.
    fn time_choose(&mut self) -> Task<Message> {
        let Some(picker) = self.time.picker.as_ref() else {
            return Task::none();
        };
        let found = self.time_found();
        let Some(chosen) = found.get(picker.selected) else {
            return Task::none();
        };
        self.time.picker = None;
        let before = self.time_zone().unwrap_or_default().to_owned();
        if chosen.city.zone == before {
            return Task::none();
        }
        let now = jiff::Timestamp::now();
        let said = |name: &str, zone: &str| {
            let offset = model::zone(zone)
                .map(|tz| model::say_offset(tz.to_offset(now)))
                .unwrap_or_default();
            format!("{name} ({offset})")
        };
        let choice = Choice {
            before_said: said(
                model::city_of(&before).map_or(before.as_str(), |c| c.name(self.lang)),
                &before,
            ),
            after_said: said(chosen.city.name(self.lang), chosen.city.zone),
            after: chosen.city.zone.to_owned(),
            before,
        };
        if self.time.ask(Ask::Zone(choice.after.clone())) {
            self.time.zone_asked = Some((choice, Doing::New));
        }
        Task::none()
    }

    /// `↑` / `↓` in a field: the day, or the minute, one more or less.
    fn time_step(&mut self, date: bool, up: bool) {
        let (date_text, time_text) = self.time_fields();
        let by: i64 = if up { 1 } else { -1 };
        let twelve = self.time_twelve();
        let iso = self.time_date_format() == "iso";
        if date {
            if let Some(d) = model::parse_date(&date_text)
                && let Ok(d) = d.checked_add(jiff::Span::new().days(by))
            {
                self.time.draft = Some((model::say_numeric(d, iso), time_text));
            }
        } else if let Some(t) = model::parse_time(&time_text) {
            let t = t.wrapping_add(jiff::Span::new().minutes(by));
            self.time.draft = Some((date_text, model::say_time(t, twelve)));
        }
    }

    /// After each message: the worker started the first time the page
    /// shows, the picker closed when it goes, the next minute asked while
    /// it shows.
    pub(super) fn time_after(&mut self) -> Task<Message> {
        if !self.time_shows() {
            self.time.picker = None;
        }
        // The picker opened, changed or closed: whole frames for a moment
        // (`background_of`), or what it covered stays on screen.
        let drawn = self
            .time
            .picker
            .as_ref()
            .map(|p| (p.query.clone(), p.selected));
        if drawn != self.time.drawn {
            self.time.drawn = drawn;
            self.fresh = Some(std::time::Instant::now() + super::FRESH);
        }
        if !self.time_shows() {
            return Task::none();
        }
        if self.time.asks.is_none() {
            let to = self.time.to.clone();
            self.time.asks = Some(timedate::start(move |heard| {
                to.as_ref()
                    .is_some_and(|to| to.unbounded_send(Message::Time(Msg::Heard(heard))).is_ok())
            }));
            tracing::info!("timedated followed");
        }
        if self.time.ticking {
            return Task::none();
        }
        self.time.ticking = true;
        let now = jiff::Zoned::now();
        let into = Duration::from_nanos(u64::try_from(now.subsec_nanosecond()).unwrap_or(0));
        let second = u64::try_from(now.second()).unwrap_or(0);
        let wait = Duration::from_secs(60 - second).saturating_sub(into);
        Task::perform(tokio::time::sleep(wait + Duration::from_millis(5)), |()| {
            Message::Time(Msg::Tick)
        })
    }

    /// A key the page takes before the window does: the picker's, and
    /// `↑` / `↓` in the fields of the time set by hand.
    pub(super) fn time_key_first(&mut self, key: &Key, captured: bool) -> Option<Task<Message>> {
        if !self.time_shows() {
            return None;
        }
        if self.time.picker.is_some() {
            let last = self.time_found().len().saturating_sub(1);
            let picker = self.time.picker.as_mut()?;
            match key.as_ref() {
                Key::Named(Named::Escape) => self.time.picker = None,
                Key::Named(Named::ArrowDown) => picker.selected = (picker.selected + 1).min(last),
                Key::Named(Named::ArrowUp) => picker.selected = picker.selected.saturating_sub(1),
                // Enter is the field's own (`on_submit`); the rest is
                // typed in it.
                _ => {}
            }
            return Some(Task::none());
        }
        let arrow = match key.as_ref() {
            Key::Named(Named::ArrowUp) => Some(true),
            Key::Named(Named::ArrowDown) => Some(false),
            _ => None,
        };
        if let Some(up) = arrow
            && !captured
            && self.time_manual()
            && matches!(self.focus, Focus::Nothing | Focus::Control(_))
        {
            let ask = |id: &'static str, date: bool| {
                operation::is_focused(id)
                    .map(move |focused| Message::Time(Msg::Step { date, up, focused }))
            };
            return Some(Task::batch([ask(DATE_FIELD, true), ask(TIME_FIELD, false)]));
        }
        None
    }

    /// The page's controls, in `Tab`'s order.
    pub(super) fn time_stops(&self) -> Vec<Stop> {
        let mut stops = Vec::new();
        if self.time.td().is_some_and(|t| t.can_ntp) {
            stops.push(Stop::Ntp);
        }
        if self.time_manual() {
            stops.push(Stop::Set);
        }
        if self.time.td().is_some() {
            stops.push(Stop::Zone);
        }
        stops.push(Stop::Format);
        stops
    }

    /// A key on the page's control `stop`.
    pub(super) fn time_key(&mut self, stop: Stop, named: Named) -> Task<Message> {
        if !matches!(named, Named::Enter | Named::Space) {
            return Task::none();
        }
        let msg = match stop {
            Stop::Ntp => Msg::Ntp,
            Stop::Set => Msg::Set,
            Stop::Zone => Msg::Picker(true),
            Stop::Format => Msg::Format,
        };
        self.time_update(msg)
    }

    /// Whether `stop` holds the keyboard.
    pub(super) fn time_focused(&self, stop: Stop) -> bool {
        matches!(self.focus, Focus::Control(i) if self.controls().get(i) == Some(&super::Control::Time(stop)))
    }
}

fn reversed(choice: Choice) -> Choice {
    Choice {
        before: choice.after,
        after: choice.before,
        before_said: choice.after_said,
        after_said: choice.before_said,
    }
}
