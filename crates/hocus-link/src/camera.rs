//! The Hocuspanda camera's lines (#619, docs/design/camera.md):
//! `hocus-camera` publishes a PipeWire video source, "Hocuspanda", which
//! reads the real camera only while an application reads it, and draws the
//! user's effects into its frames.
//!
//! - The compositor starts it and tells it, one JSON object a line on its
//!   standard input, how its frames look ([`CameraLook`]): as it starts and
//!   when the configuration, the language or the current project change.
//! - It tells the compositor, one JSON object a line on its standard
//!   output, what happens to it ([`FromCamera`]): read or not, the real
//!   camera it reads, the light on the face (the screen light's "auto").
//! - The compositor tells the shell what the camera's window shows
//!   (`camera` [`CameraShown`]) as it connects and as that changes.
//!
//! The names its nodes go by on PipeWire are here: the shell's in-use sign
//! leaves its own streams out (they are no application's use).

use serde::{Deserialize, Serialize};

/// The source's node's name (`node.name`).
pub const CAMERA_NODE: &str = "hocuspanda-camera";
/// What applications list it as (`node.description`).
pub const CAMERA_DESCRIPTION: &str = "Hocuspanda";
/// Its own stream reading the real camera.
pub const CAMERA_INPUT_NODE: &str = "hocuspanda-camera-input";
/// The camera's window's preview, reading the source.
pub const CAMERA_PREVIEW_NODE: &str = "hocuspanda-camera-preview";
/// What every node of the Hocuspanda camera's name starts with.
pub const CAMERA_PREFIX: &str = "hocuspanda-camera";
/// Its frames: 1280 × 720.
pub const CAMERA_SIZE: (u32, u32) = (1280, 720);
/// At most 30 a second.
pub const CAMERA_FPS: u32 = 30;

/// How the Hocuspanda camera's frames look: the effects on, and what the
/// banner says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraLook {
    /// Lumière: exposure, white balance and warmth from the frame's
    /// statistics, and a light denoise.
    pub lighting: bool,
    /// Bandeau: the banner, when on.
    pub banner: Option<Banner>,
    /// Fond: what is behind the person (lot 3).
    #[serde(default)]
    pub backdrop: Backdrop,
    /// « Je reviens », when on: the person gone for 10 s, or the screen
    /// locked, the picture is a card on a living wallpaper.
    #[serde(default)]
    pub away: Option<Away>,
    /// The screen is locked.
    #[serde(default)]
    pub locked: bool,
    /// Présentateur: the person cut out, for the shared screen's stream
    /// (`FromCamera::Presenter`); the real camera is read for it.
    #[serde(default)]
    pub presenter: bool,
}

/// A living wallpaper of the desktop's (docs/design/effects.md) that the
/// camera draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scene {
    /// Aurore en musique (#801).
    Aurora,
    /// Bassin de koïs (#803).
    Koi,
}

impl Scene {
    /// Its word in `[camera] background`.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Aurora => "aurora",
            Self::Koi => "koi",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        [Self::Aurora, Self::Koi]
            .into_iter()
            .find(|s| s.word() == word)
    }
}

/// What is behind the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backdrop {
    /// The room, as the camera sees it.
    #[default]
    Room,
    /// The room blurred.
    Blur,
    /// A living wallpaper.
    Scene(Scene),
}

/// « Je reviens »: the living wallpaper under its card, and the language
/// its words are in (`hocus_words::Lang`'s code).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Away {
    pub scene: Scene,
    pub lang: String,
}

/// Why « Je reviens » shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AwayWhy {
    /// Nobody in the picture for 10 s.
    Gone,
    /// The screen locked.
    Locked,
}

/// The banner across the bottom of the picture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Banner {
    /// The user's name.
    pub name: String,
    /// Its second line, said in the desktop's language ("projet
    /// hocus-panda"): the current project desktop's, when it is one and
    /// chosen.
    pub project: Option<String>,
}

/// What `hocus-camera` tells the compositor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum FromCamera {
    /// Its source is on the graph, as this node.
    Ready { node: u32 },
    /// An application reads it (`true`), or none does any more.
    Read(bool),
    /// The real camera it reads (its description), or none found.
    Source(Option<String>),
    /// The light on the face, from 0 (black) to 1 (white): the centre of
    /// the real camera's picture, averaged over a second; said every half
    /// second while it is read, when it changed.
    Face(f32),
    /// Where its frames are made: a GPU's name, or none (the processor).
    Device(Option<String>),
    /// « Je reviens » shows, and why; or no longer.
    Away(Option<AwayWhy>),
    /// The person cut out (Présentateur), `width` × `height` pixels: its
    /// premultiplied RGBA, four bytes a pixel, follows this line on the
    /// output, raw.
    Presenter { width: u32, height: u32 },
}

impl FromCamera {
    /// One line of its output, or `None` (a line of something else).
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }

    /// The line to write.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl CameraLook {
    /// One line of its input, or `None`.
    pub fn read(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }

    /// The line to write.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// What the camera's window shows of the configuration and of the camera:
/// sent as the shell connects and as it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraShown {
    /// `[camera] lighting`: Lumière.
    pub lighting: bool,
    /// `[camera] banner`: Bandeau.
    pub banner: bool,
    /// `[camera] screen-light`.
    pub screen_light: bool,
    /// `[camera] screen-light-strength`: `auto`, `soft`, `medium` or
    /// `strong`.
    pub strength: String,
    /// The Hocuspanda camera is there (its program runs, its source is on
    /// the graph).
    pub ready: bool,
    /// An application reads it (its window's preview too).
    pub read: bool,
    /// The real camera it reads, if one was found.
    pub source: Option<String>,
    /// `[camera] background`: `room`, `blur`, or a living wallpaper's
    /// word.
    #[serde(default)]
    pub background: String,
    /// `[camera] away`: « Je reviens ».
    #[serde(default)]
    pub away: bool,
    /// `[camera] presenter`: Présentateur.
    #[serde(default)]
    pub presenter: bool,
    /// The microphone held muted: the screen locked during a call, with
    /// « Je reviens » on. The shell mutes the default source, and unmutes
    /// it as this ends if it muted it.
    #[serde(default)]
    pub hush: bool,
}

impl CameraShown {
    /// Whether an effect is chosen.
    pub fn effects(&self) -> bool {
        self.lighting || self.banner || (!self.background.is_empty() && self.background != "room")
    }

    /// The violet dot on the camera's tile: an effect is on, in the
    /// pictures of the Hocuspanda camera, which something reads (an
    /// application reading the camera itself gets none).
    pub fn dot(&self) -> bool {
        self.read && self.effects()
    }
}
