//! The screens: `[outputs.NAME]`, `[[screen-sets]]`.

use super::*;

/// One screen's settings, `[outputs.eDP-1]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct OutputSettings {
    /// From 0.5 to 4. None: 1 or 2 by its density
    /// (`output_spec::default_scale`).
    #[serde(deserialize_with = "scale")]
    pub scale: Option<f64>,
    /// The bar's form on this screen, over `[panel] form` (`auto` too:
    /// the Settings write each screen's, 6.4). None: `[panel] form`.
    pub panel_form: Option<FormChoice>,
    /// `mode = "2560x1600@60"`: its size in pixels and its refresh rate
    /// (Hz, whole or not). None: the mode it prefers.
    #[serde(deserialize_with = "mode")]
    pub mode: Option<ModeWanted>,
    /// `position = [x, y]`: its top-left corner, logical pixels. Used when
    /// every screen on has one and they neither overlap nor leave one
    /// apart; otherwise the screens go left to right.
    #[serde(deserialize_with = "position")]
    pub position: Option<(i32, i32)>,
    /// `enabled = false` turns it off, plugged; its windows go to another.
    pub enabled: bool,
    /// Variable refresh rate (VRR, adaptive sync), when it offers it.
    pub vrr: bool,
    /// Its own wallpaper, with `[wallpaper] per-screen` (6.3); kept when
    /// per-screen is off, for when it is on again.
    #[serde(deserialize_with = "some_picture")]
    pub wallpaper: Option<hocus_link::wallpaper::Picture>,
}

impl OutputSettings {
    /// The same, without its wallpaper: what the screens' layout takes.
    pub fn screen_part(&self) -> Self {
        Self {
            wallpaper: None,
            ..self.clone()
        }
    }
}

/// A set of screens and its own arrangement, `[[screen-sets]]`: its
/// screens by identity (windows.md §12), which is primary, and each
/// screen's place, mode, scale and VRR over its `[outputs.NAME]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenSet {
    pub screens: Vec<String>,
    /// The primary screen, by identity, over `[panel] output`.
    pub primary: Option<String>,
    /// By identity.
    pub outputs: BTreeMap<String, SetScreen>,
}

impl ScreenSet {
    /// Whether it is the set of `keys`, in any order.
    pub fn is(&self, keys: &[String]) -> bool {
        let mut mine = self.screens.clone();
        let mut theirs = keys.to_vec();
        mine.sort();
        mine.dedup();
        theirs.sort();
        theirs.dedup();
        !mine.is_empty() && mine == theirs
    }
}

/// A screen's arrangement in a set: what it says wins over
/// `[outputs.NAME]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetScreen {
    #[serde(deserialize_with = "scale")]
    pub scale: Option<f64>,
    #[serde(deserialize_with = "mode")]
    pub mode: Option<ModeWanted>,
    #[serde(deserialize_with = "position")]
    pub position: Option<(i32, i32)>,
    pub vrr: Option<bool>,
}

impl SetScreen {
    /// `base` with what this says over it.
    pub fn over(&self, base: OutputSettings) -> OutputSettings {
        OutputSettings {
            scale: self.scale.or(base.scale),
            mode: self.mode.or(base.mode),
            position: self.position.or(base.position),
            vrr: self.vrr.unwrap_or(base.vrr),
            ..base
        }
    }
}

impl Config {
    /// The file's arrangement of the set of `keys`.
    pub fn screen_set(&self, keys: &[String]) -> Option<&ScreenSet> {
        self.screen_sets.iter().find(|s| s.is(keys))
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            scale: None,
            panel_form: None,
            mode: None,
            position: None,
            enabled: true,
            vrr: false,
            wallpaper: None,
        }
    }
}

/// A mode asked for in the file: `2560x1600@60`, `1920x1080@59.94`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeWanted {
    pub width: i32,
    pub height: i32,
    /// In Hz.
    pub refresh: f64,
}

