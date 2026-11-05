//! The trial area of "Bureaux et fenêtres" (6.14, "ESSAYER ICI"): a small
//! screen at the desktop's real sizes with a fixed "Terminal" and a
//! "Notes" window the pointer moves, by its title bar or by its content
//! while the move key is held. It sticks, lines up and snaps with the
//! compositor's own geometry ([`hocus_geometry`]), as the page's settings
//! say, and wobbles as a shear that follows the pointer and settles with
//! an overshoot. Nothing in it is written.
//!
//! Plain data and decisions on a clock the caller gives, tested without
//! a window; the page draws it and feeds it the pointer.

use std::time::Duration;

use hocus_geometry::{ALL, GAP, Pull, Rect, TEAR_OFF, Zone, pull, torn_off, unpulled};

/// The screen, logical pixels.
pub const SCREEN: (i32, i32) = (640, 230);
/// The fixed window.
pub const TERMINAL: Rect = Rect::new(24, 28, 210, 140);
/// The window moved: its size, and where it starts.
pub const NOTES: Rect = Rect::new(318, 64, 184, 124);
/// The windows' title bars.
pub const TITLE: i32 = 22;
/// A zone taken in this long (windows.md §9's 200 ms spring).
pub const SETTLE: Duration = Duration::from_millis(200);
/// The wobble settles in about this long.
pub const WOBBLE: Duration = Duration::from_millis(400);
/// The most the top of the window lags, logical pixels.
const SHEAR_MOST: f64 = 14.0;

/// The work area of the screen: the gap all round.
pub fn area() -> Rect {
    Rect::new(0, 0, SCREEN.0, SCREEN.1).inset(GAP)
}

/// What the page's settings make of a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    pub magnet: bool,
    pub snap: bool,
    pub wobble: bool,
    pub reduced: bool,
}

/// What happened, as the line under the screen says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// Nothing done yet.
    Start,
    /// A press on the content without the key: the application's click.
    NeedsKey,
    /// Held against the Terminal, at the gap.
    Stuck,
    /// Held flush with the screen's edge.
    Edge,
    /// Over a zone: let go, it takes it.
    Over(Zone),
    /// It took the zone.
    Snapped(Zone),
    /// Let go where it was.
    Placed,
}

/// A drag under way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    /// The pointer where the drag began, and the window's corner then.
    from: (f64, f64),
    initial: (i32, i32),
    /// Snapped when grabbed: where it was held across (0 to 1) and down,
    /// until it is torn off.
    held: Option<(f64, f64, i32)>,
    pull: Pull,
    zone: Option<Zone>,
    /// The last pointer and its time, for the wobble's speed.
    last: (f64, f64, Duration),
}

/// The trial's state.
#[derive(Debug, Clone, PartialEq)]
pub struct Trial {
    /// Where "Notes" is.
    pub notes: Rect,
    /// In a zone, and its size before.
    snapped: Option<(Zone, (i32, i32))>,
    drag: Option<Drag>,
    /// Going into its zone: from there, since then.
    settle: Option<(Rect, Duration)>,
    /// The wobble: its lag (pixels, the top behind the bottom) as the
    /// pointer last moved, and since when it settles.
    shear: (f64, Duration),
    pub said: Said,
}

impl Default for Trial {
    fn default() -> Self {
        Self {
            notes: NOTES,
            snapped: None,
            drag: None,
            settle: None,
            shear: (0.0, Duration::ZERO),
            said: Said::Start,
        }
    }
}

impl Trial {
    /// A press of the pointer at `at` (the screen's coordinates); `key`:
    /// the move key is held. Whether the window takes it.
    pub fn press(&mut self, at: (f64, f64), key: bool, now: Duration) -> bool {
        if !self.notes.contains(at.0, at.1) {
            return false;
        }
        let title = at.1 < f64::from(self.notes.y + TITLE);
        if !title && !key {
            self.said = Said::NeedsKey;
            return true;
        }
        self.settle = None;
        let held = self.snapped.map(|_| {
            (
                (at.0 - f64::from(self.notes.x)) / f64::from(self.notes.width.max(1)),
                at.1 - f64::from(self.notes.y),
                self.notes.height,
            )
        });
        self.drag = Some(Drag {
            from: at,
            initial: (self.notes.x, self.notes.y),
            held,
            pull: Pull::default(),
            zone: None,
            last: (at.0, at.1, now),
        });
        true
    }

