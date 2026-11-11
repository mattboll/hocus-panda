//! The windows in the bar (#604, variant B as the maintainer changed it):
//! the windows of the bar's screen's current desktop, over its desktops'
//! tiles, in the room the column leaves free.
//!
//! - **What it lists**: the current desktop's windows, minimised ones too
//!   (greyed, as their desktop's list has them, 3.9c), the focused one
//!   first and then by most recent focus (the compositor's `recent`
//!   line), at most 3; past 3, a 20 px "+ N autres" opening the current
//!   desktop's list (3.9b) until the overview comes (8.5). No window: no
//!   group.
//! - **A line**: 30 px, its application's icon (18 px), its title (13 px,
//!   one line, cut with an ellipsis) and a ✕ on the right; the focused
//!   window's line lit `rgba(255,255,255,0.06)`. A click on a line focuses
//!   and raises its window (back from minimised); the ✕ asks it to close,
//!   as `Super+Q` does: the application decides.
//! - **Room**: 14 px over the desktops' tiles, in what the column leaves
//!   free (`crate::column`): the to-do list and the agents' group never
//!   give way to it, Today ends over it. As tall as its lines, which a
//!   change of focus only reorders: nothing else moves then. Its lines
//!   fold into "+ N" when the room is short; without room for one line,
//!   no group.
//! - **The mini form**: the first line's window's icon in a 44 × 34
//!   button 14 px over the desktops' dots; a 150 ms hover or a click opens
//!   the same list in a popover ([`crate::popover::windows`]).
//! - `[panel] windows = false` (the compositor's `bar-windows hidden`)
//!   hides it in both forms.
//!
//! Each bar has a surface of its own for it, opened with the bar and kept
//! as long as it, drawing nothing and taking no pointer while nothing shows
//! ([`draw`]): a surface made later would take the keyboard's keys from
//! one holding it (#617). It draws only when what it lists changes
//! (a window opening, closing, retitled, focused), never at rest.

pub mod draw;

use hocus_words::shell::windows as txt;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{
    DeskWindow, Desktops, EDGE, Form, Look, POPOVER_NAMESPACE, Recent, Titles, WINDOWS_NAMESPACE,
};
use iced::window;

use crate::{
    clock::time::Lang,
    column::{self, GAP, Part},
    popover::{self, Effect, Item},
};

/// The lines shown at most.
pub const MOST: usize = 3;
/// A window's line, the "+ N" line's.
pub const LINE: f32 = 30.0;
pub const MORE: f32 = 20.0;
/// The mini form's item's height.
pub const ITEM: f32 = 34.0;

/// A window as the bar lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    /// The compositor's id for it.
    pub id: u64,
    /// Its application's id, for its icon (empty: none).
    pub app: String,
    /// Its title; its application's id until it says one.
    pub title: String,
    /// It holds the focus: its line is lit.
    pub focused: bool,
    /// Minimised: greyed, its icon at half.
    pub minimized: bool,
    /// Kept above the others, on all desktops (#723): a mark each after
    /// its title.
    pub above: bool,
    pub everywhere: bool,
}

/// A row of the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Window(Line),
    /// "+ N autres": the windows that did not fit.
    More(usize),
}

/// The windows of `desktops`' current desktop, the focused one first, then
/// by `recent`'s order (the most recently focused first), then any it
/// does not name, front to back; with their titles.
pub fn lines_of(
    desktops: &Desktops,
    titles: Option<&Titles>,
    recent: Option<&Recent>,
) -> Vec<Line> {
    let rank = |w: &DeskWindow| {
        recent
            .and_then(|r| r.ids.iter().position(|id| *id == w.id))
            .unwrap_or(usize::MAX)
    };
    let mut windows: Vec<&DeskWindow> = desktops
        .windows
        .iter()
        .rev()
        .filter(|w| w.desktop == desktops.current)
        .collect();
    // Stable: those `recent` does not name stay front to back.
    windows.sort_by_key(|w| (w.look != Look::Focused, rank(w)));
    windows
        .into_iter()
        .map(|w| Line {
            id: w.id,
            app: w.app.clone(),
            title: titles
                .and_then(|t| t.titles.iter().find(|(id, _)| *id == w.id))
                .map(|(_, title)| title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| w.app.clone()),
            focused: w.look == Look::Focused,
            minimized: w.look == Look::Minimized,
            above: false,
            everywhere: false,
        })
        .collect()
}

/// The windows kept above the others and those on all desktops, as the
/// compositor last said (#723).
static STATES: Mutex<Option<hocus_link::WindowStates>> = Mutex::new(None);

