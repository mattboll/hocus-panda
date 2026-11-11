//! The desktop's theme (bar.md §13, issue #402): one table of tokens with a
//! dark and a light value each, and the accent, as `[appearance] theme` and
//! `accent` choose them. The compositor reads the file and tells the shell
//! ([`crate::ToShell::Theme`]) and the applications (the Settings portal);
//! the Settings read the same keys from the file themselves. Both programs
//! draw with these values, so that the bar, its popovers, the lock, the
//! authentication dialog and the Settings window change together.
//!
//! High contrast (`[appearance] high-contrast`, #423) reinforces the theme
//! chosen, dark or light, and keeps the accent: its own tables of tokens
//! ([`DARK_CONTRAST`], [`LIGHT_CONTRAST`]), text at 7:1 and borders at 3:1
//! at least, the accent as text lifted or darkened to 7:1, and the colours
//! drawn for the dark theme read by [`on_contrast`].
//!
//! Givre (`theme = "frost"`, #740, bar.md §13 "Givre") is a themed theme:
//! the shell's surfaces take its tokens ([`FROST`]: night-blue glass, icy
//! text and hairlines), and read the colours drawn for the dark theme by
//! [`on_frost`]; the Settings window and the applications stay dark
//! ([`Scheme::windows`]), only the accent is the theme's. High contrast
//! wins over it: the dark theme's high-contrast tokens. Matrix
//! (`theme = "matrix"`, #431, variant C "discret, avec lisière") is the
//! second: near-black glass, green only for the accent and a few marks
//! ([`MATRIX`], [`on_matrix`]).
//!
//! No toolkit here: colours are red, green, blue and an opacity, which each
//! program turns into its own.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::BadLine;

/// `[appearance] theme`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scheme {
    #[default]
    Dark,
    Light,
    /// Givre: icy blue on night-blue glass, for the shell's surfaces only.
    Frost,
    /// Matrix: near-black glass, the phosphor green in touches.
    Matrix,
}

impl Scheme {
    pub const ALL: [Self; 4] = [Self::Dark, Self::Light, Self::Frost, Self::Matrix];

    /// Its word in the file and on the line.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Frost => "frost",
            Self::Matrix => "matrix",
        }
    }

    /// The theme application windows draw in, the Settings' among them: a
    /// themed theme (Givre) keeps them dark, only its accent shows there.
    pub const fn windows(self) -> Self {
        match self {
            Self::Frost | Self::Matrix => Self::Dark,
            scheme => scheme,
        }
    }

    /// Whether it is a themed theme: one of the shell's own look (Givre,
    /// Matrix), with marks of its own and its pandas' dress.
    pub const fn themed(self) -> bool {
        matches!(self, Self::Frost | Self::Matrix)
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.word() == word)
    }

    /// Its tokens.
    pub const fn tokens(self) -> &'static Tokens {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
            Self::Frost => &FROST,
            Self::Matrix => &MATRIX,
        }
    }

    /// Its tokens under high contrast: a themed theme's are the dark
    /// one's (high contrast wins over the look).
    pub const fn contrast_tokens(self) -> &'static Tokens {
        match self {
            Self::Dark | Self::Frost | Self::Matrix => &DARK_CONTRAST,
            Self::Light => &LIGHT_CONTRAST,
        }
    }

    /// `org.freedesktop.appearance` `color-scheme`: 1 prefers dark, 2
    /// prefers light (0, no preference, is never said).
    pub const fn portal_value(self) -> u32 {
        match self {
            Self::Dark | Self::Frost | Self::Matrix => 1,
            Self::Light => 2,
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

impl FromStr for Scheme {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::of(s).ok_or_else(|| BadLine(format!("no theme {s:?}: dark, light, frost or matrix")))
    }
}

/// `[appearance] accent`: six hues chosen not to be the bar's states
/// (amber is `state.wait`, orange `state.stuck`), the pandas' green first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Accent {
    #[default]
    Green,
    Blue,
    Violet,
    Rose,
    Teal,
    /// The wallpaper's dominant hue, lifted to the others' lightness.
    Wallpaper,
    /// Givre's ice blue, `#7FD8FF`: the accent choosing Givre sets.
    Ice,
    /// Matrix's phosphor green, `#00F060`: the accent choosing Matrix sets.
    Phosphor,
}

/// An accent's three values: `base` fills the switches, the selection and
/// the check; text in the accent is `on_dark` or `on_light`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccentColours {
    pub base: [u8; 3],
    pub on_dark: [u8; 3],
    pub on_light: [u8; 3],
}

/// The default backdrop's hue lifted to the accents' lightness (Nuit, the
/// gradient of [`crate::backdrop`]). The other wallpapers give their own
/// ([`crate::wallpaper`]).
pub const BACKDROP_ACCENT: AccentColours = AccentColours {
    base: [0x7A, 0x96, 0xE0],
    on_dark: [0xA4, 0xB8, 0xF0],
    on_light: [0x3A, 0x58, 0xB0],
};

impl Default for AccentColours {
    fn default() -> Self {
        BACKDROP_ACCENT
    }
}

impl AccentColours {
    /// A colour's hue lifted to the six accents' lightness: the base at
    /// about theirs, text on dark lighter and on light darker, each until
    /// it reads at 4.5 to 1 on the window. A colour without a hue (a grey)
    /// gives a grey-blue.
    pub fn lifted([r, g, b]: [u8; 3]) -> Self {
        let (hue, saturation, _) = hsl([r, g, b]);
        let saturation = if saturation < 0.08 {
            0.25
        } else {
            saturation.clamp(0.45, 0.65)
        };
        let base = rgb_of(hue, saturation, 0.66);
        let mut on_dark_l: f64 = 0.78;
        let mut on_dark = rgb_of(hue, saturation, on_dark_l);
        while contrast_rgb(on_dark, DARK.window) < 4.6 && on_dark_l < 0.97 {
            on_dark_l += 0.01;
            on_dark = rgb_of(hue, saturation, on_dark_l);
        }
        let mut on_light_l: f64 = 0.44;
        let mut on_light = rgb_of(hue, saturation, on_light_l);
        while contrast_rgb(on_light, LIGHT.window) < 4.6 && on_light_l > 0.05 {
            on_light_l -= 0.01;
            on_light = rgb_of(hue, saturation, on_light_l);
        }
        Self {
            base,
            on_dark,
            on_light,
        }
    }

    /// For the log: `RRGGBB,RRGGBB,RRGGBB`.
    fn word(self) -> String {
        let hex = |[r, g, b]: [u8; 3]| format!("{r:02x}{g:02x}{b:02x}");
        format!(
            "{},{},{}",
            hex(self.base),
            hex(self.on_dark),
            hex(self.on_light)
        )
    }
}

