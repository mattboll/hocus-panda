//! Window motion on the frame clock (#33): each window's opacity and scale
//! are `hocus_ui::motion` values, evaluated at the clock's time when the
//! scene is drawn. While one runs, the loop asks for frames, which the
//! output's clock paces; at rest it asks for none. Nothing is queued per
//! frame, and a new target retargets a running value from where it is.
//! Reduced motion turns movement into a short fade.

use std::time::Duration;

use hocus_ui::motion::{Animated, Curve, Easing, SMALL, STANDARD};

/// Where an opening window starts from, and where a closing one goes.
const OPEN_SCALE: f32 = 0.96;
/// A dialog appears from its parent with a shorter scale.
const DIALOG_SCALE: f32 = 0.92;

/// A window's opacity and scale, as they move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowMotion {
    opacity: Animated,
    scale: Animated,
}

impl Default for WindowMotion {
    fn default() -> Self {
        Self::still()
    }
}

impl WindowMotion {
    /// Shown as it is.
    pub fn still() -> Self {
        Self {
            opacity: Animated::new(1.0),
            scale: Animated::new(1.0),
        }
    }

    /// Opening (docs/design/motion.md): a fade and a scale from 96 %,
    /// out-cubic over the standard duration; with reduced motion, a short
    /// fade only.
    pub fn opening(now: Duration, reduced: bool) -> Self {
        Self::appearing(now, reduced, OPEN_SCALE, STANDARD)
    }

    /// A dialog: from its parent, smaller and quicker.
    pub fn dialog(now: Duration, reduced: bool) -> Self {
        Self::appearing(now, reduced, DIALOG_SCALE, SMALL)
    }

    fn appearing(now: Duration, reduced: bool, from: f32, duration: Duration) -> Self {
        let mut motion = Self {
            opacity: Animated::new(0.0),
            scale: Animated::new(if reduced { 1.0 } else { from }),
        };
        let (curve, duration) = if reduced {
            (Easing::Linear, SMALL)
        } else {
            (Easing::OutCubic, duration)
        };
        motion.opacity.set(1.0, Curve::Ease(curve, duration), now);
        if !reduced {
            motion.scale.set(1.0, Curve::Ease(curve, duration), now);
        }
        motion
    }

    /// Closing (docs/design/motion.md): it dissolves, shrinking back to
    /// 96 %, in-cubic, from wherever it is now; with reduced motion, a short
    /// fade only.
    pub fn closing(&mut self, now: Duration, reduced: bool) {
        if reduced {
            self.opacity
                .set(0.0, Curve::Ease(Easing::Linear, SMALL), now);
            return;
        }
        let curve = Curve::Ease(Easing::InCubic, STANDARD);
        self.opacity.set(0.0, curve, now);
        self.scale.set(OPEN_SCALE, curve, now);
    }

    /// Fades to `opacity` from wherever it is now.
    pub fn fade_to(&mut self, opacity: f32, duration: Duration, now: Duration) {
        self.opacity
            .set(opacity, Curve::Ease(Easing::OutCubic, duration), now);
    }

    /// Opacity and scale at `now`.
    pub fn at(&self, now: Duration) -> (f32, f32) {
        (self.opacity.value(now), self.scale.value(now))
    }

    pub fn is_running(&self, now: Duration) -> bool {
        self.opacity.is_running(now) || self.scale.is_running(now)
    }
}

impl crate::State {
    /// Whether any window is moving at `now`.
    pub fn animating(&self, now: Duration) -> bool {
        self.windows.values().any(|m| m.motion.is_running(now)) || self.snapshots_moving(now)
    }

    /// A window's motion as it opens (a dialog's when it has a parent), or
    /// none in tests that ask for instant motion.
    pub fn opening_motion(&self, dialog: bool) -> WindowMotion {
        if self.instant_motion {
            return WindowMotion::still();
        }
        let (now, reduced) = (self.clock.now(), self.config.appearance.reduced_motion);
        if dialog {
            WindowMotion::dialog(now, reduced)
        } else {
            WindowMotion::opening(now, reduced)
        }
    }

    /// After each dispatch: frames keep coming while something moves, and
    /// stop when it is still; pictures done moving go.
    pub fn refresh_motion(&mut self) {
        self.settle_snapshots();
        if self.animating(self.clock.now()) {
            self.needs_redraw = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn opening_fades_and_grows_then_rests() {
        let m = WindowMotion::opening(ms(1000), false);
        assert_eq!(m.at(ms(1000)), (0.0, OPEN_SCALE));
        let (alpha, scale) = m.at(ms(1125));
        assert!(alpha > 0.5 && alpha < 1.0, "{alpha}");
        assert!(scale > OPEN_SCALE && scale < 1.0, "{scale}");
        assert_eq!(m.at(ms(1250)), (1.0, 1.0));
        assert!(!m.is_running(ms(1250)));
    }

    #[test]
    fn reduced_motion_only_fades_and_shortly() {
        let m = WindowMotion::opening(ms(0), true);
        assert_eq!(m.at(ms(0)).1, 1.0, "never scaled");
        assert!(m.at(ms(75)).0 > 0.4 && m.at(ms(75)).0 < 0.6);
        assert!(!m.is_running(ms(150)));
    }

    #[test]
    fn a_retarget_halfway_goes_on_from_where_it_was() {
        let mut m = WindowMotion::opening(ms(0), false);
        let (before, _) = m.at(ms(125));
        m.fade_to(0.0, STANDARD, ms(125));
        let (after, _) = m.at(ms(125));
        assert!(
            (before - after).abs() < 1e-6,
            "no jump: {before} then {after}"
        );
        assert!(m.at(ms(200)).0 < before, "on its way back");
        assert_eq!(m.at(ms(375)).0, 0.0);
    }

    #[test]
    fn closing_halfway_through_opening_dissolves_from_there() {
        let mut m = WindowMotion::opening(ms(0), false);
        let (alpha, scale) = m.at(ms(100));
        m.closing(ms(100), false);
        assert_eq!(m.at(ms(100)), (alpha, scale), "from where it was");
        let (later, _) = m.at(ms(200));
        assert!(later < alpha, "fading");
        assert_eq!(m.at(ms(350)), (0.0, OPEN_SCALE));
        assert!(!m.is_running(ms(350)));
    }

    #[test]
    fn dialogs_open_quicker() {
        let m = WindowMotion::dialog(ms(0), false);
        assert_eq!(m.at(ms(0)).1, DIALOG_SCALE);
        assert!(!m.is_running(ms(150)));
    }
}
