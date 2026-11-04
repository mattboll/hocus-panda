//! The calendar laid out (Day.5, variant A), at the mockup's sizes at ×1:
//! the header, the month's six weeks, the divider, the day's line and its
//! agenda. Pure: what the day holds, what the view shows and a way to
//! measure text in, places out, for the panel to draw and to hit.

use std::collections::HashSet;

use hocus_day::model::{EventId, When};
use iced::{Font, Point, Rectangle, Size};
use jiff::{
    Timestamp, ToSpan,
    civil::{Date, Weekday},
    tz::TimeZone,
};

use super::words::{Hours, Words};
use crate::{
    levels::draw::{LINE, MANROPE, MANROPE_BOLD},
    popover::kit::{self, MANROPE_SEMIBOLD},
};

// --- The mockup's sizes, at ×1 --------------------------------------------------

/// The briefing's width (§9).
pub const WIDTH: u32 = 320;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 14.0;
const PAD_SIDE: f32 = 16.0;
pub(super) const PAD_BOTTOM: f32 = 12.0;
pub(super) const LEFT: f32 = BORDER + PAD_SIDE;
pub(super) const RIGHT: f32 = WIDTH as f32 - BORDER - PAD_SIDE;
const ROOM: f32 = RIGHT - LEFT;
/// Its parts 10 px apart.
const GAP: f32 = 10.0;
/// The header: 28 px; the month (16 px, 700), "Aujourd'hui" (26 px high,
/// 10 px in, 12 px 600) and the two 26 × 26 arrows, 6 px apart.
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const HEADER: f32 = 28.0;
pub(super) const TITLE: f32 = 16.0;
const BUTTON: f32 = 26.0;
const BUTTON_GAP: f32 = 6.0;
const TODAY_PAD: f32 = 10.0;
pub(super) const TODAY_TEXT: f32 = 12.0;
/// The weeks' column, then seven; the letters' row 18 px (11 px, 700;
/// "sem." 10 px, 600); six rows of 32 px, 2 px apart.
const WEEK_COLUMN: f32 = 28.0;
const LETTERS_ROW: f32 = 18.0;
pub(super) const LETTER: f32 = 11.0;
pub(super) const WEEK_WORD: f32 = 10.0;
const WEEKS: usize = 6;
const ROW: f32 = 32.0;
const ROW_GAP: f32 = 2.0;
/// A day: 34 × 30, its number (Martian Mono 12.5 px, line height 1) 5 px
/// in from its top, its dot 4 px under.
const DAY: Size = Size::new(34.0, 30.0);
const NUMBER_TOP: f32 = 5.0;
pub(super) const NUMBER: f32 = 12.5;
const DOT_GAP: f32 = 4.0;
pub(super) const DOT: f32 = 4.0;
pub(super) const WEEK_NUMBER: f32 = 10.5;
/// The day's line: its name 13.5 px 700, then 8 px on "aujourd'hui"
/// (12.5 px), the count on its right (12 px).
pub(super) const DAY_NAME: f32 = 13.5;
pub(super) const RELATIVE: f32 = 12.5;
pub(super) const COUNT: f32 = 12.0;
const DAY_GAP: f32 = 8.0;
/// How far the day's line may reach into the right padding before its
/// name is cut, as the mockup's line does with a long day ("Mercredi 4
/// novembre", "aujourd'hui" and "6 événements" are 299 px in 286).
const LINE_OVER: f32 = 14.0;
/// The list: 2 px between rows, 236 px at most.
const LIST_GAP: f32 = 2.0;
pub(super) const LIST_MOST: f32 = 236.0;
/// An all-day row: 28 px, 4 px more under it, 10 px in, an 8 px dot, 8 px
/// on its title (12.5 px, 600); its span on its right (11.5 px).
const ALL_DAY: f32 = 28.0;
const ALL_DAY_AFTER: f32 = 4.0;
pub(super) const ALL_DAY_PAD: f32 = 10.0;
pub(super) const ALL_DAY_DOT: f32 = 8.0;
const ALL_DAY_GAP: f32 = 8.0;
pub(super) const ALL_DAY_TITLE: f32 = 12.5;
pub(super) const SPAN: f32 = 11.5;
/// A timed row: 44 px at least, 6 px and 8 px in; a 42 px column with the
/// start (Martian Mono 12 px) 3 px over the end (10.5 px), 10 px, a 3 px
/// bar, 10 px, the title (13.5 px, 600) 2 px over what is under it (12 px,
/// a 13 px icon 5 px before).
const EVENT_LEAST: f32 = 44.0;
pub(super) const EVENT_PAD_Y: f32 = 6.0;
pub(super) const EVENT_PAD_X: f32 = 8.0;
const TIME_COLUMN: f32 = 42.0;
const TIME_GAP: f32 = 10.0;
pub(super) const BAR: f32 = 3.0;
pub(super) const START: f32 = 12.0;
pub(super) const END: f32 = 10.5;
pub(super) const TIME_LINES_GAP: f32 = 3.0;
pub(super) const EVENT_TITLE: f32 = 13.5;
pub(super) const META: f32 = 12.0;
pub(super) const META_GAP: f32 = 2.0;
pub(super) const ICON: f32 = 13.0;
pub(super) const ICON_GAP: f32 = 5.0;
/// An empty day's note: 13.5 px, 14 px above it, 12 px under.
pub(super) const NOTE: f32 = 13.5;
const NOTE_ABOVE: f32 = 14.0;
const NOTE_BELOW: f32 = 12.0;
/// "Choisir des agendas…" under the note: 30 px, 14 px in, 8 px under it.
pub(super) const CHOOSE: f32 = 30.0;
const CHOOSE_PAD: f32 = 14.0;
const CHOOSE_ABOVE: f32 = 8.0;
/// An event unfolded: its details under its title, 12 px on lines 1.4
/// apart, the description on 4 lines at most, 8 px under them.
pub(super) const DETAILS: f32 = 12.0;
pub(super) const DETAILS_LINE: f32 = 1.4;
const DETAILS_BELOW: f32 = 8.0;
const DESCRIPTION_LINES: usize = 4;
/// Martian Mono's line, as CSS's `normal`: its ascent and descent.
pub(super) const MONO_LINE: f32 = 1.2;
/// Manrope's ascent, for lines of several sizes on one baseline.
const ASCENT: f32 = 1.066;

