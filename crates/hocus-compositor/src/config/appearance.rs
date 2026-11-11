//! How the desktop looks: `[appearance]`, its wallpaper.

use super::*;

/// How the desktop looks and moves, `[appearance]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Appearance {
    /// Windows fade instead of moving, and do not wobble.
    pub reduced_motion: bool,
    /// The theme reinforced (#423): the shell, the Settings, the lock and
    /// the authentication dialog draw its high-contrast tokens, with no
    /// glass (it implies `reduce-transparency`); the focused window's rim
    /// is 2 px wide instead of 1; applications are told (`contrast`).
    pub high_contrast: bool,
    /// The Xcursor theme; else `XCURSOR_THEME`'s, else `default`.
    pub cursor_theme: Option<String>,
    /// The cursor's size in logical pixels; else `XCURSOR_SIZE`'s, else 24.
    #[serde(deserialize_with = "cursor_size")]
    pub cursor_size: Option<u32>,
    /// The bar is opaque instead of glass over what is behind it.
    pub reduce_transparency: bool,
    /// `"dark"` or `"light"`: the shell, the Settings, the lock and the
    /// authentication dialog draw in it, and applications are told
    /// (#402); `"frost"` (Givre, #740) or `"matrix"` (#431): the shell's
    /// surfaces only, the Settings and applications dark.
    #[serde(deserialize_with = "scheme")]
    pub theme: hocus_link::theme::Scheme,
    /// `"green"`, `"blue"`, `"violet"`, `"rose"`, `"teal"`, `"wallpaper"`
    /// `"ice"` (Givre's) or `"phosphor"` (Matrix's): what is chosen, on or
    /// focused.
    #[serde(deserialize_with = "accent")]
    pub accent: hocus_link::theme::Accent,
    /// The Settings' text, from 80 to 150 %: written as `1.2`, held in
    /// hundredths.
    #[serde(deserialize_with = "text_scale")]
    pub text_scale: TextScale,
}

/// The desktop's background, `[wallpaper]` (6.3, docs/design/settings.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Wallpaper {
    /// `"night"`, `"mist"`, `"dawn"`, `"forest"`, `"rain"`, `"stars"`,
    /// `"plain"`, or a picture's path: every screen's, but those that have their own.
    #[serde(deserialize_with = "picture")]
    pub picture: hocus_link::wallpaper::Picture,
    /// Each screen its own: `[outputs.NAME] wallpaper`, else `picture`.
    pub per_screen: bool,
    /// How a picture of one's own fills its screen.
    #[serde(deserialize_with = "fit")]
    pub fit: hocus_link::wallpaper::Fit,
    /// `"plain"`'s colour, and around a picture not covering: `#RRGGBB`.
    #[serde(deserialize_with = "colour")]
    pub color: [u8; 3],
    /// The lock screen: `"dimmed"`, `"blurred"` or `"picture"`.
    #[serde(deserialize_with = "lock_look")]
    pub lock: hocus_link::wallpaper::LockLook,
    /// The lock's own picture, with `lock = "picture"`.
    #[serde(deserialize_with = "picture")]
    pub lock_picture: hocus_link::wallpaper::Picture,
    /// A wallpaper that moves ("rain", "stars", "aurora") moves; else its still
    /// picture (#433).
    pub animate: bool,
    /// On battery a moving wallpaper goes at half its frames, stays
    /// still, or goes as on mains power.
    pub on_battery: OnBattery,
}

/// `[wallpaper] on-battery` (#433).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnBattery {
    /// Half its frames, so half its pace.
    #[default]
    Half,
    /// Its still picture.
    Still,
    /// As on mains power.
    Full,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self {
            picture: hocus_link::wallpaper::Picture::default(),
            per_screen: false,
            fit: hocus_link::wallpaper::Fit::default(),
            color: hocus_link::wallpaper::DEFAULT_COLOR,
            lock: hocus_link::wallpaper::LockLook::default(),
            lock_picture: hocus_link::wallpaper::Picture::default(),
            animate: true,
            on_battery: OnBattery::Half,
        }
    }
}

