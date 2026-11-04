//! The calendar (Day.5, variant A; #429): a popover opened by a click on
//! the clock's group (the date and the clock, the mini form's clock too),
//! 320 px wide, its top on the group's: the month, Monday first with the
//! ISO weeks, a dot under each day with events; then the chosen day's
//! agenda, its all-day events first and its timed ones by start, the
//! list scrolling past 236 px. It always opens on today.
//!
//! - **Clicks**: a day chooses it (one of the months around turns to its
//!   month); the arrows turn the month, not the day chosen; "Aujourd'hui"
//!   brings today back. An event opens its video call, else its own page
//!   (both close the popover), else unfolds its details under its row.
//! - **The keyboard** is on today as it opens: `←` `→` a day, `↑` `↓` a
//!   week, `Page Up` `Page Down` a month, `Home` `End` the week's ends,
//!   turning the month past it; `Enter` or `Space` chooses the day. `Tab`
//!   goes through the arrows, "Aujourd'hui", the days (one stop) and each
//!   event, where `Enter` acts as a click and `↑` `↓` go from one to the
//!   next. `Esc` closes, the keyboard back on the clock.
//! - **The data** is the day's ([`crate::day::snapshot`]), read off the
//!   loop by its workers: the window of 2 months back to 12 ahead. As the
//!   popover turns to a month (or opens on one) whose six weeks reach out
//!   of the window, it asks the workers for those months (Day.5b,
//!   [`crate::day::months`]): a day of them says "Chargement…" until they
//!   answer, its month without dots, then its dots and its agenda show; a
//!   month an account did not give says its days are not loaded. A change
//!   of the day draws the popover again while it is open, and nothing
//!   while it is closed.
//! - **At rest** it draws nothing, but at midnight and as today's events
//!   end (they grey out).
//!
//! [`open_on`] opens it on a day with an event chosen: the next meeting's
//! card's (Day.2).

mod draw;
pub mod layout;
pub mod words;

use std::{
    collections::HashSet,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_day::{
    MonthState,
    model::{EventId, Status, When, midnight},
    snapshot::Holds,
    source::Range,
};
use iced::{
    Point, Rectangle, Renderer, Theme, keyboard, mouse,
    widget::canvas::{self, Frame, Geometry},
    window,
};
use jiff::{Timestamp, Zoned, civil::Date, tz::TimeZone};

pub use self::layout::WIDTH;
use self::{
    layout::{Act, Agenda, Context, Ev, Laid, Stop, Target, View},
    words::{Hours, Words},
};
use super::{Event as Seen, Item, Pressed, event, kit};
use crate::clock::time::Lang;

/// A notch of a mouse's wheel scrolls the list by a row.
const NOTCH: f32 = 44.0;

/// What the popover shows, kept across its draws and read by its height.
static VIEW: Mutex<Option<View>> = Mutex::new(None);
/// What [`open_on`] asked it to open on.
static ASKED: Mutex<Option<(Date, Option<EventId>)>> = Mutex::new(None);
/// How many times it opened: each opening asks for its month again.
static OPENED: AtomicU64 = AtomicU64::new(0);

fn view<T>(with: impl FnOnce(&mut View) -> T) -> T {
    let mut view = VIEW.lock().unwrap_or_else(PoisonError::into_inner);
    let view = view.get_or_insert_with(|| View::on(crate::notifications::wall().date(), None));
    with(view)
}

/// From the shell's start: a change of the day draws the calendar again
/// while it is open (its surface alone), and does nothing while it is
/// closed.
pub fn start() {
    crate::day::on_change(|| {
        super::changed(Item::Calendar);
        true
    });
}

/// Opens the calendar on `bar` (its screen's name; else the first bar)
/// beside its clock, on `day` with `event` chosen (none: the day alone):
/// that event holds the keyboard and is scrolled to, unfolded when a
/// click unfolds it (no video call, no page). What was open closes; the
/// calendar open already turns to it. For the next meeting's card (Day.2).
pub fn open_on(bar: &str, day: Date, event: Option<EventId>) {
    *ASKED.lock().unwrap_or_else(PoisonError::into_inner) = Some((day, event));
    let effects = super::with(|c| c.open_calendar(bar, Instant::now()));
    super::send(effects);
}

/// Its popover opens: on what [`open_on`] asked, else on today.
pub(super) fn opening() {
    let asked = ASKED.lock().unwrap_or_else(PoisonError::into_inner).take();
    let now = crate::notifications::wall();
    let opened = match asked {
        Some((day, Some(id))) => {
            let unfolds = agenda(day, &now)
                .events()
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.act() == Act::Unfold);
            View::on(day, unfolds.map(|u| (id, u)))
        }
        Some((day, None)) => View::on(day, None),
        None => View::on(now.date(), None),
    };
    tracing::debug!(day = %opened.selected, focus = ?opened.focus, "the calendar opens");
    OPENED.fetch_add(1, Ordering::Relaxed);
    *VIEW.lock().unwrap_or_else(PoisonError::into_inner) = Some(opened);
}

