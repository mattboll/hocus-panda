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
        Fire, FireElement,
        draw::Clip,
        kept::Changed,
        mesh::Mesh,
        snow::{Holder, MAX_CAP},
        snow_look::{self, CapLook, REACH_BELOW},
        snowing::Snowing,
    },
};

/// Far enough to keep nothing out.
const FAR: f32 = 1.0e7;
/// A cap's room around its edge and its snow, logical pixels: its
/// shadow's blur, its rounded ends, its glints.
const REACH: f64 = 10.0;

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
    /// each of its frames), and itself: made again only as the snow moves
    /// on, telling the flakes' places alone, those where the wallpaper
    /// shows alone drawn (#815).
    pub(super) fn snow_layer<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
    ) -> Option<(FireElement, FireElement)> {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Snow);
        let snowing = self.snow_on(frame)?;
        let snow = self.fx.snow.as_ref()?;
        let field = snow.fields.get(&frame.output.name())?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let falling = snowing == Snowing::Falling;
        let drawn = Some((snow.generation, frame.scale.to_bits(), falling));
        let mut shown = field.shown.borrow_mut();
        let layer = match shown.again(screen) {
            Some(again) if field.drawn.get() == drawn => again,
            _ => {
                let scale = frame.scale as f32;
                let mesh = snow_look::layer(field, scale, falling, frame.wall_seen);
                // Where it changed: each flake's places, the ground's
                // columns that grew, the clumps'.
                let pieces = snow_look::pieces(field, scale, falling);
                let changed = snow_look::layer_changes(&field.pieces.borrow(), &pieces)
                    .map_or(Changed::All, Changed::Within);
                *field.pieces.borrow_mut() = pieces;
                let clip = Clip::new([-FAR, -FAR, FAR, FAR], [], 1.0);
                let layer = shown.redraw(renderer, screen, mesh, Mesh::default(), clip, changed)?;
                field.drawn.set(drawn);
                layer
            }
        };
        Some((FireElement::blank(field.blank.clone(), screen), layer))
    }

    /// The id of the blank standing for the snow's layer on `frame`'s
    /// output.
    pub(super) fn snow_blank(&self, frame: &Frame) -> Option<Id> {
        let snow = self.fx.snow.as_ref()?;
        Some(snow.fields.get(&frame.output.name())?.blank.clone())
    }

    /// The cap on `holder`'s edge as drawn now (global; `edge` its top),
    /// `alpha` opaque, on `frame`'s output, front first (its glints, its
    /// snow): above the edge, and on its top few pixels alone
    /// ([`REACH_BELOW`]). Its snow made again only as it changes, or its
    /// edge moves to another fraction of a pixel (#815).
    pub(super) fn snow_cap<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        holder: &Holder,
        edge: Rectangle<f64, Logical>,
        alpha: f32,
    ) -> Vec<FireElement> {
        self.snow_cap_parts(renderer, frame, holder, edge, alpha)
            .unwrap_or_default()
    }

    fn snow_cap_parts<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
        holder: &Holder,
        edge: Rectangle<f64, Logical>,
        alpha: f32,
    ) -> Option<Vec<FireElement>> {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Snow);
        let snowing = self.snow_on(frame)?;
        let snow = self.fx.snow.as_ref()?;
        let cap = snow.caps.get(holder)?;
        let Frame {
            origin,
            scale,
            area,
            ..
        } = *frame;
        // Where it may draw: around its edge, from above its highest snow
        // to below its shadow (output-local, physical, whole pixels).
        let screen: Rectangle<i32, Physical> =
            Rectangle::from_size(area.size.to_f64().to_physical_precise_round(scale));
        let at = |x: f64, y: f64| ((x - origin.x) * scale, (y - origin.y) * scale);
        let (x0, y0) = at(edge.loc.x - REACH, edge.loc.y - f64::from(MAX_CAP) - REACH);
        let (x1, y1) = at(
            edge.loc.x + edge.size.w + REACH,
            edge.loc.y + REACH_BELOW + REACH,
        );
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        let geometry = Rectangle::<i32, Physical>::new(
            (x0, y0).into(),
            (x1.ceil() as i32 - x0, y1.ceil() as i32 - y0).into(),
        )
        .intersection(screen)?;
        // Its look from the element's corner: the output's, moved there.
        let corner = (
            origin.x + f64::from(geometry.loc.x) / scale,
            origin.y + f64::from(geometry.loc.y) / scale,
        );
        let look = CapLook {
            edge,
            origin: corner,
            scale: scale as f32,
            alpha,
            glints: (snowing == Snowing::Falling).then(|| snow.time()),
        };
        let (ex, ey) = at(edge.loc.x, edge.loc.y);
        let made_of = [
            edge.size.w as f32,
            edge.size.h as f32,
            (ex - f64::from(geometry.loc.x)) as f32,
            (ey - f64::from(geometry.loc.y)) as f32,
            scale as f32,
            alpha,
        ];
        // What its edge holds, below its top few pixels (a bar's beyond its
        // cap's inset too).
        let s = scale as f32 * 12.0;
        let (gx, gy) = (geometry.loc.x as f32, geometry.loc.y as f32);
        let (ex0, ey0) = at(edge.loc.x, edge.loc.y + REACH_BELOW);
        let (ex1, ey1) = at(edge.loc.x + edge.size.w, edge.loc.y + edge.size.h);
        let (ex0, ey0, ex1) = (ex0 as f32 - gx, ey0 as f32 - gy, ex1 as f32 - gx);
        let ey1 = (ey1 as f32 - gy).max(ey0);
        let clip = Clip::new([-FAR, -FAR, FAR, FAR], [[ex0 - s, ey0, ex1 + s, ey1]], 1.0);
        let mut drawn = cap.drawn.borrow_mut();
        let drawn = drawn.entry(frame.output.name()).or_default();
        let same_look = drawn.look == made_of;
        // Its columns as drawn, within a twentieth of a pixel: not drawn
        // again as its slopes settle by less.
        let still = drawn.h.len() == cap.h.len()
            && drawn
                .h
                .iter()
                .zip(&cap.h)
                .all(|(a, b)| (a - b).abs() < snow_look::UNSEEN);
        let body = match drawn.shown.again(geometry) {
            Some(again) if still && same_look => Some(again),
            _ => {
                // What changed: where its columns moved.
                let changed = match snow_look::cap_changes(&drawn.h, &cap.h, &look) {
                    Some(columns) if same_look => Changed::Within(
                        columns
                            .map(|[x0, x1]| {
                                let x0 = x0.floor() as i32;
                                [x0, 0, x1.ceil() as i32 - x0, geometry.size.h]
                            })
                            .into_iter()
                            .collect(),
                    ),
                    _ => Changed::All,
                };
                drawn.h.clone_from(&cap.h);
                drawn.look = made_of;
                match snow_look::cap(cap, &look) {
                    Some(over) => {
                        drawn
                            .shown
                            .redraw(renderer, geometry, over, Mesh::default(), clip, changed)
                    }
                    None => {
                        drawn.shown = Default::default();
                        None
                    }
                }
            }
        };
        // Its glints over it, an element of their own: they twinkle at
        // each of the snow's frames, the snow under them changes less.
        let glints = body.as_ref().map(|_| {
            // Drawn straight: in the output's places, as shapes are.
            let mut glints = snow_look::glints(cap, &look);
            glints.shift(geometry.loc.x as f32, geometry.loc.y as f32);
            let hole = [ex0 - s + gx, ey0 + gy, ex1 + s + gx, ey1 + gy];
            drawn.glints.shapes(
                geometry,
                [Mesh::default(), glints, Mesh::default()],
                Clip::new([-FAR, -FAR, FAR, FAR], [hole], 1.0),
            )
        });
        Some(glints.into_iter().chain(body).collect())
    }

    /// The cap on the bar of `frame`'s output, when it shows.
    pub(super) fn snow_bar_cap<R: Fire>(
        &self,
        renderer: &mut R,
        frame: &Frame,
    ) -> Vec<FireElement> {
        let holder = Holder::Bar(frame.output.name());
        match self.snow_edge(&holder, frame.now) {
            Some(edge) => self.snow_cap(renderer, frame, &holder, edge, 1.0),
            None => Vec::new(),
        }
    }
}
