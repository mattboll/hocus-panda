//! The bubbles as the 5.3 mockup draws them (sizes at ×1), and the "+N"
//! chip under them.
//!
//! A bubble's card: 340 px wide, radius 14, a 1 px rim, the popovers' tint
//! (near opaque, `rgba(22,27,42,0.95) → rgba(14,17,28,0.97)`); its 32 px
//! icon, its first line (the application, "· maintenant", "Urgent" for a
//! critical one), its title in bold 13.5 px, its body at 12.5 px on two
//! lines at most, its progress, a ✕ at its top right, its actions as small
//! buttons under it, and a 2 px line at its bottom counting its time down.
//! A critical one has an orange rim and glow.
//!
//! On a GPU the surface is transparent around the card, which its shadow
//! (and a critical one's glow) is drawn in, as the mockup's CSS; it takes
//! the pointer on the card only. On the CPU its buffer has no alpha: the
//! surface is the card alone, opaque, its corners cut by the compositor.
//!
//! The countdown's line draws itself again as it shortens, about a pixel
//! at a time (30 times a second at most), while it runs: nothing draws at
//! rest, nor while the countdowns are paused.

use hocus_words::shell::notifications_draw as txt;
use std::time::{Duration, Instant};

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{
    note::{Note, Urgency},
    stack::Countdown,
};
use crate::theme::Ink;
use crate::{
    clock::time::Lang,
    levels::{
        draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
        icons::Icon,
    },
    popover::kit::{self, MANROPE_SEMIBOLD},
};

// --- The mockup's sizes ------------------------------------------------------------

/// The card's width, its rim, its radius.
pub const WIDTH: f32 = 340.0;
const BORDER: f32 = 1.0;
const RADIUS: f32 = 14.0;
/// Inside the rim: the padding of the card's clickable part.
const PAD_TOP: f32 = 12.0;
const PAD_BOTTOM: f32 = 10.0;
const PAD_LEFT: f32 = 12.0;
const PAD_RIGHT: f32 = 34.0;
pub const ICON: f32 = 32.0;
const ICON_GAP: f32 = 11.0;
/// The text's column.
pub const COLUMN_X: f32 = BORDER + PAD_LEFT + ICON + ICON_GAP;
pub const COLUMN: f32 = WIDTH - COLUMN_X - PAD_RIGHT - BORDER;
const COLUMN_GAP: f32 = 3.0;
/// The first line, 11.5 px; the word after the name 6 px after it,
/// "Urgent" 4 px more.
const SMALL: f32 = 11.5;
const SMALL_GAP: f32 = 6.0;
const URGENT_GAP: f32 = 4.0;
/// The title, 13.5 px bold on 1.3 lines; the body, 12.5 px on 1.4.
const TITLE: f32 = 13.5;
const TITLE_LINE: f32 = 1.3;
const BODY: f32 = 12.5;
const BODY_LINE: f32 = 1.4;
/// Lines of each at most.
const TITLE_LINES: usize = 2;
const BODY_LINES: usize = 2;
/// The progress: 4 px under the body, 5 px high.
const PROGRESS_TOP: f32 = 4.0;
pub const PROGRESS: f32 = 5.0;
/// The ✕: a 24 px circle, 8 px from the top right inside the rim.
const CLOSE: f32 = 24.0;
const CLOSE_IN: f32 = 8.0;
/// The actions: 28 px buttons, 12 px in, 6 px apart, 12 px from the
/// card's bottom; their text 12 px.
const ACTION: f32 = 28.0;
const ACTION_PAD: f32 = 12.0;
const ACTION_GAP: f32 = 6.0;
const ACTION_TEXT: f32 = 12.0;
const ACTION_RADIUS: f32 = 8.0;
const ACTIONS_RIGHT: f32 = WIDTH - BORDER - 12.0;
/// The countdown: 2 px, 14 px from the sides inside the rim.
const COUNTDOWN: f32 = 2.0;
const COUNTDOWN_IN: f32 = 14.0;
/// Its line draws again at most this often.
const COUNTDOWN_STEP: Duration = Duration::from_millis(33);

