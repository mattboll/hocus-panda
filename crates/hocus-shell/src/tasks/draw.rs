//! The to-do list's surface (Day.4, bar.md §4 group 5, §5), as the
//! approved mockup's variant C draws it, at ×1.
//!
//! - **The full form's group**, 14 px under Today (or where it would be),
//!   the agents' group under it (`crate::column`): an 18 px header, "À
//!   FAIRE" (Manrope 700, 11.5 px, spaced 0.1 em, `text.dim`) and on its
//!   right the `+` (22 × 18, radius 6, its
//!   14 px plus `text.dim`, lit `0.06` under the pointer); a 22 px line per
//!   task, at most 3 (fewer on a short bar, once Today gave way: the
//!   column's room): its 14 px box (radius 4, a 1.5 px `0.38` border) in
//!   a 22 × 22 button 4 px past the group's left, lit under the pointer,
//!   then 4 px and the title (13 px, `text`, cut with an ellipsis; resting
//!   600 ms on a cut title shows it whole in a tooltip); past 3, a 20 px
//!   "+ N autres" (12.5 px, `text.faint`), a button opening the tasks'
//!   popover. No task: "Rien à faire." (13 px, `text.faint`). The field to
//!   add a task is its first line, 26 px (2 px above and under), radius 7,
//!   `0.06`, a 1 px `rgba(108,194,135,0.65)` border, 13 px.
//! - A ticked box fills with `#6CC287` and shows a dark check (`#0b1020`),
//!   its title `text.faint` and struck through; "Pas enregistré" (12 px,
//!   `#F7A77A`) takes the title's place for 3 s when the source refused.
//! - **The mini form's item**, 14 px under the meeting's mark (or the
//!   sign of what is in use), over the agents' item: 44 × 34,
//!   radius 10, a 16 px box (`text.dim`) and how many tasks are open
//!   (Martian Mono 11 px), lit `0.06` under the pointer and `0.08` while
//!   the popover shows; a 150 ms hover or a click opens it.
//!
//! As tall as what it shows, one pixel taking no pointer when nothing
//! shows (no board set and no local task). While the form switches it
//! holds both forms, which cross-fade as the other groups do.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{EDGE, Form, TASKS_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};

use super::{At, GROUP_LINES, Key, Lines, Look, Words};
use crate::theme::Ink;
use crate::{
    Message,
    clock::motion::bezier,
    levels::draw::{
        DIM, FAINT, HOVER, MANROPE, MANROPE_BOLD, OPEN, TEXT, faded, pointer, rgb, ring,
    },
    popover::{self, Item, Pressed, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// Around what it draws: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;
/// The group's sides on the surface: the bar's 12 px padding, less its
/// rim; its width, the bar's inside.
const SIDE: f32 = 11.0;
pub const WIDTH: f32 = 172.0;

/// The header: 18 px; its label, spaced 0.1 em.
const HEADER: f32 = 18.0;
const LABEL: f32 = 11.5;
const LABEL_SPACING: f32 = 0.1 * LABEL;
/// The `+`: 22 × 18, radius 6, 4 px past the group's right; its plus
/// 14 px.
const ADD: Size = Size::new(22.0, 18.0);
const ADD_OUT: f32 = 4.0;
const ADD_RADIUS: f32 = 6.0;
const PLUS: f32 = 14.0;
/// A task's line: 22 px; its box's button 22 × 22, 4 px past the group's
/// left, radius 6; the box 14 px, radius 4, its border 1.5 px; the title
/// 4 px after the button, 13 px.
pub const LINE: f32 = 22.0;
const BUTTON: f32 = 22.0;
const BUTTON_OUT: f32 = 4.0;
const BUTTON_RADIUS: f32 = 6.0;
pub const BOX: f32 = 14.0;
const BOX_RADIUS: f32 = 4.0;
const BOX_BORDER: f32 = 1.5;
const TITLE_GAP: f32 = 4.0;
const TITLE: f32 = 13.0;
/// "Pas enregistré": 12 px, peach.
const UNSAVED: f32 = 12.0;
pub const UNSAVED_INK: Ink = Ink::Dark(rgb(0xF7A77A));
/// "+ N autres": 20 px, 12.5 px, a button 6 px past its text each side.
const MORE: f32 = 20.0;
const MORE_TEXT: f32 = 12.5;
const MORE_OUT: f32 = 6.0;
/// The field: 26 px, 2 px above and under, radius 7, 8 px in.
const FIELD: f32 = 26.0;
const FIELD_MARGIN: f32 = 2.0;
const FIELD_RADIUS: f32 = 7.0;
const FIELD_PAD: f32 = 8.0;
/// The mini item: 44 × 34, radius 10; its box 16 px, 5 px before the
/// count (Martian Mono 11 px).
const MINI: Size = Size::new(44.0, 34.0);
const MINI_RADIUS: f32 = 10.0;
const MINI_ICON: f32 = 16.0;
const MINI_GAP: f32 = 5.0;
const MINI_TEXT: f32 = 11.0;
const MONO: iced::Font = iced::Font::with_name("Martian Mono");

const BORDER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.38));
pub const TICKED: Ink = Ink::Accent(1.0);
const CHECK: Ink = Ink::Dark(rgb(0x0B1020));
pub const FIELD_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
pub const FIELD_BORDER: Ink = Ink::Accent(0.65);
/// A title's tooltip, after resting this long on it.
const TIP: std::time::Duration = std::time::Duration::from_millis(600);
/// The line's close, `ease-in`.
const EASE_IN: (f32, f32, f32, f32) = (0.42, 0.0, 1.0, 1.0);

// --- The group laid out ----------------------------------------------------------------

/// A line of the group, laid out from the group's top.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    key: Key,
    look: Look,
    /// Its title, and as cut to its room (none: it fits).
    title: String,
    cut: Option<String>,
    top: f32,
    height: f32,
    alpha: f32,
}

/// What the group shows, from its top.
#[derive(Debug, Clone, PartialEq, Default)]
struct Laid {
    /// 0: nothing shows.
    height: f32,
    field: Option<Rectangle>,
    rows: Vec<Row>,
    /// "Rien à faire."'s middle.
    empty: Option<f32>,
    more: Option<(String, Rectangle)>,
    /// The mini item's count (none: no item).
    mini: Option<usize>,
    /// Day.1's first-run card, in the group's place (full form).
    card: Option<crate::setup::card::Card>,
}

