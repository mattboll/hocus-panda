//! The face (lot 4): MediaPipe's face landmarker, its three models run
//! with tract on the CPU, as MediaPipe chains them.
//!
//! - **The detector** (BlazeFace, short range, 128 × 128, the frame
//!   letterboxed): the face's box and its eyes, only while no face is
//!   followed.
//! - **The mesh** (256 × 256): 478 points of the face (its irises among
//!   them), in a square around it turned so that the eyes are level: from
//!   the detector's box, then from the last frame's points (the face
//!   followed from frame to frame), and found again by the detector when
//!   the mesh says no face is there.
//! - **The expressions** (52 blendshapes, from 146 of the points): blinks
//!   and the mouth, for the panda that will take the person's place; run
//!   only when asked.
//!
//! The points are kept still between frames ([`OneEuro`]); the head's
//! pose ([`Pose`]) is read from them.

use tract_onnx::prelude::*;

use crate::track::{self, Anchors, Frame, OneEuro, Plan, Range, Roi};

static DETECTOR: &[u8] = include_bytes!("../models/face_detector.onnx");
static MESH: &[u8] = include_bytes!("../models/face_landmarks.onnx");
static BLENDSHAPES: &[u8] = include_bytes!("../models/face_blendshapes.onnx");

const DETECTOR_SIZE: usize = 128;
const MESH_SIZE: usize = 256;
/// The mesh's points.
pub const POINTS: usize = 478;
/// The square around the face: its box times this.
const ROI_SCALE: f32 = 1.5;
/// A face is there over this (the detector's score, the mesh's).
const THRESHOLD: f32 = 0.5;

/// Some of the mesh's points (MediaPipe's numbering; the person's right
/// is the picture's left).
pub mod at {
    pub const FOREHEAD: usize = 10;
    pub const CHIN: usize = 152;
    pub const NOSE: usize = 1;
    pub const RIGHT_EYE_OUT: usize = 33;
    pub const RIGHT_EYE_IN: usize = 133;
    pub const LEFT_EYE_IN: usize = 362;
    pub const LEFT_EYE_OUT: usize = 263;
    pub const RIGHT_CHEEK: usize = 234;
    pub const LEFT_CHEEK: usize = 454;
}

/// The points the expressions' model reads (MediaPipe's
/// `kLandmarksSubsetIdxs`).
const SUBSET: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65,
    66, 67, 70, 78, 80, 81, 82, 84, 87, 88, 91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136,
    144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159, 160, 161, 162, 163, 168, 172,
    173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251, 263, 267, 269, 270, 276, 282, 283,
    284, 285, 288, 291, 293, 295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318, 321, 323, 324,
    332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378, 379, 380, 381, 382, 384, 385,
    386, 387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472,
    473, 474, 475, 476, 477,
];

/// Some of the 52 expressions (MediaPipe's order).
pub mod shape {
    pub const EYE_BLINK_LEFT: usize = 9;
    pub const EYE_BLINK_RIGHT: usize = 10;
    pub const JAW_OPEN: usize = 25;
    pub const MOUTH_SMILE_LEFT: usize = 44;
    pub const MOUTH_SMILE_RIGHT: usize = 45;
}

/// A face found: its points in the frame's pixels (`x`, `y`, and a depth
/// `z` on the same scale, smaller nearer), smoothed between frames, and
/// its expressions when asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub points: Vec<[f32; 3]>,
    pub expressions: Option<Vec<f32>>,
}

/// Where the head is and how it is turned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Between the eyes, frame pixels.
    pub eyes: (f32, f32),
    /// From one eye's middle to the other's, frame pixels, in depth too
    /// (so that it barely changes as the head turns aside).
    pub span: f32,
    /// The eyes' line's angle (radians, the frame's y going down).
    pub roll: f32,
    /// The face's middle (between its forehead and chin, its cheeks).
    pub centre: (f32, f32),
    /// From forehead to chin, frame pixels.
    pub height: f32,
    /// The forehead's top, at the hair (frame pixels).
    pub forehead: (f32, f32),
    /// From one side of the face to the other (its temples), frame
    /// pixels, in depth too.
    pub width: f32,
}