/// Room around the card on a GPU surface, for its shadow (`0 14px 40px`:
/// three deviations, and the offset below): left, top, right, bottom. The
/// screen's top and the bar's gap leave 8 px above and on the right.
const ROOM: (f32, f32, f32, f32) = (60.0, 8.0, 8.0, 74.0);

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.95));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.97));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const RIM_CRITICAL: Ink = Ink::Dark(rgba(242, 148, 91, 0.75));
const BODY_TEXT: Ink = Ink::Dark(rgb(0xC9D0DC));
const URGENT: Ink = Ink::Dark(rgb(0xF7A77A));
const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.03));
const CLOSE_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const ACTION_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const ACTION_FILL_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const ACTION_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.14));
const TRACK: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const PROGRESS_FILL: Ink = Ink::Dark(rgb(0x8FB3FF));
const RUNNING: Ink = Ink::Accent(0.7);
const PAUSED: Ink = Ink::Dark(rgba(255, 255, 255, 0.35));
/// The launcher's letter tile (5.1), its letter at the launcher's 19 px
/// for 48 px, here for 32 px.
const NEUTRAL: (Color, Color) = (rgb(0x5C6576), rgb(0x9AA3B4));
const LETTER: f32 = 13.0;
const NEUTRAL_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.14));
/// The shadow `0 14px 40px rgba(0,0,0,0.42)` and a critical one's glow
/// `0 0 24px rgba(242,148,91,0.25)`: offset, deviation, alpha.
const SHADOW: (f32, f32, f32) = (14.0, 20.0, 0.42);
const GLOW: (f32, f32, f32) = (0.0, 12.0, 0.25);

/// The chip: 11.5 px semibold, 4 px and 10 px in, radius 10.
const CHIP_TEXT: f32 = 11.5;
const CHIP_PAD: (f32, f32) = (4.0, 10.0);
const CHIP_RADIUS: f32 = 10.0;
const CHIP_FILL: Ink = Ink::Dark(rgba(14, 17, 28, 0.9));
const CHIP_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));

// --- Words ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn now() -> Self {
        Self {
            lang: Lang::of_messages(),
        }
    }

    fn when(self) -> &'static str {
        txt::NOW.get(self.lang)
    }

    fn urgent(self) -> &'static str {
        "Urgent"
    }

    /// The ✕'s name, for screen readers.
    pub fn close(self) -> &'static str {
        txt::CLOSE.get(self.lang)
    }

    fn waiting(self, n: usize) -> String {
        txt::waiting(self.lang, n)
    }
}

// --- Laying a bubble out ----------------------------------------------------------

/// A bubble laid out, in its card's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    /// The card.
    pub size: Size,
    /// The application's name as it fits, and where the words after it
    /// start.
    app: String,
    when_x: f32,
    urgent_x: Option<f32>,
    title: Vec<String>,
    body: Vec<String>,
    /// The progress's top.
    pub progress: Option<f32>,
    /// The actions: key, label, button.
    pub actions: Vec<(String, String, Rectangle)>,
    /// The card's clickable part, under its rim.
    button: f32,
}

/// `lines` of `text` broken to fit the column, `most` at most, the last
/// cut with an ellipsis when more were left.
fn lines(text: &str, size: f32, font: iced::Font, most: usize) -> Vec<String> {
    let width = |t: &str| kit::measure(t, size, font);
    let mut all: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .flat_map(|l| kit::wrap(l, COLUMN, width))
        .collect();
    let more = all.len() > most;
    all.truncate(most);
    let count = all.len();
    for (i, line) in all.iter_mut().enumerate() {
        let last = i + 1 == count;
        let cut = if last && more {
            kit::elide(&format!("{line}…"), COLUMN, width)
        } else {
            kit::elide(line, COLUMN, width)
        };
        *line = cut;
    }
    all
}

