//! Today (bar.md §4 group 4; Day.3, variant A): today's events after the
//! next meeting's card, in plain lines, first of the day's groups under
//! the card and the sign of what is in use (`crate::column`), in the full
//! form only; the to-do list and the agents' group under it.
//!
//! - **What it lists**: today's events that take time and have not ended
//!   ([`crate::meeting::today`]), of every shown calendar, featured or not
//!   (`[[day.calendars]] featured`), the card's own left out
//!   ([`crate::meeting::ahead`]'s first), in order of start. Nothing but
//!   the card's: no group.
//! - **Room**: what the spacer would otherwise have: the column's down to
//!   14 px over the desktops' group, less what the to-do list and the
//!   agents' group under it take ([`crate::column::room`]). When some lines
//!   do not fit, the last line that fits says how many more ("+ 5 autres
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
//! - **The weather** (#814, variant C): with `[weather]` on and a place
//!   set, the header carries the weather now on its right ("☁ 9°"; from
//!   18:00 tomorrow's, "demain 11°"), the rain coming later today is a line
//!   among the events at its start ("14:15 Pluie → 15 h 45", "Il pleut →
//!   15 h 45" while it rains), and tomorrow closes the list ("Demain ⛅
//!   3° / 11°"). The group then shows even with no event left. A click on
//!   the weather opens its popover ([`crate::popover::weather`]). In the
//!   mini form, the same surface holds the weather's item under the clock
//!   (its icon, its temperature, three drops when rain comes within 3 h),
//!   first of the mini column.
//!
//! Each bar has a surface of its own for it, opened while it shows and
//! removed after ([`draw`]).

pub mod draw;

use hocus_words::shell::today as txt;
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
    weather,
};

/// The header's height, a line's.
pub const HEADER: f32 = 18.0;
pub const LINE: f32 = 20.0;
/// The mini form's weather item: 5 px in, the icon (22 px), 1 px, the
/// temperature's line, 5 px; "dem." and the drops below when they show.
pub const MINI_BASE: f32 = 51.0;
pub const MINI_TOMORROW: f32 = 13.0;
pub const MINI_DROPS: f32 = 10.0;

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
    /// Its start, seconds since 1970: the rain's line goes among the
    /// events by it.
    pub at: i64,
}

/// A row of the group, under its header.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Event(Line),
    /// "+ N autres aujourd'hui": what did not fit.
    More(usize),
    /// The rain coming later today (#814).
    Rain(weather::RainLine),
    /// Tomorrow's weather, last.
    Tomorrow(weather::TomorrowLine),
}

impl Row {
    /// What tells it from the others as they move.
    pub fn key(&self) -> &str {
        match self {
            Self::Event(line) => &line.id,
            Self::More(_) => "+",
            Self::Rain(_) => "weather:rain",
            Self::Tomorrow(_) => "weather:tomorrow",
        }
    }
}

/// An instant's time in `now`'s zone, as the desktop writes it.
fn hhmm(t: jiff::Timestamp, now: &Zoned) -> String {
    crate::meeting::hhmm(t, now)
}

/// Today's events but the card's, at `now`.
pub fn lines_of(day: &Day, now: &Zoned) -> Vec<Line> {
    let zone = now.time_zone();
    let card = crate::meeting::ahead(day, now).first().map(|s| s.event);
    crate::meeting::today(day, now)
        .into_iter()
        .filter(|shown| card.is_none_or(|card| !std::ptr::eq(card, shown.event)))
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
                at: event.when.start(zone).as_second(),
            }
        })
        .collect()
}

