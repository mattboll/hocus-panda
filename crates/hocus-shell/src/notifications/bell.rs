//! The bell (5.4), on the bar's notifications place: the button that opens
//! the history (a hover of 150 ms in the mini form, or a click), its badge
//! counting what the user has not seen, a slash in amber while they are not
//! to be disturbed.
//!
//! Until the footer's four buttons (5.5) it has a small surface of its own
//! over the bar ([`hocus_link::BELL_NAMESPACE`]), opened with the bar's
//! others and after them (so that it is above the bar's), where 5.3's dot
//! was: in the mini form under the keyboard layout's button, in the full
//! form the footer's second place of four. [`draw`] draws it in any frame,
//! for the footer to take.

use std::sync::{Mutex, OnceLock, PoisonError};

use hocus_link::{BELL_NAMESPACE, EDGE, Form};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};

use crate::{
    levels::draw::{MANROPE_BOLD, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The button, and the room around it on its surface for its ring and its
/// badge's.
pub const BUTTON: Size = Size::new(40.0, 32.0);
const RADIUS: f32 = 10.0;
const ROOM: f32 = 6.0;
pub const SURFACE: (u32, u32) = (52, 44);
/// The bell in a 24 px box, drawn at 20 px.
const GLYPH_BOX: f32 = 24.0;
const GLYPH: f32 = 20.0;
/// The badge: 15 px high, 3 px in, its right 5 px past the bell's box and
/// its top 4 px over it, a 2 px ring in the bar's colour.
const BADGE: f32 = 15.0;
const BADGE_PAD: f32 = 3.0;
const BADGE_TEXT: f32 = 9.5;
const BADGE_RING: f32 = 2.0;
const BADGE_FILL: Color = rgb(0x8FB3FF);
const BADGE_INK: Color = rgb(0x0B1020);
const BADGE_RIM: Color = Color::from_rgba8(14, 17, 28, 0.95);
/// The button lit under the pointer, and while the history shows.
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const OPEN: Color = Color::from_rgba8(255, 255, 255, 0.10);

/// The bell's stroke, and in amber while the user is not to be disturbed.
const INK: &str = "#C9D0DC";
pub const AMBER: &str = "#F5C46E";

/// The button's centre from the bar's left, and over the bar's bottom, in
/// `form`: in the mini form under the keyboard layout's button, 14 px under
/// it as the 3.8 mockup has the notifications' place; in the full form the
/// footer's second place of four (its 34 px row, 12 px over the bar's
/// bottom).
fn centre(form: Form) -> (f32, f32) {
    let bar = form.width() as f32;
    match form {
        Form::Mini => (bar / 2.0, 22.0),
        Form::Full => (12.0 + (bar - 24.0) * 1.5 / 4.0, 29.0),
    }
}

/// How far over the bar's bottom the button's centre is, in `form`.
pub fn rise(form: Form) -> f32 {
    centre(form).1
}

/// The bell's surface on a bar in `form`, anchored to the screen's bottom
/// right: its size, its margins (top, right, bottom, left), and the button
/// in it.
pub fn placed(form: Form) -> ((u32, u32), (i32, i32, i32, i32), Rectangle) {
    let (x, y) = centre(form);
    let (w, h) = (SURFACE.0 as f32, SURFACE.1 as f32);
    let right = form.width() as f32 - x - w / 2.0;
    let bottom = y - h / 2.0;
    (
        SURFACE,
        (
            0,
            EDGE + right.round() as i32,
            EDGE + bottom.round() as i32,
            0,
        ),
        Rectangle::new(Point::new(ROOM, ROOM), BUTTON),
    )
}

/// The bell's glyph, `dnd`: amber and slashed.
fn glyph(dnd: bool) -> svg::Handle {
    static GLYPHS: OnceLock<[svg::Handle; 2]> = OnceLock::new();
    let make = |colour: &str, slash: &str| {
        svg::Handle::from_memory(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                 height=\"24\"><path d=\"M6 16.5 V11 a6 6 0 0 1 12 0 V16.5 L19.5 18 H4.5 Z M10 \
                 20.5 a2 2 0 0 0 4 0\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"1.8\" \
                 stroke-linejoin=\"round\"/>{slash}</svg>"
            )
            .into_bytes(),
        )
    };
    let glyphs = GLYPHS.get_or_init(|| {
        [
            make(INK, ""),
            make(
                AMBER,
                "<path d=\"M4 4 L20 20\" stroke=\"#F5C46E\" stroke-width=\"2\" \
                 stroke-linecap=\"round\"/>",
            ),
        ]
    });
    glyphs[usize::from(dnd)].clone()
}

/// What the bell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shows {
    /// What the badge counts (none at 0).
    pub count: usize,
    pub dnd: bool,
    pub open: bool,
    pub hover: bool,
    pub focused: bool,
}

