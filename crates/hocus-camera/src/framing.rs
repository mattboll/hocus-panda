//! Image › Cadrage (lot 4): the picture cropped round the face and
//! following it, 16:9 kept, so that the face stays in the middle.
//!
//! - **Where**: the face's middle at the picture's middle across and at
//!   [`AT_Y`] of its height; zoomed [`ZOOM`] times (the mockup's
//!   `scale(1.32)`), less when the face is near, so that its head and
//!   some of the shoulders stay in.
//! - **Still**: a face moving less than [`STILL`] of the picture moves
//!   nothing (no jitter); further, the crop follows it over about
//!   [`FOLLOW`] (no jump). Without a face for [`LOST`], the whole picture
//!   comes back, as slowly.
//! - **Drawn** on the made frame, between its pixels: after what is
//!   behind the person and their accessory, before the reactions and the
//!   banner (which stay where they are in the picture).

use std::time::Duration;

use crate::{
    face::Pose,
    look::{BYTES, H, W},
};

/// The mockup's zoom.
pub const ZOOM: f32 = 1.32;
/// The face's middle at this part of the picture's height.
pub const AT_Y: f32 = 0.45;
/// The crop's height is at least this many times the face's (from its
/// forehead to its chin): the head and the shoulders' top stay in.
const ROOM: f32 = 2.2;
/// A move under this part of the picture is none.
pub const STILL: f32 = 0.04;
/// The crop goes most of the way to where it is wanted in about this
/// (a time constant: two thirds of the way).
pub const FOLLOW: Duration = Duration::from_millis(450);
/// Without a face this long, the whole picture again.
pub const LOST: Duration = Duration::from_millis(1500);

/// Where the crop is: its middle (frame pixels) and its zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub cx: f32,
    pub cy: f32,
    pub zoom: f32,
}

impl Window {
    const WHOLE: Self = Self {
        cx: W as f32 / 2.0,
        cy: H as f32 / 2.0,
        zoom: 1.0,
    };

    /// Its size, frame pixels.
    pub fn size(&self) -> (f32, f32) {
        (W as f32 / self.zoom, H as f32 / self.zoom)
    }

    /// Kept in the frame.
    fn inside(mut self) -> Self {
        self.zoom = self.zoom.max(1.0);
        let (w, h) = self.size();
        self.cx = self.cx.clamp(w / 2.0, W as f32 - w / 2.0);
        self.cy = self.cy.clamp(h / 2.0, H as f32 - h / 2.0);
        self
    }

    /// For a face at `pose`.
    pub fn for_face(pose: &Pose) -> Self {
        let zoom = ZOOM.min(H as f32 / (ROOM * pose.height.max(1.0))).max(1.0);
        let h = H as f32 / zoom;
        Self {
            cx: pose.centre.0,
            cy: pose.centre.1 + (0.5 - AT_Y) * h,
            zoom,
        }
        .inside()
    }

    /// Whether `other` is within [`STILL`] of it.
    fn near(&self, other: &Self) -> bool {
        let (w, h) = self.size();
        (self.cx - other.cx).abs() < STILL * w
            && (self.cy - other.cy).abs() < STILL * h
            && (self.zoom - other.zoom).abs() < STILL * self.zoom
    }
}

/// Cadrage's state: where the crop is, where it goes, and how long the
/// face has been away.
#[derive(Debug, Clone)]
pub struct Framing {
    pub now: Window,
    goal: Window,
    unseen: Duration,
    /// The frame before the crop.
    source: Vec<u8>,
    /// For each of the frame's columns and rows, the source's pixel and
    /// how far towards the next (of 256).
    columns: Vec<(usize, u32)>,
    rows: Vec<(usize, u32)>,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            now: Window::WHOLE,
            goal: Window::WHOLE,
            unseen: Duration::ZERO,
            source: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }
}