/// Hue (degrees), saturation and lightness of a colour.
fn hsl([r, g, b]: [u8; 3]) -> (f64, f64, f64) {
    let [r, g, b] = [r, g, b].map(|v| f64::from(v) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f64::EPSILON {
        return (220.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn rgb_of(h: f64, s: f64, l: f64) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// WCAG's relative luminance.
fn luminance_rgb([r, g, b]: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast_rgb(a: [u8; 3], b: Rgba) -> f64 {
    let (x, y) = (luminance_rgb(a), luminance_rgb([b.r, b.g, b.b]));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

impl Accent {
    pub const ALL: [Self; 8] = [
        Self::Green,
        Self::Blue,
        Self::Violet,
        Self::Rose,
        Self::Teal,
        Self::Wallpaper,
        Self::Ice,
        Self::Phosphor,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Rose => "rose",
            Self::Teal => "teal",
            Self::Wallpaper => "wallpaper",
            Self::Ice => "ice",
            Self::Phosphor => "phosphor",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.word() == word)
    }

    /// Its name, in French then English.
    pub const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Green => ("Vert des pandas", "Panda green"),
            Self::Blue => ("Bleu", "Blue"),
            Self::Violet => ("Violet", "Violet"),
            Self::Rose => ("Rose", "Pink"),
            Self::Teal => ("Sarcelle", "Teal"),
            Self::Wallpaper => ("Du fond d'écran", "From the wallpaper"),
            Self::Ice => ("Bleu glacé", "Ice blue"),
            Self::Phosphor => ("Vert phosphore", "Phosphor green"),
        }
    }

    pub const fn colours(self) -> AccentColours {
        match self {
            Self::Green => AccentColours {
                base: [0x6C, 0xC2, 0x87],
                on_dark: [0x7E, 0xE0, 0xA0],
                on_light: [0x2E, 0x7D, 0x4A],
            },
            Self::Blue => AccentColours {
                base: [0x5B, 0x8D, 0xEF],
                on_dark: [0x8F, 0xB3, 0xFF],
                on_light: [0x2F, 0x5F, 0xC4],
            },
            Self::Violet => AccentColours {
                base: [0x9B, 0x84, 0xF0],
                on_dark: [0xC3, 0xB4, 0xFF],
                on_light: [0x5B, 0x45, 0xB8],
            },
            Self::Rose => AccentColours {
                base: [0xE0, 0x7A, 0xB5],
                on_dark: [0xF2, 0xA6, 0xD0],
                on_light: [0xA8, 0x32, 0x6F],
            },
            Self::Teal => AccentColours {
                base: [0x3F, 0xB8, 0xAF],
                on_dark: [0x7F, 0xD8, 0xD0],
                on_light: [0x1D, 0x76, 0x6F],
            },
            Self::Wallpaper => BACKDROP_ACCENT,
            Self::Ice => AccentColours {
                base: [0x7F, 0xD8, 0xFF],
                on_dark: [0x7F, 0xD8, 0xFF],
                on_light: [0x1F, 0x6E, 0x94],
            },
            Self::Phosphor => AccentColours {
                base: [0x00, 0xF0, 0x60],
                on_dark: [0x00, 0xF0, 0x60],
                on_light: [0x0B, 0x70, 0x34],
            },
        }
    }
}

impl fmt::Display for Choice {
    /// `SCHEME ACCENT WALLPAPER`, then `contrast` under high contrast: for
    /// the log (the Settings' tests read it), not the line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.scheme,
            self.accent,
            self.wallpaper.word()
        )?;
        if self.contrast {
            f.write_str(" contrast")?;
        }
        Ok(())
    }
}

impl fmt::Display for Accent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

impl FromStr for Accent {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::of(s).ok_or_else(|| {
            BadLine(format!(
                "no accent {s:?}: green, blue, violet, rose, teal, wallpaper, ice or phosphor"
            ))
        })
    }
}

/// The theme the desktop draws in: written `theme SCHEME ACCENT
/// WALLPAPER` on the line (the wallpaper's accent's three colours,
/// [`AccentColours`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Choice {
    pub scheme: Scheme,
    pub accent: Accent,
    /// What `accent = "wallpaper"` is now: the primary screen's
    /// wallpaper's (6.3), Nuit's by default.
    pub wallpaper: AccentColours,
    /// `[appearance] high-contrast`: the theme reinforced (#423).
    pub contrast: bool,
}

impl Choice {
    pub const fn tokens(self) -> &'static Tokens {
        if self.contrast {
            self.scheme.contrast_tokens()
        } else {
            self.scheme.tokens()
        }
    }

    /// A colour drawn for the dark theme, as this theme reads it: itself,
    /// [`on_light`]'s, or [`on_contrast`]'s.
    pub fn reads(self, dark: Rgba) -> Rgba {
        match (self.contrast, self.scheme) {
            (false, Scheme::Dark) => dark,
            (false, Scheme::Light) => on_light(dark),
            (false, Scheme::Frost) => on_frost(dark),
            (false, Scheme::Matrix) => on_matrix(dark),
            (true, scheme) => on_contrast(dark, scheme),
        }
    }

    /// The same theme as application windows draw in it (the Settings):
    /// Givre's windows are dark, with its accent.
    pub const fn in_windows(self) -> Self {
        Self {
            scheme: self.scheme.windows(),
            ..self
        }
    }

    pub const fn accent(self) -> AccentColours {
        match self.accent {
            Accent::Wallpaper => self.wallpaper,
            accent => accent.colours(),
        }
    }

    /// Text in the accent, on this theme's backgrounds; under high
    /// contrast lifted (dark) or darkened (light) until it reads at 7:1 on
    /// every background of the theme (the keyboard's ring too).
    pub fn accent_text(self) -> [u8; 3] {
        let colours = self.accent();
        let text = match self.scheme {
            Scheme::Dark | Scheme::Frost | Scheme::Matrix => colours.on_dark,
            Scheme::Light => colours.on_light,
        };
        if self.contrast {
            to_contrast(text, self.tokens(), CONTRAST_TEXT)
        } else {
            text
        }
    }
}

/// Text's contrast under high contrast, and the borders' and controls'.
pub const CONTRAST_TEXT: f64 = 7.0;
pub const CONTRAST_BORDER: f64 = 3.0;

/// The backgrounds text is read on in `tokens`' theme: the windows', the
/// sidebars', the bars' and the popovers', the menus'.
pub fn backgrounds(tokens: &Tokens) -> [Rgba; 6] {
    [
        tokens.window,
        tokens.sidebar,
        tokens.bar_opaque_top,
        tokens.bar_opaque_bottom,
        tokens.popover_opaque_bottom,
        tokens.menu,
    ]
}

/// `rgb`, its hue kept, lighter (on dark backgrounds) or darker until it
/// reads at `ratio` on every background of `tokens`.
fn to_contrast(rgb: [u8; 3], tokens: &Tokens, ratio: f64) -> [u8; 3] {
    let worst = |c: [u8; 3]| {
        backgrounds(tokens)
            .iter()
            .map(|b| contrast_rgb(c, *b))
            .fold(f64::INFINITY, f64::min)
    };
    let dark = luminance_rgb([tokens.window.r, tokens.window.g, tokens.window.b]) < 0.2;
    let (hue, saturation, mut lightness) = hsl(rgb);
    let mut out = rgb;
    while worst(out) < ratio && (0.0..=1.0).contains(&lightness) {
        lightness += if dark { 0.01 } else { -0.01 };
        out = rgb_of(hue, saturation, lightness.clamp(0.0, 1.0));
    }
    out
}

/// A colour: red, green, blue, and its opacity from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

