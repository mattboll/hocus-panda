//! The volume above 100 % (`[sound] boost`, #897, bar.md §4.2 and §9):
//! with it on, the outputs go up to 150 %, the sliders' last third
//! amplified; the keys and the wheel stop once at 100 %, and go beyond it
//! only when pressed again soon after.
//!
//! `wpctl` says a volume as the cube root of the linear one: 150 % is
//! PipeWire's 3.375, which may crackle; the applications' own volumes
//! stay at 100 % at most.

use std::time::{Duration, Instant};

/// The most a volume goes without boost, in percent.
pub const MOST: f32 = 100.0;
/// The most it goes with boost.
pub const BOOST_MOST: f32 = 150.0;
/// A key pressed again within this long of stopping at 100 % goes beyond
/// it; after a longer pause, it stops there again.
pub const STOP_FOR: Duration = Duration::from_secs(1);
/// A volume this close to 100 % is at it (`wpctl` rounds to a thousandth).
const AT: f32 = 0.5;

/// The most a volume goes, with `boost` or not.
pub const fn most(boost: bool) -> f32 {
    if boost { BOOST_MOST } else { MOST }
}

/// The volume at `share` of a slider (0 to 1), in whole percent: the
/// whole track is 0 to 100 %, or with `boost` 0 to 150 %, 100 % two thirds
/// along.
pub fn volume_at(share: f32, boost: bool) -> f32 {
    (share.clamp(0.0, 1.0) * most(boost)).round()
}

/// Where `volume` is along a slider, from 0 to 1.
pub fn share_of(volume: f32, boost: bool) -> f32 {
    (volume / most(boost)).clamp(0.0, 1.0)
}

/// Where 100 % is along a slider with boost: its notch, where the amber
/// starts.
pub const MARK: f32 = MOST / BOOST_MOST;

/// The keys' and the wheel's stop at 100 %, with boost: when the last
/// step stopped there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stop {
    at: Option<Instant>,
}

impl Stop {
    /// The most `steps` (up less down) taken from `volume` at `now` may
    /// reach, in percent: 100 % on the way up from below or at it, unless
    /// a step stopped at 100 % within [`STOP_FOR`]; 150 % above it or on
    /// the way down. Without `boost`, always 100 %.
    pub fn most(&mut self, volume: f32, steps: i32, boost: bool, now: Instant) -> f32 {
        if !boost {
            return MOST;
        }
        if steps <= 0 || volume > MOST + AT {
            return BOOST_MOST;
        }
        if volume >= MOST - AT {
            if self
                .at
                .is_some_and(|at| now.saturating_duration_since(at) <= STOP_FOR)
            {
                return BOOST_MOST;
            }
            self.at = Some(now);
            return MOST;
        }
        if volume + crate::STEP * steps as f32 >= MOST - AT {
            self.at = Some(now);
        }
        MOST
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sliders_place_is_a_volume_to_100_or_to_150_with_boost() {
        assert_eq!(volume_at(0.0, false), 0.0);
        assert_eq!(volume_at(0.5, false), 50.0);
        assert_eq!(volume_at(1.0, false), 100.0);
        // With boost: the first two thirds to 100 %, the last to 150 %.
        assert_eq!(volume_at(MARK, true), 100.0);
        assert_eq!(volume_at(0.5, true), 75.0);
        assert_eq!(volume_at(0.8, true), 120.0);
        assert_eq!(volume_at(1.0, true), 150.0);
        assert_eq!(volume_at(1.4, true), 150.0);
        // And back: 120 % is 80 % along; without boost, full.
        assert!((share_of(120.0, true) - 0.8).abs() < 1e-6);
        assert_eq!(share_of(120.0, false), 1.0);
        assert!((share_of(100.0, true) - MARK).abs() < 1e-6);
    }

    #[test]
    fn the_keys_stop_once_at_100_then_go_beyond_if_pressed_again_soon() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        // Without boost: 100 %, always.
        assert_eq!(Stop::default().most(100.0, 1, false, t0), MOST);
        let mut stop = Stop::default();
        // From 95 % up: it stops at 100 %.
        assert_eq!(stop.most(95.0, 1, true, t0), MOST);
        // At 100 % within a second: beyond.
        assert_eq!(stop.most(100.0, 1, true, t0 + ms(600)), BOOST_MOST);
        // Above it: up to 150 %; down: never held.
        assert_eq!(stop.most(105.0, 1, true, t0 + ms(5000)), BOOST_MOST);
        assert_eq!(stop.most(105.0, -1, true, t0 + ms(5000)), BOOST_MOST);
        // At 100 % after a pause: it stops there again, once.
        let later = t0 + ms(8000);
        assert_eq!(stop.most(100.0, 1, true, later), MOST);
        assert_eq!(stop.most(100.0, 1, true, later + ms(300)), BOOST_MOST);
        // Steps that do not reach 100 % set no stop: at 100 % a while
        // after them, the first press stops.
        let mut stop = Stop::default();
        assert_eq!(stop.most(50.0, 2, true, t0), MOST);
        assert_eq!(stop.most(100.0, 1, true, t0 + ms(200)), MOST);
        // A fast spin from 90 % (three notches): stopped at 100 %.
        let mut stop = Stop::default();
        assert_eq!(stop.most(90.0, 3, true, t0), MOST);
        assert_eq!(stop.most(100.0, 1, true, t0 + ms(1500)), MOST);
    }
}
