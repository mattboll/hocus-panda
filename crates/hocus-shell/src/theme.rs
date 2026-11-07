//! The theme the shell draws in (bar.md §13, #402): the compositor says it
//! as the shell connects and whenever `[appearance] theme` or `accent`
//! changes in the file ([`hocus_link::ToShell::Theme`]); every surface is
//! then drawn again, in it.
//!
//! The shell's colours are [`Ink`]s, constants that give their colour in
//! the theme of the moment: a token of [`hocus_link::theme::Tokens`], the
//! accent, or a colour drawn for the dark theme that the light one reads
//! by [`hocus_link::theme::on_light`]. Colours that are the same in both
//! (the agents' states, the gauges, the pandas) stay plain `Color`s.
//!
//! Givre (#740) adds marks of its own, drawn only in it ([`frost`]): the
//! viewfinder corners of the bar, its popovers and the bubbles
//! ([`corner_marks`]), the clocks' glow ([`glow`]) and the pandas' visor.

use std::sync::{PoisonError, RwLock};

use hocus_link::theme::{Accent, Choice, Rgba, Scheme, Tokens};
use iced::Color;

static NOW: RwLock<Choice> = RwLock::new(Choice {
    scheme: Scheme::Dark,
    accent: Accent::Green,
    wallpaper: hocus_link::theme::BACKDROP_ACCENT,
    contrast: false,
});

/// The theme from now on; whether it changed.
pub fn set(choice: Choice) -> bool {
    let mut now = NOW.write().unwrap_or_else(PoisonError::into_inner);
    let changed = *now != choice;
    *now = choice;
    changed
}

/// The theme now.
pub fn now() -> Choice {
    *NOW.read().unwrap_or_else(PoisonError::into_inner)
}

/// Whether the shell draws on light backgrounds.
pub fn light() -> bool {
    now().scheme == Scheme::Light
}

/// Whether the shell draws Givre's own marks: Givre, without high
/// contrast (which wins over the look).
pub fn frost() -> bool {
    let choice = now();
    choice.scheme.themed() && !choice.contrast
}

/// Which of a surface's corners take Givre's marks: the bar all four, a
/// popover or a bubble the top left and bottom right (the mockup's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corners {
    Four,
    Diagonal,
}

/// Givre's viewfinder corner marks in `rect`: 10 px arms, 1.5 px thick,
/// 5 px in, `#BFEFFF` at 75 %. Nothing in another theme.
pub fn corner_marks(f: &mut iced::widget::canvas::Frame, rect: iced::Rectangle, which: Corners) {
    use iced::{
        Point,
        widget::canvas::{Path, Stroke},
    };
    if !frost() {
        return;
    }
    const IN: f32 = 5.0;
    const ARM: f32 = 10.0;
    const WIDTH: f32 = 1.5;
    let near = IN + WIDTH / 2.0;
    let (left, top) = (rect.x + near, rect.y + near);
    let (right, bottom) = (rect.x + rect.width - near, rect.y + rect.height - near);
    let reach = ARM - WIDTH / 2.0;
    // Each corner: its point, and which way its arms go.
    let mut marks = vec![(left, top, 1.0, 1.0), (right, bottom, -1.0, -1.0)];
    if which == Corners::Four {
        marks.push((right, top, -1.0, 1.0));
        marks.push((left, bottom, 1.0, -1.0));
    }
    let colour = color(hocus_link::theme::FROST_CORNER);
    for (x, y, dx, dy) in marks {
        let path = Path::new(|p| {
            p.move_to(Point::new(x, y + dy * reach));
            p.line_to(Point::new(x, y));
            p.line_to(Point::new(x + dx * reach, y));
        });
        f.stroke(
            &path,
            Stroke::default().with_color(colour).with_width(WIDTH),
        );
    }
}

/// The bar's column's four corner marks in Givre, over its glass: a
/// canvas that draws nothing in another theme.
#[derive(Debug, Clone, Copy)]
pub struct BarMarks;

