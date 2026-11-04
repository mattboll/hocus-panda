//! The sign of what is in use, under the clock (5.12, variant A): 48 wide,
//! radius 12, `rgba(255,255,255,0.05)` (`0.10` under the pointer and while
//! its popover shows), 6 px in above and under its tiles. One 30 × 24 tile
//! per kind in use (radius 7, 4 px apart), each a 16 px icon in its colour
//! on a tint of it (`0x29` of 255): the microphone orange `#F5A35B` (grey
//! `#A9B2C2` on `rgba(255,255,255,0.06)` while muted), the camera green
//! `#6CC287`, the screen blue `#8FB3FF`. The mini form stacks the tiles;
//! the full one sets them in a row, 9 px in at its sides, so that one tile
//! is the same 48 × 36 sign in both. Centred in the bar, where the agents'
//! group (the full form) or the agents' item (the mini) was; they move
//! under it.
//!
//! It pops in over 200 ms as it appears (`ease-out`, from 0.9 of its size
//! and transparent), a fade alone with reduced motion; then it draws only
//! when what it shows changes.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{Form, IN_USE_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard, mouse,
    widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
};

use super::Kind;
use crate::{
    Message,
    levels::draw::{faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// A kind's tile, and between two.
pub const TILE: Size = Size::new(30.0, 24.0);
const TILE_GAP: f32 = 4.0;
const TILE_RADIUS: f32 = 7.0;
/// Above and under the tiles; at the full form's row's sides.
const PAD_Y: f32 = 6.0;
const PAD_X: f32 = 9.0;
const RADIUS: f32 = 12.0;
const ICON: f32 = 16.0;
/// The mini form's sign.
const WIDTH: f32 = 48.0;
/// Between it and what comes under it, as between the bar's items.
pub const AFTER: f32 = 14.0;
/// It pops in this long as it appears.
pub const POP: Duration = Duration::from_millis(200);
/// It draws again this often while it pops in.
const POP_STEP: Duration = Duration::from_millis(16);
const FILL: Color = Color::from_rgba8(255, 255, 255, 0.05);
const LIT: Color = Color::from_rgba8(255, 255, 255, 0.10);
pub const MICROPHONE: Color = rgb(0xF5A35B);
pub const CAMERA: Color = rgb(0x6CC287);
pub const SCREEN: Color = rgb(0x8FB3FF);
pub const MUTED: Color = rgb(0xA9B2C2);
const MUTED_TILE: Color = Color::from_rgba8(255, 255, 255, 0.06);
/// A tile's tint of its colour: the mockup's `#RRGGBB29`.
const TINT: f32 = 41.0 / 255.0;
/// Around the sign on its surface: the keyboard's ring.
const ROOM: f32 = 5.0;
/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;

/// Kind `kind`'s colour (the microphone's grey while it is `muted`).
pub fn colour(kind: Kind, muted: bool) -> Color {
    match kind {
        Kind::Microphone if muted => MUTED,
        Kind::Microphone => MICROPHONE,
        Kind::Camera => CAMERA,
        Kind::Screen => SCREEN,
    }
}

/// The sign's size in `form` with `kinds` tiles (one at least).
pub fn size(form: Form, kinds: usize) -> Size {
    let n = kinds.max(1) as f32;
    let along = |tile: f32| n * tile + (n - 1.0) * TILE_GAP;
    match form {
        Form::Mini => Size::new(WIDTH, 2.0 * PAD_Y + along(TILE.height)),
        Form::Full => Size::new(2.0 * PAD_X + along(TILE.width), 2.0 * PAD_Y + TILE.height),
    }
}

/// Its tiles, in a sign at `rect` in `form`.
pub fn tiles(rect: Rectangle, form: Form, kinds: usize) -> Vec<Rectangle> {
    (0..kinds)
        .map(|i| {
            let i = i as f32;
            let at = match form {
                Form::Mini => Point::new(
                    rect.center_x() - TILE.width / 2.0,
                    rect.y + PAD_Y + i * (TILE.height + TILE_GAP),
                ),
                Form::Full => {
                    Point::new(rect.x + PAD_X + i * (TILE.width + TILE_GAP), rect.y + PAD_Y)
                }
            };
            Rectangle::new(at, TILE)
        })
        .collect()
}

/// The sign's top under the bar's in `form`: where the agents' group (the
/// full form) or the agents' item (the mini) was.
fn top(form: Form) -> f32 {
    match form {
        Form::Full => crate::agents::draw::TOP,
        Form::Mini => crate::agents::MINI_TOP,
    }
}

/// Its surface's size and its top under the bar's, in `form` (both forms'
/// places while the bar switches), with `kinds` tiles.
pub fn placement(form: Form, switching: bool, kinds: usize) -> ((u32, u32), f32) {
    let tall = |h: f32| (h.max(1.0) + 2.0 * ROOM).ceil() as u32;
    let height = |form| size(form, kinds).height;
    match (switching, form) {
        (true, _) => {
            let (a, b) = (top(Form::Full), top(Form::Mini));
            let most = height(Form::Full).max(height(Form::Mini));
            (
                (FULL_WIDTH, tall(a.max(b) - a.min(b) + most)),
                a.min(b) - ROOM,
            )
        }
        (false, Form::Full) => ((FULL_WIDTH, tall(height(form))), top(form) - ROOM),
        (false, Form::Mini) => ((MINI_WIDTH, tall(height(form))), top(form) - ROOM),
    }
}

/// How far it has popped in `t` after it appeared: its opacity, and its
/// size (from 0.9; always whole without `motion`).
pub fn pop(t: Duration, motion: bool) -> (f32, f32) {
    let x = (t.as_secs_f32() / POP.as_secs_f32()).clamp(0.0, 1.0);
    let k = crate::clock::motion::bezier((0.0, 0.0, 0.58, 1.0), x);
    let scale = if motion { 0.9 + 0.1 * k } else { 1.0 };
    (k, scale)
}

/// "0:12", "12:04", "1:02:03": how long it has recorded (5.10).
pub fn elapsed(time: Duration) -> String {
    let s = time.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The sign of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct Sign {
    pub bar: String,
    pub forms: Vec<(Form, f32)>,
    pub bar_width: f32,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
    /// It pops in from smaller (no reduced motion).
    pub motion: bool,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    hover: bool,
    drawn: Option<Instant>,
}

/// What the sign last said it shows at rest, for the log.
static SAID: Mutex<Option<String>> = Mutex::new(None);

/// The sign went: when it comes again, it says so again.
pub fn unsaid() {
    *SAID.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

impl Sign {
    /// The surface's top under the bar's, as placed for its forms now.
    fn surface_top(&self, kinds: usize) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false, kinds).1,
            _ => placement(Form::Full, true, kinds).1,
        }
    }

    /// The sign in `form` on a surface `size` large, centred in the bar as
    /// wide as it is drawn now.
    fn rect_of(&self, form: Form, size: Size, kinds: usize) -> Rectangle {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        let sign = self::size(form, kinds);
        Rectangle::new(
            Point::new(
                centre - sign.width / 2.0,
                top(form) - self.surface_top(kinds),
            ),
            sign,
        )
    }

    /// The sign alone (not while the bar switches), for the pointer.
    fn item(&self, size: Size, kinds: usize) -> Option<Rectangle> {
        match self.forms.as_slice() {
            [(form, _)] if kinds > 0 => Some(self.rect_of(*form, size, kinds)),
            _ => None,
        }
    }

    /// `rect` (the surface's) in the bar's frame: from its right edge
    /// (negative inside it), from its top.
    fn in_bar(&self, rect: Rectangle, size: Size, kinds: usize) -> Rectangle {
        Rectangle::new(
            Point::new(rect.x - size.width - 1.0, self.surface_top(kinds) + rect.y),
            rect.size(),
        )
    }
}

impl canvas::Program<Message> for Sign {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.drawn = Some(*now);
                // Popping in: drawn again until it is whole.
                let appeared = super::snapshot().appeared?;
                (now.saturating_duration_since(appeared) < POP)
                    .then(|| canvas::Action::request_redraw_at(*now + POP_STEP))
            }
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.at);
                state.at = at;
                let kinds = super::snapshot().kinds().len();
                let rect = self.item(size, kinds)?;
                let over = at.is_some_and(|at| rect.contains(at));
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
                                item: Item::InUse,
                                rect: self.in_bar(rect, size, kinds),
                            });
                        } else {
                            let at = at.unwrap_or(Point::new(size.width / 2.0, size.height));
                            popover::event(popover::Event::ItemLeave {
                                at: self
                                    .in_bar(Rectangle::new(at, Size::ZERO), size, kinds)
                                    .position(),
                            });
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item: Item::InUse,
                            rect: self.in_bar(rect, size, kinds),
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
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if let Some(backdrop) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, size, backdrop);
        }
        let uses = super::snapshot();
        let kinds = uses.kinds();
        if let Some(rect) = self.item(size, kinds.len()) {
            let in_bar = self.in_bar(rect, size, kinds.len());
            popover::with(|c| {
                c.set_items(&self.bar, IN_USE_NAMESPACE, vec![(Item::InUse, in_bar)])
            });
        }
        if kinds.is_empty() {
            return vec![frame.into_geometry()];
        }
        let now = state.drawn.unwrap_or_else(Instant::now);
        let t = uses
            .appeared
            .map_or(POP, |appeared| now.saturating_duration_since(appeared));
        let (opacity, scale) = pop(t, self.motion);
        let still = self.forms.len() == 1;
        let (open, focused) = if still {
            popover::with(|c| {
                (
                    c.shows(&self.bar, Item::InUse),
                    c.focused(&self.bar, Item::InUse),
                )
            })
        } else {
            (false, false)
        };
        for &(form, alpha) in &self.forms {
            let rect = self.rect_of(form, size, kinds.len());
            let lit = still && (state.hover || open);
            let centre = Vector::new(rect.center_x(), rect.center_y());
            frame.with_save(|f| {
                f.translate(centre);
                f.scale(scale);
                f.translate(Vector::new(-centre.x, -centre.y));
                draw_sign(f, rect, form, (&kinds, uses.muted), lit, alpha * opacity);
            });
            if focused {
                ring(&mut frame, rect, RADIUS);
            }
        }
        if t >= POP && still {
            let said = format!(
                "{kinds:?} muted={} lit={}",
                uses.muted,
                still && (state.hover || open)
            );
            let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.as_deref() != Some(said.as_str()) {
                tracing::debug!(
                    kinds = ?kinds,
                    muted = uses.muted,
                    form = ?self.forms.first().map(|f| f.0),
                    "the in-use sign is drawn at rest"
                );
                *kept = Some(said);
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
        if state.hover {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The sign at `rect` in `form`: its tint, then a tile per kind.
pub fn draw_sign(
    f: &mut Frame,
    rect: Rectangle,
    form: Form,
    (kinds, muted): (&[Kind], bool),
    lit: bool,
    alpha: f32,
) {
    f.fill(
        &kit::rounded(rect, RADIUS),
        faded(if lit { LIT } else { FILL }, alpha),
    );
    for (kind, tile) in kinds.iter().zip(tiles(rect, form, kinds.len())) {
        let grey = *kind == Kind::Microphone && muted;
        let ink = colour(*kind, muted);
        let tint = if grey {
            MUTED_TILE
        } else {
            Color { a: TINT, ..ink }
        };
        f.fill(&kit::rounded(tile, TILE_RADIUS), faded(tint, alpha));
        let icon = Rectangle::new(
            Point::new(tile.center_x() - ICON / 2.0, tile.center_y() - ICON / 2.0),
            Size::new(ICON, ICON),
        );
        draw_icon(f, *kind, icon, faded(ink, alpha));
    }
}

/// Kind `kind`'s icon (the mockup's 24 px drawings, stroked 2 px) in
/// `rect`.
pub fn draw_icon(f: &mut Frame, kind: Kind, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| match kind {
        Kind::Microphone => {
            // A capsule from 3.5 to 15 (radius 3), its stand (a half
            // circle under it) and its foot.
            b.rounded_rectangle(p(9.0, 3.5), Size::new(6.0 * k, 11.5 * k), (3.0 * k).into());
            let c = 6.0 * 0.552_285;
            b.move_to(p(6.0, 11.5));
            b.bezier_curve_to(p(6.0, 11.5 + c), p(12.0 - c, 17.5), p(12.0, 17.5));
            b.bezier_curve_to(p(12.0 + c, 17.5), p(18.0, 11.5 + c), p(18.0, 11.5));
            b.move_to(p(12.0, 17.5));
            b.line_to(p(12.0, 20.5));
        }
        Kind::Camera => {
            b.move_to(p(3.5, 7.0));
            b.line_to(p(15.0, 7.0));
            b.line_to(p(15.0, 17.0));
            b.line_to(p(3.5, 17.0));
            b.close();
            b.move_to(p(15.0, 10.5));
            b.line_to(p(20.5, 7.5));
            b.line_to(p(20.5, 16.5));
            b.line_to(p(15.0, 13.5));
        }
        Kind::Screen => {
            b.move_to(p(3.5, 5.0));
            b.line_to(p(20.5, 5.0));
            b.line_to(p(20.5, 16.0));
            b.line_to(p(3.5, 16.0));
            b.close();
            b.move_to(p(9.0, 20.0));
            b.line_to(p(15.0, 20.0));
            b.move_to(p(12.0, 16.0));
            b.line_to(p(12.0, 20.0));
        }
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(2.0 * k)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recordings_time_reads() {
        assert_eq!(elapsed(Duration::from_secs(12)), "0:12");
        assert_eq!(elapsed(Duration::from_secs(724)), "12:04");
        assert_eq!(elapsed(Duration::from_secs(3723)), "1:02:03");
    }

    #[test]
    fn one_tile_a_kind_stacked_in_the_mini_form_in_a_row_in_the_full_one() {
        // One kind: the same 48 × 36 sign in both forms.
        assert_eq!(size(Form::Mini, 1), Size::new(48.0, 36.0));
        assert_eq!(size(Form::Full, 1), Size::new(48.0, 36.0));
        // Three: stacked, 4 px apart; in a row.
        assert_eq!(
            size(Form::Mini, 3),
            Size::new(48.0, 12.0 + 3.0 * 24.0 + 8.0)
        );
        assert_eq!(
            size(Form::Full, 3),
            Size::new(18.0 + 3.0 * 30.0 + 8.0, 36.0)
        );
        let mini = tiles(
            Rectangle::new(Point::ORIGIN, size(Form::Mini, 3)),
            Form::Mini,
            3,
        );
        assert_eq!(mini[0], Rectangle::new(Point::new(9.0, 6.0), TILE));
        assert_eq!(mini[2].y, 6.0 + 2.0 * 28.0);
        assert!(mini.iter().all(|t| t.x == 9.0));
        let full = tiles(
            Rectangle::new(Point::ORIGIN, size(Form::Full, 3)),
            Form::Full,
            3,
        );
        assert_eq!(full[1], Rectangle::new(Point::new(9.0 + 34.0, 6.0), TILE));
        assert!(full.iter().all(|t| t.y == 6.0));
    }

    #[test]
    fn the_sign_sits_where_the_agents_started_and_they_move_under_it() {
        let ((w, h), top) = placement(Form::Mini, false, 1);
        assert_eq!(w, 54);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert_eq!(h, 36 + 10);
        let ((_, h3), _) = placement(Form::Mini, false, 3);
        assert_eq!(h3, 92 + 10);
        let ((w, h), top) = placement(Form::Full, false, 3);
        assert_eq!((w, h, top), (194, 46, crate::agents::draw::TOP - ROOM));
        // While switching, both forms' places on one surface.
        let ((_, h), top) = placement(Form::Full, true, 2);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert!(top + h as f32 >= crate::agents::draw::TOP + size(Form::Full, 2).height);
    }

    #[test]
    fn it_pops_in_over_200_ms_a_fade_alone_with_reduced_motion() {
        assert_eq!(pop(Duration::ZERO, true), (0.0, 0.9));
        let (half, scale) = pop(Duration::from_millis(100), true);
        assert!(half > 0.5 && half < 1.0, "{half}");
        assert!(scale > 0.95 && scale < 1.0, "{scale}");
        assert_eq!(pop(POP, true), (1.0, 1.0));
        assert_eq!(pop(Duration::from_secs(5), true), (1.0, 1.0));
        // Reduced motion: its size whole, its opacity fading in.
        let (faded, whole) = pop(Duration::from_millis(100), false);
        assert_eq!((faded, whole), (half, 1.0));
        assert_eq!(pop(Duration::ZERO, false), (0.0, 1.0));
    }

    #[test]
    fn the_kinds_colours() {
        assert_eq!(colour(Kind::Microphone, false), rgb(0xF5A35B));
        assert_eq!(colour(Kind::Microphone, true), rgb(0xA9B2C2));
        assert_eq!(colour(Kind::Camera, true), rgb(0x6CC287));
        assert_eq!(colour(Kind::Screen, false), rgb(0x8FB3FF));
    }
}