/// How `note`'s bubble is laid out.
pub fn lay(note: &Note) -> Laid {
    let words = Words::now();
    let small = |t: &str, font| kit::measure(t, SMALL, font);
    let when = small(words.when(), MANROPE);
    let urgent = (note.urgency == Urgency::Critical)
        .then(|| small(words.urgent(), MANROPE_BOLD) + SMALL_GAP + URGENT_GAP);
    let room = COLUMN - when - SMALL_GAP - urgent.unwrap_or(0.0);
    let app = kit::elide(note.app.trim(), room.max(0.0), |t| small(t, MANROPE_BOLD));
    let when_x = COLUMN_X + small(&app, MANROPE_BOLD) + SMALL_GAP;
    let urgent_x = urgent.map(|_| when_x + when + SMALL_GAP + URGENT_GAP);
    let title = lines(&note.summary, TITLE, MANROPE_BOLD, TITLE_LINES);
    let body = lines(&note.body, BODY, MANROPE, BODY_LINES);
    let mut column = SMALL * LINE;
    if !title.is_empty() {
        column += COLUMN_GAP + title.len() as f32 * TITLE * TITLE_LINE;
    }
    if !body.is_empty() {
        column += COLUMN_GAP + body.len() as f32 * BODY * BODY_LINE;
    }
    let progress = note.progress.map(|_| {
        let top = BORDER + PAD_TOP + column + COLUMN_GAP + PROGRESS_TOP;
        column += COLUMN_GAP + PROGRESS_TOP + PROGRESS;
        top
    });
    let button = PAD_TOP + column.max(ICON) + PAD_BOTTOM;
    // The actions in rows from the column's left, a row more when one
    // does not fit.
    let mut actions = Vec::new();
    let (mut x, mut y) = (COLUMN_X, BORDER + button);
    for (key, label) in &note.actions {
        let label = kit::elide(label, ACTIONS_RIGHT - COLUMN_X - 2.0 * ACTION_PAD, |t| {
            kit::measure(t, ACTION_TEXT, MANROPE_SEMIBOLD)
        });
        let width = (kit::measure(&label, ACTION_TEXT, MANROPE_SEMIBOLD) + 2.0 * ACTION_PAD).ceil();
        if x > COLUMN_X && x + width > ACTIONS_RIGHT {
            x = COLUMN_X;
            y += ACTION + ACTION_GAP;
        }
        actions.push((
            key.clone(),
            label,
            Rectangle::new(Point::new(x, y), Size::new(width, ACTION)),
        ));
        x += width + ACTION_GAP;
    }
    let below = actions.last().map_or(0.0, |(_, _, r)| {
        r.y + r.height + ACTION_PAD - BORDER - button
    });
    let height = (BORDER + button + below + BORDER).ceil();
    Laid {
        size: Size::new(WIDTH, height),
        app,
        when_x,
        urgent_x,
        title,
        body,
        progress,
        actions,
        button,
    }
}

/// A bubble's surface for a card of `size` whose top is `top` under the
/// screen's and whose right is `right` from its right edge: the surface's
/// size, its margins (top, right, bottom, left; anchored to the top
/// right), and the card in it. `card`: on the CPU, the card alone.
pub fn placed(
    size: Size,
    top: f32,
    right: f32,
    card: bool,
) -> ((u32, u32), (i32, i32, i32, i32), Rectangle) {
    let (left, above, after, below) = if card { (0.0, 0.0, 0.0, 0.0) } else { ROOM };
    let surface = (
        (size.width + left + after).ceil() as u32,
        (size.height + above + below).ceil() as u32,
    );
    let margin = (
        (top - above).round() as i32,
        (right - after).round() as i32,
        0,
        0,
    );
    (
        surface,
        margin,
        Rectangle::new(Point::new(left, above), size),
    )
}

/// The chip's size for `n` waiting.
pub fn chip_size(n: usize) -> (u32, u32) {
    let text = kit::measure(&Words::now().waiting(n), CHIP_TEXT, MANROPE_SEMIBOLD);
    (
        (text + 2.0 * (CHIP_PAD.1 + BORDER)).ceil() as u32,
        (CHIP_TEXT * LINE + 2.0 * (CHIP_PAD.0 + BORDER)).ceil() as u32,
    )
}

// --- Drawing a bubble ---------------------------------------------------------------

/// What a bubble's surface draws under its controls ([`super::bubble`]):
/// its shadow, its card lit while the pointer is on it, its icon, its
/// words, its progress and its countdown's line, as they are when it
/// draws.
#[derive(Debug, Clone)]
pub struct Back {
    pub id: window::Id,
    /// The compositor blurs behind the shell (else the opaque colours).
    pub glass: bool,
    pub card: bool,
}

