//! The tray's surface (5.14, variant A): just above the footer, under a
//! 1 px `rgba(255,255,255,0.08)` line across the bar's inside, 6 px under
//! it; in the full form a row of 34 × 34 cells 4 px apart from the bar's
//! inside left, wrapping past four; in the mini form a column of 40 × 32
//! cells 2 px apart, centred. The cells' bottom is 10 px over the footer's
//! top (the mockup's gap), and the bar's groups above keep their 14 px over
//! the line: they go up by [`lift`]. Each cell holds its 22 px icon (or its
//! name's letter on a colour of its own when it has none), its overlay over
//! the bottom right; lit `0.08` under the pointer and `0.10` while its menu
//! shows, as the footer's buttons are. An icon coming pops in over 200 ms
//! (from 80 %, fading in, `ease-out`), at once without motion.
//!
//! A left click activates the item, a middle click its second action, the
//! wheel scrolls it, a right click opens its menu; resting 400 ms on an icon
//! shows its tooltip ([`super::tip`]).
//!
//! It is a surface of its own, opened while an icon shows and taking the
//! pointer on its cells only; while the bar switches its form it holds both
//! forms' places, and the two cross-fade as the other groups do.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::{Act, Node, Role};
use hocus_link::{EDGE, Form, TRAY_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Vector, mouse,
    widget::canvas::{self, Frame, Geometry},
    window,
};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};

use super::{Ask, ICON, Shown};
use crate::theme::Ink;
use crate::{
    Message,
    clock::motion::bezier,
    notifications::bell,
    popover::{self, Item, kit},
    ui::items::{Control, Controls, Items, Press},
};

/// The cells, in the full form and in the mini, and the gaps between them.
pub const FULL_CELL: Size = Size::new(34.0, 34.0);
pub const MINI_CELL: Size = Size::new(40.0, 32.0);
const FULL_GAP: f32 = 4.0;
const MINI_GAP: f32 = 2.0;
/// The line over the cells, and the room between them.
const LINE: f32 = 1.0;
const UNDER_LINE: f32 = 6.0;
const LINE_COLOUR: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
/// The cells' bottom over the footer's top.
const OVER_FOOTER: f32 = 10.0;
/// The footer's top over the bar's bottom, in each form (5.5).
const FULL_FOOTER: f32 = 13.0 + crate::footer::FULL_BUTTON.height;
const MINI_FOOTER: f32 = crate::footer::MINI_HEIGHT;
/// Around the cells on the surface: the keyboard's ring.
const ROOM: f32 = 6.0;
/// The bar's inside from the surface's sides: 12 px (6 in the mini form)
/// less its rim; the surface is the bar inside its rim.
const FULL_SIDE: f32 = 11.0;
const MINI_SIDE: f32 = 5.0;
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// The overlay: half the icon, at its bottom right.
const OVERLAY: f32 = ICON / 2.0;
/// An icon's pop-in.
const POP: Duration = Duration::from_millis(200);
const POP_FROM: f32 = 0.8;
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);
/// The letter of an icon that has none.
const LETTER: f32 = 11.0;
const LETTER_RADIUS: f32 = 6.0;

fn cell(form: Form) -> Size {
    match form {
        Form::Full => FULL_CELL,
        Form::Mini => MINI_CELL,
    }
}

fn gap(form: Form) -> f32 {
    match form {
        Form::Full => FULL_GAP,
        Form::Mini => MINI_GAP,
    }
}

fn side(form: Form) -> f32 {
    match form {
        Form::Full => FULL_SIDE,
        Form::Mini => MINI_SIDE,
    }
}

fn surface_width(form: Form) -> u32 {
    match form {
        Form::Full => FULL_WIDTH,
        Form::Mini => MINI_WIDTH,
    }
}

/// How many cells a row holds: the full form's inside, 172 px, holds four.
fn per_row(form: Form) -> usize {
    match form {
        Form::Full => {
            let inside = FULL_WIDTH as f32 - 2.0 * FULL_SIDE;
            (((inside + FULL_GAP) / (FULL_CELL.width + FULL_GAP)).floor() as usize).max(1)
        }
        Form::Mini => 1,
    }
}

