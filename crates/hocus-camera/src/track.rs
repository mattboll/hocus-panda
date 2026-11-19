//! What the face's and the hands' models share (lot 4): a model run with
//! tract, a square part of the frame turned and scaled into a model's
//! picture ([`Roi`]), MediaPipe's anchors and their boxes ([`Anchors`],
//! [`Detection`]), and the filter that keeps points still between frames
//! ([`OneEuro`]).
//!
//! The models are MediaPipe's (Apache-2.0, `models/LICENSE-MEDIAPIPE`),
//! their graphs written as ONNX by `scripts/camera/landmarks-to-onnx.py`,
//! their weights unchanged (16-bit floats, as MediaPipe stores them): in
//! the program, no download.

use tract_onnx::prelude::*;

pub type Plan = std::sync::Arc<TypedSimplePlan>;

/// A model read and planned for its input's shape.
pub fn plan(bytes: &[u8], shape: &[usize]) -> TractResult<Plan> {
    tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(bytes))?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

/// `plan` run on `input` (its shape the plan's): its outputs, flat.
pub fn run(plan: &Plan, shape: &[usize], input: Vec<f32>) -> TractResult<Vec<Vec<f32>>> {
    let tensor = tract_ndarray::ArrayD::from_shape_vec(shape, input)?;
    let outputs = plan.run(tvec!(Tensor::from(tensor).into()))?;
    outputs
        .iter()
        .map(|o| Ok(o.to_plain_array_view::<f32>()?.iter().copied().collect()))
        .collect()
}

/// The picture's values: from 0 to 1, or from -1 to 1 (the face's
/// detector).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Unit,
    Signed,
}

/// A frame a model looks at: its pixels, four bytes each, red, green and
/// blue at `order` (`[0, 1, 2]` for RGBA, `[2, 1, 0]` for B, G, R, x).
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub order: [usize; 3],
}

/// A square of the frame, turned: what a model looks at. Its centre and
/// side in the frame's pixels; `angle` the direction of its x axis in the
/// frame (radians, the frame's y going down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roi {
    pub cx: f32,
    pub cy: f32,
    pub size: f32,
    pub angle: f32,
}

impl Roi {
    /// The whole frame, its short side padded: what a detector looks at.
    pub fn whole(width: usize, height: usize) -> Self {
        Self {
            cx: width as f32 / 2.0,
            cy: height as f32 / 2.0,
            size: width.max(height) as f32,
            angle: 0.0,
        }
    }

    /// A point of the square, `a` and `b` from 0 to 1 across it, in the
    /// frame.
    pub fn to_frame(&self, a: f32, b: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        let (u, v) = ((a - 0.5) * self.size, (b - 0.5) * self.size);
        (self.cx + c * u - s * v, self.cy + s * u + c * v)
    }

    /// The square of `frame` (RGBA, or B, G, R, x with `order`), into a
    /// planar `n` × `n` picture, between the frame's pixels (black out of
    /// it).
    pub fn sample(&self, frame: &Frame<'_>, n: usize, range: Range, out: &mut Vec<f32>) {
        let &Frame {
            pixels: frame,
            width,
            height,
            order,
        } = frame;
        out.clear();
        out.resize(3 * n * n, 0.0);
        let (s, c) = self.angle.sin_cos();
        let step = self.size / n as f32;
        // Each picture pixel's centre, in the frame.
        let (x0, y0) = self.to_frame(0.5 / n as f32, 0.5 / n as f32);
        let (dxu, dyu) = (c * step, s * step);
        let (dxv, dyv) = (-s * step, c * step);
        let (lo, span) = match range {
            Range::Unit => (0.0, 1.0 / 255.0),
            Range::Signed => (-1.0, 2.0 / 255.0),
        };
        let (wi, hi) = (width as isize, height as isize);
        let px = |x: isize, y: isize| -> [f32; 3] {
            if x < 0 || y < 0 || x >= wi || y >= hi {
                return [0.0; 3];
            }
            let at = (y as usize * width + x as usize) * 4;
            order.map(|o| f32::from(frame.get(at + o).copied().unwrap_or(0)))
        };
        let (planes, rest) = out.split_at_mut(n * n);
        let (greens, blues) = rest.split_at_mut(n * n);
        for v in 0..n {
            let (mut x, mut y) = (x0 + dxv * v as f32 - 0.5, y0 + dyv * v as f32 - 0.5);
            for u in 0..n {
                let (fx, fy) = (x.floor(), y.floor());
                let (tx, ty) = (x - fx, y - fy);
                let (ix, iy) = (fx as isize, fy as isize);
                let (a, b) = (px(ix, iy), px(ix + 1, iy));
                let (cc, d) = (px(ix, iy + 1), px(ix + 1, iy + 1));
                let at = v * n + u;
                for ((((plane, a), b), cc), d) in [&mut *planes, &mut *greens, &mut *blues]
                    .into_iter()
                    .zip(a)
                    .zip(b)
                    .zip(cc)
                    .zip(d)
                {
                    let top = a + (b - a) * tx;
                    let bottom = cc + (d - cc) * tx;
                    if let Some(p) = plane.get_mut(at) {
                        *p = lo + (top + (bottom - top) * ty) * span;
                    }
                }
                x += dxu;
                y += dyu;
            }
        }
    }

