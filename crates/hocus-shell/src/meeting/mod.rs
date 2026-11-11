//! The next meeting (bar.md §4 group 2, §5, §6; Day.2, variant B): its
//! card under the clock in the full form, its mark in the mini, and the
//! halo's meeting states.
//!
//! - **Which meeting**: the first of today's events of the featured
//!   calendars ([`hocus_day::Day::featured`]: `[[day.calendars]]
//!   featured`, every shown calendar without it) that take time
//!   ([`hocus_day::model::Event::takes_time`]: timed, neither cancelled,
//!   declined nor free) and have not ended. One in progress keeps the card
//!   until its end. Nothing left today: no card, and the column under it
//!   (the sign of what is in use, Today, the to-do list, the agents'
//!   group: `crate::column`) back under the clock.
//! - **When it changes**: as the day changes ([`crate::day::on_change`],
//!   on the day's thread), and at the clock's minute change ([`minute`]):
//!   the card's minutes, its line and its colours change then, nothing
//!   draws in between. The times are whole minutes: the meeting's start
//!   and end to the nearest, the clock's minute as it shows it, so that
//!   "dans 1 min" lasts until the start.
//! - **The halo** (§6): told to the compositor (`meeting ten|two|none`)
//!   as it changes: in 10 min or less, in 2 min or less; none once it has
//!   started, nor once the user opened it or joined it.
//! - **Clicks**: the card opens the calendar (Day.5,
//!   [`crate::popover::calendar`]) on today with its meeting chosen, the
//!   join button its video call in the default browser through the
//!   launcher's `xdg-open`; the mini form's mark opens its popover
//!   ([`crate::popover::meeting`]).
//!
//! The card has a surface of its own on each bar, opened while a meeting
//! is left today and removed after ([`draw`]).

pub mod draw;

use hocus_words::shell::meeting as txt;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_day::{
    Day,
    model::{self, Colour},
};
use hocus_link::{EDGE, Form, MEETING_NAMESPACE, MeetingNear, ToCompositor};
use iced::window;
use jiff::{Timestamp, Zoned};

use crate::{
    clock::time::Lang,
    popover::{self, Effect, Item},
};

/// Under the card (or the mark), as between the bar's items.
pub const AFTER: f32 = 14.0;
/// The card's height (full form) and the mark's button's (mini).
pub const CARD_HEIGHT: f32 = 44.0;
pub const MARK_HEIGHT: f32 = 16.0;
/// "10 min or less", "2 min or less".
const SOON: i64 = 10;
const NOW: i64 = 2;

/// Where the meeting is in its day, at a minute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// It starts in `minutes` (1 at least).
    Ahead { minutes: i64 },
    /// It started: `left` minutes until its end (1 at least), `share` of
    /// it gone (0 to 1).
    Under { left: i64, share: f32 },
}

/// How the card looks in its phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// More than 10 min ahead.
    Far,
    /// 10 min or less.
    Soon,
    /// 2 min or less.
    Now,
    /// In progress.
    Under,
}

impl Phase {
    pub fn look(self) -> Look {
        match self {
            Self::Ahead { minutes } if minutes <= NOW => Look::Now,
            Self::Ahead { minutes } if minutes <= SOON => Look::Soon,
            Self::Ahead { .. } => Look::Far,
            Self::Under { .. } => Look::Under,
        }
    }

    /// The halo's state (§6): none once the meeting has started.
    pub fn near(self) -> Option<MeetingNear> {
        match self.look() {
            Look::Now => Some(MeetingNear::Two),
            Look::Soon => Some(MeetingNear::Ten),
            Look::Far | Look::Under => None,
        }
    }
}

/// The card's meeting, as it shows at a minute.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The event's id (an occurrence's, for a recurring one).
    pub id: String,
    pub title: String,
    /// Its calendar's colour.
    pub colour: [u8; 3],
    /// Its start and end, `HH:MM` in the system's zone.
    pub start: String,
    pub end: String,
    pub phase: Phase,
    /// Its video call's address, which the join button opens.
    pub video: Option<String>,
    /// Its page in its calendar, which the card opens.
    pub page: Option<String>,
    pub place: Option<String>,
    /// The event after it today: its start and its title (the mini
    /// popover's "ENSUITE").
    pub then: Option<(String, String)>,
}