const fn hex(rgb: u32) -> Rgba {
    Rgba {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 1.0,
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    Rgba { r, g, b, a }
}

/// The tokens, each with its value in each theme (bar.md §13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// The Settings window's content, the polkit dialog's and the lock's
    /// panels.
    pub window: Rgba,
    pub sidebar: Rgba,
    /// `text`, `text.2`, `text.dim`, `text.faint`.
    pub text: Rgba,
    pub text2: Rgba,
    pub dim: Rgba,
    pub faint: Rgba,
    /// A group's card, its rim, the dividers between its rows.
    pub card: Rgba,
    pub card_rim: Rgba,
    pub divider: Rgba,
    /// A button's or a field's fill and rim; a menu's list.
    pub control: Rgba,
    pub control_rim: Rgba,
    pub menu: Rgba,
    /// A switch turned off; a segmented control's track and its chosen
    /// segment.
    pub switch_off: Rgba,
    pub segmented: Rgba,
    pub segment_chosen: Rgba,
    /// The focused window's rim, and with high contrast.
    pub window_rim: Rgba,
    pub high_contrast: Rgba,
    /// `bar.bg` on glass (top, bottom), and opaque.
    pub bar_top: Rgba,
    pub bar_bottom: Rgba,
    pub bar_opaque_top: Rgba,
    pub bar_opaque_bottom: Rgba,
    /// `bar.border`.
    pub bar_border: Rgba,
    /// The popovers' background on glass (top, bottom), and opaque.
    pub popover_top: Rgba,
    pub popover_bottom: Rgba,
    pub popover_opaque_top: Rgba,
    pub popover_opaque_bottom: Rgba,
    /// What hovers and fills are tinted with: white on dark, the text's
    /// ink on light, each at its own opacity.
    pub over: Rgba,
    /// The shadows' colour.
    pub shadow: Rgba,
    /// The Settings' toast: dark in both themes.
    pub toast: Rgba,
    /// Text and marks on an accent's fill.
    pub on_accent: Rgba,
}

pub const DARK: Tokens = Tokens {
    window: hex(0x141926),
    sidebar: hex(0x0F131C),
    text: hex(0xEEF1F6),
    text2: hex(0xD5DBE5),
    dim: hex(0xA9B2C2),
    faint: hex(0x8C96A8),
    card: rgba(255, 255, 255, 0.035),
    card_rim: rgba(255, 255, 255, 0.07),
    divider: rgba(255, 255, 255, 0.06),
    control: rgba(255, 255, 255, 0.06),
    control_rim: rgba(255, 255, 255, 0.12),
    menu: hex(0x1F2534),
    switch_off: rgba(255, 255, 255, 0.18),
    segmented: rgba(255, 255, 255, 0.05),
    segment_chosen: rgba(255, 255, 255, 0.13),
    window_rim: rgba(255, 255, 255, 0.14),
    high_contrast: rgba(255, 255, 255, 0.90),
    bar_top: rgba(22, 27, 42, 0.70),
    bar_bottom: rgba(14, 17, 28, 0.78),
    bar_opaque_top: hex(0x161B2A),
    bar_opaque_bottom: hex(0x0E111C),
    bar_border: rgba(255, 255, 255, 0.10),
    popover_top: rgba(22, 27, 42, 0.72),
    popover_bottom: rgba(14, 17, 28, 0.82),
    popover_opaque_top: hex(0x161B2A),
    popover_opaque_bottom: hex(0x0E111C),
    over: hex(0xFFFFFF),
    shadow: hex(0x000000),
    toast: hex(0x262C3C),
    on_accent: hex(0x0B1020),
};

pub const LIGHT: Tokens = Tokens {
    window: hex(0xF5F6F9),
    sidebar: hex(0xE9ECF2),
    text: hex(0x141926),
    text2: hex(0x2A3142),
    dim: hex(0x4A5466),
    faint: hex(0x5C6678),
    card: hex(0xFFFFFF),
    card_rim: rgba(20, 25, 38, 0.09),
    divider: rgba(20, 25, 38, 0.08),
    control: hex(0xFFFFFF),
    control_rim: rgba(20, 25, 38, 0.16),
    menu: hex(0xFFFFFF),
    switch_off: rgba(20, 25, 38, 0.22),
    segmented: rgba(20, 25, 38, 0.06),
    segment_chosen: hex(0xFFFFFF),
    window_rim: rgba(20, 25, 38, 0.18),
    high_contrast: hex(0x141926),
    bar_top: rgba(245, 246, 249, 0.62),
    bar_bottom: rgba(238, 240, 245, 0.70),
    bar_opaque_top: hex(0xF3F4F8),
    bar_opaque_bottom: hex(0xEEF0F5),
    bar_border: rgba(20, 25, 38, 0.12),
    popover_top: rgba(245, 246, 249, 0.84),
    popover_bottom: rgba(238, 240, 245, 0.90),
    popover_opaque_top: hex(0xF5F6F9),
    popover_opaque_bottom: hex(0xEEF0F5),
    over: hex(0x141926),
    shadow: hex(0x0B1020),
    toast: hex(0x262C3C),
    on_accent: hex(0x0B1020),
};

/// Givre (#740, bar.md §13): night-blue glass, icy text and hairlines.
/// Its texts read at 7:1 at least on the opaque fallback (text 15.8,
/// text.2 12.0, text.dim 7.6), the accent at 10.9; its rim, the icy
/// hairline, at 3:1 over it. The Settings never draw in it.
pub const FROST: Tokens = Tokens {
    window: hex(0x0E1A30),
    sidebar: hex(0x08101F),
    text: hex(0xEAF6FF),
    text2: hex(0xC4D9EA),
    dim: hex(0x93AEC8),
    faint: hex(0x7D98B2),
    card: rgba(160, 220, 255, 0.05),
    card_rim: rgba(190, 232, 255, 0.10),
    divider: rgba(190, 232, 255, 0.08),
    control: rgba(255, 255, 255, 0.06),
    control_rim: rgba(190, 232, 255, 0.16),
    menu: hex(0x111E36),
    switch_off: rgba(255, 255, 255, 0.18),
    segmented: rgba(255, 255, 255, 0.05),
    segment_chosen: rgba(255, 255, 255, 0.13),
    window_rim: rgba(255, 255, 255, 0.14),
    high_contrast: rgba(255, 255, 255, 0.90),
    bar_top: rgba(10, 20, 40, 0.58),
    bar_bottom: rgba(6, 12, 26, 0.68),
    bar_opaque_top: hex(0x0E1A30),
    bar_opaque_bottom: hex(0x08101F),
    bar_border: rgba(190, 232, 255, 0.40),
    popover_top: rgba(10, 20, 40, 0.62),
    popover_bottom: rgba(6, 12, 26, 0.74),
    popover_opaque_top: hex(0x0E1A30),
    popover_opaque_bottom: hex(0x08101F),
    over: hex(0xFFFFFF),
    shadow: hex(0x000000),
    toast: hex(0x262C3C),
    on_accent: hex(0x04111D),
};

/// Givre's viewfinder corner marks: 10 px, 1.5 px thick, `#BFEFFF` at
/// 75 %, 5 px in from the corners of the bar, its popovers and the
/// notifications' bubbles.
pub const FROST_CORNER: Rgba = rgba(0xBF, 0xEF, 0xFF, 0.75);
/// The clock's glow in Givre: `#7FD8FF` at 35 %, 14 px.
pub const FROST_GLOW: Rgba = rgba(0x7F, 0xD8, 0xFF, 0.35);
/// The pandas' green, and what it is in Givre.
pub const PANDA_GREEN: [u8; 3] = [0x7B, 0xC4, 0x7F];

