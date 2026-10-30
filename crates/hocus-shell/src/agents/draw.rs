//! The agents' group's surface, under the clock, as the approved 4.0
//! mockup draws it (bar.md §4 group 3): the full form only (the mini
//! form's agents are 4.1's).
//!
//! Its top 14 px under the clock's group, 100 px under the bar's top; as
//! tall as what the group shows, and one pixel, taking no pointer, when it
//! shows nothing (hidden, or the mini form). A change of what it shows
//! resizes it (layer-shell actions) and draws it alone.
//!
//! "Sources" and "Un autre outil ?" open the Sources popover, a row's
//! "Brancher" the consent popover, "Ne rien brancher" declines the tools
//! listed ([`crate::popover`]).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{AGENTS_NAMESPACE, EDGE, Form};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    BUTTON, BUTTON_TEXT, CARD_RADIUS, FOLLOWED, HEADER, IDLE, INTRO, INTRO_LINE, LABEL, LINK,
    Layout, MARK, MARK_GAP, MARK_INK, MARK_LETTER, MARK_RADIUS, NAME, Words,
};
use crate::{
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE_BOLD, SIDE, TEXT, faded, pointer, ring},
    popover::{self, Item, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// Around the group: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;
/// The group's top under the bar's: 14 px of padding, the clock's 72 px
/// and the 14 px between groups.
pub const TOP: f32 = 100.0;

const CARD: Color = Color::from_rgba8(255, 255, 255, 0.05);
const PLUG: Color = Color::from_rgba8(108, 194, 135, 0.18);
const PLUG_HOVER: Color = Color::from_rgba8(108, 194, 135, 0.28);

// --- Where the surface goes ------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    asked: Option<(u32, u32)>,
    input: Option<bool>,
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);
static LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);
/// What the compositor said changed since the group was laid out.
static STALE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the group shows changed: laid out again as it next draws.
pub fn stale() {
    STALE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The group as last laid out.
pub fn layout() -> Option<Layout> {
    LAYOUT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The group's height as laid out (0: it shows nothing).
fn group_height() -> f32 {
    layout().map_or(0.0, |l| l.height)
}

/// The surface's size in `form` (the full one while switching).
fn size_of(form: Form, switching: bool) -> (u32, u32) {
    let height = group_height();
    match (switching, form) {
        _ if height <= 0.0 => (
            if form == Form::Mini && !switching {
                MINI_WIDTH
            } else {
                FULL_WIDTH
            },
            1,
        ),
        (false, Form::Mini) => (MINI_WIDTH, 1),
        _ => (FULL_WIDTH, (height + 2.0 * ROOM).ceil() as u32),
    }
}

/// Its layer-shell margins: under the clock.
pub fn margin() -> (i32, i32, i32, i32) {
    (EDGE + (TOP - ROOM) as i32, EDGE + 1, 0, 0)
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let size = size_of(form, false);
    places(|places| {
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                asked: Some(size),
                input: None,
            },
        );
    });
    (size, margin())
}

pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

fn refit(place: &mut Place) -> Option<crate::network::draw::Placed> {
    let size = size_of(place.form, place.switching);
    if place.asked == Some(size) {
        return None;
    }
    place.asked = Some(size);
    Some((place.id, size, margin()))
}

/// Bar `bar` in `form`, switching or not: the surface's new size, if it
/// changes.
pub fn form(bar: &str, form: Form, switching: bool) -> Option<crate::network::draw::Placed> {
    places(|places| {
        let place = places.get_mut(bar)?;
        place.form = form;
        place.switching = switching;
        refit(place)
    })
}