/// What a bubble shows, as the stack has it.
struct Shown {
    note: Note,
    laid: Laid,
    countdown: Countdown,
    paused: bool,
}

impl Back {
    fn shown(&self) -> Option<Shown> {
        super::with(|c| {
            let paused = c.stack.paused();
            c.stack.bubble(self.id).map(|b| Shown {
                note: b.note.clone(),
                laid: b.laid.clone(),
                countdown: b.countdown(),
                paused,
            })
        })
    }
}

/// The card in a surface of `size`: on the CPU the surface; on a GPU,
/// inside the room its shadow takes.
pub fn card_at(size: Size, card: bool) -> Rectangle {
    if card {
        return Rectangle::new(Point::ORIGIN, size);
    }
    Rectangle::new(
        Point::new(ROOM.0, ROOM.1),
        Size::new(size.width - ROOM.0 - ROOM.2, size.height - ROOM.1 - ROOM.3),
    )
}

/// Where the card is on its surface: on the CPU the surface; on a GPU,
/// inside the room its shadow takes.
pub fn card_origin(card: bool) -> Point {
    if card {
        Point::ORIGIN
    } else {
        Point::new(ROOM.0, ROOM.1)
    }
}

/// The ✕ in the card's frame.
pub fn close_rect() -> Rectangle {
    Rectangle::new(
        Point::new(WIDTH - BORDER - CLOSE_IN - CLOSE, BORDER + CLOSE_IN),
        Size::new(CLOSE, CLOSE),
    )
}

impl<Message> canvas::Program<Message> for Back {
    /// When its line last drew.
    type State = Option<Instant>;

