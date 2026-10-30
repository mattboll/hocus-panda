//! The desktops' surface, above the connections, as the approved 3.9
//! mockup draws it (bar.md §4 group 8, §5).
//!
//! Full form: a 30 px tile per desktop, four to a row, 6 px apart (rows
//! 6 px apart), 14 px above the connections; radius 6, `rgba(255,255,255,
//! 0.05)` (0.10 under the pointer). In each, its windows at their places on
//! the screen, 4 px in, radius 1.5, in stacking order, `rgba(236,239,244,
//! 0.20)`, the focused window of the current desktop 0.40, each with its
//! application's icon (10 px, centred, ringed 1 px in `rgba(14,17,28,0.6)`)
//! when it is 12 × 12 px at least (3.9b); the current desktop ringed 2 px
//! inside in the accent, the others 1 px in `rgba(255,255,255,0.10)`; an
//! empty one shows its number. Mini form: an
//! 18 × 13 dot per desktop, two to a row, 4 px apart, the current one
//! filled with the accent, those with windows `rgba(255,255,255,0.30)`,
//! the empty ones 0.10 with a 1 px `rgba(255,255,255,0.12)` ring.
//!
//! A click goes to that desktop, the wheel over them to the next or the
//! previous one, a notch a desktop; `Enter` or `Space` on the tile holding
//! the keyboard goes there. Resting on a tile (or a dot) shows the list of
//! its windows (3.9b, [`crate::popover::desktop`]), which the controller
//! opens. Nothing moves in them but the ring (the accent
//! dot), from the desktop left to the one shown, in the strip's 320 ms and
//! curve; none with reduced motion or on the CPU.
//!
//! It is a surface of its own, so that a desktop's change draws only it,
//! and nothing else's draws it. It follows the connections' group as that
//! grows or shrinks, as the connections follow the levels, and is as tall
//! as its rows.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{DESKTOPS_NAMESPACE, DeskWindow, DesktopTo, Desktops, EDGE, Form, Look, SHARE};
use iced::{
    Color, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, Style},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use crate::{
    clock::motion::bezier,
    levels::{
        draw::{ACCENT, FAINT, LINE, SIDE, faded, pointer, ring},
        model::Words,
    },
    popover::{self, Item, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// Around the tiles: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;
/// Between the bar's groups.
const GAP: f32 = 14.0;

const COLUMNS: usize = 4;
const TILE_HEIGHT: f32 = 30.0;
const TILE_GAP: f32 = 6.0;
const TILE_RADIUS: f32 = 6.0;
const TILE: Color = Color::from_rgba8(255, 255, 255, 0.05);
const TILE_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const TILE_RING: Color = Color::from_rgba8(255, 255, 255, 0.10);
const CURRENT_RING: f32 = 2.0;
/// The windows' sketch: 4 px inside the tile; each window half a pixel in
/// from its place, so that windows side by side on the screen (8 px apart,
/// a tenth of a pixel in the tile) show apart, as the mockup's do.
const INSET: f32 = 4.0;
const APART: f32 = 0.5;
const SKETCH_RADIUS: f32 = 1.5;
const SKETCH: Color = Color::from_rgba8(236, 239, 244, 0.20);
const SKETCH_FOCUSED: Color = Color::from_rgba8(236, 239, 244, 0.40);
/// A window's application's icon in its sketch (3.9b): 10 px, ringed 1 px
/// out, when the sketch is 12 × 12 px at least.
const ICON: f32 = 10.0;
const ICON_ROOM: f32 = 12.0;
const ICON_RADIUS: f32 = 3.0;
const ICON_RING: Color = Color::from_rgba8(14, 17, 28, 0.6);
/// An empty desktop's number: 11 px, 600, `text.faint`.
const NUMBER: f32 = 11.0;

const DOT_COLUMNS: usize = 2;
const DOT: Size = Size::new(18.0, 13.0);
const DOT_GAP: f32 = 4.0;
const DOT_RADIUS: f32 = 4.0;
const DOT_FULL: Color = Color::from_rgba8(255, 255, 255, 0.30);
const DOT_EMPTY: Color = Color::from_rgba8(255, 255, 255, 0.10);
const DOT_RING: Color = Color::from_rgba8(255, 255, 255, 0.12);

/// The ring's move: the strip's slide (windows.md §12).
const RING_MOVE: Duration = Duration::from_millis(320);
const STRIP: (f32, f32, f32, f32) = (0.22, 0.9, 0.3, 1.0);

const fn columns(form: Form) -> usize {
    match form {
        Form::Full => COLUMNS,
        Form::Mini => DOT_COLUMNS,
    }
}

/// The rows `count` desktops take in `form`: one at least.
fn rows(form: Form, count: usize) -> usize {
    count.div_ceil(columns(form)).max(1)
}

/// The height of `count` desktops' tiles (or dots) in `form`.
fn group_height(form: Form, count: usize) -> f32 {
    let rows = rows(form, count) as f32;
    match form {
        Form::Full => rows * TILE_HEIGHT + (rows - 1.0) * TILE_GAP,
        Form::Mini => rows * DOT.height + (rows - 1.0) * DOT_GAP,
    }
}

/// Desktop `index`'s tile (or dot) in `form`, its group's sides `left` and
/// `right` and its top `top` on the surface: rows from the top, filled from
/// the left; a tile's sides on whole pixels.
pub fn tile(form: Form, index: usize, (left, right): (f32, f32), top: f32) -> Rectangle {
    let (row, column) = (index / columns(form), index % columns(form));
    match form {
        Form::Full => {
            let width = (right - left - TILE_GAP * (COLUMNS - 1) as f32) / COLUMNS as f32;
            let from = left + column as f32 * (width + TILE_GAP);
            let (x0, x1) = (from.round(), (from + width).round());
            Rectangle::new(
                Point::new(x0, top + row as f32 * (TILE_HEIGHT + TILE_GAP)),
                Size::new(x1 - x0, TILE_HEIGHT),
            )
        }
        Form::Mini => {
            let width = DOT_COLUMNS as f32 * DOT.width + DOT_GAP;
            let x = ((left + right - width) / 2.0).round() + column as f32 * (DOT.width + DOT_GAP);
            Rectangle::new(
                Point::new(x, top + row as f32 * (DOT.height + DOT_GAP)),
                DOT,
            )
        }
    }
}

/// A window's sketch in `tile`: its share of the screen in the tile, 4 px
/// in, and half a pixel in from its place.
pub fn sketch(tile: Rectangle, rect: [u16; 4]) -> Rectangle {
    let share = |v: u16| f32::from(v) / f32::from(SHARE);
    let (width, height) = (tile.width - 2.0 * INSET, tile.height - 2.0 * INSET);
    let [x, y, w, h] = rect.map(share);
    Rectangle::new(
        Point::new(
            tile.x + INSET + x * width + APART,
            tile.y + INSET + y * height + APART,
        ),
        Size::new(
            (w * width - 2.0 * APART).max(1.0),
            (h * height - 2.0 * APART).max(1.0),
        ),
    )
}

// --- Where the surface goes ----------------------------------------------------

/// A bar's desktops' surface, as last placed.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    /// The connections' group's tops above the levels' surface's bottom
    /// (full, mini).
    tops: (f32, f32),
    /// The desktops shown.
    count: usize,
    /// The surface's bottom above the levels' surface's bottom.
    bottom: f32,
    asked: Option<((u32, u32), i32)>,
    /// Whether it takes the pointer, as last asked.
    input: Option<bool>,
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The bottom above the levels' surface's of a form's surface over
/// connections whose top is `top` there: 14 px over them less the ring's
/// room, on a whole pixel, so that the tiles' edges are too.
fn bottom_of(top: f32) -> f32 {
    (top + GAP - ROOM).round()
}

/// The surface's size and its bottom above the levels' surface's, for
/// `form` (both while the bar switches) over connections whose tops are
/// `tops`, for `count` desktops.
fn placement(
    form: Form,
    switching: bool,
    (full, mini): (f32, f32),
    count: usize,
) -> ((u32, u32), f32) {
    let height = |form| group_height(form, count) + 2.0 * ROOM;
    let (full_bottom, mini_bottom) = (bottom_of(full), bottom_of(mini));
    match (switching, form) {
        (true, _) => {
            let bottom = full_bottom.min(mini_bottom);
            let top = (full_bottom + height(Form::Full)).max(mini_bottom + height(Form::Mini));
            ((FULL_WIDTH, (top - bottom).ceil() as u32), bottom)
        }
        (false, Form::Full) => ((FULL_WIDTH, height(Form::Full).ceil() as u32), full_bottom),
        (false, Form::Mini) => ((MINI_WIDTH, height(Form::Mini).ceil() as u32), mini_bottom),
    }
}

/// The layer-shell margins of a surface `bottom` above the levels'.
fn margin(bottom: f32) -> (i32, i32, i32, i32) {
    let bottom = EDGE + crate::levels::draw::BOTTOM + bottom as i32;
    (0, EDGE + 1, bottom, 0)
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let tops = crate::network::draw::group_tops(bar);
    let count = super::snapshot(bar).map_or(0, |d| usize::from(d.count));
    places(|places| {
        let (size, bottom) = placement(form, false, tops, count);
        let margin = margin(bottom);
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                tops,
                count,
                bottom,
                asked: Some((size, margin.2)),
                input: Some(true),
            },
        );
        (size, margin)
    })
}

pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

/// The surface's new size and margins, if they change.
fn refit(place: &mut Place) -> Option<crate::network::draw::Placed> {
    let (size, bottom) = placement(place.form, place.switching, place.tops, place.count);
    place.bottom = bottom;
    let margin = margin(bottom);
    if place.asked == Some((size, margin.2)) {
        return None;
    }
    place.asked = Some((size, margin.2));
    Some((place.id, size, margin))
}

fn resize(placed: Option<crate::network::draw::Placed>) {
    if let Some((id, size, margin)) = placed {
        popover::send(vec![popover::Effect::Resize {
            id,
            size,
            margin: Some(margin),
        }]);
    }
}

/// Bar `bar` in `form`, switching or not: the surface's new size and
/// margins, if they change.
pub fn form(bar: &str, form: Form, switching: bool) -> Option<crate::network::draw::Placed> {
    places(|places| {
        let place = places.get_mut(bar)?;
        place.form = form;
        place.switching = switching;
        refit(place)
    })
}

/// Bar `bar`'s connections moved or changed rows: the surface follows,
/// moved by layer-shell actions (nothing else is drawn).
pub fn follow(bar: &str) {
    let tops = crate::network::draw::group_tops(bar);
    resize(places(|places| {
        let place = places.get_mut(bar)?;
        if (place.tops.0 - tops.0).abs() < 0.01 && (place.tops.1 - tops.1).abs() < 0.01 {
            return None;
        }
        place.tops = tops;
        refit(place)
    }));
}