    /// Whether a window is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The pointer moved to `at` at `now`.
    pub fn motion(&mut self, at: (f64, f64), rules: Rules, now: Duration) {
        let Some(mut drag) = self.drag else {
            return;
        };
        let (dx, dy) = (at.0 - drag.from.0, at.1 - drag.from.1);
        // A snapped window dragged far enough: its size back under the
        // pointer.
        if let Some((along, down, height)) = drag.held {
            if dx.hypot(dy) < TEAR_OFF {
                return;
            }
            let size = self.snapped.map_or((NOTES.width, NOTES.height), |(_, s)| s);
            let (x, y) = torn_off(size, along, (down, height), at.0, at.1);
            self.snapped = None;
            self.notes = Rect::new(x, y, size.0, size.1);
            drag.from = at;
            drag.initial = (x, y);
            drag.held = None;
            self.drag = Some(drag);
            return;
        }
        let before_pull = drag.pull;
        let x = drag.initial.0 + dx.round() as i32;
        let y = drag.initial.1 + dy.round() as i32;
        let free = Rect::new(x, y, self.notes.width, self.notes.height);
        let area = area();
        drag.zone = if rules.snap {
            Zone::at(area, at.0, at.1)
        } else {
            None
        };
        drag.pull = if rules.magnet && drag.zone.is_none() {
            pull(free, ALL, &[TERMINAL], area)
        } else {
            Pull::default()
        };
        let (px, py) = drag.pull.offset();
        let before = (self.notes, before_pull);
        self.notes = Rect::new(x + px, y + py, free.width, free.height);
        // The wobble follows the window's motion, not the magnet's jumps:
        // held still by the magnet, it does not bend (windows.md §6).
        let dt = now.saturating_sub(drag.last.2).as_secs_f64();
        if rules.wobble && !rules.reduced && dt > 0.0 {
            let moved = (self.notes.x - before.0.x, self.notes.y - before.0.y);
            let (across, _) = unpulled(moved, before.1, drag.pull);
            let speed = f64::from(across) / dt;
            let lag = (speed * 0.02).clamp(-SHEAR_MOST, SHEAR_MOST);
            let now_lag = self.shear_at(now);
            self.shear = (0.6 * lag + 0.4 * now_lag, now);
        }
        drag.last = (at.0, at.1, now);
        self.said = match drag.zone {
            Some(zone) => Said::Over(zone),
            None => self.stuck(drag.pull),
        };
        self.drag = Some(drag);
    }

    /// What holds it, the magnet's pull `pull` given.
    fn stuck(&self, pull: Pull) -> Said {
        if pull.x.is_none() && pull.y.is_none() {
            return Said::Placed;
        }
        let n = self.notes;
        let t = TERMINAL;
        let beside = n.x == t.right() + GAP
            || n.right() == t.x - GAP
            || n.y == t.bottom() + GAP
            || n.bottom() == t.y - GAP
            || n.x == t.x
            || n.y == t.y
            || n.right() == t.right()
            || n.bottom() == t.bottom();
        if beside { Said::Stuck } else { Said::Edge }
    }

