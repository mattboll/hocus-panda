//! The effects' particles (#793): one CPU simulation, stepped at a fixed
//! 60 steps a second from the effect's start whatever the frames' pace,
//! with a seeded generator, so that a moment of an effect is the same on
//! every machine and in tests; drawn on the GPU as one batch of quads a
//! blend ([`super::draw`]). Embers for the flames (#794); the snow,
//! confetti and fireworks take the same.

use std::time::Duration;

/// A step of the simulation.
pub const STEP: f32 = 1.0 / 60.0;
/// At most this many steps at once, after a long pause (a screen off):
/// the effect is near its end by then.
const MOST_STEPS: u64 = 600;

/// xorshift64*: small, seeded, the same everywhere.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// From 0 to 1, 1 excluded.
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (r >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// How a particle is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A glowing dot in its halo, white-hot to red as it ages, its light
    /// added to what is under it.
    Ember,
    /// A soft grey puff, swelling and fading in then out.
    Smoke,
}

/// One particle, in logical pixels (global) and seconds.
#[derive(Debug, Clone, Copy)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Added to `vy` each second (up is negative).
    pub lift: f32,
    /// Its sideways sway, pixels a second each second, at its height.
    pub sway: f32,
    /// Its size: an ember's dot, a puff's radius; and how fast it grows.
    pub size: f32,
    pub grow: f32,
    pub life: f32,
    pub max: f32,
    pub look: Look,
}

impl Particle {
    /// How far through its life, from 0 to 1.
    pub fn age(&self) -> f32 {
        (self.life / self.max).clamp(0.0, 1.0)
    }
}

/// Particles, stepped to a time.
#[derive(Debug)]
pub struct Particles {
    pub list: Vec<Particle>,
    pub rng: Rng,
    /// Steps taken since the start.
    steps: u64,
}

impl Particles {
    pub fn new(seed: u64) -> Self {
        Self {
            list: Vec::new(),
            rng: Rng::new(seed),
            steps: 0,
        }
    }

