//! Shapes in a view box, for illustrations (the pandas): made once, drawn
//! stretched into a rectangle.

use vello_cpu::kurbo::{Affine, BezPath, Ellipse, RoundedRect, Shape as _, Vec2};

/// Flattening tolerance, in view-box units.
const TOLERANCE: f64 = 0.05;

#[derive(Debug, Clone)]
pub struct Shape(pub(crate) BezPath);

impl Shape {
    /// An SVG path's `d`.
    pub fn svg(d: &str) -> Option<Self> {
        BezPath::from_svg(d).ok().map(Self)
    }

    /// An ellipse turned `degrees` clockwise about its centre.
    pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, degrees: f32) -> Self {
        let e = Ellipse::new(
            (f64::from(cx), f64::from(cy)),
            (f64::from(rx), f64::from(ry)),
            f64::from(degrees).to_radians(),
        );
        Self(e.to_path(TOLERANCE))
    }

    pub fn circle(cx: f32, cy: f32, r: f32) -> Self {
        Self::ellipse(cx, cy, r, r, 0.0)
    }

    pub fn rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Self {
        let r = RoundedRect::new(
            f64::from(x),
            f64::from(y),
            f64::from(x + w),
            f64::from(y + h),
            f64::from(radius),
        );
        Self(r.to_path(TOLERANCE))
    }
}

impl Shape {
    /// The shape moved by `t`, for a part that animates.
    pub fn transformed(&self, t: Transform) -> Self {
        let mut path = self.0.clone();
        path.apply_affine(t.0);
        Self(path)
    }
}

/// A move, turn or stretch in a view box; `then` chains them in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform(Affine);

impl Transform {
    pub const IDENTITY: Self = Self(Affine::IDENTITY);

    pub fn translate(dx: f32, dy: f32) -> Self {
        Self(Affine::translate((f64::from(dx), f64::from(dy))))
    }

    /// Turned `degrees` clockwise about (`cx`, `cy`).
    pub fn rotate(degrees: f32, cx: f32, cy: f32) -> Self {
        Self(Affine::rotate_about(
            f64::from(degrees).to_radians(),
            vello_cpu::kurbo::Point::new(f64::from(cx), f64::from(cy)),
        ))
    }

    /// Stretched by `sx`, `sy` about (`cx`, `cy`).
    pub fn scale(sx: f32, sy: f32, cx: f32, cy: f32) -> Self {
        let c = Vec2::new(f64::from(cx), f64::from(cy));
        Self(
            Affine::translate(c)
                * Affine::scale_non_uniform(f64::from(sx), f64::from(sy))
                * Affine::translate(-c),
        )
    }

    /// This one, then `next`.
    pub fn then(self, next: Self) -> Self {
        Self(next.0 * self.0)
    }

    /// Where it takes a point.
    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let p = self.0 * vello_cpu::kurbo::Point::new(f64::from(x), f64::from(y));
        (p.x as f32, p.y as f32)
    }
}

impl Shape {
    /// Its bounds: left, top, right, bottom.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let b = self.0.bounding_box();
        (b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32)
    }
}