impl Framing {
    /// A frame `dt` after the last: the face at `pose` (`None`: none seen
    /// in it).
    pub fn update(&mut self, pose: Option<&Pose>, dt: Duration) {
        match pose {
            Some(pose) => {
                self.unseen = Duration::ZERO;
                let wanted = Window::for_face(pose);
                if !self.goal.near(&wanted) {
                    self.goal = wanted;
                }
            }
            None => {
                self.unseen += dt;
                if self.unseen >= LOST {
                    self.goal = Window::WHOLE;
                }
            }
        }
        let k = 1.0 - (-dt.as_secs_f32() / FOLLOW.as_secs_f32()).exp();
        let towards = |a: f32, b: f32| a + (b - a) * k;
        self.now = Window {
            cx: towards(self.now.cx, self.goal.cx),
            cy: towards(self.now.cy, self.goal.cy),
            zoom: towards(self.now.zoom, self.goal.zoom),
        }
        .inside();
    }

    /// Where the crop goes.
    pub fn goal(&self) -> Window {
        self.goal
    }

    /// Back to the whole picture at once (Cadrage turned off, the camera
    /// let go).
    pub fn reset(&mut self) {
        self.now = Window::WHOLE;
        self.goal = Window::WHOLE;
        self.unseen = Duration::ZERO;
    }

