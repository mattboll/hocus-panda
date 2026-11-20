//! The hands (lot 4, Réactions › Gestes): MediaPipe's hand landmarker,
//! its two models run with tract on the CPU, as MediaPipe chains them,
//! and the two gestures read from a hand's points.
//!
//! - **The palm's detector** (192 × 192, the frame letterboxed): the
//!   palm's box, its wrist and its fingers' roots; run when asked (the
//!   caller says how often: rarely while no hand is in the picture).
//! - **The hand's points** (224 × 224): 21 points, in a square around the
//!   palm turned so that the fingers point up, then around the last
//!   frame's points (the hand followed), until it says no hand is there.
//! - **The gestures** ([`gesture`]): a thumb up (the thumb pointing up,
//!   above the hand, the four fingers folded), a hand raised (the four
//!   fingers open, pointing up, the hand high in the picture).

use tract_onnx::prelude::*;

use crate::track::{self, Anchors, Frame, Plan, Range, Roi};

static DETECTOR: &[u8] = include_bytes!("../models/hand_detector.onnx");
static LANDMARKS: &[u8] = include_bytes!("../models/hand_landmarks.onnx");

const DETECTOR_SIZE: usize = 192;
const LANDMARKS_SIZE: usize = 224;
/// A hand's points.
pub const POINTS: usize = 21;
/// At most this many hands are followed.
pub const MOST: usize = 2;
const THRESHOLD: f32 = 0.5;

/// The points (MediaPipe's numbering).
pub mod at {
    pub const WRIST: usize = 0;
    pub const THUMB_MCP: usize = 2;
    pub const THUMB_TIP: usize = 4;
    pub const INDEX_MCP: usize = 5;
    pub const MIDDLE_MCP: usize = 9;
    pub const RING_MCP: usize = 13;
    pub const MIDDLE_TIP: usize = 12;
    /// Each finger's (index, middle, ring, little) knuckle, middle joint
    /// and tip.
    pub const FINGERS: [(usize, usize, usize); 4] =
        [(5, 6, 8), (9, 10, 12), (13, 14, 16), (17, 18, 20)];
    /// The palm's points, which the square around the hand is of.
    pub const PALM: [usize; 12] = [0, 1, 2, 3, 5, 6, 9, 10, 13, 14, 17, 18];
}

/// A hand found: its 21 points in the frame's pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Hand {
    pub points: Vec<[f32; 3]>,
}

impl Hand {
    fn xy(&self, i: usize) -> (f32, f32) {
        let [x, y, _] = self.points.get(i).copied().unwrap_or_default();
        (x, y)
    }

    fn reach(&self, a: usize, b: usize) -> f32 {
        let ((ax, ay), (bx, by)) = (self.xy(a), self.xy(b));
        ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
    }

    /// The direction from `a` to `b`'s angle away from straight up
    /// (radians, 0 to π).
    fn off_up(&self, a: usize, b: usize) -> f32 {
        let ((ax, ay), (bx, by)) = (self.xy(a), self.xy(b));
        let (dx, dy) = (bx - ax, by - ay);
        // Up is the frame's -y.
        dx.atan2(-dy).abs()
    }
}

/// What a hand says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    ThumbUp,
    Raised,
}

/// A finger's tip this much farther from the wrist than its middle joint
/// is open; nearer than it, folded.
const OPEN: f32 = 1.15;
const FOLDED: f32 = 1.0;
/// Pointing up: within this of straight up (radians).
const UP: f32 = 0.7;
/// A hand raised: its middle finger's tip in this top part of the picture.
const HIGH: f32 = 0.6;

/// The gesture `hand` makes, in a frame `height` pixels tall.
pub fn gesture(hand: &Hand, height: f32) -> Option<Gesture> {
    let palm = hand.reach(at::WRIST, at::MIDDLE_MCP);
    if palm <= 1.0 {
        return None;
    }
    let ratio = |(_, joint, tip): (usize, usize, usize)| {
        hand.reach(at::WRIST, tip) / hand.reach(at::WRIST, joint).max(1e-3)
    };
    let fingers = at::FINGERS.map(ratio);
    let folded = fingers.iter().all(|&r| r < FOLDED);
    let open = fingers.iter().all(|&r| r > OPEN);
    let (_, thumb_y) = hand.xy(at::THUMB_TIP);
    let highest_else = (0..POINTS)
        .filter(|&i| i != at::THUMB_TIP && i != at::THUMB_TIP - 1)
        .map(|i| hand.xy(i).1)
        .fold(f32::MAX, f32::min);
    let thumb_out = hand.reach(at::THUMB_MCP, at::THUMB_TIP) > 0.45 * palm;
    if folded
        && thumb_out
        && hand.off_up(at::THUMB_MCP, at::THUMB_TIP) < UP
        && thumb_y < highest_else - 0.25 * palm
    {
        return Some(Gesture::ThumbUp);
    }
    let (_, tip_y) = hand.xy(at::MIDDLE_TIP);
    if open && hand.off_up(at::WRIST, at::MIDDLE_MCP) < UP && tip_y < HIGH * height {
        return Some(Gesture::Raised);
    }
    None
}

