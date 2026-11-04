//! Today (bar.md §4 group 4; Day.3, variant A): today's events after the
//! next meeting's card, in plain lines, 14 px under the agents' group, in
//! the full form only.
//!
//! - **What it lists**: today's events that take time and have not ended
//!   ([`crate::meeting::ahead`]), the card's own left out, in order of
//!   start. Nothing after the card: no group.
//! - **Room**: what the spacer would otherwise have, between the groups
//!   above it (the clock, the card, the sign of what is in use, the agents)
//!   and the desktops' group under it, 14 px from each. When some lines do
//!   not fit, the last line that fits says how many more ("+ 5 autres
//!   aujourd'hui"); with no room for even one line, no group: Today gives
//!   way first (§4).
//! - **When it changes**: as the day changes ([`crate::day::on_change`]),
//!   at the clock's minute change ([`minute`]) when a meeting ended, and as
//!   the groups around it move ([`moved`]). Its lines say no minutes:
//!   nothing draws between two changes.
//! - **Clicks**: a line with a video link opens it, else the calendar
//!   (Day.5) on today with its event chosen; "+ N" opens the calendar on
//!   today. Resting
//!   600 ms on a line shows its times and its whole title in a tooltip.
//!
//! Each bar has a surface of its own for it, opened while it shows and
//! removed after ([`draw`]).

pub mod draw;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_day::{Day, model::Colour};
use hocus_link::{EDGE, Form, TODAY_NAMESPACE};
use iced::window;
use jiff::Zoned;

use crate::{
    clock::time::Lang,
    popover::{self, Effect},
};

/// The header's height, a line's, and the room kept between Today and the
/// groups above and under it.
pub const HEADER: f32 = 18.0;
pub const LINE: f32 = 20.0;
pub const GAP: f32 = 14.0;

/// An event of Today's.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub id: String,
    /// `HH:MM`, its start and its end.
    pub start: String,
    pub end: String,
    pub colour: [u8; 3],
    pub title: String,
    pub video: Option<String>,
    pub page: Option<String>,
}

/// A row of the group, under its header.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Event(Line),
    /// "+ N autres aujourd'hui": what did not fit.
    More(usize),
}

impl Row {
    /// What tells it from the others as they move.
    pub fn key(&self) -> &str {
        match self {
            Self::Event(line) => &line.id,
            Self::More(_) => "+",
        }
    }
}

/// `HH:MM` of an instant in `now`'s zone.
fn hhmm(t: jiff::Timestamp, now: &Zoned) -> String {
    t.to_zoned(now.time_zone().clone())
        .strftime("%H:%M")
        .to_string()
}

/// Today's events after the card's, at `now`.
pub fn lines_of(day: &Day, now: &Zoned) -> Vec<Line> {
    let zone = now.time_zone();
    crate::meeting::ahead(day, now)
        .into_iter()
        .skip(1)
        .map(|shown| {
            let event = shown.event;
            let Colour(colour) = shown.calendar.colour;
            Line {
                id: event.id.as_str().to_owned(),
                start: hhmm(event.when.start(zone), now),
                end: hhmm(event.when.end(zone), now),
                colour,
                title: event.title.clone(),
                video: event.video.clone().filter(|v| !v.is_empty()),
                page: event.page.clone().filter(|p| !p.is_empty()),
            }
        })
        .collect()
}

/// The rows `lines` take in `room` px (its header's included): all of them
/// when they fit, else as many as fit less one, then "+ N"; none without
/// a line, or without room for one.
pub fn fit(lines: &[Line], room: f32) -> Vec<Row> {
    let most = ((room - HEADER) / LINE).floor().max(0.0) as usize;
    if lines.is_empty() || most == 0 {
        return Vec::new();
    }
    if lines.len() <= most {
        return lines.iter().cloned().map(Row::Event).collect();
    }
    let shown = most - 1;
    let mut rows: Vec<Row> = lines[..shown].iter().cloned().map(Row::Event).collect();
    rows.push(Row::More(lines.len() - shown));
    rows
}

/// The group's height with `rows`.
pub fn height(rows: &[Row]) -> f32 {
    if rows.is_empty() {
        0.0
    } else {
        HEADER + LINE * rows.len() as f32
    }
}

