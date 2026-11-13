//! The snow drawn (#795): its layer under the windows, over the wallpaper
//! (the flakes, the ground's pile, the clumps), on the screens it falls
//! on; and the caps on the windows' and the bars' top edges, each right in
//! front of what holds it, so that a window in front covers it. A cap is
//! drawn above its edge and never over what is under it, but the top few
//! pixels of the edge its snow rests on and shadows. On the screen alone:
//! never in a capture, a share's or a recording's frames.

use smithay::{
    backend::renderer::element::Id,
    utils::{Logical, Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{
        FireElement,
        draw::Clip,
        snow::Holder,
        snow_look::{self, CapLook, REACH_BELOW},
        snowing::Snowing,
    },
};

/// Far enough to keep nothing out.
const FAR: f32 = 1.0e7;

impl State {
    /// Whether the snow shows on `frame`'s output, and how.
    fn snow_on(&self, frame: &Frame) -> Option<Snowing> {
        if !frame.purpose.screen() || self.wall_hidden(frame.output) {
            return None;
        }
        self.fx.snow.as_ref()?;
        self.snowing()
    }

    /// The snow's layer on `frame`'s output: the element standing for it
    /// behind the glass (blank, so that the glass is not blurred again at
    /// each of its frames), and itself.
    pub(super) fn snow_layer(&self, frame: &Frame) -> Option<(FireElement, FireElement)> {
        let snowing = self.snow_on(frame)?;
        let snow = self.fx.snow.as_ref()?;
        let field = snow.fields.get(&frame.output.name())?;
        let scale = frame.scale as f32;
        let mesh = snow_look::layer(field, scale, snowing == Snowing::Falling);
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
        Some((
            FireElement::blank(field.ids[1].clone(), screen),
            FireElement::shapes(
                field.ids[0].clone(),
                field.commit(snow.generation),
                screen,
                mesh,
                Default::default(),
                clip,
            ),
        ))
    }

    /// The id of the blank standing for the snow's layer on `frame`'s
    /// output.
    pub(super) fn snow_blank(&self, frame: &Frame) -> Option<Id> {
        let snow = self.fx.snow.as_ref()?;
        Some(snow.fields.get(&frame.output.name())?.ids[1].clone())
    }

    /// The cap on `holder`'s edge as drawn now (global; `edge` its top),
    /// `alpha` opaque, on `frame`'s output: above the edge, and on its top
    /// few pixels alone ([`REACH_BELOW`]).
    pub(super) fn snow_cap(
        &self,
        frame: &Frame,
        holder: &Holder,
        edge: Rectangle<f64, Logical>,
        alpha: f32,
    ) -> Option<FireElement> {
        let snowing = self.snow_on(frame)?;
        let snow = self.fx.snow.as_ref()?;
        let cap = snow.caps.get(holder)?;
        let Frame {
            origin,
            scale,
            area,
            ..
        } = *frame;
        let look = CapLook {
            edge,
            origin: (origin.x, origin.y),
            scale: scale as f32,
            alpha,
            glints: (snowing == Snowing::Falling).then(|| snow.time()),
        };
        let (over, added) = snow_look::cap(cap, &look)?;
        let bounds = [over.bounds(), added.bounds()]
            .into_iter()
            .flatten()
            .reduce(|a, b| {
                let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
                let (x1, y1) = (
                    (a[0] + a[2]).max(b[0] + b[2]),
                    (a[1] + a[3]).max(b[1] + b[3]),
                );
                [x0, y0, x1 - x0, y1 - y0]
            })?;
        let screen: Rectangle<i32, Physical> =
            Rectangle::from_size(area.size.to_f64().to_physical_precise_round(scale));
        let geometry = Rectangle::new((bounds[0], bounds[1]).into(), (bounds[2], bounds[3]).into())
            .intersection(screen)?;
        // What its edge holds, below its top few pixels (a bar's beyond its
        // cap's inset too).
        let s = scale as f32 * 12.0;
        let x0 = ((edge.loc.x - origin.x) * scale) as f32;
        let x1 = ((edge.loc.x + edge.size.w - origin.x) * scale) as f32;
        let y0 = ((edge.loc.y + REACH_BELOW - origin.y) * scale) as f32;
        let y1 = ((edge.loc.y + edge.size.h - origin.y) * scale).max(f64::from(y0)) as f32;
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [[x0 - s, y0, x1 + s, y1]], 1.0);
        Some(FireElement::shapes(
            cap.id.clone(),
            cap.commit(snow.generation),
            geometry,
            over,
            added,
            clip,
        ))
    }

    /// The cap on the bar of `frame`'s output, when it shows.
    pub(super) fn snow_bar_cap(&self, frame: &Frame) -> Option<FireElement> {
        let holder = Holder::Bar(frame.output.name());
        let edge = self.snow_edge(&holder, frame.now)?;
        self.snow_cap(frame, &holder, edge, 1.0)
    }
}
