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
//! No toolkit here: colours are red, green, blue and an opacity, which each
//! program turns into its own.

use std::{fmt, str::FromStr};

use crate::BadLine;

/// `[appearance] theme`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Scheme {
    #[default]
    Dark,
    Light,
}

impl Scheme {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    /// Its word in the file and on the line.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.word() == word)
    }

    /// Its tokens.
    pub const fn tokens(self) -> &'static Tokens {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }

    /// Its tokens under high contrast.
    pub const fn contrast_tokens(self) -> &'static Tokens {
        match self {
            Self::Dark => &DARK_CONTRAST,
            Self::Light => &LIGHT_CONTRAST,
        }
    }

    /// `org.freedesktop.appearance` `color-scheme`: 1 prefers dark, 2
    /// prefers light (0, no preference, is never said).
    pub const fn portal_value(self) -> u32 {
        match self {
            Self::Dark => 1,
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
        Self::of(s).ok_or_else(|| BadLine(format!("no theme {s:?}: dark or light")))
    }
}

/// `[appearance] accent`: six hues chosen not to be the bar's states
/// (amber is `state.wait`, orange `state.stuck`), the pandas' green first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Accent {
    #[default]
    Green,
    Blue,
    Violet,
    Rose,
    Teal,
    /// The wallpaper's dominant hue, lifted to the others' lightness.
    Wallpaper,
}

/// An accent's three values: `base` fills the switches, the selection and
/// the check; text in the accent is `on_dark` or `on_light`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Written on the line: `RRGGBB,RRGGBB,RRGGBB`.
    fn word(self) -> String {
        let hex = |[r, g, b]: [u8; 3]| format!("{r:02x}{g:02x}{b:02x}");
        format!(
            "{},{},{}",
            hex(self.base),
            hex(self.on_dark),
            hex(self.on_light)
        )
    }

    fn of_word(word: &str) -> Option<Self> {
        let colour = |s: &str| -> Option<[u8; 3]> {
            let n = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
            Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
        };
        let mut parts = word.split(',');
        let colours = Self {
            base: colour(parts.next()?)?,
            on_dark: colour(parts.next()?)?,
            on_light: colour(parts.next()?)?,
        };
        parts.next().is_none().then_some(colours)
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
    pub const ALL: [Self; 6] = [
        Self::Green,
        Self::Blue,
        Self::Violet,
        Self::Rose,
        Self::Teal,
        Self::Wallpaper,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Rose => "rose",
            Self::Teal => "teal",
            Self::Wallpaper => "wallpaper",
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
        }
    }
}

impl fmt::Display for Choice {
    /// `SCHEME ACCENT WALLPAPER`, then `contrast` under high contrast, as
    /// `theme` on the line carries it.
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

impl FromStr for Choice {
    type Err = BadLine;

    /// `SCHEME ACCENT [WALLPAPER] [contrast]`: without the wallpaper's
    /// colours, Nuit's.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.split_whitespace();
        let scheme = words
            .next()
            .ok_or_else(|| BadLine("no theme".to_owned()))?
            .parse()?;
        let accent = words
            .next()
            .ok_or_else(|| BadLine("no accent".to_owned()))?
            .parse()?;
        let mut next = words.next();
        let wallpaper = match next {
            None | Some("contrast") => AccentColours::default(),
            Some(word) => {
                next = words.next();
                AccentColours::of_word(word)
                    .ok_or_else(|| BadLine(format!("the wallpaper's accent {word:?}")))?
            }
        };
        let contrast = match next {
            None => false,
            Some("contrast") => true,
            Some(word) => return Err(BadLine(format!("after the theme, {word:?}"))),
        };
        Ok(Self {
            scheme,
            accent,
            wallpaper,
            contrast,
        })
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
                "no accent {s:?}: green, blue, violet, rose, teal or wallpaper"
            ))
        })
    }
}

/// The theme the desktop draws in: written `theme SCHEME ACCENT
/// WALLPAPER` on the line (the wallpaper's accent's three colours,
/// [`AccentColours`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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
            (true, scheme) => on_contrast(dark, scheme),
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
            Scheme::Dark => colours.on_dark,
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
        assert_eq!(choice.to_string().parse::<Choice>(), Ok(choice));
        assert_eq!(choice.accent(), choice.wallpaper);
        assert_eq!(
            "dark green".parse::<Choice>().map(|c| c.wallpaper),
            Ok(BACKDROP_ACCENT)
        );
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

    #[test]
    fn high_contrast_rides_on_the_line() {
        for scheme in Scheme::ALL {
            for contrast in [false, true] {
                let choice = Choice {
                    scheme,
                    accent: Accent::Teal,
                    contrast,
                    ..Choice::default()
                };
                assert_eq!(choice.to_string().parse::<Choice>(), Ok(choice));
            }
        }
        assert_eq!(
            "dark green contrast".parse::<Choice>().map(|c| c.contrast),
            Ok(true)
        );
        assert!(
            "dark green 7ee0a0,7ee0a0,7ee0a0 loud"
                .parse::<Choice>()
                .is_err()
        );
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
