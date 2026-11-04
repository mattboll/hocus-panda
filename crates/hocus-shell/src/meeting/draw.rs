//! The next meeting's surface (Day.2, variant B), under the clock.
//!
//! - **The card** (full form): 44 px high, the bar's inner width (172 px),
//!   radius 10, `rgba(255,255,255,0.05)` in a 1 px `rgba(255,255,255,0.06)`
//!   border; 6 px in, a 6 × 30 stripe in the calendar's colour (radius 3),
//!   then 8 px, its title (Manrope 700, 13.5 px, one line) over its time
//!   line (12 px, `text.dim`: the start in Martian Mono 11 px, then
//!   " · dans 25 min"). With a video link, the join button: 30 × 30,
//!   radius 8, 6 px from the card's right, a 16 px camera. 10 min or less,
//!   2 min or less, and in progress, it turns blue ([`Look`]), its colours
//!   changing over 300 ms (at once without motion); in progress a 2 px line
//!   along its bottom shows the share gone.
//! - **The mark** (mini form): 30 × 4, radius 2, in a 48 × 16 button.
//!
//! It draws when what it shows changes (the minute's change, the day's),
//! under the pointer, and while its colours change; never at rest.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{Form, MEETING_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
        text::Alignment,
    },
};

use super::{Card, Look, Phase, Words};
use crate::{
    Message,
    levels::draw::{DIM, MANROPE, MANROPE_BOLD, TEXT, faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// Around the card on its surface: the keyboard's ring.
const ROOM: f32 = 5.0;
/// The bar's sides' padding, and the card's width within them.
const SIDE: f32 = 12.0;
const CARD_WIDTH: f32 = 172.0;
const RADIUS: f32 = 10.0;
const BORDER: f32 = 1.0;
const PAD: f32 = 6.0;
const STRIPE: Size = Size::new(6.0, 30.0);
const STRIPE_GAP: f32 = 8.0;
const TITLE: f32 = 13.5;
const LINE_TEXT: f32 = 12.0;
const MONO_TEXT: f32 = 11.0;
const LINES_GAP: f32 = 1.0;
/// The time line's end before the join button (the title's is 6 px).
const LINE_TO_JOIN: f32 = 2.0;
const JOIN: f32 = 30.0;
const JOIN_RADIUS: f32 = 8.0;
const CAMERA: f32 = 16.0;
const PROGRESS: f32 = 2.0;
/// The mini form's mark and its button.
const MARK: Size = Size::new(30.0, 4.0);
const MARK_BUTTON: Size = Size::new(48.0, 16.0);
const MARK_RADIUS: f32 = 8.0;
const MARK_GLOW: f32 = 8.0;
/// The colours' change.
pub const FADE: Duration = Duration::from_millis(300);
const FADE_STEP: Duration = Duration::from_millis(16);

pub(crate) const MONO: iced::Font = iced::Font::with_name("Martian Mono");
pub(crate) const BLUE: Color = rgb(0x8FB3FF);
pub(crate) const JOIN_BLUE: Color = rgb(0x5B8DEF);
const JOIN_BLUE_HOVER: Color = rgb(0x6E9BF2);
const JOIN_GREY: Color = Color::from_rgba8(255, 255, 255, 0.09);
const JOIN_GREY_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.14);
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
const OPEN: Color = Color::from_rgba8(255, 255, 255, 0.08);

/// The card's fill and border in `look`.
pub fn card_colours(look: Look) -> (Color, Color) {
    match look {
        Look::Far => (
            Color::from_rgba8(255, 255, 255, 0.05),
            Color::from_rgba8(255, 255, 255, 0.06),
        ),
        Look::Soon => (
            Color::from_rgba8(143, 179, 255, 0.12),
            Color::from_rgba8(143, 179, 255, 0.30),
        ),
        Look::Now => (
            Color::from_rgba8(143, 179, 255, 0.18),
            Color::from_rgba8(143, 179, 255, 0.55),
        ),
        Look::Under => (
            Color::from_rgba8(143, 179, 255, 0.09),
            Color::from_rgba8(143, 179, 255, 0.22),
        ),
    }
}

/// The join button's fill and its camera's colour in `look`.
fn join_colours(look: Look, hover: bool) -> (Color, Color) {
    match (look, hover) {
        (Look::Far, false) => (JOIN_GREY, TEXT),
        (Look::Far, true) => (JOIN_GREY_HOVER, TEXT),
        (_, false) => (JOIN_BLUE, Color::WHITE),
        (_, true) => (JOIN_BLUE_HOVER, Color::WHITE),
    }
}

/// The relative time's colour and font in `look`: blue and 600 from 10
/// min, 700 from 2, blue and 600 in progress.
pub fn relative_style(look: Look) -> (Color, iced::Font) {
    match look {
        Look::Far => (DIM, MANROPE),
        Look::Soon | Look::Under => (BLUE, kit::MANROPE_SEMIBOLD),
        Look::Now => (BLUE, MANROPE_BOLD),
    }
}

/// The surface's top under the bar's in `form`: right under the clock.
fn top(form: Form) -> f32 {
    match form {
        Form::Full => crate::agents::draw::TOP,
        Form::Mini => crate::agents::MINI_TOP,
    }
}

/// Its surface's size and its top under the bar's, in `form` (both forms'
/// places while the bar switches).
pub fn placement(form: Form, switching: bool) -> ((u32, u32), f32) {
    let tall = |h: f32| (h + 2.0 * ROOM).ceil() as u32;
    let height = |form| match form {
        Form::Full => super::CARD_HEIGHT,
        Form::Mini => super::MARK_HEIGHT,
    };
    match (switching, form) {
        (true, _) => {
            let (a, b) = (top(Form::Full), top(Form::Mini));
            let bottom = (a + height(Form::Full)).max(b + height(Form::Mini));
            ((FULL_WIDTH, tall(bottom - a.min(b))), a.min(b) - ROOM)
        }
        (false, Form::Full) => ((FULL_WIDTH, tall(height(form))), top(form) - ROOM),
        (false, Form::Mini) => ((MINI_WIDTH, tall(height(form))), top(form) - ROOM),
    }
}

/// From `a` to `b` by `t`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// The card's part under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Card,
    Join,
    Mark,
}

