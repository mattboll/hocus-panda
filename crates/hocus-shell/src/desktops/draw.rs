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
//!
//! Several screens (8.4): one group of tiles per screen the bar shows, its
//! own first, 10 px apart, each shaped like its screen: the tiles keep
//! their width, their height follows the screen's (30 px at 16:10 and
//! narrower, down to 20 px at 21:9 and wider); in the mini form a block of
//! dots per screen, a wider screen's dots lower (18 × 9 at 21:9). No
//! label: the shape and the sketch say which screen. A click, `Enter` or
//! the wheel on a group goes to a desktop of its screen.
//!
//! A window moved over a tile (8.5 B): the tile lights,
//! `rgba(108,194,135,0.28)` with a 2 px `#7EE0A0` ring, as where the
//! window would go; the compositor says which (`drop-over`), knowing where
//! the tiles are as the surface says them (`desk-tiles`), each time they
//! move or change.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    DESKTOPS_NAMESPACE, DeskScreen, DeskTile, DeskWindow, DesktopTo, Desktops, EDGE, Form, Look,
    SHARE, ToCompositor,
};
use iced::{
    Color, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, Style},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use crate::theme::Ink;
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
const TILE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const TILE_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const TILE_RING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const CURRENT_RING: f32 = 2.0;
/// The windows' sketch: 4 px inside the tile; each window half a pixel in
/// from its place, so that windows side by side on the screen (8 px apart,
/// a tenth of a pixel in the tile) show apart, as the mockup's do.
const INSET: f32 = 4.0;
const APART: f32 = 0.5;
const SKETCH_RADIUS: f32 = 1.5;
const SKETCH: Ink = Ink::Dark(Color::from_rgba8(236, 239, 244, 0.20));
const SKETCH_FOCUSED: Ink = Ink::Dark(Color::from_rgba8(236, 239, 244, 0.40));
/// A window's application's icon in its sketch (3.9b): 10 px, ringed 1 px
/// out, when the sketch is 12 × 12 px at least.
const ICON: f32 = 10.0;
const ICON_ROOM: f32 = 12.0;
const ICON_RADIUS: f32 = 3.0;
const ICON_RING: Ink = Ink::Dark(Color::from_rgba8(14, 17, 28, 0.6));
/// An empty desktop's number: 11 px, 600, `text.faint`.
const NUMBER: f32 = 11.0;

const DOT_COLUMNS: usize = 2;
const DOT: Size = Size::new(18.0, 13.0);
const DOT_GAP: f32 = 4.0;
const DOT_RADIUS: f32 = 4.0;
const DOT_FULL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.30));
const DOT_EMPTY: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const DOT_RING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));

/// A tile a window would be let go on (8.5 B): lit green, ringed 2 px.
const DROP_FILL: Ink = Ink::Fixed(Color::from_rgba8(108, 194, 135, 0.28));
const DROP_RING: Ink = Ink::Fixed(Color::from_rgb8(0x7E, 0xE0, 0xA0));
const DROP_RING_WIDTH: f32 = 2.0;

/// Where each bar's tiles were last said to be, by the bar's screen.
static TOLD: Mutex<Option<HashMap<String, Vec<DeskTile>>>> = Mutex::new(None);

/// Bar `bar`'s tiles are `tiles` on its surface (as shown, `shown`): the
/// compositor is told when that changed, for a window let go on one.
fn tell_tiles(bar: &str, tiles: &[(Item, Rectangle)], shown: &[Shown]) {
    let tiles: Vec<DeskTile> = tiles
        .iter()
        .filter_map(|(item, r)| {
            let Item::Desktop { row, index } = item else {
                return None;
            };
            Some(DeskTile {
                output: shown.get(usize::from(*row))?.screen.output.clone(),
                desktop: *index,
                rect: [
                    r.x.round() as i32,
                    r.y.round() as i32,
                    r.width.round() as i32,
                    r.height.round() as i32,
                ],
            })
        })
        .collect();
    let mut told = TOLD.lock().unwrap_or_else(PoisonError::into_inner);
    let told = told.get_or_insert_with(HashMap::new);
    if told.get(bar) == Some(&tiles) {
        return;
    }
    told.insert(bar.to_owned(), tiles.clone());
    popover::tell(ToCompositor::DeskTiles {
        bar: bar.to_owned(),
        tiles,
    });
}