/// The group's `+`, from the group's top left.
fn add_rect() -> Rectangle {
    Rectangle::new(Point::new(WIDTH + ADD_OUT - ADD.width, 0.0), ADD)
}

/// A line's box's button, its line from `top`.
fn button_rect(top: f32) -> Rectangle {
    Rectangle::new(
        Point::new(-BUTTON_OUT, top + (LINE - BUTTON) / 2.0),
        Size::new(BUTTON, BUTTON),
    )
}

/// Where a line's title starts.
fn title_x() -> f32 {
    -BUTTON_OUT + BUTTON + TITLE_GAP
}

/// `lines` laid out, and the field if open on `bar`'s group, cut with
/// `width`.
fn lay_out(
    lines: &Lines,
    field: bool,
    most: usize,
    words: Words,
    mut width: impl FnMut(&str, f32) -> f32,
    card: Option<crate::setup::card::Card>,
) -> Laid {
    if let Some(card) = card {
        return Laid {
            height: card.height,
            mini: lines.shows.then(|| lines.open()),
            card: Some(card),
            ..Laid::default()
        };
    }
    if !lines.shows {
        return Laid::default();
    }
    let mut y = HEADER;
    let field = field.then(|| {
        let rect = Rectangle::new(Point::new(0.0, y + FIELD_MARGIN), Size::new(WIDTH, FIELD));
        y += FIELD + 2.0 * FIELD_MARGIN;
        rect
    });
    let room = WIDTH - title_x();
    // The lines that stay, at most `most`, with those going among them;
    // one coming in under them fades in as they close.
    let going: Vec<f32> = lines.lines.iter().filter_map(|l| l.going).collect();
    let closing = going
        .iter()
        .copied()
        .map(|t| bezier(EASE_IN, t))
        .fold(0.0_f32, f32::max);
    let mut rows = Vec::new();
    let mut staying = 0;
    for line in &lines.lines {
        let goes = line.going.is_some();
        if !goes && staying == most {
            continue;
        }
        let (height, alpha) = match line.going {
            Some(t) => {
                let t = bezier(EASE_IN, t);
                (LINE * (1.0 - t), 1.0 - t)
            }
            None if staying + going.len() >= most => (LINE, closing.max(0.0)),
            None => (LINE, 1.0),
        };
        // One coming in shows as the others close: fully once they did.
        let alpha = if going.is_empty() { 1.0 } else { alpha };
        if !goes {
            staying += 1;
        }
        let cut = (width(&line.title, TITLE) > room)
            .then(|| kit::elide(&line.title, room, |t| width(t, TITLE)));
        rows.push(Row {
            key: line.key.clone(),
            look: line.look,
            title: line.title.clone(),
            cut,
            top: y,
            height,
            alpha,
        });
        y += height;
    }
    let rest = lines
        .lines
        .iter()
        .filter(|l| l.going.is_none())
        .count()
        .saturating_sub(most);
    // Nothing to do (not every line gone into "+ N" on a short bar).
    let mut empty = None;
    if rows.is_empty() && field.is_none() && rest == 0 {
        empty = Some(y + LINE / 2.0);
        y += LINE;
    }
    let more = (rest > 0).then(|| {
        let said = words.more(rest);
        let w = width(&said, MORE_TEXT);
        let rect = Rectangle::new(
            Point::new(-MORE_OUT, y),
            Size::new(w + 2.0 * MORE_OUT, MORE),
        );
        y += MORE;
        (said, rect)
    });
    Laid {
        height: y,
        field,
        rows,
        empty,
        more,
        mini: Some(lines.open()),
        card: None,
    }
}

/// The group's height at rest for `laid`: the lines going count until
/// they went.
fn resting_height(laid: &Laid) -> f32 {
    if laid.height <= 0.0 {
        return 0.0;
    }
    laid.height + laid.rows.iter().map(|r| LINE - r.height).sum::<f32>()
}

/// The least the group takes once its lines gave way (`laid` laid out
/// with them): its header, the field if open, and its last line, "+ N"
/// (or "Rien à faire."); Day.1's card, all of it.
fn least_of(laid: &Laid) -> f32 {
    if laid.card.is_some() || laid.height <= 0.0 {
        return laid.height;
    }
    let field = laid.field.map_or(0.0, |_| FIELD + 2.0 * FIELD_MARGIN);
    let last = if laid.empty.is_some() {
        LINE
    } else if !laid.rows.is_empty() || laid.more.is_some() {
        MORE
    } else {
        0.0
    };
    HEADER + field + last
}

/// How many of `open` tasks' lines show in `room` (the field over them if
/// `field`), at most 3: all of them when they fit, else as many as fit
/// over "+ N", none at the least. The to-do list gives way after Today
/// (§4, `crate::column`).
pub fn lines_that_fit(open: usize, field: bool, room: f32) -> usize {
    let head = HEADER
        + if field {
            FIELD + 2.0 * FIELD_MARGIN
        } else {
            0.0
        };
    if open <= GROUP_LINES && head + open as f32 * LINE <= room {
        return GROUP_LINES;
    }
    let fit = ((room - head - MORE) / LINE).floor().max(0.0);
    (fit as usize).min(GROUP_LINES)
}

// --- Where the surface goes ------------------------------------------------------------

/// Bar `bar`'s group's top under the bar's, in its column (full form):
/// under Today, or where it would be.
fn group_top(bar: &str) -> f32 {
    crate::column::top(bar, Form::Full, crate::column::Part::Tasks)
}

/// Bar `bar`'s mini item's top under the bar's, in its column.
fn item_top(bar: &str) -> f32 {
    crate::column::top(bar, Form::Mini, crate::column::Part::Tasks)
}

/// Bar `bar`'s group's top as last placed.
pub fn full_top(bar: &str) -> f32 {
    places(|places| places.get(bar).map(|p| p.top)).unwrap_or_else(|| group_top(bar))
}

/// Bar `bar`'s mini item's top as last placed.
fn mini_top(bar: &str) -> f32 {
    places(|places| places.get(bar).map(|p| p.mini_top)).unwrap_or_else(|| item_top(bar))
}