/// Its height as it shows now.
pub(super) fn height() -> u32 {
    let view = view(|v| v.clone());
    let read = Read::now(&view);
    let laid = layout::lay_out(&view, &read.context(), &mut kit::measure);
    layout::height(&laid)
}

impl Agenda {
    fn events(&self) -> &[Ev] {
        match self {
            Self::Events(events) => events,
            Self::NoSource | Self::Loading | Self::NotLoaded => &[],
        }
    }
}

/// The day's agenda for `day`, as the day holds it now (`now`).
fn agenda(day: Date, now: &Zoned) -> Agenda {
    let held = crate::day::snapshot();
    if !held
        .accounts()
        .iter()
        .any(|a| a.account.holds == Holds::Calendars)
    {
        return Agenda::NoSource;
    }
    let zone = now.time_zone();
    let (from, to) = (midnight(day, zone), midnight(layout::days_on(day, 1), zone));
    let events: Vec<Ev> = held
        .events(from, to, zone)
        .into_iter()
        // Google keeps a cancelled event with its times: the bar leaves
        // it out (day.md).
        .filter(|s| s.event.status != Status::Cancelled)
        .map(|s| Ev {
            id: s.event.id.clone(),
            title: s.event.title.clone(),
            when: s.event.when,
            colour: s.calendar.colour.0,
            calendar: s.calendar.name.clone(),
            place: s.event.place.clone(),
            video: s.event.video.clone(),
            page: s.event.page.clone(),
            description: s.event.description.clone(),
        })
        .collect();
    let within = Range::window(now).covers(&Range { from, to });
    if within || !events.is_empty() {
        return Agenda::Events(events);
    }
    match held.month(day.first_of_month()) {
        Some(MonthState::Loaded) | None => Agenda::Events(events),
        Some(MonthState::Loading) => Agenda::Loading,
        Some(MonthState::Failed(_)) => Agenda::NotLoaded,
    }
}

/// The months of `month`'s six weeks outside the window, asked of the
/// workers (as the popover turns to it): they read those they have not
/// read yet, off the loop.
fn ask_months(month: Date) {
    let wall = crate::notifications::wall();
    let zone = wall.time_zone();
    let outside = Range::window(&wall).months_outside(
        layout::grid_start(month),
        layout::grid_end(month),
        zone,
    );
    if outside.is_empty() {
        return;
    }
    let said: Vec<String> = outside
        .iter()
        .map(|m| m.strftime("%Y-%m").to_string())
        .collect();
    tracing::debug!(months = %said.join(" "), "the calendar asks for months outside the window");
    crate::day::months(&outside);
}

/// The days from `first` to `last` included.
fn each_day(first: Date, last: Date) -> impl Iterator<Item = Date> {
    std::iter::successors(Some(first), move |d| {
        (*d < last).then(|| layout::days_on(*d, 1))
    })
}

/// What the layout reads of the day and the clock, now.
struct Read {
    words: Words,
    hours: Hours,
    today: Date,
    now: Timestamp,
    zone: TimeZone,
    busy: HashSet<Date>,
    agenda: Agenda,
}