/// The group was laid out again: every bar's surface to its height.
pub fn laid_out(layout: Layout) {
    *LAYOUT.lock().unwrap_or_else(PoisonError::into_inner) = Some(layout);
    let effects: Vec<popover::Effect> = places(|places| {
        places
            .values_mut()
            .filter_map(refit)
            .map(|(id, size, margin)| popover::Effect::Resize {
                id,
                size,
                margin: Some(margin),
            })
            .collect()
    });
    if !effects.is_empty() {
        popover::send(effects);
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

// --- The surface ------------------------------------------------------------------

/// The agents' group of one bar.
#[derive(Debug, Clone)]
pub struct Group {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now.
    pub bar_width: f32,
    pub words: Words,
    /// Without glass: the bar's opaque colour behind.
    pub backdrop: Option<Color>,
    pub bar: String,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: Option<Item>,
    last: Option<Point>,
}

impl<Message> canvas::Program<Message> for Group {
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
                if STALE.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    laid_out(super::lay_out());
                }
                let shows = self.full() && group_height() > 0.0;
                takes_pointer(&self.bar, shows);
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
        let size = bounds.size();
        let items = self.items(size);
        let in_bar: Vec<(Item, Rectangle)> = items
            .iter()
            .map(|(item, r)| {
                (
                    *item,
                    Rectangle::new(self.in_bar(r.position(), size), r.size()),
                )
            })
            .collect();
        log_items(&in_bar);
        popover::with(|c| c.set_items(&self.bar, AGENTS_NAMESPACE, in_bar));
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return vec![frame.into_geometry()];
        };
        let origin = self.origin(size);
        for &(form, alpha) in &self.forms {
            if form == Form::Full {
                self.group(&mut frame, &layout, origin, state.hover, alpha);
            }
        }
        for (item, rect) in items {
            if popover::with(|c| c.focused(&self.bar, item)) {
                let radius = match item {
                    Item::Plug(_) => BUTTON / 2.0,
                    _ => 4.0,
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
        tracing::debug!(items = %said, "the agents group's items");
        last.clone_from(&said);
    }
}

/// A text from `x` with its line centred on `middle`.
fn said(text: &str, at: Point, size: f32, colour: Color, align: Alignment) -> canvas::Text {
    kit::text(text.to_owned(), at, size, colour, align)
}

impl Group {
    /// Whether the full form shows alone (not switching): the group takes
    /// the pointer.
    fn full(&self) -> bool {
        matches!(self.forms.as_slice(), [(Form::Full, _)])
    }

    /// The group's top left on the surface.
    fn origin(&self, size: Size) -> Point {
        let left = size.width + 2.0 - self.bar_width;
        Point::new(left + SIDE, ROOM)
    }

    /// The group's items and where they are on the surface, in the full
    /// form alone.
    fn items(&self, size: Size) -> Vec<(Item, Rectangle)> {
        if !self.full() {
            return Vec::new();
        }
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return Vec::new();
        };
        // Not before the surface took the group's new height: a press there
        // would reach what is under it (the resize is the compositor's to
        // apply, a frame or more later).
        if size.height + 0.5 < layout.height + 2.0 * ROOM {
            return Vec::new();
        }
        let o = self.origin(size);
        let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
        let mut items = vec![(Item::Sources, at(layout.sources))];
        if let Some(card) = &layout.card {
            for row in &card.rows {
                items.push((Item::Plug(row.tool), at(row.button)));
            }
            items.push((Item::AnotherTool, at(card.another)));
            items.push((Item::Decline, at(card.decline)));
        }
        items
    }

    /// A point of the surface (`size`) in the bar's frame.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        Point::new(at.x - size.width - 1.0, TOP - ROOM + at.y)
    }

    fn group(&self, f: &mut Frame, layout: &Layout, o: Point, hover: Option<Item>, alpha: f32) {
        let lit = |item: Item, normal: Color| if hover == Some(item) { TEXT } else { normal };
        // The header: AGENTS, spaced; Sources on its right.
        for (ch, x) in kit::spaced(self.words.header(), |ch| {
            kit::measure(ch, LABEL, MANROPE_BOLD)
        }) {
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                said(
                    &ch.to_string(),
                    Point::new(o.x + x - kit::LEFT, o.y + HEADER / 2.0),
                    LABEL,
                    faded(DIM, alpha),
                    Alignment::Left,
                ),
            ));
        }
        let sources = layout.sources;
        f.fill_text(said(
            self.words.sources(),
            Point::new(o.x + sources.x, o.y + sources.center_y()),
            LINK,
            faded(lit(Item::Sources, FAINT), alpha),
            Alignment::Left,
        ));
        if let Some(card) = &layout.card {
            let rect = Rectangle::new(
                Point::new(o.x + card.rect.x, o.y + card.rect.y),
                card.rect.size(),
            );
            f.fill(&kit::rounded(rect, CARD_RADIUS), faded(CARD, alpha));
            let left = rect.x + 10.0;
            for (i, line) in card.intro.iter().enumerate() {
                let mut text = said(
                    line,
                    Point::new(
                        left,
                        o.y + card.intro_top + (i as f32 + 0.5) * INTRO * INTRO_LINE,
                    ),
                    INTRO,
                    faded(DIM, alpha),
                    Alignment::Left,
                );
                text.line_height = iced::widget::text::LineHeight::Relative(INTRO_LINE);
                f.fill_text(text);
            }
            for row in &card.rows {
                let middle = o.y + row.top + row.height / 2.0;
                let tool = row.tool;
                mark(
                    f,
                    Point::new(left, middle - MARK / 2.0),
                    tool.mark(),
                    (MARK, MARK_RADIUS, MARK_LETTER),
                    alpha,
                );
                let first = middle - (row.name.len() as f32 - 1.0) * NAME * LINE / 2.0;
                for (i, line) in row.name.iter().enumerate() {
                    f.fill_text(said(
                        line,
                        Point::new(left + MARK + MARK_GAP, first + i as f32 * NAME * LINE),
                        NAME,
                        faded(TEXT, alpha),
                        Alignment::Left,
                    ));
                }
                let button = Rectangle::new(
                    Point::new(o.x + row.button.x, o.y + row.button.y),
                    row.button.size(),
                );
                let fill = if hover == Some(Item::Plug(tool)) {
                    PLUG_HOVER
                } else {
                    PLUG
                };
                f.fill(&kit::rounded(button, BUTTON / 2.0), faded(fill, alpha));
                f.fill_text(kit::text_in(
                    MANROPE_BOLD,
                    said(
                        self.words.plug(),
                        button.center(),
                        BUTTON_TEXT,
                        faded(ACCENT, alpha),
                        Alignment::Center,
                    ),
                ));
            }
            for (item, rect, word) in [
                (Item::AnotherTool, card.another, self.words.another()),
                (Item::Decline, card.decline, self.words.decline()),
            ] {
                f.fill_text(said(
                    word,
                    Point::new(o.x + rect.x, o.y + rect.center_y()),
                    LINK,
                    faded(lit(item, FAINT), alpha),
                    Alignment::Left,
                ));
            }
        }
        if let Some((first, second, followed)) = &layout.idle {
            f.fill_text(said(
                self.words.no_agent(),
                Point::new(o.x, o.y + first),
                IDLE,
                faded(FAINT, alpha),
                Alignment::Left,
            ));
            f.fill_text(said(
                followed,
                Point::new(o.x, o.y + second),
                FOLLOWED,
                faded(FAINT, alpha),
                Alignment::Left,
            ));
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        let items = self.items(size);
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

/// A tool's mark at `at`: its letter on its colour, `size` px, rounded.
pub fn mark(
    f: &mut Frame,
    at: Point,
    (letter, colour): (&str, Color),
    (size, radius, letter_size): (f32, f32, f32),
    alpha: f32,
) {
    let rect = Rectangle::new(at, Size::new(size, size));
    f.fill(
        &Path::rounded_rectangle(at, rect.size(), radius.into()),
        faded(colour, alpha),
    );
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        said(
            letter,
            rect.center(),
            letter_size,
            faded(MARK_INK, alpha),
            Alignment::Center,
        ),
    ));
}