/// The windows' states changed: their lines show their marks.
pub fn set_states(states: hocus_link::WindowStates) {
    *STATES.lock().unwrap_or_else(PoisonError::into_inner) = Some(states);
    changed_now();
    // A desktop's list or the mini form's popover shows them too.
    let open = popover::with(|c| {
        matches!(
            c.open_on(),
            Some((_, Item::Windows | Item::WindowsMore | Item::Desktop { .. }))
        )
    });
    if open {
        popover::send(vec![Effect::Wake(POPOVER_NAMESPACE)]);
    }
}

/// Whether window `id` is kept above the others, and on all desktops.
pub fn marks(id: u64) -> (bool, bool) {
    STATES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map_or((false, false), |s| {
            (s.above.contains(&id), s.all_desktops.contains(&id))
        })
}

/// Bar `bar`'s screen's current desktop's windows, as [`lines_of`] lists
/// them.
pub fn lines(bar: &str) -> Vec<Line> {
    let Some(desktops) = crate::desktops::snapshot(bar) else {
        return Vec::new();
    };
    let titles = crate::desktops::titles(bar);
    let recent = crate::desktops::recent(bar);
    let mut lines = lines_of(&desktops, titles.as_ref(), recent.as_ref());
    for line in &mut lines {
        (line.above, line.everywhere) = marks(line.id);
    }
    lines
}

/// The rows `lines` take in `room` px: all of them when they are 3 at
/// most and fit; else as many as fit with "+ N" under them, 3 at most;
/// none without a line, or without room for one line and "+ N".
pub fn fit(lines: &[Line], room: f32) -> Vec<Row> {
    let all = lines.len();
    if all == 0 {
        return Vec::new();
    }
    if all <= MOST && all as f32 * LINE <= room {
        return lines.iter().cloned().map(Row::Window).collect();
    }
    let most = (((room - MORE) / LINE).floor().max(0.0) as usize).min(MOST);
    if most == 0 {
        return Vec::new();
    }
    let mut rows: Vec<Row> = lines[..most].iter().cloned().map(Row::Window).collect();
    rows.push(Row::More(all - most));
    rows
}

/// The group's height with `rows`.
pub fn rows_height(rows: &[Row]) -> f32 {
    rows.iter()
        .map(|r| match r {
            Row::Window(_) => LINE,
            Row::More(_) => MORE,
        })
        .sum()
}

// --- What shows, and where -------------------------------------------------------

/// A surface's size and its layer-shell margins, as last asked for.
type Asked = ((u32, u32), (i32, i32, i32, i32));

/// The windows' surface on a bar.
#[derive(Debug, Clone, Default)]
struct Place {
    id: Option<window::Id>,
    form: Option<Form>,
    switching: bool,
    /// The current desktop's windows, as last listed.
    lines: Vec<Line>,
    /// The full form's rows.
    rows: Vec<Row>,
    /// The mini form's item's window: the first line's.
    item: Option<Line>,
    /// The group's top and the item's, under the bar's top.
    full_top: f32,
    mini_top: f32,
    asked: Option<Asked>,
    /// Whether it takes the pointer, as last asked: only while it shows.
    input: Option<bool>,
}

#[derive(Debug, Default)]
struct State {
    /// `[panel] windows = false`.
    hidden: bool,
    places: HashMap<String, Place>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// What bar `bar` shows now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub rows: Vec<Row>,
    pub item: Option<Line>,
    /// The windows of the current desktop, listed or not.
    pub count: usize,
    /// The group's top, the item's, and the surface's, under the bar's
    /// top.
    pub full_top: f32,
    pub mini_top: f32,
    pub surface_top: f32,
    /// The surface's bottom under the bar's top, as last asked for.
    pub surface_bottom: f32,
}

/// What bar `bar` shows now, and where.
pub fn shown(bar: &str) -> Shown {
    state(|s| {
        s.places
            .get(bar)
            .map(|p| Shown {
                rows: p.rows.clone(),
                item: p.item.clone(),
                count: p.lines.len(),
                full_top: p.full_top,
                mini_top: p.mini_top,
                surface_top: p
                    .asked
                    .map_or(p.full_top, |(_, margin)| (margin.0 - EDGE) as f32),
                surface_bottom: p.asked.map_or(p.full_top, |((_, height), margin)| {
                    (margin.0 - EDGE) as f32 + height as f32
                }),
            })
            .unwrap_or_default()
    })
}