    /// The square around `points` (frame pixels) seen turned by `angle`:
    /// their box in that turn, its centre moved by `shift` of its height
    /// along the turn's y, its long side times `scale`.
    pub fn around(points: &[(f32, f32)], angle: f32, scale: f32, shift: f32) -> Option<Self> {
        let (s, c) = angle.sin_cos();
        let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            let (u, v) = (c * x + s * y, -s * x + c * y);
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        if u1 <= u0 || v1 <= v0 {
            return None;
        }
        let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0 + shift * (v1 - v0));
        Some(Self {
            cx: c * cu - s * cv,
            cy: s * cu + c * cv,
            size: (u1 - u0).max(v1 - v0) * scale,
            angle,
        })
    }
}

/// A turn from `from` to `to`, made so that it points the way `target`
/// says (radians: 0 along the frame's x, π/2 down its y).
pub fn angle_between(from: (f32, f32), to: (f32, f32), target: f32) -> f32 {
    let along = (to.1 - from.1).atan2(to.0 - from.0);
    along - target
}

/// MediaPipe's anchors for a square input of `size`: on each grid of
/// `strides` (layers of the same stride together), two of them a cell
/// per layer, their centres in the input from 0 to 1.
#[derive(Debug, Clone)]
pub struct Anchors {
    pub centres: Vec<(f32, f32)>,
}

impl Anchors {
    pub fn new(size: usize, strides: &[usize]) -> Self {
        let mut centres = Vec::new();
        let mut rest = strides;
        while let Some(&stride) = rest.first() {
            let same = rest.iter().take_while(|&&s| s == stride).count();
            rest = rest.get(same..).unwrap_or_default();
            let cells = size.div_ceil(stride);
            for y in 0..cells {
                for x in 0..cells {
                    for _ in 0..2 * same {
                        centres.push((
                            (x as f32 + 0.5) / cells as f32,
                            (y as f32 + 0.5) / cells as f32,
                        ));
                    }
                }
            }
        }
        Self { centres }
    }
}

/// A detector's find: its box and its points, from 0 to 1 across the
/// square it looked at, and how sure it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub score: f32,
    /// Centre and size.
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub points: Vec<(f32, f32)>,
}

impl Detection {
    fn overlap(&self, other: &Self) -> f32 {
        let ix = ((self.cx + self.w / 2.0).min(other.cx + other.w / 2.0)
            - (self.cx - self.w / 2.0).max(other.cx - other.w / 2.0))
        .max(0.0);
        let iy = ((self.cy + self.h / 2.0).min(other.cy + other.h / 2.0)
            - (self.cy - self.h / 2.0).max(other.cy - other.h / 2.0))
        .max(0.0);
        let inter = ix * iy;
        let union = self.w * self.h + other.w * other.h - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }
}

pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x.clamp(-100.0, 100.0)).exp())
}

