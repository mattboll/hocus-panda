//! The theme the window draws in (#402): `[appearance] theme` and `accent`
//! as the file says them, read with the rest of it ([`crate::worker`]);
//! the window is drawn again in it as soon as the file changes, so that it
//! is the page's own preview.
//!
//! The window's colours are [`Ink`]s, as the shell's are: a token of
//! [`hocus_link::theme::Tokens`], the accent, or a colour drawn for the
//! dark theme that the light one reads by [`hocus_link::theme::on_light`].

use std::sync::{PoisonError, RwLock};

use hocus_link::theme::{Accent, Choice, Rgba, Scheme, Tokens};
use iced::Color;

static NOW: RwLock<Choice> = RwLock::new(Choice {
    scheme: Scheme::Dark,
    accent: Accent::Green,
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

pub fn tokens() -> &'static Tokens {
    now().tokens()
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
    /// The accent's base, at this opacity.
    Accent(f32),
    /// Text in the accent.
    AccentText,
    /// The accent's base lit, under the pointer.
    AccentLit,
}

impl Ink {
    pub fn get(self) -> Color {
        self.in_theme(now())
    }

    pub fn in_theme(self, choice: Choice) -> Color {
        match self {
            Self::Token(token) => color(token(choice.tokens())),
            Self::Dark(c) => match choice.scheme {
                Scheme::Dark => c,
                Scheme::Light => color(hocus_link::theme::on_light(rgba(c))),
            },
            Self::Accent(alpha) => {
                let [r, g, b] = choice.accent().base;
                Color::from_rgba8(r, g, b, alpha)
            }
            Self::AccentText => {
                let [r, g, b] = choice.accent_text();
                Color::from_rgb8(r, g, b)
            }
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

impl From<Ink> for iced::Background {
    fn from(ink: Ink) -> Self {
        ink.get().into()
    }
}

/// A colour drawn for the dark theme, read in the theme of the moment.
pub fn ink(c: Color) -> Color {
    Ink::Dark(c).get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inks_follow_the_theme() {
        let dark = Choice::default();
        let text = Ink::Token(|t| t.text);
        assert_eq!(text.in_theme(dark), Color::from_rgb8(0xEE, 0xF1, 0xF6));
        assert_eq!(
            Ink::Accent(1.0).in_theme(dark),
            Color::from_rgb8(0x6C, 0xC2, 0x87)
        );
        let light = Choice {
            scheme: Scheme::Light,
            accent: Accent::Blue,
        };
        assert_eq!(text.in_theme(light), Color::from_rgb8(0x14, 0x19, 0x26));
        assert_eq!(
            Ink::Accent(1.0).in_theme(light),
            Color::from_rgb8(0x5B, 0x8D, 0xEF)
        );
        assert_eq!(
            Ink::AccentText.in_theme(light),
            Color::from_rgb8(0x2F, 0x5F, 0xC4)
        );
        // A white veil reads as the ink, a little stronger.
        let veil = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08)).in_theme(light);
        assert_eq!(veil.into_rgba8()[..3], [0x14, 0x19, 0x26]);
        assert!((veil.a - 0.1).abs() < 0.01, "{veil:?}");
    }
}