/// Every bar's connections changed rows.
pub fn follow_all() {
    let bars: Vec<String> = places(|places| places.keys().cloned().collect());
    for bar in bars {
        follow(&bar);
    }
}

/// Bar `bar`'s screen has `count` desktops now: the surface to their rows.
pub fn count(bar: &str, count: usize) {
    resize(places(|places| {
        let place = places.get_mut(bar)?;
        place.count = count;
        refit(place)
    }));
}

/// Bar `bar`'s surface's middle above the levels' surface's bottom, for its
/// opaque backdrop.
pub fn middle(bar: &str) -> f32 {
    places(|places| {
        places.get(bar).map_or(300.0, |p| {
            p.bottom + p.asked.map_or(0.0, |((_, height), _)| height as f32 / 2.0)
        })
    })
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

/// Where bar `bar`'s desktops are: their surface's bottom above the levels'
/// surface's, and the connections' group's tops.
fn where_is(bar: &str) -> (f32, (f32, f32)) {
    places(|places| {
        places
            .get(bar)
            .map_or((20.0 + GAP - ROOM, (20.0, 0.0)), |p| (p.bottom, p.tops))
    })
}

// --- The surface ----------------------------------------------------------------

/// The desktops of one bar.
#[derive(Debug, Clone)]
pub struct Tiles {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now; the surface's right edge is the bar's
    /// inside right edge.
    pub bar_width: f32,
    /// The ring moves (else it changes at once).
    pub motion: bool,
    pub words: Words,
    /// Without glass: the bar's opaque colour behind.
    pub backdrop: Option<Color>,
    pub bar: String,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    shown: Option<Desktops>,
    /// The ring on its way to the current desktop: from where it was in
    /// each form (full, mini), since then.
    ring: Option<([Rectangle; 2], Instant)>,
    now: Option<Instant>,
    pointer: Option<Point>,
    hover: Option<u8>,
    last: Option<Point>,
    wheel: kit::Wheel,
}

impl State {
    fn count(&self) -> usize {
        self.shown.as_ref().map_or(0, |d| usize::from(d.count))
    }
}

impl<Message> canvas::Program<Message> for Tiles {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                self.refresh(state, bounds.size(), *now);
                state.ring.is_some().then(canvas::Action::request_redraw)
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
        let size = bounds.size();
        let items = self.items(state, size);
        let in_bar = items
            .iter()
            .map(|(item, r)| {
                (
                    *item,
                    Rectangle::new(self.in_bar(r.position(), size), r.size()),
                )
            })
            .collect();
        popover::with(|c| c.set_items(&self.bar, DESKTOPS_NAMESPACE, in_bar));
        let Some(shown) = &state.shown else {
            return vec![frame.into_geometry()];
        };
        let (right, left) = self.sides(size);
        let column = Rectangle::new(
            Point::new(left.max(0.0), 0.0),
            Size::new((right - left.max(0.0)).max(0.0), size.height),
        );
        let now = state.now.unwrap_or_else(Instant::now);
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                for index in 0..usize::from(shown.count) {
                    let rect = self.rect(form, size, index, usize::from(shown.count));
                    let hover = state.hover == Some(index as u8);
                    match form {
                        Form::Full => {
                            full(f, rect, shown, index, hover, state.ring.is_none(), alpha)
                        }
                        Form::Mini => mini(f, rect, shown, index, state.ring.is_none(), alpha),
                    }
                }
                // The ring on its way.
                if state.ring.is_some() {
                    let rect = self.ring(state, form, size, now);
                    match form {
                        Form::Full => {
                            inner_ring(f, rect, TILE_RADIUS, CURRENT_RING, faded(ACCENT, alpha))
                        }
                        Form::Mini => f.fill(
                            &Path::rounded_rectangle(
                                rect.position(),
                                rect.size(),
                                DOT_RADIUS.into(),
                            ),
                            faded(ACCENT, alpha),
                        ),
                    }
                }
            }
        });
        for (item, rect) in items {
            if popover::with(|c| c.focused(&self.bar, item)) {
                let radius = match self.forms.first() {
                    Some((Form::Mini, _)) => DOT_RADIUS,
                    _ => TILE_RADIUS,
                };
                ring(&mut frame, rect, radius);
            }
        }
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

