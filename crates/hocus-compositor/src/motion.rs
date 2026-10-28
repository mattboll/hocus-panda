//! Motion on the frame clock (docs/design/windows.md §4–§10): each
//! animation is a start time, a duration and a curve, evaluated in closed
//! form at the time a frame is drawn. While one runs, drawing a frame asks
//! for the next one, which the output's clock paces; at rest nothing is
//! drawn. Nothing is queued per tick.

use std::time::Duration;

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bezier(pub f64, pub f64, pub f64, pub f64);

impl Bezier {
    /// The windows' spring: a quick start and a slight overshoot.
    pub const SPRING: Self = Self(0.2, 0.9, 0.25, 1.08);
    /// CSS's `ease-in`, to leave.
    pub const EASE_IN: Self = Self(0.42, 0.0, 1.0, 1.0);
    /// CSS's `ease-out`, to arrive.
    pub const EASE_OUT: Self = Self(0.0, 0.0, 0.58, 1.0);
    pub const LINEAR: Self = Self(0.0, 0.0, 1.0, 1.0);
    /// The desktops' strip: fast away, landing softly (§12).
    pub const STRIP: Self = Self(0.22, 0.9, 0.3, 1.0);

    fn axis(p1: f64, p2: f64, t: f64) -> f64 {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    }

    fn slope(p1: f64, p2: f64, t: f64) -> f64 {
        let u = 1.0 - t;
        3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
    }

    /// The curve's value at progress `x` from 0 to 1.
    pub fn at(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        if x == 0.0 || x == 1.0 {
            return x;
        }
        let Self(x1, y1, x2, y2) = self;
        // Newton's method on x(t) = x, then bisection if it strays.
        let mut t = x;
        for _ in 0..8 {
            let err = Self::axis(x1, x2, t) - x;
            let d = Self::slope(x1, x2, t);
            if err.abs() < 1e-7 || d.abs() < 1e-9 {
                break;
            }
            t = (t - err / d).clamp(0.0, 1.0);
        }
        if (Self::axis(x1, x2, t) - x).abs() > 1e-5 {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..40 {
                t = (lo + hi) / 2.0;
                if Self::axis(x1, x2, t) < x {
                    lo = t;
                } else {
                    hi = t;
                }
            }
        }
        Self::axis(y1, y2, t)
    }
}

/// An animation: from its start, over its duration, along its curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    pub start: Duration,
    pub duration: Duration,
    pub curve: Bezier,
}

impl Tween {
    pub fn new(start: Duration, duration: Duration, curve: Bezier) -> Self {
        Self {
            start,
            duration,
            curve,
        }
    }

    /// Linear progress at `now`, from 0 to 1.
    pub fn linear(&self, now: Duration) -> f64 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_sub(self.start).as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// The curve's value at `now`: 0 at the start, 1 at the end (and a
    /// little past it on the way, for a spring).
    pub fn at(&self, now: Duration) -> f64 {
        self.curve.at(self.linear(now))
    }

    pub fn done(&self, now: Duration) -> bool {
        now >= self.start + self.duration
    }
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Durations of slice 1 (docs/design/windows.md).
pub const OPEN: Duration = Duration::from_millis(220);
pub const CLOSE: Duration = Duration::from_millis(160);
/// Opening and closing with reduced motion: a fade, no scaling.
pub const FADE_REDUCED: Duration = Duration::from_millis(120);
pub const FOCUS_RIM: Duration = Duration::from_millis(120);
pub const PULSE: Duration = Duration::from_millis(600);
pub const SETTLE: Duration = Duration::from_millis(200);
pub const MINIMIZE: Duration = Duration::from_millis(220);
pub const GRIP_IN: Duration = Duration::from_millis(120);
/// The grip stays this long after the pointer leaves, then fades.
pub const GRIP_STAYS: Duration = Duration::from_millis(300);
pub const PREVIEW: Duration = Duration::from_millis(120);
/// The pointer stays this long in a zone before its preview shows.
pub const PREVIEW_DELAY: Duration = Duration::from_millis(80);

/// The desktops' strip slides in this time (§12).
pub const DESKTOP_SLIDE: Duration = Duration::from_millis(320);
/// Alt+Tab (§11): held this long, its panel shows; it appears and
/// vanishes in these times, scaling from this much; its selection's ring
/// moves in this time.
pub const SWITCHER_DELAY: Duration = Duration::from_millis(100);
pub const SWITCHER_IN: Duration = Duration::from_millis(150);
pub const SWITCHER_OUT: Duration = Duration::from_millis(100);
pub const SWITCHER_RING: Duration = Duration::from_millis(120);
pub const SWITCHER_SCALE: f64 = 0.96;

/// Where an opening window starts, and where a closing one goes.
pub const OPEN_SCALE: f64 = 0.94;
pub const CLOSE_SCALE: f64 = 0.97;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curves_start_at_0_and_end_at_1() {
        for curve in [
            Bezier::SPRING,
            Bezier::EASE_IN,
            Bezier::EASE_OUT,
            Bezier::LINEAR,
            Bezier::STRIP,
        ] {
            assert_eq!(curve.at(0.0), 0.0);
            assert_eq!(curve.at(1.0), 1.0);
        }
        assert!((Bezier::LINEAR.at(0.3) - 0.3).abs() < 1e-6);
        // Ease-in starts slow, ease-out fast.
        assert!(Bezier::EASE_IN.at(0.25) < 0.15);
        assert!(Bezier::EASE_OUT.at(0.25) > 0.35);
    }

    #[test]
    fn the_spring_overshoots_a_little_then_lands() {
        let peak = (1..100)
            .map(|i| Bezier::SPRING.at(f64::from(i) / 100.0))
            .fold(0.0, f64::max);
        assert!(peak > 1.0 && peak < 1.05, "{peak}");
        assert!(Bezier::SPRING.at(0.3) > 0.7, "quick to start");
    }

    #[test]
    fn a_tween_is_sampled_at_any_time() {
        let ms = Duration::from_millis;
        let open = Tween::new(ms(1000), OPEN, Bezier::LINEAR);
        assert_eq!(open.at(ms(900)), 0.0);
        assert!((open.at(ms(1110)) - 0.5).abs() < 1e-6);
        assert!(!open.done(ms(1219)));
        assert!(open.done(ms(1220)));
        assert_eq!(open.at(ms(5000)), 1.0);
    }
}