    /// `frame` (B, G, R, x) cropped to the window and scaled back to the
    /// frame's size, between its pixels.
    pub fn apply(&mut self, frame: &mut [u8]) {
        if self.now.zoom <= 1.0005 || frame.len() < BYTES {
            return;
        }
        let (w, h) = self.now.size();
        let (x0, y0) = (self.now.cx - w / 2.0, self.now.cy - h / 2.0);
        let spans = |n: usize, from: f32, size: f32, most: usize, out: &mut Vec<(usize, u32)>| {
            out.clear();
            out.extend((0..n).map(|i| {
                let f =
                    (from + (i as f32 + 0.5) * size / n as f32 - 0.5).clamp(0.0, (most - 1) as f32);
                let a = (f as usize).min(most - 2);
                (a, ((f - a as f32) * 256.0) as u32)
            }));
        };
        spans(W, x0, w, W, &mut self.columns);
        spans(H, y0, h, H, &mut self.rows);
        self.source.clear();
        self.source
            .extend_from_slice(frame.get(..BYTES).unwrap_or_default());
        let stride = W * 4;
        for (row, &(sy, ty)) in frame.chunks_exact_mut(stride).zip(&self.rows) {
            let (Some(top), Some(bottom)) = (
                self.source.get(sy * stride..(sy + 1) * stride),
                self.source.get((sy + 1) * stride..(sy + 2) * stride),
            ) else {
                continue;
            };
            for (px, &(sx, tx)) in row.as_chunks_mut::<4>().0.iter_mut().zip(&self.columns) {
                let at = |line: &[u8], x: usize| -> [u32; 3] {
                    let p = line.get(x * 4..x * 4 + 3).unwrap_or(&[0, 0, 0]);
                    [0, 1, 2].map(|c| u32::from(p.get(c).copied().unwrap_or(0)))
                };
                let (a, b, c, d) = (
                    at(top, sx),
                    at(top, sx + 1),
                    at(bottom, sx),
                    at(bottom, sx + 1),
                );
                for (ch, out) in px.iter_mut().take(3).enumerate() {
                    let (a, b, c, d) = (
                        a.get(ch).copied().unwrap_or(0),
                        b.get(ch).copied().unwrap_or(0),
                        c.get(ch).copied().unwrap_or(0),
                        d.get(ch).copied().unwrap_or(0),
                    );
                    let upper = a * (256 - tx) + b * tx;
                    let lower = c * (256 - tx) + d * tx;
                    *out = ((upper * (256 - ty) + lower * ty + (1 << 15)) >> 16) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{face::FaceTracker, track::Frame};

    /// The portrait at `scale` (as if further away), its middle moved
    /// by (`dx`, `dy`), the rest the wall's grey.
    fn placed(picture: &[u8], scale: f32, dx: f32, dy: f32) -> Vec<u8> {
        let mut out = vec![0u8; BYTES];
        let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
        for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = ((i % W) as f32 - dx - cx) / scale + cx;
            let y = ((i / W) as f32 - dy - cy) / scale + cy;
            *px = if (0.0..W as f32).contains(&x) && (0.0..H as f32).contains(&y) {
                let at = (y as usize * W + x as usize) * 4;
                [picture[at], picture[at + 1], picture[at + 2], 255]
            } else {
                [140, 136, 128, 255]
            };
        }
        out
    }

    fn bgrx(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect()
    }

    fn rgba(bgrx: &[u8]) -> Vec<u8> {
        bgrx.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[b, g, r, _]| [r, g, b, 255])
            .collect()
    }

    fn frame(pixels: &[u8]) -> Frame<'_> {
        Frame {
            pixels,
            width: W,
            height: H,
            order: [0, 1, 2],
        }
    }

    /// The face where it is in the picture, and where Cadrage puts it, as
    /// the face's model sees them.
    fn framed(framing: &mut Framing, picture: &[u8]) -> anyhow::Result<((f32, f32), (f32, f32))> {
        let mut tracker = FaceTracker::new()?;
        let pose = tracker
            .track(&frame(picture), 1.0 / 30.0, false)?
            .expect("a face")
            .pose();
        // Two seconds of frames.
        for _ in 0..60 {
            framing.update(Some(&pose), Duration::from_millis(33));
        }
        let mut made = bgrx(picture);
        framing.apply(&mut made);
        let shown = rgba(&made);
        let mut again = FaceTracker::new()?;
        let after = again
            .track(&frame(&shown), 1.0 / 30.0, false)?
            .expect("a face, cropped")
            .pose();
        Ok((pose.centre, after.centre))
    }

    /// Wherever the face is in the picture (as far as the crop can
    /// reach), Cadrage puts it in the middle, zoomed in; a small move
    /// moves nothing.
    #[test]
    fn cadrage_keeps_the_face_in_the_middle() -> anyhow::Result<()> {
        let person = crate::face::test_face();
        let mut framing = Framing::default();
        // The portrait at 0.6: a face of a call's size.
        for (dx, dy) in [(0.0, 0.0), (-140.0, -60.0), (130.0, -120.0)] {
            let picture = placed(&person, 0.6, dx, dy);
            let (before, after) = framed(&mut framing, &picture)?;
            let (tx, ty) = (W as f32 / 2.0, AT_Y * H as f32);
            assert!(
                (after.0 - tx).abs() < 0.05 * W as f32 && (after.1 - ty).abs() < 0.06 * H as f32,
                "moved by ({dx}, {dy}): the face from {before:?} to {after:?}"
            );
            assert!(framing.now.zoom > 1.2, "{:?}", framing.now);
        }
        // Still: a move of a few pixels moves nothing.
        let held = framing.goal();
        let mut tracker = FaceTracker::new()?;
        let nudged = placed(&person, 0.6, 134.0, -116.0);
        let pose = tracker
            .track(&frame(&nudged), 1.0 / 30.0, false)?
            .expect("a face")
            .pose();
        for _ in 0..30 {
            framing.update(Some(&pose), Duration::from_millis(33));
        }
        assert_eq!(framing.goal(), held);
        // A real picture: a person further away, off the middle and
        // near the top. Cadrage zooms in and brings them to the middle
        // across (the frame's top keeps them a little higher).
        let far = crate::track::test_picture(include_bytes!("../data/far.jpg"));
        let mut on_far = Framing::default();
        let (before, after) = framed(&mut on_far, &far)?;
        assert!(on_far.now.zoom > 1.2, "{:?}", on_far.now);
        assert!(
            (after.0 - W as f32 / 2.0).abs()
                < (before.0 - W as f32 / 2.0).abs().max(0.03 * W as f32),
            "the face from {before:?} to {after:?}"
        );
        // No face: the whole picture comes back.
        for _ in 0..200 {
            framing.update(None, Duration::from_millis(33));
        }
        assert!((framing.now.zoom - 1.0).abs() < 0.01, "{:?}", framing.now);
        Ok(())
    }
}