pub(super) const MARTIAN: Font = Font::with_name("Martian Mono");

/// A text's width at a size in a font.
pub(super) type Measure<'a> = &'a mut dyn FnMut(&str, f32, Font) -> f32;

// --- What it shows ---------------------------------------------------------------

/// An event of the day shown, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ev {
    pub id: EventId,
    pub title: String,
    pub when: When,
    pub colour: [u8; 3],
    pub calendar: String,
    pub place: Option<String>,
    pub video: Option<String>,
    pub page: Option<String>,
    pub description: Option<String>,
}

impl Ev {
    /// What a click on it does: its video call, else its own page, else
    /// its details unfold.
    pub fn act(&self) -> Act {
        match (&self.video, &self.page) {
            (Some(video), _) => Act::Video(video.clone()),
            (None, Some(page)) => Act::Page(page.clone()),
            (None, None) => Act::Unfold,
        }
    }
}

/// What a click on an event does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Video(String),
    Page(String),
    Unfold,
}

/// The day's agenda, as the day holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agenda {
    /// No calendar account at all (Day.1 not set up).
    NoSource,
    /// A day outside the window the shell keeps, its month asked of the
    /// accounts and not answered yet (Day.5b).
    Loading,
    /// A day outside the window, its month not given by an account.
    NotLoaded,
    /// Its events, in the bar's order.
    Events(Vec<Ev>),
}

/// What holds the keyboard in the popover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Previous,
    Next,
    Today,
    /// The days: one stop, on the focused day.
    Days,
    Event(EventId),
    /// "Choisir des agendas…", without a source.
    Choose,
}

/// What the popover shows: the month, the day chosen, the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The month shown, its first day.
    pub month: Date,
    /// The day whose agenda shows.
    pub selected: Date,
    /// The day the keyboard is on, which may differ.
    pub focus_day: Date,
    /// The keyboard was used on the days: the focused one is outlined.
    pub keyed: bool,
    pub focus: Option<Stop>,
    pub unfolded: Option<EventId>,
    pub scroll: f32,
    /// The event holding the keyboard (or unfolded) is to be scrolled to.
    pub reveal: bool,
}

impl View {
    /// As it opens: `day`'s month, `day` chosen and the keyboard on it;
    /// with `event`, that event holding the keyboard, unfolded if its
    /// click unfolds it.
    pub fn on(day: Date, event: Option<(EventId, bool)>) -> Self {
        let reveal = event.is_some();
        let (focus, unfolded) = match event {
            Some((id, unfolds)) => (Stop::Event(id.clone()), unfolds.then_some(id)),
            None => (Stop::Days, None),
        };
        Self {
            month: day.first_of_month(),
            selected: day,
            focus_day: day,
            keyed: false,
            focus: Some(focus),
            reveal,
            unfolded,
            scroll: 0.0,
        }
    }

    /// `day` chosen: its agenda shows, its month too, the keyboard on it.
    pub fn select(&mut self, day: Date) {
        self.selected = day;
        self.focus_day = day;
        self.month = day.first_of_month();
        self.unfolded = None;
        self.scroll = 0.0;
    }

    /// The month shown turned by `by` months; the day chosen stays.
    pub fn turn(&mut self, by: i32) {
        self.month = self
            .month
            .checked_add(by.months())
            .unwrap_or(self.month)
            .first_of_month();
    }

    /// The keyboard moved to `day`: its month shows.
    pub fn focus_on(&mut self, day: Date) {
        self.focus_day = day;
        self.keyed = true;
        self.focus = Some(Stop::Days);
        self.month = day.first_of_month();
    }
}

/// What the layout reads.
pub struct Context<'a> {
    pub words: Words,
    pub hours: Hours,
    pub today: Date,
    pub now: Timestamp,
    pub zone: &'a TimeZone,
    /// The days of the six weeks that have events.
    pub busy: &'a HashSet<Date>,
    pub agenda: &'a Agenda,
}

// --- Laid out --------------------------------------------------------------------

/// A day of the month, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub date: Date,
    pub rect: Rectangle,
    /// Of the month shown (else of the months around).
    pub in_month: bool,
    pub weekend: bool,
    pub today: bool,
    pub selected: bool,
    /// The keyboard's outline.
    pub outlined: bool,
    pub busy: bool,
    /// Its number's middle, and its dot's.
    pub number_middle: f32,
    pub dot_middle: f32,
}

/// A week's row.
#[derive(Debug, Clone, PartialEq)]
pub struct Week {
    pub number: i8,
    pub middle: f32,
    pub days: Vec<Cell>,
}

/// A text on a line, from its top.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub text: String,
    pub at: Point,
}

/// The day's line.
#[derive(Debug, Clone, PartialEq)]
pub struct DayLine {
    pub title: Said,
    pub relative: Option<Said>,
    pub count: Option<Said>,
}

/// What shows under an event's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub video: bool,
    pub text: String,
}

/// A row of the agenda's.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// Its span on its right: "toute la journée", "7 – 8 nov.".
    AllDay { span: String },
    Timed {
        start: String,
        end: String,
        meta: Option<Meta>,
    },
}