/// Matrix (#431, variant C): glass black with a night-blue cast, never
/// green nor brown; green only for the accent, the rims at 14 %, the
/// clock's colon, a hairline atop the bar and the lock's card, and the
/// code edge. Its texts read at 7:1 at least on the opaque fallback (text
/// 16.3, text.2 12.0, text.dim 7.3), the accent at 12.2. The Settings
/// never draw in it.
pub const MATRIX: Tokens = Tokens {
    window: hex(0x0C1218),
    sidebar: hex(0x060A12),
    text: hex(0xE8F1EC),
    text2: hex(0xC2D2C8),
    dim: hex(0x8FA79A),
    faint: hex(0x7A9285),
    card: rgba(0, 240, 96, 0.04),
    card_rim: rgba(255, 255, 255, 0.07),
    divider: rgba(255, 255, 255, 0.06),
    control: rgba(255, 255, 255, 0.06),
    control_rim: rgba(255, 255, 255, 0.12),
    menu: hex(0x0F161E),
    switch_off: rgba(255, 255, 255, 0.18),
    segmented: rgba(255, 255, 255, 0.05),
    segment_chosen: rgba(255, 255, 255, 0.13),
    window_rim: rgba(255, 255, 255, 0.14),
    high_contrast: rgba(255, 255, 255, 0.90),
    bar_top: rgba(8, 13, 22, 0.58),
    bar_bottom: rgba(4, 8, 15, 0.68),
    bar_opaque_top: hex(0x0C1218),
    bar_opaque_bottom: hex(0x060A12),
    bar_border: rgba(0, 240, 96, 0.14),
    popover_top: rgba(8, 13, 22, 0.62),
    popover_bottom: rgba(4, 8, 15, 0.74),
    popover_opaque_top: hex(0x0C1218),
    popover_opaque_bottom: hex(0x060A12),
    over: hex(0xFFFFFF),
    shadow: hex(0x000000),
    toast: hex(0x262C3C),
    on_accent: hex(0x001A0A),
};

/// Matrix's green: its hairlines, the clock's colon, the code edge.
pub const MATRIX_GREEN: Rgba = rgba(0x00, 0xF0, 0x60, 1.0);
/// The code edge's glyphs' heads, as the rain's.
pub const MATRIX_HEAD: Rgba = rgba(0xCC, 0xFF, 0xCC, 1.0);

/// The dark theme under high contrast (#423): opaque backgrounds, white
/// text, solid rims and dividers, 2 px borders.
pub const DARK_CONTRAST: Tokens = Tokens {
    window: hex(0x05070C),
    sidebar: hex(0x000000),
    text: hex(0xFFFFFF),
    text2: hex(0xF4F6FA),
    dim: hex(0xE2E6EE),
    faint: hex(0xD0D6E0),
    card: rgba(0, 0, 0, 0.0),
    card_rim: hex(0x8E98AB),
    divider: hex(0x6B7488),
    control: hex(0x000000),
    control_rim: hex(0xD0D6E0),
    menu: hex(0x0A0D16),
    switch_off: hex(0x000000),
    segmented: hex(0x000000),
    segment_chosen: hex(0x3A4255),
    window_rim: rgba(255, 255, 255, 0.14),
    high_contrast: rgba(255, 255, 255, 0.90),
    bar_top: hex(0x05070C),
    bar_bottom: hex(0x05070C),
    bar_opaque_top: hex(0x05070C),
    bar_opaque_bottom: hex(0x05070C),
    bar_border: hex(0xAEB6C6),
    popover_top: hex(0x0A0D16),
    popover_bottom: hex(0x0A0D16),
    popover_opaque_top: hex(0x0A0D16),
    popover_opaque_bottom: hex(0x0A0D16),
    over: hex(0xFFFFFF),
    shadow: hex(0x000000),
    toast: hex(0x262C3C),
    on_accent: hex(0x0B1020),
};

/// The light theme under high contrast.
pub const LIGHT_CONTRAST: Tokens = Tokens {
    window: hex(0xFFFFFF),
    sidebar: hex(0xF1F3F7),
    text: hex(0x000000),
    text2: hex(0x0B0F18),
    dim: hex(0x1C2230),
    faint: hex(0x2E3546),
    card: hex(0xFFFFFF),
    card_rim: hex(0x4A5468),
    divider: hex(0x6B7488),
    control: hex(0xFFFFFF),
    control_rim: hex(0x2E3546),
    menu: hex(0xFFFFFF),
    switch_off: hex(0xFFFFFF),
    segmented: hex(0xFFFFFF),
    segment_chosen: hex(0xDDE1EA),
    window_rim: rgba(20, 25, 38, 0.18),
    high_contrast: hex(0x000000),
    bar_top: hex(0xFFFFFF),
    bar_bottom: hex(0xFFFFFF),
    bar_opaque_top: hex(0xFFFFFF),
    bar_opaque_bottom: hex(0xFFFFFF),
    bar_border: hex(0x2E3546),
    popover_top: hex(0xFFFFFF),
    popover_bottom: hex(0xFFFFFF),
    popover_opaque_top: hex(0xFFFFFF),
    popover_opaque_bottom: hex(0xFFFFFF),
    over: hex(0x000000),
    shadow: hex(0x0B1020),
    toast: hex(0x262C3C),
    on_accent: hex(0x0B1020),
};

/// How a colour drawn for the dark theme reads under high contrast in
/// `scheme`, for the colours the programs draw that are not one of the
/// [`Tokens`]: the pairs below first (the texts, the backgrounds, the
/// states as text), then by rule. A white veil faint enough to be a fill
/// or a hover goes (high contrast has no fills: what holds the keyboard
/// has its ring); a stronger one, a rule or a lit item, is a solid
/// surface a step off the background; stronger still, a mark or a rim,
/// the cards' rim. A grey becomes a text's colour, a dark background the
/// theme's, a coloured tint goes. Other colours (the gauges, the pandas)
/// keep theirs on dark, and read as [`on_light`] says on light.
pub fn on_contrast(dark: Rgba, scheme: Scheme) -> Rgba {
    let tokens = scheme.contrast_tokens();
    let light = scheme == Scheme::Light;
    let Rgba { r, g, b, a } = dark;
    let (low, high) = (r.min(g).min(b), r.max(g).max(b));
    let grey = high - low <= 0x28;
    if !grey && a < 0.3 {
        // A state's tint behind a row: none, its word says it.
        return Rgba { a: 0.0, ..dark };
    }
    if let Some((_, on_dark, on_light)) = CONTRAST_PAIRS.iter().find(|(d, _, _)| *d == [r, g, b]) {
        let [r, g, b] = if light { *on_light } else { *on_dark };
        // The backgrounds are opaque.
        let a = if [r, g, b] == [tokens.window.r, tokens.window.g, tokens.window.b] {
            1.0
        } else {
            a
        };
        return Rgba { r, g, b, a };
    }
    let solid = |c: Rgba| Rgba { a: 1.0, ..c };
    if grey && low >= 0xC8 && a < 1.0 {
        return match a {
            a if a <= 0.065 => Rgba { a: 0.0, ..dark },
            a if a <= 0.16 => solid(if light { hex(0xE3E7EF) } else { hex(0x262C3C) }),
            _ => tokens.card_rim,
        };
    }
    if grey && high <= 0x40 && a > 0.0 {
        // A dark background, or the black of a shade or a shadow.
        if high == 0 {
            return if light {
                Rgba { a: a * 0.5, ..dark }
            } else {
                dark
            };
        }
        return Rgba { a, ..tokens.window };
    }
    if grey {
        return Rgba {
            a,
            ..if high >= 0xC8 {
                tokens.text
            } else {
                tokens.dim
            }
        };
    }
    if light { on_light(dark) } else { dark }
}

