//! The desktop's effects, for the fun of it (#434, the foundation #793,
//! docs/design/effects.md): each off by default, its own switch in
//! `[effects]` and in Settings › Appearance › Effects.
//!
//! What they share: when they may play ([`State::effects_still`]), the
//! window pass that draws a window's picture through a shader of its own
//! ([`draw::Fire`]), the particles ([`particles`]) and the shapes drawn in
//! one call ([`mesh`]), the layer under the windows and the decorations on
//! their edges (`crate::render::snow`), and the windows' motion
//! ([`motion`]). Built with its first effects: closing a window in flames
//! ([`flames`], #794), snow that settles ([`snow`], #795); then the agents
//! as creatures on their windows ([`creatures`], #796), and windows in
//! jelly ([`jelly`], #797).
//!
//! Every effect follows the same rules: none with reduced motion or in
//! the power saver, over a fullscreen window, while a screen or a window
//! is shared or the screen recorded; never in a capture, a share's or a
//! recording's frames; never on the CPU (pixman draws none: what would
//! have played plays as it does without the effect); nothing drawn per
//! frame once it is over.

pub mod creatures;
pub mod draw;
pub mod flames;
pub mod jelly;
pub mod mesh;
pub mod motion;
pub mod particles;
pub mod rig;
pub mod snow;
pub mod snow_look;
pub mod snowing;

pub use draw::{Fire, FireElement};

/// What the effects playing now keep from frame to frame.
#[derive(Debug, Default)]
pub struct Live {
    /// The windows' motions, followed while an effect reacts to them.
    pub motions: motion::Motions,
    /// The snow, while it is on (#795).
    pub snow: Option<snow::Snow>,
    /// The snows started so far, for their seeds.
    snows: u64,
    /// The agents as creatures on their windows (#796).
    pub creatures: creatures::Creatures,
}

use crate::{State, model::Mode, model::WindowId};

/// Why the effects do not play now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Still {
    ReducedMotion,
    PowerSaver,
    /// A screen or a window is shared, or the screen recorded.
    Shown,
    /// The compositor draws on the CPU.
    Cpu,
}

impl State {
    /// Why no effect plays now; `None`: they may.
    pub(crate) fn effects_still(&self) -> Option<Still> {
        if self.reduced_motion() {
            Some(Still::ReducedMotion)
        } else if self.power_saving() {
            Some(Still::PowerSaver)
        } else if self.share.shares.values().any(|s| s.node.is_some()) || self.record.recording() {
            Some(Still::Shown)
        } else if !self.graphics.blur {
            Some(Still::Cpu)
        } else {
            None
        }
    }

    /// Whether window `id`, closing now, burns (#794): the flames are on,
    /// the effects may play, and it is an application's window (not a
    /// dialog) that is not fullscreen nor under a fullscreen one.
    pub(crate) fn burns_on_close(&self, id: WindowId) -> bool {
        if !self.config.effects.flames {
            return false;
        }
        if let Some(still) = self.effects_still() {
            tracing::debug!(window = id.0, ?still, "closes without flames");
            return false;
        }
        let fullscreen = self.model.mode(id) == Mode::Fullscreen
            || self
                .output_of(id)
                .is_some_and(|o| self.fullscreen_in_front(&o));
        let dialog = self.model.parent(id).is_some();
        if fullscreen || dialog {
            tracing::debug!(window = id.0, fullscreen, dialog, "closes without flames");
            return false;
        }
        true
    }

    /// Whether window `id`, dragged now, goes in jelly (#797): the jelly
    /// is on, the effects may play, and no fullscreen window is in front
    /// on its screen.
    pub(crate) fn jellies(&self, id: WindowId) -> bool {
        self.config.effects.jelly
            && self.effects_still().is_none()
            && !self
                .output_of(id)
                .is_some_and(|o| self.fullscreen_in_front(&o))
    }
}
