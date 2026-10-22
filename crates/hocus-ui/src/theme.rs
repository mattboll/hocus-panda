//! Theme tokens: colours, radii, spacing and the type scale. Widgets take
//! their looks from here, never from literals, so that a theme (#22) is a
//! different set of tokens.

/// A colour, not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Mixes towards `other` by `t` (0 to 1), alpha included.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color::rgba(
            m(self.r, other.r),
            m(self.g, other.g),
            m(self.b, other.b),
            m(self.a, other.a),
        )
    }
}

/// The interface's typefaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Typeface {
    /// The whole interface (Manrope).
    Interface,
    /// Large titles, the clock (Fraunces, #22; the interface face until then).
    Display,
    /// Terminals and code (JetBrains Mono, #22).
    Mono,
}

/// A step of the type scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub typeface: Typeface,
    pub size: f32,
    pub weight: u16,
    /// Line height as a multiple of the size.
    pub line_height: f32,
    pub color: Color,
}

impl TextStyle {
    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    pub fn with_weight(self, weight: u16) -> Self {
        Self { weight, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    /// Behind everything.
    pub background: Color,
    /// Cards and panels.
    pub surface: Color,
    /// Hovered and pressed surfaces.
    pub surface_hover: Color,
    pub surface_pressed: Color,
    pub border: Color,
    pub text: Color,
    /// Secondary text; still 4.5:1 on the surface.
    pub muted: Color,
    pub accent: Color,
    /// Text on the accent.
    pub on_accent: Color,
    /// An agent acting on screen.
    pub agent: Color,
    pub waiting: Color,
    pub error: Color,
    pub shadow: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    pub small: f32,
    pub medium: f32,
    pub large: f32,
}

/// Spacing steps, on a 4 px grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    pub caption: TextStyle,
    pub body: TextStyle,
    pub label: TextStyle,
    pub title: TextStyle,
    pub headline: TextStyle,
    pub display: TextStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub colors: Colors,
    pub radii: Radii,
    pub spacing: Spacing,
    pub text: TypeScale,
    /// Width of the keyboard focus ring.
    pub focus_ring: f32,
}

impl Theme {
    /// Bamboo, dark: the default theme of the identity.
    pub fn bamboo_dark() -> Self {
        let text = Color::rgb(0xee, 0xf2, 0xee);
        let style = |size, weight, line_height, color| TextStyle {
            typeface: Typeface::Interface,
            size,
            weight,
            line_height,
            color,
        };
        let muted = Color::rgb(0xa8, 0xb4, 0xab);
        Self {
            colors: Colors {
                background: Color::rgb(0x10, 0x14, 0x12),
                surface: Color::rgb(0x1b, 0x22, 0x1e),
                surface_hover: Color::rgb(0x24, 0x2d, 0x28),
                surface_pressed: Color::rgb(0x2c, 0x37, 0x31),
                border: Color::rgba(0xff, 0xff, 0xff, 0x14),
                text,
                muted,
                accent: Color::rgb(0x7b, 0xc4, 0x7f),
                on_accent: Color::rgb(0x0d, 0x1a, 0x10),
                agent: Color::rgb(0xb4, 0x8c, 0xff),
                waiting: Color::rgb(0xf2, 0xa6, 0x5a),
                error: Color::rgb(0xf2, 0x7a, 0x6b),
                shadow: Color::rgba(0, 0, 0, 0x60),
            },
            radii: Radii {
                small: 8.0,
                medium: 12.0,
                large: 16.0,
            },
            spacing: Spacing {
                xs: 4.0,
                s: 8.0,
                m: 12.0,
                l: 16.0,
                xl: 24.0,
            },
            text: TypeScale {
                caption: style(12.0, 700, 1.3, muted),
                body: style(14.0, 500, 1.4, text),
                label: style(13.0, 600, 1.3, text),
                title: style(16.0, 600, 1.3, text),
                headline: style(22.0, 600, 1.25, text),
                display: TextStyle {
                    typeface: Typeface::Display,
                    ..style(64.0, 400, 1.1, text)
                },
            },
            focus_ring: 2.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::bamboo_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_from_hex() {
        assert_eq!(Color::hex("#7bc47f"), Some(Color::rgb(0x7b, 0xc4, 0x7f)));
        assert_eq!(Color::hex("#00000080"), Some(Color::rgba(0, 0, 0, 0x80)));
        assert_eq!(Color::hex("7bc47f"), None);
    }
}
