//! Panda, Robot and Grave: the voice transformed in the frequency domain,
//! a phase vocoder written here (no pitch shifter's code was taken).
//!
//! - **Frames**: 1024 samples (21 ms) under a Hann window, every 256
//!   (Panda, Grave) or every 320 (Robot); overlap-added under the same
//!   window, each sample divided by the windows' squares that cover it.
//! - **Pitch** (Panda, Grave): each bin's true frequency from its phase's
//!   advance since the last frame; the spectrum divided by its envelope
//!   (the log spectrum's cepstrum kept under 1.25 ms: the formants) into
//!   its excitation, which moves to `ratio` times its frequency, phases
//!   carried on at the new frequencies; then the envelope again, read at
//!   `1 / formants` times each bin: the formants move by `formants`, not
//!   by the pitch's ratio, so that the voice stays a voice (the pitch
//!   alone would be a chipmunk or a monster).
//! - **Robot**: each frame's magnitudes kept and its phases set to zero:
//!   every 320 samples the same pulse, shaped by the voice, so the voice is
//!   on one note, 150 Hz, metallic.
//! - **The level** follows the input's over a second: what goes in at a
//!   level comes out at it.
//! - **Its delay**: a frame, 1024 samples (21 ms).

use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

use crate::{RATE, effect::Effect};

/// A frame's length.
const N: usize = 1024;
const BINS: usize = N / 2 + 1;
/// The envelope: the cepstrum's first this many quefrencies (1.25 ms:
/// under the pitch period of any voice).
const LIFTER: usize = 60;

/// What the frames are made into.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Pitch {
        ratio: f32,
        formants: f32,
        /// Over this many hertz, this gain (the panda's roundness).
        soften: Option<(f32, f32)>,
    },
    Robot,
}

/// A voice made by frames.
pub struct Shifter {
    kind: Kind,
    hop: usize,
    window: Vec<f32>,
    /// One over the windows' squares that cover each place of a hop.
    norm: Vec<f32>,
    /// The last [`N`] samples, a ring: the oldest at `at`.
    input: Vec<f32>,
    at: usize,
    /// Samples since the last frame.
    since: usize,
    /// The overlap-added output, and what is ready to go out.
    sum: Vec<f32>,
    ready: std::collections::VecDeque<f32>,
    last_phase: Vec<f32>,
    phase: Vec<f32>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex32>,
    cepstrum: Vec<Complex32>,
    scratch: Vec<Complex32>,
    magnitude: Vec<f32>,
    frequency: Vec<f32>,
    envelope: Vec<f32>,
    moved: Vec<f32>,
    moved_freq: Vec<f32>,
    loudest: Vec<f32>,
    soften: Vec<f32>,
    /// The input's and the output's mean squares, and the gain between.
    level_in: f32,
    level_out: f32,
}

impl std::fmt::Debug for Shifter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shifter")
            .field("kind", &self.kind)
            .field("hop", &self.hop)
            .finish_non_exhaustive()
    }
}

impl Shifter {
    /// Panda: higher (a fifth up), rounder (the formants up less, the
    /// highs softened).
    pub fn panda() -> Self {
        Self::new(
            Kind::Pitch {
                ratio: PANDA,
                formants: 1.15,
                soften: Some((4000.0, 0.55)),
            },
            256,
        )
    }

    /// Grave: an octave lower, the formants a little lower.
    pub fn deep() -> Self {
        Self::new(
            Kind::Pitch {
                ratio: DEEP,
                formants: 0.88,
                soften: None,
            },
            256,
        )
    }

    /// Robot: on one note, 150 Hz.
    pub fn robot() -> Self {
        Self::new(Kind::Robot, ROBOT_HOP)
    }