    fn update(
        &self,
        state: &mut Option<Instant>,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let shown = self.shown()?;
        *state = Some(*now);
        // The line shortens a pixel at a time, at 30 frames a second at
        // most, while it runs.
        match shown.countdown {
            Countdown::Running { total, .. } if !shown.paused => {
                let pixel = total.div_f32(WIDTH - 2.0 * (BORDER + COUNTDOWN_IN));
                Some(canvas::Action::request_redraw_at(
                    *now + pixel.max(COUNTDOWN_STEP),
                ))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Option<Instant>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let Some(shown) = self.shown() else {
            return vec![f.into_geometry()];
        };
        let size = bounds.size();
        let card = card_at(size, self.card);
        let critical = shown.note.urgency == Urgency::Critical;
        if !self.card {
            f.draw_svg(
                Rectangle::new(Point::ORIGIN, size),
                svg::Svg::new(shadow(size, card, critical)),
            );
        }
        let now = state.unwrap_or_else(Instant::now);
        let lit = super::bubble::lit(self.id);
        f.with_save(|f| {
            f.translate(iced::Vector::new(card.x, card.y));
            draw_card(f, &shown, lit, self.glass && !self.card, now);
        });
        vec![f.into_geometry()]
    }
}

/// The card, in its frame.
fn draw_card(f: &mut Frame, shown: &Shown, lit: bool, glass: bool, now: Instant) {
    let laid = &shown.laid;
    let note = &shown.note;
    let size = laid.size;
    let (top, bottom) = if glass {
        (GLASS_TOP, GLASS_BOTTOM)
    } else {
        (OPAQUE_TOP, OPAQUE_BOTTOM)
    };
    let shape = Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into());
    let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
        .add_stop(0.0, top.get())
        .add_stop(1.0, bottom.get());
    f.fill(&shape, canvas::Fill::from(tint));
    if lit {
        let button = Rectangle::new(
            Point::new(BORDER, BORDER),
            Size::new(WIDTH - 2.0 * BORDER, laid.button),
        );
        crate::theme::hover(f, &kit::rounded(button, RADIUS - BORDER), HOVER.get());
    }
    let critical = note.urgency == Urgency::Critical;
    let rim = Path::rounded_rectangle(
        Point::new(BORDER / 2.0, BORDER / 2.0),
        Size::new(size.width - BORDER, size.height - BORDER),
        (RADIUS - BORDER / 2.0).into(),
    );
    f.stroke(
        &rim,
        Stroke {
            style: canvas::Style::Solid(if critical {
                RIM_CRITICAL.get()
            } else {
                RIM.get()
            }),
            width: BORDER,
            ..Stroke::default()
        },
    );
    // Givre's marks; an urgent bubble has its rim instead.
    if !critical {
        crate::theme::corner_marks(
            f,
            Rectangle::new(Point::ORIGIN, size),
            crate::theme::Corners::Diagonal,
        );
    }
    // The icon, else the first letter of its application on a square.
    let icon = Rectangle::new(
        Point::new(BORDER + PAD_LEFT, BORDER + PAD_TOP),
        Size::new(ICON, ICON),
    );
    match &note.icon {
        Some(Icon::Raster(handle)) => f.draw_image(icon, canvas::Image::new(handle.clone())),
        Some(Icon::Vector(handle)) => f.draw_svg(icon, svg::Svg::new(handle.clone())),
        // The launcher's tile for an application without an icon: a
        // neutral square and the name's first letter.
        None => {
            let radius = (ICON * 0.28).round();
            let fill = canvas::gradient::Linear::new(
                icon.position(),
                Point::new(icon.x + icon.width, icon.y + icon.height),
            )
            .add_stop(0.0, NEUTRAL.0)
            .add_stop(1.0, NEUTRAL.1);
            f.fill(&kit::rounded(icon, radius), canvas::Fill::from(fill));
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(icon.x + 0.5, icon.y + 0.5),
                    Size::new(ICON - 1.0, ICON - 1.0),
                    (radius - 0.5).into(),
                ),
                Stroke {
                    style: canvas::Style::Solid(NEUTRAL_RIM.get()),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            let first = note
                .app
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    first,
                    icon.center(),
                    LETTER,
                    Color::WHITE,
                    Alignment::Center,
                ),
            ));
        }
    }
    // The first line.
    let words = Words::now();
    let mut y = BORDER + PAD_TOP;
    let middle = y + SMALL * LINE / 2.0;
    let text = |content: String, x: f32, y: f32, size: f32, colour: Color, font| {
        kit::text_in(
            font,
            kit::text(content, Point::new(x, y), size, colour, Alignment::Left),
        )
    };
    f.fill_text(text(
        laid.app.clone(),
        COLUMN_X,
        middle,
        SMALL,
        DIM.get(),
        MANROPE_BOLD,
    ));
    f.fill_text(text(
        words.when().to_owned(),
        laid.when_x,
        middle,
        SMALL,
        FAINT.get(),
        MANROPE,
    ));
    if let Some(x) = laid.urgent_x {
        f.fill_text(text(
            words.urgent().to_owned(),
            x,
            middle,
            SMALL,
            URGENT.get(),
            MANROPE_BOLD,
        ));
    }
    y += SMALL * LINE;
    if !laid.title.is_empty() {
        y += COLUMN_GAP;
        for line in &laid.title {
            let h = TITLE * TITLE_LINE;
            f.fill_text(text(
                line.clone(),
                COLUMN_X,
                y + h / 2.0,
                TITLE,
                TEXT.get(),
                MANROPE_BOLD,
            ));
            y += h;
        }
    }
    if !laid.body.is_empty() {
        y += COLUMN_GAP;
        for line in &laid.body {
            let h = BODY * BODY_LINE;
            f.fill_text(text(
                line.clone(),
                COLUMN_X,
                y + h / 2.0,
                BODY,
                BODY_TEXT.get(),
                MANROPE,
            ));
            y += h;
        }
    }
    if let (Some(top), Some(value)) = (laid.progress, note.progress) {
        let track = Rectangle::new(Point::new(COLUMN_X, top), Size::new(COLUMN, PROGRESS));
        f.fill(&kit::rounded(track, PROGRESS / 2.0), TRACK.get());
        let width = COLUMN * f32::from(value) / 100.0;
        if width > 0.0 {
            let fill = Rectangle::new(track.position(), Size::new(width, PROGRESS));
            f.fill(
                &kit::rounded(fill, (PROGRESS / 2.0).min(width / 2.0)),
                PROGRESS_FILL.get(),
            );
        }
    }
    // The countdown.
    if let Some(left) = shown.countdown.left(now) {
        let full = WIDTH - 2.0 * (BORDER + COUNTDOWN_IN);
        let width = full * left;
        if width > 0.0 {
            let line = Rectangle::new(
                Point::new(BORDER + COUNTDOWN_IN, size.height - BORDER - COUNTDOWN),
                Size::new(width, COUNTDOWN),
            );
            let colour = if shown.paused { PAUSED } else { RUNNING };
            f.fill(
                &kit::rounded(line, (COUNTDOWN / 2.0).min(width / 2.0)),
                colour.get(),
            );
        }
    }
}

