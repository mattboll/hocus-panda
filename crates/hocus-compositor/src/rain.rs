//! A window's rain (#432, docs/design/windows.md §5): with `[windows]
//! animation = "rain"`, an application's window or dialog larger than
//! 200 × 200 px opens in 360 ms as columns of glyphs fall through it, the
//! window showing behind each head, and closes in 300 ms as its columns
//! drain downward. A window closed while it opens drains back from where
//! it is. At most three rains at once (the others open and close as
//! standard), none for the windows of the last session coming back, none
//! with reduced motion or in the power saver (their usual motion then).
//!
//! The rain is a start time and a seed: each frame the shader
//! ([`crate::matrix`]) computes every column from the time. The window is
//! drawn into a picture (kept from frame to frame while its size stays,
//! drawn once for a closing window's last frame) and the shader draws the
//! picture. Input goes to the window from its first frame: the rain is
//! only how it is drawn. On pixman the window fades over the same time
//! instead, so the frames counted are the same.

use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use crate::{State, config::Animation, model::WindowId, wobble::Kept};

pub const OPEN: Duration = Duration::from_millis(360);
pub const CLOSE: Duration = Duration::from_millis(300);
/// At most this many rains at once.
pub const AT_ONCE: usize = 3;
/// A window smaller than this either way opens as standard.
pub const SMALLEST: i32 = 200;

/// Which way a rain goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fall {
    Open,
    Close,
    /// Closed while it opened: the opening played back from where it was
    /// (seconds into it) to nothing.
    Back(f32),
}

/// A window's rain under way.
#[derive(Debug)]
pub struct Rain {
    pub start: Duration,
    pub fall: Fall,
    pub seed: f32,
    pub id: Id,
    /// The window's picture, the renderer's.
    pub picture: RefCell<Option<Kept>>,
    /// The window committed since its picture was drawn, or its frame
    /// changed (its rim's focus, as drawn): drawn again.
    pub stale: Cell<bool>,
    pub drawn_focus: Cell<f32>,
    commits: Cell<CommitCounter>,
}

impl Rain {
    pub fn new(start: Duration, fall: Fall, seed: f32) -> Self {
        Self {
            start,
            fall,
            seed,
            id: Id::new(),
            picture: RefCell::new(None),
            stale: Cell::new(true),
            drawn_focus: Cell::new(-1.0),
            commits: Cell::new(CommitCounter::default()),
        }
    }

    pub fn duration(&self) -> Duration {
        match self.fall {
            Fall::Open => OPEN,
            Fall::Close => CLOSE,
            Fall::Back(from) => Duration::from_secs_f32(from.max(0.0)),
        }
    }

    pub fn done(&self, now: Duration) -> bool {
        now >= self.start + self.duration()
    }

    fn elapsed(&self, now: Duration) -> f32 {
        now.saturating_sub(self.start)
            .min(self.duration())
            .as_secs_f32()
    }

    /// The shader's time at `now` and whether it closes: an opening's
    /// seconds (played backward for [`Fall::Back`]), or a closing's.
    pub fn shader_time(&self, now: Duration) -> (f32, bool) {
        let t = self.elapsed(now);
        match self.fall {
            Fall::Open => (t, false),
            Fall::Close => (t, true),
            Fall::Back(from) => ((from - t).max(0.0), false),
        }
    }

    /// How opaque the window is drawn where the shader is not (pixman):
    /// fading in over the opening, out over the closing.
    pub fn fade(&self, now: Duration) -> f32 {
        let (t, closing) = self.shader_time(now);
        if closing {
            1.0 - t / CLOSE.as_secs_f32()
        } else {
            t / OPEN.as_secs_f32()
        }
        .clamp(0.0, 1.0)
    }

    /// The commit of this frame's drawing: a new one each frame.
    pub fn next_commit(&self) -> CommitCounter {
        let mut c = self.commits.get();
        c.increment();
        self.commits.set(c);
        c
    }
}

impl State {
    /// Whether windows rain now as they open and close.
    fn rain_chosen(&self) -> bool {
        self.config.windows.animation == Animation::Rain
            && !self.reduced_motion()
            && !self.power_saving()
    }

    /// The rains under way, opening and closing.
    pub fn rains(&self) -> usize {
        self.windows.values().filter(|m| m.rain.is_some()).count()
            + self.leaving.iter().filter(|l| l.rain.is_some()).count()
    }

    fn rain_seed(&mut self) -> f32 {
        self.rain_seeds = self.rain_seeds.wrapping_add(1);
        (self.rain_seeds % 997) as f32 * 1.618
    }

    /// Window `id` of size `size` maps at `now`: its rain, if it rains.
    pub(crate) fn rain_on_map(&mut self, size: (i32, i32), now: Duration) -> Option<Rain> {
        let rains = self.rain_chosen()
            && size.0 > SMALLEST
            && size.1 > SMALLEST
            && !self.session_restoring()
            && self.rains() < AT_ONCE;
        rains.then(|| {
            let seed = self.rain_seed();
            Rain::new(now, Fall::Open, seed)
        })
    }

    /// Window `id`, `size` large, closes at `now`: its rain, if it rains:
    /// back from where its opening is, or down.
    pub(crate) fn rain_on_close(
        &mut self,
        id: WindowId,
        size: (f64, f64),
        now: Duration,
    ) -> Option<Rain> {
        let opening = self
            .windows
            .get_mut(&id)
            .and_then(|m| m.rain.take())
            .filter(|r| !r.done(now));
        if let Some(opening) = opening {
            let (t, _) = opening.shader_time(now);
            return Some(Rain::new(now, Fall::Back(t), opening.seed));
        }
        let rains = self.rain_chosen()
            && size.0 > f64::from(SMALLEST)
            && size.1 > f64::from(SMALLEST)
            && self.rains() < AT_ONCE;
        rains.then(|| {
            let seed = self.rain_seed();
            Rain::new(now, Fall::Close, seed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_rain_closed_while_opening_goes_back_from_where_it_was() {
        let open = Rain::new(ms(1000), Fall::Open, 1.0);
        assert_eq!(open.shader_time(ms(1000)), (0.0, false));
        assert!((open.fade(ms(1180)) - 0.5).abs() < 1e-3);
        assert!(open.done(ms(1360)));
        let (t, _) = open.shader_time(ms(1100));
        let back = Rain::new(ms(1100), Fall::Back(t), 1.0);
        // Never more shown than when it closed, nothing left at its end.
        let mut last = back.shader_time(ms(1100)).0;
        for k in 0..=10 {
            let (now, closing) = back.shader_time(ms(1100 + k * 10));
            assert!(!closing && now <= last + 1e-6);
            last = now;
        }
        assert_eq!(back.shader_time(ms(1200)).0, 0.0);
        assert!(back.done(ms(1201)));
        let close = Rain::new(ms(0), Fall::Close, 1.0);
        assert_eq!(close.fade(ms(0)), 1.0);
        assert_eq!(close.fade(ms(300)), 0.0);
    }
}