/// The height bar `bar`'s group (the full form) or item (the mini) takes,
/// as last laid out: 0 when nothing shows.
pub fn height(bar: &str, form: Form) -> f32 {
    places(|places| {
        places.get(bar).map_or(0.0, |p| match form {
            Form::Full => p.heights.full,
            Form::Mini if p.heights.mini => MINI.height,
            Form::Mini => 0.0,
        })
    })
}

/// The least bar `bar`'s group takes in the full form, its lines gone
/// into "+ N": 0 when it does not show.
pub fn least(bar: &str) -> f32 {
    places(|places| places.get(bar).map_or(0.0, |p| p.heights.least))
}

/// A layer surface's margins: top, right, bottom, left.
type Margin = (i32, i32, i32, i32);

#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    asked: ((u32, u32), Margin),
    input: Option<bool>,
    /// What its group last laid out took (its field is its own).
    heights: Heights,
    /// The full form's group's top under the bar's, and the mini item's.
    top: f32,
    mini_top: f32,
    /// How many lines its group last showed at most, for its room.
    most: usize,
}

/// The group's heights as last laid out: the full form's, the least it
/// takes once its lines gave way, and whether the mini item shows.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Heights {
    full: f32,
    least: f32,
    mini: bool,
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);
/// Each bar's lines as drawn, from the top: what its `Task(slot)` items
/// are.
static SLOTS: Mutex<Option<HashMap<String, Vec<Key>>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The surface's top under the bar's and its size, in `form`, for a
/// group `h` tall, its top `top` and the mini item's `mini_top`.
fn placement(
    form: Form,
    switching: bool,
    h: Heights,
    top: f32,
    mini_top: f32,
) -> (f32, (u32, u32)) {
    let tall = |height: f32| (height + 2.0 * ROOM).ceil() as u32;
    let full = (top, h.full);
    let mini = (mini_top, if h.mini { MINI.height } else { 0.0 });
    match (switching, form) {
        (false, Form::Full) if full.1 > 0.0 => (full.0 - ROOM, (FULL_WIDTH, tall(full.1))),
        (false, Form::Full) => (full.0 - ROOM, (FULL_WIDTH, 1)),
        (false, Form::Mini) if mini.1 > 0.0 => (mini.0 - ROOM, (MINI_WIDTH, tall(mini.1))),
        (false, Form::Mini) => (mini.0 - ROOM, (MINI_WIDTH, 1)),
        (true, _) if full.1 <= 0.0 && mini.1 <= 0.0 => (full.0 - ROOM, (FULL_WIDTH, 1)),
        (true, _) => {
            let top = full.0.min(mini.0);
            let bottom = (full.0 + full.1).max(mini.0 + mini.1);
            (top - ROOM, (FULL_WIDTH, tall(bottom - top)))
        }
    }
}

fn margin(top: f32) -> Margin {
    (EDGE + top.round() as i32, EDGE + 1, 0, 0)
}

/// The to-do list of bar `bar` in `form`, on `output`: its surface's id,
/// and what opens it.
pub fn open(bar: &str, form: Form, output: OutputOption) -> (window::Id, Vec<Message>) {
    let id = window::Id::unique();
    let (group, item) = (group_top(bar), item_top(bar));
    let (top, size) = placement(form, false, Heights::default(), group, item);
    let margin = margin(top);
    places(|places| {
        places.insert(
            bar.to_owned(),
            Place {
                heights: Heights::default(),
                top: group,
                mini_top: item,
                most: GROUP_LINES,
                id,
                form,
                switching: false,
                asked: (size, margin),
                input: None,
            },
        )
    });
    let settings = NewLayerShellSettings {
        size: Some(size),
        layer: Layer::Top,
        anchor: Anchor::Right | Anchor::Top,
        exclusive_zone: Some(0),
        margin: Some(margin),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: output,
        events_transparent: false,
        namespace: Some(TASKS_NAMESPACE.to_owned()),
    };
    (id, vec![Message::NewLayerShell { settings, id }])
}

/// Its surface on bar `bar`.
pub fn surface(bar: &str) -> Option<window::Id> {
    places(|places| places.get(bar).map(|p| p.id))
}

/// What line `slot` of bar `bar`'s group is.
pub fn slot(bar: &str, slot: u8) -> Option<Key> {
    let slots = SLOTS.lock().unwrap_or_else(PoisonError::into_inner);
    slots.as_ref()?.get(bar)?.get(usize::from(slot)).cloned()
}

fn refit(place: &mut Place) -> Option<((u32, u32), Margin)> {
    let (top, size) = placement(
        place.form,
        place.switching,
        place.heights,
        place.top,
        place.mini_top,
    );
    let margin = margin(top);
    if place.asked == (size, margin) {
        return None;
    }
    place.asked = (size, margin);
    Some((size, margin))
}

/// Bar `bar` is in `form`, switching or not: its surface's new size and
/// margins, if they change. The field closes as the form changes.
pub fn form(bar: &str, form: Form, switching: bool) -> Vec<Message> {
    let (top, item) = (group_top(bar), item_top(bar));
    let changed = places(|places| {
        let place = places.get_mut(bar)?;
        place.top = top;
        place.mini_top = item;
        let changed = place.form != form || place.switching != switching;
        place.form = form;
        place.switching = switching;
        Some((changed, place.id, refit(place)))
    });
    let Some((changed, id, fitted)) = changed else {
        return Vec::new();
    };
    if changed {
        super::close_field_at(&At::Group(bar.to_owned()), "the bar's form changed");
    }
    fitted
        .map(|(size, margin)| {
            vec![
                Message::SizeChange { id, size },
                Message::MarginChange { id, margin },
            ]
        })
        .unwrap_or_default()
}

/// The groups above moved, or this one's height changed: every bar's
/// surface to its place in the column.
pub fn moved() {
    // The column's tops read first: not under this lock.
    let bars: Vec<String> = places(|places| places.keys().cloned().collect());
    let tops: Vec<(String, f32, f32)> = bars
        .into_iter()
        .map(|b| (group_top(&b), item_top(&b), b))
        .map(|(t, m, b)| (b, t, m))
        .collect();
    let mut effects: Vec<popover::Effect> = places(|places| {
        tops.iter()
            .filter_map(|(bar, top, item)| {
                let place = places.get_mut(bar)?;
                place.top = *top;
                place.mini_top = *item;
                let (size, margin) = refit(place)?;
                Some(popover::Effect::Resize {
                    id: place.id,
                    size,
                    margin: Some(margin),
                })
            })
            .collect()
    });
    if !effects.is_empty() {
        // Drawn again where it goes: on the CPU its backdrop is the bar's
        // colour there.
        effects.push(popover::Effect::Wake(TASKS_NAMESPACE));
        popover::send(effects);
    }
}