impl ModeWanted {
    /// `2560x1600@60`; the refresh rate may be left out (`2560x1600`: its
    /// highest).
    pub fn parse(text: &str) -> Option<Self> {
        let (size, refresh) = match text.trim().split_once('@') {
            Some((size, refresh)) => (size, Some(refresh)),
            None => (text.trim(), None),
        };
        let (w, h) = size.split_once('x')?;
        let width: i32 = w.parse().ok()?;
        let height: i32 = h.parse().ok()?;
        let refresh = match refresh {
            Some(r) => r
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite() && *r > 0.0)?,
            None => 0.0,
        };
        (width > 0 && height > 0).then_some(Self {
            width,
            height,
            refresh,
        })
    }

    /// Of `modes` (width, height, refresh in mHz), the one asked: that
    /// size, the refresh rate nearest the one asked within half a hertz,
    /// or its highest when none was asked.
    pub fn pick(&self, modes: &[(i32, i32, i32)]) -> Option<(i32, i32, i32)> {
        let sized = modes
            .iter()
            .copied()
            .filter(|(w, h, _)| (*w, *h) == (self.width, self.height));
        if self.refresh == 0.0 {
            return sized.max_by_key(|m| m.2);
        }
        let asked = self.refresh * 1000.0;
        sized
            .map(|m| (m, (f64::from(m.2) - asked).abs()))
            .filter(|(_, off)| *off <= 500.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m)
    }
}

impl std::fmt::Display for ModeWanted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}@{}", self.width, self.height, self.refresh)
    }
}

pub(super) fn mode<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ModeWanted>, D::Error> {
    let text = String::deserialize(deserializer)?;
    ModeWanted::parse(&text).map(Some).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "mode {text:?}: WIDTHxHEIGHT@HZ, as \"2560x1600@60\""
        ))
    })
}

pub(super) fn position<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<(i32, i32)>, D::Error> {
    let [x, y] = <[i32; 2]>::deserialize(deserializer)?;
    Ok(Some((x, y)))
}

pub(super) fn scale<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let scale = f64::deserialize(deserializer)?;
    if (0.5..=4.0).contains(&scale) {
        Ok(Some(scale))
    } else {
        Err(serde::de::Error::custom(format!(
            "scale {scale}: from 0.5 to 4"
        )))
    }
}

/// Where a screen's scale came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFrom {
    /// `[outputs.NAME] scale`.
    Config,
    /// 1 or 2 by its density, `output_spec::default_scale`; the density in
    /// whole dpi, rounded down, when its size is believed.
    Automatic { dpi: Option<u32> },
}

impl ScaleFrom {
    /// In words, after the scale in the journal, for a user looking for the
    /// name to put in the configuration.
    pub fn describe(self, name: &str) -> String {
        match self {
            Self::Config => format!("(from [outputs.{name}] in config.toml)"),
            Self::Automatic { dpi } => {
                let density = dpi.map_or_else(|| "size unknown".to_owned(), |d| format!("{d} dpi"));
                format!(
                    "(automatic: {density}) — [outputs.{name}] scale = ... in config.toml sets it"
                )
            }
        }
    }
}

impl Config {
    /// The scale of the screen on connector `name`: the configured one, or
    /// 1 or 2 by its density.
    pub fn output_scale(
        &self,
        name: &str,
        pixels: (i32, i32),
        millimetres: (i32, i32),
    ) -> (f64, ScaleFrom) {
        output_scale(self.outputs.get(name), pixels, millimetres)
    }
}

/// The scale of a screen with `settings`: theirs, or
/// 1 or 2 by its density.
pub fn output_scale(
    settings: Option<&OutputSettings>,
    pixels: (i32, i32),
    millimetres: (i32, i32),
) -> (f64, ScaleFrom) {
    match settings.and_then(|o| o.scale) {
        Some(scale) => (scale, ScaleFrom::Config),
        None => (
            crate::output_spec::default_scale(pixels, millimetres),
            ScaleFrom::Automatic {
                dpi: crate::output_spec::density(pixels, millimetres)
                    // Down: a screen shown at 191 dpi or less never takes 2 by itself.
                    .map(|dpi| dpi.floor() as u32),
            },
        ),
    }
}