impl Read {
    fn now(view: &View) -> Self {
        let wall = crate::notifications::wall();
        let zone = wall.time_zone().clone();
        let (first, end) = (layout::grid_start(view.month), layout::grid_end(view.month));
        let held = crate::day::snapshot();
        let mut busy = HashSet::new();
        for shown in held.events(midnight(first, &zone), midnight(end, &zone), &zone) {
            if shown.event.status == Status::Cancelled {
                continue;
            }
            let (from, last) = match shown.event.when {
                When::AllDay { first, last } => (first, last),
                When::Timed { start, end } => {
                    let last = end
                        .checked_sub(jiff::SignedDuration::from_nanos(1))
                        .unwrap_or(end)
                        .max(start);
                    (
                        start.to_zoned(zone.clone()).date(),
                        last.to_zoned(zone.clone()).date(),
                    )
                }
            };
            let last = last.min(layout::days_on(end, -1));
            busy.extend(each_day(from.max(first), last));
        }
        Self {
            words: Words::new(Lang::of_messages()),
            hours: Hours::from_env(),
            today: wall.date(),
            now: wall.timestamp(),
            agenda: agenda(view.selected, &wall),
            zone,
            busy,
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            words: self.words,
            hours: self.hours,
            today: self.today,
            now: self.now,
            zone: &self.zone,
            busy: &self.busy,
            agenda: &self.agenda,
        }
    }

    /// When what it shows changes by itself: today's next event ending
    /// (greyed then), else midnight (today moves).
    fn next_change(&self, view: &View) -> Option<Duration> {
        let midnight = midnight(layout::days_on(self.today, 1), &self.zone);
        let ends = (view.selected == self.today)
            .then(|| {
                self.agenda
                    .events()
                    .iter()
                    .filter(|e| !e.when.all_day())
                    .map(|e| e.when.end(&self.zone))
                    .filter(|end| *end > self.now)
                    .min()
            })
            .flatten();
        let at = ends.map_or(midnight, |end| end.min(midnight));
        let wait = at.duration_since(self.now);
        // Just past the moment, as the clock looks.
        Duration::try_from(wait)
            .ok()
            .map(|wait| wait + Duration::from_millis(20))
    }
}

// --- The panel ------------------------------------------------------------------