// --- What shows, and where -------------------------------------------------------

/// A surface's size and its layer-shell margins, as last asked for.
type Asked = ((u32, u32), (i32, i32, i32, i32));

/// Today's surface on a bar.
#[derive(Debug, Clone, Default)]
struct Place {
    id: Option<window::Id>,
    form: Option<Form>,
    switching: bool,
    rows: Vec<Row>,
    top: f32,
    asked: Option<Asked>,
}

#[derive(Debug, Default)]
struct State {
    lines: Vec<Line>,
    places: HashMap<String, Place>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// The rows bar `bar` shows now, and its group's top under the bar's.
pub fn shown(bar: &str) -> (Vec<Row>, f32) {
    state(|s| {
        s.places
            .get(bar)
            .map(|p| (p.rows.clone(), p.top))
            .unwrap_or_default()
    })
}

/// The group's top under the bar's: 14 px under the agents' group, or
/// where it would be when it does not show.
fn top() -> f32 {
    let agents = crate::agents::draw::group_height_shown();
    crate::agents::draw::full_top() + if agents > 0.0 { agents + GAP } else { 0.0 }
}

/// Bar `bar`'s room for Today from `top`: down to 14 px over its desktops'
/// group.
fn room(bar: &str, top: f32) -> f32 {
    let height = popover::with(|c| c.bar_height(bar));
    let under = crate::desktops::draw::top_above_bottom(bar).unwrap_or(height);
    // The to-do list under Today keeps its own room (Day.4).
    height - under - GAP - top - crate::tasks::draw::room_taken(bar)
}

/// Starts following the day: woken by it on its thread.
pub fn start() {
    crate::day::on_change(|| {
        refresh();
        true
    });
}

/// The clock's minute changed: the events again (a meeting may have
/// ended).
pub fn minute() {
    refresh();
}

/// The groups around it moved, or a bar changed height: each bar's rows
/// again.
pub fn moved() {
    place_all(false);
}

/// Today's events as the day and the clock say now: if they changed, every
/// bar's rows again.
pub fn refresh() {
    let now = crate::notifications::wall();
    let lines = lines_of(&crate::day::snapshot(), &now);
    let changed = state(|s| {
        if s.lines == lines {
            return false;
        }
        s.lines = lines;
        true
    });
    if changed {
        place_all(true);
    }
}

/// Every bar's rows for the lines and the room now: its surface comes,
/// goes, moves or draws again as they change.
fn place_all(lines_changed: bool) {
    let bars: Vec<String> = state(|s| s.places.keys().cloned().collect());
    // Measured outside the lock: the agents', the desktops' and the
    // popovers' own locks.
    let tops: Vec<(String, f32, f32)> = bars
        .into_iter()
        .map(|bar| {
            let top = top();
            let room = room(&bar, top);
            (bar, top, room)
        })
        .collect();
    let mut effects = Vec::new();
    let mut placed: Vec<(String, Option<window::Id>)> = Vec::new();
    let mut redraw = false;
    let words = Words::new(Lang::of_messages());
    state(|s| {
        for (bar, top, room) in tops {
            let Some(place) = s.places.get_mut(&bar) else {
                continue;
            };
            let full = place.form == Some(Form::Full) || place.switching;
            let rows = if full {
                fit(&s.lines, room)
            } else {
                Vec::new()
            };
            if rows != place.rows {
                redraw = true;
                tracing::debug!(bar, shows = %words.said(&rows), room, "today's rows");
            }
            place.rows = rows;
            place.top = top;
            effects.extend(place.fit(&bar, &mut placed));
        }
    });
    if !placed.is_empty() {
        popover::with(|c| {
            for (bar, id) in placed {
                c.set_today(&bar, id);
            }
        });
    }
    if redraw || lines_changed {
        effects.push(Effect::Wake(TODAY_NAMESPACE));
    }
    if !effects.is_empty() {
        popover::send(effects);
    }
    // The to-do list, 14 px under Today, follows it (Day.4).
    crate::tasks::draw::moved();
}

impl Place {
    /// Its surface for its rows: opened, resized, moved or removed.
    fn fit(&mut self, bar: &str, placed: &mut Vec<(String, Option<window::Id>)>) -> Vec<Effect> {
        let shows = !self.rows.is_empty();
        let asked = draw::placement(self.top, height(&self.rows));
        let margin = (EDGE + asked.1.round() as i32, EDGE + 1, 0, 0);
        let asked = (asked.0, margin);
        match (self.id, shows) {
            (None, true) => {
                let id = window::Id::unique();
                self.id = Some(id);
                self.asked = Some(asked);
                placed.push((bar.to_owned(), Some(id)));
                vec![Effect::Layer {
                    id,
                    bar: bar.to_owned(),
                    namespace: TODAY_NAMESPACE,
                    size: asked.0,
                    margin: asked.1,
                }]
            }
            (Some(id), false) => {
                self.id = None;
                self.asked = None;
                placed.push((bar.to_owned(), None));
                vec![Effect::Remove(id)]
            }
            (Some(id), true) if self.asked != Some(asked) => {
                self.asked = Some(asked);
                vec![Effect::Resize {
                    id,
                    size: asked.0,
                    margin: Some(asked.1),
                }]
            }
            _ => Vec::new(),
        }
    }
}

/// Bar `bar` opened in `form`: its rows, its surface if they show (to
/// open after the bar's own).
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let top = top();
    let room = room(bar, top);
    let mut placed = Vec::new();
    let effects = state(|s| {
        let rows = if form == Form::Full {
            fit(&s.lines, room)
        } else {
            Vec::new()
        };
        let place = s.places.entry(bar.to_owned()).or_default();
        place.form = Some(form);
        place.rows = rows;
        place.top = top;
        place.fit(bar, &mut placed)
    });
    if !placed.is_empty() {
        popover::with(|c| {
            for (bar, id) in placed {
                c.set_today(&bar, id);
            }
        });
    }
    effects
}

