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

    /// Hue (0 to 360), saturation and lightness (0 to 1).
    pub fn hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = (
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
        );
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0.0, 0.0, l);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h, s.clamp(0.0, 1.0), l)
    }

    pub fn from_hsl(h: f32, s: f32, l: f32) -> Color {
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = l - c / 2.0;
        let (r, g, b) = match h.rem_euclid(360.0) {
            h if h < 60.0 => (c, x, 0.0),
            h if h < 120.0 => (x, c, 0.0),
            h if h < 180.0 => (0.0, c, x),
            h if h < 240.0 => (0.0, x, c),
            h if h < 300.0 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Color::rgb(to(r), to(g), to(b))
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
    /// A panel's own surface: the surface, see-through over the blur behind
    /// the panel when the theme has glass on (#37).
    Glass,
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
    /// The hairline just inside every decorated window's edge, as the
    /// boards draw it: faint, in the theme's own tint.
    pub window_border: Color,
    /// From the accent chosen; not in theme files.
    #[serde(skip, default)]
    pub accent: Color,
    #[serde(skip, default)]
    pub on_accent: Color,
    /// The surface, see-through while there is glass; not in theme files.
    #[serde(skip, default)]
    pub glass: Color,
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
            Paint::Glass => self.glass,
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

/// Depth: frosted glass behind panels and shadows under windows (#37).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Effects {
    /// How opaque a panel's surface is over the blur behind it, 0 to 1.
    pub glass: f32,
    /// How far the blur behind panels reaches, logical pixels.
    pub blur: f32,
    /// Windows' shadows at their darkest, 0 to 1 (their colour is black).
    pub window_shadow: f32,
}

impl Default for Effects {
    /// The dark theme's.
    fn default() -> Self {
        Self {
            glass: 0.84,
            blur: 32.0,
            window_shadow: 0.5,
        }
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
    #[serde(default)]
    effects: Effects,
}

/// Where the world's light comes from through the day (#161): shadows
/// under windows and panels fall away from it, as the boards draw them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Daylight {
    Morning,
    #[default]
    Afternoon,
    Evening,
    Night,
}

impl Daylight {
    /// The light at `hour` (0 to 23), as the user reads the time.
    pub fn at(hour: u8) -> Self {
        match hour {
            6..=11 => Self::Morning,
            12..=17 => Self::Afternoon,
            18..=21 => Self::Evening,
            _ => Self::Night,
        }
    }

    /// The shadow's offset and blur, logical pixels, and how much darker
    /// than the theme's it is: the morning sun low on one side, the
    /// afternoon's high, the evening's low on the other, the night's lamp.
    fn shape(self) -> (f32, f32, f32, f32) {
        match self {
            Self::Morning => (26.0, 30.0, 60.0, 1.0),
            Self::Afternoon => (0.0, 36.0, 70.0, 1.0),
            Self::Evening => (-32.0, 28.0, 64.0, 1.0),
            Self::Night => (0.0, 22.0, 60.0, 1.3),
        }
    }
}

/// The shadow under windows and panels: black, offset and blurred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    /// At its darkest, 0 to 1.
    pub alpha: f32,
}

