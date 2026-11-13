//! The fireflies drawn (#804): their glows on the wallpaper, under the
//! windows and the bar (added to it, the bright ones' wide glow screened
//! over it), on each screen they are out on; a blank in their place behind
//! the glass of the bar. On the screen alone: never in a capture, a
//! share's or a recording's frames.

use smithay::{
    backend::renderer::element::Id,
    utils::{Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{FireElement, Plays, draw::Clip, fireflies::Swarm, mesh::Mesh},
};

/// Far enough to keep nothing out.
const FAR: f32 = 1.0e7;

impl State {
    /// The fireflies over `frame`'s output, when they show there.
    fn fireflies_on(&self, frame: &Frame) -> Option<&Swarm> {
        if !frame.purpose.screen() || self.wall_hidden(frame.output) {
            return None;
        }
        self.fireflies_out()?;
        self.fx.fireflies.as_ref()?.swarms.get(&frame.output.name())
    }

    /// Their glows on `frame`'s output: the blank standing for them behind
    /// the glass, and themselves, telling only what of them changed
    /// (#815).
    pub(super) fn fireflies_layer(&self, frame: &Frame) -> Option<(FireElement, FireElement)> {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Fireflies);
        let swarm = self.fireflies_on(frame)?;
        let flies = self.fx.fireflies.as_ref()?;
        let moving = self.fireflies_out() == Some(Plays::Moving);
        // As of the effects' last frame: the same between two.
        let alpha = if moving {
            flies.out(self.fx.pace.at)
        } else {
            1.0
        };
        let (added, screened) = swarm.glows(
            (frame.origin.x, frame.origin.y),
            frame.scale as f32,
            alpha,
            moving,
        );
        if added.is_empty() && screened.is_empty() {
            return None;
        }
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let glows =
            swarm
                .glows
                .borrow_mut()
                .shapes(screen, [Mesh::default(), added, screened], clip);
        Some((FireElement::blank(swarm.blank.clone(), screen), glows))
    }

    /// The id of the blank standing for the fireflies on `frame`'s output.
    pub(super) fn fireflies_blank(&self, frame: &Frame) -> Option<Id> {
        let flies = self.fx.fireflies.as_ref()?;
        Some(flies.swarms.get(&frame.output.name())?.blank.clone())
    }
}