/// Between two screens' groups (8.4), in both forms.
const SCREEN_GAP: f32 = 10.0;
/// A screen's tiles' height at its narrowest and widest (8.4): 30 px at
/// 16:10 (and narrower), 20 px at 21:9 (and wider).
const TILE_LOWEST: f32 = 20.0;
const TILE_ASPECT: f32 = 1.6;

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

/// A screen's desktops in a bar (8.4): how many, and its tiles' height,
/// after its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Group {
    pub count: usize,
    pub tile: f32,
}

impl Group {
    /// A screen's group: its desktops, if said yet.
    fn of(screen: &DeskScreen, desktops: Option<&Desktops>) -> Self {
        Self {
            count: desktops.map_or(0, |d| usize::from(d.count)),
            tile: tile_height(screen.width, screen.height),
        }
    }

    /// Its tiles' (or dots') height in `form`.
    fn height_in(self, form: Form) -> f32 {
        match form {
            Form::Full => self.tile,
            Form::Mini => (DOT.height * self.tile / TILE_HEIGHT).round(),
        }
    }

    /// Its height in `form`, rows and the gaps between them.
    fn total(self, form: Form) -> f32 {
        let rows = rows(form, self.count) as f32;
        let gap = match form {
            Form::Full => TILE_GAP,
            Form::Mini => DOT_GAP,
        };
        rows * self.height_in(form) + (rows - 1.0) * gap
    }
}

/// A screen's tiles' height (8.4): 30 px at 16:10 and narrower, lower as
/// it is wider, 20 px at 21:9 and wider, on a whole pixel; 30 px when its
/// size is not known.
pub fn tile_height(width: u32, height: u32) -> f32 {
    if width == 0 || height == 0 {
        return TILE_HEIGHT;
    }
    let aspect = width as f32 / height as f32;
    (TILE_HEIGHT * TILE_ASPECT / aspect)
        .round()
        .clamp(TILE_LOWEST, TILE_HEIGHT)
}

/// The height of the groups' tiles (or dots) in `form`, 10 px between two
/// screens'; a row's when there is none.
fn group_height(form: Form, groups: &[Group]) -> f32 {
    if groups.is_empty() {
        return Group {
            count: 0,
            tile: TILE_HEIGHT,
        }
        .total(form);
    }
    groups.iter().map(|g| g.total(form)).sum::<f32>() + SCREEN_GAP * (groups.len() - 1) as f32
}

/// Group `at`'s top below the first's.
fn group_top(form: Form, groups: &[Group], at: usize) -> f32 {
    groups
        .iter()
        .take(at)
        .map(|g| g.total(form) + SCREEN_GAP)
        .sum()
}