    /// The pointer let go at `now`: over a zone, it takes it.
    pub fn release(&mut self, rules: Rules, now: Duration) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        self.shear = (self.shear_at(now), now);
        let Some(zone) = drag.zone else {
            if matches!(self.said, Said::Over(_)) {
                self.said = Said::Placed;
            }
            return;
        };
        let size = (self.notes.width, self.notes.height);
        let target = zone.rect(area());
        if !rules.reduced {
            self.settle = Some((self.notes, now));
        }
        self.snapped = Some((zone, size));
        self.notes = target;
        self.said = Said::Snapped(zone);
    }

    /// The zone shown under the pointer while dragging.
    pub fn preview(&self) -> Option<Rect> {
        self.drag.and_then(|d| d.zone).map(|z| z.rect(area()))
    }

    /// The zone it is snapped to.
    pub fn snapped(&self) -> Option<Zone> {
        self.snapped.map(|(z, _)| z)
    }

    /// Where it is drawn at `now`: on its way into its zone.
    pub fn drawn(&self, now: Duration) -> (f64, f64, f64, f64) {
        let to = self.notes;
        let rect = |r: Rect| {
            (
                f64::from(r.x),
                f64::from(r.y),
                f64::from(r.width),
                f64::from(r.height),
            )
        };
        let Some((from, since)) = self.settle else {
            return rect(to);
        };
        let t = (now.saturating_sub(since).as_secs_f64() / SETTLE.as_secs_f64()).min(1.0);
        // A spring's ease-out, a little past and back.
        let e = 1.0 - (1.0 - t).powi(3) + 0.08 * (std::f64::consts::PI * t).sin() * (1.0 - t);
        let lerp = |a: f64, b: f64| a + (b - a) * e;
        let (a, b) = (rect(from), rect(to));
        (
            lerp(a.0, b.0),
            lerp(a.1, b.1),
            lerp(a.2, b.2),
            lerp(a.3, b.3),
        )
    }

    /// How far its top lags behind at `now`: the lag as the window last
    /// moved, settling with an overshoot in about [`WOBBLE`] once it stops.
    pub fn shear_at(&self, now: Duration) -> f64 {
        let (lag, since) = self.shear;
        if lag == 0.0 {
            return 0.0;
        }
        // Held still, even with the button down, it settles too.
        let t = now.saturating_sub(since).as_secs_f64() / WOBBLE.as_secs_f64();
        if t >= 1.0 {
            return 0.0;
        }
        // Damped: past the middle once, then still.
        lag * (-4.0 * t).exp() * (std::f64::consts::PI * 1.5 * t).cos()
    }

    /// Whether it still moves at `now`: into its zone, or wobbling.
    pub fn moving(&self, now: Duration) -> bool {
        self.settle
            .is_some_and(|(_, since)| now.saturating_sub(since) < SETTLE)
            || (self.shear.0 != 0.0 && now.saturating_sub(self.shear.1) < WOBBLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules {
        magnet: true,
        snap: true,
        wobble: true,
        reduced: false,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Grabbed by its title bar and moved by (`dx`, `dy`).
    fn dragged(trial: &mut Trial, dx: f64, dy: f64, rules: Rules) {
        let (x, y) = (
            f64::from(trial.notes.x) + 40.0,
            f64::from(trial.notes.y) + 10.0,
        );
        assert!(trial.press((x, y), false, ms(0)));
        trial.motion((x + dx, y + dy), rules, ms(16));
    }

    #[test]
    fn it_sticks_to_the_terminal_at_the_gap() {
        let mut trial = Trial::default();
        // Its left edge 10 px short of the gap right of the Terminal.
        let dx = f64::from(TERMINAL.right() + GAP + 10 - NOTES.x);
        dragged(&mut trial, dx, 0.0, RULES);
        assert_eq!(trial.notes.x, TERMINAL.right() + GAP);
        assert_eq!(trial.said, Said::Stuck);
        // The magnet off: where the pointer puts it.
        let mut trial = Trial::default();
        let rules = Rules {
            magnet: false,
            ..RULES
        };
        dragged(&mut trial, dx, 0.0, rules);
        assert_eq!(trial.notes.x, TERMINAL.right() + GAP + 10);
        assert_eq!(trial.said, Said::Placed);
    }

    #[test]
    fn the_content_needs_the_key_the_title_bar_does_not() {
        let mut trial = Trial::default();
        let inside = (f64::from(NOTES.x) + 50.0, f64::from(NOTES.y) + 60.0);
        assert!(trial.press(inside, false, ms(0)));
        assert!(!trial.dragging());
        assert_eq!(trial.said, Said::NeedsKey);
        assert!(trial.press(inside, true, ms(0)));
        assert!(trial.dragging());
        assert!(
            !trial.press((5.0, 5.0), true, ms(0)),
            "the screen, not a window"
        );
    }

    #[test]
    fn against_a_side_it_takes_the_half_and_dragged_away_its_size_back() {
        let mut trial = Trial::default();
        let (x, y) = (f64::from(NOTES.x) + 40.0, f64::from(NOTES.y) + 10.0);
        trial.press((x, y), false, ms(0));
        trial.motion((3.0, 120.0), RULES, ms(16));
        assert_eq!(trial.said, Said::Over(Zone::Left));
        assert_eq!(trial.preview(), Some(Zone::Left.rect(area())));
        trial.release(RULES, ms(32));
        assert_eq!(trial.snapped(), Some(Zone::Left));
        assert_eq!(trial.notes, Zone::Left.rect(area()));
        assert!(trial.moving(ms(100)), "it springs into place");
        assert!(!trial.moving(ms(700)));
        // Dragged away: its size back, under the pointer.
        let at = (100.0, 15.0);
        trial.press(at, false, ms(1000));
        trial.motion((at.0 + 5.0, at.1), RULES, ms(1010));
        assert_eq!(trial.snapped(), Some(Zone::Left), "not torn off yet");
        trial.motion((at.0 + 60.0, at.1 + 40.0), RULES, ms(1030));
        assert_eq!(trial.snapped(), None);
        assert_eq!(
            (trial.notes.width, trial.notes.height),
            (NOTES.width, NOTES.height)
        );
        assert!(trial.notes.contains(at.0 + 60.0, at.1 + 40.0));
    }

    #[test]
    fn without_snapping_an_edge_takes_nothing() {
        let mut trial = Trial::default();
        let rules = Rules {
            snap: false,
            ..RULES
        };
        let (x, y) = (f64::from(NOTES.x) + 40.0, f64::from(NOTES.y) + 10.0);
        trial.press((x, y), false, ms(0));
        trial.motion((3.0, 120.0), rules, ms(16));
        assert_eq!(trial.preview(), None);
        trial.release(rules, ms(32));
        assert_eq!(trial.snapped(), None);
    }

    #[test]
    fn it_wobbles_while_moved_and_settles_and_not_without_the_wobble() {
        let mut trial = Trial::default();
        dragged(&mut trial, 120.0, 0.0, RULES);
        let lag = trial.shear_at(ms(16));
        assert!(lag.abs() > 1.0, "{lag}");
        trial.release(RULES, ms(20));
        assert!(trial.moving(ms(200)));
        assert_eq!(trial.shear_at(ms(500)), 0.0);
        for rules in [
            Rules {
                wobble: false,
                ..RULES
            },
            Rules {
                reduced: true,
                ..RULES
            },
        ] {
            let mut trial = Trial::default();
            dragged(&mut trial, 120.0, 0.0, rules);
            assert_eq!(trial.shear_at(ms(16)), 0.0);
        }
        // Held still by the magnet, it does not bend.
        let mut trial = Trial::default();
        let dx = f64::from(TERMINAL.right() + GAP + 4 - NOTES.x);
        let (x, y) = (f64::from(NOTES.x) + 40.0, f64::from(NOTES.y) + 10.0);
        trial.press((x, y), false, ms(0));
        trial.motion((x + dx, y), RULES, ms(16));
        for i in 1..=10u32 {
            trial.motion(
                (x + dx - f64::from(i) * 0.8, y),
                RULES,
                ms(16 + 16 * u64::from(i)),
            );
        }
        assert_eq!(trial.notes.x, TERMINAL.right() + GAP);
        assert!(
            trial.shear_at(ms(200)).abs() < 1.0,
            "{}",
            trial.shear_at(ms(200))
        );
        // Reduced motion: the zone at once.
        let mut trial = Trial::default();
        let reduced = Rules {
            reduced: true,
            ..RULES
        };
        let (x, y) = (f64::from(NOTES.x) + 40.0, f64::from(NOTES.y) + 10.0);
        trial.press((x, y), false, ms(0));
        trial.motion((3.0, 120.0), reduced, ms(16));
        trial.release(reduced, ms(32));
        assert!(!trial.moving(ms(33)));
    }
}
