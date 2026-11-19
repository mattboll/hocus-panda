//! Studio: the voice fuller and more even, as a broadcast chain makes it,
//! with no delay.
//!
//! - **An equaliser** (biquads, Robert Bristow-Johnson's cookbook): the
//!   rumble under 80 Hz out, +3 dB of body under 180 Hz, -2 dB of
//!   boxiness around 350 Hz, +3 dB of presence around 3.2 kHz, +2 dB of
//!   air over 9 kHz.
//! - **A gentle de-esser**: the sibilants' band (around 7 kHz) watched;
//!   while it stands out of the voice, what is over 5 kHz goes down, by
//!   6 dB at most.
//! - **A compressor**: over -26 dBFS, 3 to 1, a soft knee of 8 dB, 5 ms in
//!   and 150 ms out, 7 dB of make-up; under -55 dBFS, a gentle expander,
//!   so that the room's hiss is not made louder with the voice.
//! - **A limiter** at -1 dBFS, at once, out over 50 ms.

use std::f32::consts::PI;

use crate::{RATE, effect::Effect};

/// A second-order filter, transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [a1 / a0, a2 / a0],
            z: [0.0; 2],
        }
    }

    fn omega(hz: f32) -> (f32, f32) {
        let w = 2.0 * PI * hz / RATE as f32;
        (w.cos(), w.sin())
    }

    fn high_pass(hz: f32, q: f32) -> Self {
        let (c, s) = Self::omega(hz);
        let alpha = s / (2.0 * q);
        Self::new(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn low_pass(hz: f32, q: f32) -> Self {
        let (c, s) = Self::omega(hz);
        let alpha = s / (2.0 * q);
        Self::new(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn band_pass(hz: f32, q: f32) -> Self {
        let (c, s) = Self::omega(hz);
        let alpha = s / (2.0 * q);
        Self::new(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    fn peak(hz: f32, q: f32, db: f32) -> Self {
        let (c, s) = Self::omega(hz);
        let alpha = s / (2.0 * q);
        let a = 10f32.powf(db / 40.0);
        Self::new(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    fn shelf(hz: f32, db: f32, low: bool) -> Self {
        let (c, s) = Self::omega(hz);
        let a = 10f32.powf(db / 40.0);
        // A shelf's slope of 1.
        let alpha = s / 2.0 * 2f32.sqrt();
        let k = 2.0 * a.sqrt() * alpha;
        if low {
            Self::new(
                a * ((a + 1.0) - (a - 1.0) * c + k),
                2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                a * ((a + 1.0) - (a - 1.0) * c - k),
                (a + 1.0) + (a - 1.0) * c + k,
                -2.0 * ((a - 1.0) + (a + 1.0) * c),
                (a + 1.0) + (a - 1.0) * c - k,
            )
        } else {
            Self::new(
                a * ((a + 1.0) + (a - 1.0) * c + k),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - k),
                (a + 1.0) - (a - 1.0) * c + k,
                2.0 * ((a - 1.0) - (a + 1.0) * c),
                (a + 1.0) - (a - 1.0) * c - k,
            )
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let [b0, b1, b2] = self.b;
        let [a1, a2] = self.a;
        let y = b0 * x + self.z[0];
        self.z[0] = b1 * x - a1 * y + self.z[1];
        self.z[1] = b2 * x - a2 * y;
        y
    }

    fn clear(&mut self) {
        self.z = [0.0; 2];
    }
}

/// A one-pole follower's coefficient for `ms` milliseconds.
fn follow(ms: f32) -> f32 {
    (-1.0 / (ms * 0.001 * RATE as f32)).exp()
}

fn db(x: f32) -> f32 {
    10.0 * x.max(1e-12).log10()
}

fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The compressor's gain, dB, for a level of `level` dBFS.
fn compressor_db(level: f32) -> f32 {
    const THRESHOLD: f32 = -26.0;
    const RATIO: f32 = 3.0;
    const KNEE: f32 = 8.0;
    const FLOOR: f32 = -55.0;
    const EXPAND: f32 = 1.5;
    let over = level - THRESHOLD;
    let compressed = if over <= -KNEE / 2.0 {
        0.0
    } else if over >= KNEE / 2.0 {
        -over * (1.0 - 1.0 / RATIO)
    } else {
        let x = over + KNEE / 2.0;
        -(1.0 - 1.0 / RATIO) * x * x / (2.0 * KNEE)
    };
    let under = (FLOOR - level).max(0.0);
    compressed - under * (EXPAND - 1.0)
}

/// The studio's chain.
#[derive(Debug, Clone)]
pub struct Studio {
    eq: [Biquad; 4],
    air: Biquad,
    /// The de-esser: its detector, and the split of what is over 5 kHz.
    sibilance: Biquad,
    split: Biquad,
    sibilant: f32,
    voiced: f32,
    /// The compressor's mean square, and its gain, dB.
    power: f32,
    reduction: f32,
    /// The limiter's gain.
    limit: f32,
    coefs: Coefs,
}

#[derive(Debug, Clone, Copy)]
struct Coefs {
    power: f32,
    attack: f32,
    release: f32,
    ess_attack: f32,
    ess_release: f32,
    limit_release: f32,
}

impl Default for Studio {
    fn default() -> Self {
        Self::new()
    }
}

impl Studio {
    pub fn new() -> Self {
        Self {
            eq: [
                Biquad::high_pass(80.0, std::f32::consts::FRAC_1_SQRT_2),
                Biquad::shelf(180.0, 3.0, true),
                Biquad::peak(350.0, 1.0, -2.0),
                Biquad::peak(3200.0, 0.8, 3.0),
            ],
            air: Biquad::shelf(9000.0, 2.0, false),
            sibilance: Biquad::band_pass(7000.0, 1.0),
            split: Biquad::low_pass(5000.0, std::f32::consts::FRAC_1_SQRT_2),
            sibilant: 0.0,
            voiced: 0.0,
            power: 0.0,
            reduction: 0.0,
            limit: 1.0,
            coefs: Coefs {
                power: follow(10.0),
                attack: follow(5.0),
                release: follow(150.0),
                ess_attack: follow(1.0),
                ess_release: follow(60.0),
                limit_release: follow(50.0),
            },
        }
    }

    #[inline]
    fn sample(&mut self, x: f32) -> f32 {
        let c = self.coefs;
        let mut y = x;
        for f in &mut self.eq {
            y = f.tick(y);
        }
        // De-esser: the sibilants' level against the voice's.
        let s = self.sibilance.tick(y);
        let (s2, y2) = (s * s, y * y);
        let k = |level: f32, now: f32| {
            if now > level {
                c.ess_attack
            } else {
                c.ess_release
            }
        };
        self.sibilant = s2 + (self.sibilant - s2) * k(self.sibilant, s2);
        self.voiced = y2 + (self.voiced - y2) * k(self.voiced, y2);
        let low = self.split.tick(y);
        let high = y - low;
        let ess = db(self.sibilant) - db(self.voiced);
        // Sibilants stand out when their band holds over a third of the
        // power, and the voice is not quiet.
        let cut = if db(self.sibilant) > -50.0 {
            ((ess + 5.0) * 1.5).clamp(0.0, 6.0)
        } else {
            0.0
        };
        y = low + high * gain(-cut);
        // Compressor.
        self.power = y * y + (self.power - y * y) * c.power;
        let target = compressor_db(db(self.power));
        let k = if target < self.reduction {
            c.attack
        } else {
            c.release
        };
        self.reduction = target + (self.reduction - target) * k;
        y *= gain(self.reduction + 7.0);
        y = self.air.tick(y);
        // Limiter at -1 dBFS.
        const CEILING: f32 = 0.891;
        let peak = y.abs() * self.limit;
        if peak > CEILING {
            self.limit = CEILING / y.abs();
        } else {
            self.limit = 1.0 + (self.limit - 1.0) * c.limit_release;
        }
        (y * self.limit).clamp(-1.0, 1.0)
    }
}

impl Effect for Studio {
    fn run(&mut self, io: &mut [f32]) {
        for x in io.iter_mut() {
            *x = self.sample(*x);
        }
    }

    fn latency(&self) -> usize {
        0
    }

    fn reset(&mut self) {
        for f in &mut self.eq {
            f.clear();
        }
        self.air.clear();
        self.sibilance.clear();
        self.split.clear();
        self.sibilant = 0.0;
        self.voiced = 0.0;
        self.power = 0.0;
        self.reduction = 0.0;
        self.limit = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// A loud tone and a quiet one, 20 dB apart, come out closer.
    #[test]
    fn studio_makes_the_voice_more_even() {
        let tone = |amp: f32| -> Vec<f32> {
            (0..RATE)
                .map(|i| amp * (2.0 * PI * 220.0 * i as f32 / RATE as f32).sin())
                .collect()
        };
        let mut studio = Studio::new();
        let mut loud = tone(0.3);
        studio.run(&mut loud);
        let mut quiet = tone(0.03);
        studio.run(&mut quiet);
        let tail = |x: &[f32]| rms(&x[RATE / 2..]);
        let spread = 20.0 * (tail(&loud) / tail(&quiet)).log10();
        assert!(spread < 12.0, "{spread} dB apart");
        assert!(loud.iter().all(|x| x.abs() <= 0.9));
    }
}