/// A full form's tile: its ground, its ring, its windows, or its number.
fn full(
    f: &mut Frame,
    rect: Rectangle,
    shown: &Desktops,
    index: usize,
    hover: bool,
    ringed: bool,
    alpha: f32,
) {
    let ground = if hover { TILE_HOVER } else { TILE };
    f.fill(
        &Path::rounded_rectangle(rect.position(), rect.size(), TILE_RADIUS.into()),
        faded(ground, alpha),
    );
    let current = index == usize::from(shown.current);
    if current && ringed {
        inner_ring(f, rect, TILE_RADIUS, CURRENT_RING, faded(ACCENT, alpha));
    } else {
        inner_ring(f, rect, TILE_RADIUS, 1.0, faded(TILE_RING, alpha));
    }
    let mut windows = on(shown, index).peekable();
    if windows.peek().is_none() {
        f.fill_text(canvas::Text {
            content: (index + 1).to_string(),
            position: rect.center(),
            max_width: f32::INFINITY,
            color: faded(FAINT, alpha),
            size: Pixels(NUMBER),
            line_height: LineHeight::Relative(LINE),
            font: kit::MANROPE_SEMIBOLD,
            align_x: Alignment::Center,
            align_y: alignment::Vertical::Center,
            shaping: Shaping::Basic,
        });
        return;
    }
    for window in windows {
        let colour = if window.look == Look::Focused {
            SKETCH_FOCUSED
        } else {
            SKETCH
        };
        let at = sketch(rect, window.rect);
        f.fill(
            &Path::rounded_rectangle(at.position(), at.size(), SKETCH_RADIUS.into()),
            faded(colour, alpha),
        );
        if let Some(place) = icon_in(at)
            && let Some(icon) = super::icons::of(&window.app)
        {
            let half = 0.5;
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(place.x - half, place.y - half),
                    Size::new(place.width + 1.0, place.height + 1.0),
                    (ICON_RADIUS + half).into(),
                ),
                Stroke {
                    style: Style::Solid(faded(ICON_RING, alpha)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            kit::app_tile(f, place, (Some(&icon), ""), (ICON_RADIUS, 0.0), alpha);
        }
    }
}

/// Where a window's icon goes in its sketch `at`: in its middle, on whole
/// pixels; none in a sketch smaller than 12 × 12 px.
pub fn icon_in(at: Rectangle) -> Option<Rectangle> {
    if at.width < ICON_ROOM || at.height < ICON_ROOM {
        return None;
    }
    let centre = at.center();
    Some(Rectangle::new(
        Point::new(
            (centre.x - ICON / 2.0).round(),
            (centre.y - ICON / 2.0).round(),
        ),
        Size::new(ICON, ICON),
    ))
}

/// A mini form's dot: the accent where it is the current desktop (unless
/// the ring is on its way), lighter with windows, faint and ringed empty.
fn mini(f: &mut Frame, rect: Rectangle, shown: &Desktops, index: usize, ringed: bool, alpha: f32) {
    let path = Path::rounded_rectangle(rect.position(), rect.size(), DOT_RADIUS.into());
    if index == usize::from(shown.current) && ringed {
        f.fill(&path, faded(ACCENT, alpha));
    } else if on(shown, index).next().is_some() {
        f.fill(&path, faded(DOT_FULL, alpha));
    } else {
        f.fill(&path, faded(DOT_EMPTY, alpha));
        inner_ring(f, rect, DOT_RADIUS, 1.0, faded(DOT_RING, alpha));
    }
}

/// Desktop `index`'s windows, bottom first, minimised ones aside: only
/// its list shows them.
fn on(shown: &Desktops, index: usize) -> impl Iterator<Item = &DeskWindow> {
    shown
        .windows
        .iter()
        .filter(move |w| usize::from(w.desktop) == index && w.look != Look::Minimized)
}

/// A ring `width` wide inside `rect`'s edge rounded by `radius`, as CSS's
/// inset `box-shadow` draws it.
fn inner_ring(f: &mut Frame, rect: Rectangle, radius: f32, width: f32, colour: Color) {
    let half = width / 2.0;
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(rect.x + half, rect.y + half),
            Size::new(rect.width - width, rect.height - width),
            (radius - half).into(),
        ),
        Stroke {
            style: Style::Solid(colour),
            width,
            ..Stroke::default()
        },
    );
}

