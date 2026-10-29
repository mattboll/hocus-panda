//! The connections' surface, above the levels, as the 3.5 and 3.6 mockups
//! draw it (variant B, rows; bar.md §4 group 9, §4.3, §5).
//!
//! Full form: one 30 px row per connection, 2 px apart, like a level's (an
//! 18 px icon, the name at 14.5 px cut with an ellipsis), the lowest 14 px
//! above the levels' group: the Wi-Fi's, then Bluetooth's under it. The
//! Wi-Fi's icon lights one to four parts with the signal and its name
//! follows; else "Connexion…", "Hors ligne" or "Wi-Fi coupé" (the icon
//! slashed), or a cable and "Câble". A 7 px orange dot on the icon when a
//! sign-in page waits. Bluetooth's row is [`crate::bluetooth::draw`]'s.
//! Mini form: 44 × 38 buttons, 2 px apart, with a 20 px icon, 14 px above
//! the sound's gauge.
//!
//! It is a surface of its own, so that a connection's change draws only it,
//! and a level's never does. It is as tall as its rows (one when there is
//! none, taking no pointer then: the bar under it does). It follows the
//! levels' group as that grows or shrinks (a row coming, the status line on
//! two lines): the levels say where their top is ([`levels_moved`]) and the
//! surface moves, by its margins, without drawing anything else. A row
//! opens its popover (§9) as the sound's row does its own.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{EDGE, Form};
use iced::{
    Color, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::model::{Link, Network};
use crate::{
    bluetooth::{self, draw::RowState, model::Row},
    levels::{
        draw::{
            DIM, FAINT, HOVER, ITEM_HEIGHT, ITEM_OUT, ITEM_RADIUS, LINE, MANROPE, MINI_ITEM_RADIUS,
            MUTED_FILL, OPEN, SIDE, TEXT, faded, pointer, ring, stroke,
        },
        model::Words,
    },
    popover::{self, Item},
};

/// The surface with one row: as wide as the bar inside its rim; its row
/// (or button) with room around for the keyboard's ring. Each more row
/// adds its height and the 2 px between.
pub const FULL_SURFACE: (u32, u32) = (194, 40);
pub const MINI_SURFACE: (u32, u32) = (54, 48);
/// Around the items: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;
/// Between the bar's groups.
const GAP: f32 = 14.0;
/// Between two rows, or two buttons.
const ROW_GAP: f32 = 2.0;
const LABEL: f32 = 14.5;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const MINI_BUTTON: Size = Size::new(44.0, 38.0);
const MINI_ICON: f32 = 20.0;
/// The sign-in page's dot, and its ring of the bar's colour.
const DOT: f32 = 7.0;
const DOT_COLOUR: Color = crate::levels::draw::rgb(0xF2945B);
const DOT_RING: Color = Color::from_rgba8(14, 17, 28, 0.9);

/// The connections shown, from the top: the Wi-Fi's while NetworkManager
/// is there, Bluetooth's while BlueZ has an adapter.
pub fn shown(network: &Network, bluetooth: &bluetooth::Bluetooth) -> Vec<Item> {
    let mut items = Vec::new();
    if network.link() != Link::Hidden {
        items.push(Item::Wifi);
    }
    if bluetooth.row() != Row::Hidden {
        items.push(Item::Bluetooth);
    }
    items
}

// --- Where the surface goes ----------------------------------------------------

/// A bar's connections' surface, as last placed.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    /// The levels' group's tops above their surface's bottom (full, mini).
    tops: (f32, f32),
    /// The rows it is tall for: one at least.
    rows: usize,
    /// The surface's bottom above the levels' surface's bottom.
    bottom: f32,
    asked: Option<((u32, u32), i32)>,
    /// Whether it takes the pointer, as last asked.
    input: Option<bool>,
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

/// A surface placed: its id, its size and its layer-shell margins.
pub type Placed = (window::Id, (u32, u32), (i32, i32, i32, i32));

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The surface's height for `rows` rows in `form`.
fn height(form: Form, rows: usize) -> f32 {
    let more = rows.max(1) as f32 - 1.0;
    match form {
        Form::Full => FULL_SURFACE.1 as f32 + more * (ITEM_HEIGHT + ROW_GAP),
        Form::Mini => MINI_SURFACE.1 as f32 + more * (MINI_BUTTON.height + ROW_GAP),
    }
}

/// The surface's size and its bottom above the levels' surface's, for
/// `form` (both while the bar switches) over levels whose tops are `tops`,
/// tall for `rows` rows.
pub fn placement(
    form: Form,
    switching: bool,
    (full, mini): (f32, f32),
    rows: usize,
) -> ((u32, u32), f32) {
    let full_bottom = full + GAP - ROOM;
    let mini_bottom = mini + GAP - ROOM;
    let full_top = full_bottom + height(Form::Full, rows);
    let mini_top = mini_bottom + height(Form::Mini, rows);
    match (switching, form) {
        (true, _) => {
            let bottom = full_bottom.min(mini_bottom).floor();
            let top = full_top.max(mini_top);
            ((FULL_SURFACE.0, (top - bottom).ceil() as u32), bottom)
        }
        (false, Form::Full) => (
            (FULL_SURFACE.0, height(Form::Full, rows) as u32),
            full_bottom.floor(),
        ),
        (false, Form::Mini) => (
            (MINI_SURFACE.0, height(Form::Mini, rows) as u32),
            mini_bottom.floor(),
        ),
    }
}

/// The layer-shell margins of a surface `bottom` above the levels'.
fn margin(bottom: f32) -> (i32, i32, i32, i32) {
    let bottom = EDGE + crate::levels::draw::BOTTOM + bottom as i32;
    (0, EDGE + 1, bottom, 0)
}

/// The rows shown now, one at least for the surface's size.
fn rows_now() -> usize {
    shown(&super::snapshot(), &bluetooth::snapshot()).len()
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let rows = rows_now();
    places(|places| {
        let tops = places.get(bar).map_or((20.0, 0.0), |p| p.tops);
        let (size, bottom) = placement(form, false, tops, rows);
        let margin = margin(bottom);
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                tops,
                rows,
                bottom,
                asked: Some((size, margin.2)),
                // A surface takes the pointer everywhere until told.
                input: Some(true),
            },
        );
        (size, margin)
    })
}

pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

/// What bar `bar`'s surface needs now, if it changed: its id, size and
/// margins.
fn refit(place: &mut Place) -> Option<Placed> {
    let (size, bottom) = placement(place.form, place.switching, place.tops, place.rows);
    place.bottom = bottom;
    let margin = margin(bottom);
    if place.asked == Some((size, margin.2)) {
        return None;
    }
    place.asked = Some((size, margin.2));
    Some((place.id, size, margin))
}

/// Bar `bar` in `form`, switching or not: the surface's new size and
/// margins, if they change.
pub fn form(bar: &str, form: Form, switching: bool) -> Option<Placed> {
    places(|places| {
        let place = places.get_mut(bar)?;
        place.form = form;
        place.switching = switching;
        refit(place)
    })
}

/// Bar `bar`'s levels' group's tops are now `tops`: the surface follows,
/// moved by layer-shell actions (nothing else is drawn).
pub fn levels_moved(bar: &str, tops: (f32, f32)) {
    let moved = places(|places| {
        let place = places.get_mut(bar)?;
        if (place.tops.0 - tops.0).abs() < 0.01 && (place.tops.1 - tops.1).abs() < 0.01 {
            return None;
        }
        place.tops = tops;
        refit(place)
    });
    if let Some((id, size, margin)) = moved {
        tracing::debug!(bar, ?margin, "the connections follow the levels");
        popover::send(vec![popover::Effect::Resize {
            id,
            size,
            margin: Some(margin),
        }]);
    }
}

/// A connection came or went: every bar's surface to its rows' height.
pub fn rows_changed() {
    let rows = rows_now();
    let effects: Vec<popover::Effect> = places(|places| {
        places
            .values_mut()
            .filter_map(|place| {
                place.rows = rows;
                refit(place)
            })
            .map(|(id, size, margin)| popover::Effect::Resize {
                id,
                size,
                margin: Some(margin),
            })
            .collect()
    });
    if !effects.is_empty() {
        tracing::debug!(rows, "the connections' rows change");
        popover::send(effects);
    }
}

