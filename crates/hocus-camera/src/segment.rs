//! Who is the person in the picture (lot 3): MediaPipe's selfie
//! segmentation, landscape, run with tract on the CPU.
//!
//! - **The model** (Apache-2.0, `models/LICENSE-MEDIAPIPE`): MediaPipe's,
//!   its graph written as ONNX by `scripts/camera/selfie-to-onnx.py`
//!   (tract reads no TensorFlow Lite holding MediaPipe's own operator),
//!   its weights unchanged. 432 KB: it is in the program, no download.
//! - **Its input**: the frame scaled to 256 × 144 (each pixel the mean of
//!   the 5 × 5 it covers), red, green, blue from 0 to 1, planar.
//! - **Its output**: each of those pixels' probability of being the
//!   person, from 0 to 1 ([`Mask`]).
//!
//! The mask is kept smooth between frames ([`Mask::follow`]): each
//! pixel moves part of the way to what the model says, faster where it
//! changes much (someone moving) than where it hesitates (an edge).

use tract_onnx::prelude::*;

/// The model, in the program.
static MODEL: &[u8] = include_bytes!("../models/selfie_segmentation_landscape.onnx");

/// The model's picture: 256 × 144.
pub const MASK_W: usize = 256;
pub const MASK_H: usize = 144;
const PIXELS: usize = MASK_W * MASK_H;

type Plan = std::sync::Arc<TypedSimplePlan>;

/// The model, ready to run.
pub struct Segmenter {
    plan: Plan,
    input: Vec<f32>,
}

impl std::fmt::Debug for Segmenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segmenter").finish_non_exhaustive()
    }
}

/// Each of the model's pixels' probability of being the person, from 0
/// to 1, rows from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub values: Vec<f32>,
}

impl Default for Mask {
    fn default() -> Self {
        Self {
            values: vec![0.0; PIXELS],
        }
    }
}

impl Mask {
    /// How much of the picture is the person, from 0 to 1.
    pub fn cover(&self) -> f32 {
        self.values.iter().sum::<f32>() / PIXELS as f32
    }

    /// A step towards `fresh`: a pixel that changed much takes most of the
    /// new value, one that barely did a little of it, so that an edge does
    /// not flicker and a move is followed.
    pub fn follow(&mut self, fresh: &Self) {
        for (old, new) in self.values.iter_mut().zip(&fresh.values) {
            let d = (new - *old).abs();
            let k = 0.35 + 0.6 * (d * 2.0).min(1.0);
            *old += (new - *old) * k;
        }
    }

