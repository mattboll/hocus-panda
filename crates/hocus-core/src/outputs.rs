//! What each screen is set to: its mode, scale, rotation, place, and whether
//! it is on. `[[outputs]]` in the configuration holds what
//! the user chose; third-party tools (wlr-randr, kanshi) change the same for
//! the running session through wlr-output-management.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// One screen's settings, by connector name; what is not set keeps the
/// screen's own (its preferred mode, scale 1, no rotation, to the right of
/// the others).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputConfig {
    /// `eDP-1`, `DP-2`, `HEADLESS-1`…
    pub name: String,
    /// Off: the screen shows nothing and its desktops wait for it.
    pub enabled: Option<bool>,
    /// `2560x1600`, or `2560x1600@59.95` (Hz).
    pub mode: Option<String>,
    /// 0.5 to 4, in steps of 1/120.
    pub scale: Option<f64>,
    pub transform: Option<Transform>,
    /// Logical position of its top-left corner.
    pub position: Option<[i32; 2]>,
    /// Adaptive sync, where the screen and the GPU support it.
    pub vrr: Option<bool>,
    /// Where unplugged screens' desktops and windows go.
    pub primary: Option<bool>,
    /// Desktops of its own, 1 to 8; `desktops.per_output` otherwise.
    pub desktops: Option<u32>,
    /// Its desktops' names, in order (`["Mail", "Code"]`); one left out or
    /// empty is called by its number.
    pub desktop_names: Vec<String>,
    /// Shows this other screen's picture, scaled to fit, instead of
    /// desktops of its own, which wait for it (#125): a projector showing
    /// the laptop.
    pub mirror: Option<String>,
}

/// A set of screens and how they are placed: in effect when exactly these
/// screens are connected. The settings app writes one when screens are
/// dragged into place, named after them until renamed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    pub name: String,
    pub outputs: Vec<String>,
    /// Logical position of each screen's top-left corner.
    pub positions: BTreeMap<String, [i32; 2]>,
}

impl Layout {
    /// Whether these are exactly its screens, in any order.
    pub fn is_for(&self, connected: &[&str]) -> bool {
        let mut mine: Vec<&str> = self.outputs.iter().map(String::as_str).collect();
        let mut theirs = connected.to_vec();
        mine.sort_unstable();
        mine.dedup();
        theirs.sort_unstable();
        theirs.dedup();
        mine == theirs
    }

    pub fn check(&self) -> Result<(), String> {
        if self.outputs.is_empty() {
            return Err("outputs: expected the names of its screens".into());
        }
        if let Some(stray) = self.positions.keys().find(|k| !self.outputs.contains(k)) {
            return Err(format!("positions.{stray}: not one of its outputs"));
        }
        Ok(())
    }
}

/// How a screen is turned, clockwise, and whether it is flipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Transform {
    #[default]
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "90")]
    Rotate90,
    #[serde(rename = "180")]
    Rotate180,
    #[serde(rename = "270")]
    Rotate270,
    #[serde(rename = "flipped")]
    Flipped,
    #[serde(rename = "flipped-90")]
    Flipped90,
    #[serde(rename = "flipped-180")]
    Flipped180,
    #[serde(rename = "flipped-270")]
    Flipped270,
}

impl Transform {
    /// Turned a quarter: width and height swap.
    pub fn swaps(self) -> bool {
        matches!(
            self,
            Self::Rotate90 | Self::Rotate270 | Self::Flipped90 | Self::Flipped270
        )
    }
}

/// A mode asked for: a size in pixels, and a refresh rate or any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeSpec {
    pub width: i32,
    pub height: i32,
    /// mHz.
    pub refresh: Option<i32>,
}

impl FromStr for ModeSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("{s:?}: expected WIDTHxHEIGHT or WIDTHxHEIGHT@HZ");
        let (size, refresh) = match s.split_once('@') {
            Some((size, hz)) => {
                let hz: f64 = hz.trim_end_matches("Hz").parse().map_err(|_| bad())?;
                if !(1.0..=1000.0).contains(&hz) {
                    return Err(bad());
                }
                (size, Some((hz * 1000.0).round() as i32))
            }
            None => (s, None),
        };
        let (w, h) = size.split_once('x').ok_or_else(bad)?;
        let width: i32 = w.parse().map_err(|_| bad())?;
        let height: i32 = h.parse().map_err(|_| bad())?;
        if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
            return Err(bad());
        }
        Ok(Self {
            width,
            height,
            refresh,
        })
    }
}

impl fmt::Display for ModeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)?;
        if let Some(mhz) = self.refresh {
            write!(f, "@{}", f64::from(mhz) / 1000.0)?;
        }
        Ok(())
    }
}

/// A mode a screen offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    /// mHz.
    pub refresh: i32,
}

impl ModeSpec {
    /// The offered mode closest to this one: the same size, the nearest
    /// refresh rate (the highest when none is asked); `None` when no mode
    /// has that size.
    pub fn pick(&self, modes: &[Mode]) -> Option<usize> {
        modes
            .iter()
            .enumerate()
            .filter(|(_, m)| m.width == self.width && m.height == self.height)
            .min_by_key(|(_, m)| match self.refresh {
                Some(r) => (m.refresh - r).abs(),
                None => -m.refresh,
            })
            .map(|(i, _)| i)
    }
}