pub(super) fn picture<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::wallpaper::Picture, D::Error> {
    hocus_link::wallpaper::Picture::of(&String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

pub(super) fn some_picture<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<hocus_link::wallpaper::Picture>, D::Error> {
    picture(deserializer).map(Some)
}

pub(super) fn fit<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::wallpaper::Fit, D::Error> {
    let word = String::deserialize(deserializer)?;
    hocus_link::wallpaper::Fit::of(&word).ok_or_else(|| {
        serde::de::Error::custom(format!("no fit {word:?}: cover, contain or center"))
    })
}

pub(super) fn colour<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 3], D::Error> {
    let text = String::deserialize(deserializer)?;
    hocus_link::wallpaper::colour_of(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("no colour {text:?}: #RRGGBB")))
}

pub(super) fn lock_look<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::wallpaper::LockLook, D::Error> {
    let word = String::deserialize(deserializer)?;
    hocus_link::wallpaper::LockLook::of(&word).ok_or_else(|| {
        serde::de::Error::custom(format!("no lock {word:?}: dimmed, blurred or picture"))
    })
}

/// `[appearance] text-scale`, in hundredths: 100 by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextScale(pub u32);

impl Default for TextScale {
    fn default() -> Self {
        Self(100)
    }
}

pub(super) fn text_scale<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<TextScale, D::Error> {
    let scale = f64::deserialize(deserializer)?;
    let hundredths = (scale * 100.0).round();
    if (80.0..=150.0).contains(&hundredths) {
        Ok(TextScale(hundredths as u32))
    } else {
        Err(serde::de::Error::custom(format!(
            "text scale {scale}: from 0.8 to 1.5"
        )))
    }
}

/// `[locale]`: its three keys, each a word.
pub(super) fn locale<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::locale::Locale, D::Error> {
    use hocus_link::locale::{DateFormat, Language, Locale, TimeFormat};
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Words {
        language: Option<String>,
        date: Option<String>,
        time: Option<String>,
    }
    let words = Words::deserialize(deserializer)?;
    let bad = |key: &str, word: &str, choices: &str| {
        serde::de::Error::custom(format!("[locale] {key} = {word:?}: {choices}"))
    };
    Ok(Locale {
        language: match words.language.as_deref() {
            None => Language::System,
            Some(word) => {
                Language::of(word).ok_or_else(|| bad("language", word, "system, fr or en"))?
            }
        },
        date: match words.date.as_deref() {
            None => DateFormat::Long,
            Some(word) => DateFormat::of(word)
                .ok_or_else(|| bad("date", word, "long, short, numeric or iso"))?,
        },
        time: match words.time.as_deref() {
            None => None,
            Some(word) => {
                Some(TimeFormat::of(word).ok_or_else(|| bad("time", word, "24h or 12h"))?)
            }
        },
    })
}

impl Appearance {
    /// The theme the shell draws in, `wallpaper` the wallpaper's accent
    /// (only kept while the accent is the wallpaper's: another accent does
    /// not change with the wallpaper).
    pub fn theme_choice(
        &self,
        wallpaper: hocus_link::theme::AccentColours,
    ) -> hocus_link::theme::Choice {
        hocus_link::theme::Choice {
            scheme: self.theme,
            accent: self.accent,
            wallpaper: if self.accent == hocus_link::theme::Accent::Wallpaper {
                wallpaper
            } else {
                hocus_link::theme::AccentColours::default()
            },
            contrast: self.high_contrast,
        }
    }
}

pub(super) fn scheme<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::theme::Scheme, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(|err: hocus_link::BadLine| serde::de::Error::custom(err.0))
}

pub(super) fn accent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::theme::Accent, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(|err: hocus_link::BadLine| serde::de::Error::custom(err.0))
}

pub(super) fn cursor_size<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    let size = u32::deserialize(deserializer)?;
    let sizes = crate::cursor::theme::SIZES;
    if sizes.contains(&size) {
        Ok(Some(size))
    } else {
        Err(serde::de::Error::custom(format!(
            "cursor size {size}: from {} to {}",
            sizes.start(),
            sizes.end()
        )))
    }
}