impl Face {
    fn point(&self, i: usize) -> [f32; 3] {
        self.points.get(i).copied().unwrap_or_default()
    }

    fn mid(&self, a: usize, b: usize) -> [f32; 3] {
        let (p, q) = (self.point(a), self.point(b));
        [0, 1, 2]
            .map(|k| (p.get(k).copied().unwrap_or(0.0) + q.get(k).copied().unwrap_or(0.0)) / 2.0)
    }

    pub fn pose(&self) -> Pose {
        let right = self.mid(at::RIGHT_EYE_OUT, at::RIGHT_EYE_IN);
        let left = self.mid(at::LEFT_EYE_IN, at::LEFT_EYE_OUT);
        let [rx, ry, rz] = right;
        let [lx, ly, lz] = left;
        let [fx, fy, _] = self.point(at::FOREHEAD);
        let [cx, cy, _] = self.point(at::CHIN);
        let [ax, ay, az] = self.point(at::RIGHT_CHEEK);
        let [bx, by, bz] = self.point(at::LEFT_CHEEK);
        Pose {
            eyes: ((rx + lx) / 2.0, (ry + ly) / 2.0),
            span: ((lx - rx).powi(2) + (ly - ry).powi(2) + (lz - rz).powi(2)).sqrt(),
            roll: (ly - ry).atan2(lx - rx),
            centre: ((ax + bx) / 2.0, (fy + cy) / 2.0),
            height: ((cx - fx).powi(2) + (cy - fy).powi(2)).sqrt(),
            forehead: (fx, fy),
            width: ((bx - ax).powi(2) + (by - ay).powi(2) + (bz - az).powi(2)).sqrt(),
        }
    }

    /// An expression's value, 0 to 1 (`shape`'s), if they were asked.
    pub fn expression(&self, i: usize) -> Option<f32> {
        self.expressions.as_ref()?.get(i).copied()
    }
}

/// The face's models, and the face followed.
pub struct FaceTracker {
    detector: Plan,
    mesh: Plan,
    blendshapes: Option<Plan>,
    anchors: Anchors,
    /// The square the mesh looks at next: the last face's.
    roi: Option<Roi>,
    input: Vec<f32>,
    filters: Vec<OneEuro>,
}

impl std::fmt::Debug for FaceTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceTracker")
            .field("roi", &self.roi)
            .finish_non_exhaustive()
    }
}

/// The points' filter: still within a pixel or two at rest, following a
/// move at once.
fn filter() -> OneEuro {
    OneEuro::new(1.2, 0.03)
}

impl FaceTracker {
    pub fn new() -> TractResult<Self> {
        Ok(Self {
            detector: track::plan(DETECTOR, &[1, 3, DETECTOR_SIZE, DETECTOR_SIZE])?,
            mesh: track::plan(MESH, &[1, 3, MESH_SIZE, MESH_SIZE])?,
            blendshapes: None,
            anchors: Anchors::new(DETECTOR_SIZE, &[8, 16, 16, 16]),
            roi: None,
            input: Vec::new(),
            filters: vec![filter(); POINTS * 3],
        })
    }