impl<Message> iced::widget::canvas::Program<Message> for BarMarks {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        if !frost() {
            return Vec::new();
        }
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        corner_marks(
            &mut frame,
            iced::Rectangle::new(iced::Point::ORIGIN, bounds.size()),
            Corners::Four,
        );
        vec![frame.into_geometry()]
    }
}

/// Givre's glow behind a clock's digits (`0 0 14px #7FD8FF` at 35 %): the
/// digit drawn again around itself, faint, by `draw` (an offset and the
/// glow's colour for that copy), before the digit itself. Nothing in
/// another theme.
pub fn glow(mut draw: impl FnMut(iced::Vector, Color)) {
    if !frost() {
        return;
    }
    let glow = hocus_link::theme::FROST_GLOW;
    for (radius, alpha) in [(2.5_f32, 0.07_f32), (5.5, 0.035)] {
        for k in 0..8 {
            let angle = k as f32 * std::f32::consts::FRAC_PI_4;
            draw(
                iced::Vector::new(radius * angle.cos(), radius * angle.sin()),
                Color::from_rgba8(glow.r, glow.g, glow.b, alpha),
            );
        }
    }
}

/// Whether high contrast reinforces the theme (#423): borders 2 px, the
/// keyboard's ring 3 px with the background between it and its item.
pub fn contrast() -> bool {
    now().contrast
}

/// The bar's, the popovers' and the bubbles' border: 1 px, 2 px under high
/// contrast (bar.md §13). Drawn inside their edge: their room does not
/// change.
pub fn border() -> f32 {
    if contrast() { 2.0 } else { 1.0 }
}

/// A hover over `path`: its fill; under high contrast, which has no fills,
/// a 1 px border in `text.dim` instead (bar.md §13).
pub fn hover(
    f: &mut iced::widget::canvas::Frame,
    path: &iced::widget::canvas::Path,
    fill: impl Into<iced::widget::canvas::Fill>,
) {
    if contrast() {
        let dim = color(now().scheme.contrast_tokens().dim);
        f.stroke(
            path,
            iced::widget::canvas::Stroke::default()
                .with_color(dim)
                .with_width(1.0),
        );
    } else {
        f.fill(path, fill);
    }
}

pub fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn rgba(c: Color) -> Rgba {
    let [r, g, b, a] = c.into_rgba8();
    Rgba {
        r,
        g,
        b,
        a: f32::from(a) / 255.0,
    }
}

/// A colour of the shell's, as the theme of the moment gives it.
#[derive(Debug, Clone, Copy)]
pub enum Ink {
    /// One of the table's tokens.
    Token(fn(&Tokens) -> Rgba),
    /// Drawn for the dark theme; the light one reads it by rule.
    Dark(Color),
    /// A surface's rim drawn for the dark theme (a faint white): in Givre
    /// its icy hairline, `bar.border`.
    Rim(Color),
    /// The accent's base, at this opacity.
    Accent(f32),
    /// Text in the accent.
    AccentText,
    /// The accent's base lit, under the pointer.
    AccentLit,
    /// The same in both themes: the pandas, a badge's ink, the shade
    /// around a capture.
    Fixed(Color),
}

impl Ink {
    pub fn get(self) -> Color {
        self.in_theme(now())
    }

    pub fn in_theme(self, choice: Choice) -> Color {
        match self {
            Self::Token(token) => color(token(choice.tokens())),
            Self::Dark(c) => match (choice.scheme, choice.contrast) {
                (Scheme::Dark, false) => c,
                _ => color(choice.reads(rgba(c))),
            },
            Self::Rim(c) => match (choice.scheme, choice.contrast) {
                (Scheme::Frost, false) => color(choice.tokens().bar_border),
                _ => Self::Dark(c).in_theme(choice),
            },
            Self::Accent(alpha) => {
                let [r, g, b] = choice.accent().base;
                Color::from_rgba8(r, g, b, alpha)
            }
            Self::AccentText => {
                let [r, g, b] = choice.accent_text();
                Color::from_rgb8(r, g, b)
            }
            Self::Fixed(c) => c,
            Self::AccentLit => {
                let lit = |v: u8| v.saturating_add(((255 - u16::from(v)) / 9) as u8);
                let [r, g, b] = choice.accent().base;
                Color::from_rgb8(lit(r), lit(g), lit(b))
            }
        }
    }