/// Colours with a value of their own under high contrast, by their dark
/// one: dark, then light.
const CONTRAST_PAIRS: &[([u8; 3], [u8; 3], [u8; 3])] = &[
    // text, text.2, dim, faint
    ([0xEE, 0xF1, 0xF6], [0xFF, 0xFF, 0xFF], [0x00, 0x00, 0x00]),
    ([0xD5, 0xDB, 0xE5], [0xF4, 0xF6, 0xFA], [0x0B, 0x0F, 0x18]),
    ([0xA9, 0xB2, 0xC2], [0xE2, 0xE6, 0xEE], [0x1C, 0x22, 0x30]),
    ([0x8C, 0x96, 0xA8], [0xD0, 0xD6, 0xE0], [0x2E, 0x35, 0x46]),
    ([0x7C, 0x86, 0x98], [0xD0, 0xD6, 0xE0], [0x2E, 0x35, 0x46]),
    ([0x6A, 0x73, 0x84], [0xD0, 0xD6, 0xE0], [0x2E, 0x35, 0x46]),
    ([0xC9, 0xD0, 0xDC], [0xF4, 0xF6, 0xFA], [0x0B, 0x0F, 0x18]),
    // the Settings' window, sidebar and menu
    ([0x14, 0x19, 0x26], [0x05, 0x07, 0x0C], [0xFF, 0xFF, 0xFF]),
    ([0x0F, 0x13, 0x1C], [0x00, 0x00, 0x00], [0xF1, 0xF3, 0xF7]),
    ([0x1F, 0x25, 0x34], [0x0A, 0x0D, 0x16], [0xFF, 0xFF, 0xFF]),
    // the bar's and popovers' backgrounds
    ([22, 27, 42], [0x05, 0x07, 0x0C], [0xFF, 0xFF, 0xFF]),
    ([14, 17, 28], [0x05, 0x07, 0x0C], [0xFF, 0xFF, 0xFF]),
    // text and marks on an accent's fill stay dark
    ([0x0B, 0x10, 0x20], [0x0B, 0x10, 0x20], [0x0B, 0x10, 0x20]),
    // the states and accents as text, at 7:1
    ([0x8F, 0xB3, 0xFF], [0xB5, 0xCC, 0xFF], [0x1C, 0x48, 0xA5]),
    ([0x7E, 0xE0, 0xA0], [0x9D, 0xF0, 0xB9], [0x15, 0x5C, 0x33]),
    ([0xF5, 0xB4, 0x4A], [0xFF, 0xD2, 0x7A], [0x6B, 0x45, 0x00]),
    ([0xF5, 0xC4, 0x6E], [0xFF, 0xD2, 0x7A], [0x6B, 0x45, 0x00]),
    ([0xF2, 0x94, 0x5B], [0xFF, 0xB4, 0x8A], [0x8A, 0x33, 0x00]),
    ([0xF7, 0xB9, 0x8A], [0xFF, 0xB4, 0x8A], [0x8A, 0x33, 0x00]),
    ([0xF7, 0xA7, 0x7A], [0xFF, 0xB4, 0x8A], [0x8A, 0x33, 0x00]),
    ([0xFF, 0xE0, 0x8A], [0xFF, 0xE0, 0x8A], [0x6B, 0x45, 0x00]),
];

/// How a colour drawn for the dark theme reads on the light one, for the
/// many colours the programs draw that are not one of the [`Tokens`]: the
/// pairs below first (each dark token, and the bar's states as text), then
/// by rule: a white at some opacity becomes the light theme's ink at a
/// little more (the light backgrounds need more to show the same edge), a
/// near-white becomes the text, a dark background a light one at the same
/// opacity, a shadow lighter. Other colours (the gauges, the pandas, the
/// states' fills) keep theirs.
pub fn on_light(dark: Rgba) -> Rgba {
    let Rgba { r, g, b, a } = dark;
    if let Some((_, light)) = PAIRS.iter().find(|(d, _)| *d == [r, g, b]) {
        return Rgba {
            r: light[0],
            g: light[1],
            b: light[2],
            a,
        };
    }
    let (low, high) = (r.min(g).min(b), r.max(g).max(b));
    let ink = LIGHT.text;
    if low >= 0xC8 && high - low <= 0x28 {
        if a < 1.0 {
            return Rgba {
                a: (a * 1.25).min(1.0),
                ..ink
            };
        }
        return ink;
    }
    if high == 0 {
        return Rgba { a: a * 0.5, ..dark };
    }
    if high <= 0x40 && high - low <= 0x20 {
        return Rgba { a, ..LIGHT.window };
    }
    dark
}

/// Colours with a value of their own on light, by their dark one.
const PAIRS: &[([u8; 3], [u8; 3])] = &[
    // text, text.2, dim, faint
    ([0xEE, 0xF1, 0xF6], [0x14, 0x19, 0x26]),
    ([0xD5, 0xDB, 0xE5], [0x2A, 0x31, 0x42]),
    ([0xA9, 0xB2, 0xC2], [0x4A, 0x54, 0x66]),
    ([0x8C, 0x96, 0xA8], [0x5C, 0x66, 0x78]),
    // the Settings' window, sidebar and menu
    ([0x14, 0x19, 0x26], [0xF5, 0xF6, 0xF9]),
    ([0x0F, 0x13, 0x1C], [0xE9, 0xEC, 0xF2]),
    ([0x1F, 0x25, 0x34], [0xFF, 0xFF, 0xFF]),
    // the bar's and popovers' backgrounds
    ([22, 27, 42], [0xF3, 0xF4, 0xF8]),
    ([14, 17, 28], [0xEE, 0xF0, 0xF5]),
    // text and marks on an accent's fill stay dark
    ([0x0B, 0x10, 0x20], [0x0B, 0x10, 0x20]),
    // the states and accents as text, darkened to read on light
    ([0x8F, 0xB3, 0xFF], [0x2F, 0x5F, 0xC4]),
    ([0x7E, 0xE0, 0xA0], [0x2E, 0x7D, 0x4A]),
    ([0xF5, 0xB4, 0x4A], [0x8F, 0x5E, 0x0A]),
    ([0xF2, 0x94, 0x5B], [0xA8, 0x4A, 0x14]),
    ([0xF7, 0xB9, 0x8A], [0xA0, 0x4C, 0x1C]),
    ([0xFF, 0xE0, 0x8A], [0x86, 0x62, 0x08]),
];