/// What bar `bar`'s group takes in `form` (the column's room for Today
/// ends 14 px over it): its height, none in the mini form.
pub fn height(bar: &str, form: Form) -> f32 {
    match form {
        Form::Full => state(|s| s.places.get(bar).map_or(0.0, |p| rows_height(&p.rows))),
        Form::Mini => 0.0,
    }
}

/// `[panel] windows`: shown or hidden, on every bar.
pub fn set_shown(shown: bool) {
    let changed = state(|s| {
        let changed = s.hidden == shown;
        s.hidden = !shown;
        changed
    });
    if changed {
        tracing::info!(shown, "the windows in the bar");
        changed_now();
    }
}

/// What the windows of a screen are changed (opened, closed, retitled,
/// focused, another desktop): each bar's lines again; the column moves
/// if the group's height changed.
pub fn changed() {
    changed_now();
}

fn changed_now() {
    if fit_all() {
        column::moved();
    }
}

/// The windows' icons were found: what shows them draws again.
pub fn icons_found() {
    let open = popover::with(|c| matches!(c.open_on(), Some((_, Item::Windows))));
    if open {
        popover::send(vec![Effect::Wake(POPOVER_NAMESPACE)]);
    }
}

/// What each bar has to place its group with, read outside the lock.
struct Room {
    bar: String,
    lines: Vec<Line>,
    /// The column's room for the full form's group.
    room: f32,
    /// The desktops' tiles' top and dots' top, under the bar's top.
    full_bottom: Option<f32>,
    mini_bottom: Option<f32>,
    /// The mini form's room over its dots.
    mini_room: f32,
}

/// Every bar's rows for its windows and its room now: its surface comes,
/// goes, moves or draws again as they change. Whether a full form's
/// group changed height (the column's room for Today changes).
pub fn fit_all() -> bool {
    let (bars, hidden) = state(|s| (s.places.keys().cloned().collect::<Vec<_>>(), s.hidden));
    // Measured outside the lock: the desktops', the column's and the
    // popovers' own locks.
    let rooms: Vec<Room> = bars
        .into_iter()
        .map(|bar| {
            let lines = if hidden { Vec::new() } else { lines(&bar) };
            // From the bar's bottom: not before its height is known, else
            // the surface would go off a short screen, which the shell does
            // not survive (#617).
            let height = popover::with(|c| c.known_height(&bar));
            let under = |form| {
                let above = crate::desktops::draw::top_above_bottom_in(&bar, form)?;
                // Over the music player's tile, when it shows (s12).
                Some(height? - above - crate::music::room(&bar, form))
            };
            let (full_bottom, mini_bottom) = (under(Form::Full), under(Form::Mini));
            let room = column::room(&bar, Part::Windows);
            let mini_room = mini_bottom.map_or(0.0, |b| b - GAP - column::end(&bar, Form::Mini));
            Room {
                bar,
                lines,
                room,
                full_bottom,
                mini_bottom,
                mini_room,
            }
        })
        .collect();
    let mut effects = Vec::new();
    let mut placed: Vec<(String, Option<window::Id>)> = Vec::new();
    let mut taller = false;
    let mut redraw = false;
    let mut listed: Vec<String> = Vec::new();
    let words = Words::new(Lang::of_messages());
    state(|s| {
        for r in rooms {
            let Some(place) = s.places.get_mut(&r.bar) else {
                continue;
            };
            let full = place.form == Some(Form::Full) || place.switching;
            let mini = place.form == Some(Form::Mini) || place.switching;
            let rows = match r.full_bottom {
                Some(_) if full => fit(&r.lines, r.room),
                _ => Vec::new(),
            };
            let item = match r.mini_bottom {
                Some(_) if mini && r.mini_room >= ITEM => r.lines.first().cloned(),
                _ => None,
            };
            if rows_height(&rows) != rows_height(&place.rows) {
                taller = true;
            }
            if rows != place.rows || item != place.item {
                redraw = true;
                tracing::debug!(
                    bar = r.bar,
                    shows = %words.said(&rows),
                    item = ?item.as_ref().map(|l| l.title.as_str()),
                    room = r.room,
                    "the windows' rows"
                );
            }
            if r.lines != place.lines {
                listed.push(r.bar.clone());
            }
            place.lines = r.lines;
            // Until the desktops are placed, it stays where it is: a top
            // from nothing would be off the screen (#617).
            if let Some(bottom) = r.full_bottom {
                place.full_top = bottom - GAP - rows_height(&rows);
            }
            if let Some(bottom) = r.mini_bottom {
                place.mini_top = bottom - GAP - ITEM;
            }
            place.rows = rows;
            place.item = item;
            effects.extend(place.fit(&r.bar, &mut placed, &mut redraw));
        }
    });
    if !placed.is_empty() {
        popover::with(|c| {
            for (bar, id) in placed {
                c.set_windows(&bar, id);
            }
        });
    }
    if redraw {
        effects.push(Effect::Wake(WINDOWS_NAMESPACE));
    }
    // The mini form's popover lists them too.
    let open = popover::with(|c| match c.open_on() {
        Some((bar, Item::Windows)) if listed.iter().any(|b| b == bar) => {
            let mut refit = c.refit();
            refit.push(Effect::Wake(POPOVER_NAMESPACE));
            refit
        }
        _ => Vec::new(),
    });
    effects.extend(open);
    if !effects.is_empty() {
        popover::send(effects);
    }
    taller
}