/// The hands' models, and the hands followed.
pub struct HandTracker {
    detector: Plan,
    landmarks: Plan,
    anchors: Anchors,
    /// The squares the points' model looks at next: the hands followed.
    rois: Vec<Roi>,
    input: Vec<f32>,
}

impl std::fmt::Debug for HandTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandTracker")
            .field("rois", &self.rois)
            .finish_non_exhaustive()
    }
}

impl HandTracker {
    pub fn new() -> TractResult<Self> {
        Ok(Self {
            detector: track::plan(DETECTOR, &[1, 3, DETECTOR_SIZE, DETECTOR_SIZE])?,
            landmarks: track::plan(LANDMARKS, &[1, 3, LANDMARKS_SIZE, LANDMARKS_SIZE])?,
            anchors: Anchors::new(DETECTOR_SIZE, &[8, 16, 16, 16]),
            rois: Vec::new(),
            input: Vec::new(),
        })
    }

    /// Whether a hand is followed.
    pub fn following(&self) -> bool {
        !self.rois.is_empty()
    }

    /// The hands in `frame`: those followed, and, when `detect`, those
    /// the palm's detector finds anew.
    pub fn track(&mut self, frame: &Frame<'_>, detect: bool) -> TractResult<Vec<Hand>> {
        let mut rois = std::mem::take(&mut self.rois);
        if detect && rois.len() < MOST {
            for roi in self.detect(frame)? {
                let new = rois.iter().all(|r| {
                    let d = ((r.cx - roi.cx).powi(2) + (r.cy - roi.cy).powi(2)).sqrt();
                    d > 0.5 * r.size.min(roi.size)
                });
                if new && rois.len() < MOST {
                    rois.push(roi);
                }
            }
        }
        let mut hands = Vec::new();
        for roi in rois {
            let Some(hand) = self.landmarks(frame, roi)? else {
                continue;
            };
            let palm: Vec<(f32, f32)> = at::PALM.iter().map(|&i| hand.xy(i)).collect();
            // From the wrist to between the middle finger's root and the
            // index's and ring's.
            let (ix, iy) = hand.xy(at::INDEX_MCP);
            let (rx, ry) = hand.xy(at::RING_MCP);
            let (mx, my) = hand.xy(at::MIDDLE_MCP);
            let towards = (((ix + rx) / 2.0 + mx) / 2.0, ((iy + ry) / 2.0 + my) / 2.0);
            let angle =
                track::angle_between(hand.xy(at::WRIST), towards, -std::f32::consts::FRAC_PI_2);
            if let Some(next) = Roi::around(&palm, angle, 2.0, -0.1) {
                // Two squares on one hand: the first kept.
                let twice = self.rois.iter().any(|r| {
                    ((r.cx - next.cx).powi(2) + (r.cy - next.cy).powi(2)).sqrt()
                        < 0.3 * r.size.min(next.size)
                });
                if !twice {
                    self.rois.push(next);
                    hands.push(hand);
                }
            }
        }
        Ok(hands)
    }

    /// The palms the detector finds: the squares the points' model looks
    /// at.
    fn detect(&mut self, frame: &Frame<'_>) -> TractResult<Vec<Roi>> {
        let whole = Roi::whole(frame.width, frame.height);
        let shape = [1, 3, DETECTOR_SIZE, DETECTOR_SIZE];
        whole.sample(frame, DETECTOR_SIZE, Range::Unit, &mut self.input);
        let out = track::run(&self.detector, &shape, std::mem::take(&mut self.input))?;
        let (Some(boxes), Some(scores)) = (out.first(), out.get(1)) else {
            anyhow::bail!("the palm's detector said nothing");
        };
        let found = track::detections(
            &self.anchors,
            boxes,
            scores,
            18,
            DETECTOR_SIZE as f32,
            THRESHOLD,
        );
        Ok(found
            .iter()
            .take(MOST)
            .filter_map(|palm| {
                let (&(wa, wb), &(ma, mb)) = (palm.points.first()?, palm.points.get(2)?);
                let angle = track::angle_between(
                    whole.to_frame(wa, wb),
                    whole.to_frame(ma, mb),
                    -std::f32::consts::FRAC_PI_2,
                );
                let (cx, cy) = whole.to_frame(palm.cx, palm.cy);
                let (w, h) = (palm.w * whole.size, palm.h * whole.size);
                // Half its height towards the fingers, its long side 2.6
                // times.
                let (s, c) = angle.sin_cos();
                let shift = -0.5 * h;
                Some(Roi {
                    cx: cx - s * shift,
                    cy: cy + c * shift,
                    size: w.max(h) * 2.6,
                    angle,
                })
            })
            .collect())
    }

