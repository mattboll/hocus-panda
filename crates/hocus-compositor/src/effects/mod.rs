//! The desktop's effects, for the fun of it (#434, the foundation #793,
//! docs/design/effects.md): each off by default, its own switch in
//! `[effects]` and in Settings › Appearance › Effects.
//!
//! What they share: when they may play ([`State::effects_still`]), the
//! window pass that draws a window's picture through a shader of its own
//! ([`draw::Fire`]), and the particles ([`particles`]). Built with its
//! first effect, closing a window in flames ([`flames`], #794).
//!
//! Every effect follows the same rules: none with reduced motion or in
//! the power saver, over a fullscreen window, while a screen or a window
//! is shared or the screen recorded; never in a capture, a share's or a
//! recording's frames; never on the CPU (pixman draws none: what would
//! have played plays as it does without the effect); nothing drawn per
//! frame once it is over.

pub mod draw;
pub mod flames;
pub mod particles;

pub use draw::{Fire, FireElement};

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
}