    fn new(kind: Kind, hop: usize) -> Self {
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(N);
        let inverse = planner.plan_fft_inverse(N);
        let window: Vec<f32> = (0..N)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / N as f32).cos())
            .collect();
        let norm = (0..hop)
            .map(|p| {
                let sum: f32 = window.iter().skip(p).step_by(hop).map(|w| w * w).sum();
                if sum > 1e-6 { 1.0 / sum } else { 0.0 }
            })
            .collect();
        let scratch = vec![
            Complex32::default();
            forward
                .get_inplace_scratch_len()
                .max(inverse.get_inplace_scratch_len())
        ];
        let soften = match kind {
            Kind::Pitch {
                soften: Some((hz, g)),
                ..
            } => (0..BINS)
                .map(|k| {
                    let f = k as f32 * RATE as f32 / N as f32;
                    // A gentle shelf: down to `g` over an octave from `hz`.
                    let t = ((f / hz).log2()).clamp(0.0, 1.0);
                    1.0 + (g - 1.0) * t
                })
                .collect(),
            _ => vec![1.0; BINS],
        };
        let mut shifter = Self {
            kind,
            hop,
            window,
            norm,
            input: vec![0.0; N],
            at: 0,
            since: 0,
            sum: vec![0.0; N],
            ready: std::collections::VecDeque::with_capacity(2 * N),
            last_phase: vec![0.0; BINS],
            phase: vec![0.0; BINS],
            forward,
            inverse,
            buffer: vec![Complex32::default(); N],
            cepstrum: vec![Complex32::default(); N],
            scratch,
            magnitude: vec![0.0; BINS],
            frequency: vec![0.0; BINS],
            envelope: vec![0.0; BINS],
            moved: vec![0.0; BINS],
            moved_freq: vec![0.0; BINS],
            loudest: vec![0.0; BINS],
            soften,
            level_in: 0.0,
            level_out: 0.0,
        };
        shifter.reset();
        shifter
    }

    /// One frame: the last [`N`] samples made, overlap-added, and a hop
    /// of output ready.
    fn frame(&mut self) {
        let (newer, older) = self.input.split_at(self.at);
        let ordered = older.iter().chain(newer);
        for ((b, x), w) in self.buffer.iter_mut().zip(ordered).zip(&self.window) {
            *b = Complex32::new(x * w, 0.0);
        }
        self.forward
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        match self.kind {
            Kind::Robot => self.robot_frame(),
            Kind::Pitch {
                ratio, formants, ..
            } => self.pitch_frame(ratio, formants),
        }
        self.inverse
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let scale = 1.0 / N as f32;
        for ((s, b), w) in self.sum.iter_mut().zip(&self.buffer).zip(&self.window) {
            *s += b.re * scale * w;
        }
        for (s, n) in self.sum.iter().take(self.hop).zip(&self.norm) {
            self.ready.push_back(s * n);
        }
        self.sum.rotate_left(self.hop);
        let len = self.sum.len();
        for s in self.sum.iter_mut().skip(len - self.hop) {
            *s = 0.0;
        }
    }

    /// The magnitudes kept, the phases zero: one pulse a frame.
    fn robot_frame(&mut self) {
        for k in 0..BINS {
            let m = self.buffer.get(k).map_or(0.0, |b| b.norm());
            if let Some(b) = self.buffer.get_mut(k) {
                *b = Complex32::new(m, 0.0);
            }
        }
        self.mirror();
    }

    /// The spectrum's upper half, the mirror of its lower.
    fn mirror(&mut self) {
        for k in BINS..N {
            let low = self.buffer.get(N - k).copied().unwrap_or_default();
            if let Some(b) = self.buffer.get_mut(k) {
                *b = low.conj();
            }
        }
    }

    /// The envelope of the spectrum in `buffer`, from its cepstrum.
    fn envelope(&mut self) {
        for (c, b) in self.cepstrum.iter_mut().zip(&self.buffer) {
            *c = Complex32::new((b.norm() + 1e-9).ln(), 0.0);
        }
        self.inverse
            .process_with_scratch(&mut self.cepstrum, &mut self.scratch);
        for (q, c) in self.cepstrum.iter_mut().enumerate() {
            let keep = !(LIFTER..=N - LIFTER).contains(&q);
            *c = if keep {
                *c / N as f32
            } else {
                Complex32::default()
            };
        }
        self.forward
            .process_with_scratch(&mut self.cepstrum, &mut self.scratch);
        for (e, c) in self.envelope.iter_mut().zip(&self.cepstrum) {
            *e = c.re.exp();
        }
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "bins of BINS-long spectra, each index checked against BINS"
    )]
    fn pitch_frame(&mut self, ratio: f32, formants: f32) {
        self.envelope();
        let hop = self.hop as f32;
        let expected = 2.0 * PI * hop / N as f32;
        for k in 0..BINS {
            let b = self.buffer[k];
            let phase = b.im.atan2(b.re);
            let mut advance = phase - self.last_phase[k] - k as f32 * expected;
            self.last_phase[k] = phase;
            advance -= 2.0 * PI * (advance / (2.0 * PI)).round();
            // In bins: k and what the phase moved more than it would.
            self.frequency[k] = k as f32 + advance / expected;
            self.magnitude[k] = b.norm() / self.envelope[k].max(1e-9);
        }
        self.moved.fill(0.0);
        self.moved_freq.fill(0.0);
        self.loudest.fill(0.0);
        for k in 0..BINS {
            let j = (k as f32 * ratio).round() as usize;
            if j >= BINS {
                break;
            }
            let m = self.magnitude[k];
            self.moved[j] += m;
            if m > self.loudest[j] {
                self.loudest[j] = m;
                self.moved_freq[j] = self.frequency[k] * ratio;
            }
        }
        for j in 0..BINS {
            // The envelope where this bin's formant came from.
            let at = j as f32 / formants;
            let i = at.floor() as usize;
            let env = if i + 1 < BINS {
                let t = at - i as f32;
                self.envelope[i] * (1.0 - t) + self.envelope[i + 1] * t
            } else {
                self.envelope[BINS - 1] * 0.1
            };
            let m = self.moved[j] * env * self.soften[j];
            self.phase[j] += self.moved_freq[j] * expected;
            self.phase[j] -= 2.0 * PI * (self.phase[j] / (2.0 * PI)).round();
            self.buffer[j] = Complex32::from_polar(m, self.phase[j]);
        }
        // DC and the highest bin real.
        self.buffer[0] = Complex32::new(self.buffer[0].norm(), 0.0);
        self.buffer[BINS - 1] = Complex32::new(self.buffer[BINS - 1].re, 0.0);
        self.mirror();
    }
}

