//! The mice and touchpads: `[pointer]`, `[touchpad]`.

use super::*;

/// `[pointer]`: every mouse, trackball and trackpoint (6.12, the
/// Settings' "Souris et pavé tactile"), given to each as it is plugged in
/// and again when the file changes. A key not known refuses the file, as
/// elsewhere; a value that does not read is logged and its default
/// applies, the rest of the file kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// From -1 (slow) to 1 (fast); libinput's.
    pub speed: f64,
    pub acceleration: Acceleration,
    pub natural_scroll: bool,
    /// The right button is the main one.
    pub left_handed: bool,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            speed: 0.0,
            acceleration: Acceleration::Adaptive,
            natural_scroll: false,
            left_handed: false,
        }
    }
}

/// `[touchpad]`: every touchpad (a device that taps), as `[pointer]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touchpad {
    pub speed: f64,
    /// On, where libinput's own default is off (the maintainer's choice).
    pub tap_to_click: bool,
    pub tap_and_drag: bool,
    /// `None`: the touchpad's own (libinput's default for it).
    pub right_click: Option<RightClick>,
    pub disable_while_typing: bool,
    pub natural_scroll: bool,
    pub scroll: ScrollWay,
    /// Off while an external mouse is plugged in.
    pub off_with_mouse: bool,
    /// Three fingers sideways change desktop (windows.md §12).
    pub swipe_desktops: bool,
    /// Four fingers up open the launcher, down close it (6.13); down with
    /// no launcher open show the overview (8.5 A).
    pub swipe_launcher: bool,
}

impl Default for Touchpad {
    fn default() -> Self {
        Self {
            speed: 0.0,
            tap_to_click: true,
            tap_and_drag: true,
            right_click: None,
            disable_while_typing: true,
            natural_scroll: false,
            scroll: ScrollWay::TwoFingers,
            off_with_mouse: false,
            swipe_desktops: true,
            swipe_launcher: true,
        }
    }
}

pub use hocus_link::input::{Acceleration, RightClick, ScrollWay};

/// A speed from -1 to 1, written `0.5` or `1`.
pub(super) fn speed_of(value: &toml::Value) -> Option<f64> {
    let speed = value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))?;
    (-1.0..=1.0).contains(&speed).then_some(speed)
}

/// `[pointer]` or `[touchpad]` read key by key: `read` takes a key and its
/// value, and says `None` when the key is not known, `Some(false)` when
/// its value does not read (its default then applies).
pub(super) fn device_table<'de, D: Deserializer<'de>>(
    deserializer: D,
    table: &str,
    mut read: impl FnMut(&str, &toml::Value) -> Option<bool>,
) -> Result<(), D::Error> {
    let keys = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    for (key, value) in &keys {
        match read(key, value) {
            None => {
                return Err(serde::de::Error::custom(format!(
                    "unknown key `{key}` in [{table}]"
                )));
            }
            Some(false) => tracing::warn!(
                key,
                %value,
                "[{table}]: a value that does not read; its default applies"
            ),
            Some(true) => {}
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for Pointer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        device_table(deserializer, "pointer", |key, value| {
            Some(match key {
                "speed" => speed_of(value).map(|v| out.speed = v).is_some(),
                "acceleration" => value
                    .as_str()
                    .and_then(Acceleration::of)
                    .map(|v| out.acceleration = v)
                    .is_some(),
                "natural-scroll" => value.as_bool().map(|v| out.natural_scroll = v).is_some(),
                "left-handed" => value.as_bool().map(|v| out.left_handed = v).is_some(),
                _ => return None,
            })
        })?;
        Ok(out)
    }
}

impl<'de> Deserialize<'de> for Touchpad {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        device_table(deserializer, "touchpad", |key, value| {
            let flag = |value: &toml::Value, to: &mut bool| value.as_bool().map(|v| *to = v);
            Some(match key {
                "speed" => speed_of(value).map(|v| out.speed = v).is_some(),
                "tap-to-click" => flag(value, &mut out.tap_to_click).is_some(),
                "tap-and-drag" => flag(value, &mut out.tap_and_drag).is_some(),
                "right-click" => value
                    .as_str()
                    .and_then(RightClick::of)
                    .map(|v| out.right_click = Some(v))
                    .is_some(),
                "disable-while-typing" => flag(value, &mut out.disable_while_typing).is_some(),
                "natural-scroll" => flag(value, &mut out.natural_scroll).is_some(),
                "scroll" => value
                    .as_str()
                    .and_then(ScrollWay::of)
                    .map(|v| out.scroll = v)
                    .is_some(),
                "off-with-mouse" => flag(value, &mut out.off_with_mouse).is_some(),
                "swipe-desktops" => flag(value, &mut out.swipe_desktops).is_some(),
                "swipe-launcher" => flag(value, &mut out.swipe_launcher).is_some(),
                _ => return None,
            })
        })?;
        Ok(out)
    }
}
