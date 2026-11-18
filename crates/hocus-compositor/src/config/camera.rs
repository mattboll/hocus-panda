//! The camera, `[camera]` (#619, docs/design/camera.md): the screen light
//! while an application uses the camera, and the Hocuspanda camera's
//! effects: Lumière and the banner (lot 2), what is behind the person,
//! « Je reviens » and the presenter (lot 3), an accessory on the head,
//! Cadrage, the gestures and the confetti (lot 4). Followed live.

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
    /// Lumière: the Hocuspanda camera's exposure, white balance, warmth
    /// and denoise.
    pub lighting: bool,
    /// Bandeau: the user's name across the bottom of its picture.
    pub banner: bool,
    /// The name it says; empty: the account's.
    pub banner_name: String,
    /// Under it, the current project desktop's name, when it is one.
    pub banner_project: bool,
    /// Fond: what is behind the person (lot 3).
    pub background: Background,
    /// « Je reviens »: nobody in the picture for 10 s, or the screen
    /// locked during a call, a card on a living wallpaper; the microphone
    /// muted while locked.
    pub away: bool,
    /// Présentateur: the person cut out over a shared screen's stream.
    pub presenter: bool,
    pub presenter_side: Side,
    pub presenter_size: Size,
    /// Visage: an accessory following the head (lot 4).
    pub face: Face,
    /// Cadrage: the picture cropped to follow the face.
    pub framing: bool,
    /// Gestes: a thumb up's reaction, « main levée ».
    pub gestures: bool,
    /// Fêtes: confetti in the picture on the agents' moments.
    pub celebrations: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            screen_light: true,
            screen_light_warmth: Warmth::Neutral,
            screen_light_strength: Strength::Auto,
            lighting: true,
            banner: false,
            banner_name: String::new(),
            banner_project: true,
            background: Background::Room,
            away: true,
            presenter: false,
            presenter_side: Side::Right,
            presenter_size: Size::Medium,
            face: Face::None,
            framing: false,
            gestures: true,
            celebrations: false,
        }
    }
}

/// Visage's accessory on the head (lot 4), or none. The panda that takes
/// the person's place is to come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    None,
    Ears,
    Beanie,
    Crown,
    Glasses,
}

impl Face {
    /// Its word in the file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ears => "ears",
            Self::Beanie => "beanie",
            Self::Crown => "crown",
            Self::Glasses => "glasses",
        }
    }

    /// What the camera is told.
    pub const fn wear(self) -> Option<hocus_link::camera::Wear> {
        use hocus_link::camera::Wear;
        match self {
            Self::None => None,
            Self::Ears => Some(Wear::Ears),
            Self::Beanie => Some(Wear::Beanie),
            Self::Crown => Some(Wear::Crown),
            Self::Glasses => Some(Wear::Glasses),
        }
    }
}

/// What is behind the person in the Hocuspanda camera's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    /// Their room, as it is.
    #[default]
    Room,
    /// Their room blurred.
    Blur,
    /// The desktop's living wallpapers.
    Aurora,
    Koi,
}

impl Background {
    /// Its word in the file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Room => "room",
            Self::Blur => "blur",
            Self::Aurora => "aurora",
            Self::Koi => "koi",
        }
    }

    /// What the camera is told.
    pub fn backdrop(self) -> hocus_link::camera::Backdrop {
        use hocus_link::camera::{Backdrop, Scene};
        match self {
            Self::Room => Backdrop::Room,
            Self::Blur => Backdrop::Blur,
            Self::Aurora => Backdrop::Scene(Scene::Aurora),
            Self::Koi => Backdrop::Scene(Scene::Koi),
        }
    }
}

/// The presenter's corner of the shared screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    #[default]
    Right,
}

/// The presenter's size: the board's 196, 262 and 340 px tall on its
/// 520 px tall screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    /// Its height, a share of the shared screen's.
    pub const fn height(self) -> f64 {
        match self {
            Self::Small => 196.0 / 520.0,
            Self::Medium => 262.0 / 520.0,
            Self::Large => 340.0 / 520.0,
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

/// The glow's width (logical pixels) and alpha, soft and strong: `auto`
/// goes between them.
const SOFT: (f64, f64) = (28.0, 0.55);
const STRONG: (f64, f64) = (70.0, 0.92);

/// The steps `auto` goes through from soft to strong.
pub const AUTO_STEPS: u8 = 8;

impl Strength {
    /// The glow's width (logical pixels) and its alpha, as the mockup's
    /// `box-shadow`s take them; `auto` as soft.
    pub const fn glow(self) -> (f64, f64) {
        match self {
            Self::Auto | Self::Soft => SOFT,
            Self::Medium => (46.0, 0.75),
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
             screen-light-strength = \"strong\"\nbackground = \"blur\"\n\
             lighting = false\nbanner = true\nbanner-name = \"Alex\"\n",
        )?;
        assert_eq!(
            camera,
            Camera {
                screen_light: false,
                screen_light_warmth: Warmth::Warm,
                screen_light_strength: Strength::Strong,
                lighting: false,
                banner: true,
                banner_name: "Alex".to_owned(),
                banner_project: true,
                background: Background::Blur,
                ..Camera::default()
            }
        );
        assert!(toml::from_str::<Camera>("background = \"beach\"\n").is_err());
        let camera: Camera = toml::from_str(
            "background = \"koi\"\naway = false\npresenter = true\n\
             presenter-side = \"left\"\npresenter-size = \"large\"\n",
        )?;
        assert_eq!(
            (camera.background, camera.away, camera.presenter),
            (Background::Koi, false, true)
        );
        assert_eq!(
            (camera.presenter_side, camera.presenter_size),
            (Side::Left, Size::Large)
        );
        let camera: Camera = toml::from_str(
            "face = \"crown\"\nframing = true\ngestures = false\ncelebrations = true\n",
        )?;
        assert_eq!(
            (
                camera.face,
                camera.framing,
                camera.gestures,
                camera.celebrations
            ),
            (Face::Crown, true, false, true)
        );
        assert_eq!(camera.face.wear(), Some(hocus_link::camera::Wear::Crown));
        assert!(toml::from_str::<Camera>("face = \"panda\"\n").is_err());
        assert!(toml::from_str::<Camera>("screen-light-warmth = \"hot\"\n").is_err());
        assert!(toml::from_str::<Camera>("screen-light-strength = 3\n").is_err());
        Ok(())
    }

    #[test]
    fn auto_goes_from_soft_to_strong() {
        assert_eq!(Strength::auto_glow(0), Strength::Soft.glow());
        assert_eq!(Strength::auto_glow(AUTO_STEPS), Strength::Strong.glow());
        let (w, a) = Strength::auto_glow(AUTO_STEPS / 2);
        assert!((w - 49.0).abs() < 1e-9 && (a - 0.735).abs() < 1e-9);
    }
}
