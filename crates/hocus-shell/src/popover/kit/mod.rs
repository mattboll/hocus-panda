//! The popovers' pieces (#465), drawn and handled the same way in each:
//! the frame and its glass, the header (its title, a word on its right or
//! a switch), the sections' dividers and spaced labels, notes and wrapped
//! lines, values in tabular figures, sliders, rows and lists that scroll,
//! the blocks of what is connected, spinners; the pointer, the keys and
//! the order `Tab` goes through.
//!
//! Plain functions and small types, at the mockups' sizes at ×1: each
//! popover lays itself out and keeps its own state, and takes from here
//! what it shares with the others.

mod app;
mod input;
mod list;
mod slider;
mod text;

use std::time::{Duration, Instant};

use hocus_link::{POPOVER_WIDTH, RADIUS};
use iced::{
    Color, Pixels, Point, Rectangle, Size, alignment,
    widget::{
        canvas::{self, Frame, Path, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
};

pub(crate) use app::app_tile;
pub(crate) use input::{Input, Pointer, awaited, cycle, input, keys, step};
pub(crate) use list::{
    Aside, Clip, List, SCROLLBAR, Spin, aside, list, name_room, reveal, row_fill, searching, stack,
};
pub(crate) use slider::{Track, Wheel, forget_scale, popover_scale, scale, set_scale, slider};
pub(crate) use text::{Widths, elide, measure, measure_in, wrap, wrapped, zero};

use crate::levels::{
    draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, VALUE, cell, rgb},
    model::Words,
};

// --- The frame, at ×1 -------------------------------------------------------------

/// The rim, and the padding inside it.
pub(crate) const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
pub(crate) const PAD_BOTTOM: f32 = 14.0;
/// The content's sides.
pub(crate) const LEFT: f32 = BORDER + PAD_SIDE;
pub(crate) const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
/// The header: its top, its height, its middle; the title's size, and
/// the word on its right's.
pub(crate) const HEADER_TOP: f32 = BORDER + PAD_TOP;
pub(crate) const HEADER: f32 = 32.0;
pub(crate) const HEADER_MIDDLE: f32 = HEADER_TOP + HEADER / 2.0;
pub(crate) const TITLE: f32 = 16.0;
const ASIDE: f32 = 13.0;
/// Icons in rows, and the gap after them.
pub(crate) const ICON: f32 = 18.0;
pub(crate) const ICON_GAP: f32 = 10.0;
/// The sections' labels: 11.5 px capitals, CSS's `letter-spacing: 0.08em`.
pub(crate) const LABEL: f32 = 11.5;
const LABEL_SPACING: f32 = 0.08 * LABEL;
pub(crate) const LABEL_PAD: f32 = 2.0;
/// What is said in place of a list, or under one: 13.5 px, 4 px above, 2
/// under.
pub(crate) const NOTE: f32 = 13.5;
pub(crate) const NOTE_TOP: f32 = 4.0;
pub(crate) const NOTE_HEIGHT: f32 = NOTE_TOP + NOTE * LINE + 2.0;
/// A list's rows: 36 px, 2 px apart, reaching 6 px past the content's
/// sides, their icon 6 px in, their name 14 px; what they say on their
/// right 12 px.
pub(crate) const ROW: f32 = 36.0;
pub(crate) const ROW_GAP: f32 = 2.0;
pub(crate) const ROW_OUT: f32 = 6.0;
pub(crate) const ROW_PAD: f32 = 6.0;
pub(crate) const ROW_TEXT: f32 = 14.0;
pub(crate) const ROW_RADIUS: f32 = 8.0;
pub(crate) const SMALL: f32 = 12.0;
/// A connected block: 48 px at least, its name 14 px over its state
/// 12.5 px; its button 28 px, 9 px padded.
const BLOCK: f32 = 48.0;
pub(crate) const NAME: f32 = 14.0;
pub(crate) const STATUS: f32 = 12.5;
const SMALL_BUTTON: f32 = 28.0;
pub(crate) const SMALL_PAD: f32 = 9.0;
pub(crate) const SMALL_RADIUS: f32 = 7.0;
/// The header's switch, its knob.
pub(crate) const SWITCH: Size = Size::new(40.0, 22.0);
const KNOB: f32 = 18.0;

/// The tint over the compositor's blur, a little denser than the bar's.
const GLASS_TOP: Color = Color::from_rgba8(22, 27, 42, 0.72);
const GLASS_BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.82);
const OPAQUE_TOP: Color = Color::from_rgba8(0x16, 0x1B, 0x2A, 1.0);
const OPAQUE_BOTTOM: Color = Color::from_rgba8(0x0E, 0x11, 0x1C, 1.0);
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
/// Buttons, and under the pointer.
pub(crate) const FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
pub(crate) const FILL_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);
const BLOCK_FILL: Color = Color::from_rgba8(108, 194, 135, 0.10);
const SWITCH_OFF: Color = Color::from_rgba8(255, 255, 255, 0.18);
const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.4);
/// `gauge.critical`'s light end: failures, a low battery, a sign-in page.
pub(crate) const PEACH: Color = rgb(0xF7B98A);