/// Desktop `index`'s tile (or dot) in `form`, `height` high, its group's
/// sides `left` and `right` and its top `top` on the surface: rows from
/// the top, filled from the left; a tile's sides on whole pixels.
pub fn tile(
    form: Form,
    index: usize,
    (left, right): (f32, f32),
    top: f32,
    height: f32,
) -> Rectangle {
    let (row, column) = (index / columns(form), index % columns(form));
    match form {
        Form::Full => {
            let width = (right - left - TILE_GAP * (COLUMNS - 1) as f32) / COLUMNS as f32;
            let from = left + column as f32 * (width + TILE_GAP);
            let (x0, x1) = (from.round(), (from + width).round());
            Rectangle::new(
                Point::new(x0, top + row as f32 * (height + TILE_GAP)),
                Size::new(x1 - x0, height),
            )
        }
        Form::Mini => {
            let width = DOT_COLUMNS as f32 * DOT.width + DOT_GAP;
            let x = ((left + right - width) / 2.0).round() + column as f32 * (DOT.width + DOT_GAP);
            Rectangle::new(
                Point::new(x, top + row as f32 * (height + DOT_GAP)),
                Size::new(DOT.width, height),
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
#[derive(Debug, Clone)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    /// The connections' group's tops above the levels' surface's bottom
    /// (full, mini).
    tops: (f32, f32),
    /// The screens' desktops shown.
    groups: Vec<Group>,
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
/// `tops`, for the screens' desktops `groups`.
fn placement(
    form: Form,
    switching: bool,
    (full, mini): (f32, f32),
    groups: &[Group],
) -> ((u32, u32), f32) {
    let height = |form| group_height(form, groups) + 2.0 * ROOM;
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

/// The groups bar `bar` shows: a screen's each, its own first (8.4).
fn groups_of(bar: &str) -> Vec<Group> {
    shaped(
        super::screens(bar)
            .iter()
            .map(|s| Group::of(s, super::snapshot(&s.output).as_ref()))
            .collect(),
    )
}

/// The groups as drawn: shaped like their screens when there are several
/// to tell apart; one alone keeps the 30 px tiles (and 13 px dots) of 3.9.
fn shaped(mut groups: Vec<Group>) -> Vec<Group> {
    if groups.len() < 2 {
        for g in &mut groups {
            g.tile = TILE_HEIGHT;
        }
    }
    groups
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let tops = crate::network::draw::group_tops(bar);
    let groups = groups_of(bar);
    places(|places| {
        let (size, bottom) = placement(form, false, tops, &groups);
        let margin = margin(bottom);
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                tops,
                groups,
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
    let (size, bottom) = placement(place.form, place.switching, place.tops, &place.groups);
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
        // The column's room ends over the desktops (Day.3).
        crate::column::moved();
    }
}

/// How far above bar `bar`'s bottom its desktops' group's top is, in the
/// full form (the column's room ends 14 px over it, `crate::column`).
pub fn top_above_bottom(bar: &str) -> Option<f32> {
    top_above_bottom_in(bar, Form::Full)
}

/// How far above bar `bar`'s bottom its desktops' tiles' (or dots') top is
/// in `form` (the windows sit 14 px over it, `crate::windows`).
pub fn top_above_bottom_in(bar: &str, form: Form) -> Option<f32> {
    places(|places| {
        let place = places.get(bar)?;
        let (_, bottom) = placement(form, false, place.tops, &place.groups);
        Some(crate::levels::draw::BOTTOM as f32 + bottom + ROOM + group_height(form, &place.groups))
    })
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

/// Bar `bar`'s screens or their desktops changed: the surface to their
/// rows.
pub fn rows_changed(bar: &str) {
    let groups = groups_of(bar);
    resize(places(|places| {
        let place = places.get_mut(bar)?;
        place.groups = groups;
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

/// A screen's desktops as the surface shows them: the screen, and its
/// desktops once said.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    screen: DeskScreen,
    desktops: Option<Desktops>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// A row per screen, the bar's own first (8.4).
    shown: Vec<Shown>,
    /// Their names as said with them (6.14).
    names: Vec<Option<String>>,
    /// Each screen's ring on its way to its current desktop: from where it
    /// was in each form (full, mini), since then, by the screen's name.
    rings: HashMap<String, ([Rectangle; 2], Instant)>,
    now: Option<Instant>,
    pointer: Option<Point>,
    /// The tile under the pointer: its row and its desktop.
    hover: Option<(u8, u8)>,
    last: Option<Point>,
    wheel: kit::Wheel,
}

impl State {
    /// The screens' groups as shown.
    fn groups(&self) -> Vec<Group> {
        shaped(
            self.shown
                .iter()
                .map(|s| Group::of(&s.screen, s.desktops.as_ref()))
                .collect(),
        )
    }

    /// Whether any desktop is said.
    fn any(&self) -> bool {
        self.shown
            .iter()
            .any(|s| s.desktops.as_ref().is_some_and(|d| d.count > 0))
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
                (!state.rings.is_empty()).then(canvas::Action::request_redraw)
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
        // While the bar switches forms there are none to let a window go on.
        tell_tiles(&self.bar, &items, &state.shown);
        if !state.any() {
            return vec![frame.into_geometry()];
        }
        let (right, left) = self.sides(size);
        let column = Rectangle::new(
            Point::new(left.max(0.0), 0.0),
            Size::new((right - left.max(0.0)).max(0.0), size.height),
        );
        let now = state.now.unwrap_or_else(Instant::now);
        let groups = state.groups();
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                for (row, shown) in state.shown.iter().enumerate() {
                    let Some(desktops) = &shown.desktops else {
                        continue;
                    };
                    let moving = state.rings.contains_key(&desktops.output);
                    for index in 0..usize::from(desktops.count) {
                        let rect = self.rect(form, size, (row, index), &groups);
                        let hover = state.hover == Some((row as u8, index as u8));
                        match form {
                            Form::Full => full(f, rect, desktops, index, hover, !moving, alpha),
                            Form::Mini => mini(f, rect, desktops, index, !moving, alpha),
                        }
                    }
                    // The tile a moved window is over (8.5 B).
                    if let Some((output, index)) = super::drop_on(&self.bar)
                        && output == desktops.output
                        && index < desktops.count
                    {
                        let rect = self.rect(form, size, (row, usize::from(index)), &groups);
                        let radius = match form {
                            Form::Full => TILE_RADIUS,
                            Form::Mini => DOT_RADIUS,
                        };
                        f.fill(
                            &Path::rounded_rectangle(rect.position(), rect.size(), radius.into()),
                            faded(DROP_FILL.get(), alpha),
                        );
                        inner_ring(
                            f,
                            rect,
                            radius,
                            DROP_RING_WIDTH,
                            faded(DROP_RING.get(), alpha),
                        );
                    }
                    // The ring on its way.
                    if moving {
                        let rect = self.ring(state, form, size, row, now);
                        match form {
                            Form::Full => inner_ring(
                                f,
                                rect,
                                TILE_RADIUS,
                                CURRENT_RING,
                                faded(ACCENT.get(), alpha),
                            ),
                            Form::Mini => f.fill(
                                &Path::rounded_rectangle(
                                    rect.position(),
                                    rect.size(),
                                    DOT_RADIUS.into(),
                                ),
                                faded(ACCENT.get(), alpha),
                            ),
                        }
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
        faded(ground.get(), alpha),
    );
    let current = index == usize::from(shown.current);
    if current && ringed {
        inner_ring(
            f,
            rect,
            TILE_RADIUS,
            CURRENT_RING,
            faded(ACCENT.get(), alpha),
        );
    } else {
        inner_ring(f, rect, TILE_RADIUS, 1.0, faded(TILE_RING.get(), alpha));
    }
    let mut windows = on(shown, index).peekable();
    if windows.peek().is_none() {
        f.fill_text(canvas::Text {
            content: (index + 1).to_string(),
            position: rect.center(),
            max_width: f32::INFINITY,
            color: faded(FAINT.get(), alpha),
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
            faded(colour.get(), alpha),
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
                    style: Style::Solid(faded(ICON_RING.get(), alpha)),
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
        f.fill(&path, faded(ACCENT.get(), alpha));
    } else if on(shown, index).next().is_some() {
        f.fill(&path, faded(DOT_FULL.get(), alpha));
    } else {
        f.fill(&path, faded(DOT_EMPTY.get(), alpha));
        inner_ring(f, rect, DOT_RADIUS, 1.0, faded(DOT_RING.get(), alpha));
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

    /// Row `row`'s desktop `index`'s tile (or dot) of `form` on the
    /// surface, of the screens' `groups`.
    fn rect(
        &self,
        form: Form,
        size: Size,
        (row, index): (usize, usize),
        groups: &[Group],
    ) -> Rectangle {
        let (right, left) = self.sides(size);
        let (bottom, (full, mini)) = where_is(&self.bar);
        // From the levels' surface's bottom to this surface's.
        let at = |top: f32| size.height - (bottom_of(top) + ROOM - bottom);
        let (sides, group_bottom) = match form {
            Form::Full => ((left + SIDE, right - SIDE), at(full)),
            Form::Mini => ((left, right), at(mini)),
        };
        let top = group_bottom - group_height(form, groups) + group_top(form, groups, row);
        let height = groups.get(row).map_or(TILE_HEIGHT, |g| g.height_in(form));
        tile(form, index, sides, top, height)
    }

    /// Where row `row`'s ring is at `now` in `form`: on its way, or on its
    /// current desktop.
    fn ring(&self, state: &State, form: Form, size: Size, row: usize, now: Instant) -> Rectangle {
        let Some(desktops) = state.shown.get(row).and_then(|s| s.desktops.as_ref()) else {
            return Rectangle::default();
        };
        let to = self.rect(
            form,
            size,
            (row, usize::from(desktops.current)),
            &state.groups(),
        );
        let Some((from, since)) = state.rings.get(&desktops.output) else {
            return to;
        };
        let from = from[usize::from(form == Form::Mini)];
        let t = now.saturating_duration_since(*since).as_secs_f32() / RING_MOVE.as_secs_f32();
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
        let groups = state.groups();
        state
            .shown
            .iter()
            .enumerate()
            .flat_map(|(row, shown)| {
                let count = shown.desktops.as_ref().map_or(0, |d| usize::from(d.count));
                (0..count).map(move |i| (row, i))
            })
            .map(|(row, i)| {
                (
                    Item::Desktop {
                        row: u8::try_from(row).unwrap_or(u8::MAX),
                        index: u8::try_from(i).unwrap_or(u8::MAX),
                    },
                    self.rect(*form, size, (row, i), &groups),
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

    /// What the compositor said last: a screen's ring sets off when its
    /// current desktop changed, from where it is.
    fn refresh(&self, state: &mut State, size: Size, now: Instant) {
        state
            .rings
            .retain(|_, (_, since)| now.saturating_duration_since(*since) < RING_MOVE);
        let shown: Vec<Shown> = super::screens(&self.bar)
            .into_iter()
            .map(|screen| {
                let desktops = super::snapshot(&screen.output);
                Shown { screen, desktops }
            })
            .collect();
        let count = shown
            .iter()
            .filter_map(|s| s.desktops.as_ref())
            .map(|d| usize::from(d.count))
            .max()
            .unwrap_or(0);
        let names: Vec<Option<String>> = (0..count).map(super::name).collect();
        if shown != state.shown || names != state.names {
            if self.motion {
                for (row, after) in shown.iter().enumerate() {
                    let before = state
                        .shown
                        .iter()
                        .position(|s| s.screen.output == after.screen.output);
                    let (Some(before), Some(now_said)) = (before, &after.desktops) else {
                        continue;
                    };
                    let Some(was) = &state.shown[before].desktops else {
                        continue;
                    };
                    if was.current != now_said.current && before == row {
                        let from = [Form::Full, Form::Mini]
                            .map(|form| self.ring(state, form, size, row, now));
                        state.rings.insert(now_said.output.clone(), (from, now));
                    }
                }
            }
            // A line per screen's row, its own first.
            for (row, s) in shown.iter().enumerate() {
                let Some(d) = &s.desktops else {
                    continue;
                };
                let said: Vec<String> = (0..usize::from(d.count))
                    .map(|i| {
                        let name = names.get(i).cloned().flatten();
                        let windows = on(d, i).count();
                        if row == 0 {
                            self.words.desktop(i + 1, name.as_deref(), windows)
                        } else {
                            let screen = (s.screen.built_in, s.screen.inches, s.screen.number);
                            self.words
                                .screen_desktop(screen, i + 1, name.as_deref(), windows)
                        }
                    })
                    .collect();
                tracing::debug!(
                    bar = self.bar,
                    current = d.current + 1,
                    screen = d.output,
                    row,
                    said = %said.join(" · "),
                    windows = ?d.windows,
                    "the desktops' tiles show it"
                );
            }
            state.shown = shown;
            state.names = names;
        }
        state.now = Some(now);
        if state.hover.is_some_and(|(row, index)| {
            state
                .shown
                .get(usize::from(row))
                .and_then(|s| s.desktops.as_ref())
                .is_none_or(|d| index >= d.count)
        }) {
            state.hover = None;
        }
        takes_pointer(&self.bar, state.any());
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
        let tile = |item: Item| match item {
            Item::Desktop { row, index } => Some((row, index)),
            _ => None,
        };
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                let now = over.and_then(|(item, _)| tile(item));
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
                // Over a screen's tiles and the gaps between them: that
                // screen's desktops.
                let at = at?;
                let row = (0..state.shown.len()).find(|row| {
                    items
                        .iter()
                        .filter(|(item, _)| {
                            tile(*item).is_some_and(|(r, _)| usize::from(r) == *row)
                        })
                        .map(|(_, r)| *r)
                        .reduce(|a, b| a.union(&b))
                        .is_some_and(|group| group.contains(at))
                })?;
                let output = state.shown[row].screen.output.clone();
                let notches = state.wheel.notches(delta, kit::scale(&self.bar));
                let to = if notches > 0 {
                    DesktopTo::Previous
                } else {
                    DesktopTo::Next
                };
                let effects = (0..notches.unsigned_abs())
                    .map(|_| popover::Effect::Desktop {
                        bar: output.clone(),
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

    /// A 16:10 screen's `count` desktops.
    fn one(count: usize) -> Group {
        Group {
            count,
            tile: TILE_HEIGHT,
        }
    }

    #[test]
    fn the_tiles_go_four_to_a_row_and_their_windows_where_they_are_on_the_screen() {
        // The full form's group: 172 px between the bar's 12 px paddings.
        let sides = (11.0, 183.0);
        let first = tile(Form::Full, 0, sides, 5.0, TILE_HEIGHT);
        assert_eq!(
            first,
            Rectangle::new(Point::new(11.0, 5.0), Size::new(39.0, 30.0))
        );
        // 38.5 px each, 6 apart, their sides on whole pixels; the last
        // ends on the group's side.
        let last = tile(Form::Full, 3, sides, 5.0, TILE_HEIGHT);
        assert_eq!(last.x + last.width, 183.0);
        assert_eq!(tile(Form::Full, 1, sides, 5.0, TILE_HEIGHT).x, 56.0);
        // Six: the fifth and the sixth on a second row, from the left.
        let fifth = tile(Form::Full, 4, sides, 5.0, TILE_HEIGHT);
        assert_eq!((fifth.x, fifth.y), (11.0, 41.0));
        assert_eq!(group_height(Form::Full, &[one(6)]), 66.0);
        assert_eq!(group_height(Form::Full, &[one(9)]), 102.0);
        // The dots: two to a row, centred in the mini bar.
        let dots = (0.0, 54.0);
        assert_eq!(tile(Form::Mini, 0, dots, 0.0, DOT.height).x, 7.0);
        assert_eq!(
            tile(Form::Mini, 3, dots, 0.0, DOT.height),
            Rectangle::new(Point::new(29.0, 17.0), DOT)
        );
        assert_eq!(group_height(Form::Mini, &[one(4)]), 30.0);
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
        let (size, bottom) = placement(Form::Full, false, tops, &[one(4)]);
        assert_eq!((size, bottom), ((194, 40), 209.0));
        assert_eq!(placement(Form::Full, false, tops, &[one(6)]).0, (194, 76));
        let (size, bottom) = placement(Form::Mini, false, tops, &[one(4)]);
        assert_eq!((size, bottom), ((54, 40), 159.0));
        // While switching, both forms.
        let (size, bottom) = placement(Form::Mini, true, tops, &[one(4)]);
        assert_eq!((size, bottom), ((194, 90), 159.0));
        assert_eq!(margin(209.0), (0, 9, 8 + 41 + 209, 0));
    }

    #[test]
    fn each_screens_row_takes_its_shape_and_two_screens_rows_are_10_px_apart() {
        // 16:10 and narrower: 30 px; 16:9, 27; 21:9 and wider, 20; not
        // known, 30.
        assert_eq!(tile_height(2560, 1600), 30.0);
        assert_eq!(tile_height(1280, 1024), 30.0);
        assert_eq!(tile_height(1920, 1080), 27.0);
        assert_eq!(tile_height(3440, 1440), 20.0);
        assert_eq!(tile_height(5120, 1440), 20.0);
        assert_eq!(tile_height(0, 0), 30.0);
        // The dots: 18 × 13 at 16:10, 18 × 9 at 21:9.
        let laptop = one(4);
        let wide = Group {
            count: 4,
            tile: tile_height(3440, 1440),
        };
        assert_eq!(laptop.height_in(Form::Mini), 13.0);
        assert_eq!(wide.height_in(Form::Mini), 9.0);
        // Two rows of four, 10 px apart: 30 + 10 + 20.
        let groups = [laptop, wide];
        assert_eq!(group_height(Form::Full, &groups), 60.0);
        assert_eq!(group_top(Form::Full, &groups, 1), 40.0);
        // Six on the laptop: two rows of its own, 6 px apart, then the
        // wide screen's 10 px under them.
        let groups = [one(6), wide];
        assert_eq!(
            group_height(Form::Full, &groups),
            30.0 + 6.0 + 30.0 + 10.0 + 20.0
        );
        // The mini form: two blocks of two rows of dots, 10 px apart.
        assert_eq!(
            group_height(Form::Mini, &[laptop, wide]),
            (2.0 * 13.0 + 4.0) + 10.0 + (2.0 * 9.0 + 4.0)
        );
        // One screen alone keeps 3.9's tiles, whatever its shape.
        assert_eq!(shaped(vec![wide]), [one(4)]);
        assert_eq!(shaped(vec![laptop, wide]), [laptop, wide]);
        // The wide screen's tiles keep their width, lower.
        let sides = (11.0, 183.0);
        let a = tile(Form::Full, 1, sides, 0.0, laptop.tile);
        let b = tile(Form::Full, 1, sides, 40.0, wide.tile);
        assert_eq!((a.x, a.width), (b.x, b.width));
        assert_eq!(b.height, 20.0);
        // The surface grows by the second screen's row.
        let tops = (200.0, 150.0);
        assert_eq!(
            placement(Form::Full, false, tops, &[laptop, wide]).0,
            (194, 70)
        );
    }
}