/// The rows `lines` and the weather's (`rain`, `tomorrow`) take in `room`
/// px (its header's included): all of them when they fit, the rain among
/// the events by its start and tomorrow last; else the weather's and as
/// many events as fit less one, then "+ N" counting the others; with less
/// room still, the weather's lines go too (the rain first). None without
/// a line, or without room for one.
pub fn fit(
    lines: &[Line],
    rain: Option<&weather::RainLine>,
    tomorrow: Option<&weather::TomorrowLine>,
    room: f32,
) -> Vec<Row> {
    let most = ((room - HEADER) / LINE).floor().max(0.0) as usize;
    let weather = usize::from(rain.is_some()) + usize::from(tomorrow.is_some());
    if (lines.is_empty() && weather == 0) || most == 0 {
        return Vec::new();
    }
    let (shown, more) = if lines.len() + weather <= most {
        (lines.len(), 0)
    } else {
        let shown = most.saturating_sub(weather + 1);
        (shown, lines.len() - shown)
    };
    let mut rows: Vec<Row> = lines
        .get(..shown)
        .unwrap_or_default()
        .iter()
        .cloned()
        .map(Row::Event)
        .collect();
    if let Some(rain) = rain {
        // Before the first event that starts after it; while it rains,
        // first.
        let at = rain.at.map_or(0, |at| {
            rows.iter()
                .position(|r| matches!(r, Row::Event(l) if l.at > at))
                .unwrap_or(rows.len())
        });
        rows.insert(at, Row::Rain(rain.clone()));
    }
    if more > 0 {
        rows.push(Row::More(more));
    }
    if let Some(tomorrow) = tomorrow {
        rows.push(Row::Tomorrow(tomorrow.clone()));
    }
    // Too little room even for the weather's: the rain goes, then "+ N",
    // then tomorrow.
    while rows.len() > most {
        let gone = rows
            .iter()
            .position(|r| matches!(r, Row::Rain(_)))
            .or_else(|| rows.iter().position(|r| matches!(r, Row::More(_))))
            .unwrap_or(0);
        rows.remove(gone);
    }
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
    /// The mini form's weather item's top.
    mini_top: f32,
    asked: Option<Asked>,
}

#[derive(Debug, Default)]
struct State {
    lines: Vec<Line>,
    /// What Today and the mini form show of the weather (#814).
    weather: Option<weather::Today>,
    mini: Option<weather::Mini>,
    places: HashMap<String, Place>,
}

/// What bar `bar`'s Today shows now.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    pub rows: Vec<Row>,
    /// The group's top under the bar's, and the mini item's.
    pub top: f32,
    pub mini_top: f32,
    /// The form it shows in.
    pub form: Option<Form>,
    /// The header's weather, while the group shows.
    pub head: Option<weather::Head>,
    /// The mini form's weather item.
    pub mini: Option<weather::Mini>,
}

/// The mini form's weather item's height (none: it does not show).
pub fn mini_height_of(mini: Option<&weather::Mini>) -> f32 {
    mini.map_or(0.0, |m| {
        MINI_BASE
            + if m.tomorrow.is_some() {
                MINI_TOMORROW
            } else {
                0.0
            }
            + if m.drops { MINI_DROPS } else { 0.0 }
    })
}