impl Tiles {
    /// The surface's right edge (the bar's inside) and the bar's inside
    /// left edge, in the surface.
    fn sides(&self, size: Size) -> (f32, f32) {
        let right = size.width;
        (right, right + 2.0 - self.bar_width)
    }

    /// Desktop `index`'s tile (or dot) of `form` on the surface, of
    /// `count`.
    fn rect(&self, form: Form, size: Size, index: usize, count: usize) -> Rectangle {
        let (right, left) = self.sides(size);
        let (bottom, (full, mini)) = where_is(&self.bar);
        // From the levels' surface's bottom to this surface's.
        let at = |top: f32| size.height - (bottom_of(top) + ROOM - bottom);
        let (sides, group_bottom) = match form {
            Form::Full => ((left + SIDE, right - SIDE), at(full)),
            Form::Mini => ((left, right), at(mini)),
        };
        tile(form, index, sides, group_bottom - group_height(form, count))
    }

    /// Where the ring is at `now` in `form`: on its way, or on the current
    /// desktop.
    fn ring(&self, state: &State, form: Form, size: Size, now: Instant) -> Rectangle {
        let Some(shown) = &state.shown else {
            return Rectangle::default();
        };
        let to = self.rect(form, size, usize::from(shown.current), state.count());
        let Some((from, since)) = state.ring else {
            return to;
        };
        let from = from[usize::from(form == Form::Mini)];
        let t = now.saturating_duration_since(since).as_secs_f32() / RING_MOVE.as_secs_f32();
        let p = bezier(STRIP, t.clamp(0.0, 1.0));
        let lerp = |a: f32, b: f32| a + (b - a) * p;
        Rectangle::new(
            Point::new(lerp(from.x, to.x), lerp(from.y, to.y)),
            Size::new(lerp(from.width, to.width), lerp(from.height, to.height)),
        )
    }