/// The surface's top under the bar's and its size, for the full form's
/// group (its top and height) and the mini form's item, in `form` (both
/// while the bar switches); none when neither shows.
fn placement(
    form: Form,
    switching: bool,
    (full_top, full): (f32, f32),
    (mini_top, mini): (f32, f32),
) -> Option<(f32, (u32, u32))> {
    let tall = |height: f32| (height + 2.0 * draw::ROOM).ceil() as u32;
    match (switching, form) {
        (false, Form::Full) if full > 0.0 => {
            Some((full_top - draw::ROOM, (draw::FULL_WIDTH, tall(full))))
        }
        (false, Form::Mini) if mini > 0.0 => {
            Some((mini_top - draw::ROOM, (draw::MINI_WIDTH, tall(mini))))
        }
        (true, _) if full > 0.0 || mini > 0.0 => {
            let spans = [(full_top, full), (mini_top, mini)];
            let shown = spans.iter().filter(|(_, h)| *h > 0.0);
            let top = shown.clone().map(|(t, _)| *t).fold(f32::INFINITY, f32::min);
            let bottom = shown.map(|(t, h)| t + h).fold(f32::NEG_INFINITY, f32::max);
            Some((top - draw::ROOM, (draw::FULL_WIDTH, tall(bottom - top))))
        }
        _ => None,
    }
}

/// The layer-shell margins of a surface whose top is `top` under the
/// bar's.
fn margin(top: f32) -> (i32, i32, i32, i32) {
    (EDGE + top.round().max(0.0) as i32, EDGE + 1, 0, 0)
}

/// The surface's height when it shows nothing: the ring's room, drawing
/// nothing. Not a pixel: at a fractional scale tiny-skia refuses a buffer
/// that small (#617). On the CPU, where it would show black, the
/// compositor leaves it out.
const IDLE: u32 = hocus_link::IDLE_HEIGHT as u32;

/// The surface's width when it shows nothing, in `form`.
fn width_of(form: Option<Form>, switching: bool) -> u32 {
    match (form, switching) {
        (Some(Form::Mini), false) => draw::MINI_WIDTH,
        _ => draw::FULL_WIDTH,
    }
}

impl Place {
    /// Its surface for what it shows: resized and moved, drawing nothing
    /// and taking no pointer when nothing shows; drawn again (`redraw`) where it
    /// moves, its backdrop the bar's colour there on the CPU. The surface
    /// stays as long as its bar: iced_layershell hands the keys to the
    /// surface made last, and one made while the emoji picker or the
    /// launcher holds the keyboard would lose their typing (#617).
    fn fit(
        &mut self,
        bar: &str,
        placed: &mut Vec<(String, Option<window::Id>)>,
        redraw: &mut bool,
    ) -> Vec<Effect> {
        let Some(id) = self.id else {
            return Vec::new();
        };
        let at = self.form.and_then(|form| {
            placement(
                form,
                self.switching,
                (self.full_top, rows_height(&self.rows)),
                (self.mini_top, if self.item.is_some() { ITEM } else { 0.0 }),
            )
        });
        let shows = at.is_some();
        // Showing nothing: in the gap over the desktops (or the dots),
        // over nothing else.
        let (top, size) = at.unwrap_or_else(|| {
            let bottom = match (self.form, self.switching) {
                (Some(Form::Mini), false) => self.mini_top + ITEM,
                _ => self.full_top + rows_height(&self.rows),
            };
            (
                bottom - draw::ROOM,
                (width_of(self.form, self.switching), IDLE),
            )
        });
        let asked = (size, margin(top));
        let mut effects = Vec::new();
        if self.asked != Some(asked) {
            self.asked = Some(asked);
            *redraw = true;
            effects.push(Effect::Resize {
                id,
                size: asked.0,
                margin: Some(asked.1),
            });
        }
        if self.input != Some(shows) {
            self.input = Some(shows);
            // The bar's items for the keyboard: its own while it shows.
            placed.push((bar.to_owned(), shows.then_some(id)));
            effects.push(Effect::Input { id, takes: shows });
        }
        effects
    }
}