/// Bar `bar`'s room for its lines now (none counted for the field), as
/// many as fit: what its group lays out at most.
fn most_for(bar: &str, lines: &Lines) -> usize {
    let open = lines.lines.iter().filter(|l| l.going.is_none()).count();
    let field = super::field().is_some_and(|f| f.at == At::Group(bar.to_owned()));
    let room = crate::column::room(bar, crate::column::Part::Tasks);
    lines_that_fit(open, field, room)
}

/// The column's room changed: the groups whose lines it changes draw
/// again (laid out as they draw).
pub fn fit_lines() {
    let bars: Vec<String> = places(|places| places.keys().cloned().collect());
    if bars.is_empty() {
        return;
    }
    let lines = super::lines();
    let most: Vec<(String, usize)> = bars
        .into_iter()
        .map(|bar| {
            let most = most_for(&bar, &lines);
            (bar, most)
        })
        .collect();
    let changed = places(|places| {
        most.iter()
            .any(|(bar, most)| places.get(bar).is_some_and(|p| p.most != *most))
    });
    if changed {
        popover::send(vec![popover::Effect::Wake(TASKS_NAMESPACE)]);
    }
}

/// Bar `bar` went.
pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
    if let Some(slots) = SLOTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        slots.remove(bar);
    }
}

/// Bar `bar`'s surface takes the pointer (`takes`), or lets it through.
fn takes_pointer(bar: &str, takes: bool) {
    let id = places(|places| {
        let place = places.get_mut(bar)?;
        if place.input == Some(takes) {
            return None;
        }
        place.input = Some(takes);
        Some(place.id)
    });
    if let Some(id) = id {
        popover::send(vec![popover::Effect::Input { id, takes }]);
    }
}

/// The surface's top under the bar's, as last asked for.
fn top_of(bar: &str) -> f32 {
    places(|places| places.get(bar).map(|p| (p.asked.1.0 - EDGE) as f32))
        .unwrap_or_else(|| group_top(bar) - ROOM)
}

// --- Drawing ---------------------------------------------------------------------------

/// A box in its button at `button`: ticked or not, the button lit under
/// the pointer.
pub fn tick_box(f: &mut Frame, button: Rectangle, ticked: bool, lit: bool, alpha: f32) {
    if lit {
        f.fill(
            &kit::rounded(button, BUTTON_RADIUS),
            faded(HOVER.get(), alpha),
        );
    }
    let centre = button.center();
    let rect = Rectangle::new(
        Point::new(centre.x - BOX / 2.0, centre.y - BOX / 2.0),
        Size::new(BOX, BOX),
    );
    if ticked {
        f.fill(&kit::rounded(rect, BOX_RADIUS), faded(TICKED.get(), alpha));
        // The mockup's check, 10 px of its 24-unit box.
        let unit = 10.0 / 24.0;
        let at = |x: f32, y: f32| Point::new(centre.x - 5.0 + x * unit, centre.y - 5.0 + y * unit);
        let check = Path::new(|p| {
            p.move_to(at(5.0, 12.5));
            p.line_to(at(10.0, 17.5));
            p.line_to(at(19.0, 7.5));
        });
        f.stroke(
            &check,
            Stroke {
                width: 3.4 * unit,
                line_cap: canvas::LineCap::Round,
                line_join: canvas::LineJoin::Round,
                ..Stroke::default().with_color(faded(CHECK.get(), alpha))
            },
        );
    } else {
        let inset = BOX_BORDER / 2.0;
        let border = Rectangle::new(
            Point::new(rect.x + inset, rect.y + inset),
            Size::new(BOX - BOX_BORDER, BOX - BOX_BORDER),
        );
        f.stroke(
            &kit::rounded(border, BOX_RADIUS - inset),
            Stroke::default()
                .with_color(faded(BORDER.get(), alpha))
                .with_width(BOX_BORDER),
        );
    }
}

/// The `+` at `rect`, lit under the pointer: a 14 px plus.
pub fn plus(f: &mut Frame, rect: Rectangle, lit: bool, alpha: f32) {
    if lit {
        f.fill(&kit::rounded(rect, ADD_RADIUS), faded(HOVER.get(), alpha));
    }
    let c = rect.center();
    let unit = PLUS / 24.0;
    let arm = 7.0 * unit;
    let cross = Path::new(|p| {
        p.move_to(Point::new(c.x, c.y - arm));
        p.line_to(Point::new(c.x, c.y + arm));
        p.move_to(Point::new(c.x - arm, c.y));
        p.line_to(Point::new(c.x + arm, c.y));
    });
    f.stroke(
        &cross,
        Stroke {
            width: 2.2 * unit,
            line_cap: canvas::LineCap::Round,
            ..Stroke::default().with_color(faded(DIM.get(), alpha))
        },
    );
}

/// A title at `at` (its line's middle), `size` px: struck through and
/// faint when ticked.
pub fn title(f: &mut Frame, said: &str, at: Point, size: f32, ticked: bool, alpha: f32) {
    let colour = if ticked { FAINT } else { TEXT };
    f.fill_text(kit::text(
        said.to_owned(),
        at,
        size,
        faded(colour.get(), alpha),
        Alignment::Left,
    ));
    if ticked {
        let width = kit::measure(said, size, MANROPE);
        f.fill_rectangle(
            Point::new(at.x, (at.y + 0.5).floor()),
            Size::new(width, 1.0),
            faded(FAINT.get(), alpha),
        );
    }
}

/// "Pas enregistré" in a title's place.
pub fn unsaved(f: &mut Frame, words: Words, at: Point, alpha: f32) {
    f.fill_text(kit::text(
        words.unsaved().to_owned(),
        at,
        UNSAVED,
        faded(UNSAVED_INK.get(), alpha),
        Alignment::Left,
    ));
}

