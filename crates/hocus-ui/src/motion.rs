//! The motion vocabulary of docs/design/motion.md: durations, easings and
//! springs, all evaluated in closed form at a given time, so that an
//! animation sampled at a virtual time gives the same frame every run, and
//! nothing is computed per tick.

use std::time::Duration;

/// Small feedback.
pub const SMALL: Duration = Duration::from_millis(150);
/// Most transitions.
pub const STANDARD: Duration = Duration::from_millis(250);
/// Large, spatial movements.
pub const LARGE: Duration = Duration::from_millis(400);

/// An easing of progress `t` from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    /// To enter.
    OutCubic,
    /// To leave.
    InCubic,
    InOutCubic,
}

impl Easing {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::OutCubic => 1.0 - (1.0 - t).powi(3),
            Self::InCubic => t * t * t,
            Self::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

/// A damped spring from 0 to 1, with unit mass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    /// 1 is critically damped: no overshoot; below 1 bounces.
    pub damping_ratio: f32,
}

impl Spring {
    /// Critically damped: the default for anything dragged or flicked.
    pub const SMOOTH: Spring = Spring {
        stiffness: 170.0,
        damping_ratio: 1.0,
    };
    /// A little bounce, where it means "settled".
    pub const SETTLE: Spring = Spring {
        stiffness: 210.0,
        damping_ratio: 0.72,
    };

    /// Position and velocity at `t` seconds, starting at `x0` with velocity
    /// `v0` towards rest at 0.
    pub fn state(self, x0: f32, v0: f32, t: f32) -> (f32, f32) {
        let w = self.stiffness.sqrt();
        let z = self.damping_ratio;
        if (z - 1.0).abs() < 1e-3 {
            // x(t) = (x0 + (v0 + w x0) t) e^(-w t)
            let c = v0 + w * x0;
            let e = (-w * t).exp();
            let x = (x0 + c * t) * e;
            let v = (c - w * (x0 + c * t)) * e;
            (x, v)
        } else if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * t).exp();
            let b = (v0 + z * w * x0) / wd;
            let (s, c) = (wd * t).sin_cos();
            let x = e * (x0 * c + b * s);
            let v = e * ((b * wd - z * w * x0) * c - (x0 * wd + z * w * b) * s);
            (x, v)
        } else {
            let r = w * (z * z - 1.0).sqrt();
            let (r1, r2) = (-z * w + r, -z * w - r);
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        }
    }

    /// When the spring is close enough to rest to stop drawing: within
    /// 1/1000 of the distance and nearly still.
    pub fn settles_after(self, x0: f32, v0: f32) -> Duration {
        let mut t = 0.0_f32;
        while t < 10.0 {
            let (x, v) = self.state(x0, v0, t);
            if x.abs() < 1e-3 * x0.abs().max(1.0) && v.abs() < 1e-2 * x0.abs().max(1.0) {
                break;
            }
            t += 1.0 / 240.0;
        }
        Duration::from_secs_f32(t)
    }
}

/// How a value moves to its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    Ease(Easing, Duration),
    Spring(Spring),
}

/// A value moving from where it was to a target, from a start time; a new
/// target retargets it from where it is, keeping a spring's velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animated {
    from: f32,
    to: f32,
    /// Velocity at the start, in units per second (springs).
    v0: f32,
    start: Duration,
    curve: Curve,
    /// When it is done.
    end: Duration,
}

impl Animated {
    /// A value at rest.
    pub fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            v0: 0.0,
            start: Duration::ZERO,
            curve: Curve::Ease(Easing::Linear, Duration::ZERO),
            end: Duration::ZERO,
        }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    /// Moves towards `to` from `now`.
    pub fn set(&mut self, to: f32, curve: Curve, now: Duration) {
        let (from, v0) = (self.value(now), self.velocity(now));
        let end = match curve {
            Curve::Ease(_, d) => now + d,
            Curve::Spring(spring) => now + spring.settles_after(from - to, v0),
        };
        *self = Self {
            from,
            to,
            v0,
            start: now,
            curve,
            end,
        };
    }

    /// Jumps to `to` without animating (reduced motion).
    pub fn jump(&mut self, to: f32) {
        *self = Self::new(to);
    }

    pub fn is_running(&self, now: Duration) -> bool {
        now < self.end
    }

    pub fn value(&self, now: Duration) -> f32 {
        if now >= self.end {
            return self.to;
        }
        let t = now.saturating_sub(self.start).as_secs_f32();
        match self.curve {
            Curve::Ease(easing, d) => {
                let p = if d.is_zero() {
                    1.0
                } else {
                    t / d.as_secs_f32()
                };
                self.from + (self.to - self.from) * easing.apply(p)
            }
            Curve::Spring(spring) => self.to + spring.state(self.from - self.to, self.v0, t).0,
        }
    }

    fn velocity(&self, now: Duration) -> f32 {
        if now >= self.end {
            return 0.0;
        }
        match self.curve {
            Curve::Spring(spring) => {
                let t = now.saturating_sub(self.start).as_secs_f32();
                spring.state(self.from - self.to, self.v0, t).1
            }
            Curve::Ease(..) => {
                // Close enough for retargeting from an eased value.
                let dt = Duration::from_millis(1);
                (self.value(now + dt) - self.value(now)) / dt.as_secs_f32()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for e in [
            Easing::Linear,
            Easing::OutCubic,
            Easing::InCubic,
            Easing::InOutCubic,
        ] {
            assert_eq!(e.apply(0.0), 0.0);
            assert_eq!(e.apply(1.0), 1.0);
        }
        assert!(Easing::OutCubic.apply(0.5) > 0.5);
        assert!(Easing::InCubic.apply(0.5) < 0.5);
    }

    #[test]
    fn a_critically_damped_spring_never_overshoots() {
        let mut a = Animated::new(0.0);
        a.set(100.0, Curve::Spring(Spring::SMOOTH), Duration::ZERO);
        let mut last = 0.0;
        for ms in (0..1000).step_by(8) {
            let v = a.value(MS(ms));
            assert!(v >= last - 1e-3 && v <= 100.0 + 1e-3, "{ms} ms: {v}");
            last = v;
        }
        assert!(!a.is_running(MS(2000)));
        assert_eq!(a.value(MS(2000)), 100.0);
    }

    #[test]
    fn a_bouncy_spring_overshoots_then_settles() {
        let mut a = Animated::new(0.0);
        a.set(100.0, Curve::Spring(Spring::SETTLE), Duration::ZERO);
        let peak = (0..600)
            .step_by(4)
            .map(|ms| a.value(MS(ms)))
            .fold(0.0, f32::max);
        assert!(peak > 100.5, "{peak}");
        assert!((a.value(MS(1500)) - 100.0).abs() < 0.5);
    }

    #[test]
    fn retargeting_starts_from_where_the_value_is() {
        let mut a = Animated::new(0.0);
        a.set(100.0, Curve::Ease(Easing::Linear, MS(200)), Duration::ZERO);
        let halfway = a.value(MS(100));
        assert!((halfway - 50.0).abs() < 1e-3);
        a.set(0.0, Curve::Spring(Spring::SMOOTH), MS(100));
        assert!((a.value(MS(100)) - halfway).abs() < 1e-3);
    }

    #[test]
    fn the_same_time_gives_the_same_value() {
        let mut a = Animated::new(10.0);
        a.set(-30.0, Curve::Spring(Spring::SETTLE), MS(5));
        assert_eq!(a.value(MS(123)), a.value(MS(123)));
    }
}