/// The cells' height for `n` icons in `form`.
fn cells_height(form: Form, n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let rows = n.div_ceil(per_row(form));
    rows as f32 * cell(form).height + (rows - 1) as f32 * gap(form)
}

/// The cells' bottom over the bar's bottom.
fn cells_bottom(form: Form) -> f32 {
    let footer = match form {
        Form::Full => FULL_FOOTER,
        Form::Mini => MINI_FOOTER,
    };
    footer + OVER_FOOTER
}

/// How far the bar's groups above (the levels, and those over them) go
/// up for `n` icons in `form`: the cells, the line and the room between,
/// and the gap over the footer; nothing without icons.
pub fn lift_for(form: Form, n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    cells_height(form, n) + UNDER_LINE + LINE + OVER_FOOTER
}

/// The surface's size and its bottom over the bar's for `n` icons in
/// `form` (both forms' while the bar switches).
fn placement(form: Form, switching: bool, n: usize) -> ((u32, u32), f32) {
    let one = |form: Form| {
        let bottom = cells_bottom(form) - ROOM;
        let top = cells_bottom(form) + cells_height(form, n) + UNDER_LINE + LINE + ROOM;
        (bottom, top)
    };
    let ((bottom, top), width) = if switching {
        let (a, b) = (one(Form::Full), one(Form::Mini));
        ((a.0.min(b.0), a.1.max(b.1)), FULL_WIDTH)
    } else {
        (one(form), surface_width(form))
    };
    ((width, (top - bottom).ceil() as u32), bottom)
}

/// Cell `i` of `n` in `form` on a surface of `size` whose bottom is
/// `bottom` over the bar's, for a bar `bar_width` wide.
fn rect(form: Form, i: usize, n: usize, size: Size, bottom: f32, bar_width: f32) -> Rectangle {
    let cell = cell(form);
    let gap = gap(form);
    let right = size.width;
    let left = right + 2.0 - bar_width;
    let per_row = per_row(form);
    let (row, column) = (i / per_row, i % per_row);
    let top_over_bar = cells_bottom(form) + cells_height(form, n);
    let y = size.height - (top_over_bar - bottom) + row as f32 * (cell.height + gap);
    let x = match form {
        Form::Full => left + FULL_SIDE + column as f32 * (cell.width + gap),
        Form::Mini => (left + right - cell.width) / 2.0,
    };
    Rectangle::new(Point::new(x, y), cell)
}

/// The line's place over `n` cells in `form`.
fn line(form: Form, n: usize, size: Size, bottom: f32, bar_width: f32) -> Rectangle {
    let right = size.width;
    let left = right + 2.0 - bar_width;
    let over_bar = cells_bottom(form) + cells_height(form, n) + UNDER_LINE + LINE;
    let side = side(form);
    Rectangle::new(
        Point::new(left + side, size.height - (over_bar - bottom)),
        Size::new((right - side) - (left + side), LINE),
    )
}

fn margin(bottom: f32) -> (i32, i32, i32, i32) {
    (0, EDGE + 1, EDGE + bottom.round() as i32, 0)
}

// --- Its places ---------------------------------------------------------------------------

/// A bar's tray, as last placed.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// Its surface, while icons show.
    id: Option<window::Id>,
    form: Form,
    switching: bool,
    /// The icons it was placed for.
    n: usize,
    asked: ((u32, u32), (i32, i32, i32, i32)),
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// How far bar `bar`'s groups go up in `form`, as its tray was placed.
pub fn lift(bar: &str, form: Form) -> f32 {
    let n = places(|places| places.get(bar).map_or(0, |p| p.n));
    lift_for(form, n)
}

/// Bar `bar`'s tray's surface, if it shows.
pub fn surface(bar: &str) -> Option<window::Id> {
    places(|places| places.get(bar).and_then(|p| p.id))
}