/// The field at `rect`: what was typed (its end when it is too long) and
/// the caret while it holds the keyboard, else the placeholder.
pub fn field(
    f: &mut Frame,
    rect: Rectangle,
    (text, focused): (&str, bool),
    (size, radius, pad): (f32, f32, f32),
    words: Words,
    alpha: f32,
) {
    f.fill(&kit::rounded(rect, radius), faded(FIELD_FILL.get(), alpha));
    let inset = 0.5;
    let rim = Rectangle::new(
        Point::new(rect.x + inset, rect.y + inset),
        Size::new(rect.width - 1.0, rect.height - 1.0),
    );
    f.stroke(
        &kit::rounded(rim, radius - inset),
        Stroke::default()
            .with_color(faded(FIELD_BORDER.get(), alpha))
            .with_width(1.0),
    );
    let room = rect.width - 2.0 * pad;
    let left = rect.x + pad;
    let middle = rect.center_y();
    if text.is_empty() {
        f.fill_text(kit::text(
            words.placeholder().to_owned(),
            Point::new(left, middle),
            size,
            faded(FAINT.get(), alpha),
            Alignment::Left,
        ));
    }
    let width = if text.is_empty() {
        0.0
    } else {
        kit::measure(text, size, MANROPE)
    };
    // Its end shows: the text slides left past the field's room.
    let x = left + (room - width - 1.0).min(0.0);
    let inner = Rectangle::new(Point::new(left, rect.y), Size::new(room, rect.height));
    f.with_clip(inner, |f| {
        if !text.is_empty() {
            f.fill_text(kit::text(
                text.to_owned(),
                Point::new(x, middle),
                size,
                faded(TEXT.get(), alpha),
                Alignment::Left,
            ));
        }
        if focused {
            let caret = size + 1.0;
            f.fill_rectangle(
                Point::new(x + width, middle - caret / 2.0),
                Size::new(1.0, caret),
                faded(TEXT.get(), alpha),
            );
        }
    });
}

/// The mini form's box: a 16 px rounded square and its check, `text.dim`.
fn mini_icon(f: &mut Frame, at: Point, alpha: f32) {
    let unit = MINI_ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * unit, at.y + y * unit);
    let stroke = Stroke {
        width: 2.0 * unit,
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default().with_color(faded(DIM.get(), alpha))
    };
    let square = Rectangle::new(p(4.0, 4.0), Size::new(16.0 * unit, 16.0 * unit));
    f.stroke(&kit::rounded(square, 4.0 * unit), stroke);
    let check = Path::new(|path| {
        path.move_to(p(8.5, 12.5));
        path.line_to(p(11.0, 15.0));
        path.line_to(p(15.5, 9.5));
    });
    f.stroke(&check, stroke);
}

// --- The surface -----------------------------------------------------------------------

/// The to-do list of one bar.
#[derive(Debug, Clone)]
pub struct Group {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now.
    pub bar_width: f32,
    pub words: Words,
    /// Without glass: the bar's opaque gradient behind.
    pub backdrop: Option<crate::agents::draw::Backdrop>,
    pub bar: String,
}

/// What the pointer is over, besides the items.
#[derive(Debug, Clone, PartialEq)]
enum Over {
    Item(Item),
    Field,
    /// A cut title: its whole text, and where it is (the bar's frame).
    Title(String, Rectangle),
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: Option<Item>,
    title: Option<String>,
    last: Option<Point>,
    lines: Lines,
    laid: Laid,
    /// What it last said it shows.
    said: Option<String>,
}

impl Group {
    /// The form that shows alone (not switching): its items take the
    /// pointer.
    fn alone(&self) -> Option<Form> {
        match self.forms.as_slice() {
            [(form, _)] => Some(*form),
            _ => None,
        }
    }

    /// The group's top left on the surface.
    fn origin(&self, size: Size) -> Point {
        let left = size.width + 2.0 - self.bar_width;
        Point::new(left + SIDE, full_top(&self.bar) - top_of(&self.bar))
    }

    /// The mini item's top left on the surface, centred in the bar as
    /// wide as it is drawn.
    fn mini_origin(&self, size: Size) -> Point {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        Point::new(
            centre - MINI.width / 2.0,
            mini_top(&self.bar) - top_of(&self.bar),
        )
    }