/// Bar `bar` opened in `form`: its surface, drawing nothing and taking
/// no pointer until the column moves (`crate::column::moved`, as the bar takes its
/// height) and it has windows to show; to open after the bar's own.
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let id = window::Id::unique();
    // Under the clock, inside the smallest screen: placed over the
    // desktops once the bar knows its height (a place off a short screen
    // keeps the shell from going on).
    let top = column::start(form) - draw::ROOM;
    let size = (width_of(Some(form), false), IDLE);
    state(|s| {
        let place = s.places.entry(bar.to_owned()).or_default();
        place.form = Some(form);
        place.id = Some(id);
        place.full_top = top + draw::ROOM;
        place.mini_top = top + draw::ROOM;
        place.asked = Some((size, margin(top)));
        place.input = None;
    });
    vec![Effect::Layer {
        id,
        bar: bar.to_owned(),
        namespace: WINDOWS_NAMESPACE,
        size,
        margin: margin(top),
    }]
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

/// The bar whose windows' surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    state(|s| {
        s.places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

// --- Words ----------------------------------------------------------------------

/// The windows' words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub(crate) lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// "+ 2 autres", "+ 1 autre".
    pub fn more(self, n: usize) -> String {
        match ((self.lang == Lang::French), n) {
            (true, 1) => "+ 1 autre".to_owned(),
            (true, n) => format!("+ {n} autres"),
            (false, n) => format!("+ {n} more"),
        }
    }

    /// The ✕'s name: "Fermer Terminal".
    pub fn close(self, title: &str) -> String {
        txt::close(self.lang, title)
    }

    /// A line's name for screen readers: its title, and whether it is
    /// the focused window or minimised.
    pub fn name(self, line: &Line) -> String {
        let state = match (line.focused, line.minimized) {
            (true, _) => ", active",
            (_, true) => txt::MINIMISED.get(self.lang),
            _ => "",
        };
        format!("{}{state}", line.title)
    }

    /// The mini form's item's name: "Fenêtres du bureau : 4".
    pub fn item(self, count: usize) -> String {
        txt::this_desktop_s_windows(self.lang, count)
    }

    /// What the rows say, for the log (and tests): each title in «», the
    /// focused one marked `*`, a minimised one `~`, then "+ N".
    pub fn said(self, rows: &[Row]) -> String {
        if rows.is_empty() {
            return "nothing".to_owned();
        }
        let rows: Vec<String> = rows
            .iter()
            .map(|row| match row {
                Row::Window(line) => {
                    let mark = match (line.focused, line.minimized) {
                        (true, _) => "*",
                        (_, true) => "~",
                        _ => "",
                    };
                    // Its states' marks (#723): above, on all desktops.
                    let above = if line.above { "^" } else { "" };
                    let everywhere = if line.everywhere { "∀" } else { "" };
                    format!("«{}»{mark}{above}{everywhere}", line.title)
                }
                Row::More(n) => self.more(*n),
            })
            .collect();
        rows.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use hocus_link::DeskWindow;

    use super::*;

    fn window(desktop: u8, id: u64, look: Look) -> DeskWindow {
        DeskWindow {
            desktop,
            rect: [0, 0, 500, 500],
            look,
            id,
            app: format!("app{id}"),
        }
    }

    fn line(id: u64) -> Line {
        Line {
            id,
            app: String::new(),
            title: format!("w{id}"),
            ..Line::default()
        }
    }

    #[test]
    fn the_current_desktops_windows_the_focused_first_then_by_recent_focus() {
        // Bottom first: 1, 2 (focused), 3 on desktop 0; 4 on desktop 1;
        // 5 minimised on desktop 0.
        let desktops = Desktops {
            output: "eDP-1".into(),
            count: 4,
            current: 0,
            windows: vec![
                window(0, 1, Look::Plain),
                window(0, 2, Look::Focused),
                window(0, 3, Look::Plain),
                window(1, 4, Look::Plain),
                window(0, 5, Look::Minimized),
            ],
        };
        let recent = Recent {
            output: "eDP-1".into(),
            ids: vec![2, 5, 4, 1, 3],
        };
        let titles = Titles {
            output: "eDP-1".into(),
            titles: vec![(1, "~/hocus — fish".into()), (2, String::new())],
        };
        let lines = lines_of(&desktops, Some(&titles), Some(&recent));
        assert_eq!(lines.iter().map(|l| l.id).collect::<Vec<_>>(), [2, 5, 1, 3]);
        // Without a title, its application's id.
        assert_eq!(lines[0].title, "app2");
        assert_eq!(lines[2].title, "~/hocus — fish");
        assert!(lines[0].focused && !lines[1].focused);
        assert!(lines[1].minimized);
        // The focused one first even when `recent` lags behind; without
        // `recent`, front to back.
        let late = Recent {
            output: "eDP-1".into(),
            ids: vec![3, 1, 2],
        };
        let lines = lines_of(&desktops, None, Some(&late));
        assert_eq!(lines.iter().map(|l| l.id).collect::<Vec<_>>(), [2, 3, 1, 5]);
        let lines = lines_of(&desktops, None, None);
        assert_eq!(lines.iter().map(|l| l.id).collect::<Vec<_>>(), [2, 5, 3, 1]);
        // Another desktop: its own.
        let other = Desktops {
            current: 1,
            ..desktops
        };
        assert_eq!(lines_of(&other, None, None).len(), 1);
    }

    #[test]
    fn three_lines_at_most_then_plus_n_and_none_without_room() {
        let lines: Vec<Line> = (1..=5).map(line).collect();
        // Two windows: two lines.
        let rows = fit(&lines[..2], 1000.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows_height(&rows), 60.0);
        // Three: three, no "+ N".
        assert_eq!(fit(&lines[..3], 1000.0).len(), 3);
        // Five: three and "+ 2".
        let rows = fit(&lines, 1000.0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3], Row::More(2));
        assert_eq!(rows_height(&rows), 110.0);
        // A short room: what fits with "+ N" under it.
        let rows = fit(&lines[..3], 89.0);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2], Row::More(1));
        // No room for a line and "+ N": no group; nothing to list: none.
        assert!(fit(&lines, 49.0).is_empty());
        assert!(fit(&[], 1000.0).is_empty());
        // One window in room for it alone.
        assert_eq!(fit(&lines[..1], 30.0).len(), 1);
    }

    #[test]
    fn its_surface_holds_its_group_or_its_item_and_both_while_switching() {
        // Two lines whose group's top is 600 px under the bar's.
        assert_eq!(
            placement(Form::Full, false, (600.0, 60.0), (700.0, 0.0)),
            Some((595.0, (194, 70)))
        );
        assert_eq!(
            placement(Form::Mini, false, (600.0, 60.0), (640.0, ITEM)),
            Some((635.0, (54, 44)))
        );
        // Switching: from the higher top to the lower bottom.
        assert_eq!(
            placement(Form::Mini, true, (600.0, 60.0), (640.0, ITEM)),
            Some((595.0, (194, 84)))
        );
        assert_eq!(
            placement(Form::Full, false, (600.0, 0.0), (640.0, ITEM)),
            None
        );
        assert_eq!(
            placement(Form::Full, true, (600.0, 0.0), (640.0, 0.0)),
            None
        );
    }

    #[test]
    fn the_windows_words_in_both_languages() {
        let (fr, en) = (Words::new(Lang::French), Words::new(Lang::English));
        assert_eq!(fr.more(2), "+ 2 autres");
        assert_eq!(fr.more(1), "+ 1 autre");
        assert_eq!(en.more(2), "+ 2 more");
        assert_eq!(fr.close("Terminal"), "Fermer Terminal");
        assert_eq!(en.close("Terminal"), "Close Terminal");
        let mut l = line(1);
        l.focused = true;
        assert_eq!(fr.name(&l), "w1, active");
        let mut l = line(2);
        l.minimized = true;
        assert_eq!(fr.name(&l), "w2, réduite");
        assert_eq!(en.name(&l), "w2, minimised");
        assert_eq!(fr.item(4), "Fenêtres du bureau : 4");
        let mut first = line(1);
        first.focused = true;
        assert_eq!(
            fr.said(&[Row::Window(first), Row::Window(l), Row::More(3)]),
            "«w1»*; «w2»~; + 3 autres"
        );
        assert_eq!(fr.said(&[]), "nothing");
    }
}