    /// The face in `frame`, `dt` seconds after the last one looked at;
    /// its expressions too when `expressions`. `None`: no face.
    pub fn track(
        &mut self,
        frame: &Frame<'_>,
        dt: f32,
        expressions: bool,
    ) -> TractResult<Option<Face>> {
        let mut raw = None;
        if let Some(roi) = self.roi {
            raw = self.mesh(frame, roi)?;
        }
        if raw.is_none() {
            self.reset();
            if let Some(roi) = self.detect(frame)? {
                raw = self.mesh(frame, roi)?;
            }
        }
        let Some(points) = raw else {
            self.reset();
            return Ok(None);
        };
        // The next frame's square, from these points.
        let xy: Vec<(f32, f32)> = points.iter().map(|&[x, y, _]| (x, y)).collect();
        let roll = {
            let (a, b) = (
                points.get(at::RIGHT_EYE_OUT).copied().unwrap_or_default(),
                points.get(at::LEFT_EYE_OUT).copied().unwrap_or_default(),
            );
            let ([ax, ay, _], [bx, by, _]) = (a, b);
            track::angle_between((ax, ay), (bx, by), 0.0)
        };
        self.roi = Roi::around(&xy, roll, ROI_SCALE, 0.0);
        let expressions = if expressions {
            Some(self.expressions(&points)?)
        } else {
            None
        };
        let smooth = points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut q = *p;
                for (k, v) in q.iter_mut().enumerate() {
                    if let Some(f) = self.filters.get_mut(i * 3 + k) {
                        *v = f.filter(*v, dt);
                    }
                }
                q
            })
            .collect();
        Ok(Some(Face {
            points: smooth,
            expressions,
        }))
    }

    /// The face lost: found again by the detector, and its points'
    /// filters start again.
    pub fn reset(&mut self) {
        self.roi = None;
        for f in &mut self.filters {
            f.reset();
        }
    }

    /// The surest face's square, from the detector.
    fn detect(&mut self, frame: &Frame<'_>) -> TractResult<Option<Roi>> {
        let whole = Roi::whole(frame.width, frame.height);
        let shape = [1, 3, DETECTOR_SIZE, DETECTOR_SIZE];
        whole.sample(frame, DETECTOR_SIZE, Range::Signed, &mut self.input);
        let out = track::run(&self.detector, &shape, std::mem::take(&mut self.input))?;
        let (Some(boxes), Some(scores)) = (out.first(), out.get(1)) else {
            anyhow::bail!("the face's detector said nothing");
        };
        let found = track::detections(
            &self.anchors,
            boxes,
            scores,
            16,
            DETECTOR_SIZE as f32,
            THRESHOLD,
        );
        let Some(face) = found.first() else {
            return Ok(None);
        };
        // Its box and its eyes, in the frame.
        let corners = [
            (face.cx - face.w / 2.0, face.cy - face.h / 2.0),
            (face.cx + face.w / 2.0, face.cy + face.h / 2.0),
        ]
        .map(|(a, b)| whole.to_frame(a, b));
        let (Some(&(ra, rb)), Some(&(la, lb))) = (face.points.first(), face.points.get(1)) else {
            return Ok(None);
        };
        let angle = track::angle_between(whole.to_frame(ra, rb), whole.to_frame(la, lb), 0.0);
        let [(x0, y0), (x1, y1)] = corners;
        // The box's centre, the square turned with the eyes.
        Ok(Some(Roi {
            cx: (x0 + x1) / 2.0,
            cy: (y0 + y1) / 2.0,
            size: (x1 - x0).max(y1 - y0) * ROI_SCALE,
            angle,
        }))
    }

    /// The mesh's points in `roi`, in the frame; `None` if it sees no face.
    fn mesh(&mut self, frame: &Frame<'_>, roi: Roi) -> TractResult<Option<Vec<[f32; 3]>>> {
        let shape = [1, 3, MESH_SIZE, MESH_SIZE];
        roi.sample(frame, MESH_SIZE, Range::Unit, &mut self.input);
        let out = track::run(&self.mesh, &shape, std::mem::take(&mut self.input))?;
        let (Some(points), Some(presence)) = (out.first(), out.get(1)) else {
            anyhow::bail!("the face's mesh said nothing");
        };
        let there = presence.first().copied().map_or(0.0, track::sigmoid);
        if there < THRESHOLD {
            return Ok(None);
        }
        let n = MESH_SIZE as f32;
        let scale = roi.size / n;
        Ok(Some(
            points
                .as_chunks::<3>()
                .0
                .iter()
                .take(POINTS)
                .map(|&[x, y, z]| {
                    let (fx, fy) = roi.to_frame(x / n, y / n);
                    [fx, fy, z * scale]
                })
                .collect(),
        ))
    }

    /// The 52 expressions, from the mesh's points.
    fn expressions(&mut self, points: &[[f32; 3]]) -> TractResult<Vec<f32>> {
        if self.blendshapes.is_none() {
            self.blendshapes = Some(track::plan(BLENDSHAPES, &[1, SUBSET.len(), 2])?);
        }
        let Some(plan) = &self.blendshapes else {
            return Ok(Vec::new());
        };
        let input = SUBSET
            .iter()
            .flat_map(|&i| {
                let [x, y, _] = points.get(i).copied().unwrap_or_default();
                [x, y]
            })
            .collect();
        let out = track::run(plan, &[1, SUBSET.len(), 2], input)?;
        Ok(out.into_iter().next().unwrap_or_default())
    }
}