    /// A point of the surface (`size`) in the bar's frame.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        Point::new(at.x - size.width - 1.0, top_of(&self.bar) + at.y)
    }

    fn in_bar_rect(&self, r: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(self.in_bar(r.position(), size), r.size())
    }

    /// Its items and where they are on the surface, in a form alone, once
    /// the surface took its size.
    fn items(&self, laid: &Laid, size: Size) -> Vec<(Item, Rectangle)> {
        let Some(form) = self.alone() else {
            return Vec::new();
        };
        if laid.height <= 0.0 {
            return Vec::new();
        }
        if form == Form::Mini {
            if laid.mini.is_none() || size.height + 0.5 < MINI.height + 2.0 * ROOM {
                return Vec::new();
            }
            return vec![(Item::Tasks, Rectangle::new(self.mini_origin(size), MINI))];
        }
        if size.height + 0.5 < resting_height(laid) + 2.0 * ROOM {
            return Vec::new();
        }
        let o = self.origin(size);
        let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
        if let Some(card) = &laid.card {
            return vec![
                (Item::DayChoose, at(card.choose)),
                (Item::DayLater, at(card.later)),
            ];
        }
        let mut items = vec![(Item::TaskAdd, at(add_rect()))];
        let boxes = laid.rows.iter().filter(|r| r.acts());
        for (slot, row) in (0..=u8::MAX).zip(boxes) {
            items.push((Item::Task(slot), at(button_rect(row.top))));
        }
        if let Some((_, rect)) = &laid.more {
            items.push((Item::TasksMore, at(*rect)));
        }
        items
    }

    /// What is under `at` on the surface.
    fn over(&self, laid: &Laid, at: Point, size: Size) -> Option<(Over, Rectangle)> {
        if let Some((item, rect)) = self
            .items(laid, size)
            .into_iter()
            .find(|(_, r)| r.contains(at))
        {
            return Some((Over::Item(item), rect));
        }
        if self.alone() != Some(Form::Full) {
            return None;
        }
        let o = self.origin(size);
        let at_group = Point::new(at.x - o.x, at.y - o.y);
        if let Some(rect) = laid.field.filter(|r| r.contains(at_group)) {
            return Some((Over::Field, rect));
        }
        laid.rows
            .iter()
            .filter(|r| r.cut.is_some())
            .find_map(|row| {
                let rect = Rectangle::new(
                    Point::new(title_x(), row.top),
                    Size::new(WIDTH - title_x(), row.height),
                );
                rect.contains(at_group).then(|| {
                    let on = Rectangle::new(Point::new(o.x + rect.x, o.y + rect.y), rect.size());
                    (Over::Title(row.title.clone(), on), on)
                })
            })
    }

    /// What it shows, laid out from `lines`, `most` of them at most.
    fn lay(&self, lines: &Lines, most: usize) -> Laid {
        let field = super::field().is_some_and(|f| f.at == At::Group(self.bar.clone()));
        // The mini form shows no card (Day.5's calendar carries the offer).
        let card = crate::setup::card_shows()
            && crate::column::groups().tasks
            && self.forms.iter().any(|(form, _)| *form == Form::Full);
        let card_words = crate::setup::Words::new(crate::clock::time::Lang::of_messages());
        let fonts = iced::advanced::graphics::text::font_system().write();
        match fonts {
            Ok(mut system) => {
                let mut width = |t: &str, size| kit::measure_in(system.raw(), t, size, MANROPE);
                let card = card.then(|| crate::setup::card::lay(card_words, &mut width));
                lay_out(lines, field, most, self.words, width, card)
            }
            Err(_) => {
                let width = |t: &str, size| t.chars().count() as f32 * size * 0.55;
                let card = card.then(|| crate::setup::card::lay(card_words, width));
                lay_out(lines, field, most, self.words, width, card)
            }
        }
    }

    /// What it shows changed: said, with its boxes' names, for tests.
    fn say(&self, state: &mut State) {
        let form = self.alone();
        let laid = &state.laid;
        let rows: Vec<String> = laid
            .rows
            .iter()
            .map(|r| {
                let name = self.words.tick(&r.title, r.look.ticked());
                match r.look {
                    Look::Unsaved { .. } => format!("{name}: {}", self.words.unsaved()),
                    _ => name,
                }
            })
            .collect();
        let said = format!(
            "form={} shows={} card={} add=«{}» rows=[{}] more={} empty={} count={} item=«{}»",
            form.map_or("switching", Form::name),
            laid.height > 0.0,
            laid.card.is_some(),
            self.words.add(),
            rows.join("; "),
            laid.more.as_ref().map_or("none", |(m, _)| m.as_str()),
            laid.empty.is_some(),
            laid.mini
                .map_or_else(|| "none".to_owned(), |n| n.to_string()),
            laid.mini.map(|n| self.words.count(n)).unwrap_or_default(),
        );
        if state.said.as_ref() == Some(&said) {
            return;
        }
        tracing::debug!(bar = %self.bar, "the tasks' group shows it: {said}");
        state.said = Some(said);
    }

    fn mini(&self, f: &mut Frame, laid: &Laid, o: Point, state: &State, alpha: f32) {
        let Some(count) = laid.mini else {
            return;
        };
        let rect = Rectangle::new(o, MINI);
        let (open, focused) = popover::with(|c| {
            (
                c.shows(&self.bar, Item::Tasks),
                c.focused(&self.bar, Item::Tasks),
            )
        });
        let lit = match (open, state.hover == Some(Item::Tasks)) {
            (true, _) => Some(OPEN),
            (false, true) => Some(HOVER),
            (false, false) => None,
        };
        if let Some(lit) = lit {
            f.fill(&kit::rounded(rect, MINI_RADIUS), faded(lit.get(), alpha));
        }
        if focused {
            ring(f, rect, MINI_RADIUS);
        }
        let said = (count > 0).then(|| count.to_string());
        let text_width = said
            .as_deref()
            .map_or(0.0, |s| kit::measure(s, MINI_TEXT, MONO) + MINI_GAP);
        let left = rect.center_x() - (MINI_ICON + text_width) / 2.0;
        mini_icon(
            f,
            Point::new(left, rect.center_y() - MINI_ICON / 2.0),
            alpha,
        );
        if let Some(said) = said {
            f.fill_text(kit::text_in(
                MONO,
                kit::text(
                    said,
                    Point::new(left + MINI_ICON + MINI_GAP, rect.center_y()),
                    MINI_TEXT,
                    faded(TEXT.get(), alpha),
                    Alignment::Left,
                ),
            ));
        }
    }

    fn group(&self, f: &mut Frame, laid: &Laid, o: Point, state: &State, alpha: f32) {
        let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
        let focused = |item| popover::with(|c| c.focused(&self.bar, item));
        if let Some(card) = &laid.card {
            let open = popover::with(|c| c.shows(&self.bar, Item::DayChoose));
            let lit = crate::setup::card::Lit {
                choose: open || state.hover == Some(Item::DayChoose),
                later: state.hover == Some(Item::DayLater),
                choose_ring: focused(Item::DayChoose),
                later_ring: focused(Item::DayLater),
            };
            let words = crate::setup::Words::new(crate::clock::time::Lang::of_messages());
            crate::setup::card::draw(f, card, words, o, lit, alpha);
            return;
        }
        // The header: its label, spaced, and the `+`.
        let mut x = o.x;
        for ch in self.words.header().chars() {
            let ch = ch.to_string();
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    ch.clone(),
                    Point::new(x, o.y + HEADER / 2.0),
                    LABEL,
                    faded(DIM.get(), alpha),
                    Alignment::Left,
                ),
            ));
            x += kit::measure(&ch, LABEL, MANROPE_BOLD) + LABEL_SPACING;
        }
        let add = at(add_rect());
        plus(f, add, state.hover == Some(Item::TaskAdd), alpha);
        if focused(Item::TaskAdd) {
            ring(f, add, ADD_RADIUS);
        }
        if let (Some(rect), Some(open)) = (laid.field, super::field()) {
            field(
                f,
                at(rect),
                (&open.text, open.focused),
                (TITLE, FIELD_RADIUS, FIELD_PAD),
                self.words,
                alpha,
            );
        }
        if let Some(middle) = laid.empty {
            f.fill_text(kit::text(
                self.words.nothing().to_owned(),
                Point::new(o.x, o.y + middle),
                TITLE,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
        }
        // The lines, clipped to each one's height: one closing or coming
        // in shows as far as it is. (A clip keeps the frame's coordinates.)
        let mut slot = 0u8;
        for row in &laid.rows {
            let a = alpha * row.alpha;
            let clip = Rectangle::new(
                Point::new(o.x - BUTTON_OUT - ROOM, o.y + row.top),
                Size::new(WIDTH + BUTTON_OUT + 2.0 * ROOM, row.height),
            );
            f.with_clip(clip, |f| {
                let button = at(button_rect(row.top));
                let item = Item::Task(slot);
                let mine = row.acts();
                let lit = mine && state.hover == Some(item);
                tick_box(f, button, row.look.ticked(), lit, a);
                if mine && popover::with(|c| c.focused(&self.bar, item)) {
                    ring(f, button, BUTTON_RADIUS);
                }
                let middle = Point::new(o.x + title_x(), o.y + row.top + LINE / 2.0);
                match row.look {
                    Look::Unsaved { .. } => unsaved(f, self.words, middle, a),
                    look => title(
                        f,
                        row.cut.as_deref().unwrap_or(&row.title),
                        middle,
                        TITLE,
                        look.ticked(),
                        a,
                    ),
                }
            });
            if row.acts() {
                slot = slot.saturating_add(1);
            }
        }
        if let Some((said, rect)) = &laid.more {
            let rect = at(*rect);
            let lit = state.hover == Some(Item::TasksMore)
                || popover::with(|c| c.shows(&self.bar, Item::TasksMore));
            if lit {
                f.fill(
                    &kit::rounded(rect, BUTTON_RADIUS),
                    faded(HOVER.get(), alpha),
                );
            }
            if focused(Item::TasksMore) {
                ring(f, rect, BUTTON_RADIUS);
            }
            f.fill_text(kit::text(
                said.clone(),
                Point::new(rect.x + MORE_OUT, rect.center_y()),
                MORE_TEXT,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
        }
    }

    fn mouse<M>(
        &self,
        state: &mut State,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let size = bounds.size();
        let at = pointer(event, bounds, cursor, state.pointer);
        state.pointer = at;
        let over = at.and_then(|at| self.over(&state.laid, at, size));
        let item = over.as_ref().and_then(|(o, r)| match o {
            Over::Item(item) => Some((*item, *r)),
            _ => None,
        });
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                // A cut title: shown whole after a rest.
                let title = over.as_ref().and_then(|(o, _)| match o {
                    Over::Title(text, rect) => Some((text.clone(), *rect)),
                    _ => None,
                });
                match &title {
                    Some((text, rect)) if state.title.as_ref() != Some(text) => {
                        crate::tray::tip::hover_text(
                            &self.bar,
                            self.in_bar_rect(*rect, size),
                            TIP,
                            text.clone(),
                            String::new(),
                        );
                    }
                    None if state.title.is_some() => crate::tray::tip::leave(),
                    _ => {}
                }
                state.title = title.map(|(t, _)| t);
                let now = item.map(|(i, _)| i);
                if now == state.hover {
                    if now.is_none() {
                        popover::event(popover::Event::Pointer { at: bar_at });
                    }
                    return None;
                }
                if state.hover.take().is_some() {
                    popover::event(popover::Event::ItemLeave { at: bar_at });
                }
                state.hover = now;
                if let Some((item, r)) = item {
                    popover::event(popover::Event::ItemEnter {
                        bar: self.bar.clone(),
                        item,
                        rect: self.in_bar_rect(r, size),
                    });
                }
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::CursorLeft => {
                if state.title.take().is_some() {
                    crate::tray::tip::leave();
                }
                state.hover.take()?;
                let at = state.last.unwrap_or_default();
                popover::event(popover::Event::ItemLeave { at });
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                if state.title.take().is_some() {
                    crate::tray::tip::leave();
                }
                match over {
                    Some((Over::Item(item), r)) => {
                        tracing::debug!(?item, "a press on the tasks' group");
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item,
                            rect: self.in_bar_rect(r, size),
                        });
                    }
                    // In the field: it takes the keyboard again.
                    Some((Over::Field, _)) => super::open_field(At::Group(self.bar.clone())),
                    _ => popover::event(popover::Event::PressElsewhere),
                }
                Some(canvas::Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }
}

impl Row {
    /// Whether its box acts: not on its way out (with motion), nor a task
    /// added whose card is not made yet.
    fn acts(&self) -> bool {
        self.height >= LINE && !matches!(self.key, Key::Adding(_))
    }
}

impl canvas::Program<Message> for Group {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                let lines = super::lines();
                // As many lines as its room in the column has (read
                // outside its lock).
                let most = most_for(&self.bar, &lines);
                let laid = self.lay(&lines, most);
                let heights = Heights {
                    full: resting_height(&laid),
                    least: least_of(&laid),
                    mini: laid.mini.is_some(),
                };
                // Its surface to the group's height, if it changed.
                let resized = places(|places| {
                    let place = places.get_mut(&self.bar)?;
                    place.most = most;
                    if place.heights == heights {
                        return None;
                    }
                    place.heights = heights;
                    Some(refit(place).map(|(size, margin)| popover::Effect::Resize {
                        id: place.id,
                        size,
                        margin: Some(margin),
                    }))
                });
                if let Some(resized) = resized {
                    popover::send(resized.into_iter().collect());
                    // Today over it keeps the room left, the agents'
                    // group under it moves (`crate::column`).
                    crate::column::moved();
                }
                takes_pointer(&self.bar, self.alone().is_some() && laid.height > 0.0);
                let keys: Vec<Key> = laid
                    .rows
                    .iter()
                    .filter(|r| r.acts())
                    .map(|r| r.key.clone())
                    .collect();
                SLOTS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert_with(HashMap::new)
                    .insert(self.bar.clone(), keys);
                let moving = lines.moving();
                state.lines = lines;
                state.laid = laid;
                self.say(state);
                moving.then(canvas::Action::request_redraw)
            }
            iced::Event::Mouse(event) => self.mouse(state, event, bounds, cursor),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                let pressed = Pressed {
                    key: key.clone(),
                    modifiers: *modifiers,
                    text: text.as_deref().map(str::to_owned),
                };
                let mine = super::field_key(&At::Group(self.bar.clone()), &pressed)
                    || popover::key(key, *modifiers, text.as_deref());
                mine.then(|| canvas::Action::request_redraw().and_capture())
            }
            _ => None,
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
        if let Some(backdrop) = self.backdrop {
            // At the group's (or the item's) middle, where the column has it
            // now, on the bar as tall as it is.
            let (top, tall) = match self.alone() {
                Some(Form::Mini) => (mini_top(&self.bar), MINI.height),
                _ => (full_top(&self.bar), height(&self.bar, Form::Full)),
            };
            let bar = popover::with(|c| c.bar_height(&self.bar));
            let colour = backdrop.at(top + tall / 2.0, bar);
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), colour);
        }
        let size = bounds.size();
        let laid = &state.laid;
        let in_bar: Vec<(Item, Rectangle)> = self
            .items(laid, size)
            .into_iter()
            .map(|(item, r)| (item, self.in_bar_rect(r, size)))
            .collect();
        log_items(&in_bar);
        popover::with(|c| c.set_items(&self.bar, TASKS_NAMESPACE, in_bar));
        if laid.height <= 0.0 {
            return vec![frame.into_geometry()];
        }
        // The bar's inside, as wide as it is while it switches.
        let left = (size.width + 2.0 - self.bar_width).max(0.0);
        let column = Rectangle::new(
            Point::new(left, 0.0),
            Size::new((size.width - left).max(0.0), size.height),
        );
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                match form {
                    Form::Full => self.group(f, laid, self.origin(size), state, alpha),
                    Form::Mini => self.mini(f, laid, self.mini_origin(size), state, alpha),
                }
            }
        });
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The items' middles in the bar's frame, logged when they change (for
/// tests, which click them).
fn log_items(items: &[(Item, Rectangle)]) {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let said: Vec<String> = items
        .iter()
        .map(|(item, r)| {
            let c = r.center();
            format!("{item:?}@{:.0},{:.0}", c.x, c.y)
        })
        .collect();
    let said = said.join(" ");
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if *last != said {
        tracing::debug!(items = %said, "the tasks' items");
        last.clone_from(&said);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::Line;
    use hocus_day::model::TaskId;

    fn line(id: &str, look: Look) -> Line {
        Line {
            key: Key::Task(TaskId::new(id)),
            title: format!("Task {id}"),
            look,
            going: None,
        }
    }

    fn laid(lines: Vec<Line>, field: bool) -> Laid {
        let lines = Lines { shows: true, lines };
        lay_out(
            &lines,
            field,
            GROUP_LINES,
            Words::new(crate::clock::time::Lang::French),
            |t, size| t.chars().count() as f32 * size * 0.5,
            None,
        )
    }

    #[test]
    fn the_group_is_18_px_and_22_px_a_task_and_20_px_for_the_rest() {
        let three: Vec<Line> = (0..3).map(|i| line(&i.to_string(), Look::Open)).collect();
        assert_eq!(laid(three.clone(), false).height, 18.0 + 3.0 * 22.0);
        let seven: Vec<Line> = (0..7).map(|i| line(&i.to_string(), Look::Open)).collect();
        let l = laid(seven, false);
        assert_eq!(l.rows.len(), 3);
        assert_eq!(l.height, 18.0 + 3.0 * 22.0 + 20.0);
        assert_eq!(l.more.as_ref().map(|m| m.0.as_str()), Some("+ 4 autres"));
        // The field first, 26 px with 2 px above and under.
        let l = laid(three, true);
        assert_eq!(l.field.map(|f| (f.y, f.height)), Some((20.0, 26.0)));
        assert_eq!(l.rows[0].top, 48.0);
        // Nothing open: the header and "Rien à faire.".
        let l = laid(Vec::new(), false);
        assert_eq!(l.height, 18.0 + 22.0);
        assert!(l.empty.is_some());
        assert_eq!(l.mini, Some(0));
        // No source: nothing.
        let none = lay_out(
            &Lines::default(),
            false,
            GROUP_LINES,
            Words::new(crate::clock::time::Lang::French),
            |_, _| 0.0,
            None,
        );
        assert_eq!(none.height, 0.0);
    }

    #[test]
    fn on_a_short_bar_its_lines_give_way_to_the_rest_its_header_staying() {
        // All of them when they fit, 3 at most.
        assert_eq!(lines_that_fit(2, false, f32::INFINITY), GROUP_LINES);
        assert_eq!(lines_that_fit(3, false, 18.0 + 3.0 * 22.0), GROUP_LINES);
        assert_eq!(
            lines_that_fit(7, false, 18.0 + 3.0 * 22.0 + 20.0),
            GROUP_LINES
        );
        // Else as many as fit over "+ N", the field counted.
        assert_eq!(lines_that_fit(3, false, 18.0 + 3.0 * 22.0 - 1.0), 2);
        assert_eq!(lines_that_fit(7, false, 18.0 + 22.0 + 20.0), 1);
        assert_eq!(lines_that_fit(7, true, 18.0 + 30.0 + 22.0 + 20.0), 1);
        assert_eq!(lines_that_fit(7, false, 18.0 + 20.0), 0);
        // Laid out with none: the header and "+ 7 autres", its least.
        let seven: Vec<Line> = (0..7).map(|i| line(&i.to_string(), Look::Open)).collect();
        let lines = Lines {
            shows: true,
            lines: seven,
        };
        let none = lay_out(
            &lines,
            false,
            0,
            Words::new(crate::clock::time::Lang::French),
            |t, size| t.chars().count() as f32 * size * 0.5,
            None,
        );
        assert!(none.rows.is_empty());
        assert_eq!(none.more.as_ref().map(|m| m.0.as_str()), Some("+ 7 autres"));
        assert_eq!(none.height, 18.0 + 20.0);
        let all = laid(lines.lines.clone(), false);
        assert_eq!(least_of(&all), 18.0 + 20.0);
        // Nothing open: the header and "Rien à faire.".
        assert_eq!(least_of(&laid(Vec::new(), false)), 18.0 + 22.0);
    }

    #[test]
    fn the_add_and_the_boxes_sit_4_px_past_the_groups_sides() {
        let add = add_rect();
        assert_eq!(add.x + add.width, WIDTH + 4.0);
        assert_eq!(add.size(), Size::new(22.0, 18.0));
        let button = button_rect(18.0);
        assert_eq!((button.x, button.y, button.width), (-4.0, 18.0, 22.0));
        // The box centred in it, from the group's left; the title 4 px
        // after the button.
        assert_eq!(button.center_x() - BOX / 2.0, 0.0);
        assert_eq!(title_x(), 22.0);
    }
}