    /// The same at `alpha` times its opacity.
    pub fn scale_alpha(self, alpha: f32) -> Color {
        self.get().scale_alpha(alpha)
    }
}

impl From<Ink> for Color {
    fn from(ink: Ink) -> Self {
        ink.get()
    }
}

impl From<Ink> for iced::widget::canvas::Fill {
    fn from(ink: Ink) -> Self {
        ink.get().into()
    }
}

impl From<Ink> for iced::Background {
    fn from(ink: Ink) -> Self {
        ink.get().into()
    }
}

/// A colour drawn for the dark theme, read in the theme of the moment.
pub fn ink(c: Color) -> Color {
    Ink::Dark(c).get()
}

/// `text`, `text.dim`, `text.faint` (bar.md §13).
pub const TEXT: Ink = Ink::Token(|t| t.text);
pub const DIM: Ink = Ink::Token(|t| t.dim);
pub const FAINT: Ink = Ink::Token(|t| t.faint);
/// `accent`, and as text.
pub const ACCENT: Ink = Ink::Accent(1.0);
pub const ACCENT_TEXT: Ink = Ink::AccentText;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inks_follow_the_theme() {
        let dark = Choice::default();
        assert_eq!(TEXT.in_theme(dark), Color::from_rgb8(0xEE, 0xF1, 0xF6));
        assert_eq!(ACCENT.in_theme(dark), Color::from_rgb8(0x6C, 0xC2, 0x87));
        let light = Choice {
            scheme: Scheme::Light,
            accent: Accent::Blue,
            ..Choice::default()
        };
        assert_eq!(TEXT.in_theme(light), Color::from_rgb8(0x14, 0x19, 0x26));
        assert_eq!(ACCENT.in_theme(light), Color::from_rgb8(0x5B, 0x8D, 0xEF));
        assert_eq!(
            ACCENT_TEXT.in_theme(light),
            Color::from_rgb8(0x2F, 0x5F, 0xC4)
        );
        // A white veil reads as the ink, a little stronger.
        let veil = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08)).in_theme(light);
        assert_eq!(veil.into_rgba8()[..3], [0x14, 0x19, 0x26]);
        assert!((veil.a - 0.1).abs() < 0.01, "{veil:?}");
        // Givre: icy texts and night-blue backgrounds, the rims its
        // hairline, the accent and the states as they are.
        let frost = Choice {
            scheme: Scheme::Frost,
            accent: Accent::Ice,
            ..Choice::default()
        };
        assert_eq!(TEXT.in_theme(frost), Color::from_rgb8(0xEA, 0xF6, 0xFF));
        assert_eq!(
            Ink::Dark(Color::from_rgb8(0xA9, 0xB2, 0xC2)).in_theme(frost),
            Color::from_rgb8(0x93, 0xAE, 0xC8)
        );
        assert_eq!(ACCENT.in_theme(frost), Color::from_rgb8(0x7F, 0xD8, 0xFF));
        let rim = Ink::Rim(Color::from_rgba8(255, 255, 255, 0.10));
        assert_eq!(rim.in_theme(frost).into_rgba8()[..3], [190, 232, 255]);
        assert_eq!(rim.in_theme(dark), Color::from_rgba8(255, 255, 255, 0.10));
        let wait = Ink::Dark(Color::from_rgb8(0xFF, 0xC2, 0x3D));
        assert_eq!(wait.in_theme(frost), Color::from_rgb8(0xFF, 0xC2, 0x3D));
        // High contrast wins over Givre.
        let contrast = Choice {
            contrast: true,
            ..frost
        };
        assert_eq!(TEXT.in_theme(contrast), Color::from_rgb8(0xFF, 0xFF, 0xFF));
        assert_eq!(
            rim.in_theme(contrast),
            Ink::Rim(Color::from_rgba8(255, 255, 255, 0.10)).in_theme(Choice {
                contrast: true,
                ..dark
            })
        );
    }
}