/// What the mini form's weather item takes of the mini column, the same
/// on every bar (0 when it does not show).
pub fn mini_height() -> f32 {
    state(|s| mini_height_of(s.mini.as_ref()))
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// What bar `bar` shows now.
pub fn shown(bar: &str) -> Shown {
    state(|s| {
        let Some(p) = s.places.get(bar) else {
            return Shown::default();
        };
        Shown {
            rows: p.rows.clone(),
            top: p.top,
            mini_top: p.mini_top,
            form: p.form,
            head: s
                .weather
                .as_ref()
                .filter(|_| !p.rows.is_empty())
                .map(|w| w.head.clone()),
            mini: s.mini.clone().filter(|_| p.form == Some(Form::Mini)),
        }
    })
}

/// Bar `bar`'s group's top under the bar's, in its column.
fn top(bar: &str) -> f32 {
    crate::column::top(bar, Form::Full, crate::column::Part::Today)
}

/// Bar `bar`'s mini weather item's top under the bar's.
fn mini_top(bar: &str) -> f32 {
    crate::column::top(bar, Form::Mini, crate::column::Part::Today)
}

/// Bar `bar`'s room for Today: what the to-do list and the agents' group
/// under it leave, down to 14 px over its desktops' group.
fn room(bar: &str) -> f32 {
    crate::column::room(bar, crate::column::Part::Today)
}

/// Starts following the day: woken by it on its thread.
pub fn start() {
    crate::day::on_change(|| {
        refresh();
        true
    });
}

/// The clock's minute changed: the events again (a meeting may have
/// ended), the weather's lines (the rain started, the evening came).
pub fn minute() {
    refresh();
}

/// The groups around it moved, or a bar changed height: each bar's rows
/// again (the column's [`crate::column::moved`] moves those under it).
pub fn moved() {
    place_all();
}

/// Today's events as the day and the clock say now: if they changed, every
/// bar's rows again.
pub fn refresh() {
    let now = crate::notifications::wall();
    // Hidden (`[panel] today`, 6.4): no lines, as with nothing after the
    // meeting.
    let lines = if crate::column::groups().today {
        lines_of(&crate::day::snapshot(), &now)
    } else {
        Vec::new()
    };
    let lang = Lang::of_messages();
    let today = weather::today(&now, lang);
    let mini = weather::mini(&now, lang);
    let (changed, mini_moved) = state(|s| {
        if s.lines == lines && s.weather == today && s.mini == mini {
            return (false, false);
        }
        let moved = mini_height_of(s.mini.as_ref()) != mini_height_of(mini.as_ref());
        s.lines = lines;
        s.weather = today;
        s.mini = mini;
        (true, moved)
    });
    if mini_moved {
        // The mini column's items under it, the meeting's mark and the
        // sign of what is in use among them.
        crate::meeting::moved();
        crate::in_use::moved();
    }
    if changed {
        crate::column::moved();
        popover::send(vec![Effect::Wake(TODAY_NAMESPACE)]);
    }
}

/// Every bar's rows for the lines and the room now: its surface comes,
/// goes, moves or draws again as they change.
fn place_all() {
    let bars: Vec<String> = state(|s| s.places.keys().cloned().collect());
    // Measured outside the lock: the agents', the desktops' and the
    // popovers' own locks.
    let tops: Vec<(String, f32, f32, f32)> = bars
        .into_iter()
        .map(|bar| {
            let (top, mini, room) = (top(&bar), mini_top(&bar), room(&bar));
            (bar, top, mini, room)
        })
        .collect();
    let mut effects = Vec::new();
    let mut placed: Vec<(String, Option<window::Id>)> = Vec::new();
    let mut redraw = false;
    let words = Words::new(Lang::of_messages());
    state(|s| {
        let mini = mini_height_of(s.mini.as_ref());
        for (bar, top, mini_top, room) in tops {
            let Some(place) = s.places.get_mut(&bar) else {
                continue;
            };
            let full = place.form == Some(Form::Full) || place.switching;
            let rows = if full {
                rows_of(&s.lines, s.weather.as_ref(), room)
            } else {
                Vec::new()
            };
            if rows != place.rows {
                redraw = true;
                tracing::debug!(bar, shows = %words.said(&rows), room, "today's rows");
            }
            place.rows = rows;
            place.top = top;
            place.mini_top = mini_top;
            effects.extend(place.fit(&bar, mini, &mut placed));
        }
    });
    if !placed.is_empty() {
        popover::with(|c| {
            for (bar, id) in placed {
                c.set_today(&bar, id);
            }
        });
    }
    if redraw {
        effects.push(Effect::Wake(TODAY_NAMESPACE));
    }
    if !effects.is_empty() {
        popover::send(effects);
    }
}

/// The rows of `lines` and of the weather's lines in `room`.
fn rows_of(lines: &[Line], weather: Option<&weather::Today>, room: f32) -> Vec<Row> {
    fit(
        lines,
        weather.and_then(|w| w.rain.as_ref()),
        weather.and_then(|w| w.tomorrow.as_ref()),
        room,
    )
}

impl Place {
    /// Its surface for its rows (the mini form's weather item `mini` px
    /// tall): opened, resized, moved or removed.
    fn fit(
        &mut self,
        bar: &str,
        mini: f32,
        placed: &mut Vec<(String, Option<window::Id>)>,
    ) -> Vec<Effect> {
        let in_mini = self.form == Some(Form::Mini) && !self.switching;
        let (shows, asked) = if in_mini {
            (mini > 0.0, draw::mini_placement(self.mini_top, mini))
        } else {
            (
                !self.rows.is_empty(),
                draw::placement(self.top, height(&self.rows)),
            )
        };
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
    let (top, mini_top, room) = (top(bar), mini_top(bar), room(bar));
    let mut placed = Vec::new();
    let effects = state(|s| {
        let rows = if form == Form::Full {
            rows_of(&s.lines, s.weather.as_ref(), room)
        } else {
            Vec::new()
        };
        let mini = mini_height_of(s.mini.as_ref());
        let place = s.places.entry(bar.to_owned()).or_default();
        place.form = Some(form);
        place.rows = rows;
        place.top = top;
        place.mini_top = mini_top;
        place.fit(bar, mini, &mut placed)
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

/// Bar `bar` in `form`, switching or not: its rows follow as the column
/// moves next (`crate::column::moved`).
pub fn form(bar: &str, form: Form, switching: bool) {
    state(|s| {
        if let Some(place) = s.places.get_mut(bar) {
            place.form = Some(form);
            place.switching = switching;
        }
    });
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
        // Their items open the weather's popover.
        Row::Rain(_) | Row::Tomorrow(_) => None,
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
    pub(crate) lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn header(self) -> &'static str {
        txt::TODAY.get(self.lang)
    }

    /// "+ 5 autres aujourd'hui", "+ 1 autre aujourd'hui".
    pub fn more(self, n: usize) -> String {
        match ((self.lang == Lang::French), n) {
            (true, 1) => "+ 1 autre aujourd'hui".to_owned(),
            (true, n) => format!("+ {n} autres aujourd'hui"),
            (false, n) => format!("+ {n} more today"),
        }
    }

    /// A line's name for screen readers: "15:00, Entretien, visio".
    pub fn name(self, line: &Line) -> String {
        let mut name = format!("{}, {}", line.start, line.title);
        if line.video.is_some() {
            name.push_str(txt::VIDEO_CALL.get(self.lang));
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
                Row::Rain(rain) => format!("{} {}", rain.start, rain.said).trim().to_owned(),
                Row::Tomorrow(t) => {
                    format!("{} {}", weather::Words::new(self.lang).tomorrow(), t.said)
                }
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
            at: 15 * 3600,
        }
    }

    fn lines(n: usize) -> Vec<Line> {
        (0..n).map(|i| line(&format!("e{i}"), "15:00")).collect()
    }

    #[test]
    fn what_fits_and_the_last_line_counts_the_rest() {
        let room = |rows: usize| HEADER + LINE * rows as f32;
        let fit = |lines: &[Line], room: f32| super::fit(lines, None, None, room);
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
    fn the_rain_goes_among_the_events_and_tomorrow_last() {
        use hocus_weather::Icon;
        let room = |rows: usize| HEADER + LINE * rows as f32;
        let mut events = lines(3);
        for (event, hour) in events.iter_mut().zip([13, 16, 18]) {
            event.at = hour * 3600;
        }
        let rain = weather::RainLine {
            at: Some(14 * 3600 + 15 * 60),
            start: "14:15".to_owned(),
            said: "Pluie → 15 h 45".to_owned(),
            icon: Icon::Rain,
        };
        let tomorrow = weather::TomorrowLine {
            icon: Icon::PartlyCloudy,
            said: "3° / 11°".to_owned(),
        };
        let keys = |rows: &[Row]| rows.iter().map(|r| r.key().to_owned()).collect::<Vec<_>>();
        let rows = fit(&events, Some(&rain), Some(&tomorrow), room(8));
        assert_eq!(
            keys(&rows),
            ["e0", "weather:rain", "e1", "e2", "weather:tomorrow"]
        );
        // While it rains: first.
        let now = weather::RainLine { at: None, ..rain.clone() };
        assert_eq!(
            keys(&fit(&events, Some(&now), Some(&tomorrow), room(8)))[0],
            "weather:rain"
        );
        // No event left: the weather alone, the group still shows.
        assert_eq!(
            keys(&fit(&[], Some(&rain), Some(&tomorrow), room(8))),
            ["weather:rain", "weather:tomorrow"]
        );
        // Short of room: the weather's lines stay, the events go into
        // "+ N" before them.
        let rows = fit(&events, Some(&rain), Some(&tomorrow), room(4));
        assert_eq!(keys(&rows), ["e0", "weather:rain", "+", "weather:tomorrow"]);
        assert!(rows.contains(&Row::More(2)));
        // Shorter still: the rain goes, then "+ N".
        let rows = fit(&events, Some(&rain), Some(&tomorrow), room(2));
        assert_eq!(keys(&rows), ["+", "weather:tomorrow"]);
        let rows = fit(&events, Some(&rain), Some(&tomorrow), room(1));
        assert_eq!(keys(&rows), ["weather:tomorrow"]);
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
