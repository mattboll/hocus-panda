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
//! A recording is a tile of its own (5.10's item, 5.12b), after the
//! camera's and before the screen's: 36 × 42, radius 8,
//! `rgba(242,84,91,0.16)` in a 1 px `rgba(242,84,91,0.55)` ring, its 10 px
//! dot `#F2545B` 4 px over its time's 16 px line (Martian Mono 12 px,
//! `#FFD9DB`). The dot beats from 1 to 0.35 of its opacity and back over
//! 1.4 s; paused, it is amber `#F5B44A` and still, the time "pause".
//!
//! It pops in over 200 ms as it appears (`ease-out`, from 0.9 of its size
//! and transparent), a fade alone with reduced motion; then it draws only
//! when what it shows changes: while a recording runs, as its time's second
//! changes, and every 50 ms while its dot beats.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{Form, IN_USE_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
        text::Alignment,
    },
};

use super::Kind;
use crate::theme::Ink;
use crate::{
    Message,
    levels::draw::{faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// A kind's tile, and between two.
pub const TILE: Size = Size::new(30.0, 24.0);
/// The recording's tile: the mini sign's 48 less its 6 px sides; 6 px in
/// above its 10 px dot, 4 px over its time's 16 px line, 6 px under it.
pub const REC_TILE: Size = Size::new(36.0, REC_PAD + REC_DOT + REC_GAP + REC_LINE + REC_PAD);
const REC_PAD: f32 = 6.0;
const REC_DOT: f32 = 10.0;
const REC_GAP: f32 = 4.0;
const REC_TIME: f32 = 12.0;
const REC_LINE: f32 = 16.0;
const REC_RADIUS: f32 = 8.0;
const REC_FILL: Ink = Ink::Dark(Color::from_rgba8(242, 84, 91, 0.16));
const REC_RING: Ink = Ink::Dark(Color::from_rgba8(242, 84, 91, 0.55));
const REC_INK: Ink = Ink::Dark(rgb(0xFFD9DB));
pub const REC_RED: Ink = Ink::Dark(rgb(0xF2545B));
pub const REC_AMBER: Ink = Ink::Dark(rgb(0xF5B44A));
const MONO: iced::Font = iced::Font::with_name("Martian Mono");
/// Martian Mono's advance, of its size: 700 of its 1000 units.
const MONO_ADVANCE: f32 = 0.7;
/// The dot's beat: from 1 to 0.35 of its opacity and back.
pub const BEAT: Duration = Duration::from_millis(1400);
const BEAT_LOW: f32 = 0.35;
/// It draws again this often while it beats.
const BEAT_STEP: Duration = Duration::from_millis(50);
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
const FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const LIT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
pub const MICROPHONE: Ink = Ink::Dark(rgb(0xF5A35B));
pub const CAMERA: Ink = Ink::Dark(rgb(0x6CC287));
pub const SCREEN: Ink = Ink::Dark(rgb(0x8FB3FF));
pub const MUTED: Ink = Ink::Dark(rgb(0xA9B2C2));
const MUTED_TILE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
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
        Kind::Microphone if muted => MUTED.get(),
        Kind::Microphone => MICROPHONE.get(),
        Kind::Camera => CAMERA.get(),
        Kind::Recording => REC_RED.get(),
        Kind::Screen => SCREEN.get(),
    }
}

/// Kind `kind`'s tile's size.
fn tile(kind: Kind) -> Size {
    match kind {
        Kind::Recording => REC_TILE,
        _ => TILE,
    }
}

/// The sign's size in `form` with `kinds`' tiles (one at least): stacked
/// in the mini form, the sign 48 wide; in a row in the full one, as tall
/// as its tallest tile.
pub fn size(form: Form, kinds: &[Kind]) -> Size {
    let sizes: Vec<Size> = if kinds.is_empty() {
        vec![TILE]
    } else {
        kinds.iter().map(|k| tile(*k)).collect()
    };
    let gaps = (sizes.len() - 1) as f32 * TILE_GAP;
    match form {
        Form::Mini => Size::new(
            WIDTH,
            2.0 * PAD_Y + sizes.iter().map(|s| s.height).sum::<f32>() + gaps,
        ),
        Form::Full => Size::new(
            2.0 * PAD_X + sizes.iter().map(|s| s.width).sum::<f32>() + gaps,
            2.0 * PAD_Y + sizes.iter().map(|s| s.height).fold(0.0, f32::max),
        ),
    }
}