    /// The hand's points in `roi`, in the frame; `None` if no hand is
    /// there.
    fn landmarks(&mut self, frame: &Frame<'_>, roi: Roi) -> TractResult<Option<Hand>> {
        let shape = [1, 3, LANDMARKS_SIZE, LANDMARKS_SIZE];
        roi.sample(frame, LANDMARKS_SIZE, Range::Unit, &mut self.input);
        let out = track::run(&self.landmarks, &shape, std::mem::take(&mut self.input))?;
        let (Some(points), Some(presence)) = (out.first(), out.get(1)) else {
            anyhow::bail!("the hand's model said nothing");
        };
        if presence.first().copied().unwrap_or(0.0) < THRESHOLD {
            return Ok(None);
        }
        let n = LANDMARKS_SIZE as f32;
        let scale = roi.size / n;
        Ok(Some(Hand {
            points: points
                .as_chunks::<3>()
                .0
                .iter()
                .take(POINTS)
                .map(|&[x, y, z]| {
                    let (fx, fy) = roi.to_frame(x / n, y / n);
                    [fx, fy, z * scale]
                })
                .collect(),
        }))
    }
}

/// The test pictures of hands (`data/`, NASA's, public domain), RGBA,
/// 1280 × 720: a thumb up, and a hand raised.
#[cfg(any(test, feature = "test-picture"))]
pub fn test_thumb_up() -> Vec<u8> {
    track::test_picture(include_bytes!("../data/thumb_up.jpg"))
}

#[cfg(any(test, feature = "test-picture"))]
pub fn test_raised_hand() -> Vec<u8> {
    track::test_picture(include_bytes!("../data/raised_hand.jpg"))
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

    fn seen(picture: &[u8]) -> TractResult<(Vec<Hand>, HandTracker)> {
        let mut tracker = HandTracker::new()?;
        let hands = tracker.track(&frame(picture), true)?;
        Ok((hands, tracker))
    }

    /// A thumb up, where the picture's hand is (its thumb's tip near
    /// (240, 360)); followed, within its budget on a real machine.
    #[test]
    fn a_thumb_up_is_a_thumb_up_in_its_budget() -> TractResult<()> {
        let picture = test_thumb_up();
        let (hands, mut tracker) = seen(&picture)?;
        let hand = hands.first().expect("a hand");
        let (x, y) = hand.xy(at::THUMB_TIP);
        assert!(
            (x - 240.0).abs() < 50.0 && (y - 370.0).abs() < 50.0,
            "{x}, {y}"
        );
        assert_eq!(gesture(hand, 720.0), Some(Gesture::ThumbUp));
        // Followed: the points' model alone keeps the thumb up where it
        // is; within half of the 100 ms between two looks (the gestures
        // look ten times a second) on a real machine (`HOCUS_TEST_BUDGETS=1`).
        let mut times = Vec::new();
        for _ in 0..7 {
            let start = track::thread_time();
            let followed = tracker.track(&frame(&picture), false)?;
            times.push(track::thread_time() - start);
            let hand = followed.first().expect("the hand followed");
            let (x, y) = hand.xy(at::THUMB_TIP);
            assert!(
                (x - 240.0).abs() < 50.0 && (y - 370.0).abs() < 50.0,
                "{x}, {y}"
            );
            assert_eq!(gesture(hand, 720.0), Some(Gesture::ThumbUp));
        }
        track::within_budget("the hand's points", times, Duration::from_millis(50));
        assert!(tracker.following());
        Ok(())
    }

    /// A hand raised, open, high in the picture.
    #[test]
    fn a_raised_hand_is_a_raised_hand() -> TractResult<()> {
        let picture = test_raised_hand();
        let (hands, _) = seen(&picture)?;
        let raised: Vec<_> = hands.iter().filter_map(|h| gesture(h, 720.0)).collect();
        assert_eq!(raised, [Gesture::Raised], "{hands:?}");
        Ok(())
    }

    /// The face's portrait has no hand, nor a gesture.
    #[test]
    fn a_portrait_has_no_gesture() -> TractResult<()> {
        let picture = crate::segment::test_person();
        let (hands, _) = seen(&picture)?;
        assert!(
            hands.iter().all(|h| gesture(h, 720.0).is_none()),
            "{hands:?}"
        );
        Ok(())
    }
}