/// The ✕ in its 24 px, lit under the pointer: two strokes.
pub fn draw_close(f: &mut Frame, lit: bool) {
    let c = Point::new(CLOSE / 2.0, CLOSE / 2.0);
    if lit {
        f.fill(&Path::circle(c, CLOSE / 2.0), CLOSE_HOVER.get());
    }
    let arm = 3.75;
    let cross = Path::new(|p| {
        p.move_to(Point::new(c.x - arm, c.y - arm));
        p.line_to(Point::new(c.x + arm, c.y + arm));
        p.move_to(Point::new(c.x + arm, c.y - arm));
        p.line_to(Point::new(c.x - arm, c.y + arm));
    });
    f.stroke(
        &cross,
        Stroke {
            style: canvas::Style::Solid(if lit { TEXT.get() } else { FAINT.get() }),
            width: 1.4,
            line_cap: canvas::LineCap::Round,
            ..Stroke::default()
        },
    );
}

/// An action's button, `size` large, lit under the pointer.
pub fn draw_action(f: &mut Frame, size: Size, label: &str, lit: bool) {
    let rect = Rectangle::new(Point::ORIGIN, size);
    f.fill(
        &kit::rounded(rect, ACTION_RADIUS),
        if lit { ACTION_FILL_HOVER } else { ACTION_FILL },
    );
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(rect.x + 0.5, rect.y + 0.5),
            Size::new(rect.width - 1.0, rect.height - 1.0),
            (ACTION_RADIUS - 0.5).into(),
        ),
        Stroke {
            style: canvas::Style::Solid(ACTION_RIM.get()),
            width: 1.0,
            ..Stroke::default()
        },
    );
    f.fill_text(kit::text_in(
        MANROPE_SEMIBOLD,
        kit::text(
            label.to_owned(),
            rect.center(),
            ACTION_TEXT,
            TEXT.get(),
            Alignment::Center,
        ),
    ));
}

/// The card's shadow on a surface of `size`, the card at `card`, and a
/// critical one's glow: blurred, outside the card only (CSS draws none
/// under it).
fn shadow(size: Size, card: Rectangle, critical: bool) -> svg::Handle {
    let (w, h) = (size.width, size.height);
    let Rectangle {
        x,
        y,
        width,
        height,
    } = card;
    let filter = |deviation: f32, id: &str| {
        format!(
            "<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" \
             width=\"{w}\" height=\"{h}\"><feGaussianBlur stdDeviation=\"{deviation}\"/>\
             </filter>"
        )
    };
    let rect = |(dy, _, alpha): (f32, f32, f32), colour: &str, id: &str| {
        format!(
            "<rect x=\"{x}\" y=\"{}\" width=\"{width}\" height=\"{height}\" rx=\"{RADIUS}\" \
             fill=\"{colour}\" fill-opacity=\"{alpha}\" filter=\"url(#{id})\"/>",
            y + dy
        )
    };
    let (filters, glow) = if critical {
        (
            filter(SHADOW.1, "shadow") + &filter(GLOW.1, "glow"),
            rect(GLOW, "#F2945B", "glow"),
        )
    } else {
        (filter(SHADOW.1, "shadow"), String::new())
    };
    svg::Handle::from_memory(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" \
             height=\"{h}\"><defs>{filters}<mask id=\"outside\" maskUnits=\"userSpaceOnUse\" \
             x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\"><rect width=\"{w}\" \
             height=\"{h}\" fill=\"#fff\"/><rect x=\"{x}\" y=\"{y}\" width=\"{width}\" \
             height=\"{height}\" rx=\"{RADIUS}\" fill=\"#000\"/></mask></defs>\
             <g mask=\"url(#outside)\">{}{glow}</g></svg>",
            rect(SHADOW, "#000", "shadow")
        )
        .into_bytes(),
    )
}