impl Shadow {
    pub fn color(&self) -> Color {
        Color::rgba(0, 0, 0, (self.alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
    pub accents: Accents,
    pub accent: Accent,
    pub faces: Faces,
    pub effects: Effects,
    /// The shadow of the hour, from [`Theme::with_daylight`].
    pub shadow: Shadow,
    pub radii: Radii,
    pub spacing: Spacing,
    pub text: TypeScale,
    /// Width of the keyboard focus ring.
    pub focus_ring: f32,
    /// Animations jump to their end (reduced motion).
    pub reduced_motion: bool,
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
            effects: file.effects,
            shadow: Shadow {
                dx: 0.0,
                dy: 0.0,
                blur: 0.0,
                alpha: 0.0,
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
            text: type_scale(),
            focus_ring: 2.0,
            reduced_motion: false,
        };
        theme.set_accent(theme.accent);
        theme.colors.glass = theme.colors.surface;
        Ok(theme.with_daylight(Daylight::default()))
    }

    /// Panels see-through over a blur of what is behind them, or as solid
    /// as their surface: the compositor turns glass on where it can blur.
    pub fn with_glass(mut self, on: bool) -> Self {
        let surface = self.colors.surface;
        self.colors.glass = if on {
            let alpha = f32::from(surface.a) * self.effects.glass.clamp(0.0, 1.0);
            surface.with_alpha(alpha.round() as u8)
        } else {
            surface
        };
        self
    }

    /// Whether panels are see-through: glass is on.
    /// Shadows as the light at that time of day casts them, as dark as
    /// the theme makes them (darker at night, at most 0.85).
    pub fn with_daylight(mut self, light: Daylight) -> Self {
        let (dx, dy, blur, darker) = light.shape();
        self.shadow = Shadow {
            dx,
            dy,
            blur,
            alpha: (self.effects.window_shadow * darker).min(0.85),
        };
        self
    }

    pub fn has_glass(&self) -> bool {
        self.colors.glass.a < self.colors.surface.a
    }

    /// Text `percent` of its size, 80 to 150.
    pub fn with_text_size(mut self, percent: u32) -> Self {
        let k = percent.clamp(80, 150) as f32 / 100.0;
        let t = &mut self.text;
        for style in [
            &mut t.caption,
            &mut t.body,
            &mut t.label,
            &mut t.title,
            &mut t.headline,
            &mut t.display,
        ] {
            style.size = (style.size * k * 2.0).round() / 2.0;
        }
        self
    }

    /// Sharper edges and plainer text: borders and muted text closer to
    /// the text colour, a thicker focus ring.
    pub fn with_high_contrast(mut self) -> Self {
        let c = &mut self.colors;
        c.muted = c.muted.mix(c.text, 0.6);
        c.border = c.border.mix(c.text, 0.5);
        c.window_border = c.window_border.mix(c.text, 0.5);
        c.surface_hover = c.surface_hover.mix(c.text, 0.06);
        self.focus_ring = 3.0;
        self
    }

    pub fn with_reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
    }

    /// The theme with an accent of its own colour (from a wallpaper): the
    /// same hue, lighter or darker until it reads at 4.5:1 on the
    /// background and the surfaces.
    pub fn with_custom_accent(mut self, color: Color) -> Self {
        let c = &self.colors;
        let grounds = [
            c.background,
            c.surface.over(c.background),
            c.surface_hover.over(c.background),
        ];
        let readable = |x: Color| grounds.iter().all(|g| x.contrast(*g) >= 4.5);
        let (h, s, l) = color.hsl();
        // Away from the background's own lightness.
        let up = c.background.luminance() < 0.5;
        let mut chosen = color.with_alpha(255);
        let mut l = l;
        while !readable(chosen) && (0.0..=1.0).contains(&l) {
            l += if up { 0.01 } else { -0.01 };
            chosen = Color::from_hsl(h, s, l.clamp(0.0, 1.0));
        }
        self.colors.accent = chosen;
        self.colors.on_accent = [Color::WHITE, self.colors.background, self.colors.text]
            .into_iter()
            .max_by(|a, b| a.contrast(chosen).total_cmp(&b.contrast(chosen)))
            .unwrap_or(Color::WHITE);
        self
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
    fn shadows_turn_with_the_hour_and_darken_at_night() {
        assert_eq!(Daylight::at(8), Daylight::Morning);
        assert_eq!(Daylight::at(14), Daylight::Afternoon);
        assert_eq!(Daylight::at(19), Daylight::Evening);
        assert_eq!(Daylight::at(2), Daylight::Night);
        let dark = Theme::builtin(ThemeName::Dark);
        // The light of the afternoon, falling straight down.
        assert_eq!(
            (dark.shadow.dx, dark.shadow.dy, dark.shadow.blur),
            (0.0, 36.0, 70.0)
        );
        let evening = dark.clone().with_daylight(Daylight::Evening);
        assert!(evening.shadow.dx < 0.0, "the low sun on the other side");
        let night = dark.with_daylight(Daylight::Night);
        assert!(
            (night.shadow.alpha - 0.65).abs() < 1e-6,
            "{}",
            night.shadow.alpha
        );
    }

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
    fn a_custom_accent_is_made_readable() {
        // A pale orange on the light theme, a dark one on the dark theme.
        for (name, color) in [
            (ThemeName::Light, Color::rgb(0xf8, 0xc0, 0x88)),
            (ThemeName::Dark, Color::rgb(0x5a, 0x2a, 0x05)),
        ] {
            let theme = Theme::builtin(name).with_custom_accent(color);
            assert_eq!(theme.contrast_failures(), vec![], "{theme}");
            let (h, _, _) = theme.colors.accent.hsl();
            assert!((20.0..45.0).contains(&h), "still orange: {h}");
        }
    }

    #[test]
    fn the_accent_comes_from_the_theme() {
        let dark = Theme::builtin(ThemeName::Dark).with_accent(Accent::Amber);
        let light = Theme::builtin(ThemeName::Light).with_accent(Accent::Amber);
        assert_eq!(dark.color(Paint::Accent), Color::rgb(0xf2, 0xa6, 0x5a));
        assert_eq!(light.color(Paint::Accent), Color::rgb(0x8f, 0x54, 0x10));
    }

    #[test]
    fn glass_is_the_surface_until_turned_on() {
        for name in ThemeName::ALL {
            let theme = Theme::builtin(name);
            assert_eq!(theme.color(Paint::Glass), theme.colors.surface);
            assert!(!theme.has_glass());
            let glass = theme.with_glass(true);
            assert!(glass.has_glass(), "{glass}");
            assert_eq!(glass.colors.glass.with_alpha(255), glass.colors.surface);
            assert!(!glass.with_glass(false).has_glass());
        }
    }

    #[test]
    fn text_reads_on_glass_whatever_is_behind() {
        for name in ThemeName::ALL {
            let theme = Theme::builtin(name).with_glass(true);
            let c = &theme.colors;
            for behind in [Color::BLACK, Color::WHITE, c.background] {
                let ground = c.glass.over(behind);
                let ratio = c.text.contrast(ground);
                assert!(
                    ratio >= 4.5,
                    "{theme}: text at {ratio:.2}:1 over {behind:?}"
                );
            }
        }
    }
}