/// A detector's outputs read: each anchor's box (`regressors`, `stride`
/// numbers an anchor: the box, then the points) and its score (a logit),
/// those over `threshold` kept, the ones overlapping a surer one by more
/// than 0.3 left out, the surest first.
pub fn detections(
    anchors: &Anchors,
    regressors: &[f32],
    scores: &[f32],
    stride: usize,
    size: f32,
    threshold: f32,
) -> Vec<Detection> {
    let mut found: Vec<Detection> = anchors
        .centres
        .iter()
        .enumerate()
        .filter_map(|(i, &(ax, ay))| {
            let score = sigmoid(*scores.get(i)?);
            if score < threshold {
                return None;
            }
            let r = regressors.get(i * stride..(i + 1) * stride)?;
            let (&[x, y, w, h], rest) = r.split_first_chunk::<4>()?;
            let points = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[px, py]| (ax + px / size, ay + py / size))
                .collect();
            Some(Detection {
                score,
                cx: ax + x / size,
                cy: ay + y / size,
                w: w / size,
                h: h / size,
                points,
            })
        })
        .collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::new();
    for d in found {
        if kept.iter().all(|k| k.overlap(&d) < 0.3) {
            kept.push(d);
        }
    }
    kept
}

/// The One Euro filter (Casiez, Roussel and Vogel, 2012): a value kept
/// still while it barely moves, followed closely as it moves fast.
#[derive(Debug, Clone, Copy)]
pub struct OneEuro {
    /// Its cut-off at rest (Hz), and how much speed raises it.
    pub min_cutoff: f32,
    pub beta: f32,
    value: Option<f32>,
    speed: f32,
}

impl OneEuro {
    pub const fn new(min_cutoff: f32, beta: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            value: None,
            speed: 0.0,
        }
    }

    fn alpha(cutoff: f32, dt: f32) -> f32 {
        let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        1.0 / (1.0 + tau / dt)
    }

    /// `x` seen `dt` seconds after the last: the value kept.
    pub fn filter(&mut self, x: f32, dt: f32) -> f32 {
        let Some(last) = self.value else {
            self.value = Some(x);
            return x;
        };
        let dt = dt.max(1e-3);
        let speed = (x - last) / dt;
        self.speed += (speed - self.speed) * Self::alpha(1.0, dt);
        let cutoff = self.min_cutoff + self.beta * self.speed.abs();
        let value = last + (x - last) * Self::alpha(cutoff, dt);
        self.value = Some(value);
        value
    }

    pub fn reset(&mut self) {
        self.value = None;
        self.speed = 0.0;
    }
}

/// A test picture (`data/`, 1280 × 720, public domain), RGBA.
#[cfg(any(test, feature = "test-picture"))]
pub fn test_picture(jpeg: &[u8]) -> Vec<u8> {
    image::load_from_memory(jpeg)
        .map(|i| i.to_rgba8().into_raw())
        .unwrap_or_default()
}

/// A thread's own time on the processor: a model's cost, whatever else
/// runs beside it.
#[cfg(test)]
pub fn thread_time() -> std::time::Duration {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    std::time::Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchors_are_mediapipe_s() {
        assert_eq!(Anchors::new(128, &[8, 16, 16, 16]).centres.len(), 896);
        assert_eq!(Anchors::new(192, &[8, 16, 16, 16]).centres.len(), 2016);
    }

    #[test]
    fn a_square_turned_maps_back() {
        let roi = Roi {
            cx: 100.0,
            cy: 50.0,
            size: 40.0,
            angle: std::f32::consts::FRAC_PI_2,
        };
        let (x, y) = roi.to_frame(1.0, 0.5);
        assert!((x - 100.0).abs() < 1e-4 && (y - 70.0).abs() < 1e-4);
        let back = Roi::around(&[(90.0, 40.0), (110.0, 60.0)], 0.0, 1.0, 0.0);
        assert_eq!(
            back,
            Some(Roi {
                cx: 100.0,
                cy: 50.0,
                size: 20.0,
                angle: 0.0
            })
        );
    }

    #[test]
    fn the_filter_holds_still_and_follows_a_move() {
        let mut f = OneEuro::new(1.0, 0.05);
        let dt = 1.0 / 30.0;
        f.filter(100.0, dt);
        let jitter: Vec<f32> = (0..30)
            .map(|i| f.filter(if i % 2 == 0 { 102.0 } else { 98.0 }, dt))
            .collect();
        assert!(jitter.iter().all(|v| (v - 100.0).abs() < 1.0), "{jitter:?}");
        let mut v = 0.0;
        for i in 0..15 {
            v = f.filter(100.0 + 20.0 * i as f32, dt);
        }
        assert!(v > 330.0, "{v}");
    }
}