    /// The tiles and where they are, unless the bar is switching.
    fn items(&self, state: &State, size: Size) -> Vec<(Item, Rectangle)> {
        let [(form, _)] = self.forms.as_slice() else {
            return Vec::new();
        };
        let count = state.count();
        (0..count)
            .map(|i| {
                (
                    Item::Desktop(u8::try_from(i).unwrap_or(u8::MAX)),
                    self.rect(*form, size, i, count),
                )
            })
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

    /// What the compositor said last: the ring sets off when the current
    /// desktop changed, from where it is.
    fn refresh(&self, state: &mut State, size: Size, now: Instant) {
        if state
            .ring
            .is_some_and(|(_, since)| now.saturating_duration_since(since) >= RING_MOVE)
        {
            state.ring = None;
        }
        let desktops = super::snapshot(&self.bar);
        if desktops != state.shown {
            if let (Some(before), Some(after)) = (&state.shown, &desktops)
                && before.current != after.current
                && self.motion
            {
                let from = [Form::Full, Form::Mini].map(|form| self.ring(state, form, size, now));
                state.ring = Some((from, now));
            }
            if let Some(desktops) = &desktops {
                let said: Vec<String> = (0..usize::from(desktops.count))
                    .map(|i| self.words.desktop(i + 1, on(desktops, i).count()))
                    .collect();
                tracing::debug!(
                    bar = self.bar,
                    current = desktops.current + 1,
                    said = %said.join(" · "),
                    windows = ?desktops.windows,
                    "the desktops' tiles show it"
                );
            }
            state.shown = desktops;
        }
        state.now = Some(now);
        if state.hover.is_some_and(|h| usize::from(h) >= state.count()) {
            state.hover = None;
        }
        takes_pointer(&self.bar, state.count() > 0);
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
        let index = |item: Item| match item {
            Item::Desktop(i) => Some(i),
            _ => None,
        };
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                let now = over.and_then(|(item, _)| index(item));
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
            mouse::Event::WheelScrolled { delta } => {
                // Over the tiles and the gaps between them.
                let group = items.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b))?;
                if !at.is_some_and(|at| group.contains(at)) {
                    return None;
                }
                let notches = state.wheel.notches(delta, kit::scale(&self.bar));
                let to = if notches > 0 {
                    DesktopTo::Previous
                } else {
                    DesktopTo::Next
                };
                let effects = (0..notches.unsigned_abs())
                    .map(|_| popover::Effect::Desktop {
                        bar: self.bar.clone(),
                        to,
                    })
                    .collect::<Vec<_>>();
                if !effects.is_empty() {
                    popover::send(effects);
                }
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiles_go_four_to_a_row_and_their_windows_where_they_are_on_the_screen() {
        // The full form's group: 172 px between the bar's 12 px paddings.
        let sides = (11.0, 183.0);
        let first = tile(Form::Full, 0, sides, 5.0);
        assert_eq!(
            first,
            Rectangle::new(Point::new(11.0, 5.0), Size::new(39.0, 30.0))
        );
        // 38.5 px each, 6 apart, their sides on whole pixels; the last
        // ends on the group's side.
        let last = tile(Form::Full, 3, sides, 5.0);
        assert_eq!(last.x + last.width, 183.0);
        assert_eq!(tile(Form::Full, 1, sides, 5.0).x, 56.0);
        // Six: the fifth and the sixth on a second row, from the left.
        let fifth = tile(Form::Full, 4, sides, 5.0);
        assert_eq!((fifth.x, fifth.y), (11.0, 41.0));
        assert_eq!(group_height(Form::Full, 6), 66.0);
        assert_eq!(group_height(Form::Full, 9), 102.0);
        // The dots: two to a row, centred in the mini bar.
        let dots = (0.0, 54.0);
        assert_eq!(tile(Form::Mini, 0, dots, 0.0).x, 7.0);
        assert_eq!(
            tile(Form::Mini, 3, dots, 0.0),
            Rectangle::new(Point::new(29.0, 17.0), DOT)
        );
        assert_eq!(group_height(Form::Mini, 4), 30.0);
        // A window on the left half of the screen, and one over all of it:
        // 4 px inside the tile.
        // Half a pixel in from their places: the halves 1 px apart.
        let left = sketch(first, [0, 0, 500, 1000]);
        assert_eq!(
            left,
            Rectangle::new(Point::new(15.5, 9.5), Size::new(14.5, 21.0))
        );
        let right = sketch(first, [500, 0, 500, 1000]);
        assert_eq!(right.x - (left.x + left.width), 1.0);
        let whole = sketch(first, [0, 0, SHARE, SHARE]);
        assert_eq!(whole.x + whole.width, first.x + first.width - INSET - APART);
        assert_eq!(
            whole.y + whole.height,
            first.y + first.height - INSET - APART
        );
        // An icon in the left half's middle, on whole pixels; none in a
        // quarter, 6.5 px wide.
        assert_eq!(
            icon_in(left),
            Some(Rectangle::new(
                Point::new(18.0, 15.0),
                Size::new(10.0, 10.0)
            ))
        );
        assert_eq!(icon_in(sketch(first, [0, 0, 250, 1000])), None);
        assert_eq!(icon_in(sketch(first, [0, 0, 1000, 500])), None);
    }

    #[test]
    fn the_surface_sits_14_px_above_the_connections_and_grows_by_its_rows() {
        // The connections' top 200 px above the levels' surface's bottom
        // (full), 150 (mini).
        let tops = (200.0, 150.0);
        let (size, bottom) = placement(Form::Full, false, tops, 4);
        assert_eq!((size, bottom), ((194, 40), 209.0));
        assert_eq!(placement(Form::Full, false, tops, 6).0, (194, 76));
        let (size, bottom) = placement(Form::Mini, false, tops, 4);
        assert_eq!((size, bottom), ((54, 40), 159.0));
        // While switching, both forms.
        let (size, bottom) = placement(Form::Mini, true, tops, 4);
        assert_eq!((size, bottom), ((194, 90), 159.0));
        assert_eq!(margin(209.0), (0, 9, 8 + 41 + 209, 0));
    }
}