impl Card {
    /// Whether the card draws the same as `other`: all but what only its
    /// popover shows (the event after it).
    fn draws_as(&self, other: &Self) -> bool {
        Self {
            then: None,
            ..self.clone()
        } == Self {
            then: None,
            ..other.clone()
        }
    }
}

/// Bars whose card's surface came (`Some`) or went, for the popovers.
type Placed = Vec<(String, Option<window::Id>)>;

/// The popovers told where the cards' surfaces are: never under the card's
/// lock, which the popovers' controller takes in turn as an item acts.
fn tell_placed(placed: Placed) {
    if placed.is_empty() {
        return;
    }
    popover::with(|c| {
        for (bar, id) in placed {
            c.set_meeting(&bar, id);
        }
    });
}

/// An instant's minute, to the nearest: a meeting's start and end.
pub(crate) fn nearest_minute(t: Timestamp) -> i64 {
    (t.as_millisecond() + 30_000).div_euclid(60_000)
}

/// The clock's minute at `now`, as it shows it.
pub(crate) fn clock_minute(now: &Zoned) -> i64 {
    now.timestamp().as_millisecond().div_euclid(60_000)
}

/// An instant's time in `now`'s zone, as the desktop writes it: "14:25",
/// or "2:25 PM" over 12 hours (`[locale] time`).
pub(crate) fn hhmm(t: Timestamp, now: &Zoned) -> String {
    let time = t.to_zoned(now.time_zone().clone()).time();
    crate::popover::calendar::words::Hours::from_env().write(time)
}

/// Today's events of the featured calendars that take time and have not
/// ended at `now`, in order: the card's meeting first, then the one after
/// it (the mini popover's "ENSUITE").
pub fn ahead<'a>(day: &'a Day, now: &Zoned) -> Vec<hocus_day::Shown<'a>> {
    let (from, to) = model::today(now);
    left(day.featured(from, to, now.time_zone()), now)
}

/// Today's events of every shown calendar, featured or not, that take
/// time and have not ended at `now`, in order: what Today lists (Day.3),
/// once the card's is left out.
pub fn today<'a>(day: &'a Day, now: &Zoned) -> Vec<hocus_day::Shown<'a>> {
    let (from, to) = model::today(now);
    left(day.events(from, to, now.time_zone()), now)
}

/// Those of `shown` that take time and have not ended at `now`.
fn left<'a>(shown: Vec<hocus_day::Shown<'a>>, now: &Zoned) -> Vec<hocus_day::Shown<'a>> {
    let zone = now.time_zone();
    let minute = clock_minute(now);
    shown
        .into_iter()
        .filter(|shown| shown.event.takes_time())
        .filter(|shown| nearest_minute(shown.event.when.end(zone)) > minute)
        .collect()
}

/// The card at `now`, from `day`: the first of today's events that take
/// time and have not ended.
pub fn card_of(day: &Day, now: &Zoned) -> Option<Card> {
    let zone = now.time_zone();
    let ahead = ahead(day, now);
    let shown = ahead.first()?;
    let event = shown.event;
    let (start, end) = (event.when.start(zone), event.when.end(zone));
    let (start_m, end_m, minute) = (
        nearest_minute(start),
        nearest_minute(end),
        clock_minute(now),
    );
    let phase = if minute < start_m {
        Phase::Ahead {
            minutes: start_m - minute,
        }
    } else {
        let long = (end_m - start_m).max(1);
        Phase::Under {
            left: (end_m - minute).max(1),
            share: ((minute - start_m) as f32 / long as f32).clamp(0.0, 1.0),
        }
    };
    let Colour(colour) = shown.calendar.colour;
    Some(Card {
        id: event.id.as_str().to_owned(),
        title: event.title.clone(),
        colour,
        start: hhmm(start, now),
        end: hhmm(end, now),
        phase,
        video: event.video.clone().filter(|v| !v.is_empty()),
        page: event.page.clone().filter(|p| !p.is_empty()),
        place: event.place.clone().filter(|p| !p.is_empty()),
        then: ahead.get(1).map(|next| {
            (
                hhmm(next.event.when.start(zone), now),
                next.event.title.clone(),
            )
        }),
    })
}

// --- What shows, and where -------------------------------------------------------

