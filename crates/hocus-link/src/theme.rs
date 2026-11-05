//! The desktop's theme (bar.md §13, issue #402): one table of tokens with a
//! dark and a light value each, and the accent, as `[appearance] theme` and
//! `accent` choose them. The compositor reads the file and tells the shell
//! ([`crate::ToShell::Theme`]) and the applications (the Settings portal);
//! the Settings read the same keys from the file themselves. Both programs
//! draw with these values, so that the bar, its popovers, the lock, the
//! authentication dialog and the Settings window change together.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentColours {
    pub base: [u8; 3],
    pub on_dark: [u8; 3],
    pub on_light: [u8; 3],
}

/// The default backdrop's hue lifted to the accents' lightness (the
/// gradient of [`crate::backdrop`]). A wallpaper of the user's own gives
/// its own when the wallpaper's page (6.3) lands.
pub const BACKDROP_ACCENT: AccentColours = AccentColours {
    base: [0x7A, 0x96, 0xE0],
    on_dark: [0xA4, 0xB8, 0xF0],
    on_light: [0x3A, 0x58, 0xB0],
};

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

/// The theme the desktop draws in: written `theme SCHEME ACCENT` on the
/// line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Choice {
    pub scheme: Scheme,
    pub accent: Accent,
}

impl Choice {
    pub const fn tokens(self) -> &'static Tokens {
        self.scheme.tokens()
    }

    pub const fn accent(self) -> AccentColours {
        self.accent.colours()
    }

    /// Text in the accent, on this theme's backgrounds.
    pub const fn accent_text(self) -> [u8; 3] {
        let colours = self.accent.colours();
        match self.scheme {
            Scheme::Dark => colours.on_dark,
            Scheme::Light => colours.on_light,
        }
    }
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
