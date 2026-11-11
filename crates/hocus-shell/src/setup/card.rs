//! The first-run card (Day.1, #425): in the full form, where the day's
//! groups (Today, To do) would be, while no source is set and "Plus tard"
//! was not said. Drawn on the to-do list's surface (`hocus-bar-tasks`),
//! which gives it its place, its pointer and its keyboard.
//!
//! As wide as the bar's inside (172 px), radius 10,
//! `rgba(255,255,255,0.05)` with a 1 px `rgba(255,255,255,0.08)` rim,
//! padding 10 px, 8 px gaps: an 18 px calendar in `#8FB3FF` and "Ta
//! journée ici" (13.5 px, 700); what it is for (12 px, `text.dim`, 1.45
//! lines); "Choisir…" (30 px, radius 8, `#6CC287`, `#0b1020` 12.5 px 700),
//! which opens the sources' popover; "Plus tard" (24 px, transparent,
//! `text.faint`), which removes the card for good.

use iced::{
    Color, Point, Rectangle, Size,
    widget::{
        canvas::{Frame, Path, Stroke},
        text::Alignment,
    },
};

use super::Words;
use crate::theme::Ink;
use crate::{
    levels::draw::{DIM, FAINT, HOVER, MANROPE_BOLD, faded, rgb},
    popover::kit,
};

/// The card's width: the bar's inside.
pub const WIDTH: f32 = 172.0;
const PAD: f32 = 10.0;
const GAP: f32 = 8.0;
const RADIUS: f32 = 10.0;
const ICON: f32 = 18.0;
const TITLE: f32 = 13.5;
const TEXT: f32 = 12.0;
const TEXT_LINE: f32 = 12.0 * 1.45;
const CHOOSE: f32 = 30.0;
pub const CHOOSE_RADIUS: f32 = 8.0;
const CHOOSE_TEXT: f32 = 12.5;
const LATER: f32 = 24.0;
pub const LATER_RADIUS: f32 = 7.0;
const LATER_TEXT: f32 = 12.5;

const FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
pub const BLUE: Ink = Ink::Dark(rgb(0x8FB3FF));
pub const GREEN: Ink = Ink::Accent(1.0);
const GREEN_HOVER: Ink = Ink::AccentLit;
pub const INK: Ink = Ink::Dark(rgb(0x0B1020));

/// The card laid out, from its top left.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub height: f32,
    /// What it says, broken into lines.
    pub lines: Vec<String>,
    pub choose: Rectangle,
    pub later: Rectangle,
}

/// The card, its text broken with `width` (text, size → px).
pub fn lay(words: Words, mut width: impl FnMut(&str, f32) -> f32) -> Card {
    let room = WIDTH - 2.0 * PAD;
    let lines = kit::wrap(words.card_text(), room, |t| width(t, TEXT));
    let mut y = PAD + ICON + GAP;
    y += lines.len() as f32 * TEXT_LINE + GAP;
    let choose = Rectangle::new(Point::new(PAD, y), Size::new(room, CHOOSE));
    y += CHOOSE + GAP;
    let later = Rectangle::new(Point::new(PAD, y), Size::new(room, LATER));
    y += LATER + PAD;
    Card {
        height: y,
        lines,
        choose,
        later,
    }
}

/// What the pointer and the keyboard are on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lit {
    pub choose: bool,
    pub later: bool,
}

/// The card at `o` (its top left), `alpha` faded.
pub fn draw(f: &mut Frame, card: &Card, words: Words, o: Point, lit: Lit, alpha: f32) {
    let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
    let whole = Rectangle::new(o, Size::new(WIDTH, card.height));
    f.fill(&kit::rounded(whole, RADIUS), faded(FILL.get(), alpha));
    let rim = Rectangle::new(
        Point::new(o.x + 0.5, o.y + 0.5),
        Size::new(WIDTH - 1.0, card.height - 1.0),
    );
    f.stroke(
        &kit::rounded(rim, RADIUS - 0.5),
        Stroke::default()
            .with_color(faded(RIM.get(), alpha))
            .with_width(1.0),
    );
    calendar(
        f,
        Point::new(o.x + PAD, o.y + PAD),
        faded(BLUE.get(), alpha),
    );
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            words.card_title().to_owned(),
            Point::new(o.x + PAD + ICON + GAP, o.y + PAD + ICON / 2.0),
            TITLE,
            faded(crate::levels::draw::TEXT.get(), alpha),
            Alignment::Left,
        ),
    ));
    let mut y = o.y + PAD + ICON + GAP;
    for line in &card.lines {
        let mut text = kit::text(
            line.clone(),
            Point::new(o.x + PAD, y + TEXT_LINE / 2.0),
            TEXT,
            faded(DIM.get(), alpha),
            Alignment::Left,
        );
        text.line_height = iced::widget::text::LineHeight::Relative(1.45);
        f.fill_text(text);
        y += TEXT_LINE;
    }
    let choose = at(card.choose);
    let fill = if lit.choose { GREEN_HOVER } else { GREEN };
    f.fill(
        &kit::rounded(choose, CHOOSE_RADIUS),
        faded(fill.get(), alpha),
    );
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            words.choose().to_owned(),
            choose.center(),
            CHOOSE_TEXT,
            faded(INK.get(), alpha),
            Alignment::Center,
        ),
    ));
    let later = at(card.later);
    if lit.later {
        f.fill(
            &kit::rounded(later, LATER_RADIUS),
            faded(HOVER.get(), alpha),
        );
    }
    f.fill_text(kit::text(
        words.later().to_owned(),
        later.center(),
        LATER_TEXT,
        faded(FAINT.get(), alpha),
        Alignment::Center,
    ));
}

/// An 18 px calendar at `at` (its top left): a rounded frame, its two
/// rings and the line under its header, in `colour`.
pub fn calendar(f: &mut Frame, at: Point, colour: Color) {
    let unit = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * unit, at.y + y * unit);
    let stroke = Stroke {
        width: 2.0 * unit,
        line_cap: iced::widget::canvas::LineCap::Round,
        line_join: iced::widget::canvas::LineJoin::Round,
        ..Stroke::default().with_color(colour)
    };
    let frame = Rectangle::new(p(3.0, 5.0), Size::new(18.0 * unit, 16.0 * unit));
    f.stroke(&kit::rounded(frame, 2.5 * unit), stroke);
    let lines = Path::new(|b| {
        b.move_to(p(3.0, 10.0));
        b.line_to(p(21.0, 10.0));
        b.move_to(p(8.0, 3.0));
        b.line_to(p(8.0, 7.0));
        b.move_to(p(16.0, 3.0));
        b.line_to(p(16.0, 7.0));
    });
    f.stroke(&lines, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_is_padded_10_px_its_parts_8_px_apart() {
        let words = Words::new(crate::clock::time::Lang::French);
        // Six characters a line at 12 px of 6 px each.
        let card = lay(words, |t, size| t.chars().count() as f32 * size * 0.5);
        let lines = card.lines.len() as f32;
        assert!(lines >= 4.0, "{:?}", card.lines);
        assert_eq!(card.choose.y, 10.0 + 18.0 + 8.0 + lines * TEXT_LINE + 8.0);
        assert_eq!(card.choose.size(), Size::new(152.0, 30.0));
        assert_eq!(card.later.y, card.choose.y + 30.0 + 8.0);
        assert_eq!(card.later.height, 24.0);
        assert_eq!(card.height, card.later.y + 24.0 + 10.0);
    }
}