/// The card's surface on a bar.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: Option<window::Id>,
    form: Form,
    switching: bool,
}

#[derive(Debug, Default)]
struct State {
    card: Option<Card>,
    /// The meeting the user opened or joined: no halo for it any more.
    acted: Option<String>,
    /// The halo's state as last told.
    told: Option<Option<MeetingNear>>,
    places: HashMap<String, Place>,
}

impl State {
    fn near(&self) -> Option<MeetingNear> {
        let card = self.card.as_ref()?;
        if self.acted.as_deref() == Some(card.id.as_str()) {
            return None;
        }
        card.phase.near()
    }

    /// The halo's line, if it changed since last told.
    fn halo(&mut self) -> Option<Effect> {
        let near = self.near();
        if self.told == Some(near) {
            return None;
        }
        self.told = Some(near);
        tracing::debug!(?near, "the next meeting's halo");
        Some(Effect::Tell(ToCompositor::Meeting(near)))
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// The card now, if a meeting is left today.
pub fn snapshot() -> Option<Card> {
    state(|s| s.card.clone())
}

/// The room the card takes under the clock in `form`, the 14 px after it
/// included: none without a meeting.
pub fn room(form: Form) -> f32 {
    if state(|s| s.card.is_none()) {
        return 0.0;
    }
    match form {
        Form::Full => CARD_HEIGHT + AFTER,
        Form::Mini => MARK_HEIGHT + AFTER,
    }
}

/// Starts following the day: woken by it on its thread.
pub fn start() {
    crate::day::on_change(|| {
        refresh();
        true
    });
}

/// The clock's minute changed (each bar's clock says it): the card at the
/// new minute.
pub fn minute() {
    refresh();
}

/// The shell is linked again: the halo's state, told again.
pub fn linked() {
    // Told on the link at once: the popovers' effects may not run yet.
    let near = state(|s| {
        let near = s.near();
        s.told = Some(near);
        near
    });
    // Said as `halo` says it: linked before the day is read, the card that
    // comes with nothing near changes nothing, and nothing else says so.
    tracing::debug!(?near, "the next meeting's halo");
    popover::tell(ToCompositor::Meeting(near));
}

/// The card as the day and the clock say now: if it changed, its surfaces
/// come, go or draw again, the groups under it move, the halo is told.
pub fn refresh() {
    let now = crate::notifications::wall();
    // Hidden (`[panel] meeting`, 6.4): no card, as with nothing left.
    let card = card_of(&crate::day::snapshot(), &now).filter(|_| crate::column::groups().meeting);
    let mut moved = false;
    let mut redraw = false;
    let mut effects = Vec::new();
    let mut placed = Placed::new();
    let changed = state(|s| {
        if s.card == card {
            return false;
        }
        let (was, is) = (s.card.is_some(), card.is_some());
        redraw = match (&s.card, &card) {
            (Some(a), Some(b)) => !a.draws_as(b),
            _ => true,
        };
        s.card = card.clone();
        if was != is {
            moved = true;
            for (bar, place) in &mut s.places {
                effects.extend(place.show(bar, is, &mut placed));
            }
        }
        effects.extend(s.halo());
        true
    });
    tell_placed(placed);
    if !changed {
        return;
    }
    let words = Words::new(Lang::of_messages());
    match &card {
        Some(card) => tracing::debug!(said = %words.said(card), "the meeting card shows"),
        None => tracing::debug!("no meeting left today: no card"),
    }
    if moved {
        crate::in_use::moved();
        crate::column::moved();
        if card.is_none() && popover::with(|c| c.open_item()) == Some(Item::MeetingMark) {
            popover::event(popover::Event::Chosen);
        }
    }
    // Only what the card draws wakes it: the event after it is the
    // popover's alone.
    if redraw {
        effects.push(Effect::Wake(MEETING_NAMESPACE));
    }
    popover::send(effects);
    popover::changed(Item::MeetingMark);
}

/// The user opened or joined the card's meeting: its halo ends (§6).
fn acted() {
    let effects: Vec<Effect> = state(|s| {
        s.acted = s.card.as_ref().map(|c| c.id.clone());
        s.halo().into_iter().collect()
    });
    popover::send(effects);
}

/// The user acted on meeting `id` elsewhere (its reminder joined, opened
/// or dismissed, Day.6): its halo ends, so that its 2-min wave does not
/// run. The card stays.
pub fn acted_on(id: &str) {
    let effects: Vec<Effect> = state(|s| {
        s.acted = Some(id.to_owned());
        s.halo().into_iter().collect()
    });
    popover::send(effects);
}

/// The card's click (or `Enter` on it), on `bar`: the calendar (Day.5) on
/// today, its meeting chosen.
pub fn open_in_calendar(bar: &str) -> Option<Effect> {
    let card = snapshot()?;
    acted();
    tracing::info!(id = card.id, "the meeting opens in the calendar");
    Some(Effect::OpenCalendar {
        bar: bar.to_owned(),
        day: crate::notifications::wall().date(),
        event: Some(hocus_day::model::EventId::new(card.id)),
    })
}

/// The join button: the meeting's video call in the browser.
pub fn join() -> Option<Effect> {
    let card = snapshot()?;
    let video = card.video?;
    acted();
    tracing::info!(id = card.id, "the meeting's video call opens");
    crate::launcher::browse(&video)
}

impl Place {
    /// The card shows on `bar` (`shown`), or goes: what opens or removes
    /// its surface.
    fn show(&mut self, bar: &str, shown: bool, placed: &mut Placed) -> Vec<Effect> {
        match (self.id, shown) {
            (None, true) => {
                let id = window::Id::unique();
                self.id = Some(id);
                let (size, margin) = placement(self.form, self.switching);
                placed.push((bar.to_owned(), Some(id)));
                vec![Effect::Layer {
                    id,
                    bar: bar.to_owned(),
                    namespace: MEETING_NAMESPACE,
                    size,
                    margin,
                }]
            }
            (Some(id), false) => {
                self.id = None;
                placed.push((bar.to_owned(), None));
                vec![Effect::Remove(id)]
            }
            _ => Vec::new(),
        }
    }
}

/// The surface's size and its margins in `form` (both forms' places while
/// the bar switches).
fn placement(form: Form, switching: bool) -> ((u32, u32), (i32, i32, i32, i32)) {
    let ((w, h), top) = draw::placement(form, switching);
    ((w, h), (EDGE + top.round() as i32, EDGE + 1, 0, 0))
}

/// Bar `bar` opened in `form`: its card, if a meeting is left.
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let mut placed = Placed::new();
    let effects = state(|s| {
        let shown = s.card.is_some();
        let place = s.places.entry(bar.to_owned()).or_insert(Place {
            id: None,
            form,
            switching: false,
        });
        place.form = form;
        place.show(bar, shown, &mut placed)
    });
    tell_placed(placed);
    effects
}

/// Bar `bar` in `form`, switching or not: its card's size and place.
pub fn form(bar: &str, form: Form, switching: bool) -> Vec<Effect> {
    state(|s| {
        let Some(place) = s.places.get_mut(bar) else {
            return Vec::new();
        };
        if place.form == form && place.switching == switching {
            return Vec::new();
        }
        place.form = form;
        place.switching = switching;
        let Some(id) = place.id else {
            return Vec::new();
        };
        let (size, margin) = placement(form, switching);
        vec![Effect::Resize {
            id,
            size,
            margin: Some(margin),
        }]
    })
}

/// Bar `bar` went: its card's surface, to remove.
pub fn forget(bar: &str) -> Option<window::Id> {
    state(|s| s.places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose card's surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    state(|s| {
        s.places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

// --- Words ----------------------------------------------------------------------

/// The card's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    /// The language its words are said in.
    pub(crate) fn lang(self) -> Lang {
        self.lang
    }
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    /// "25 min", "1 h 05".
    pub(crate) fn minutes(minutes: i64) -> String {
        if minutes >= 60 {
            format!("{}\u{a0}h\u{a0}{:02}", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}\u{a0}min")
        }
    }

    /// "dans 25 min", "fin dans 18 min": the time line's after the start.
    pub fn relative(self, phase: Phase) -> String {
        match (phase, (self.lang == Lang::French)) {
            (Phase::Ahead { minutes }, true) => format!("dans {}", Self::minutes(minutes)),
            (Phase::Ahead { minutes }, false) => format!("in {}", Self::minutes(minutes)),
            (Phase::Under { left, .. }, true) => format!("fin dans {}", Self::minutes(left)),
            (Phase::Under { left, .. }, false) => format!("ends in {}", Self::minutes(left)),
        }
    }

    /// The join button's name.
    pub fn join_call(self) -> &'static str {
        self.pick(txt::JOIN_THE_VIDEO_CALL)
    }

    /// The mini popover's label.
    pub fn heading(self, phase: Phase) -> &'static str {
        match phase {
            Phase::Ahead { .. } => self.pick(txt::NEXT_MEETING),
            Phase::Under { .. } => self.pick(txt::NOW),
        }
    }

