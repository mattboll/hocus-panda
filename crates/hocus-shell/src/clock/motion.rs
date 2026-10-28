//! The minute change's motions, from the 3.2 mockup's CSS keyframes: each
//! changed digit's new glyph comes in, and for some transitions the old one
//! goes out, inside the digit's cell, which clips them.
//!
//! A pose is where a glyph is at a time: moved by a fraction of its line's
//! height, its opacity, a vertical blur, and a fold (a turn about a
//! horizontal axis, drawn as a vertical squash: `cos` of the angle).

use std::time::Duration;

use hocus_link::Transition;

/// A glyph at one moment of its motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Moved down by this fraction of its line's height (up when negative).
    pub dy: f32,
    pub alpha: f32,
    /// Vertical blur, logical pixels.
    pub blur: f32,
    /// Its height's scale, folded about `pivot`: 1 flat, 0 edge on,
    /// negative past it.
    pub fold: f32,
    pub pivot: Pivot,
}

/// The edge a fold turns about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pivot {
    Top,
    Bottom,
}

impl Pose {
    pub const REST: Self = Self {
        dy: 0.0,
        alpha: 1.0,
        blur: 0.0,
        fold: 1.0,
        pivot: Pivot::Top,
    };
}

/// CSS's `cubic-bezier(x1, y1, x2, y2)` at linear progress `x`.
fn bezier((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    let axis = |p1: f32, p2: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..30 {
        let t = (lo + hi) / 2.0;
        if axis(x1, x2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    axis(y1, y2, (lo + hi) / 2.0)
}

const OVERSHOOT: (f32, f32, f32, f32) = (0.2, 0.9, 0.25, 1.08);
const ODOMETER_OUT: (f32, f32, f32, f32) = (0.4, 0.0, 0.8, 0.4);
const FLIP_IN: (f32, f32, f32, f32) = (0.3, 0.7, 0.3, 1.2);
const EASE_IN: (f32, f32, f32, f32) = (0.42, 0.0, 1.0, 1.0);

fn mix(a: f32, b: f32, p: f32) -> f32 {
    a + (b - a) * p
}

/// Progress through the keyframe segment from `from` to `to` (fractions of
/// the motion) at `x`, eased as CSS eases each segment.
fn segment(x: f32, from: f32, to: f32, curve: (f32, f32, f32, f32)) -> f32 {
    bezier(curve, (x - from) / (to - from))
}

/// The new glyph's motion and the old one's, as durations.
fn durations(transition: Transition) -> (Duration, Option<Duration>) {
    let ms = Duration::from_millis;
    match transition {
        Transition::Odometer => (ms(520), Some(ms(360))),
        Transition::Roll => (ms(340), None),
        Transition::Flip => (ms(480), Some(ms(220))),
        Transition::Neon => (ms(700), None),
        Transition::None => (Duration::ZERO, None),
    }
}

/// How long a minute change moves.
pub fn duration(transition: Transition) -> Duration {
    let (incoming, outgoing) = durations(transition);
    incoming.max(outgoing.unwrap_or_default())
}

fn progress(elapsed: Duration, of: Duration) -> f32 {
    if of.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / of.as_secs_f32()).clamp(0.0, 1.0)
}

/// The new digit `elapsed` into the motion.
pub fn incoming(transition: Transition, elapsed: Duration) -> Pose {
    let x = progress(elapsed, durations(transition).0);
    match transition {
        // 0%: 105% below, blurred 3px; 70%: 8% above, sharp; 100%: home.
        Transition::Odometer => {
            if x < 0.7 {
                let p = segment(x, 0.0, 0.7, OVERSHOOT);
                Pose {
                    dy: mix(1.05, -0.08, p),
                    blur: mix(3.0, 0.0, p),
                    ..Pose::REST
                }
            } else {
                Pose {
                    dy: mix(-0.08, 0.0, segment(x, 0.7, 1.0, OVERSHOOT)),
                    ..Pose::REST
                }
            }
        }
        // From 60% below, transparent.
        Transition::Roll => {
            let p = bezier(OVERSHOOT, x);
            Pose {
                dy: mix(0.6, 0.0, p),
                alpha: p.clamp(0.0, 1.0),
                ..Pose::REST
            }
        }
        // Turned -92° about its top, 20% opaque; 60%: 12° past, opaque;
        // 100%: flat.
        Transition::Flip => {
            let (angle, alpha) = if x < 0.6 {
                let p = segment(x, 0.0, 0.6, FLIP_IN);
                (mix(-92.0, 12.0, p), mix(0.2, 1.0, p))
            } else {
                (mix(12.0, 0.0, segment(x, 0.6, 1.0, FLIP_IN)), 1.0)
            };
            Pose {
                alpha: alpha.clamp(0.0, 1.0),
                fold: angle.to_radians().cos(),
                pivot: Pivot::Top,
                ..Pose::REST
            }
        }
        // Linear opacity: 0, 1 at 8%, 0.15 at 14%, 1 at 22%, 0.4 at 30%,
        // 1 from 38%.
        Transition::Neon => {
            const STEPS: [(f32, f32); 7] = [
                (0.0, 0.0),
                (0.08, 1.0),
                (0.14, 0.15),
                (0.22, 1.0),
                (0.30, 0.4),
                (0.38, 1.0),
                (1.0, 1.0),
            ];
            let alpha = STEPS.windows(2).find(|w| x <= w[1].0).map_or(1.0, |w| {
                mix(w[0].1, w[1].1, (x - w[0].0) / (w[1].0 - w[0].0))
            });
            Pose {
                alpha,
                ..Pose::REST
            }
        }
        Transition::None => Pose::REST,
    }
}

/// The old digit `elapsed` into the motion; `None` once gone, or when the
/// transition does not show it.
pub fn outgoing(transition: Transition, elapsed: Duration) -> Option<Pose> {
    let of = durations(transition).1?;
    if elapsed >= of {
        return None;
    }
    let x = progress(elapsed, of);
    match transition {
        // Up 105%, fading, blurring to 3px.
        Transition::Odometer => {
            let p = bezier(ODOMETER_OUT, x);
            Some(Pose {
                dy: mix(0.0, -1.05, p),
                alpha: 1.0 - p,
                blur: mix(0.0, 3.0, p),
                ..Pose::REST
            })
        }
        // Turned to 92° about its bottom, fading.
        Transition::Flip => {
            let p = bezier(EASE_IN, x);
            Some(Pose {
                alpha: 1.0 - p,
                fold: mix(0.0, 92.0, p).to_radians().cos(),
                pivot: Pivot::Bottom,
                ..Pose::REST
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_odometer_comes_from_below_overshoots_and_lands() {
        let t = Transition::Odometer;
        assert_eq!(duration(t), ms(520));
        let start = incoming(t, ms(0));
        assert!((start.dy - 1.05).abs() < 1e-3 && start.blur == 3.0);
        // The overshoot: above home at about 70%.
        let top = (0..=520)
            .map(|m| incoming(t, ms(m)).dy)
            .fold(f32::MAX, f32::min);
        assert!((-0.1..-0.07).contains(&top), "{top}");
        assert!(incoming(t, ms(364)).blur < 0.01);
        assert_eq!(incoming(t, ms(520)), Pose::REST);
        assert_eq!(incoming(t, ms(5000)), Pose::REST);
        // The old one leaves upward, fading and blurring, gone at 360 ms.
        let out = outgoing(t, ms(180)).expect("leaving");
        assert!(out.dy < 0.0 && out.alpha < 1.0 && out.blur > 0.0);
        assert_eq!(outgoing(t, ms(360)), None);
    }

    #[test]
    fn every_motion_ends_at_rest() {
        for t in Transition::ALL {
            let end = duration(t);
            assert_eq!(incoming(t, end), Pose::REST, "{t:?}");
            assert_eq!(outgoing(t, end), None, "{t:?}");
        }
        assert_eq!(duration(Transition::None), Duration::ZERO);
        assert_eq!(duration(Transition::Neon), ms(700));
    }

    #[test]
    fn flip_and_neon_follow_their_keyframes() {
        let flip = incoming(Transition::Flip, ms(0));
        assert!(flip.fold < 0.0 && (flip.alpha - 0.2).abs() < 1e-3);
        assert_eq!(flip.pivot, Pivot::Top);
        let past = incoming(Transition::Flip, ms(288));
        assert!((past.fold - 12f32.to_radians().cos()).abs() < 1e-3);
        let out = outgoing(Transition::Flip, ms(110)).expect("folding");
        assert_eq!(out.pivot, Pivot::Bottom);
        assert!(out.fold < 1.0);
        let neon = |m| incoming(Transition::Neon, ms(m)).alpha;
        assert!((neon(56) - 1.0).abs() < 1e-3);
        assert!((neon(98) - 0.15).abs() < 1e-3);
        assert!((neon(210) - 0.4).abs() < 1e-3);
        assert!((neon(400) - 1.0).abs() < 1e-3);
        assert_eq!(outgoing(Transition::Neon, ms(10)), None);
        assert_eq!(outgoing(Transition::Roll, ms(10)), None);
    }
}
