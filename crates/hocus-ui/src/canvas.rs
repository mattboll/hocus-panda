//! The painting interface widgets draw through (decision 0009): logical
//! coordinates in, vello_cpu underneath. Changing renderer stays here.

use std::{fmt, sync::Arc};

use parley::{Layout, PositionedLayoutItem};
use vello_cpu::{
    Glyph, Image, ImageSource, Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
    kurbo::{Affine, Arc as KArc, BezPath, Cap, Rect as KRect, RoundedRect, Shape as _, Stroke},
    peniko::ImageSampler,
};

use crate::{
    geom::{PixelRect, Point, Rect},
    theme::Color,
};

/// Flattening tolerance, in physical pixels.
const TOLERANCE: f64 = 0.1;

fn paint(c: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Draws one region of a surface: logical coordinates, `scale` physical
/// pixels each, the region's corner at the origin of the pixels.
pub struct Canvas {
    ctx: RenderContext,
    resources: Resources,
    scale: f32,
    /// The region's corner, in physical pixels.
    origin: (f64, f64),
    /// Offsets pushed by translated subtrees, in logical pixels.
    offsets: Vec<(f32, f32)>,
}

impl fmt::Debug for Canvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Canvas")
            .field("scale", &self.scale)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

impl Canvas {
    pub(crate) fn new(region: PixelRect, scale: f32) -> Self {
        Self {
            ctx: RenderContext::new(region.w as u16, region.h as u16),
            resources: Resources::new(),
            scale,
            origin: (f64::from(region.x), f64::from(region.y)),
            offsets: Vec::new(),
        }
    }

    /// Starts again on another region of the same size.
    pub(crate) fn reuse(&mut self, region: PixelRect, scale: f32) -> bool {
        if u32::from(self.ctx.width()) != region.w || u32::from(self.ctx.height()) != region.h {
            return false;
        }
        self.ctx.reset();
        self.scale = scale;
        self.origin = (f64::from(region.x), f64::from(region.y));
        self.offsets.clear();
        true
    }

    pub(crate) fn finish(&mut self, target: &mut Pixmap) {
        self.ctx.flush();
        self.ctx.render(target, &mut self.resources);
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    fn offset(&self) -> (f32, f32) {
        self.offsets
            .iter()
            .fold((0.0, 0.0), |(x, y), (dx, dy)| (x + dx, y + dy))
    }

    /// Logical to the region's physical pixels.
    fn transform(&self) -> Affine {
        let (dx, dy) = self.offset();
        let s = f64::from(self.scale);
        Affine::translate((-self.origin.0, -self.origin.1))
            * Affine::scale(s)
            * Affine::translate((f64::from(dx), f64::from(dy)))
    }

    fn rounded(r: Rect, radius: f32) -> RoundedRect {
        RoundedRect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.bottom()),
            f64::from(radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0)),
        )
    }

    pub fn fill_rect(&mut self, r: Rect, radius: f32, color: Color) {
        self.ctx.set_transform(self.transform());
        self.ctx.set_paint(paint(color));
        self.ctx
            .fill_path(&Self::rounded(r, radius).to_path(TOLERANCE));
    }

    /// A line of `width` inside the rectangle's edge.
    pub fn stroke_rect(&mut self, r: Rect, radius: f32, width: f32, color: Color) {
        let half = width / 2.0;
        let inner = Rect::new(r.x + half, r.y + half, r.w - width, r.h - width);
        self.ctx.set_transform(self.transform());
        self.ctx.set_paint(paint(color));
        self.ctx.set_stroke(Stroke::new(f64::from(width)));
        self.ctx
            .stroke_path(&Self::rounded(inner, radius - half).to_path(TOLERANCE));
    }

    /// A soft shadow under a rounded rectangle.
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color) {
        self.ctx.set_transform(self.transform());
        self.ctx.set_paint(paint(color));
        let rect = KRect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.bottom()),
        );
        self.ctx
            .fill_blurred_rounded_rect(&rect, radius, blur / 2.0, false);
    }

    /// An arc of a circle, angles in radians from 3 o'clock, clockwise.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(
        &mut self,
        center: Point,
        radius: f32,
        start: f32,
        sweep: f32,
        width: f32,
        color: Color,
    ) {
        let arc = KArc::new(
            (f64::from(center.x), f64::from(center.y)),
            (f64::from(radius), f64::from(radius)),
            f64::from(start),
            f64::from(sweep),
            0.0,
        );
        self.ctx.set_transform(self.transform());
        self.ctx.set_paint(paint(color));
        self.ctx
            .set_stroke(Stroke::new(f64::from(width)).with_caps(Cap::Round));
        self.ctx.stroke_path(&arc.into_path(TOLERANCE));
    }

    /// A path in a `view`×`view` box, drawn into `r`: icons.
    pub fn path(&mut self, path: &BezPath, view: f32, r: Rect, color: Color) {
        let fit = Affine::translate((f64::from(r.x), f64::from(r.y)))
            * Affine::scale_non_uniform(f64::from(r.w / view), f64::from(r.h / view));
        self.ctx.set_transform(self.transform() * fit);
        self.ctx.set_paint(paint(color));
        self.ctx.fill_path(path);
    }

    /// Premultiplied pixels stretched over `r`.
    pub fn image(&mut self, pixmap: &Arc<Pixmap>, r: Rect) {
        let sx = f64::from(r.w) / f64::from(pixmap.width());
        let sy = f64::from(r.h) / f64::from(pixmap.height());
        let place =
            Affine::translate((f64::from(r.x), f64::from(r.y))) * Affine::scale_non_uniform(sx, sy);
        self.ctx.set_transform(self.transform() * place);
        self.ctx.set_paint_transform(Affine::IDENTITY);
        self.ctx.set_paint(Image {
            image: ImageSource::Pixmap(pixmap.clone()),
            sampler: ImageSampler::default(),
        });
        self.ctx.fill_rect(&KRect::new(
            0.0,
            0.0,
            f64::from(pixmap.width()),
            f64::from(pixmap.height()),
        ));
    }

    /// Text laid out at this canvas's scale, its top left corner at `at`.
    pub fn text(&mut self, layout: &Layout<()>, at: Point, color: Color) {
        let (dx, dy) = self.offset();
        let s = f64::from(self.scale);
        // Glyph positions are physical already: only move them.
        let place = Affine::translate((
            (f64::from(at.x + dx)) * s - self.origin.0,
            (f64::from(at.y + dy)) * s - self.origin.1,
        ));
        self.ctx.set_transform(place);
        self.ctx.set_paint(paint(color));
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                // parley and vello_cpu disagree on the type of variable font
                // coordinates.
                let coords: Vec<i16> = run
                    .normalized_coords()
                    .iter()
                    .map(|c| c.to_bits())
                    .collect();
                let glyphs = glyph_run.positioned_glyphs().map(|g| Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                });
                if let Err(err) = self
                    .ctx
                    .glyph_run(&mut self.resources, run.font())
                    .font_size(run.font_size())
                    .normalized_coords(&coords)
                    .fill_glyphs(glyphs)
                {
                    tracing::debug!(?err, "some glyphs could not be drawn");
                }
            }
        }
    }

    /// Clips what follows to `r` until [`Self::pop`].
    pub fn push_clip(&mut self, r: Rect, radius: f32) {
        self.ctx.set_transform(self.transform());
        self.ctx
            .push_clip_layer(&Self::rounded(r, radius).to_path(TOLERANCE));
    }

    /// Draws what follows at `opacity` until [`Self::pop`].
    pub fn push_opacity(&mut self, opacity: f32) {
        self.ctx.push_opacity_layer(opacity);
    }

    pub fn pop(&mut self) {
        self.ctx.pop_layer();
    }

    /// Moves what follows by `(dx, dy)` logical pixels until
    /// [`Self::pop_offset`].
    pub fn push_offset(&mut self, dx: f32, dy: f32) {
        self.offsets.push((dx, dy));
    }

    pub fn pop_offset(&mut self) {
        self.offsets.pop();
    }
}