/// The bell's button at `button` in frame `f`, as it `shows`.
pub fn draw(f: &mut Frame, button: Rectangle, shows: Shows) {
    let lit = if shows.open {
        Some(OPEN)
    } else if shows.hover {
        Some(HOVER)
    } else {
        None
    };
    if let Some(lit) = lit {
        f.fill(&kit::rounded(button, RADIUS), lit);
    }
    if shows.focused {
        ring(f, button, RADIUS);
    }
    let centre = button.center();
    let glyph_rect = Rectangle::new(
        Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
        Size::new(GLYPH, GLYPH),
    );
    f.draw_svg(glyph_rect, svg::Svg::new(glyph(shows.dnd)));
    if shows.count == 0 {
        return;
    }
    let count = shows.count.to_string();
    let width = (kit::measure(&count, BADGE_TEXT, MANROPE_BOLD) + 2.0 * BADGE_PAD).max(BADGE);
    let right = centre.x + GLYPH_BOX / 2.0 + 5.0;
    let top = centre.y - GLYPH_BOX / 2.0 - 4.0;
    let badge = Rectangle::new(Point::new(right - width, top), Size::new(width, BADGE));
    let rim = Rectangle::new(
        Point::new(badge.x - BADGE_RING, badge.y - BADGE_RING),
        Size::new(
            badge.width + 2.0 * BADGE_RING,
            badge.height + 2.0 * BADGE_RING,
        ),
    );
    f.fill(&kit::rounded(rim, BADGE / 2.0 + BADGE_RING), BADGE_RIM);
    f.fill(&kit::rounded(badge, BADGE / 2.0), BADGE_FILL);
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            count,
            badge.center(),
            BADGE_TEXT,
            BADGE_INK,
            Alignment::Center,
        ),
    ));
}

// --- Its surface ------------------------------------------------------------------

/// The bells' surfaces: their bar, and its form.
static BELLS: Mutex<Vec<(window::Id, String, Form)>> = Mutex::new(Vec::new());

/// A bell on bar `bar` in `form`, on `output`: its surface, and what opens
/// it (taking the pointer on its button only).
pub fn open(bar: &str, form: Form, output: OutputOption) -> (window::Id, [crate::Message; 2]) {
    let id = window::Id::unique();
    let (size, margin, button) = placed(form);
    BELLS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, bar.to_owned(), form));
    let settings = NewLayerShellSettings {
        size: Some(size),
        layer: Layer::Top,
        anchor: Anchor::Right | Anchor::Bottom,
        exclusive_zone: Some(0),
        margin: Some(margin),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: output,
        events_transparent: false,
        namespace: Some(BELL_NAMESPACE.to_owned()),
    };
    (
        id,
        [
            crate::Message::NewLayerShell { settings, id },
            super::region(id, Some(button)),
        ],
    )
}

/// Bar `bar` is in `form` now: its bell's surface and its new margins, if
/// they change (its size does not).
pub fn form(bar: &str, form: Form) -> Option<(window::Id, (i32, i32, i32, i32))> {
    let mut bells = BELLS.lock().unwrap_or_else(PoisonError::into_inner);
    let bell = bells.iter_mut().find(|(_, b, _)| b == bar)?;
    if bell.2 == form {
        return None;
    }
    bell.2 = form;
    Some((bell.0, placed(form).1))
}

/// Bar `bar` went.
pub fn forget(bar: &str) {
    BELLS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(_, b, _)| b != bar);
}

