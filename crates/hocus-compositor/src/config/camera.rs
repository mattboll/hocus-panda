//! The camera, `[camera]` (#619, docs/design/camera.md): the screen light
//! while an application uses the camera, and the Hocuspanda camera's
//! effects. Followed live.

use serde::Deserialize;

/// What the desktop does while the camera is in use. Keys it does not know
/// are let through: the section will hold the later lots' effects.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Camera {
    /// The edges of the call's screen glow while the camera is in use.
    pub screen_light: bool,
    pub screen_light_warmth: Warmth,
    pub screen_light_strength: Strength,
    /// A frame along the edges, or the whole screen around the call.
    pub screen_light_shape: LightShape,
    /// Lumière: the Hocuspanda camera's exposure, white balance, warmth
    /// and denoise.
    pub lighting: bool,
    /// Bandeau: the user's name across the bottom of its picture.
    pub banner: bool,
    /// The name it says; empty: the account's.
    pub banner_name: String,
    /// Under it, the current project desktop's name, when it is one.
    pub banner_project: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            screen_light: true,
            screen_light_warmth: Warmth::Neutral,
            screen_light_strength: Strength::Auto,
            screen_light_shape: LightShape::Frame,
            lighting: true,
            banner: false,
            banner_name: String::new(),
            banner_project: true,
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

/// Where the screen light is: a wide frame along the screen's edges, or
/// all of the call's screen but the call's window (the maintainer's
/// choice of 2026-11-18: a laptop's screen is a weak light, it takes far
/// more of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LightShape {
    #[default]
    Frame,
    AroundCall,
}

impl LightShape {
    /// Its word in the file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::AroundCall => "around-call",
        }
    }
}

/// How far the screen light reaches in, and how bright it is: fixed, or
/// following the light on the face while the Hocuspanda camera is read
/// (`auto`; soft otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    #[default]
    Auto,
    Soft,
    Medium,
    Strong,
}

/// The frame's width, a part of the screen's shorter side, and its alpha
/// at the edge, soft and strong: `auto` goes between them. Strong is an
/// eighth of the side (135 px on a 1080 p screen), its light fading out
/// by half as much again inward.
const SOFT: (f64, f64) = (1.0 / 16.0, 0.85);
const MEDIUM: (f64, f64) = (1.0 / 11.0, 0.92);
const STRONG: (f64, f64) = (1.0 / 8.0, 0.97);

/// The steps `auto` goes through from soft to strong.
pub const AUTO_STEPS: u8 = 8;

impl Strength {
    /// The frame's width, a part of the screen's shorter side, and its
    /// alpha at the edge (around the call, everywhere); `auto` as soft.
    pub const fn glow(self) -> (f64, f64) {
        match self {
            Self::Auto | Self::Soft => SOFT,
            Self::Medium => MEDIUM,
            Self::Strong => STRONG,
        }
    }

    /// `auto`'s glow at step `level` of [`AUTO_STEPS`], soft to strong.
    pub fn auto_glow(level: u8) -> (f64, f64) {
        let t = f64::from(level.min(AUTO_STEPS)) / f64::from(AUTO_STEPS);
        (
            SOFT.0 + (STRONG.0 - SOFT.0) * t,
            SOFT.1 + (STRONG.1 - SOFT.1) * t,
        )
    }

    /// Its word in the file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Soft => "soft",
            Self::Medium => "medium",
            Self::Strong => "strong",
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
             screen-light-strength = \"strong\"\nscreen-light-shape = \"around-call\"\n\
             background = \"blur\"\n\
             lighting = false\nbanner = true\nbanner-name = \"Alex\"\n",
        )?;
        assert_eq!(
            camera,
            Camera {
                screen_light: false,
                screen_light_warmth: Warmth::Warm,
                screen_light_strength: Strength::Strong,
                screen_light_shape: LightShape::AroundCall,
                lighting: false,
                banner: true,
                banner_name: "Alex".to_owned(),
                banner_project: true,
            }
        );
        assert!(toml::from_str::<Camera>("screen-light-warmth = \"hot\"\n").is_err());
        assert!(toml::from_str::<Camera>("screen-light-strength = 3\n").is_err());
        assert!(toml::from_str::<Camera>("screen-light-shape = \"ring\"\n").is_err());
        Ok(())
    }

    #[test]
    fn auto_goes_from_soft_to_strong() {
        assert_eq!(Strength::auto_glow(0), Strength::Soft.glow());
        assert_eq!(Strength::auto_glow(AUTO_STEPS), Strength::Strong.glow());
        let (w, a) = Strength::auto_glow(AUTO_STEPS / 2);
        assert!((w - 0.093_75).abs() < 1e-9 && (a - 0.91).abs() < 1e-9);
    }
}