/// Bar `bar` in `form`, switching or not, with the icons shown now: its
/// tray's surface opens, moves or goes.
pub fn sync(bar: &str, form: Form, switching: bool) -> Vec<Message> {
    let n = super::shown().len();
    let (size, bottom) = placement(form, switching, n);
    let margin = margin(bottom);
    places(|places| {
        let place = places.entry(bar.to_owned()).or_insert(Place {
            id: None,
            form,
            switching,
            n: 0,
            asked: ((0, 0), (0, 0, 0, 0)),
        });
        let mut messages = Vec::new();
        let changed = place.form != form || place.switching != switching || place.n != n;
        place.form = form;
        place.switching = switching;
        place.n = n;
        match (place.id, n) {
            (Some(id), 0) => {
                place.id = None;
                messages.push(Message::RemoveWindow(id));
                crate::popover::with(|c| c.set_items(bar, TRAY_NAMESPACE, Vec::new()));
            }
            (None, 0) => {}
            (None, _) => {
                let id = window::Id::unique();
                place.id = Some(id);
                place.asked = (size, margin);
                let settings = NewLayerShellSettings {
                    size: Some(size),
                    layer: Layer::Top,
                    anchor: crate::side::anchor(Anchor::Right | Anchor::Bottom),
                    exclusive_zone: Some(0),
                    margin: Some(crate::side::margin(margin)),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: if bar == crate::ALONE {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(bar.to_owned())
                    },
                    events_transparent: false,
                    namespace: Some(TRAY_NAMESPACE.to_owned()),
                };
                crate::side::opened(id);
                messages.push(Message::NewLayerShell { settings, id });
                messages.push(region(id, form, switching, n));
            }
            (Some(id), _) => {
                if place.asked.0 != size {
                    messages.push(Message::SizeChange { id, size });
                }
                if place.asked.1 != margin {
                    messages.push(Message::MarginChange {
                        id,
                        margin: crate::side::moved(id, margin),
                    });
                }
                place.asked = (size, margin);
                if changed {
                    messages.push(region(id, form, switching, n));
                }
            }
        }
        crate::popover::with(|c| c.set_tray(bar, place.id));
        messages
    })
}