/// The test picture of a person (`data/person.jpg`, NASA's portrait of
/// Jeffrey Hoffman, public domain), RGBA, 1280 × 720.
#[cfg(any(test, feature = "test-picture"))]
pub fn test_face() -> Vec<u8> {
    crate::segment::test_person()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn frame(pixels: &[u8]) -> Frame<'_> {
        Frame {
            pixels,
            width: 1280,
            height: 720,
            order: [0, 1, 2],
        }
    }

    /// The portrait's eyes, nose and chin where they are (measured on the
    /// picture: his eyes at about (488, 368) and (620, 357)); level;
    /// smiling, eyes open; the face followed within its budget.
    #[test]
    fn the_face_s_points_are_found_in_their_budget() -> TractResult<()> {
        let picture = test_face();
        let mut tracker = FaceTracker::new()?;
        let face = tracker
            .track(&frame(&picture), 1.0 / 30.0, true)?
            .expect("a face");
        assert_eq!(face.points.len(), POINTS);
        let pose = face.pose();
        let near = |(x, y): (f32, f32), (ex, ey): (f32, f32), d: f32| {
            ((x - ex).powi(2) + (y - ey).powi(2)).sqrt() < d
        };
        assert!(near(pose.eyes, (554.0, 362.0), 15.0), "{pose:?}");
        assert!((110.0..160.0).contains(&pose.span), "{pose:?}");
        assert!(pose.roll.abs() < 0.15, "{pose:?}");
        let [_, chin, _] = face.points[at::CHIN];
        assert!((590.0..680.0).contains(&chin), "the chin at {chin}");
        let smile = face.expression(shape::MOUTH_SMILE_LEFT).unwrap_or(0.0);
        let blink = face.expression(shape::EYE_BLINK_LEFT).unwrap_or(1.0);
        assert!(smile > 0.3 && blink < 0.5, "smile {smile}, blink {blink}");
        // Followed: the mesh alone, each frame, within a frame at 30 a
        // second, on its thread (the median of a few, in the thread's own
        // time on the processor).
        let mut times: Vec<Duration> = (0..9)
            .map(|_| {
                let start = track::thread_time();
                let _ = tracker.track(&frame(&picture), 1.0 / 30.0, false);
                track::thread_time() - start
            })
            .collect();
        times.sort();
        let median = times[times.len() / 2];
        assert!(median < Duration::from_millis(30), "{median:?}: {times:?}");
        Ok(())
    }

    /// Nobody: no face, and the detector alone runs.
    #[test]
    fn an_empty_picture_has_no_face() -> TractResult<()> {
        let picture = vec![90u8; 1280 * 720 * 4];
        let mut tracker = FaceTracker::new()?;
        assert!(
            tracker
                .track(&frame(&picture), 1.0 / 30.0, false)?
                .is_none()
        );
        Ok(())
    }
}