/// Scales go to clients in 120ths (fractional-scale-v1): a scale is set to
/// the nearest one, so that what clients draw lands on whole pixels.
pub fn round_scale(scale: f64) -> f64 {
    (scale * 120.0).round() / 120.0
}

impl OutputConfig {
    /// What the file says is wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("name: expected the output's name, as `hocus msg outputs` lists".into());
        }
        if self.mirror.as_deref() == Some(self.name.as_str()) {
            return Err(format!(
                "mirror = {:?}: a screen cannot show itself",
                self.name
            ));
        }
        if let Some(mode) = &self.mode {
            mode.parse::<ModeSpec>()
                .map_err(|e| format!("mode = {e}"))?;
        }
        if let Some(scale) = self.scale
            && !(0.5..=4.0).contains(&scale)
        {
            return Err(format!("scale = {scale}: expected 0.5 to 4"));
        }
        if let Some(n) = self.desktops
            && !(1..=8).contains(&n)
        {
            return Err(format!("desktops = {n}: expected 1 to 8"));
        }
        if self.desktop_names.len() > 8 {
            return Err(format!(
                "desktop_names: {} names, expected at most 8",
                self.desktop_names.len()
            ));
        }
        if let Some(long) = self.desktop_names.iter().find(|n| n.chars().count() > 32) {
            return Err(format!(
                "desktop_names: {long:?} is longer than 32 characters"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: i32, height: i32, hz: i32) -> Mode {
        Mode {
            width,
            height,
            refresh: hz * 1000,
        }
    }

    #[test]
    fn modes_are_read_with_or_without_a_rate() {
        assert_eq!(
            "2560x1600".parse(),
            Ok(ModeSpec {
                width: 2560,
                height: 1600,
                refresh: None
            })
        );
        assert_eq!(
            "1920x1080@59.94".parse::<ModeSpec>().map(|m| m.refresh),
            Ok(Some(59_940))
        );
        assert!("1920*1080".parse::<ModeSpec>().is_err());
        assert!("0x1080".parse::<ModeSpec>().is_err());
        assert!("1920x1080@0".parse::<ModeSpec>().is_err());
    }

    #[test]
    fn the_nearest_rate_of_the_size_asked_is_picked() {
        let modes = [
            mode(2560, 1440, 60),
            mode(2560, 1440, 144),
            mode(1920, 1080, 60),
        ];
        let spec = |s: &str| s.parse::<ModeSpec>().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(spec("2560x1440@120").pick(&modes), Some(1));
        assert_eq!(spec("2560x1440").pick(&modes), Some(1));
        assert_eq!(spec("1920x1080@75").pick(&modes), Some(2));
        assert_eq!(spec("1280x720").pick(&modes), None);
    }

    #[test]
    fn a_layout_is_for_exactly_its_screens() {
        let layout = Layout {
            name: "Home".into(),
            outputs: vec!["eDP-1".into(), "DP-1".into()],
            positions: BTreeMap::from([("DP-1".into(), [0, 0]), ("eDP-1".into(), [0, 1440])]),
        };
        assert!(layout.is_for(&["DP-1", "eDP-1"]));
        assert!(!layout.is_for(&["eDP-1"]));
        assert!(!layout.is_for(&["eDP-1", "DP-1", "HDMI-A-1"]));
        assert!(layout.check().is_ok());
        let stray = Layout {
            positions: BTreeMap::from([("HDMI-A-1".into(), [0, 0])]),
            ..layout
        };
        assert!(stray.check().is_err_and(|e| e.contains("HDMI-A-1")));
    }

    #[test]
    fn scales_land_on_120ths() {
        assert_eq!(round_scale(1.5), 1.5);
        assert_eq!(round_scale(1.333), 160.0 / 120.0);
    }

    #[test]
    fn a_wrong_setting_says_which() {
        let config = |f: fn(&mut OutputConfig)| {
            let mut c = OutputConfig {
                name: "DP-1".into(),
                ..Default::default()
            };
            f(&mut c);
            c.check()
        };
        assert!(config(|_| {}).is_ok());
        assert!(
            config(|c| c.scale = Some(5.0))
                .err()
                .is_some_and(|e| e.contains("scale"))
        );
        assert!(
            config(|c| c.mode = Some("big".into()))
                .err()
                .is_some_and(|e| e.contains("mode"))
        );
        assert!(config(|c| c.name.clear()).is_err_and(|e| e.contains("name")));
    }

    #[test]
    fn transforms_read_as_the_settings_write_them() {
        #[derive(Deserialize)]
        struct T {
            t: Transform,
        }
        let t = |s: &str| toml::from_str::<T>(&format!("t = \"{s}\"")).map(|t| t.t);
        assert_eq!(t("90").ok(), Some(Transform::Rotate90));
        assert_eq!(t("flipped-270").ok(), Some(Transform::Flipped270));
        assert!(Transform::Rotate270.swaps() && !Transform::Flipped.swaps());
    }
}