/// Bar `bar` went: its tray's surface, if it had one.
pub fn forget(bar: &str) -> Option<window::Id> {
    places(|places| places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose tray's surface is `id`.
fn bar_of(id: window::Id) -> Option<(String, Place)> {
    places(|places| {
        places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(b, p)| (b.clone(), *p))
    })
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: i32 = 1 << 20;

type WlRegion = wayland_client::protocol::wl_region::WlRegion;

/// Surface `id` takes the pointer on its cells only; on none while the bar
/// switches.
fn region(id: window::Id, form: Form, switching: bool, n: usize) -> Message {
    let rects: Vec<(i32, i32, i32, i32)> = if switching {
        Vec::new()
    } else {
        let ((w, h), bottom) = placement(form, false, n);
        let size = Size::new(w as f32, h as f32);
        (0..n)
            .map(|i| {
                let r = rect(form, i, n, size, bottom, form.width() as f32);
                let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
                let right = (r.x + r.width).ceil() as i32;
                let bottom = (r.y + r.height).ceil() as i32;
                (x, y, right - x, bottom - y)
            })
            .collect()
    };
    let callback = iced_layershell::actions::ActionCallback::new(move |wl: &WlRegion| {
        wl.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
        for &(x, y, w, h) in &rects {
            wl.add(x, y, w, h);
        }
    });
    Message::SetInputRegion { id, callback }
}

// --- The surface ----------------------------------------------------------------------------

/// The tray of a bar on its surface.
#[derive(Debug, Clone)]
pub struct Tray {
    pub id: window::Id,
    /// Its bar's screen's name.
    pub bar: String,
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    pub bar_width: f32,
    pub motion: bool,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// A wheel's notches not yet whole.
    scroll: (f32, f32),
    /// What it last said it shows.
    said: Option<(Form, Vec<String>)>,
}

impl Tray {
    fn place(&self) -> Option<(String, Place)> {
        bar_of(self.id)
    }

    /// Its surface's bottom over the bar's, as placed for its forms now.
    fn bottom(&self, n: usize) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false, n).1,
            _ => placement(Form::Full, true, n).1,
        }
    }

    /// The icons and their cells, unless the bar is switching.
    fn cells(&self, shown: &[Shown], size: Size) -> Vec<(u32, Rectangle)> {
        let [(form, _)] = self.forms.as_slice() else {
            return Vec::new();
        };
        let n = shown.len();
        let bottom = self.bottom(n);
        shown
            .iter()
            .enumerate()
            .map(|(i, s)| (s.key, rect(*form, i, n, size, bottom, self.bar_width)))
            .collect()
    }

    /// From the surface (`size`) to the bar's frame.
    fn to_bar(&self, bar: &str, size: Size, n: usize) -> Vector {
        let height = popover::with(|c| c.bar_height(bar));
        Vector::new(-size.width - 1.0, height - self.bottom(n) - size.height)
    }

    /// What it shows changed: said, by the icons' names and places.
    fn say(&self, state: &mut State, bar: &str, shown: &[Shown], size: Size) {
        let [(form, _)] = self.forms.as_slice() else {
            return;
        };
        let names: Vec<String> = shown
            .iter()
            .zip(self.cells(shown, size))
            .map(|(s, (_, r))| {
                let r = r + self.to_bar(bar, size, shown.len());
                format!("{}@{:.0},{:.0}", s.name, r.center_x(), r.center_y())
            })
            .collect();
        let said = Some((*form, names));
        if said == state.said {
            return;
        }
        state.said = said;
        if let Some((form, names)) = &state.said {
            tracing::debug!(bar, form = form.name(), ?names, "the tray shows it");
        }
    }

    /// The wheel on icon `key`: its item scrolls, a notch at a time.
    fn wheel(&self, state: &mut State, key: u32, bar: &str, delta: mouse::ScrollDelta) {
        let (x, y) = match delta {
            mouse::ScrollDelta::Lines { x, y } => (x, y),
            mouse::ScrollDelta::Pixels { x, y } => {
                // A notch each 15 px, as the levels count them.
                state.scroll.0 += x / 15.0;
                state.scroll.1 += y / 15.0;
                let whole = (state.scroll.0.trunc(), state.scroll.1.trunc());
                state.scroll.0 -= whole.0;
                state.scroll.1 -= whole.1;
                whole
            }
        };
        let scale = kit::scale(bar).max(1.0);
        let notches = |v: f32| (v / scale).round() as i32;
        if notches(y) != 0 {
            super::ask(Ask::Scroll {
                key,
                delta: notches(y) * 120,
                horizontal: false,
            });
        }
        if notches(x) != 0 {
            super::ask(Ask::Scroll {
                key,
                delta: notches(x) * 120,
                horizontal: true,
            });
        }
    }
}

/// How far into its pop-in an icon that came `since` is at `now`, 0 to 1.
fn popped(since: Instant, now: Instant, motion: bool) -> f32 {
    if !motion {
        return 1.0;
    }
    let t = now.saturating_duration_since(since).as_secs_f32() / POP.as_secs_f32();
    if t >= 1.0 { 1.0 } else { bezier(EASE_OUT, t) }
}

impl Items for Tray {
    type State = State;

    fn bar(&self) -> &str {
        &self.bar
    }

