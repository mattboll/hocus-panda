//! Themes: colour tokens, typefaces, radii, spacing and the type scale.
//! Widgets name tokens ([`Paint`]), never colours, so that switching theme
//! or accent redraws everything in the new colours.
//!
//! The built-in themes are TOML files in `themes/`, from
//! docs/design/identity.md: dark, light, matrix, hud and paper, each with
//! its own variants of the accents.

use std::{fmt, str::FromStr};

use serde::Deserialize;

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
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const BLACK: Color = Color::rgb(0, 0, 0);

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

    /// This colour over `below`, opaque.
    pub fn over(self, below: Color) -> Color {
        below
            .with_alpha(255)
            .mix(self.with_alpha(255), f32::from(self.a) / 255.0)
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f32 {
        let channel = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// WCAG contrast ratio between two opaque colours, 1 to 21.
    pub fn contrast(self, other: Color) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::hex(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("{s:?} is not #rrggbb or #rrggbbaa")))
    }
}

/// A colour by its role in the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Behind everything.
    Background,
    /// Cards and panels.
    Surface,
    SurfaceHover,
    SurfacePressed,
    Border,
    Text,
    /// Secondary text.
    Muted,
    Accent,
    /// Text and icons on the accent.
    OnAccent,
    /// An agent acting on screen.
    Agent,
    Waiting,
    Error,
    Shadow,
    /// A colour that is not the theme's (content: a picture, a user's colour).
    Rgba(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    pub background: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_pressed: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub agent: Color,
    pub waiting: Color,
    pub error: Color,
    pub shadow: Color,
    /// From the accent chosen; not in theme files.
    #[serde(skip, default)]
    pub accent: Color,
    #[serde(skip, default)]
    pub on_accent: Color,
}

impl Colors {
    pub fn get(&self, paint: Paint) -> Color {
        match paint {
            Paint::Background => self.background,
            Paint::Surface => self.surface,
            Paint::SurfaceHover => self.surface_hover,
            Paint::SurfacePressed => self.surface_pressed,
            Paint::Border => self.border,
            Paint::Text => self.text,
            Paint::Muted => self.muted,
            Paint::Accent => self.accent,
            Paint::OnAccent => self.on_accent,
            Paint::Agent => self.agent,
            Paint::Waiting => self.waiting,
            Paint::Error => self.error,
            Paint::Shadow => self.shadow,
            Paint::Rgba(c) => c,
        }
    }
}

/// The accent options of the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Bamboo,
    Amber,
    Blue,
    Pink,
}

impl Accent {
    pub const ALL: [Accent; 4] = [Self::Bamboo, Self::Amber, Self::Blue, Self::Pink];

    pub fn name(self) -> &'static str {
        match self {
            Self::Bamboo => "bamboo",
            Self::Amber => "amber",
            Self::Blue => "blue",
            Self::Pink => "pink",
        }
    }
}

impl FromStr for Accent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == s)
            .ok_or_else(|| format!("unknown accent {s:?}: bamboo, amber, blue or pink"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accents {
    pub default: Accent,
    pub bamboo: Color,
    pub amber: Color,
    pub blue: Color,
    pub pink: Color,
}

impl Accents {
    pub fn get(&self, accent: Accent) -> Color {
        match accent {
            Accent::Bamboo => self.bamboo,
            Accent::Amber => self.amber,
            Accent::Blue => self.blue,
            Accent::Pink => self.pink,
        }
    }
}

/// The bundled typefaces (SIL Open Font License, fonts/licenses/).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Face {
    Manrope,
    Fraunces,
    JetbrainsMono,
    SpaceGrotesk,
}

impl Face {
    pub const ALL: [Face; 4] = [
        Self::Manrope,
        Self::Fraunces,
        Self::JetbrainsMono,
        Self::SpaceGrotesk,
    ];

    /// The family name in the font files.
    pub fn family(self) -> &'static str {
        match self {
            Self::Manrope => "Manrope",
            Self::Fraunces => "Fraunces",
            Self::JetbrainsMono => "JetBrains Mono",
            Self::SpaceGrotesk => "Space Grotesk",
        }
    }
}

/// Which face plays which role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Faces {
    pub interface: Face,
    pub display: Face,
    pub mono: Face,
}

impl Faces {
    pub fn get(&self, typeface: Typeface) -> Face {
        match typeface {
            Typeface::Interface => self.interface,
            Typeface::Display => self.display,
            Typeface::Mono => self.mono,
        }
    }
}

/// A role a typeface plays; the theme says which face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Typeface {
    /// The whole interface.
    Interface,
    /// Large titles, the clock.
    Display,
    /// Terminals and code.
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
    pub color: Paint,
}

impl TextStyle {
    pub fn with_color(self, color: Paint) -> Self {
        Self { color, ..self }
    }

    pub fn with_weight(self, weight: u16) -> Self {
        Self { weight, ..self }
    }
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

/// The built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeName {
    Dark,
    Light,
    Matrix,
    Hud,
    Paper,
}

impl ThemeName {
    pub const ALL: [ThemeName; 5] = [
        Self::Dark,
        Self::Light,
        Self::Matrix,
        Self::Hud,
        Self::Paper,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Matrix => "matrix",
            Self::Hud => "hud",
            Self::Paper => "paper",
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::Dark => include_str!("../themes/dark.toml"),
            Self::Light => include_str!("../themes/light.toml"),
            Self::Matrix => include_str!("../themes/matrix.toml"),
            Self::Hud => include_str!("../themes/hud.toml"),
            Self::Paper => include_str!("../themes/paper.toml"),
        }
    }
}

