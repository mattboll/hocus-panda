//! Lumière, from the frame's statistics, no model: the same arithmetic on
//! the CPU ([`Cpu`]) and on the GPU ([`crate::gpu`]).
//!
//! - **Statistics** ([`stats`]): the mean of each channel over the frame,
//!   every eighth pixel each way; and the light on the face, the mean
//!   brightness of the picture's centre (no face is looked for: the user
//!   sits in front of their camera).
//! - **White balance**: grey-world, each channel's gain bringing its mean
//!   to the frame's brightness, taken at 70 % and held within ¾ and 4⁄3,
//!   so that a room's colour stays a little.
//! - **Warmth**: then red raised and blue lowered by 2 %, up to 6 % when
//!   the picture was cold (blue over red).
//! - **Exposure**: a curve `e·x / (1 + (e − 1)·x)` that keeps black and
//!   white where they are, its `e` taking the balanced mean brightness to
//!   [`TARGET`], within 0.6 and 3.
//! - **Denoise**: over time, each pixel 40 % the new frame and 60 % the
//!   last one while it barely changes, all of the new one as it moves.
//!
//! The correction follows the statistics smoothly ([`Correction::towards`])
//! so that the picture never flickers. Values are in the frames' own
//! encoding (sRGB's), from 0 to 1.

use hocus_link::camera::CAMERA_SIZE;

/// The frames' size, pixels.
pub const W: usize = CAMERA_SIZE.0 as usize;
pub const H: usize = CAMERA_SIZE.1 as usize;
/// A frame's bytes, four a pixel.
pub const BYTES: usize = W * H * 4;

/// The brightness Lumière takes a frame's mean to.
pub const TARGET: f32 = 0.46;
/// How far a frame moves the correction: about a fifth of a second at 30
/// frames a second.
pub const FOLLOW: f32 = 0.15;

/// The denoise: a pixel's channels changing less than this (of 255) are
/// still, more than [`MOVING`] are moving.
pub const STILL: f32 = 5.0;
pub const MOVING: f32 = 20.0;
/// A still pixel takes this much of the new frame.
pub const KEEP_NEW: f32 = 0.4;

/// The picture's centre, where the face is: a part of its width and
/// height.
pub const FACE_X: (f32, f32) = (0.35, 0.65);
pub const FACE_Y: (f32, f32) = (0.2, 0.6);

/// What a frame's pixels say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// Each channel's mean, red, green, blue.
    pub mean: [f32; 3],
    /// The centre's mean brightness.
    pub face: f32,
}

/// Rec. 601's brightness of a colour.
pub fn luma([r, g, b]: [f32; 3]) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// The statistics of an RGBA frame `width` × `height`, every eighth pixel
/// each way.
pub fn stats(rgba: &[u8], width: usize, height: usize) -> Stats {
    const STEP: usize = 8;
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    let (fx0, fx1) = (
        (FACE_X.0 * width as f32) as usize,
        (FACE_X.1 * width as f32) as usize,
    );
    let (fy0, fy1) = (
        (FACE_Y.0 * height as f32) as usize,
        (FACE_Y.1 * height as f32) as usize,
    );
    let mut face = 0f64;
    let mut face_n = 0u64;
    for y in (STEP / 2..height).step_by(STEP) {
        let row = y * width * 4;
        for x in (STEP / 2..width).step_by(STEP) {
            let Some(&[r, g, b]) = rgba.get(row + x * 4..row + x * 4 + 3) else {
                continue;
            };
            let [r, g, b] = [r, g, b].map(u64::from);
            sum = [sum[0] + r, sum[1] + g, sum[2] + b];
            n += 1;
            if (fx0..fx1).contains(&x) && (fy0..fy1).contains(&y) {
                face += f64::from(luma([r as f32, g as f32, b as f32]));
                face_n += 1;
            }
        }
    }
    let n = n.max(1) as f32 * 255.0;
    Stats {
        mean: sum.map(|s| s as f32 / n),
        face: (face / (face_n.max(1) as f64 * 255.0)) as f32,
    }
}

/// What Lumière does to each channel: its gain, then the exposure's
/// curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correction {
    pub gain: [f32; 3],
    pub exposure: f32,
}

impl Correction {
    /// Nothing changed.
    pub const NONE: Self = Self {
        gain: [1.0; 3],
        exposure: 1.0,
    };