    /// The mask at `(x, y)`, from 0 to 1 across the picture each way,
    /// between its pixels.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let fx = (x * MASK_W as f32 - 0.5).clamp(0.0, (MASK_W - 1) as f32);
        let fy = (y * MASK_H as f32 - 0.5).clamp(0.0, (MASK_H - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(MASK_W - 1), (y0 + 1).min(MASK_H - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let v = |x: usize, y: usize| self.values.get(y * MASK_W + x).copied().unwrap_or(0.0);
        let top = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
        let bottom = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

impl Segmenter {
    /// The model read and planned for its picture's size.
    pub fn new() -> TractResult<Self> {
        let plan = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))?
            .with_input_fact(0, f32::fact([1, 3, MASK_H, MASK_W]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            plan,
            input: vec![0.0; 3 * PIXELS],
        })
    }

    /// The person in the frame, `width` × `height` pixels of four bytes,
    /// red, green and blue at `order` (`[0, 1, 2]` for RGBA, `[2, 1, 0]`
    /// for B, G, R, x), into `mask`.
    pub fn run(
        &mut self,
        frame: &[u8],
        width: usize,
        height: usize,
        order: [usize; 3],
        mask: &mut Mask,
    ) -> TractResult<()> {
        self.scale(frame, width, height, order);
        let input = tract_ndarray::Array4::from_shape_vec(
            (1, 3, MASK_H, MASK_W),
            std::mem::take(&mut self.input),
        )?;
        let outputs = self.plan.run(tvec!(Tensor::from(input).into()))?;
        let Some(output) = outputs.first() else {
            anyhow::bail!("the model said nothing");
        };
        let view = output.to_plain_array_view::<f32>()?;
        mask.values.clear();
        mask.values.extend(view.iter().map(|v| v.clamp(0.0, 1.0)));
        mask.values.resize(PIXELS, 0.0);
        self.input = vec![0.0; 3 * PIXELS];
        Ok(())
    }

    /// The frame scaled to the model's picture, each pixel the mean of the
    /// ones it covers, planar, from 0 to 1.
    fn scale(&mut self, frame: &[u8], width: usize, height: usize, order: [usize; 3]) {
        let (sx, sy) = (width as f32 / MASK_W as f32, height as f32 / MASK_H as f32);
        let (planes, rest) = self.input.split_at_mut(PIXELS);
        let (greens, blues) = rest.split_at_mut(PIXELS);
        for my in 0..MASK_H {
            let y0 = (my as f32 * sy) as usize;
            let y1 = (((my + 1) as f32 * sy) as usize).clamp(y0 + 1, height.max(1));
            for mx in 0..MASK_W {
                let x0 = (mx as f32 * sx) as usize;
                let x1 = (((mx + 1) as f32 * sx) as usize).clamp(x0 + 1, width.max(1));
                let mut sum = [0u32; 3];
                for y in y0..y1 {
                    let row = y * width * 4;
                    let Some(run) = frame.get(row + x0 * 4..row + x1 * 4) else {
                        continue;
                    };
                    for px in run.as_chunks::<4>().0 {
                        for (s, &at) in sum.iter_mut().zip(&order) {
                            *s += u32::from(px.get(at).copied().unwrap_or(0));
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)).max(1) as f32 * 255.0;
                let at = my * MASK_W + mx;
                for (plane, s) in [&mut *planes, &mut *greens, &mut *blues]
                    .into_iter()
                    .zip(sum)
                {
                    if let Some(v) = plane.get_mut(at) {
                        *v = s as f32 / n;
                    }
                }
            }
        }
    }
}

/// The test picture of a person (`data/person.jpg`, NASA's, public
/// domain), RGBA, 1280 × 720.
#[cfg(any(test, feature = "test-picture"))]
pub fn test_person() -> Vec<u8> {
    static JPEG: &[u8] = include_bytes!("../data/person.jpg");
    image::load_from_memory(JPEG)
        .map(|i| i.to_rgba8().into_raw())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// The person's face and shoulders are the person, the wall in the
    /// corners is not; within a frame's budget on the CPU.
    #[test]
    fn the_model_finds_the_person_in_its_budget() -> TractResult<()> {
        let frame = test_person();
        assert_eq!(frame.len(), 1280 * 720 * 4);
        let mut segmenter = Segmenter::new()?;
        let mut mask = Mask::default();
        segmenter.run(&frame, 1280, 720, [0, 1, 2], &mut mask)?;
        let (w, h) = (MASK_W as f32, MASK_H as f32);
        let mean = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let mut sum = 0.0;
            let mut n = 0.0;
            for y in (y0 * h) as usize..(y1 * h) as usize {
                for x in (x0 * w) as usize..(x1 * w) as usize {
                    sum += mask.values[y * MASK_W + x];
                    n += 1.0;
                }
            }
            sum / n
        };
        // The face (the picture's 40 % to 55 % across, 30 % to 60 % down)
        // and the collar.
        assert!(mean(0.40, 0.30, 0.55, 0.60) > 0.95, "the face");
        assert!(mean(0.45, 0.85, 0.60, 1.0) > 0.9, "the collar");
        // The wall above the shuttle and the flag's top.
        assert!(mean(0.0, 0.0, 0.2, 0.4) < 0.05, "the wall");
        assert!(mean(0.82, 0.0, 1.0, 0.3) < 0.05, "the flag");
        let cover = mask.cover();
        assert!((0.15..0.6).contains(&cover), "{cover}");
        // Its budget: half a frame at 30 a second, as the median of a
        // few runs (the first plans its memory), in the thread's own time
        // on the processor (the other tests running beside it do not
        // count).
        let cpu = || {
            let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
            Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
        };
        let mut times: Vec<Duration> = (0..9)
            .map(|_| {
                let start = cpu();
                let _ = segmenter.run(&frame, 1280, 720, [0, 1, 2], &mut mask);
                cpu() - start
            })
            .collect();
        times.sort();
        let median = times[times.len() / 2];
        assert!(
            median < Duration::from_millis(16),
            "{median:?} a frame: {times:?}"
        );
        Ok(())
    }

    #[test]
    fn the_mask_follows_a_move_and_holds_an_edge() {
        let mut mask = Mask::default();
        let mut fresh = Mask::default();
        fresh.values[0] = 1.0;
        fresh.values[1] = 0.2;
        mask.follow(&fresh);
        assert!(mask.values[0] > 0.9, "a move: {}", mask.values[0]);
        assert!(mask.values[1] < 0.2 * 0.7, "a hesitation: {}", mask.values[1]);
        assert!((mask.at(0.0, 0.0) - mask.values[0]).abs() < 1e-6);
    }
}