impl Part {
    fn item(self) -> Item {
        match self {
            Self::Card => Item::Meeting,
            Self::Join => Item::Join,
            Self::Mark => Item::MeetingMark,
        }
    }
}

/// The next meeting of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub bar: String,
    pub forms: Vec<(Form, f32)>,
    pub bar_width: f32,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
    /// Its colours change over 300 ms (no reduced motion, not on the CPU).
    pub motion: bool,
    pub words: Words,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    hover: Option<Part>,
    drawn: Option<Instant>,
    /// The look as last drawn, and the one it changes from, since when.
    look: Option<Look>,
    fade: Option<(Look, Instant)>,
}

/// What it last said it shows at rest, for the log.
static SAID: Mutex<Option<String>> = Mutex::new(None);

impl View {
    fn surface_top(&self) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false).1,
            _ => placement(Form::Full, true).1,
        }
    }

    /// The card (full form), on a surface `size` large.
    fn card_rect(&self, size: Size) -> Rectangle {
        Rectangle::new(
            Point::new(
                size.width + 1.0 - SIDE - CARD_WIDTH,
                top(Form::Full) - self.surface_top(),
            ),
            Size::new(CARD_WIDTH, super::CARD_HEIGHT),
        )
    }

    /// The mark's button (mini form), centred in the bar as drawn now.
    fn mark_rect(&self, size: Size) -> Rectangle {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        Rectangle::new(
            Point::new(
                centre - MARK_BUTTON.width / 2.0,
                top(Form::Mini) - self.surface_top(),
            ),
            MARK_BUTTON,
        )
    }

    /// Its parts that take the pointer, not while the bar switches.
    fn parts(&self, size: Size, card: &Card) -> Vec<(Part, Rectangle)> {
        match self.forms.as_slice() {
            [(Form::Full, _)] => {
                let rect = self.card_rect(size);
                let mut parts = Vec::new();
                if card.video.is_some() {
                    parts.push((Part::Join, join_rect(rect)));
                }
                parts.push((Part::Card, rect));
                parts
            }
            [(Form::Mini, _)] => vec![(Part::Mark, self.mark_rect(size))],
            _ => Vec::new(),
        }
    }

    /// `rect` (the surface's) in the bar's frame.
    fn in_bar(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(
            Point::new(rect.x - size.width - 1.0, self.surface_top() + rect.y),
            rect.size(),
        )
    }
}