/// Manrope at 600, the mockups' for connected names and some buttons.
pub(crate) const MANROPE_SEMIBOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..MANROPE
};

/// The words of the shell's language, for what a height measures.
pub(crate) fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// A popover's height, logical pixels, its content ending at `end`.
pub(crate) fn height(end: f32) -> u32 {
    (end + PAD_BOTTOM + BORDER).round() as u32
}

/// A popover's glass (or its opaque colour) and its rim.
pub(crate) fn background(f: &mut Frame, size: Size, glass: bool) {
    let (top, bottom) = if glass {
        (GLASS_TOP, GLASS_BOTTOM)
    } else {
        (OPAQUE_TOP, OPAQUE_BOTTOM)
    };
    let radius = RADIUS as f32;
    let shape = Path::rounded_rectangle(Point::ORIGIN, size, radius.into());
    let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
        .add_stop(0.0, top)
        .add_stop(1.0, bottom);
    f.fill(&shape, canvas::Fill::from(tint));
    let rim = Path::rounded_rectangle(
        Point::new(BORDER / 2.0, BORDER / 2.0),
        Size::new(size.width - BORDER, size.height - BORDER),
        (radius - BORDER / 2.0).into(),
    );
    f.stroke(
        &rim,
        canvas::Stroke {
            style: canvas::Style::Solid(RIM),
            width: BORDER,
            ..canvas::Stroke::default()
        },
    );
}

/// `content` in Manrope at `size`, its line centred on `position`.
pub(crate) fn text(
    content: String,
    position: Point,
    size: f32,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font: MANROPE,
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// `content` in `font`.
pub(crate) fn text_in(font: iced::Font, mut text: canvas::Text) -> canvas::Text {
    text.font = font;
    text
}

/// The header's title, on its left.
pub(crate) fn title(f: &mut Frame, title: &str) {
    f.fill_text(text_in(
        MANROPE_BOLD,
        text(
            title.to_owned(),
            Point::new(LEFT, HEADER_MIDDLE),
            TITLE,
            TEXT,
            Alignment::Left,
        ),
    ));
}

/// The word on the header's right: what the popover is about.
pub(crate) fn title_aside(f: &mut Frame, word: String) {
    f.fill_text(text(
        word,
        Point::new(RIGHT, HEADER_MIDDLE),
        ASIDE,
        FAINT,
        Alignment::Right,
    ));
}

/// A section's divider: 1 px across the content at `y`.
pub(crate) fn rule(f: &mut Frame, y: f32) {
    f.fill_rectangle(Point::new(LEFT, y), Size::new(RIGHT - LEFT, 1.0), RULE);
}

/// `word`'s letters and their places from the content's left, spaced as a
/// label's, `width` measuring each in the label's font.
pub(crate) fn spaced(word: &str, mut width: impl FnMut(&str) -> f32) -> Vec<(char, f32)> {
    let mut x = LEFT;
    word.chars()
        .map(|ch| {
            let at = x;
            x += width(&ch.to_string()) + LABEL_SPACING;
            (ch, at)
        })
        .collect()
}

/// A label's letters, spaced, measured in its font.
pub(crate) fn label_letters(word: &str) -> Vec<(char, f32)> {
    spaced(word, |ch| measure(ch, LABEL, MANROPE_BOLD))
}

/// A label's `letters`, their line centred on `middle`.
pub(crate) fn label(f: &mut Frame, letters: &[(char, f32)], middle: f32) {
    for (ch, x) in letters {
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                ch.to_string(),
                Point::new(*x, middle),
                LABEL,
                DIM,
                Alignment::Left,
            ),
        ));
    }
}

