//! The moments drawn (#810, #811), on their screen alone and never in a
//! capture, a share's or a recording's frames: the fireworks' sparks and
//! flashes on the wallpaper, under the windows and the bar, their light
//! on the windows' edges (`crate::render::stack`); the confetti in flight
//! on the wallpaper too, and those settled right in front of their edge,
//! above it and on its top few pixels alone.
//!
//! What a moment draws is decided here, from its zone: the bar's moment
//! when a window is maximised (#800) gives it another.

use smithay::{
    backend::renderer::{
        Color32F,
        element::{Id, Kind, solid::SolidColorRenderElement},
    },
    utils::{Logical, Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{FireElement, draw::Clip, mesh::Mesh, snow::Holder, snow_look::REACH_BELOW},
};

/// Far enough to keep nothing out.
const FAR: f32 = 1.0e7;
/// The room a settled piece takes above its edge, and to its sides
/// (logical): its half diagonal, turned, a few pixels up.
const ABOVE: f64 = 22.0;
const SIDES: f64 = 14.0;

/// The whole of `frame`'s output, physical.
fn screen(frame: &Frame) -> Rectangle<i32, Physical> {
    Rectangle::from_size(
        frame
            .area
            .size
            .to_f64()
            .to_physical_precise_round(frame.scale),
    )
}

impl State {
    /// Whether a moment on screen `output` shows on `frame`.
    fn moment_on(&self, frame: &Frame, output: &str) -> bool {
        frame.purpose.screen() && frame.output.name() == output && !self.wall_hidden(frame.output)
    }

    /// The fireworks on `frame`'s output: the blank standing for them
    /// behind the glass, and themselves.
    pub(super) fn fireworks_layer(&self, frame: &Frame) -> Option<(FireElement, FireElement)> {
        let show = self.fx.fireworks.as_ref()?;
        if !self.moment_on(frame, &show.output) {
            return None;
        }
        let (added, screened) = show.shapes((frame.origin.x, frame.origin.y), frame.scale as f32);
        let screen = screen(frame);
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let layer =
            show.drawn
                .borrow_mut()
                .shapes(screen, [Mesh::default(), added, screened], clip);
        Some((FireElement::blank(show.blank.clone(), screen), layer))
    }

    /// The id of the blank standing for the fireworks.
    pub(super) fn fireworks_blank(&self) -> Option<Id> {
        Some(self.fx.fireworks.as_ref()?.blank.clone())
    }

    /// The light of the fireworks' flashes on a window drawn at `visual`
    /// (global) on `frame`'s output: its edges' colours, top, right,
    /// bottom, left.
    pub(super) fn fireworks_light(
        &self,
        frame: &Frame,
        visual: Rectangle<f64, Logical>,
    ) -> Option<[[f32; 4]; 4]> {
        let show = self.fx.fireworks.as_ref()?;
        if !self.moment_on(frame, &show.output) {
            return None;
        }
        show.light_on(
            [
                visual.loc.x,
                visual.loc.y,
                visual.loc.x + visual.size.w,
                visual.loc.y + visual.size.h,
            ]
            .map(|v| v as f32),
        )
    }

    /// The confetti in flight on `frame`'s output: the blank standing for
    /// them behind the glass, and themselves.
    pub(super) fn confetti_layer(&self, frame: &Frame) -> Option<(FireElement, FireElement)> {
        let confetti = self.fx.confetti.as_ref()?;
        if !self.moment_on(frame, &confetti.output) {
            return None;
        }
        let over = confetti.flying((frame.origin.x, frame.origin.y), frame.scale as f32);
        let screen = screen(frame);
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        let layer = confetti.drawn.borrow_mut().shapes(
            screen,
            [over, Mesh::default(), Mesh::default()],
            clip,
        );
        Some((FireElement::blank(confetti.blank.clone(), screen), layer))
    }

    /// The id of the blank standing for the confetti in flight.
    pub(super) fn confetti_blank(&self) -> Option<Id> {
        Some(self.fx.confetti.as_ref()?.blank.clone())
    }

    /// The confetti settled on `holder`'s edge, drawn at `edge` (global),
    /// `alpha` opaque, on `frame`'s output: above it, and on its top few
    /// pixels alone ([`REACH_BELOW`]).
    pub(super) fn confetti_front(
        &self,
        frame: &Frame,
        holder: &Holder,
        edge: Rectangle<f64, Logical>,
        alpha: f32,
    ) -> Option<FireElement> {
        let confetti = self.fx.confetti.as_ref()?;
        if !self.moment_on(frame, &confetti.output) || !confetti.lies_on(holder) {
            return None;
        }
        let Frame { origin, scale, .. } = *frame;
        let at = |x: f64, y: f64| ((x - origin.x) * scale, (y - origin.y) * scale);
        let (x0, y0) = at(edge.loc.x - SIDES, edge.loc.y - ABOVE);
        let (x1, y1) = at(edge.loc.x + edge.size.w + SIDES, edge.loc.y + REACH_BELOW);
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        let geometry = Rectangle::<i32, Physical>::new(
            (x0, y0).into(),
            (x1.ceil() as i32 - x0, y1.ceil() as i32 - y0).into(),
        )
        .intersection(screen(frame))?;
        // Its shapes and its clip output-local, as the element's matrix
        // takes them.
        let (ex, ey) = at(edge.loc.x, edge.loc.y);
        let mesh = confetti.lying(holder, [ex as f32, ey as f32], scale as f32, alpha)?;
        // Never below the edge's top few pixels.
        let (_, below) = at(0.0, edge.loc.y + REACH_BELOW);
        let clip = Clip::new([-FAR, -FAR, FAR, below as f32], [], 1.0);
        let mut fronts = confetti.fronts.borrow_mut();
        let drawn = fronts.entry(holder.clone()).or_default();
        Some(drawn.shapes(geometry, [mesh, Mesh::default(), Mesh::default()], clip))
    }

    /// The confetti settled on the bar of `frame`'s output, when it shows.
    pub(super) fn confetti_bar(&self, frame: &Frame) -> Option<FireElement> {
        self.fx.confetti.as_ref()?;
        let holder = Holder::Bar(frame.output.name());
        let edge = self.snow_edge(&holder, frame.now)?;
        self.confetti_front(frame, &holder, edge, 1.0)
    }
}

/// How wide the fireworks' light on a window's edge is, logical pixels:
/// just out of what it shows.
const LIT: f64 = 2.0;

/// The fireworks' light on the edges of a window at `rect` (logical,
/// output-local), `edges` their colours (top, right, bottom, left,
/// premultiplied), `alpha` as opaque as it is drawn; `frame` the
/// effects' frame, `ids` theirs.
pub(super) fn flash_edges(
    edges: &[[f32; 4]; 4],
    rect: Rectangle<f64, Logical>,
    scale: f64,
    alpha: f32,
    ids: &[Id; 4],
    frame: u64,
) -> Vec<SolidColorRenderElement> {
    let (x, y, w, h) = (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
    let r = crate::decoration::RADIUS;
    let places = [
        ((x + r, y - LIT), (w - 2.0 * r, LIT)),
        ((x + w, y + r), (LIT, h - 2.0 * r)),
        ((x + r, y + h), (w - 2.0 * r, LIT)),
        ((x - LIT, y + r), (LIT, h - 2.0 * r)),
    ];
    places
        .into_iter()
        .zip(edges)
        .zip(ids)
        .filter(|(((_, size), colour), _)| size.0 > 0.0 && size.1 > 0.0 && colour[3] > 0.004)
        .map(|(((at, size), c), id)| {
            let place = Rectangle::<f64, Logical>::new(at.into(), size.into());
            SolidColorRenderElement::new(
                id.clone(),
                place.to_physical_precise_round(scale),
                frame as usize,
                Color32F::new(c[0] * alpha, c[1] * alpha, c[2] * alpha, c[3] * alpha),
                Kind::Unspecified,
            )
        })
        .collect()
}
