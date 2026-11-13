//! How the windows move (#793): each one's velocity and acceleration, from
//! where it is drawn frame to frame, for the effects that react to it (the
//! snow shed by a window shaken, #795).

use std::{collections::HashMap, time::Duration};

use crate::model::WindowId;

/// The longest a frame counts for: after a pause, a window that moved
/// reads as if it moved in one frame of a 30 Hz screen, as the approved
/// board measured it.
const LONGEST_FRAME: f64 = 1.0 / 30.0;
/// Two looks closer than this are the same frame's.
const SHORTEST_FRAME: Duration = Duration::from_millis(1);
/// The velocity is smoothed over about this long, seconds: a window moves
/// by whole pixels, and one dragged slowly moves a pixel one frame in
/// three, which is no jolt. A shake still is.
const SMOOTHING: f64 = 0.025;

/// A window's motion as of its last frame, logical pixels and seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Motion {
    /// Its velocity, pixels a second.
    pub v: (f64, f64),
    /// How sharply its velocity changed, pixels a second each second.
    pub acc: f64,
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    at: Duration,
    pos: (f64, f64),
    motion: Motion,
}

/// The windows' motions, followed while an effect needs them.
#[derive(Debug, Default)]
pub struct Motions(HashMap<WindowId, Seen>);

impl Motions {
    /// Window `id` drawn at `pos` (its corner, global) at `now`: its motion
    /// since the last frame. A window seen first is still.
    pub fn track(&mut self, id: WindowId, pos: (f64, f64), now: Duration) -> Motion {
        let seen = self.0.entry(id).or_insert(Seen {
            at: now,
            pos,
            motion: Motion::default(),
        });
        if now < seen.at + SHORTEST_FRAME {
            return seen.motion;
        }
        let dt = (now - seen.at).as_secs_f64().min(LONGEST_FRAME);
        let k = dt / (SMOOTHING + dt);
        let last = seen.motion.v;
        let v = (
            last.0 + ((pos.0 - seen.pos.0) / dt - last.0) * k,
            last.1 + ((pos.1 - seen.pos.1) / dt - last.1) * k,
        );
        let acc = (v.0 - seen.motion.v.0).hypot(v.1 - seen.motion.v.1) / dt;
        seen.at = now;
        seen.pos = pos;
        seen.motion = Motion { v, acc };
        seen.motion
    }

    /// Forgets the windows `keep` says no to.
    pub fn retain(&mut self, mut keep: impl FnMut(WindowId) -> bool) {
        self.0.retain(|id, _| keep(*id));
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_window_starting_off_reads_its_speed_and_its_jolt() {
        let mut m = Motions::default();
        let id = WindowId(1);
        assert_eq!(m.track(id, (100.0, 50.0), ms(1000)), Motion::default());
        // Still, then 10 px in 20 ms: 500 px/s, smoothed, from 0 in 20 ms.
        assert_eq!(m.track(id, (100.0, 50.0), ms(1020)).acc, 0.0);
        let moved = m.track(id, (110.0, 50.0), ms(1040));
        let k = 0.02 / (SMOOTHING + 0.02);
        assert!((moved.v.0 - 500.0 * k).abs() < 1e-6, "{moved:?}");
        assert!((moved.acc - 25_000.0 * k).abs() < 1e-3, "{moved:?}");
        assert!(moved.acc > 10_000.0, "a jolt: {moved:?}");
        // The same frame again changes nothing.
        assert_eq!(m.track(id, (120.0, 50.0), ms(1040)), moved);
        // A pixel one frame in three is no jolt.
        let mut x = 120.0;
        let mut most: f64 = 0.0;
        for frame in 1..=60u64 {
            if frame % 3 == 0 {
                x += 1.0;
            }
            let acc = m.track(id, (x, 50.0), ms(2000 + frame * 17)).acc;
            if frame > 20 {
                most = most.max(acc);
            }
        }
        assert!(most < 2600.0, "{most}");
        m.retain(|_| false);
        assert_eq!(m.track(id, (0.0, 0.0), ms(2000)), Motion::default());
    }
}