/// The join button in the card at `card`.
fn join_rect(card: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(
            card.x + card.width - BORDER - PAD - JOIN,
            card.center_y() - JOIN / 2.0,
        ),
        Size::new(JOIN, JOIN),
    )
}

impl canvas::Program<Message> for View {
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
                let look = super::snapshot().map(|c| c.phase.look());
                if let (Some(before), Some(look)) = (state.look, look)
                    && before != look
                    && self.motion
                {
                    state.fade = Some((before, *now));
                }
                state.look = look;
                let fading = state
                    .fade
                    .filter(|(_, since)| now.saturating_duration_since(*since) < FADE);
                if fading.is_none() {
                    state.fade = None;
                }
                fading.map(|_| canvas::Action::request_redraw_at(*now + FADE_STEP))
            }
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.at);
                state.at = at;
                let card = super::snapshot()?;
                let parts = self.parts(size, &card);
                let over = at.and_then(|at| {
                    parts
                        .iter()
                        .find(|(_, r)| r.contains(at))
                        .map(|(p, r)| (*p, *r))
                });
                match seen {
                    mouse::Event::CursorEntered
                    | mouse::Event::CursorMoved { .. }
                    | mouse::Event::CursorLeft => {
                        let part = over.map(|(p, _)| p);
                        if part == state.hover {
                            return None;
                        }
                        tracing::trace!(?part, ?at, "the pointer on the meeting card");
                        state.hover = part;
                        match over {
                            Some((part, rect)) => popover::event(popover::Event::ItemEnter {
                                bar: self.bar.clone(),
                                item: part.item(),
                                rect: self.in_bar(rect, size),
                            }),
                            None => {
                                let at = at.unwrap_or(Point::new(size.width / 2.0, size.height));
                                popover::event(popover::Event::ItemLeave {
                                    at: self
                                        .in_bar(Rectangle::new(at, Size::ZERO), size)
                                        .position(),
                                });
                            }
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) => {
                        let (part, rect) = over?;
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item: part.item(),
                            rect: self.in_bar(rect, size),
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
        let Some(card) = super::snapshot() else {
            return vec![frame.into_geometry()];
        };
        let parts = self.parts(size, &card);
        let items = parts
            .iter()
            .map(|(p, r)| (p.item(), self.in_bar(*r, size)))
            .collect();
        popover::with(|c| c.set_items(&self.bar, MEETING_NAMESPACE, items));
        let now = state.drawn.unwrap_or_else(Instant::now);
        let look = card.phase.look();
        // Its colours, from the look before while they change.
        let fade = state.fade.map(|(from, since)| {
            let t = now.saturating_duration_since(since).as_secs_f32() / FADE.as_secs_f32();
            (from, t.clamp(0.0, 1.0))
        });
        let colour = |of: &dyn Fn(Look) -> Color| match fade {
            Some((from, t)) => mix(of(from), of(look), t),
            None => of(look),
        };
        let still = self.forms.len() == 1;
        let (focused, open) = if still {
            popover::with(|c| {
                (
                    [Item::Meeting, Item::Join, Item::MeetingMark]
                        .into_iter()
                        .find(|i| c.focused(&self.bar, *i)),
                    c.shows(&self.bar, Item::MeetingMark),
                )
            })
        } else {
            (None, false)
        };
        for &(form, alpha) in &self.forms {
            match form {
                Form::Full => {
                    let rect = self.card_rect(size);
                    let shown = Shown {
                        card: &card,
                        fill: colour(&|l| card_colours(l).0),
                        border: colour(&|l| card_colours(l).1),
                        join: colour(&|l| join_colours(l, state.hover == Some(Part::Join)).0),
                        lit: still && state.hover == Some(Part::Card),
                    };
                    draw_card(&mut frame, rect, &shown, self.words, alpha);
                    match focused {
                        Some(Item::Meeting) => ring(&mut frame, rect, RADIUS),
                        Some(Item::Join) => ring(&mut frame, join_rect(rect), JOIN_RADIUS),
                        _ => {}
                    }
                }
                Form::Mini => {
                    let rect = self.mark_rect(size);
                    let lit = if open {
                        Some(OPEN)
                    } else if still && state.hover == Some(Part::Mark) {
                        Some(HOVER)
                    } else {
                        None
                    };
                    if let Some(lit) = lit {
                        frame.fill(&kit::rounded(rect, MARK_RADIUS), faded(lit, alpha));
                    }
                    draw_mark(&mut frame, rect.center(), card.phase, alpha);
                    if focused == Some(Item::MeetingMark) {
                        ring(&mut frame, rect, MARK_RADIUS);
                    }
                }
            }
        }
        if still && fade.is_none() {
            let said = format!(
                "{} form={:?} lit={:?}",
                self.words.said(&card),
                self.forms.first().map(|f| f.0),
                state.hover
            );
            let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.as_deref() != Some(said.as_str()) {
                // Its names, for screen readers once iced has them (§11).
                tracing::debug!(
                    said = %said,
                    name = %self.words.name(&card),
                    join = card.video.as_ref().map(|_| self.words.join_call()),
                    "the meeting card is drawn at rest"
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
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// What a card draws, its colours as they are now.
struct Shown<'a> {
    card: &'a Card,
    fill: Color,
    border: Color,
    join: Color,
    lit: bool,
}

/// `text`'s width in `font` at `size`.
fn width(text: &str, size: f32, font: iced::Font) -> f32 {
    kit::measure(text, size, font)
}

/// The card at `rect`.
fn draw_card(f: &mut Frame, rect: Rectangle, shown: &Shown<'_>, words: Words, alpha: f32) {
    let card = shown.card;
    let look = card.phase.look();
    f.fill(&kit::rounded(rect, RADIUS), faded(shown.fill, alpha));
    if shown.lit {
        f.fill(&kit::rounded(rect, RADIUS), faded(HOVER, alpha));
    }
    f.stroke(
        &kit::rounded(
            Rectangle::new(
                Point::new(rect.x + BORDER / 2.0, rect.y + BORDER / 2.0),
                Size::new(rect.width - BORDER, rect.height - BORDER),
            ),
            RADIUS - BORDER / 2.0,
        ),
        Stroke::default()
            .with_color(faded(shown.border, alpha))
            .with_width(BORDER),
    );
    let [r, g, b] = card.colour;
    let stripe = Rectangle::new(
        Point::new(rect.x + BORDER + PAD, rect.center_y() - STRIPE.height / 2.0),
        STRIPE,
    );
    f.fill(
        &kit::rounded(stripe, STRIPE.width / 2.0),
        faded(Color::from_rgb8(r, g, b), alpha),
    );
    let x = stripe.x + STRIPE.width + STRIPE_GAP;
    let right = match card.video {
        Some(_) => join_rect(rect).x - PAD,
        None => rect.x + rect.width - BORDER - PAD,
    };
    let room = (right - x).max(0.0);
    // The title over the time line, 1 px apart, centred in the card.
    let (title_h, line_h) = (TITLE * crate::levels::draw::LINE, LINE_TEXT * 1.25);
    let top = rect.center_y() - (title_h + LINES_GAP + line_h) / 2.0;
    let title_middle = top + title_h / 2.0;
    let line_middle = top + title_h + LINES_GAP + line_h / 2.0;
    let title = kit::elide(&card.title, room, |t| width(t, TITLE, MANROPE_BOLD));
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            title,
            Point::new(x, title_middle),
            TITLE,
            faded(TEXT, alpha),
            Alignment::Left,
        ),
    ));
    let (rel_colour, rel_font) = relative_style(look);
    let relative = words.relative(card.phase);
    let mut at = x;
    if let Phase::Ahead { .. } = card.phase {
        f.fill_text(kit::text_in(
            MONO,
            kit::text(
                card.start.clone(),
                Point::new(at, line_middle),
                MONO_TEXT,
                faded(DIM, alpha),
                Alignment::Left,
            ),
        ));
        at += width(&card.start, MONO_TEXT, MONO);
        let dot = " · ";
        f.fill_text(kit::text(
            dot.to_owned(),
            Point::new(at, line_middle),
            LINE_TEXT,
            faded(DIM, alpha),
            Alignment::Left,
        ));
        at += width(dot, LINE_TEXT, MANROPE);
    }
    // The time line runs to 2 px from the button: "14:25 · dans 25 min"
    // is 112 px in these fonts, the title's 108 (the spec's "about 110").
    let line_right = match card.video {
        Some(_) => join_rect(rect).x - LINE_TO_JOIN,
        None => right,
    };
    let relative = kit::elide(&relative, (line_right - at).max(0.0), |t| {
        width(t, LINE_TEXT, rel_font)
    });
    f.fill_text(kit::text_in(
        rel_font,
        kit::text(
            relative,
            Point::new(at, line_middle),
            LINE_TEXT,
            faded(rel_colour, alpha),
            Alignment::Left,
        ),
    ));
    if card.video.is_some() {
        let join = join_rect(rect);
        f.fill(&kit::rounded(join, JOIN_RADIUS), faded(shown.join, alpha));
        let (_, ink) = join_colours(look, false);
        camera(
            f,
            Rectangle::new(
                Point::new(
                    join.center_x() - CAMERA / 2.0,
                    join.center_y() - CAMERA / 2.0,
                ),
                Size::new(CAMERA, CAMERA),
            ),
            faded(ink, alpha),
        );
    }
    if let Phase::Under { share, .. } = card.phase
        && share > 0.0
    {
        let inner = Rectangle::new(
            Point::new(rect.x + BORDER, rect.y + BORDER),
            Size::new(rect.width - 2.0 * BORDER, rect.height - 2.0 * BORDER),
        );
        f.fill(
            &bottom_strip(inner, RADIUS - BORDER, PROGRESS, share),
            faded(BLUE, alpha),
        );
    }
}

/// The bottom `height` px of a rounded `rect` (corners `radius`), from its
/// left to `share` of its width: the card's line of the time gone, cut
/// round at its corners as the card is.
fn bottom_strip(rect: Rectangle, radius: f32, height: f32, share: f32) -> Path {
    let bottom = rect.y + rect.height;
    let end = rect.x + rect.width * share.clamp(0.0, 1.0);
    // How far in the rounded edge is at `y`, near the bottom.
    let inset = |y: f32| {
        let dy = (y - (bottom - radius)).max(0.0);
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    };
    const STEPS: usize = 6;
    let ys: Vec<f32> = (0..=STEPS)
        .map(|i| bottom - height + height * i as f32 / STEPS as f32)
        .collect();
    Path::new(|b| {
        let left = |y: f32| rect.x + inset(y);
        let right = |y: f32| (rect.x + rect.width - inset(y)).min(end).max(left(y));
        b.move_to(Point::new(left(ys[0]), ys[0]));
        for y in &ys[1..] {
            b.line_to(Point::new(left(*y), *y));
        }
        for y in ys.iter().rev() {
            b.line_to(Point::new(right(*y), *y));
        }
        b.close();
    })
}

/// The mini form's mark, centred on `centre`: faint blue more than 10 min
/// ahead, bright and glowing within 10, filled to the share gone in
/// progress.
pub fn draw_mark(f: &mut Frame, centre: Point, phase: Phase, alpha: f32) {
    let rect = Rectangle::new(
        Point::new(centre.x - MARK.width / 2.0, centre.y - MARK.height / 2.0),
        MARK,
    );
    let radius = MARK.height / 2.0;
    match phase.look() {
        Look::Far => f.fill(
            &kit::rounded(rect, radius),
            faded(Color::from_rgba8(143, 179, 255, 0.60), alpha),
        ),
        Look::Soon | Look::Now => {
            // An 8 px glow, as rings fading out.
            const RINGS: usize = 8;
            for i in (1..=RINGS).rev() {
                let out = MARK_GLOW * i as f32 / RINGS as f32;
                let glow = Rectangle::new(
                    Point::new(rect.x - out, rect.y - out),
                    Size::new(rect.width + 2.0 * out, rect.height + 2.0 * out),
                );
                let k = 1.0 - i as f32 / (RINGS as f32 + 1.0);
                f.fill(
                    &kit::rounded(glow, radius + out),
                    faded(Color::from_rgba8(143, 179, 255, 0.95 * 0.22 * k * k), alpha),
                );
            }
            f.fill(&kit::rounded(rect, radius), faded(BLUE, alpha));
        }
        Look::Under => {
            f.fill(
                &kit::rounded(rect, radius),
                faded(Color::from_rgba8(143, 179, 255, 0.25), alpha),
            );
            if let Phase::Under { share, .. } = phase
                && share > 0.0
            {
                let gone = Rectangle::new(
                    rect.position(),
                    Size::new((rect.width * share).max(MARK.height), rect.height),
                );
                f.fill(&kit::rounded(gone, radius), faded(BLUE, alpha));
            }
        }
    }
}

/// The mockup's camera (24 px drawing, stroked 2 px at 24) in `rect`.
pub fn camera(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(3.5, 7.0));
        b.line_to(p(15.0, 7.0));
        b.line_to(p(15.0, 17.0));
        b.line_to(p(3.5, 17.0));
        b.close();
        b.move_to(p(15.0, 10.5));
        b.line_to(p(20.5, 7.5));
        b.line_to(p(20.5, 16.5));
        b.line_to(p(15.0, 13.5));
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(2.0 * k)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

/// The mockup's pin (24 px drawing, stroked 2 px at 24) in `rect`.
pub fn pin(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        // Its drop: from its tip at the bottom, round over its head.
        b.move_to(p(12.0, 21.0));
        b.bezier_curve_to(p(12.0, 21.0), p(5.5, 14.8), p(5.5, 10.0));
        b.arc(canvas::path::Arc {
            center: p(12.0, 10.0),
            radius: 6.5 * k,
            start_angle: iced::Radians(std::f32::consts::PI),
            end_angle: iced::Radians(2.0 * std::f32::consts::PI),
        });
        b.bezier_curve_to(p(18.5, 14.8), p(12.0, 21.0), p(12.0, 21.0));
        b.close();
        b.circle(p(12.0, 10.0), 2.2 * k);
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(2.0 * k)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surface_sits_under_the_clock_in_both_forms() {
        // The card's 44 px at the agents' old top, 5 px of room around it.
        assert_eq!(placement(Form::Full, false), ((194, 54), 95.0));
        // The mark's 16 px button at the mini form's.
        assert_eq!(placement(Form::Mini, false), ((54, 26), 63.0));
        // Switching: both.
        let ((w, h), top) = placement(Form::Full, true);
        assert_eq!((w, top), (194, 63.0));
        assert!(top + h as f32 >= 100.0 + 44.0);
    }

    #[test]
    fn its_colours_by_how_near_the_meeting_is() {
        let (fill, border) = card_colours(Look::Far);
        assert_eq!((fill.a, border.a), (0.05, 0.06));
        assert_eq!(
            card_colours(Look::Soon).0,
            Color::from_rgba8(143, 179, 255, 0.12)
        );
        assert_eq!(
            card_colours(Look::Soon).1,
            Color::from_rgba8(143, 179, 255, 0.30)
        );
        assert_eq!(
            card_colours(Look::Now).0,
            Color::from_rgba8(143, 179, 255, 0.18)
        );
        assert_eq!(
            card_colours(Look::Now).1,
            Color::from_rgba8(143, 179, 255, 0.55)
        );
        assert_eq!(
            card_colours(Look::Under).0,
            Color::from_rgba8(143, 179, 255, 0.09)
        );
        assert_eq!(
            card_colours(Look::Under).1,
            Color::from_rgba8(143, 179, 255, 0.22)
        );
        assert_eq!(join_colours(Look::Far, false), (JOIN_GREY, TEXT));
        assert_eq!(join_colours(Look::Soon, false), (JOIN_BLUE, Color::WHITE));
        assert_eq!(join_colours(Look::Under, false), (JOIN_BLUE, Color::WHITE));
        assert_eq!(relative_style(Look::Far), (DIM, MANROPE));
        assert_eq!(relative_style(Look::Soon), (BLUE, kit::MANROPE_SEMIBOLD));
        assert_eq!(relative_style(Look::Now), (BLUE, MANROPE_BOLD));
        assert_eq!(relative_style(Look::Under), (BLUE, kit::MANROPE_SEMIBOLD));
    }

    #[test]
    fn the_join_button_and_the_titles_room() {
        let card = Rectangle::new(Point::new(11.0, 5.0), Size::new(CARD_WIDTH, 44.0));
        let join = join_rect(card);
        // 6 px from the card's right (inside its border), centred.
        assert_eq!(join.x + join.width, card.x + card.width - 7.0);
        assert_eq!(join.center_y(), card.center_y());
        // The title's room: about 110 px with it, 140 without.
        let x = card.x + BORDER + PAD + STRIPE.width + STRIPE_GAP;
        assert_eq!(join.x - PAD - x, 108.0);
        assert_eq!(card.x + card.width - BORDER - PAD - x, 144.0);
    }
}