/// How a colour drawn for the dark theme reads in Givre, for the colours
/// the shell draws that are not one of the [`Tokens`]: the pairs below
/// first (each dark token), then by rule: a dark background turns night
/// blue at the same opacity, a grey text icy at the same lightness. White
/// veils, rims and hovers stay white (the mockup's); the states, the
/// gauges, the accents and the pandas keep their colours.
pub fn on_frost(dark: Rgba) -> Rgba {
    let Rgba { r, g, b, a } = dark;
    if let Some((_, frost)) = FROST_PAIRS.iter().find(|(d, _)| *d == [r, g, b]) {
        return Rgba {
            r: frost[0],
            g: frost[1],
            b: frost[2],
            a,
        };
    }
    let (low, high) = (r.min(g).min(b), r.max(g).max(b));
    let grey = high - low <= 0x28;
    if grey && high > 0 && high <= 0x40 {
        let tint = |v: u8, k: f32| (f32::from(v) * k).round() as u8;
        return Rgba {
            r: tint(r, 0.45),
            g: tint(g, 0.74),
            b: tint(b, 0.95),
            a,
        };
    }
    if grey && low >= 0x50 && !(low >= 0xC8 && a < 1.0) {
        let (_, _, lightness) = hsl([r, g, b]);
        let [r, g, b] = rgb_of(207.0, 0.38, lightness);
        return Rgba { r, g, b, a };
    }
    dark
}

/// Colours with a value of their own in Givre, by their dark one.
const FROST_PAIRS: &[([u8; 3], [u8; 3])] = &[
    // text, text.2, dim, faint
    ([0xEE, 0xF1, 0xF6], [0xEA, 0xF6, 0xFF]),
    ([0xD5, 0xDB, 0xE5], [0xC4, 0xD9, 0xEA]),
    ([0xA9, 0xB2, 0xC2], [0x93, 0xAE, 0xC8]),
    ([0x8C, 0x96, 0xA8], [0x7D, 0x98, 0xB2]),
    // the windows', sidebars' and menus' backgrounds (the lock's panels)
    ([0x14, 0x19, 0x26], [0x0E, 0x1A, 0x30]),
    ([0x0F, 0x13, 0x1C], [0x08, 0x10, 0x1F]),
    ([0x1F, 0x25, 0x34], [0x11, 0x1E, 0x36]),
    // the bar's and popovers' backgrounds
    ([22, 27, 42], [10, 20, 40]),
    ([14, 17, 28], [6, 12, 26]),
    // text and marks on an accent's fill
    ([0x0B, 0x10, 0x20], [0x04, 0x11, 0x1D]),
];

/// How a colour drawn for the dark theme reads in Matrix: as in Givre
/// ([`on_frost`]), its own values: the dark tokens their Matrix values; a
/// dark background near black with a night-blue cast; a grey text a
/// barely green grey at the same lightness; the rest as drawn.
pub fn on_matrix(dark: Rgba) -> Rgba {
    let Rgba { r, g, b, a } = dark;
    if let Some((_, matrix)) = MATRIX_PAIRS.iter().find(|(d, _)| *d == [r, g, b]) {
        return Rgba {
            r: matrix[0],
            g: matrix[1],
            b: matrix[2],
            a,
        };
    }
    let (low, high) = (r.min(g).min(b), r.max(g).max(b));
    let grey = high - low <= 0x28;
    if grey && high > 0 && high <= 0x40 {
        let tint = |v: u8, k: f32| (f32::from(v) * k).round() as u8;
        return Rgba {
            r: tint(r, 0.36),
            g: tint(g, 0.48),
            b: tint(b, 0.52),
            a,
        };
    }
    if grey && low >= 0x50 && !(low >= 0xC8 && a < 1.0) {
        let (_, _, lightness) = hsl([r, g, b]);
        let [r, g, b] = rgb_of(145.0, 0.14, lightness);
        return Rgba { r, g, b, a };
    }
    dark
}