/// Its tiles, in a sign at `rect` in `form`: centred across the mini
/// form's column, along the full form's row.
pub fn tiles(rect: Rectangle, form: Form, kinds: &[Kind]) -> Vec<Rectangle> {
    let mut along = 0.0;
    kinds
        .iter()
        .map(|kind| {
            let tile = tile(*kind);
            let at = match form {
                Form::Mini => {
                    Point::new(rect.center_x() - tile.width / 2.0, rect.y + PAD_Y + along)
                }
                Form::Full => {
                    Point::new(rect.x + PAD_X + along, rect.center_y() - tile.height / 2.0)
                }
            };
            along += TILE_GAP
                + match form {
                    Form::Mini => tile.height,
                    Form::Full => tile.width,
                };
            Rectangle::new(at, tile)
        })
        .collect()
}

/// The sign's top under the bar's in `form`: under the next meeting's
/// card when it shows (Day.2), else under the clock (`crate::column`).
fn top(form: Form) -> f32 {
    crate::column::shared_top(form, crate::column::Part::InUse)
}

/// Its surface's size and its top under the bar's, in `form` (both forms'
/// places while the bar switches), with `kinds`' tiles.
pub fn placement(form: Form, switching: bool, kinds: &[Kind]) -> ((u32, u32), f32) {
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

/// The dot's opacity at `t` into its beat (ease-in-out, 1 to 0.35 and
/// back, 5.10).
pub fn beat(t: Duration) -> f32 {
    let phase = (t.as_secs_f32() / BEAT.as_secs_f32()).fract();
    let k = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    1.0 - (1.0 - BEAT_LOW) * k
}

/// The time's size in a tile `width` wide: 12 px while `said` fits inside
/// its 1 px ring ("0:12"), smaller for what is longer ("12:04", "pause").
pub fn time_size(said: &str, width: f32) -> f32 {
    let wide = said.chars().count() as f32 * MONO_ADVANCE * REC_TIME;
    if wide <= 0.0 {
        return REC_TIME;
    }
    REC_TIME.min(REC_TIME * (width - 2.0) / wide)
}

/// When a recording that has recorded `elapsed` at `now` draws again: its
/// dot's next step while it beats (`motion`), else as its time's second
/// changes (a few milliseconds after, so as not to wake just before it).
pub fn next_draw(now: Instant, elapsed: Duration, motion: bool) -> Instant {
    if motion {
        return now + BEAT_STEP;
    }
    let into = Duration::from_nanos((elapsed.as_nanos() % 1_000_000_000) as u64);
    now + (Duration::from_secs(1) - into) + Duration::from_millis(5)
}

/// What the recording's tile shows: its time (or "pause"), whether it is
/// paused, its dot's opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct RecShown {
    pub said: String,
    pub paused: bool,
    pub dot: f32,
}