/// Bar `bar` in `form`, switching or not.
pub fn form(bar: &str, form: Form, switching: bool) {
    let changed = state(|s| {
        let Some(place) = s.places.get_mut(bar) else {
            return false;
        };
        let changed = place.form != Some(form) || place.switching != switching;
        place.form = Some(form);
        place.switching = switching;
        changed
    });
    if changed {
        place_all(false);
    }
}

/// Bar `bar` went: its surface, to remove.
pub fn forget(bar: &str) -> Option<window::Id> {
    state(|s| s.places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose Today's surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    state(|s| {
        s.places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

/// Row `index` of bar `bar`, clicked (or `Enter` on it): an event's video
/// call, else the calendar (Day.5) on today, the event chosen; "+ N", the
/// calendar on today.
pub fn open(bar: &str, index: usize) -> Option<Effect> {
    let row = state(|s| s.places.get(bar).and_then(|p| p.rows.get(index).cloned()))?;
    match row {
        Row::Event(line) => match &line.video {
            Some(video) => {
                tracing::info!(id = line.id, "a line of today opens its video call");
                crate::launcher::browse(video)
            }
            None => {
                tracing::info!(id = line.id, "a line of today opens the calendar");
                Some(Effect::OpenCalendar {
                    bar: bar.to_owned(),
                    day: crate::notifications::wall().date(),
                    event: Some(hocus_day::model::EventId::new(line.id)),
                })
            }
        },
        Row::More(_) => open_more(bar),
    }
}

/// "+ N autres aujourd'hui", on `bar`: the calendar (Day.5) on today.
pub fn open_more(bar: &str) -> Option<Effect> {
    tracing::info!("today's \"+ N\" opens the calendar");
    Some(Effect::OpenCalendar {
        bar: bar.to_owned(),
        day: crate::notifications::wall().date(),
        event: None,
    })
}

// --- Words ----------------------------------------------------------------------

/// Today's words, in the shell's language.
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

    pub fn header(self) -> &'static str {
        if self.fr { "AUJOURD'HUI" } else { "TODAY" }
    }

    /// "+ 5 autres aujourd'hui", "+ 1 autre aujourd'hui".
    pub fn more(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "+ 1 autre aujourd'hui".to_owned(),
            (true, n) => format!("+ {n} autres aujourd'hui"),
            (false, n) => format!("+ {n} more today"),
        }
    }

    /// A line's name for screen readers: "15:00, Entretien, visio".
    pub fn name(self, line: &Line) -> String {
        let mut name = format!("{}, {}", line.start, line.title);
        if line.video.is_some() {
            name.push_str(if self.fr { ", visio" } else { ", video call" });
        }
        name
    }

    /// The tooltip's two parts: "15:00–15:45", and the whole title.
    pub fn tip(line: &Line) -> (String, String) {
        (format!("{}–{}", line.start, line.end), line.title.clone())
    }

    /// What the rows say, for the log (and tests).
    pub fn said(self, rows: &[Row]) -> String {
        if rows.is_empty() {
            return "nothing".to_owned();
        }
        let rows: Vec<String> = rows
            .iter()
            .map(|row| match row {
                Row::Event(line) => format!(
                    "{} «{}» #{:02X}{:02X}{:02X}",
                    line.start, line.title, line.colour[0], line.colour[1], line.colour[2]
                ),
                Row::More(n) => self.more(*n),
            })
            .collect();
        format!("{} {}", self.header(), rows.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, start: &str) -> Line {
        Line {
            id: id.to_owned(),
            start: start.to_owned(),
            end: "23:00".to_owned(),
            colour: [0xC7, 0x92, 0xEA],
            title: format!("Événement {id}"),
            video: None,
            page: None,
        }
    }

    fn lines(n: usize) -> Vec<Line> {
        (0..n).map(|i| line(&format!("e{i}"), "15:00")).collect()
    }

    #[test]
    fn what_fits_and_the_last_line_counts_the_rest() {
        let room = |rows: usize| HEADER + LINE * rows as f32;
        // Nine events, room for five lines: four, and "+ 5".
        let rows = fit(&lines(9), room(5) + 7.0);
        assert_eq!(rows.len(), 5);
        assert!(rows[..4].iter().all(|r| matches!(r, Row::Event(_))));
        assert_eq!(rows[4], Row::More(5));
        // All fit: all, no "+ N".
        assert_eq!(fit(&lines(3), room(5)).len(), 3);
        assert_eq!(fit(&lines(5), room(5)).len(), 5);
        assert!(!fit(&lines(5), room(5)).contains(&Row::More(1)));
        // One more than fits: "+ 2" in the last line's place.
        assert_eq!(fit(&lines(6), room(5))[4], Row::More(2));
        // No room for even one line: no group; nothing to list: none.
        assert!(fit(&lines(3), room(1) - 1.0).is_empty());
        assert!(fit(&[], room(5)).is_empty());
        // The group's height: 18 px and 20 a line.
        assert_eq!(height(&fit(&lines(3), room(5))), 78.0);
        assert_eq!(height(&[]), 0.0);
    }

    #[test]
    fn todays_words_in_both_languages() {
        let (fr, en) = (Words::new(Lang::French), Words::new(Lang::English));
        assert_eq!(fr.header(), "AUJOURD'HUI");
        assert_eq!(en.header(), "TODAY");
        assert_eq!(fr.more(5), "+ 5 autres aujourd'hui");
        assert_eq!(fr.more(1), "+ 1 autre aujourd'hui");
        assert_eq!(en.more(5), "+ 5 more today");
        assert_eq!(en.more(1), "+ 1 more today");
        let mut interview = line("i", "15:00");
        interview.end = "15:45".to_owned();
        interview.title = "Entretien — poste back-end".to_owned();
        assert_eq!(fr.name(&interview), "15:00, Entretien — poste back-end");
        interview.video = Some("https://meet.example.org/x".to_owned());
        assert_eq!(
            fr.name(&interview),
            "15:00, Entretien — poste back-end, visio"
        );
        assert_eq!(
            en.name(&interview),
            "15:00, Entretien — poste back-end, video call"
        );
        assert_eq!(
            Words::tip(&interview),
            (
                "15:00–15:45".to_owned(),
                "Entretien — poste back-end".to_owned()
            )
        );
    }
}