/// Colours with a value of their own in Matrix, by their dark one.
const MATRIX_PAIRS: &[([u8; 3], [u8; 3])] = &[
    // text, text.2, dim, faint
    ([0xEE, 0xF1, 0xF6], [0xE8, 0xF1, 0xEC]),
    ([0xD5, 0xDB, 0xE5], [0xC2, 0xD2, 0xC8]),
    ([0xA9, 0xB2, 0xC2], [0x8F, 0xA7, 0x9A]),
    ([0x8C, 0x96, 0xA8], [0x7A, 0x92, 0x85]),
    // the windows', sidebars' and menus' backgrounds (the lock's panels)
    ([0x14, 0x19, 0x26], [0x0C, 0x12, 0x18]),
    ([0x0F, 0x13, 0x1C], [0x06, 0x0A, 0x12]),
    ([0x1F, 0x25, 0x34], [0x0F, 0x16, 0x1E]),
    // the bar's and popovers' backgrounds
    ([22, 27, 42], [8, 13, 22]),
    ([14, 17, 28], [4, 8, 15]),
    // text and marks on an accent's fill
    ([0x0B, 0x10, 0x20], [0x00, 0x1A, 0x0A]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Rgba) -> f64 {
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    fn contrast(a: Rgba, b: Rgba) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn the_texts_pass_4_5_to_1_on_their_backgrounds() {
        for tokens in [&DARK, &LIGHT] {
            for text in [tokens.text, tokens.text2, tokens.dim, tokens.faint] {
                for under in [tokens.window, tokens.sidebar, tokens.bar_opaque_bottom] {
                    let ratio = contrast(text, under);
                    assert!(ratio >= 4.5, "{text:?} on {under:?}: {ratio:.2}");
                }
            }
        }
        for accent in Accent::ALL {
            let c = accent.colours();
            let on = |rgb: [u8; 3]| Rgba {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                a: 1.0,
            };
            assert!(
                contrast(on(c.on_dark), DARK.window) >= 4.5,
                "{accent} on dark"
            );
            assert!(
                contrast(on(c.on_light), LIGHT.window) >= 4.5,
                "{accent} on light"
            );
        }
    }

    #[test]
    fn the_dark_tokens_read_as_the_light_ones() {
        assert_eq!(on_light(DARK.text), LIGHT.text);
        assert_eq!(on_light(DARK.dim), LIGHT.dim);
        assert_eq!(on_light(DARK.window), LIGHT.window);
        assert_eq!(on_light(DARK.menu), LIGHT.menu);
        let rim = on_light(DARK.card_rim);
        assert_eq!([rim.r, rim.g, rim.b], [0x14, 0x19, 0x26]);
        assert!((rim.a - 0.0875).abs() < 1e-6, "{rim:?}");
        // A gauge keeps its colour.
        assert_eq!(on_light(hex(0x5B8DEF)), hex(0x5B8DEF));
    }

    #[test]
    fn a_lifted_hue_reads_on_both_themes() {
        for rgb in [
            [0x1B, 0x22, 0x33],
            [0xFF, 0xE0, 0x10],
            [0x10, 0x80, 0x20],
            [0x80, 0x80, 0x80],
            [0xF0, 0x30, 0x90],
        ] {
            let lifted = AccentColours::lifted(rgb);
            let on = |rgb: [u8; 3]| Rgba {
                r: rgb[0],
                g: rgb[1],
                b: rgb[2],
                a: 1.0,
            };
            assert!(contrast(on(lifted.on_dark), DARK.window) >= 4.5, "{rgb:?}");
            assert!(
                contrast(on(lifted.on_light), LIGHT.window) >= 4.5,
                "{rgb:?}"
            );
        }
        // A slate stays blue.
        let [r, _, b] = AccentColours::lifted([0x1B, 0x22, 0x33]).base;
        assert!(b > r, "{r} {b}");
        let choice = Choice {
            scheme: Scheme::Light,
            accent: Accent::Wallpaper,
            wallpaper: AccentColours::lifted([0x1D, 0x3A, 0x2E]),
            contrast: false,
        };
        assert_eq!(choice.accent(), choice.wallpaper);
        assert_eq!(Choice::default().wallpaper, BACKDROP_ACCENT);
    }

    /// Every accent, the wallpaper's among them (a few hues of their own).
    fn every_accent(scheme: Scheme) -> Vec<Choice> {
        let mut all: Vec<Choice> = Accent::ALL
            .into_iter()
            .map(|accent| Choice {
                scheme,
                accent,
                contrast: true,
                ..Choice::default()
            })
            .collect();
        for rgb in [[0x1B, 0x22, 0x33], [0xFF, 0xE0, 0x10], [0xF0, 0x30, 0x90]] {
            all.push(Choice {
                scheme,
                accent: Accent::Wallpaper,
                wallpaper: AccentColours::lifted(rgb),
                contrast: true,
            });
        }
        all
    }

    #[test]
    fn under_high_contrast_text_reads_at_7_to_1_and_borders_at_3_to_1() {
        for scheme in Scheme::ALL {
            let tokens = scheme.contrast_tokens();
            let under = backgrounds(tokens);
            for text in [tokens.text, tokens.text2, tokens.dim, tokens.faint] {
                for back in under {
                    let ratio = contrast(text, back);
                    assert!(
                        ratio >= CONTRAST_TEXT,
                        "{scheme}: {text:?} on {back:?}: {ratio:.2}"
                    );
                }
            }
            for edge in [
                tokens.bar_border,
                tokens.card_rim,
                tokens.divider,
                tokens.control_rim,
            ] {
                for back in under {
                    let ratio = contrast(edge, back);
                    assert!(
                        ratio >= CONTRAST_BORDER,
                        "{scheme}: {edge:?} on {back:?}: {ratio:.2}"
                    );
                }
            }
            // Text on a chosen segment, and on the toast (dark in both).
            for (text, back) in [
                (tokens.text, tokens.segment_chosen),
                (DARK_CONTRAST.text, tokens.toast),
            ] {
                assert!(
                    contrast(text, back) >= CONTRAST_TEXT,
                    "{text:?} on {back:?}"
                );
            }
            // The backgrounds are opaque: no glass under high contrast.
            for back in [tokens.bar_top, tokens.bar_bottom, tokens.popover_top] {
                assert_eq!(back.a, 1.0);
            }
            // The accent as text, and the keyboard's ring, for each accent.
            for choice in every_accent(scheme) {
                let [r, g, b] = choice.accent_text();
                for back in under {
                    let ratio = contrast(hex(u32::from_be_bytes([0, r, g, b])), back);
                    assert!(
                        ratio >= CONTRAST_TEXT,
                        "{scheme} {}: {r:02x}{g:02x}{b:02x} on {back:?}: {ratio:.2}",
                        choice.accent
                    );
                }
            }
            // The states and the texts the programs draw outside the
            // tokens.
            for (dark, _, _) in CONTRAST_PAIRS {
                let read = on_contrast(
                    hex(u32::from_be_bytes([0, dark[0], dark[1], dark[2]])),
                    scheme,
                );
                let is_background = under
                    .iter()
                    .any(|b| [b.r, b.g, b.b] == [read.r, read.g, read.b])
                    || [read.r, read.g, read.b] == [0x0B, 0x10, 0x20];
                if !is_background {
                    for back in under {
                        let ratio = contrast(read, back);
                        assert!(
                            ratio >= CONTRAST_TEXT,
                            "{scheme}: {dark:02x?} → {read:?} on {back:?}: {ratio:.2}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn under_high_contrast_the_dark_colours_read_by_rule() {
        let veil = |a| rgba(255, 255, 255, a);
        for scheme in Scheme::ALL {
            let tokens = scheme.contrast_tokens();
            // A fill or a hover goes; a rim is the cards' rim.
            assert_eq!(on_contrast(veil(0.05), scheme).a, 0.0);
            assert_eq!(on_contrast(veil(0.3), scheme), tokens.card_rim);
            // The texts are the tokens'.
            assert_eq!(on_contrast(DARK.text, scheme), tokens.text);
            assert_eq!(on_contrast(DARK.faint, scheme), tokens.faint);
            // A background is the theme's, opaque.
            assert_eq!(on_contrast(rgba(22, 27, 42, 0.7), scheme), tokens.window);
            // A state's tint goes.
            assert_eq!(on_contrast(rgba(245, 180, 74, 0.08), scheme).a, 0.0);
        }
        // A gauge keeps its colour on dark.
        assert_eq!(on_contrast(hex(0x5B8DEF), Scheme::Dark), hex(0x5B8DEF));
        let choice = Choice {
            contrast: true,
            ..Choice::default()
        };
        assert_eq!(choice.tokens(), &DARK_CONTRAST);
        assert_eq!(choice.reads(DARK.dim), DARK_CONTRAST.dim);
    }

    /// `over` at `alpha` over the opaque `under`.
    fn composited(over: Rgba, under: Rgba) -> Rgba {
        let mix =
            |o: u8, u: u8| (f32::from(o) * over.a + f32::from(u) * (1.0 - over.a)).round() as u8;
        rgba(
            mix(over.r, under.r),
            mix(over.g, under.g),
            mix(over.b, under.b),
            1.0,
        )
    }

    #[test]
    fn givre_reads_at_7_to_1_on_its_opaque_fallback_and_its_rims_at_3_to_1() {
        let t = &FROST;
        let opaque = [
            t.bar_opaque_top,
            t.bar_opaque_bottom,
            t.popover_opaque_top,
            t.popover_opaque_bottom,
            t.window,
            t.sidebar,
        ];
        for under in opaque {
            for text in [t.text, t.text2, t.dim] {
                let ratio = contrast(text, under);
                assert!(ratio >= 7.0, "{text:?} on {under:?}: {ratio:.2}");
            }
            let ratio = contrast(t.faint, under);
            assert!(ratio >= 4.5, "faint on {under:?}: {ratio:.2}");
            let ice = hex(0x7FD8FF);
            assert!(contrast(ice, under) >= 7.0, "the accent on {under:?}");
            for edge in [t.bar_border, FROST_CORNER] {
                let ratio = contrast(composited(edge, under), under);
                assert!(ratio >= 3.0, "{edge:?} on {under:?}: {ratio:.2}");
            }
        }
        // On the menus, the texts at 4.5:1 at least.
        for text in [t.text, t.text2, t.dim, t.faint] {
            assert!(contrast(text, t.menu) >= 4.5, "{text:?} on the menu");
        }
        // The spec's ratios, to a tenth.
        let at = |text| (contrast(text, t.bar_opaque_top) * 10.0).round() / 10.0;
        assert_eq!([at(t.text), at(t.text2), at(t.dim)], [15.8, 12.0, 7.6]);
        assert_eq!(at(hex(0x7FD8FF)), 10.9);
        // Text on the accent's fill.
        assert!(contrast(t.on_accent, hex(0x7FD8FF)) >= 7.0);
        // The states keep their colours, and still read on it.
        for state in [0xFFC23D, 0xFF8A66, 0x7EE0A0, 0x8FB3FF] {
            assert_eq!(on_frost(hex(state)), hex(state));
            assert!(contrast(hex(state), t.bar_opaque_top) >= 4.5, "{state:06x}");
        }
        // The glass is the table's: night blue, partly see-through.
        assert_eq!(t.bar_top, rgba(10, 20, 40, 0.58));
        assert_eq!(t.bar_bottom, rgba(6, 12, 26, 0.68));
    }

    #[test]
    fn the_dark_colours_read_icy_in_givre() {
        assert_eq!(on_frost(DARK.text), FROST.text);
        assert_eq!(on_frost(DARK.dim), FROST.dim);
        assert_eq!(on_frost(DARK.faint), FROST.faint);
        assert_eq!(on_frost(rgba(22, 27, 42, 0.95)), rgba(10, 20, 40, 0.95));
        // A dark background of a surface's own turns night blue.
        let back = on_frost(rgba(26, 31, 48, 0.97));
        assert!(back.b > back.g && back.g > back.r, "{back:?}");
        assert_eq!(back.a, 0.97);
        // A grey text turns icy, as light as it was.
        let grey = on_frost(hex(0xC9D0DC));
        assert!(grey.b > grey.r, "{grey:?}");
        let (_, _, l0) = hsl([0xC9, 0xD0, 0xDC]);
        let (_, _, l1) = hsl([grey.r, grey.g, grey.b]);
        assert!((l0 - l1).abs() < 0.01, "{l0} {l1}");
        // White veils and the pandas' colours stay.
        assert_eq!(
            on_frost(rgba(255, 255, 255, 0.08)),
            rgba(255, 255, 255, 0.08)
        );
        assert_eq!(on_frost(hex(0x7BC47F)), hex(0x7BC47F));
    }

    #[test]
    fn givre_keeps_windows_dark_and_yields_to_high_contrast() {
        let frost = Choice {
            scheme: Scheme::Frost,
            accent: Accent::Ice,
            ..Choice::default()
        };
        assert_eq!(frost.tokens(), &FROST);
        let windows = frost.in_windows();
        assert_eq!(windows.scheme, Scheme::Dark);
        assert_eq!(windows.tokens(), &DARK);
        assert_eq!(windows.accent(), Accent::Ice.colours());
        assert_eq!(windows.reads(DARK.text), DARK.text);
        assert_eq!(frost.accent_text(), [0x7F, 0xD8, 0xFF]);
        // Applications are told dark.
        assert_eq!(Scheme::Frost.portal_value(), 1);
        // High contrast wins: the dark theme's high-contrast tokens.
        let contrast = Choice {
            contrast: true,
            ..frost
        };
        assert_eq!(contrast.tokens(), &DARK_CONTRAST);
        assert_eq!(contrast.reads(DARK.dim), DARK_CONTRAST.dim);
        // In the log and in the file.
        assert_eq!(frost.to_string(), "frost ice 7a96e0,a4b8f0,3a58b0");
        assert_eq!("frost".parse::<Scheme>(), Ok(Scheme::Frost));
        assert_eq!("ice".parse::<Accent>(), Ok(Accent::Ice));
        assert!("givre".parse::<Scheme>().is_err());
        assert!(Scheme::Frost.themed() && !Scheme::Dark.themed());
    }

    #[test]
    fn matrix_reads_at_7_to_1_and_keeps_its_green_for_touches() {
        let t = &MATRIX;
        for under in [
            t.bar_opaque_top,
            t.bar_opaque_bottom,
            t.popover_opaque_top,
            t.popover_opaque_bottom,
            t.window,
            t.sidebar,
        ] {
            for text in [t.text, t.text2, t.dim] {
                let ratio = contrast(text, under);
                assert!(ratio >= 7.0, "{text:?} on {under:?}: {ratio:.2}");
            }
            assert!(contrast(t.faint, under) >= 4.5, "faint on {under:?}");
            assert!(
                contrast(MATRIX_GREEN, under) >= 7.0,
                "the accent on {under:?}"
            );
        }
        for text in [t.text, t.text2, t.dim, t.faint] {
            assert!(contrast(text, t.menu) >= 4.5, "{text:?} on the menu");
        }
        let at = |text| (contrast(text, t.bar_opaque_top) * 10.0).round() / 10.0;
        assert_eq!([at(t.text), at(t.text2), at(t.dim)], [16.3, 12.0, 7.3]);
        assert!(contrast(t.on_accent, MATRIX_GREEN) >= 7.0);
        // The glass: black with a night-blue cast, never green nor brown.
        for glass in [
            t.bar_top,
            t.bar_bottom,
            t.bar_opaque_top,
            t.bar_opaque_bottom,
        ] {
            assert!(glass.b > glass.g && glass.g > glass.r, "{glass:?}");
        }
        // The dark colours read as Matrix's; the states keep theirs.
        assert_eq!(on_matrix(DARK.text), t.text);
        assert_eq!(on_matrix(DARK.faint), t.faint);
        assert_eq!(on_matrix(rgba(22, 27, 42, 0.95)), rgba(8, 13, 22, 0.95));
        for state in [0xFFC23D, 0xFF8A66, 0x7EE0A0, 0x8FB3FF] {
            assert_eq!(on_matrix(hex(state)), hex(state));
        }
        assert_eq!(
            on_matrix(rgba(255, 255, 255, 0.08)),
            rgba(255, 255, 255, 0.08)
        );
        let grey = on_matrix(hex(0xC9D0DC));
        assert!(grey.g > grey.r && grey.g > grey.b, "{grey:?}");
        // Windows dark, in its accent; high contrast wins; on the line.
        let matrix = Choice {
            scheme: Scheme::Matrix,
            accent: Accent::Phosphor,
            ..Choice::default()
        };
        assert_eq!(matrix.in_windows().tokens(), &DARK);
        assert_eq!(matrix.accent_text(), [0x00, 0xF0, 0x60]);
        assert_eq!(Scheme::Matrix.portal_value(), 1);
        assert_eq!(
            Choice {
                contrast: true,
                ..matrix
            }
            .tokens(),
            &DARK_CONTRAST
        );
        assert_eq!("matrix".parse::<Scheme>(), Ok(Scheme::Matrix));
        assert_eq!("phosphor".parse::<Accent>(), Ok(Accent::Phosphor));
        assert!(Scheme::Matrix.themed());
    }

    #[test]
    fn words_go_both_ways() {
        for scheme in Scheme::ALL {
            assert_eq!(scheme.word().parse::<Scheme>().ok(), Some(scheme));
        }
        for accent in Accent::ALL {
            assert_eq!(accent.word().parse::<Accent>().ok(), Some(accent));
        }
        assert!("pink".parse::<Accent>().is_err());
    }
}