/// Bar `bar`'s surface takes the pointer (`takes`), or lets it through to
/// the bar where it draws no row: asked when it changes.
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
        tracing::debug!(bar, takes, "the connections' surface and the pointer");
        popover::send(vec![popover::Effect::Input { id, takes }]);
    }
}

/// Where bar `bar`'s connections are: their surface's bottom above the
/// levels' surface's, and the levels' group's tops.
fn where_is(bar: &str) -> (f32, (f32, f32)) {
    places(|places| {
        places
            .get(bar)
            .map_or((20.0 + GAP - ROOM, (20.0, 0.0)), |p| (p.bottom, p.tops))
    })
}

// --- The surface ----------------------------------------------------------------

/// The connections of one bar.
#[derive(Debug, Clone)]
pub struct Connections {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now; the surface's right edge is the bar's
    /// inside right edge.
    pub bar_width: f32,
    pub words: Words,
    /// Without glass: the bar's opaque colour behind.
    pub backdrop: Option<Color>,
    pub bar: String,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    link: Option<Link>,
    warns: bool,
    /// The label as it fits, and what it was made from.
    label: String,
    made_for: String,
    bluetooth: RowState,
    /// The rows shown, from the top.
    shown: Vec<Item>,
    pointer: Option<Point>,
    hover: Option<Item>,
    last: Option<Point>,
}