impl RecShown {
    /// The recording `rec` at `now`: one that ended says the time it ended
    /// at, still, while it lingers.
    pub fn of(
        rec: &super::Seen<super::Rec>,
        now: Instant,
        words: super::Words,
        motion: bool,
    ) -> Self {
        let paused = rec.what.shown.paused;
        let elapsed = rec.what.elapsed(rec.ended.unwrap_or(now));
        Self {
            said: if paused {
                words.paused().to_owned()
            } else {
                self::elapsed(elapsed)
            },
            paused,
            dot: if paused || !motion || !rec.live() {
                1.0
            } else {
                beat(elapsed)
            },
        }
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
    /// It pops in from smaller and the recording's dot beats (no reduced
    /// motion, not on the CPU).
    pub motion: bool,
    pub words: super::Words,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    hover: bool,
    drawn: Option<Instant>,
}

/// What the sign last said it shows at rest, for the log.
static SAID: Mutex<Option<String>> = Mutex::new(None);

/// What the recording's tile last said, for the log.
static READ: Mutex<Option<String>> = Mutex::new(None);

/// The sign went: when it comes again, it says so again.
pub fn unsaid() {
    *SAID.lock().unwrap_or_else(PoisonError::into_inner) = None;
    *READ.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

impl Sign {
    /// The surface's top under the bar's, as placed for its forms now.
    fn surface_top(&self, kinds: &[Kind]) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false, kinds).1,
            _ => placement(Form::Full, true, kinds).1,
        }
    }

    /// The sign in `form` on a surface `size` large, centred in the bar as
    /// wide as it is drawn now.
    fn rect_of(&self, form: Form, size: Size, kinds: &[Kind]) -> Rectangle {
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
    fn item(&self, size: Size, kinds: &[Kind]) -> Option<Rectangle> {
        match self.forms.as_slice() {
            [(form, _)] if !kinds.is_empty() => Some(self.rect_of(*form, size, kinds)),
            _ => None,
        }
    }

    /// `rect` (the surface's) in the bar's frame: from its right edge
    /// (negative inside it), from its top.
    fn in_bar(&self, rect: Rectangle, size: Size, kinds: &[Kind]) -> Rectangle {
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
                let uses = super::snapshot();
                // Popping in: drawn again until it is whole.
                let popping = uses
                    .appeared
                    .filter(|appeared| now.saturating_duration_since(*appeared) < POP)
                    .map(|_| *now + POP_STEP);
                // Recording: as its time's second changes, and while its
                // dot beats. Paused, ended or none: nothing wakes.
                let recording = uses
                    .live_recording()
                    .filter(|rec| !rec.shown.paused)
                    .map(|rec| next_draw(*now, rec.elapsed(*now), self.motion));
                let next = match (popping, recording) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }?;
                Some(canvas::Action::request_redraw_at(next))
            }
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.at);
                state.at = at;
                let kinds = super::snapshot().kinds();
                let kinds = kinds.as_slice();
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
        let under = crate::backdrop::under(renderer, size, self.backdrop);
        let uses = super::snapshot();
        let kinds = uses.kinds();
        if let Some(rect) = self.item(size, &kinds) {
            let in_bar = self.in_bar(rect, size, &kinds);
            popover::with(|c| {
                c.set_items(&self.bar, IN_USE_NAMESPACE, vec![(Item::InUse, in_bar)])
            });
        }
        if kinds.is_empty() {
            return crate::backdrop::over(under, frame);
        }
        let now = state.drawn.unwrap_or_else(Instant::now);
        let t = uses
            .appeared
            .map_or(POP, |appeared| now.saturating_duration_since(appeared));
        let (opacity, scale) = pop(t, self.motion);
        let rec = uses
            .recording
            .as_ref()
            .map(|rec| RecShown::of(rec, now, self.words, self.motion));
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
            let rect = self.rect_of(form, size, &kinds);
            let lit = still && (state.hover || open);
            let centre = Vector::new(rect.center_x(), rect.center_y());
            frame.with_save(|f| {
                f.translate(centre);
                f.scale(scale);
                f.translate(Vector::new(-centre.x, -centre.y));
                let shown = Shown {
                    kinds: &kinds,
                    muted: uses.muted,
                    rec: rec.as_ref(),
                };
                draw_sign(f, rect, form, &shown, lit, alpha * opacity);
            });
            if focused {
                ring(&mut frame, rect, RADIUS);
            }
        }
        if let Some(rec) = rec.as_ref().filter(|_| still) {
            let mut read = READ.lock().unwrap_or_else(PoisonError::into_inner);
            if read.as_deref() != Some(rec.said.as_str()) {
                tracing::debug!(
                    time = %rec.said,
                    paused = rec.paused,
                    "the in-use sign's recording reads"
                );
                *read = Some(rec.said.clone());
            }
        }
        if t >= POP && still {
            let said = format!(
                "{kinds:?} muted={} paused={:?} lit={}",
                uses.muted,
                rec.as_ref().map(|r| r.paused),
                still && (state.hover || open)
            );
            let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.as_deref() != Some(said.as_str()) {
                tracing::debug!(
                    kinds = ?kinds,
                    muted = uses.muted,
                    paused = ?rec.as_ref().map(|r| r.paused),
                    form = ?self.forms.first().map(|f| f.0),
                    "the in-use sign is drawn at rest"
                );
                *kept = Some(said);
            }
        }
        crate::backdrop::over(under, frame)
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

/// What the sign shows: its kinds, the microphone muted, the recording.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    pub kinds: &'a [Kind],
    pub muted: bool,
    pub rec: Option<&'a RecShown>,
}