    fn namespace(&self) -> &'static str {
        TRAY_NAMESPACE
    }

    fn controls(&self, _state: &State, size: Size) -> Controls {
        let shown = super::shown();
        let n = shown.len();
        let to_bar = self.to_bar(&self.bar, size, n);
        // Placed for other icons: none until it is placed again.
        if self.place().is_none_or(|(_, place)| place.n != n) {
            return Controls {
                to_bar,
                ..Controls::default()
            };
        }
        let controls = shown
            .iter()
            .zip(self.cells(&shown, size))
            .map(|(s, (key, rect))| {
                Control::new(
                    Item::Tray(key),
                    rect,
                    Role::Button,
                    s.name.clone(),
                    bell::RADIUS,
                )
                .node(|n| n.act(Act::Menu))
            })
            .collect();
        Controls {
            to_bar,
            controls,
            group: Some(Node::new(
                "tray",
                Role::Toolbar,
                crate::a11y::Words::new(crate::clock::time::Lang::of_messages()).tray(),
            )),
        }
    }

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                let (bar, _) = self.place()?;
                let shown = super::shown();
                self.say(state, &bar, &shown, bounds.size());
                let moving = self.motion
                    && shown
                        .iter()
                        .any(|s| now.saturating_duration_since(s.since) < POP);
                moving.then(canvas::Action::request_redraw)
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let shown = super::shown();
                let (key, _) = self
                    .cells(&shown, bounds.size())
                    .into_iter()
                    .find(|(_, r)| at.is_some_and(|at| r.contains(at)))?;
                self.wheel(state, key, &self.bar, *delta);
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    /// An icon under the pointer shows its tooltip.
    fn hovered(&self, _state: &mut State, on: Option<(Item, Rectangle)>) {
        match on {
            Some((Item::Tray(key), rect)) => super::tip::hover(&self.bar, key, rect),
            _ => super::tip::leave(),
        }
    }

    /// A left click activates the icon's item, the middle one its
    /// secondary action, a right click shows its menu (or closes it).
    fn pressed(
        &self,
        _state: &mut State,
        button: mouse::Button,
        on: Option<(Item, Rectangle)>,
    ) -> Press {
        let Some((Item::Tray(key), rect)) = on else {
            return Press::Passed;
        };
        super::tip::leave();
        let bar = self.bar.clone();
        match button {
            mouse::Button::Left => super::ask(Ask::Activate { key, bar, rect }),
            mouse::Button::Middle => super::ask(Ask::Secondary { key }),
            mouse::Button::Right => {
                if popover::with(|c| c.shows(&bar, Item::Tray(key))) {
                    popover::event(popover::Event::Chosen);
                } else {
                    super::ask(Ask::Menu { key, bar, rect });
                }
            }
            _ => return Press::Passed,
        }
        Press::Taken
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        bounds: Rectangle,
        lit: Option<Item>,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let under = crate::backdrop::under(renderer, size, self.backdrop);
        let Some((bar, place)) = self.place() else {
            return crate::backdrop::over(under, frame);
        };
        let shown = super::shown();
        let n = shown.len();
        if n != place.n {
            return crate::backdrop::over(under, frame);
        }
        let now = Instant::now();
        let still = self.forms.len() == 1;
        let bottom = self.bottom(n);
        let left = (size.width + 2.0 - self.bar_width).max(0.0);
        let column = Rectangle::new(
            Point::new(left, 0.0),
            Size::new((size.width - left).max(0.0), size.height),
        );
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                let line = line(form, n, size, bottom, self.bar_width);
                f.fill_rectangle(line.position(), line.size(), LINE_COLOUR.scale_alpha(alpha));
                for (i, item) in shown.iter().enumerate() {
                    let cell = rect(form, i, n, size, bottom, self.bar_width);
                    let open = still && popover::with(|c| c.shows(&bar, Item::Tray(item.key)));
                    let hover = still && lit == Some(Item::Tray(item.key));
                    bell::lit(f, cell, open, hover);
                    let p = popped(item.since, now, self.motion);
                    let side = ICON * (POP_FROM + (1.0 - POP_FROM) * p);
                    let centre = cell.center();
                    let icon = Rectangle::new(
                        Point::new(centre.x - side / 2.0, centre.y - side / 2.0),
                        Size::new(side, side),
                    );
                    let opacity = alpha * p;
                    match &item.icon {
                        Some(handle) => {
                            f.draw_image(icon, canvas::Image::new(handle.clone()).opacity(opacity))
                        }
                        None => kit::app_tile(
                            f,
                            icon,
                            (None, &item.name),
                            (LETTER_RADIUS, LETTER),
                            opacity,
                        ),
                    }
                    if let Some(overlay) = &item.overlay {
                        let o = OVERLAY * side / ICON;
                        let at = Rectangle::new(
                            Point::new(icon.x + icon.width - o, icon.y + icon.height - o),
                            Size::new(o, o),
                        );
                        f.draw_image(at, canvas::Image::new(overlay.clone()).opacity(opacity));
                    }
                }
            }
        });
        crate::backdrop::over(under, frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(form: Form, n: usize) -> Vec<Rectangle> {
        let ((w, h), bottom) = placement(form, false, n);
        let size = Size::new(w as f32, h as f32);
        (0..n)
            .map(|i| rect(form, i, n, size, bottom, form.width() as f32))
            .collect()
    }

    #[test]
    fn the_full_forms_row_starts_at_the_bars_inside_and_wraps_past_four() {
        assert_eq!(per_row(Form::Full), 4);
        let ((w, h), bottom) = placement(Form::Full, false, 3);
        assert_eq!(w, 194);
        // The cells' bottom 10 px over the footer's top (47 px over the
        // bar's bottom), the line 6 px over them.
        assert_eq!(bottom, 47.0 + 10.0 - ROOM);
        assert_eq!(h as f32, 2.0 * ROOM + 34.0 + 6.0 + 1.0);
        let cells = rects(Form::Full, 3);
        let xs: Vec<f32> = cells.iter().map(|r| r.x).collect();
        assert_eq!(xs, [11.0, 49.0, 87.0]);
        for r in &cells {
            assert_eq!(r.size(), FULL_CELL);
            assert_eq!(bottom + h as f32 - (r.y + r.height), 57.0);
        }
        // Five: a second row under the first, the fifth on its left.
        let cells = rects(Form::Full, 5);
        assert_eq!(cells[4].x, 11.0);
        assert_eq!(cells[4].y - cells[0].y, 38.0);
        assert_eq!(lift_for(Form::Full, 5), 34.0 * 2.0 + 4.0 + 17.0);
        // The line across the bar's inside, 6 px over the first row.
        let ((w, h), bottom) = placement(Form::Full, false, 3);
        let size = Size::new(w as f32, h as f32);
        let line = line(Form::Full, 3, size, bottom, 196.0);
        assert_eq!((line.x, line.width), (11.0, 172.0));
        assert_eq!(rects(Form::Full, 3)[0].y - (line.y + 1.0), 6.0);
    }

    #[test]
    fn the_mini_forms_column_is_centred_over_the_bell() {
        let ((w, h), bottom) = placement(Form::Mini, false, 3);
        assert_eq!(w, 54);
        assert_eq!(bottom, 81.0 + 10.0 - ROOM);
        let cells = rects(Form::Mini, 3);
        for (i, r) in cells.iter().enumerate() {
            assert_eq!(r.size(), MINI_CELL);
            assert_eq!(r.x, 7.0);
            if i > 0 {
                assert_eq!(r.y - (cells[i - 1].y + 32.0), 2.0);
            }
        }
        assert_eq!(bottom + h as f32 - (cells[2].y + 32.0), 91.0);
        assert_eq!(lift_for(Form::Mini, 3), 3.0 * 32.0 + 2.0 * 2.0 + 17.0);
        assert_eq!(lift_for(Form::Mini, 0), 0.0);
    }

    #[test]
    fn while_the_bar_switches_the_surface_holds_both_forms() {
        let ((w, h), bottom) = placement(Form::Mini, true, 2);
        assert_eq!(w, 194);
        assert_eq!(placement(Form::Full, true, 2), ((w, h), bottom));
        let size = Size::new(w as f32, h as f32);
        for form in [Form::Full, Form::Mini] {
            for i in 0..2 {
                let r = rect(form, i, 2, size, bottom, form.width() as f32);
                assert!(r.y >= ROOM - 0.5 && r.y + r.height <= size.height, "{r:?}");
            }
        }
    }

    #[test]
    fn an_icon_pops_in_over_200_ms() {
        let t = Instant::now();
        assert_eq!(popped(t, t, true), 0.0);
        let mid = popped(t, t + Duration::from_millis(100), true);
        assert!(mid > 0.5 && mid < 1.0, "{mid}");
        assert_eq!(popped(t, t + POP, true), 1.0);
        assert_eq!(popped(t, t, false), 1.0);
    }
}