/// An event's row, in the list's content (from its top).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: EventId,
    pub kind: RowKind,
    pub title: String,
    pub colour: [u8; 3],
    /// Over already, today: at half its opacity.
    pub past: bool,
    pub act: Act,
    pub top: f32,
    /// Its row's height, and with its details when unfolded.
    pub base: f32,
    pub height: f32,
    /// Its details, unfolded: each line.
    pub details: Vec<String>,
    /// Where its title starts, from the row's left.
    pub inset: f32,
}

/// The list's viewport on the popover, and its content's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub view: Rectangle,
    pub content: f32,
}

impl Viewport {
    pub fn scrolls(self) -> bool {
        self.content > self.view.height + 0.5
    }

    pub fn most_scroll(self) -> f32 {
        (self.content - self.view.height).max(0.0)
    }
}

/// Under the day's line.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A note, centred: an empty day, no source, a day loading or not
    /// loaded.
    Note {
        text: String,
        middle: f32,
    },
    Rows {
        list: Viewport,
        rows: Vec<Row>,
    },
}

/// What can be clicked: an event by its row's place in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Previous,
    Next,
    Today,
    Day(Date),
    Event(usize),
    /// "Choisir des agendas…": Day.1's popover, at its first step.
    Choose,
}

/// The popover laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub title: Said,
    pub header_middle: f32,
    pub today_button: Rectangle,
    /// "Aujourd'hui" in `text.faint`: today's month and today shown.
    pub today_faint: bool,
    pub previous: Rectangle,
    pub next: Rectangle,
    pub letters_middle: f32,
    pub week_centre: f32,
    pub columns: [f32; 7],
    pub weeks: Vec<Week>,
    pub rule: f32,
    pub day_line: DayLine,
    pub body: Body,
    /// "Choisir des agendas…" under the note without a source: Day.1's
    /// popover.
    pub choose: Option<Rectangle>,
    /// The list's scroll, clamped (and to the event to reveal).
    pub scroll: f32,
    pub end: f32,
}

/// The six weeks shown for `month` (its first day): from the Monday on or
/// before its first.
pub fn grid_start(month: Date) -> Date {
    let lead = i64::from(month.weekday().to_monday_zero_offset());
    month.checked_sub(lead.days()).unwrap_or(month)
}

/// The day after the six weeks.
pub fn grid_end(month: Date) -> Date {
    let start = grid_start(month);
    start
        .checked_add(((WEEKS * 7) as i64).days())
        .unwrap_or(start)
}

/// The same day `by` months on, or that month's last when it has fewer.
pub fn months_on(day: Date, by: i32) -> Date {
    // jiff's months clamp to the month's last day.
    day.checked_add(by.months()).unwrap_or(day)
}

/// The day `by` days on.
pub fn days_on(day: Date, by: i64) -> Date {
    day.checked_add(by.days()).unwrap_or(day)
}

/// The Monday (`back`), else the Sunday, of `day`'s week.
pub fn week_end(day: Date, back: bool) -> Date {
    let offset = i64::from(day.weekday().to_monday_zero_offset());
    if back {
        days_on(day, -offset)
    } else {
        days_on(day, 6 - offset)
    }
}

/// `text` from `left`'s top at `top`, so that its line's baseline sits on
/// `baseline` (Manrope at `size`).
fn on_baseline(text: String, left: f32, baseline: f32, size: f32) -> Said {
    Said {
        text,
        at: Point::new(left, baseline - ASCENT * size),
    }
}