    pub fn then(self) -> &'static str {
        self.pick(txt::NEXT)
    }

    pub fn join(self) -> &'static str {
        self.pick(txt::JOIN)
    }

    pub fn show_in_calendar(self) -> &'static str {
        self.pick(txt::SHOW_IN_THE_CALENDAR)
    }

    /// The card's name for screen readers: "Prochaine réunion : Point
    /// produit, 14:00, dans 25 minutes, visio".
    pub fn name(self, card: &Card) -> String {
        let long = |minutes: i64| {
            if minutes >= 60 {
                let (h, m) = (minutes / 60, minutes % 60);
                match ((self.lang == Lang::French), m) {
                    (true, 0) => format!("{h} h"),
                    (false, 0) => format!("{h} h"),
                    (true, m) => format!("{h} h {m:02}"),
                    (false, m) => format!("{h} h {m:02}"),
                }
            } else if minutes == 1 {
                self.pick(txt::N_1_MINUTE).to_owned()
            } else {
                format!("{minutes} minutes")
            }
        };
        let when = match (card.phase, (self.lang == Lang::French)) {
            (Phase::Ahead { minutes }, true) => format!("dans {}", long(minutes)),
            (Phase::Ahead { minutes }, false) => format!("in {}", long(minutes)),
            (Phase::Under { left, .. }, true) => format!("fin dans {}", long(left)),
            (Phase::Under { left, .. }, false) => format!("ends in {}", long(left)),
        };
        let heading = match card.phase {
            Phase::Ahead { .. } => self.pick(txt::NEXT_MEETING_2),
            Phase::Under { .. } => self.pick(txt::MEETING_NOW),
        };
        let colon = txt::TEXT.get(self.lang);
        let mut name = format!("{heading}{colon} {}, {}, {when}", card.title, card.start);
        if card.video.is_some() {
            name.push_str(self.pick(txt::VIDEO_CALL));
        } else if let Some(place) = &card.place {
            name.push_str(", ");
            name.push_str(place);
        }
        name
    }

    /// What the card says, for the log (and tests): its title, its time
    /// line, its look, its button.
    pub fn said(self, card: &Card) -> String {
        let line = match card.phase {
            Phase::Ahead { .. } => format!("{} · {}", card.start, self.relative(card.phase)),
            Phase::Under { .. } => self.relative(card.phase),
        };
        let share = match card.phase {
            Phase::Under { share, .. } => format!(" elapsed={share:.2}"),
            Phase::Ahead { .. } => String::new(),
        };
        format!(
            "«{}» {line} look={:?}{share} join={} page={} colour=#{:02X}{:02X}{:02X}",
            card.title,
            card.phase.look(),
            card.video.is_some(),
            card.page.is_some(),
            card.colour[0],
            card.colour[1],
            card.colour[2],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_day::{
        Snapshot,
        model::{AccountId, Answer, Calendar, CalendarId, Event, EventId, FreeBusy, Status, When},
    };

    fn at(hm: &str) -> Zoned {
        format!("2026-11-04T{hm}:00[UTC]").parse().expect("a time")
    }

    fn t(hm: &str) -> Timestamp {
        at(hm).timestamp()
    }

    fn event(id: &str, title: &str, from: &str, to: &str) -> Event {
        Event {
            id: EventId::new(id),
            calendar: CalendarId::new("work"),
            title: title.to_owned(),
            when: When::Timed {
                start: t(from),
                end: t(to),
            },
            status: Status::Confirmed,
            answer: Answer::Unanswered,
            free_busy: FreeBusy::Busy,
            place: None,
            video: None,
            page: None,
            description: None,
            alarms: Vec::new(),
        }
    }

    fn day(events: Vec<Event>) -> Day {
        let mut snapshot = Snapshot::loading(hocus_day::Account {
            id: AccountId::new("fixture"),
            kind: hocus_day::Kind::Fixture,
            holds: hocus_day::Holds::Calendars,
            name: "day.toml".to_owned(),
        });
        snapshot.calendars = vec![Calendar {
            id: CalendarId::new("work"),
            account: AccountId::new("fixture"),
            name: "Travail".to_owned(),
            colour: Colour([0x8F, 0xB3, 0xFF]),
            read_only: false,
            shown: true,
            featured: true,
        }];
        snapshot.events = events;
        let mut day = Day::default();
        day.update(snapshot);
        day
    }

    fn fr() -> Words {
        Words::new(Lang::French)
    }

    #[test]
    fn the_card_is_the_first_timed_event_not_ended_skipping_those_that_take_no_time() {
        let mut all_day = event("a", "Congés", "00:00", "00:00");
        all_day.when = When::AllDay {
            first: jiff::civil::date(2026, 11, 4),
            last: jiff::civil::date(2026, 11, 4),
        };
        let mut declined = event("d", "Refusée", "13:30", "14:30");
        declined.answer = Answer::Declined;
        let mut free = event("f", "Libre", "13:40", "14:30");
        free.free_busy = FreeBusy::Free;
        let mut cancelled = event("c", "Annulée", "13:50", "14:30");
        cancelled.status = Status::Cancelled;
        let over = event("o", "Finie", "12:00", "13:00");
        let point = event("p", "Point produit", "14:25", "14:55");
        let sprint = event("s", "Revue de sprint", "16:00", "17:00");
        let day = day(vec![
            all_day, declined, free, cancelled, over, point, sprint,
        ]);
        let card = card_of(&day, &at("14:00")).expect("a card");
        assert_eq!(card.title, "Point produit");
        assert_eq!(card.phase, Phase::Ahead { minutes: 25 });
        assert_eq!((card.start.as_str(), card.end.as_str()), ("14:25", "14:55"));
        assert_eq!(
            card.then,
            Some(("16:00".to_owned(), "Revue de sprint".to_owned()))
        );
        assert_eq!(card.colour, [0x8F, 0xB3, 0xFF]);
    }

    #[test]
    fn a_meeting_in_progress_keeps_the_card_until_its_end_then_the_next_takes_it() {
        let day = day(vec![
            event("p", "Point produit", "14:00", "14:30"),
            event("e", "Entretien", "14:15", "15:00"),
        ]);
        let card = card_of(&day, &at("14:12")).expect("a card");
        assert_eq!(card.title, "Point produit");
        assert_eq!(
            card.phase,
            Phase::Under {
                left: 18,
                share: 0.4
            }
        );
        // Overlapping: the one in progress keeps it.
        assert_eq!(
            card_of(&day, &at("14:29")).expect("a card").title,
            "Point produit"
        );
        // Ended: the next one, in progress since 14:15.
        let card = card_of(&day, &at("14:30")).expect("a card");
        assert_eq!(card.title, "Entretien");
        assert_eq!(card.phase.look(), Look::Under);
        assert_eq!(card.then, None);
        // Nothing left today: no card.
        assert_eq!(card_of(&day, &at("15:00")), None);
    }

    #[test]
    fn the_minutes_are_whole_dans_1_min_lasting_until_the_start() {
        // The fixture's "now+25m" lands a few milliseconds past the minute.
        let mut point = event("p", "Point produit", "14:25", "14:55");
        point.when = When::Timed {
            start: t("14:25") + jiff::SignedDuration::from_millis(250),
            end: t("14:55") + jiff::SignedDuration::from_millis(250),
        };
        let day = day(vec![point]);
        let phase = |hms: &str| {
            let now: Zoned = format!("2026-11-04T{hms}[UTC]").parse().expect("a time");
            card_of(&day, &now).expect("a card").phase
        };
        assert_eq!(phase("14:00:00.004"), Phase::Ahead { minutes: 25 });
        assert_eq!(phase("14:00:59.9"), Phase::Ahead { minutes: 25 });
        assert_eq!(phase("14:24:00"), Phase::Ahead { minutes: 1 });
        assert_eq!(phase("14:24:59.9"), Phase::Ahead { minutes: 1 });
        assert_eq!(
            phase("14:25:00"),
            Phase::Under {
                left: 30,
                share: 0.0
            }
        );
    }

    #[test]
    fn its_looks_and_the_halo_by_how_near_it_is() {
        let ahead = |minutes| Phase::Ahead { minutes };
        assert_eq!(ahead(11).look(), Look::Far);
        assert_eq!(ahead(11).near(), None);
        assert_eq!(ahead(10).look(), Look::Soon);
        assert_eq!(ahead(10).near(), Some(MeetingNear::Ten));
        assert_eq!(ahead(3).near(), Some(MeetingNear::Ten));
        assert_eq!(ahead(2).look(), Look::Now);
        assert_eq!(ahead(2).near(), Some(MeetingNear::Two));
        assert_eq!(ahead(1).near(), Some(MeetingNear::Two));
        let under = Phase::Under {
            left: 18,
            share: 0.4,
        };
        assert_eq!(under.look(), Look::Under);
        assert_eq!(under.near(), None);
    }

    #[test]
    fn the_halo_ends_once_the_user_opened_the_meeting() {
        let card = card_of(
            &day(vec![event("p", "Point produit", "14:08", "14:30")]),
            &at("14:00"),
        );
        let mut s = State {
            card,
            ..State::default()
        };
        assert_eq!(
            s.halo(),
            Some(Effect::Tell(ToCompositor::Meeting(Some(MeetingNear::Ten))))
        );
        // Told once.
        assert_eq!(s.halo(), None);
        s.acted = Some("p".to_owned());
        assert_eq!(s.halo(), Some(Effect::Tell(ToCompositor::Meeting(None))));
    }

    #[test]
    fn the_cards_words_in_both_languages() {
        let en = Words::new(Lang::English);
        let ahead = |minutes| Phase::Ahead { minutes };
        assert_eq!(fr().relative(ahead(25)), "dans 25\u{a0}min");
        assert_eq!(en.relative(ahead(25)), "in 25\u{a0}min");
        assert_eq!(fr().relative(ahead(65)), "dans 1\u{a0}h\u{a0}05");
        assert_eq!(en.relative(ahead(65)), "in 1\u{a0}h\u{a0}05");
        assert_eq!(fr().relative(ahead(1)), "dans 1\u{a0}min");
        let under = Phase::Under {
            left: 18,
            share: 0.4,
        };
        assert_eq!(fr().relative(under), "fin dans 18\u{a0}min");
        assert_eq!(en.relative(under), "ends in 18\u{a0}min");
        assert_eq!(fr().join_call(), "Rejoindre la visio");
        assert_eq!(en.join_call(), "Join the video call");
        assert_eq!(fr().heading(ahead(3)), "PROCHAINE RÉUNION");
        assert_eq!(en.heading(ahead(3)), "NEXT MEETING");
        assert_eq!(fr().heading(under), "EN COURS");
        assert_eq!(en.heading(under), "NOW");
        assert_eq!((fr().then(), en.then()), ("ENSUITE", "NEXT"));
        assert_eq!((fr().join(), en.join()), ("Rejoindre", "Join"));
        assert_eq!(fr().show_in_calendar(), "Voir dans le calendrier");
        assert_eq!(en.show_in_calendar(), "Show in the calendar");
        let mut point = event("p", "Point produit", "14:00", "14:30");
        point.video = Some("https://meet.example.org/abc".to_owned());
        let card = card_of(&day(vec![point]), &at("13:35")).expect("a card");
        assert_eq!(
            fr().name(&card),
            "Prochaine réunion : Point produit, 14:00, dans 25 minutes, visio"
        );
        assert_eq!(
            en.name(&card),
            "Next meeting: Point produit, 14:00, in 25 minutes, video call"
        );
    }

    #[test]
    fn the_room_it_takes_under_the_clock() {
        // The column under it moves up 58 px (the card and its gap)
        // without one; the mini form's items 30 (the mark's button and its
        // gap).
        assert_eq!(CARD_HEIGHT + AFTER, 58.0);
        assert_eq!(MARK_HEIGHT + AFTER, 30.0);
    }
}