    /// What a frame of these statistics needs.
    pub fn of(stats: &Stats) -> Self {
        let [r, g, b] = stats.mean;
        let y = luma(stats.mean).max(1e-3);
        let balance = |m: f32| (1.0 + 0.7 * (y / m.max(1e-3) - 1.0)).clamp(0.75, 4.0 / 3.0);
        let mut gain = [balance(r), balance(g), balance(b)];
        let cold = ((b - r) / y).clamp(0.0, 0.5);
        let warm = 0.02 + 0.08 * cold;
        gain[0] *= 1.0 + warm;
        gain[2] *= 1.0 - warm;
        let x = luma([
            (r * gain[0]).min(1.0),
            (g * gain[1]).min(1.0),
            (b * gain[2]).min(1.0),
        ])
        .clamp(1e-3, 0.999);
        // The curve's e that takes x to TARGET.
        let exposure = (TARGET * (1.0 - x) / (x * (1.0 - TARGET))).clamp(0.6, 3.0);
        Self { gain, exposure }
    }

    /// A step from here towards `to`, `k` of the way.
    pub fn towards(self, to: Self, k: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * k;
        Self {
            gain: [
                mix(self.gain[0], to.gain[0]),
                mix(self.gain[1], to.gain[1]),
                mix(self.gain[2], to.gain[2]),
            ],
            exposure: mix(self.exposure, to.exposure),
        }
    }

    /// Channel `c`'s value `v` corrected.
    pub fn apply(self, v: f32, c: usize) -> f32 {
        let x = (v * self.gain.get(c).copied().unwrap_or(1.0)).min(1.0);
        let e = self.exposure;
        (e * x / (1.0 + (e - 1.0) * x)).clamp(0.0, 1.0)
    }

    /// Each channel's 256 values corrected.
    pub fn table(self) -> [[u8; 256]; 3] {
        let mut table = [[0u8; 256]; 3];
        for (c, row) in table.iter_mut().enumerate() {
            for (v, out) in row.iter_mut().enumerate() {
                *out = to_byte(self.apply(v as f32 / 255.0, c));
            }
        }
        table
    }
}

/// A value from 0 to 1 as a byte, rounded.
pub fn to_byte(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// How much of the new frame a pixel takes, its channels having changed by
/// at most `diff` (of 255): [`KEEP_NEW`] still, all of it moving.
pub fn keep_new(diff: f32) -> f32 {
    let t = ((diff - STILL) / (MOVING - STILL)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    KEEP_NEW + (1.0 - KEEP_NEW) * smooth
}

/// The banner over the frame: premultiplied RGBA, its rows from `top` to
/// the frame's bottom, as wide as the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub top: usize,
    pub pixels: Vec<u8>,
}

/// How a frame is made: Lumière on or not, the banner if any.
#[derive(Debug, Clone, Copy)]
pub struct Making<'a> {
    pub lighting: bool,
    pub correction: Correction,
    pub overlay: Option<&'a Overlay>,
}

/// The frames made on the CPU: the last one kept for the denoise.
#[derive(Debug, Default)]
pub struct Cpu {
    last: Vec<u8>,
}

impl Cpu {
    /// RGBA `frame` (`W` × `H`) made into `out`, B, G, R, x.
    pub fn make(&mut self, frame: &[u8], making: &Making<'_>, out: &mut [u8]) {
        let (Some(frame), Some(out)) = (frame.get(..BYTES), out.get_mut(..BYTES)) else {
            return;
        };
        if making.lighting {
            let [tr, tg, tb] = making.correction.table();
            let look = |t: &[u8; 256], v: u8| t.get(usize::from(v)).copied().unwrap_or(v);
            let corrected = frame.as_chunks::<4>().0.iter().map(|px| {
                let [r, g, b, _] = *px;
                [look(&tr, r), look(&tg, g), look(&tb, b)]
            });
            if self.last.len() == BYTES {
                for ((new, to), old) in corrected
                    .zip(out.as_chunks_mut::<4>().0.iter_mut())
                    .zip(self.last.as_chunks::<4>().0.iter())
                {
                    // The last frame's, B, G, R.
                    let &[ob, og, or, _] = old;
                    let old = [or, og, ob];
                    let diff = new
                        .iter()
                        .zip(old)
                        .map(|(n, o)| n.abs_diff(o))
                        .max()
                        .unwrap_or(0);
                    let k = keep_new(f32::from(diff));
                    let mix = |n: u8, o: u8| {
                        (f32::from(o) + (f32::from(n) - f32::from(o)) * k)
                            .round()
                            .clamp(0.0, 255.0) as u8
                    };
                    let [r, g, b] = [0, 1, 2].map(|c| {
                        mix(
                            new.get(c).copied().unwrap_or(0),
                            old.get(c).copied().unwrap_or(0),
                        )
                    });
                    to.copy_from_slice(&[b, g, r, 255]);
                }
            } else {
                for (new, to) in corrected.zip(out.as_chunks_mut::<4>().0.iter_mut()) {
                    let [r, g, b] = new;
                    to.copy_from_slice(&[b, g, r, 255]);
                }
            }
            // The denoise's memory is the frame before the banner.
            self.last.clear();
            self.last.extend_from_slice(out);
        } else {
            self.last.clear();
            for (px, to) in frame
                .as_chunks::<4>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                let &[r, g, b, _] = px;
                *to = [b, g, r, 255];
            }
        }
        if let Some(overlay) = making.overlay {
            blend(overlay, out);
        }
    }
}