/// The calendar on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
    /// Drawn on the CPU: text in the list cut to it.
    pub software: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    laid: Option<Laid>,
    shown: Option<View>,
    /// The month last asked of the workers ([`ask_months`]), and in which
    /// opening.
    asked: Option<(u64, Date)>,
    pointer: kit::Pointer<Target>,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            kit::Input::Redraw(_) => self.refresh(state),
            kit::Input::Mouse(seen, at) => self.mouse(state, seen, at),
            kit::Input::Key(pressed) => {
                let action = self.key(state, &pressed);
                self.refresh::<Message>(state);
                action
            }
            kit::Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        if let (Some(laid), Some(view)) = (&state.laid, &state.shown) {
            let clip = kit::Clip::new(self.software, draw::list_view(laid));
            draw::calendar(&mut frame, laid, view, state.pointer.hover, clip);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

impl Panel {
    /// What it shows, read afresh, and when it next changes by itself.
    fn refresh<Message>(&self, state: &mut State) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let shown = view(|v| v.clone());
        // Turned to a month (or opened on one): those of its weeks out of
        // the window asked for, once.
        let asking = (OPENED.load(Ordering::Relaxed), shown.month);
        if state.asked != Some(asking) {
            state.asked = Some(asking);
            ask_months(shown.month);
        }
        let read = Read::now(&shown);
        let laid = layout::lay_out(&shown, &read.context(), &mut kit::measure);
        let shown = view(|v| {
            // The list scrolled as laid out (clamped, to what is revealed).
            v.scroll = laid.scroll;
            v.reveal = false;
            v.clone()
        });
        if state.laid.as_ref() != Some(&laid) || state.shown.as_ref() != Some(&shown) {
            if state.laid.as_ref().map(layout::height) != Some(layout::height(&laid)) {
                tracing::debug!(height = layout::height(&laid), "the calendar's height");
            }
            tracing::debug!(calendar = %laid.said(&shown), "the calendar shows it");
            let targets: Vec<String> = laid
                .targets()
                .iter()
                .filter(|(t, _)| !matches!(t, Target::Day(_)))
                .map(|(t, r)| format!("{t:?}@{:.0},{:.0}", r.center().x, r.center().y))
                .collect();
            tracing::debug!(targets = %targets.join(" "), "the calendar's targets");
            if let Some(name) = self.focused_name(&laid, &shown, &read) {
                tracing::debug!(name, "the calendar's keyboard is on");
            }
            state.laid = Some(laid);
            state.shown = Some(shown.clone());
            super::refit(Item::Calendar);
        }
        let wait = read.next_change(&shown)?;
        Some(canvas::Action::request_redraw_at(Instant::now() + wait))
    }

    /// What holds the keyboard, as a screen reader would say it.
    fn focused_name(&self, laid: &Laid, view: &View, read: &Read) -> Option<String> {
        let words = read.words;
        match view.focus.as_ref()? {
            Stop::Previous => Some(words.previous().to_owned()),
            Stop::Next => Some(words.next().to_owned()),
            Stop::Today => Some(words.today_button().to_owned()),
            Stop::Choose => Some(words.choose_calendars().to_owned()),
            Stop::Days => {
                let count = if view.focus_day == view.selected {
                    read.agenda.events().len()
                } else {
                    let now = crate::notifications::wall();
                    agenda(view.focus_day, &now).events().len()
                };
                Some(words.day_name(view.focus_day, count, read.today))
            }
            Stop::Event(id) => laid
                .rows()
                .iter()
                .find(|r| r.id == *id)
                .map(|r| r.title.clone()),
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let laid = state.laid.as_ref()?;
        let target = at.and_then(|at| {
            laid.targets()
                .into_iter()
                .find(|(_, r)| r.contains(at))
                .map(|(t, _)| t)
        });
        let in_list = at.is_some_and(|at| draw::list_view(laid).is_some_and(|v| v.contains(at)));
        let ids: Vec<EventId> = laid.rows().iter().map(|r| r.id.clone()).collect();
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.click(target, &ids);
                    self.refresh::<Message>(state);
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } if in_list => {
                let by = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * NOTCH,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                view(|v| v.scroll = (v.scroll + by).max(0.0));
                self.refresh::<Message>(state);
                redraw
            }
            _ => None,
        }
    }

    /// A click on `target`, the day's events' rows being `ids`.
    fn click(&self, target: Target, ids: &[EventId]) {
        tracing::debug!(?target, "a click in the calendar");
        let today = crate::notifications::wall().date();
        match target {
            Target::Previous => view(|v| v.turn(-1)),
            Target::Next => view(|v| v.turn(1)),
            Target::Today => view(|v| {
                v.select(today);
                v.focus = Some(Stop::Days);
                v.keyed = false;
            }),
            Target::Day(day) => view(|v| {
                v.select(day);
                v.focus = Some(Stop::Days);
                v.keyed = false;
            }),
            Target::Event(row) => {
                if let Some(id) = ids.get(row) {
                    self.act_on(id);
                }
            }
            // Day.1's popover, beside the clock, in its place.
            Target::Choose => event(Seen::Replace(Item::DayChoose)),
        }
    }

    /// Event `id` acted on: its call, its page, or its details.
    fn act_on(&self, id: &EventId) {
        let now = crate::notifications::wall();
        let day = view(|v| v.selected);
        let Some(act) = agenda(day, &now)
            .events()
            .iter()
            .find(|e| e.id == *id)
            .map(Ev::act)
        else {
            return;
        };
        match act {
            Act::Video(url) | Act::Page(url) => {
                tracing::info!(id = id.as_str(), "the calendar opens an event's address");
                super::send(crate::launcher::browse(&url).into_iter().collect());
                event(Seen::Chosen);
            }
            Act::Unfold => view(|v| {
                v.unfolded = if v.unfolded.as_ref() == Some(id) {
                    None
                } else {
                    Some(id.clone())
                };
                v.reveal = true;
            }),
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let captured = Some(canvas::Action::request_redraw().and_capture());
        let Key::Named(named) = &pressed.key else {
            return None;
        };
        if *named == Named::Escape {
            event(Seen::Escape);
            return Some(canvas::Action::capture());
        }
        let events: Vec<EventId> = state
            .laid
            .as_ref()
            .map(|l| l.rows().iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default();
        let focus = view(|v| v.focus.clone());
        let choose = state.laid.as_ref().is_some_and(|l| l.choose.is_some());
        if *named == Named::Tab {
            let order: Vec<Stop> = [Stop::Previous, Stop::Next, Stop::Today, Stop::Days]
                .into_iter()
                .chain(choose.then_some(Stop::Choose))
                .chain(events.into_iter().map(Stop::Event))
                .collect();
            let next = step(&order, focus.as_ref(), pressed.modifiers.shift());
            view(|v| {
                v.keyed |= next == Some(Stop::Days);
                v.reveal = matches!(next, Some(Stop::Event(_)));
                v.focus = next;
            });
            return captured;
        }
        match focus {
            None | Some(Stop::Days) => self.day_key(*named).then_some(captured).flatten(),
            Some(Stop::Event(id)) => match named {
                Named::Enter | Named::Space => {
                    self.act_on(&id);
                    captured
                }
                Named::ArrowUp | Named::ArrowDown => {
                    let at = events.iter().position(|e| *e == id)?;
                    let next = if *named == Named::ArrowUp {
                        at.checked_sub(1)?
                    } else {
                        at + 1
                    };
                    let next = events.get(next)?.clone();
                    view(|v| {
                        v.focus = Some(Stop::Event(next));
                        v.reveal = true;
                    });
                    captured
                }
                _ => None,
            },
            Some(Stop::Choose) => match named {
                Named::Enter | Named::Space => {
                    self.click(Target::Choose, &[]);
                    captured
                }
                _ => None,
            },
            Some(stop @ (Stop::Previous | Stop::Next | Stop::Today)) => match named {
                Named::Enter | Named::Space => {
                    let target = match stop {
                        Stop::Previous => Target::Previous,
                        Stop::Next => Target::Next,
                        _ => Target::Today,
                    };
                    self.click(target, &[]);
                    view(|v| v.focus = Some(stop));
                    captured
                }
                _ => None,
            },
        }
    }

    /// A key on the days: whether it was theirs.
    fn day_key(&self, named: keyboard::key::Named) -> bool {
        use keyboard::key::Named;
        let from = view(|v| v.focus_day);
        let to = match named {
            Named::ArrowLeft => layout::days_on(from, -1),
            Named::ArrowRight => layout::days_on(from, 1),
            Named::ArrowUp => layout::days_on(from, -7),
            Named::ArrowDown => layout::days_on(from, 7),
            Named::PageUp => layout::months_on(from, -1),
            Named::PageDown => layout::months_on(from, 1),
            Named::Home => layout::week_end(from, true),
            Named::End => layout::week_end(from, false),
            Named::Enter | Named::Space => {
                view(|v| {
                    v.select(v.focus_day);
                    v.focus = Some(Stop::Days);
                    v.keyed = true;
                });
                return true;
            }
            _ => return false,
        };
        view(|v| v.focus_on(to));
        true
    }
}

/// What holds the keyboard after `focus` in `order` (`back`: before it),
/// round its ends; from nothing, the first (the last).
fn step(order: &[Stop], focus: Option<&Stop>, back: bool) -> Option<Stop> {
    let n = order.len() as isize;
    if n == 0 {
        return None;
    }
    let at = focus.and_then(|f| order.iter().position(|s| s == f));
    let by = if back { -1 } else { 1 };
    let next = match at {
        Some(i) => (i as isize + by).rem_euclid(n),
        None if by > 0 => 0,
        None => n - 1,
    };
    order.get(next as usize).cloned()
}