/// The middle of a label's line from `top`, padded.
pub(crate) fn label_middle(top: f32) -> f32 {
    top + LABEL_PAD + LABEL * LINE / 2.0
}

/// A note from `top`, in `text.faint`.
pub(crate) fn note(said: String, top: f32) -> canvas::Text {
    text(
        said,
        Point::new(LEFT, top + NOTE_TOP + NOTE * LINE / 2.0),
        NOTE,
        FAINT,
        Alignment::Left,
    )
}

/// `lines` from `top` at `x`, `size` px on lines `height` em apart, in
/// `colour`.
pub(crate) fn lines(
    lines: &[String],
    (x, top): (f32, f32),
    size: f32,
    height: f32,
    colour: Color,
) -> impl Iterator<Item = canvas::Text> {
    lines.iter().enumerate().map(move |(i, line)| {
        let mut said = text(
            line.clone(),
            Point::new(x, top + (i as f32 + 0.5) * size * height),
            size,
            colour,
            Alignment::Left,
        );
        said.line_height = LineHeight::Relative(height);
        said
    })
}

/// `value` rounded, right-aligned on `at`, in tabular figures: each digit
/// centred in a cell as wide as the font's `0`, `zero`.
pub(crate) fn figures(f: &mut Frame, at: Point, value: f32, zero: f32, colour: Color) {
    let digits = (value.round() as u32).to_string();
    let mut x = at.x - zero * digits.len() as f32;
    for ch in digits.chars() {
        cell(f, ch, x + zero / 2.0, at.y, VALUE, MANROPE, colour);
        x += zero;
    }
}

/// The `0`'s advance until it is measured.
pub(crate) const ZERO: f32 = VALUE * 0.6;

/// A rounded rectangle at `rect`.
pub(crate) fn rounded(rect: Rectangle, radius: f32) -> Path {
    Path::rounded_rectangle(rect.position(), rect.size(), radius.into())
}

// --- What is connected: a block, its name and state, its button --------------------

/// A connected block's height, its state on `lines` lines: 48 px at least.
pub(crate) fn block_height(lines: f32) -> f32 {
    BLOCK.max(NAME * LINE + 1.0 + lines * STATUS * LINE)
}

/// The room a connected block's name and state have, between its icon and
/// its button, `button` wide.
pub(crate) fn block_room(button: f32) -> f32 {
    RIGHT - button - ICON_GAP - (LEFT + ICON + ICON_GAP)
}

/// A connected block from `top`, `height` tall, lit green: where it is.
pub(crate) fn block(f: &mut Frame, top: f32, height: f32) -> Rectangle {
    let block = Rectangle::new(
        Point::new(LEFT - ROW_OUT, top),
        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, height),
    );
    f.fill(&rounded(block, ROW_RADIUS), BLOCK_FILL);
    block
}

/// A block's name (cut to fit already), in 600, and the middle of its
/// state's first line: both centred on `middle`, its state `lines` long.
pub(crate) fn block_name(f: &mut Frame, name: String, middle: f32, lines: f32) -> f32 {
    let column = NAME * LINE + 1.0 + lines * STATUS * LINE;
    let name_middle = middle - column / 2.0 + NAME * LINE / 2.0;
    f.fill_text(text_in(
        MANROPE_SEMIBOLD,
        text(
            name,
            Point::new(LEFT + ICON + ICON_GAP, name_middle),
            NAME,
            TEXT,
            Alignment::Left,
        ),
    ));
    name_middle + NAME * LINE / 2.0 + 1.0 + STATUS * LINE / 2.0
}

/// A block's button ("Déconnecter"), `width` wide, on its right: where it
/// is, the block from `top` and `height` tall.
pub(crate) fn block_button(top: f32, height: f32, width: f32) -> Rectangle {
    Rectangle::new(
        Point::new(RIGHT - width, top + (height - SMALL_BUTTON) / 2.0),
        Size::new(width, SMALL_BUTTON),
    )
}