/// The calendar laid out for `view`, as `context` holds the day.
pub fn lay_out(view: &View, context: &Context<'_>, measure: Measure<'_>) -> Laid {
    let words = context.words;
    // The header.
    let header_middle = HEADER_TOP + HEADER / 2.0;
    let next = Rectangle::new(
        Point::new(RIGHT - BUTTON, header_middle - BUTTON / 2.0),
        Size::new(BUTTON, BUTTON),
    );
    let previous = Rectangle::new(
        Point::new(next.x - BUTTON_GAP - BUTTON, next.y),
        Size::new(BUTTON, BUTTON),
    );
    let today_width = measure(words.today_button(), TODAY_TEXT, MANROPE_SEMIBOLD) + 2.0 * TODAY_PAD;
    let today_button = Rectangle::new(
        Point::new(previous.x - BUTTON_GAP - today_width, next.y),
        Size::new(today_width, BUTTON),
    );
    let title_room = today_button.x - BUTTON_GAP - LEFT;
    let title = kit::elide(&words.month_title(view.month), title_room, |t| {
        measure(t, TITLE, MANROPE_BOLD)
    });
    let title = Said {
        text: title,
        at: Point::new(LEFT, header_middle),
    };
    let today_faint =
        view.month == context.today.first_of_month() && view.selected == context.today;
    // The month.
    let top = HEADER_TOP + HEADER + GAP;
    let letters_middle = top + LETTERS_ROW / 2.0;
    let column = (ROOM - WEEK_COLUMN) / 7.0;
    let columns: [f32; 7] = std::array::from_fn(|i| LEFT + WEEK_COLUMN + (i as f32 + 0.5) * column);
    let start = grid_start(view.month);
    let weeks = (0..WEEKS)
        .map(|w| {
            let row_top = top + LETTERS_ROW + ROW_GAP + w as f32 * (ROW + ROW_GAP);
            let monday = days_on(start, 7 * w as i64);
            let days = (0..7)
                .map(|i| {
                    let date = days_on(monday, i as i64);
                    let rect = Rectangle::new(
                        Point::new(
                            columns[i] - DAY.width / 2.0,
                            row_top + (ROW - DAY.height) / 2.0,
                        ),
                        DAY,
                    );
                    Cell {
                        date,
                        rect,
                        in_month: date.first_of_month() == view.month,
                        weekend: matches!(date.weekday(), Weekday::Saturday | Weekday::Sunday),
                        today: date == context.today,
                        selected: date == view.selected,
                        outlined: view.keyed
                            && view.focus == Some(Stop::Days)
                            && date == view.focus_day,
                        busy: context.busy.contains(&date),
                        number_middle: rect.y + NUMBER_TOP + NUMBER / 2.0,
                        dot_middle: rect.y + NUMBER_TOP + NUMBER + DOT_GAP + DOT / 2.0,
                    }
                })
                .collect();
            Week {
                number: monday.iso_week_date().week(),
                middle: row_top + ROW / 2.0,
                days,
            }
        })
        .collect();
    let month_end = top + LETTERS_ROW + WEEKS as f32 * (ROW_GAP + ROW);
    // The divider, and the day's line on one baseline.
    let rule = month_end + GAP;
    let line_top = rule + 1.0 + GAP;
    let baseline = line_top + ASCENT * DAY_NAME;
    let count = match context.agenda {
        Agenda::Events(events) => words.count(events.len()),
        Agenda::NoSource | Agenda::Loading | Agenda::NotLoaded => None,
    };
    let count_width = count
        .as_deref()
        .map_or(0.0, |c| measure(c, COUNT, MANROPE) + DAY_GAP);
    let relative = words.relative(view.selected, context.today);
    let relative_width = relative.map_or(0.0, |r| measure(r, RELATIVE, MANROPE) + DAY_GAP);
    let name = kit::elide(
        &words.day_title(view.selected, context.today.year()),
        ROOM + LINE_OVER - count_width - relative_width,
        |t| measure(t, DAY_NAME, MANROPE_BOLD),
    );
    let name_width = measure(&name, DAY_NAME, MANROPE_BOLD);
    let count_right =
        (LEFT + name_width + relative_width + count_width).clamp(RIGHT, RIGHT + LINE_OVER);
    let day_line = DayLine {
        title: on_baseline(name, LEFT, baseline, DAY_NAME),
        relative: relative.map(|r| {
            on_baseline(
                r.to_owned(),
                LEFT + name_width + DAY_GAP,
                baseline,
                RELATIVE,
            )
        }),
        count: count.map(|c| on_baseline(c, count_right, baseline, COUNT)),
    };
    let line_end = line_top + DAY_NAME * LINE;
    // The agenda.
    let note = |text: &str| {
        let middle = line_end + GAP + NOTE_ABOVE + NOTE * LINE / 2.0;
        (
            Body::Note {
                text: text.to_owned(),
                middle,
            },
            middle + NOTE * LINE / 2.0 + NOTE_BELOW,
        )
    };
    let mut choose = None;
    let (body, end, scroll) = match context.agenda {
        Agenda::NoSource => {
            let (body, end) = note(words.no_source());
            // Its button under the note, the note's space under it.
            let width =
                measure(words.choose_calendars(), TODAY_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHOOSE_PAD;
            let top = end - NOTE_BELOW + CHOOSE_ABOVE;
            choose = Some(Rectangle::new(
                Point::new((LEFT + RIGHT - width) / 2.0, top),
                Size::new(width, CHOOSE),
            ));
            (body, top + CHOOSE + NOTE_BELOW, 0.0)
        }
        Agenda::Loading => {
            let (body, end) = note(words.loading());
            (body, end, 0.0)
        }
        Agenda::NotLoaded => {
            let (body, end) = note(words.not_loaded());
            (body, end, 0.0)
        }
        Agenda::Events(events) if events.is_empty() => {
            let (body, end) = note(words.empty());
            (body, end, 0.0)
        }
        Agenda::Events(events) => {
            let rows = rows(events, view, context, measure);
            let content = rows.last().map_or(0.0, |r| r.top + r.height);
            let top = line_end + GAP;
            let list = Viewport {
                view: Rectangle::new(
                    Point::new(LEFT, top),
                    Size::new(ROOM, content.min(LIST_MOST)),
                ),
                content,
            };
            let mut scroll = view.scroll.clamp(0.0, list.most_scroll());
            let shown = match &view.focus {
                Some(Stop::Event(id)) if view.reveal => Some(id),
                _ => None,
            }
            .or(view.unfolded.as_ref().filter(|_| view.reveal));
            if let Some(row) = shown.and_then(|id| rows.iter().find(|r| r.id == *id)) {
                scroll = kit::reveal(
                    (row.top, row.height),
                    list.view.height,
                    scroll,
                    list.most_scroll(),
                );
            }
            let end = top + list.view.height;
            (Body::Rows { list, rows }, end, scroll)
        }
    };
    Laid {
        title,
        header_middle,
        today_button,
        today_faint,
        previous,
        next,
        letters_middle,
        week_centre: LEFT + WEEK_COLUMN / 2.0,
        columns,
        weeks,
        rule,
        day_line,
        body,
        choose,
        scroll,
        end,
    }
}

/// The day's rows: the all-day events first, then the timed ones by start.
fn rows(events: &[Ev], view: &View, context: &Context<'_>, measure: Measure<'_>) -> Vec<Row> {
    let (all_day, timed): (Vec<&Ev>, Vec<&Ev>) = events.iter().partition(|e| e.when.all_day());
    // The time column holds the longest time (the 12 hours' "12:30 PM").
    let times: Vec<(String, String)> = timed
        .iter()
        .map(|e| {
            let (start, end) = times(e, context);
            (context.hours.write(start), context.hours.write(end))
        })
        .collect();
    let mut column = TIME_COLUMN;
    for (start, end) in &times {
        column = column
            .max(measure(start, START, MARTIAN))
            .max(measure(end, END, MARTIAN));
    }
    let column = column.ceil();
    let timed_inset = EVENT_PAD_X + column + TIME_GAP + BAR + TIME_GAP;
    let mut out = Vec::new();
    let mut top = 0.0;
    for event in all_day {
        let span = match event.when {
            When::AllDay { first, last } if first != last => context.words.span(first, last),
            _ => context.words.all_day().to_owned(),
        };
        let span_width = measure(&span, SPAN, MANROPE);
        let inset = ALL_DAY_PAD + ALL_DAY_DOT + ALL_DAY_GAP;
        let room = ROOM - inset - ALL_DAY_GAP - span_width - ALL_DAY_PAD;
        let title = kit::elide(&event.title, room, |t| {
            measure(t, ALL_DAY_TITLE, MANROPE_SEMIBOLD)
        });
        let row = row(
            event,
            RowKind::AllDay { span },
            title,
            (top, ALL_DAY, inset),
            view,
            context,
            measure,
        );
        top += row.height + LIST_GAP + ALL_DAY_AFTER;
        out.push(row);
    }
    for (event, (start, end)) in timed.into_iter().zip(times) {
        let room = ROOM - timed_inset - EVENT_PAD_X;
        let title = kit::elide(&event.title, room, |t| {
            measure(t, EVENT_TITLE, MANROPE_SEMIBOLD)
        });
        let meta = match (&event.video, &event.place) {
            (Some(video), _) => Some(Meta {
                video: true,
                text: host_and_path(video),
            }),
            (None, Some(place)) => Some(Meta {
                video: false,
                text: place.clone(),
            }),
            (None, None) => None,
        }
        .map(|meta| Meta {
            text: kit::elide(&meta.text, room - ICON - ICON_GAP, |t| {
                measure(t, META, MANROPE)
            }),
            ..meta
        });
        let column = EVENT_TITLE * LINE + meta.as_ref().map_or(0.0, |_| META_GAP + META * LINE);
        let times = START * MONO_LINE + TIME_LINES_GAP + END * MONO_LINE;
        let base = EVENT_LEAST
            .max(2.0 * EVENT_PAD_Y + column.max(times))
            .ceil();
        let row = row(
            event,
            RowKind::Timed { start, end, meta },
            title,
            (top, base, timed_inset),
            view,
            context,
            measure,
        );
        top += row.height + LIST_GAP;
        out.push(row);
    }
    out
}

/// An event's row from `top`, `base` tall folded, its title `inset` in:
/// its details when it is the one unfolded.
fn row(
    event: &Ev,
    kind: RowKind,
    title: String,
    (top, base, inset): (f32, f32, f32),
    view: &View,
    context: &Context<'_>,
    measure: Measure<'_>,
) -> Row {
    let act = event.act();
    let unfolded = act == Act::Unfold && view.unfolded.as_ref() == Some(&event.id);
    let details = if unfolded {
        details(event, inset, context, measure)
    } else {
        Vec::new()
    };
    let more = if details.is_empty() {
        0.0
    } else {
        (details.len() as f32 * DETAILS * DETAILS_LINE + DETAILS_BELOW).ceil()
    };
    let past = context.today == view.selected
        && !event.when.all_day()
        && event.when.end(context.zone) <= context.now;
    Row {
        id: event.id.clone(),
        kind,
        title,
        colour: event.colour,
        past,
        act,
        top,
        base,
        height: base + more,
        details,
        inset,
    }
}

/// An event's start and end in the zone, as times of day.
fn times(event: &Ev, context: &Context<'_>) -> (jiff::civil::Time, jiff::civil::Time) {
    let at = |t: Timestamp| t.to_zoned(context.zone.clone()).time();
    (
        at(event.when.start(context.zone)),
        at(event.when.end(context.zone)),
    )
}

/// An event's details, unfolded: its time, its place (an all-day row's:
/// a timed one has it under its title), its calendar's name, its
/// description on four lines at most, each cut to the row.
fn details(event: &Ev, inset: f32, context: &Context<'_>, measure: Measure<'_>) -> Vec<String> {
    let room = ROOM - inset - EVENT_PAD_X;
    let mut width = |t: &str| measure(t, DETAILS, MANROPE);
    let when = match event.when {
        When::AllDay { first, last } if first != last => context.words.span(first, last),
        When::AllDay { .. } => context.words.all_day().to_owned(),
        When::Timed { .. } => {
            let (start, end) = times(event, context);
            format!(
                "{} – {}",
                context.hours.write(start),
                context.hours.write(end)
            )
        }
    };
    let mut lines = vec![when];
    // A timed row shows its place under its title already.
    if event.when.all_day() {
        lines.extend(event.place.clone());
    }
    lines.push(event.calendar.clone());
    let mut lines: Vec<String> = lines
        .into_iter()
        .map(|l| kit::elide(&l, room, &mut width))
        .collect();
    if let Some(description) = event.description.as_deref().map(plain) {
        let mut wrapped = kit::wrap(&description, room, &mut width);
        if wrapped.len() > DESCRIPTION_LINES {
            wrapped.truncate(DESCRIPTION_LINES);
            if let Some(last) = wrapped.last_mut() {
                *last = kit::elide(&format!("{last}…"), room, &mut width);
            }
        }
        lines.extend(
            wrapped
                .into_iter()
                .filter(|l| !l.is_empty())
                .map(|l| kit::elide(&l, room, &mut width)),
        );
    }
    lines
}

/// A description as plain text: a source's HTML (Google's) without its
/// tags, its entities read, its spaces and lines as one space each.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            ch => out.push(ch),
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A video call's address as it shows: its host and its path,
/// "meet.google.com/xvr-pqat-kmd".
pub fn host_and_path(address: &str) -> String {
    let rest = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"))
        .unwrap_or(address);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    rest.trim_end_matches('/').to_owned()
}

impl Laid {
    /// What can be clicked, where (on the popover): the event rows within
    /// the list only, scrolled.
    pub fn targets(&self) -> Vec<(Target, Rectangle)> {
        let mut targets = vec![
            (Target::Today, self.today_button),
            (Target::Previous, self.previous),
            (Target::Next, self.next),
        ];
        for cell in self.weeks.iter().flat_map(|w| &w.days) {
            targets.push((Target::Day(cell.date), cell.rect));
        }
        if let Some(rect) = self.choose {
            targets.push((Target::Choose, rect));
        }
        if let Body::Rows { list, rows } = &self.body {
            for (i, row) in rows.iter().enumerate() {
                if let Some(rect) = self.row_rect(*list, row).intersection(&list.view) {
                    targets.push((Target::Event(i), rect));
                }
            }
        }
        targets
    }

    /// `row` on the popover, scrolled.
    pub fn row_rect(&self, list: Viewport, row: &Row) -> Rectangle {
        Rectangle::new(
            Point::new(list.view.x, list.view.y + row.top - self.scroll),
            Size::new(list.view.width, row.height),
        )
    }

    /// The cell of `day`, if it shows.
    #[cfg(test)]
    pub fn cell(&self, day: Date) -> Option<&Cell> {
        self.weeks
            .iter()
            .flat_map(|w| &w.days)
            .find(|c| c.date == day)
    }

    /// The rows, if the day has some.
    pub fn rows(&self) -> &[Row] {
        match &self.body {
            Body::Rows { rows, .. } => rows,
            Body::Note { .. } => &[],
        }
    }

    /// What it shows, for the log and tests.
    pub fn said(&self, view: &View) -> String {
        let mut said = format!(
            "month={} title=«{}» today_button={} selected={} focus={:?} focus_day={} keyed={} \
             day=«{}»",
            view.month.strftime("%Y-%m"),
            self.title.text,
            if self.today_faint { "faint" } else { "text" },
            view.selected,
            view.focus,
            view.focus_day,
            view.keyed,
            self.day_line.title.text,
        );
        if let Some(relative) = &self.day_line.relative {
            said.push_str(&format!(" rel=«{}»", relative.text));
        }
        if let Some(count) = &self.day_line.count {
            said.push_str(&format!(" count=«{}»", count.text));
        }
        let busy: Vec<String> = self
            .weeks
            .iter()
            .flat_map(|w| &w.days)
            .filter(|c| c.busy)
            .map(|c| c.date.to_string())
            .collect();
        said.push_str(&format!(
            " weeks={} busy=[{}]",
            self.weeks
                .iter()
                .map(|w| w.number.to_string())
                .collect::<Vec<_>>()
                .join(","),
            busy.join(",")
        ));
        if self.choose.is_some() {
            said.push_str(" choose=«Choisir des agendas…»");
        }
        match &self.body {
            Body::Note { text, .. } => said.push_str(&format!(" note=«{text}»")),
            Body::Rows { list, rows } => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|r| {
                        let mut row = match &r.kind {
                            RowKind::AllDay { span } => format!("all-day «{}» {span}", r.title),
                            RowKind::Timed { start, end, meta } => {
                                let meta = meta.as_ref().map_or(String::new(), |m| {
                                    format!(
                                        " {}={}",
                                        if m.video { "video" } else { "place" },
                                        m.text
                                    )
                                });
                                format!("{start}–{end} «{}»{meta}", r.title)
                            }
                        };
                        if r.past {
                            row.push_str(" past");
                        }
                        if !r.details.is_empty() {
                            row.push_str(&format!(" unfolded=[{}]", r.details.join(" | ")));
                        }
                        row.push_str(&format!(" h={}", r.height));
                        row
                    })
                    .collect();
                said.push_str(&format!(
                    " list={}/{} scroll={} rows=[{}]",
                    list.view.height,
                    list.content,
                    self.scroll,
                    rows.join("; ")
                ));
            }
        }
        said
    }
}

