//! The bell (5.4): the button that opens the history, its badge counting
//! what the user has not seen, a slash in amber while they are not to be
//! disturbed. Since 5.5 it is one of the footer's buttons
//! ([`crate::footer`]), the second of four in the full form, over the
//! user's avatar in the mini form; [`draw`] draws it there.

use std::sync::OnceLock;

use iced::{
    Color, Point, Rectangle, Size,
    advanced::svg,
    widget::{canvas::Frame, text::Alignment},
};

use crate::{
    levels::draw::{MANROPE_BOLD, faded, rgb, ring},
    popover::kit,
};

/// The footer's buttons' corners.
pub const RADIUS: f32 = 9.0;
/// The bell in a 24 px box, drawn at 20 px.
const GLYPH_BOX: f32 = 24.0;
const GLYPH: f32 = 20.0;
/// The badge: 15 px high, 3 px in, its right 6 px past the bell's box and
/// its top 1 px under the button's, a 2 px ring in the bar's colour (the
/// 5.5 mockup's).
const BADGE: f32 = 15.0;
const BADGE_PAD: f32 = 3.0;
const BADGE_TEXT: f32 = 9.5;
const BADGE_RING: f32 = 2.0;
const BADGE_FILL: Color = rgb(0x8FB3FF);
const BADGE_INK: Color = rgb(0x0B1020);
const BADGE_RIM: Color = Color::from_rgba8(14, 17, 28, 0.95);
/// A footer's button lit under the pointer, and while its popover shows.
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const OPEN: Color = Color::from_rgba8(255, 255, 255, 0.10);

/// The footer's glyphs' stroke, and the bell's in amber while the user is
/// not to be disturbed.
pub const INK: &str = "#C9D0DC";
pub const AMBER: &str = "#F5C46E";

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

/// A footer's button at `button`: lit while its popover shows (`open`),
/// or under the pointer (`hover`), ringed while it holds the keyboard.
pub fn lit(f: &mut Frame, button: Rectangle, open: bool, hover: bool, focused: bool) {
    let lit = if open {
        Some(OPEN)
    } else if hover {
        Some(HOVER)
    } else {
        None
    };
    if let Some(lit) = lit {
        f.fill(&kit::rounded(button, RADIUS), lit);
    }
    if focused {
        ring(f, button, RADIUS);
    }
}

/// A 20 px glyph centred on `button`, `alpha` opaque.
pub fn glyph_in(f: &mut Frame, button: Rectangle, handle: svg::Handle, alpha: f32) {
    let centre = button.center();
    let rect = Rectangle::new(
        Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
        Size::new(GLYPH, GLYPH),
    );
    f.draw_svg(rect, svg::Svg::new(handle).opacity(alpha));
}

/// The bell's button at `button` in frame `f`, as it `shows`, `alpha`
/// opaque (two forms cross-fade while the bar switches).
pub fn draw(f: &mut Frame, button: Rectangle, shows: Shows, alpha: f32) {
    lit(f, button, shows.open, shows.hover, shows.focused);
    glyph_in(f, button, glyph(shows.dnd), alpha);
    if shows.count == 0 {
        return;
    }
    let count = shows.count.to_string();
    let width = (kit::measure(&count, BADGE_TEXT, MANROPE_BOLD) + 2.0 * BADGE_PAD).max(BADGE);
    let right = button.center_x() + GLYPH_BOX / 2.0 + 6.0;
    let top = button.y + 1.0;
    let badge = Rectangle::new(Point::new(right - width, top), Size::new(width, BADGE));
    let rim = Rectangle::new(
        Point::new(badge.x - BADGE_RING, badge.y - BADGE_RING),
        Size::new(
            badge.width + 2.0 * BADGE_RING,
            badge.height + 2.0 * BADGE_RING,
        ),
    );
    f.fill(
        &kit::rounded(rim, BADGE / 2.0 + BADGE_RING),
        faded(BADGE_RIM, alpha),
    );
    f.fill(&kit::rounded(badge, BADGE / 2.0), faded(BADGE_FILL, alpha));
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            count,
            badge.center(),
            BADGE_TEXT,
            faded(BADGE_INK, alpha),
            Alignment::Center,
        ),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_badge_sits_at_the_buttons_top_right_inside_the_footers_room() {
        // A 40 × 34 button: the badge's right 2 px inside the button's, as
        // the mockup has it, its ring 1 px over the button's top.
        let button = Rectangle::new(Point::new(6.0, 6.0), Size::new(40.0, 34.0));
        let right = button.center_x() + GLYPH_BOX / 2.0 + 6.0;
        assert_eq!(button.x + button.width - right, 2.0);
        assert_eq!(button.y + 1.0 - BADGE_RING, button.y - 1.0);
    }
}