/// A small button at `rect` saying `word`, lit under the pointer.
pub(crate) fn small_button(f: &mut Frame, rect: Rectangle, word: &str, hover: bool) {
    f.fill(
        &rounded(rect, SMALL_RADIUS),
        if hover { FILL_HOVER } else { FILL },
    );
    f.fill_text(text(
        word.to_owned(),
        rect.center(),
        STATUS,
        if hover { TEXT } else { DIM },
        Alignment::Center,
    ));
}

// --- The header's switch -----------------------------------------------------------

/// The switch's knob slides over this, `ease-out`.
const SLIDE: Duration = Duration::from_millis(160);
/// The switch shows what was asked this long at most, until it is said.
const SWITCH_ASKED_FOR: Duration = Duration::from_secs(3);

/// The header's switch: what it asked and when, and its knob's slide.
#[derive(Debug, Default)]
pub(crate) struct Switch {
    asked: Option<(bool, Instant)>,
    /// From where (0 off, 1 on), since when.
    knob: Option<(f32, Instant)>,
}

impl Switch {
    /// Where it is.
    pub(crate) fn rect() -> Rectangle {
        Rectangle::new(
            Point::new(RIGHT - SWITCH.width, HEADER_MIDDLE - SWITCH.height / 2.0),
            SWITCH,
        )
    }

    /// Whether it shows on at `now`: as asked, until what is `said` says
    /// it, or a while.
    pub(crate) fn shown(&self, said: bool, now: Instant) -> bool {
        match self.asked {
            Some((asked, since)) if now < since + SWITCH_ASKED_FOR => asked,
            _ => said,
        }
    }

    /// The knob's place, 0 (off) to 1 (on), at `now`.
    pub(crate) fn knob_at(&self, on: bool, now: Instant, motion: bool) -> f32 {
        let to = if on { 1.0 } else { 0.0 };
        match self.knob {
            Some((from, since)) if motion && now < since + SLIDE => {
                let t = now.duration_since(since).as_secs_f32() / SLIDE.as_secs_f32();
                from + (to - from) * crate::clock::motion::bezier((0.0, 0.0, 0.58, 1.0), t)
            }
            _ => to,
        }
    }

    /// Flipped at `now` from what it shows: its knob slides from where it
    /// is. What it asks for.
    pub(crate) fn flip(&mut self, said: bool, now: Instant, motion: bool) -> bool {
        let on = self.shown(said, now);
        let from = self.knob_at(on, now, motion);
        self.asked = Some((!on, now));
        self.knob = Some((from, now));
        !on
    }

    /// What it asked is `said`, or was waited for long enough.
    pub(crate) fn settle(&mut self, said: bool, now: Instant) {
        let done = self.asked.is_some_and(|(asked, _)| asked == said);
        awaited(&mut self.asked, done, now, SWITCH_ASKED_FOR);
    }

    /// Whether its knob slides at `now`.
    pub(crate) fn sliding(&self, now: Instant) -> bool {
        self.knob.is_some_and(|(_, since)| now < since + SLIDE)
    }

    /// It, its knob at `p` of its way (0 off, 1 on).
    pub(crate) fn draw(f: &mut Frame, p: f32) {
        let rect = Self::rect();
        f.fill(
            &rounded(rect, SWITCH.height / 2.0),
            mix(SWITCH_OFF, ACCENT, p),
        );
        let centre = Point::new(
            rect.x + 2.0 + (SWITCH.width - KNOB - 4.0) * p + KNOB / 2.0,
            rect.center_y(),
        );
        // `box-shadow: 0 1px 3px`: rings fading out, 1 px down.
        for (grow, share) in [(2.0_f32, 0.15_f32), (1.2, 0.25), (0.5, 0.35)] {
            f.fill(
                &Path::circle(Point::new(centre.x, centre.y + 1.0), KNOB / 2.0 + grow),
                KNOB_SHADOW.scale_alpha(share),
            );
        }
        f.fill(&Path::circle(centre, KNOB / 2.0), TEXT);
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