impl FromStr for ThemeName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.name() == s)
            .ok_or_else(|| format!("unknown theme {s:?}: dark, light, matrix, hud or paper"))
    }
}

/// A theme file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    colors: Colors,
    accents: Accents,
    fonts: Faces,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
    pub accents: Accents,
    pub accent: Accent,
    pub faces: Faces,
    pub radii: Radii,
    pub spacing: Spacing,
    pub text: TypeScale,
    /// Width of the keyboard focus ring.
    pub focus_ring: f32,
}

impl Theme {
    /// A built-in theme with its default accent.
    pub fn builtin(name: ThemeName) -> Self {
        match Self::from_toml(name.file()) {
            Ok(theme) => theme,
            // The files are part of the build and tested.
            Err(err) => unreachable!("built-in theme {}: {err}", name.name()),
        }
    }

    /// A theme from a file in the format of `themes/*.toml`.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut theme = Self {
            name: file.name,
            colors: file.colors,
            accents: file.accents,
            accent: file.accents.default,
            faces: file.fonts,
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
            text: type_scale(),
            focus_ring: 2.0,
        };
        theme.set_accent(theme.accent);
        Ok(theme)
    }

    /// The theme with another of its accents.
    pub fn with_accent(mut self, accent: Accent) -> Self {
        self.set_accent(accent);
        self
    }

    fn set_accent(&mut self, accent: Accent) {
        let color = self.accents.get(accent);
        self.accent = accent;
        self.colors.accent = color;
        // Whichever of white, the background and the text reads best on it.
        self.colors.on_accent = [Color::WHITE, self.colors.background, self.colors.text]
            .into_iter()
            .max_by(|a, b| a.contrast(color).total_cmp(&b.contrast(color)))
            .unwrap_or(Color::WHITE);
    }

    pub fn color(&self, paint: Paint) -> Color {
        self.colors.get(paint)
    }

    /// Text and background pairs under 4.5:1, with their ratio: what the
    /// theme must not have.
    pub fn contrast_failures(&self) -> Vec<(String, f32)> {
        let c = &self.colors;
        let grounds = [
            ("background", c.background),
            ("surface", c.surface.over(c.background)),
            ("surface_hover", c.surface_hover.over(c.background)),
        ];
        let inks = [
            ("text", c.text),
            ("muted", c.muted),
            ("accent", c.accent),
            ("agent", c.agent),
            ("waiting", c.waiting),
            ("error", c.error),
        ];
        let mut failures = Vec::new();
        for (ink, color) in inks {
            for (ground, below) in grounds {
                let ratio = color.over(below).contrast(below);
                if ratio < 4.5 {
                    failures.push((format!("{ink} on {ground}"), ratio));
                }
            }
        }
        let ratio = c.on_accent.contrast(c.accent);
        if ratio < 4.5 {
            failures.push(("on_accent on accent".into(), ratio));
        }
        failures
    }
}

fn type_scale() -> TypeScale {
    let style = |size, weight, line_height, color| TextStyle {
        typeface: Typeface::Interface,
        size,
        weight,
        line_height,
        color,
    };
    TypeScale {
        caption: style(12.0, 700, 1.3, Paint::Muted),
        body: style(14.0, 500, 1.4, Paint::Text),
        label: style(13.0, 600, 1.3, Paint::Text),
        title: style(16.0, 600, 1.3, Paint::Text),
        headline: style(22.0, 600, 1.25, Paint::Text),
        display: TextStyle {
            typeface: Typeface::Display,
            ..style(64.0, 400, 1.1, Paint::Text)
        },
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::builtin(ThemeName::Dark)
    }
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.accent.name())
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

    #[test]
    fn contrast_follows_wcag() {
        assert!((Color::BLACK.contrast(Color::WHITE) - 21.0).abs() < 0.01);
        assert!((Color::WHITE.contrast(Color::WHITE) - 1.0).abs() < 0.01);
    }

    #[test]
    fn every_builtin_theme_reads_with_every_accent() {
        for name in ThemeName::ALL {
            for accent in Accent::ALL {
                let theme = Theme::builtin(name).with_accent(accent);
                assert_eq!(theme.name, name.name());
                assert_eq!(
                    theme.contrast_failures(),
                    vec![],
                    "{theme}: text under 4.5:1"
                );
            }
        }
    }

    #[test]
    fn a_wrong_file_says_what_is_wrong() {
        let text = ThemeName::Dark.file().replace("#0E0F12", "#0E0F1");
        let err = Theme::from_toml(&text).err().unwrap_or_default();
        assert!(err.contains("#0E0F1"), "{err}");
    }

    #[test]
    fn the_accent_comes_from_the_theme() {
        let dark = Theme::builtin(ThemeName::Dark).with_accent(Accent::Amber);
        let light = Theme::builtin(ThemeName::Light).with_accent(Accent::Amber);
        assert_eq!(dark.color(Paint::Accent), Color::rgb(0xf2, 0xa6, 0x5a));
        assert_eq!(light.color(Paint::Accent), Color::rgb(0x8f, 0x54, 0x10));
    }
}
