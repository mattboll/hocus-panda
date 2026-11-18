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
}

impl CameraShown {
    /// Whether an effect is chosen.
    pub fn effects(&self) -> bool {
        self.lighting || self.banner
    }

    /// The violet dot on the camera's tile: an effect is on, in the
    /// pictures of the Hocuspanda camera, which something reads (an
    /// application reading the camera itself gets none).
    pub fn dot(&self) -> bool {
        self.read && self.effects()
    }
}