/// Panda's pitch: a fifth up.
pub const PANDA: f32 = 1.5;
/// Grave's: an octave down.
pub const DEEP: f32 = 0.5;
/// Robot's hop: its note, `RATE / ROBOT_HOP` = 150 Hz.
pub const ROBOT_HOP: usize = 320;

impl Effect for Shifter {
    fn run(&mut self, io: &mut [f32]) {
        let slow = 1.0 / RATE as f32;
        for x in io.iter_mut() {
            let v = *x;
            if let Some(oldest) = self.input.get_mut(self.at) {
                *oldest = v;
            }
            self.at = (self.at + 1) % N;
            self.since += 1;
            if self.since == self.hop {
                self.since = 0;
                self.frame();
            }
            let y = self.ready.pop_front().unwrap_or(0.0);
            // The output's level, the input's: followed over a second.
            self.level_in += (v * v - self.level_in) * slow;
            self.level_out += (y * y - self.level_out) * slow;
            let g = if self.level_out > 1e-9 {
                (self.level_in / self.level_out).sqrt().clamp(0.25, 4.0)
            } else {
                1.0
            };
            *x = (y * g).clamp(-1.0, 1.0);
        }
    }

    fn latency(&self) -> usize {
        N
    }

    fn reset(&mut self) {
        self.input.fill(0.0);
        self.at = 0;
        self.sum.fill(0.0);
        self.ready.clear();
        self.ready.extend(std::iter::repeat_n(0.0, self.hop));
        self.since = 0;
        self.last_phase.fill(0.0);
        self.phase.fill(0.0);
        self.level_in = 0.0;
        self.level_out = 0.0;
    }
}