impl<Message> canvas::Program<Message> for Connections {
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
                self.refresh(state, &super::snapshot(), &bluetooth::snapshot());
                None
            }
            iced::Event::Mouse(event) => self.mouse(state, event, bounds, cursor),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
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
        if let Some(colour) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), colour);
        }
        if state.shown.is_empty() {
            return vec![frame.into_geometry()];
        }
        let size = bounds.size();
        let radius = match self.forms.first() {
            Some((Form::Mini, _)) => MINI_ITEM_RADIUS,
            _ => ITEM_RADIUS,
        };
        for (item, rect) in self.items(state, size) {
            let (open, focused) =
                popover::with(|c| (c.shows(&self.bar, item), c.focused(&self.bar, item)));
            let lit = if open {
                Some(OPEN)
            } else if state.hover == Some(item) {
                Some(HOVER)
            } else {
                None
            };
            if let Some(colour) = lit {
                frame.fill(
                    &Path::rounded_rectangle(rect.position(), rect.size(), radius.into()),
                    colour,
                );
            }
            if focused {
                ring(&mut frame, rect, radius);
            }
        }
        let (right, left) = self.sides(size);
        let column = Rectangle::new(
            Point::new(left.max(0.0), 0.0),
            Size::new((right - left.max(0.0)).max(0.0), size.height),
        );
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                for (slot, item) in state.shown.iter().rev().enumerate() {
                    let rect = self.rect(form, size, slot);
                    match (item, form) {
                        (Item::Wifi, Form::Full) => {
                            if let Some(link) = &state.link {
                                full(f, rect, link, state, alpha);
                            }
                        }
                        (Item::Wifi, Form::Mini) => {
                            if let Some(link) = &state.link {
                                mini(f, rect, link, state.warns, alpha);
                            }
                        }
                        (Item::Bluetooth, Form::Full) => {
                            bluetooth::draw::full(f, rect, &state.bluetooth, alpha);
                        }
                        (Item::Bluetooth, Form::Mini) => {
                            bluetooth::draw::mini(f, rect, &state.bluetooth, alpha);
                        }
                        (Item::Sound | Item::Brightness | Item::Battery, _) => {}
                    }
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

impl Connections {
    /// The surface's right edge (the bar's inside) and the bar's inside
    /// left edge, in the surface.
    fn sides(&self, size: Size) -> (f32, f32) {
        let right = size.width;
        (right, right + 2.0 - self.bar_width)
    }

    /// The row (full) or the button (mini) of `form` in `slot`, from the
    /// lowest (0), on the surface.
    fn rect(&self, form: Form, size: Size, slot: usize) -> Rectangle {
        let (right, left) = self.sides(size);
        let (bottom, (full, mini)) = where_is(&self.bar);
        // From the levels' surface's bottom to this surface's.
        let at = |above: f32| size.height - (above - bottom);
        let slot = slot as f32;
        match form {
            Form::Full => {
                let (from, to) = (left + SIDE - ITEM_OUT, right - SIDE + ITEM_OUT);
                let low = at(full + GAP) - slot * (ITEM_HEIGHT + ROW_GAP);
                Rectangle::new(
                    Point::new(from, low - ITEM_HEIGHT),
                    Size::new(to - from, ITEM_HEIGHT),
                )
            }
            Form::Mini => {
                let low = at(mini + GAP) - slot * (MINI_BUTTON.height + ROW_GAP);
                Rectangle::new(
                    Point::new(
                        (left + right - MINI_BUTTON.width) / 2.0,
                        low - MINI_BUTTON.height,
                    ),
                    MINI_BUTTON,
                )
            }
        }
    }

    /// The items shown and where they are, unless the bar is switching.
    fn items(&self, state: &State, size: Size) -> Vec<(Item, Rectangle)> {
        let [(form, _)] = self.forms.as_slice() else {
            return Vec::new();
        };
        state
            .shown
            .iter()
            .rev()
            .enumerate()
            .map(|(slot, item)| (*item, self.rect(*form, size, slot)))
            .collect()
    }

    /// A point of the surface (`size`) in the bar's frame.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        let (bottom, _) = where_is(&self.bar);
        let above = crate::levels::draw::BOTTOM as f32 + bottom;
        Point::new(
            at.x - size.width - 1.0,
            popover::with(|c| c.bar_height(&self.bar)) - above - size.height + at.y,
        )
    }

    /// What the rows show now, their labels cut to fit.
    fn refresh(&self, state: &mut State, network: &Network, bt: &bluetooth::Bluetooth) {
        let link = network.link();
        state.warns = network.warns();
        let label = self.words.label(&link).to_owned();
        if label != state.made_for {
            let room = FULL_SURFACE.0 as f32 - 2.0 * SIDE - ICON - ICON_GAP;
            state.label = match iced::advanced::graphics::text::font_system().write() {
                Ok(mut system) => crate::popover::sound::elide(&label, room, |t| {
                    crate::popover::sound::measure(system.raw(), t, MANROPE, LABEL)
                }),
                Err(_) => label.clone(),
            };
            state.made_for = label;
        }
        if state.link.as_ref() != Some(&link) {
            tracing::debug!(?link, "the connections' row shows it");
        }
        state.link = Some(link);
        let row = FULL_SURFACE.0 as f32 - 2.0 * SIDE + 2.0 * ITEM_OUT;
        state.bluetooth.refresh(self.words, bt, row);
        state.shown = shown(network, bt);
        if state.hover.is_some_and(|h| !state.shown.contains(&h)) {
            state.hover = None;
        }
        takes_pointer(&self.bar, !state.shown.is_empty());
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        let items = self.items(state, size);
        let at = pointer(event, bounds, cursor, state.pointer);
        state.pointer = at;
        let over = at.and_then(|at| items.iter().find(|(_, r)| r.contains(at)).copied());
        let rect =
            |item: Rectangle| Rectangle::new(self.in_bar(item.position(), size), item.size());
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                let now = over.map(|(item, _)| item);
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
                if let Some((item, r)) = over {
                    popover::event(popover::Event::ItemEnter {
                        bar: self.bar.clone(),
                        item,
                        rect: rect(r),
                    });
                }
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::CursorLeft => {
                state.hover.take()?;
                let at = state.last.unwrap_or_default();
                popover::event(popover::Event::ItemLeave { at });
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let event = match over {
                    Some((item, r)) => popover::Event::ItemPress {
                        bar: self.bar.clone(),
                        item,
                        rect: rect(r),
                    },
                    None => popover::Event::PressElsewhere,
                };
                popover::event(event);
                Some(canvas::Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }
}

impl Words {
    /// The row's words for `link`.
    pub fn label(self, link: &Link) -> &str {
        match link {
            Link::Hidden => "",
            Link::Wired => self.wired(),
            Link::Off => self.wifi_off(),
            Link::Offline => self.offline(),
            Link::Connecting => self.connecting(),
            Link::Connected { name, .. } => name,
        }
    }
}

/// The full form's row in `rect`: the icon, the label.
fn full(f: &mut Frame, rect: Rectangle, link: &Link, state: &State, alpha: f32) {
    let icon = Point::new(rect.x + ITEM_OUT, rect.center_y() - ICON / 2.0);
    glyph(f, icon, ICON, link, alpha);
    if state.warns {
        dot(
            f,
            Point::new(icon.x + ICON + 3.0 - DOT / 2.0, icon.y - 2.0 + DOT / 2.0),
            alpha,
        );
    }
    let colour = match link {
        Link::Connected { .. } | Link::Wired => TEXT,
        Link::Connecting => DIM,
        Link::Off | Link::Offline | Link::Hidden => FAINT,
    };
    f.fill_text(canvas::Text {
        content: state.label.clone(),
        position: Point::new(icon.x + ICON + ICON_GAP, rect.center_y()),
        max_width: f32::INFINITY,
        color: faded(colour, alpha),
        size: Pixels(LABEL),
        line_height: LineHeight::Relative(LINE),
        font: MANROPE,
        align_x: Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    });
}

/// The mini form's button in `rect`: the icon, 20 px.
fn mini(f: &mut Frame, rect: Rectangle, link: &Link, warns: bool, alpha: f32) {
    let icon = Point::new(
        rect.center_x() - MINI_ICON / 2.0,
        rect.center_y() - MINI_ICON / 2.0,
    );
    glyph(f, icon, MINI_ICON, link, alpha);
    if warns {
        // The mockup's: 9 px from the button's right, 7 from its top.
        let centre = Point::new(
            rect.x + rect.width - 9.0 - DOT / 2.0,
            rect.y + 7.0 + DOT / 2.0,
        );
        dot(f, centre, alpha);
    }
}

/// The row's icon for `link`, `size` px at `at`.
fn glyph(f: &mut Frame, at: Point, size: f32, link: &Link, alpha: f32) {
    match link {
        Link::Wired => ethernet(f, at, size, faded(TEXT, alpha)),
        Link::Connected { bars, .. } => wifi(f, at, size, *bars, faded(TEXT, alpha), false, alpha),
        Link::Connecting => wifi(f, at, size, 4, faded(DIM, alpha), false, alpha),
        Link::Off | Link::Offline | Link::Hidden => {
            wifi(f, at, size, 0, faded(TEXT, alpha), true, alpha);
        }
    }
}

pub(crate) fn dot(f: &mut Frame, centre: Point, alpha: f32) {
    f.fill(
        &Path::circle(centre, DOT / 2.0 + 2.0),
        faded(DOT_RING, alpha),
    );
    f.fill(&Path::circle(centre, DOT / 2.0), faded(DOT_COLOUR, alpha));
}

// --- Icons, from the mockup's SVGs (a 24-unit box) ------------------------------

/// The Wi-Fi's icon, `size` px at `at`: its dot and three arcs, the first
/// `bars` in `colour`, the others unlit; slashed when off or offline.
pub(crate) fn wifi(
    f: &mut Frame,
    at: Point,
    size: f32,
    bars: u8,
    colour: Color,
    slash: bool,
    alpha: f32,
) {
    let s = size / 24.0;
    let unlit = faded(MUTED_FILL, alpha);
    let lit = |n: u8| if bars >= n { colour } else { unlit };
    // `M2.5 9a13.5 13.5 0 0 1 19 0`, `M5.5 12.3a9.3 9.3 0 0 1 13 0`,
    // `M8.5 15.5a5 5 0 0 1 7 0`: arcs over their chords, bulging up.
    for (n, (x, y, chord, r)) in [
        (4, (2.5, 9.0, 19.0, 13.5)),
        (3, (5.5, 12.3, 13.0, 9.3)),
        (2, (8.5, 15.5, 7.0, 5.0)),
    ] {
        arc_up(f, at, s, (x, y), chord, r, stroke(2.2 * s, lit(n)));
    }
    f.fill(
        &Path::circle(Point::new(at.x + 12.0 * s, at.y + 19.0 * s), 1.6 * s),
        lit(1),
    );
    if slash {
        let line = Path::line(
            Point::new(at.x + 4.0 * s, at.y + 3.0 * s),
            Point::new(at.x + 20.0 * s, at.y + 21.0 * s),
        );
        f.stroke(&line, stroke(2.2 * s, faded(FAINT, alpha)));
    }
}

/// An SVG `a r r 0 0 1 chord 0` from `(x, y)`: the arc over the chord.
fn arc_up(
    f: &mut Frame,
    at: Point,
    s: f32,
    (x, y): (f32, f32),
    chord: f32,
    r: f32,
    stroke: canvas::Stroke<'_>,
) {
    let half = chord / 2.0;
    let depth = (r * r - half * half).max(0.0).sqrt();
    let angle = (half / r).asin();
    let up = -std::f32::consts::FRAC_PI_2;
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: Point::new(at.x + (x + half) * s, at.y + (y + depth) * s),
            radius: r * s,
            start_angle: Radians(up - angle),
            end_angle: Radians(up + angle),
        });
    });
    f.stroke(&arc, stroke);
}