    /// Steps them to `elapsed` from the start: each step `spawn` adds what
    /// is born then (the step's time, in seconds), then each ages and
    /// moves; the dead go.
    pub fn advance(
        &mut self,
        elapsed: Duration,
        mut spawn: impl FnMut(f32, &mut Rng, &mut Vec<Particle>),
    ) {
        let due = (elapsed.as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for step in from..due {
            let at = step as f32 * STEP;
            spawn(at, &mut self.rng, &mut self.list);
            self.list.retain_mut(|p| {
                p.life += STEP;
                if p.life >= p.max {
                    return false;
                }
                p.vx += (p.y * 0.05 + p.life * 7.0).sin() * p.sway * STEP;
                p.vy += p.lift * STEP;
                p.x += p.vx * STEP;
                p.y += p.vy * STEP;
                p.size += p.grow * STEP;
                true
            });
        }
        self.steps = self.steps.max(due);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Takes out those of `look`.
    pub fn clear(&mut self, look: Look) {
        self.list.retain(|p| p.look != look);
    }
}

/// Floats a vertex: its place (output-local, physical), its place across
/// its quad (-1 to 1), its colour (premultiplied), its shape (look, the
/// dot's share of the quad, the quad's radius in pixels, unused).
pub const FLOATS: usize = 12;

/// The quads of `particles` (global logical, the output's corner at
/// `origin`, `scale` physical pixels a logical one) into `smoke` and
/// `embers`, as the shaders read them; the bounds of what they cover,
/// output-local and physical: `None` when none is drawn.
pub fn vertices(
    particles: &[Particle],
    origin: (f64, f64),
    scale: f64,
    smoke: &mut Vec<f32>,
    embers: &mut Vec<f32>,
) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    let s = scale as f32;
    for p in particles {
        let a = 1.0 - p.age();
        let (out, radius, colour, shape) = match p.look {
            Look::Smoke => {
                let alpha = (p.age() * std::f32::consts::PI).sin() * 0.10;
                let grey = [120.0 / 255.0, 118.0 / 255.0, 128.0 / 255.0];
                (
                    &mut *smoke,
                    p.size,
                    [grey[0] * alpha, grey[1] * alpha, grey[2] * alpha, alpha],
                    [0.0, 0.0],
                )
            }
            Look::Ember => {
                let heat: [f32; 3] = if a > 0.6 {
                    [255.0, 236.0, 180.0]
                } else if a > 0.3 {
                    [255.0, 170.0, 70.0]
                } else {
                    [255.0, 90.0, 30.0]
                };
                let halo = p.size * 9.0;
                let dot = p.size * (0.6 + a * 0.6);
                (
                    &mut *embers,
                    halo,
                    [heat[0] / 255.0, heat[1] / 255.0, heat[2] / 255.0, a],
                    [1.0, dot / halo],
                )
            }
        };
        if radius <= 0.0 || colour[3] <= 0.0 {
            continue;
        }
        let cx = (f64::from(p.x) - origin.0) as f32 * s;
        let cy = (f64::from(p.y) - origin.1) as f32 * s;
        let r = radius * s;
        let (x0, y0, x1, y1) = (cx - r, cy - r, cx + r, cy + r);
        bounds = Some(match bounds {
            None => (x0, y0, x1, y1),
            Some((a0, b0, a1, b1)) => (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)),
        });
        for (x, y, u, v) in [
            (x0, y0, -1.0, -1.0),
            (x1, y0, 1.0, -1.0),
            (x0, y1, -1.0, 1.0),
            (x1, y0, 1.0, -1.0),
            (x1, y1, 1.0, 1.0),
            (x0, y1, -1.0, 1.0),
        ] {
            out.extend_from_slice(&[
                x, y, u, v, colour[0], colour[1], colour[2], colour[3], shape[0], shape[1], r, 0.0,
            ]);
        }
    }
    bounds.map(|(x0, y0, x1, y1)| {
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        (x0, y0, x1.ceil() as i32 - x0, y1.ceil() as i32 - y0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rising(at: f32, rng: &mut Rng, out: &mut Vec<Particle>) {
        if at < 0.5 {
            out.push(Particle {
                x: 100.0 + rng.next() * 10.0,
                y: 100.0,
                vx: 0.0,
                vy: -60.0,
                lift: -20.0,
                sway: 60.0,
                size: 1.0,
                grow: 0.0,
                life: 0.0,
                max: 1.0,
                look: Look::Ember,
            });
        }
    }

    /// Stepped in one go or frame by frame, the same particles: what a
    /// moment shows does not depend on the frames' pace.
    #[test]
    fn the_same_moment_whatever_the_frames() {
        let mut once = Particles::new(7);
        once.advance(Duration::from_millis(800), rising);
        let mut framed = Particles::new(7);
        for ms in (0..=800).step_by(7) {
            framed.advance(Duration::from_millis(ms), rising);
        }
        framed.advance(Duration::from_millis(800), rising);
        assert_eq!(once.list.len(), framed.list.len());
        for (a, b) in once.list.iter().zip(&framed.list) {
            assert_eq!((a.x, a.y, a.life), (b.x, b.y, b.life));
        }
        // They rose, and all are gone a second after the last is born.
        assert!(once.list.iter().all(|p| p.y < 100.0));
        once.advance(Duration::from_millis(1600), rising);
        assert!(once.is_empty());
    }

    #[test]
    fn a_quad_each_and_its_bounds() {
        let mut p = Particles::new(1);
        p.advance(Duration::from_millis(100), rising);
        let (mut smoke, mut embers) = (Vec::new(), Vec::new());
        let bounds = vertices(&p.list, (0.0, 0.0), 2.0, &mut smoke, &mut embers);
        assert!(smoke.is_empty());
        assert_eq!(embers.len(), p.list.len() * 6 * FLOATS);
        let (x, y, w, h) = bounds.unwrap_or_default();
        // A halo nine times its dot, at scale 2.
        assert!(w >= 36 && h >= 36 && x < 200 && y < 200, "{bounds:?}");
    }
}