/// The sign at `rect` in `form`: its tint, then a tile per kind.
pub fn draw_sign(
    f: &mut Frame,
    rect: Rectangle,
    form: Form,
    shown: &Shown<'_>,
    lit: bool,
    alpha: f32,
) {
    let Shown { kinds, muted, rec } = *shown;
    f.fill(
        &kit::rounded(rect, RADIUS),
        faded(if lit { LIT.get() } else { FILL.get() }, alpha),
    );
    for (kind, tile) in kinds.iter().zip(tiles(rect, form, kinds)) {
        if *kind == Kind::Recording {
            if let Some(rec) = rec {
                draw_rec(f, tile, rec, alpha);
            }
            continue;
        }
        let grey = *kind == Kind::Microphone && muted;
        let ink = colour(*kind, muted);
        let tint = if grey {
            MUTED_TILE.get()
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

/// The recording's tile at `rect` (5.10's item): its tint and ring, its dot
/// over its time.
pub fn draw_rec(f: &mut Frame, rect: Rectangle, rec: &RecShown, alpha: f32) {
    f.fill(
        &kit::rounded(rect, REC_RADIUS),
        faded(REC_FILL.get(), alpha),
    );
    f.stroke(
        &kit::rounded(
            Rectangle::new(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
            ),
            REC_RADIUS - 0.5,
        ),
        Stroke::default()
            .with_color(faded(REC_RING.get(), alpha))
            .with_width(1.0),
    );
    let dot = Point::new(rect.center_x(), rect.y + REC_PAD + REC_DOT / 2.0);
    let colour = if rec.paused { REC_AMBER } else { REC_RED };
    f.fill(
        &Path::circle(dot, REC_DOT / 2.0),
        faded(colour.get(), alpha * rec.dot),
    );
    f.fill_text(kit::text_in(
        MONO,
        kit::text(
            rec.said.clone(),
            Point::new(
                rect.center_x(),
                rect.y + REC_PAD + REC_DOT + REC_GAP + REC_LINE / 2.0,
            ),
            time_size(&rec.said, rect.width),
            faded(REC_INK.get(), alpha),
            Alignment::Center,
        ),
    ));
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
        // Its tile has no icon: its dot and its time.
        Kind::Recording => {}
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

    const MIC: Kind = Kind::Microphone;
    const CAM: Kind = Kind::Camera;
    const REC: Kind = Kind::Recording;
    const SCR: Kind = Kind::Screen;

    #[test]
    fn the_recordings_time_reads_and_its_dot_beats() {
        assert_eq!(elapsed(Duration::from_secs(12)), "0:12");
        assert_eq!(elapsed(Duration::from_secs(724)), "12:04");
        assert_eq!(elapsed(Duration::from_secs(3723)), "1:02:03");
        assert!((beat(Duration::ZERO) - 1.0).abs() < 1e-4);
        assert!((beat(Duration::from_millis(700)) - BEAT_LOW).abs() < 1e-4);
        assert!((beat(BEAT) - 1.0).abs() < 1e-3);
        // 12 px while it fits in the tile's 34 px inside its ring; longer,
        // as wide as that.
        assert_eq!(time_size("0:12", 36.0), 12.0);
        let fit = |said: &str| said.chars().count() as f32 * MONO_ADVANCE * time_size(said, 36.0);
        assert!((fit("12:04") - 34.0).abs() < 1e-3);
        assert!((fit("1:02:03") - 34.0).abs() < 1e-3);
        assert!((fit("pause") - 34.0).abs() < 1e-3);
    }

    #[test]
    fn the_recording_draws_again_as_its_second_changes_or_as_its_dot_beats() {
        let now = Instant::now();
        // Still: just after the time's next second.
        let next = next_draw(now, Duration::from_millis(12_300), false);
        assert_eq!(next - now, Duration::from_millis(705));
        let next = next_draw(now, Duration::from_secs(12), false);
        assert_eq!(next - now, Duration::from_millis(1005));
        // Beating: every 50 ms.
        assert_eq!(
            next_draw(now, Duration::from_millis(12_300), true) - now,
            BEAT_STEP
        );
    }

    #[test]
    fn the_recordings_tile_says_its_time_or_pause_and_stops_once_ended() {
        use hocus_link::RecordingShown;
        let t0 = Instant::now();
        let fr = super::super::Words::new(crate::clock::time::Lang::French);
        let en = super::super::Words::new(crate::clock::time::Lang::English);
        let mut rec = super::super::Seen {
            what: super::super::Rec {
                shown: RecordingShown {
                    id: 1,
                    paused: false,
                    elapsed: 2_000,
                },
                at: t0,
            },
            since: t0,
            ended: None,
        };
        let at = |s: u64| t0 + Duration::from_millis(s);
        let shown = RecShown::of(&rec, at(10_500), fr, false);
        assert_eq!(shown.said, "0:12");
        assert!(!shown.paused);
        assert_eq!(shown.dot, 1.0);
        // Beating with motion (2.1 s in: its lowest); still without.
        assert!(RecShown::of(&rec, at(100), fr, true).dot < 0.5);
        // Ended: the time it ended at, still.
        rec.ended = Some(at(3_000));
        let shown = RecShown::of(&rec, at(5_900), fr, true);
        assert_eq!((shown.said.as_str(), shown.dot), ("0:05", 1.0));
        rec.ended = None;
        rec.what.shown.paused = true;
        assert_eq!(RecShown::of(&rec, at(9_000), fr, true).said, "pause");
        let paused = RecShown::of(&rec, at(9_000), en, true);
        assert_eq!((paused.said.as_str(), paused.dot), ("paused", 1.0));
        assert!(paused.paused);
    }

    #[test]
    fn one_tile_a_kind_stacked_in_the_mini_form_in_a_row_in_the_full_one() {
        // One kind: the same 48 × 36 sign in both forms.
        assert_eq!(size(Form::Mini, &[MIC]), Size::new(48.0, 36.0));
        assert_eq!(size(Form::Full, &[SCR]), Size::new(48.0, 36.0));
        // Three: stacked, 4 px apart; in a row.
        let three = [MIC, CAM, SCR];
        assert_eq!(
            size(Form::Mini, &three),
            Size::new(48.0, 12.0 + 3.0 * 24.0 + 8.0)
        );
        assert_eq!(
            size(Form::Full, &three),
            Size::new(18.0 + 3.0 * 30.0 + 8.0, 36.0)
        );
        let mini = tiles(
            Rectangle::new(Point::ORIGIN, size(Form::Mini, &three)),
            Form::Mini,
            &three,
        );
        assert_eq!(mini[0], Rectangle::new(Point::new(9.0, 6.0), TILE));
        assert_eq!(mini[2].y, 6.0 + 2.0 * 28.0);
        assert!(mini.iter().all(|t| t.x == 9.0));
        let full = tiles(
            Rectangle::new(Point::ORIGIN, size(Form::Full, &three)),
            Form::Full,
            &three,
        );
        assert_eq!(full[1], Rectangle::new(Point::new(9.0 + 34.0, 6.0), TILE));
        assert!(full.iter().all(|t| t.y == 6.0));
    }

    #[test]
    fn the_recordings_tile_is_36_by_42_in_both_forms() {
        assert_eq!(REC_TILE, Size::new(36.0, 42.0));
        // Alone: the sign's 6 px sides around it in the mini form; 9 px in
        // the full one's row.
        assert_eq!(size(Form::Mini, &[REC]), Size::new(48.0, 54.0));
        assert_eq!(size(Form::Full, &[REC]), Size::new(54.0, 54.0));
        let all = [MIC, CAM, REC, SCR];
        // Stacked under the microphone's and the camera's, over the
        // screen's.
        let mini = size(Form::Mini, &all);
        assert_eq!(mini, Size::new(48.0, 12.0 + 3.0 * 24.0 + 42.0 + 12.0));
        let t = tiles(Rectangle::new(Point::ORIGIN, mini), Form::Mini, &all);
        assert_eq!(t[2], Rectangle::new(Point::new(6.0, 6.0 + 56.0), REC_TILE));
        assert_eq!(t[3].y, 6.0 + 56.0 + 42.0 + 4.0);
        assert_eq!(t[3].x, 9.0);
        // In the row, the sign grows by its width; the others centred on
        // its height.
        let full = size(Form::Full, &all);
        assert_eq!(full, Size::new(18.0 + 3.0 * 30.0 + 36.0 + 12.0, 54.0));
        let t = tiles(Rectangle::new(Point::ORIGIN, full), Form::Full, &all);
        assert_eq!(t[2], Rectangle::new(Point::new(9.0 + 68.0, 6.0), REC_TILE));
        assert_eq!(t[3].x, 9.0 + 68.0 + 40.0);
        assert_eq!(t[0].y, 15.0);
        assert_eq!(colour(REC, false), rgb(0xF2545B));
    }

    #[test]
    fn the_sign_sits_under_the_clock_its_surface_holding_both_forms_while_switching() {
        let ((w, h), top) = placement(Form::Mini, false, &[MIC]);
        assert_eq!(w, 54);
        assert_eq!(top, crate::column::MINI_TOP - ROOM);
        assert_eq!(h, 36 + 10);
        let ((_, h3), _) = placement(Form::Mini, false, &[MIC, CAM, SCR]);
        assert_eq!(h3, 92 + 10);
        let ((w, h), top) = placement(Form::Full, false, &[MIC, CAM, SCR]);
        assert_eq!((w, h, top), (194, 46, crate::column::TOP - ROOM));
        let ((_, h), _) = placement(Form::Mini, false, &[REC]);
        assert_eq!(h, 54 + 10);
        // While switching, both forms' places on one surface.
        let ((_, h), top) = placement(Form::Full, true, &[MIC, CAM]);
        assert_eq!(top, crate::column::MINI_TOP - ROOM);
        assert!(top + h as f32 >= crate::column::TOP + size(Form::Full, &[MIC, CAM]).height);
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