/// The cable's icon: `rect x4 y3 w16 h12 rx2` and its plug.
pub(crate) fn ethernet(f: &mut Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    f.stroke(
        &Path::rounded_rectangle(p(4.0, 3.0), Size::new(16.0 * s, 12.0 * s), (2.0 * s).into()),
        line,
    );
    let path = Path::new(|b| {
        b.move_to(p(8.0, 15.0));
        b.line_to(p(8.0, 18.0));
        b.line_to(p(16.0, 18.0));
        b.line_to(p(16.0, 15.0));
        for (from, to) in [
            ((12.0, 18.0), (12.0, 21.0)),
            ((9.0, 7.0), (9.0, 10.0)),
            ((12.0, 7.0), (12.0, 10.0)),
            ((15.0, 7.0), (15.0, 10.0)),
        ] {
            b.move_to(p(from.0, from.1));
            b.line_to(p(to.0, to.1));
        }
    });
    f.stroke(&path, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surface_sits_14_px_above_the_levels_and_follows_them() {
        // Three rows and a one-line status line: the sound's item's top
        // 133 px above the levels' surface's bottom.
        let tops = (133.0, 83.1);
        let (size, bottom) = placement(Form::Full, false, tops, 1);
        assert_eq!(size, FULL_SURFACE);
        // The row's bottom 14 px above, its ring's room under it.
        assert_eq!(bottom, 142.0);
        let (size, bottom) = placement(Form::Mini, false, tops, 1);
        assert_eq!(size, MINI_SURFACE);
        assert_eq!(bottom, 92.0);
        // While switching, both.
        let (size, bottom) = placement(Form::Mini, true, tops, 1);
        assert_eq!(bottom, 92.0);
        assert_eq!(size, (194, 90));
        // The status line on two lines: up by its line.
        let (_, higher) = placement(Form::Full, false, (151.5, 83.1), 1);
        assert!(higher > 142.0);
        assert_eq!(margin(142.0), (0, 9, 8 + 41 + 142, 0));
        // None shown: as tall as one. Two: the second row 2 px above the
        // first, the bottom where it was.
        assert_eq!(placement(Form::Full, false, tops, 0).0, FULL_SURFACE);
        assert_eq!(
            placement(Form::Full, false, tops, 2),
            ((194, 40 + 32), 142.0)
        );
        assert_eq!(placement(Form::Mini, false, tops, 2), ((54, 48 + 40), 92.0));
        assert_eq!(placement(Form::Mini, true, tops, 2).0, (194, 50 + 72));
    }

    #[test]
    fn the_rows_words() {
        let fr = Words::new(crate::clock::time::Lang::French);
        assert_eq!(fr.label(&Link::Off), "Wi-Fi coupé");
        assert_eq!(fr.label(&Link::Connecting), "Connexion…");
        assert_eq!(
            fr.label(&Link::Connected {
                name: "Livebox-A3F2".to_owned(),
                bars: 3
            }),
            "Livebox-A3F2"
        );
        assert_eq!(fr.label(&Link::Wired), "Câble");
    }
}
