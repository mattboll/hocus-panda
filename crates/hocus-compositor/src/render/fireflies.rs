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
        if !frame.purpose.screen() || self.fullscreen_in_front(frame.output) {
            return None;
        }
        self.fireflies_out()?;
        self.fx.fireflies.as_ref()?.swarms.get(&frame.output.name())
    }

    /// Their glows on `frame`'s output: the blank standing for them behind
    /// the glass, and themselves.
    pub(super) fn fireflies_layer(&self, frame: &Frame) -> Option<(FireElement, FireElement)> {
        let swarm = self.fireflies_on(frame)?;
        let flies = self.fx.fireflies.as_ref()?;
        let moving = self.fireflies_out() == Some(Plays::Moving);
        let alpha = if moving { flies.out(frame.now) } else { 1.0 };
        let (added, screened) = swarm.glows(
            (frame.origin.x, frame.origin.y),
            frame.scale as f32,
            alpha,
            moving,
        );
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let [x0, y0, x1, y1] = [&added, &screened]
            .iter()
            .filter_map(|m| m.bounds())
            .map(|[x, y, w, h]| [x, y, x + w, y + h])
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })?;
        let geometry =
            Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into()).intersection(screen)?;
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        Some((
            FireElement::blank(swarm.ids[1].clone(), screen),
            FireElement::shapes(
                swarm.ids[0].clone(),
                swarm.commit(),
                geometry,
                Mesh::default(),
                added,
                clip,
            )
            .screened(screened),
        ))
    }

    /// The id of the blank standing for the fireflies on `frame`'s output.
    pub(super) fn fireflies_blank(&self, frame: &Frame) -> Option<Id> {
        let flies = self.fx.fireflies.as_ref()?;
        Some(flies.swarms.get(&frame.output.name())?.ids[1].clone())
    }
}