// --- The chip ------------------------------------------------------------------

/// The "+N waiting" chip's surface.
#[derive(Debug, Clone)]
pub struct Chip {
    pub id: window::Id,
    pub card: bool,
}

impl<Message> canvas::Program<Message> for Chip {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let Some(n) = super::with(|c| c.stack.chip(self.id)) else {
            return vec![f.into_geometry()];
        };
        let size = bounds.size();
        let fill = if self.card {
            over(CHIP_FILL.get(), Color::BLACK)
        } else {
            CHIP_FILL.get()
        };
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, CHIP_RADIUS.into()),
            fill,
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(0.5, 0.5),
                Size::new(size.width - 1.0, size.height - 1.0),
                (CHIP_RADIUS - 0.5).into(),
            ),
            Stroke {
                style: canvas::Style::Solid(CHIP_RIM.get()),
                width: 1.0,
                ..Stroke::default()
            },
        );
        f.fill_text(kit::text_in(
            MANROPE_SEMIBOLD,
            kit::text(
                Words::now().waiting(n),
                Point::new(size.width / 2.0, size.height / 2.0),
                CHIP_TEXT,
                DIM.get(),
                Alignment::Center,
            ),
        ));
        vec![f.into_geometry()]
    }
}

/// `colour` over `under`, opaque.
fn over(colour: Color, under: Color) -> Color {
    Color::from_rgb(
        colour.r * colour.a + under.r * (1.0 - colour.a),
        colour.g * colour.a + under.g * (1.0 - colour.a),
        colour.b * colour.a + under.b * (1.0 - colour.a),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::note::{LIFE, Timeout};

    fn note() -> Note {
        Note {
            id: 1,
            app: "Slack".to_owned(),
            entry: None,
            icon: None,
            summary: "Camille dans #design".to_owned(),
            body: String::new(),
            actions: Vec::new(),
            default: false,
            urgency: Urgency::Normal,
            timeout: Timeout::After(LIFE),
            progress: None,
            resident: false,
            pid: 0,
            agent: false,
            conversation: None,
            icon_from: crate::notifications::note::IconFrom::default(),
            at: 0,
        }
    }

    #[test]
    fn a_bubble_is_340_wide_its_text_beside_its_icon() {
        let laid = lay(&note());
        assert_eq!(laid.size.width, 340.0);
        // The icon's height, its padding and the rims: 56 px at least.
        assert!(laid.size.height >= 56.0, "{laid:?}");
        assert_eq!(COLUMN_X, 56.0);
        assert_eq!(COLUMN, 249.0);
        // Actions under the text, from its left, 28 px high, 12 px from
        // the bottom.
        let mut with = note();
        with.actions = vec![
            ("reply".to_owned(), "Répondre".to_owned()),
            ("read".to_owned(), "Marquer comme lu".to_owned()),
        ];
        let acted = lay(&with);
        let (_, _, first) = &acted.actions[0];
        assert_eq!(first.x, COLUMN_X);
        assert_eq!(first.height, 28.0);
        assert!((acted.size.height - (first.y + 28.0 + 12.0 + 1.0)).abs() < 1.0);
        assert!(acted.size.height > laid.size.height + 30.0);
        // A long body on two lines at most.
        let mut long = note();
        long.body = "mot ".repeat(200);
        assert_eq!(lay(&long).body.len(), 2);
        assert!(lay(&long).body[1].ends_with('…'));
    }

    #[test]
    fn its_surface_holds_its_shadow_on_a_gpu_and_is_the_card_on_the_cpu() {
        let card = Size::new(340.0, 90.0);
        let (size, margin, at) = placed(card, 8.0, 212.0, false);
        assert_eq!(size, (340 + 60 + 8, 90 + 8 + 74));
        assert_eq!(margin, (0, 204, 0, 0));
        assert_eq!(at, Rectangle::new(Point::new(60.0, 8.0), card));
        let (size, margin, at) = placed(card, 106.0, 212.0, true);
        assert_eq!((size, margin), ((340, 90), (106, 212, 0, 0)));
        assert_eq!(at.position(), Point::ORIGIN);
    }
}
