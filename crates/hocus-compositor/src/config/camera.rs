//! The camera, `[camera]` (#619, docs/design/camera.md): the screen light
//! while an application uses the camera. Followed live.

use serde::Deserialize;

/// What the desktop does while the camera is in use. Keys it does not know
/// are let through: the section will hold the Hocuspanda camera's own
/// (#619's later lots), which other programs read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Camera {
    /// The edges of the call's screen glow while the camera is in use.
    pub screen_light: bool,
    pub screen_light_warmth: Warmth,
    pub screen_light_strength: Strength,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            screen_light: true,
            screen_light_warmth: Warmth::Neutral,
            screen_light_strength: Strength::Soft,
        }
    }
}

/// The screen light's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Warmth {
    Cool,
    #[default]
    Neutral,
    Warm,
}

impl Warmth {
    /// Its colour, as the mockup's (`rgb()`).
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Cool => [230, 240, 255],
            Self::Neutral => [255, 236, 214],
            Self::Warm => [255, 214, 170],
        }
    }
}

/// How far the screen light reaches in, and how bright it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    #[default]
    Soft,
    Medium,
    Strong,
}

impl Strength {
    /// The glow's width (logical pixels) and its alpha, as the mockup's
    /// `box-shadow`s take them.
    pub const fn glow(self) -> (f64, f64) {
        match self {
            Self::Soft => (28.0, 0.55),
            Self::Medium => (46.0, 0.75),
            Self::Strong => (70.0, 0.92),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_read_and_others_are_refused() -> anyhow::Result<()> {
        let camera: Camera = toml::from_str(
            "screen-light = false\nscreen-light-warmth = \"warm\"\n\
             screen-light-strength = \"strong\"\nbackground = \"blur\"\n",
        )?;
        assert_eq!(
            camera,
            Camera {
                screen_light: false,
                screen_light_warmth: Warmth::Warm,
                screen_light_strength: Strength::Strong,
            }
        );
        assert!(toml::from_str::<Camera>("screen-light-warmth = \"hot\"\n").is_err());
        assert!(toml::from_str::<Camera>("screen-light-strength = 3\n").is_err());
        Ok(())
    }
}