/// The popover's height for `laid`.
pub fn height(laid: &Laid) -> u32 {
    (laid.end + PAD_BOTTOM + BORDER).round() as u32
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::clock::time::Lang;

    fn measure(t: &str, size: f32, _font: Font) -> f32 {
        t.chars().count() as f32 * size * 0.55
    }

    fn ev(id: &str, title: &str, when: When) -> Ev {
        Ev {
            id: EventId::new(id),
            title: title.to_owned(),
            when,
            colour: [0x8F, 0xB3, 0xFF],
            calendar: "Travail".to_owned(),
            place: None,
            video: None,
            page: None,
            description: None,
        }
    }

    fn at(day: Date, h: i8, m: i8) -> Timestamp {
        day.at(h, m, 0, 0)
            .to_zoned(TimeZone::UTC)
            .map(|z| z.timestamp())
            .unwrap_or(Timestamp::UNIX_EPOCH)
    }

    fn timed(id: &str, title: &str, day: Date, (h, m): (i8, i8), (eh, em): (i8, i8)) -> Ev {
        ev(
            id,
            title,
            When::Timed {
                start: at(day, h, m),
                end: at(day, eh, em),
            },
        )
    }

    const TODAY: Date = date(2026, 11, 4);
    static UTC: TimeZone = TimeZone::UTC;

    fn context<'a>(busy: &'a HashSet<Date>, agenda: &'a Agenda) -> Context<'a> {
        Context {
            words: Words::new(Lang::French),
            hours: Hours::TwentyFour,
            today: TODAY,
            now: at(TODAY, 9, 47),
            zone: &UTC,
            busy,
            agenda,
        }
    }

    #[test]
    fn six_weeks_from_the_monday_with_iso_week_numbers() {
        let view = View::on(TODAY, None);
        let busy = HashSet::from([date(2026, 11, 10), date(2026, 12, 3)]);
        let agenda = Agenda::Events(Vec::new());
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        assert_eq!(laid.weeks.len(), 6);
        // November 2026 starts on a Sunday: the grid on Monday 26 October,
        // ISO week 44.
        let first = &laid.weeks[0];
        assert_eq!(first.days[0].date, date(2026, 10, 26));
        assert_eq!(first.number, 44);
        assert!(!first.days[0].in_month && first.days[6].in_month);
        assert!(first.days[5].weekend && first.days[6].weekend && !first.days[4].weekend);
        assert_eq!(laid.weeks[5].days[6].date, date(2026, 12, 6));
        let today = laid.cell(TODAY).expect("today shows");
        assert!(today.today && today.selected && !today.outlined);
        assert!(laid.cell(date(2026, 11, 10)).is_some_and(|c| c.busy));
        assert!(
            laid.cell(date(2026, 12, 3))
                .is_some_and(|c| c.busy && !c.in_month)
        );
        // 34 × 30 buttons in seven equal columns after the 28 px one; six
        // rows of 32 px, 2 px apart, under the 18 px letters.
        assert_eq!(today.rect.size(), Size::new(34.0, 30.0));
        let column = (ROOM - 28.0) / 7.0;
        let (a, b) = (&laid.weeks[1].days[0], &laid.weeks[1].days[1]);
        assert!((b.rect.x - a.rect.x - column).abs() < 0.01);
        assert!((laid.weeks[2].middle - laid.weeks[1].middle - 34.0).abs() < 0.01);
        assert_eq!(
            laid.weeks[0].middle,
            HEADER_TOP + 28.0 + 10.0 + 18.0 + 2.0 + 16.0
        );
        // Its dot 4 px under its number.
        assert!((today.dot_middle - today.number_middle - (12.5 / 2.0 + 4.0 + 2.0)).abs() < 0.01);
        assert!(laid.today_faint);
        assert_eq!(laid.title.text, "Novembre 2026");
    }

    #[test]
    fn the_day_lists_all_day_events_first_then_by_start_past_ones_greyed() {
        let mut daily = timed("a", "Point d'équipe quotidien", TODAY, (9, 30), (9, 45));
        daily.video = Some("https://meet.google.com/abc-mnop-xyz?authuser=0".to_owned());
        let mut lunch = timed("b", "Déjeuner d'équipe", TODAY, (12, 30), (13, 30));
        lunch.place = Some("Brasserie du Marché".to_owned());
        let away = ev(
            "c",
            "Week-end à la montagne",
            When::AllDay {
                first: TODAY,
                last: date(2026, 11, 5),
            },
        );
        let home = ev(
            "d",
            "Télétravail",
            When::AllDay {
                first: TODAY,
                last: TODAY,
            },
        );
        let agenda = Agenda::Events(vec![daily, home, away, lunch]);
        let busy = HashSet::new();
        let view = View::on(TODAY, None);
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        let rows = laid.rows();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Télétravail",
                "Week-end à la montagne",
                "Point d'équipe quotidien",
                "Déjeuner d'équipe"
            ]
        );
        assert_eq!(
            rows[0].kind,
            RowKind::AllDay {
                span: "toute la journée".to_owned()
            }
        );
        assert_eq!(
            rows[1].kind,
            RowKind::AllDay {
                span: "4 – 5 nov.".to_owned()
            }
        );
        let RowKind::Timed { start, end, meta } = &rows[2].kind else {
            panic!("{:?}", rows[2]);
        };
        assert_eq!((start.as_str(), end.as_str()), ("09:30", "09:45"));
        assert_eq!(
            meta.as_ref().map(|m| (m.video, m.text.as_str())),
            Some((true, "meet.google.com/abc-mnop-xyz"))
        );
        // Over at 09:47: greyed; the lunch not yet, and never an all-day.
        assert!(rows[2].past && !rows[3].past && !rows[0].past);
        // All-day rows 28 px, 4 + 2 px under; timed ones 44 px at least.
        assert_eq!((rows[0].top, rows[0].height), (0.0, 28.0));
        assert_eq!(rows[1].top, 34.0);
        assert_eq!(rows[2].top, 68.0);
        assert!(rows[2].height >= 44.0);
        assert_eq!(rows[3].top, rows[2].top + rows[2].height + 2.0);
        let said = laid.said(&view);
        assert!(
            said.contains("rel=«aujourd'hui» count=«4 événements»"),
            "{said}"
        );
        assert!(
            said.contains("09:30–09:45 «Point d'équipe quotidien» video="),
            "{said}"
        );
        // Another day: nothing greyed.
        let mut tomorrow = view.clone();
        tomorrow.select(date(2026, 11, 5));
        let laid = lay_out(&tomorrow, &context(&busy, &agenda), &mut measure);
        assert!(laid.rows().iter().all(|r| !r.past));
        assert!(!laid.today_faint);
    }

    #[test]
    fn past_236_px_the_list_scrolls_and_reveals_what_holds_the_keyboard() {
        let events: Vec<Ev> = (0..9)
            .map(|i| {
                let mut e = timed(&format!("e{i}"), "Réunion", TODAY, (8 + i, 0), (8 + i, 30));
                e.place = Some("Salle".to_owned());
                e
            })
            .collect();
        let agenda = Agenda::Events(events);
        let busy = HashSet::new();
        let mut view = View::on(TODAY, Some((EventId::new("e8"), false)));
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        let Body::Rows { list, .. } = laid.body else {
            panic!("{:?}", laid.body);
        };
        assert_eq!(list.view.height, LIST_MOST);
        assert!(list.scrolls());
        // The last row, to reveal: the list scrolled to its end.
        assert_eq!(laid.scroll, list.most_scroll());
        assert!(height(&laid) <= 580, "{}", height(&laid));
        view.reveal = false;
        view.scroll = 10_000.0;
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        assert_eq!(laid.scroll, list.most_scroll());
        // Only what shows of the rows takes clicks.
        let events = laid
            .targets()
            .into_iter()
            .filter(|(t, _)| matches!(t, Target::Event(_)))
            .count();
        assert!((4..=6).contains(&events), "{events}");
    }

    #[test]
    fn an_event_without_a_link_unfolds_its_details() {
        let mut lone = timed("x", "Atelier", TODAY, (14, 0), (15, 30));
        lone.place = Some("Salle Jupiter".to_owned());
        lone.description = Some(
            "<p>Un long ordre du jour&nbsp;: un, deux, trois, quatre, cinq, six, sept, huit, \
             neuf, dix, onze, douze, treize, quatorze, quinze, seize, dix-sept, dix-huit, \
             dix-neuf, vingt, et ainsi de suite jusqu'à la fin de la réunion.</p>"
                .to_owned(),
        );
        let mut paged = timed("y", "Revue", TODAY, (16, 0), (17, 0));
        paged.page = Some("https://calendar.example.org/y".to_owned());
        assert_eq!(lone.act(), Act::Unfold);
        assert_eq!(
            paged.act(),
            Act::Page("https://calendar.example.org/y".to_owned())
        );
        let agenda = Agenda::Events(vec![lone, paged]);
        let busy = HashSet::new();
        let mut view = View::on(TODAY, None);
        let folded = lay_out(&view, &context(&busy, &agenda), &mut measure);
        view.unfolded = Some(EventId::new("x"));
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        let row = &laid.rows()[0];
        // Its place under its title already.
        assert_eq!(row.details[0], "14:00 – 15:30");
        assert_eq!(row.details[1], "Travail");
        // The description on four lines at most, the last cut.
        assert_eq!(row.details.len(), 2 + 4, "{:?}", row.details);
        assert!(row.details[2].starts_with("Un long ordre du jour :"));
        assert!(row.details[5].ends_with('…'), "{:?}", row.details);
        assert!(row.height > folded.rows()[0].height);
        assert_eq!(laid.rows()[1].top, row.top + row.height + 2.0);
        // An event with a page never unfolds.
        view.unfolded = Some(EventId::new("y"));
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        assert!(laid.rows().iter().all(|r| r.details.is_empty()));
    }

    #[test]
    fn an_empty_day_no_source_and_a_day_loading_or_not_loaded_say_so() {
        let busy = HashSet::new();
        let view = View::on(date(2026, 11, 13), None);
        for (agenda, says) in [
            (Agenda::Events(Vec::new()), "Rien de prévu ce jour-là."),
            (Agenda::NoSource, "Aucun agenda branché."),
            (Agenda::Loading, "Chargement…"),
            (Agenda::NotLoaded, "Ce jour n'est pas chargé."),
        ] {
            let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
            let Body::Note { text, middle } = &laid.body else {
                panic!("{:?}", laid.body);
            };
            assert_eq!(text, says);
            assert!(laid.day_line.count.is_none());
            let line_end = laid.rule + 1.0 + GAP + DAY_NAME * LINE;
            assert!((middle - (line_end + GAP + 14.0 + NOTE * LINE / 2.0)).abs() < 0.01);
            // Without a source, "Choisir des agendas…" under the note
            // (Day.1's popover), 8 px under it, the 12 px under the button.
            let note_end = middle + NOTE * LINE / 2.0;
            match (&agenda, laid.choose) {
                (Agenda::NoSource, Some(choose)) => {
                    assert!((choose.y - (note_end + 8.0)).abs() < 0.01);
                    assert_eq!(choose.height, CHOOSE);
                    assert!((laid.end - (choose.y + CHOOSE + 12.0)).abs() < 0.01);
                }
                (Agenda::NoSource, None) => panic!("no button without a source"),
                (_, None) => assert!((laid.end - (note_end + 12.0)).abs() < 0.01),
                (_, Some(_)) => panic!("a button with a source"),
            }
        }
    }

    #[test]
    fn the_keyboard_moves_by_days_weeks_and_months() {
        assert_eq!(months_on(date(2026, 1, 31), 1), date(2026, 2, 28));
        assert_eq!(months_on(date(2026, 11, 4), -1), date(2026, 10, 4));
        assert_eq!(week_end(TODAY, true), date(2026, 11, 2));
        assert_eq!(week_end(TODAY, false), date(2026, 11, 8));
        let mut view = View::on(TODAY, None);
        view.focus_on(days_on(TODAY, 28));
        assert_eq!(view.month, date(2026, 12, 1));
        assert_eq!(view.selected, TODAY);
        assert!(view.keyed);
        let busy = HashSet::new();
        let agenda = Agenda::Events(Vec::new());
        let laid = lay_out(&view, &context(&busy, &agenda), &mut measure);
        assert!(laid.cell(date(2026, 12, 2)).is_some_and(|c| c.outlined));
        // The arrows turn the month, the day chosen stays.
        view.turn(1);
        assert_eq!((view.month, view.selected), (date(2027, 1, 1), TODAY));
        assert_eq!(grid_start(date(2027, 1, 1)), date(2026, 12, 28));
        assert_eq!(grid_end(date(2027, 1, 1)), date(2027, 2, 8));
        assert_eq!(
            host_and_path("https://meet.google.com/xvr-pqat-kmd/"),
            "meet.google.com/xvr-pqat-kmd"
        );
        assert_eq!(plain("a<br>b &amp; c"), "a b & c");
    }
}