/// `overlay` over B, G, R, x `out`.
pub fn blend(overlay: &Overlay, out: &mut [u8]) {
    let start = overlay.top * W * 4;
    let Some(rows) = out.get_mut(start..BYTES) else {
        return;
    };
    for (to, over) in rows
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(overlay.pixels.as_chunks::<4>().0.iter())
    {
        let (&[r, g, b, a], [tb, tg, tr, _]) = (over, to);
        if a == 0 {
            continue;
        }
        let keep = 255 - u32::from(a);
        let mix = |o: u8, under: u8| {
            (u32::from(o) + (u32::from(under) * keep + 127) / 255).min(255) as u8
        };
        *tb = mix(b, *tb);
        *tg = mix(g, *tg);
        *tr = mix(r, *tr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(rgb: [u8; 3]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(BYTES);
        for _ in 0..W * H {
            frame.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        frame
    }

    fn mean_bgrx(out: &[u8]) -> [f32; 3] {
        let mut sum = [0f64; 3];
        for px in out.as_chunks::<4>().0.iter() {
            sum[0] += f64::from(px[2]);
            sum[1] += f64::from(px[1]);
            sum[2] += f64::from(px[0]);
        }
        sum.map(|s| (s / (W * H) as f64) as f32)
    }

    #[test]
    fn the_curve_keeps_black_and_white() {
        let c = Correction {
            gain: [1.0; 3],
            exposure: 2.5,
        };
        assert_eq!(c.apply(0.0, 0), 0.0);
        assert!((c.apply(1.0, 1) - 1.0).abs() < 1e-6);
        assert!(c.apply(0.2, 2) > 0.2);
    }

    /// A dark, blue frame comes out at the target's brightness, its red and
    /// blue nearer each other, a little warm.
    #[test]
    fn lumiere_brightens_a_dark_frame_and_balances_it() {
        let frame = flat([55, 65, 90]);
        let stats = stats(&frame, W, H);
        let correction = Correction::of(&stats);
        let mut out = vec![0; BYTES];
        Cpu::default().make(
            &frame,
            &Making {
                lighting: true,
                correction,
                overlay: None,
            },
            &mut out,
        );
        let [r, g, b] = mean_bgrx(&out);
        let y = luma([r, g, b]) / 255.0;
        assert!((y - TARGET).abs() < 0.03, "{r} {g} {b}: {y}");
        assert!((b - r).abs() < (90.0 - 55.0) / 2.0, "{r} {b}");
    }

    #[test]
    fn without_lumiere_the_frame_is_as_it_came() {
        let frame = flat([40, 50, 75]);
        let mut out = vec![0; BYTES];
        Cpu::default().make(
            &frame,
            &Making {
                lighting: false,
                correction: Correction::NONE,
                overlay: None,
            },
            &mut out,
        );
        assert_eq!(&out[..4], &[75, 50, 40, 255]);
    }

    /// Noise on a still picture is halved, and a moving pixel is taken
    /// whole.
    #[test]
    fn the_denoise_keeps_still_pixels_and_follows_moving_ones() {
        assert!((keep_new(0.0) - KEEP_NEW).abs() < 1e-6);
        assert!((keep_new(30.0) - 1.0).abs() < 1e-6);
        let mut cpu = Cpu::default();
        let making = Making {
            lighting: true,
            correction: Correction::NONE,
            overlay: None,
        };
        let mut out = vec![0; BYTES];
        cpu.make(&flat([100, 100, 100]), &making, &mut out);
        cpu.make(&flat([104, 104, 104]), &making, &mut out);
        assert_eq!(out[0], 102, "a still pixel: 40 % of the new");
        cpu.make(&flat([200, 200, 200]), &making, &mut out);
        assert_eq!(out[0], 200, "a moving one: all of it");
    }

    #[test]
    fn the_face_is_the_centre() {
        let mut frame = flat([20, 20, 20]);
        for y in H / 4..H / 2 {
            for x in W * 2 / 5..W * 3 / 5 {
                let at = (y * W + x) * 4;
                frame[at..at + 3].copy_from_slice(&[200, 200, 200]);
            }
        }
        let s = stats(&frame, W, H);
        assert!(s.face > 0.3, "{s:?}");
        assert!(luma(s.mean) < 0.2, "{s:?}");
    }
}