/// The bar and its form, if a bell's surface is `id`.
pub fn of(id: window::Id) -> Option<(String, Form)> {
    BELLS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(b, _, _)| *b == id)
        .map(|(_, bar, form)| (bar.clone(), *form))
}

/// The bell's surface on bar `bar`.
#[derive(Debug, Clone)]
pub struct Surface {
    pub bar: String,
    pub form: Form,
    /// On the CPU, the bar's colour under it (its buffer has no alpha).
    pub backdrop: Option<Color>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: bool,
}

impl Surface {
    fn button(&self) -> Rectangle {
        placed(self.form).2
    }

    /// A point of the surface (`size`) in the bar's frame: from the bar's
    /// right edge (negative inside it), and from its top.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        let (_, margin, _) = placed(self.form);
        let right = (margin.1 - EDGE) as f32;
        let bottom = (margin.2 - EDGE) as f32;
        let height = popover::with(|c| c.bar_height(&self.bar));
        Point::new(
            at.x - size.width - right,
            height - bottom - size.height + at.y,
        )
    }

    fn in_bar_rect(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(self.in_bar(rect.position(), size), rect.size())
    }
}

impl<Message> canvas::Program<Message> for Surface {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        let button = self.button();
        match event {
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.pointer);
                state.pointer = at;
                let over = at.is_some_and(|at| button.contains(at));
                tracing::trace!(?seen, ?at, over, "the bell: the pointer");
                match seen {
                    mouse::Event::CursorEntered
                    | mouse::Event::CursorMoved { .. }
                    | mouse::Event::CursorLeft => {
                        if over == state.hover {
                            return None;
                        }
                        state.hover = over;
                        if over {
                            popover::event(popover::Event::ItemEnter {
                                bar: self.bar.clone(),
                                item: Item::Notifications,
                                rect: self.in_bar_rect(button, size),
                            });
                        } else {
                            let at = at.unwrap_or_else(|| button.center());
                            popover::event(popover::Event::ItemLeave {
                                at: self.in_bar(at, size),
                            });
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item: Item::Notifications,
                            rect: self.in_bar_rect(button, size),
                        });
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
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
        let mut f = Frame::new(renderer, bounds.size());
        if let Some(backdrop) = self.backdrop {
            f.fill_rectangle(Point::ORIGIN, bounds.size(), backdrop);
        }
        let button = self.button();
        // Where it is in the bar, for the keyboard going through the bar's
        // items (the bar's height read first: not under the same lock).
        let in_bar = self.in_bar_rect(button, bounds.size());
        popover::with(|c| {
            c.set_items(
                &self.bar,
                hocus_link::BELL_NAMESPACE,
                vec![(Item::Notifications, in_bar)],
            );
        });
        let (count, dnd) = super::with(|c| (c.stack.badge(), c.stack.dnd.is_on()));
        let (open, focused) = popover::with(|c| {
            (
                c.shows(&self.bar, Item::Notifications),
                c.focused(&self.bar, Item::Notifications),
            )
        });
        draw(
            &mut f,
            button,
            Shows {
                count,
                dnd,
                open,
                hover: state.hover,
                focused,
            },
        );
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_sits_where_the_dot_of_5_3_was_its_badge_and_ring_on_its_surface() {
        // Mini: centred on the bar, its centre 22 px over the bar's bottom.
        let (size, margin, button) = placed(Form::Mini);
        assert_eq!(size, (52, 44));
        assert_eq!(margin, (0, EDGE + 2, EDGE, 0));
        assert_eq!(button, Rectangle::new(Point::new(6.0, 6.0), BUTTON));
        // Full: the footer's second place of four, 29 px over the bottom.
        let (_, margin, _) = placed(Form::Full);
        assert_eq!(margin, (0, EDGE + 94, EDGE + 7, 0));
        // The badge's ring and the keyboard's stay on the surface.
        let centre = button.center();
        assert!(centre.x + GLYPH_BOX / 2.0 + 5.0 + BADGE_RING <= SURFACE.0 as f32);
        assert!(centre.y - GLYPH_BOX / 2.0 - 4.0 - BADGE_RING >= 0.0);
        assert!(button.x >= 5.0 && button.y >= 5.0);
    }
}
