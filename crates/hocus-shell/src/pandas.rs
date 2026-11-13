//! The pandas as the shell draws them (#649): the validated drawings
//! (`hocus_pandas`), still, or a frame of the burst a panda plays when
//! something changes: its agent's state, the opening of the surface that
//! shows it, the pointer coming over it. A burst lasts a few seconds
//! ([`hocus_pandas::lasts`]); a surface asks to be drawn again only while
//! one of its pandas plays one, so nothing is drawn at rest.
//!
//! Pandas play only when the shell moves things (not with reduced motion,
//! nor on the CPU) and the compositor says they may (not in power saver):
//! [`lively`]. Otherwise they are the still drawings, at their sizes.
//!
//! In Givre they wear its ice visor, in Matrix Neo's glasses
//! ([`hocus_pandas::Dress`], #740, #431), still or moving alike.

use std::{
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use iced::advanced::svg;

pub use hocus_pandas::{BREATH, Dress, Mood, VIEW_BOX};

use crate::agents::Look;

static LIVELY: AtomicBool = AtomicBool::new(true);

/// Whether the pandas play their moods now.
pub fn lively() -> bool {
    LIVELY.load(Ordering::Relaxed)
}

/// The compositor said whether the pandas play (and the shell moves).
pub fn set_lively(lively: bool) {
    if LIVELY.swap(lively, Ordering::Relaxed) != lively {
        tracing::debug!(lively, "the pandas play their moods");
    }
}

/// An agent's row's look as its panda's mood.
pub fn mood(look: Look) -> Mood {
    match look {
        Look::Waiting => Mood::Waiting,
        Look::Stuck => Mood::Stuck,
        // Good news: its arms up among its confetti.
        Look::ToSee | Look::News => Mood::Done,
        Look::Working => Mood::Working,
        Look::Resting => Mood::Resting,
    }
}

/// A panda playing its mood: since when, for how long its parts move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Burst {
    pub since: Instant,
    pub window: Duration,
}

impl Burst {
    /// `mood`'s burst from `now`: about 2.5 s, one breath for a resting
    /// panda.
    pub fn of(mood: Mood, now: Instant) -> Self {
        Self::lasting(now, mood.burst())
    }

    /// A burst from `now` whose parts move for `window`.
    pub fn lasting(now: Instant, window: Duration) -> Self {
        Self { since: now, window }
    }

    /// Whether it still draws frames at `now`.
    pub fn plays(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) < hocus_pandas::lasts(self.window)
    }
}

/// How the theme of the moment dresses the pandas.
pub fn dress() -> Dress {
    if crate::theme::frost() {
        Dress::Visor
    } else if crate::theme::matrix() {
        Dress::Glasses
    } else {
        Dress::Plain
    }
}

/// `mood`'s still drawing, in the theme's dress.
pub fn still(mood: Mood) -> svg::Handle {
    const DRESSES: [Dress; 3] = [Dress::Plain, Dress::Visor, Dress::Glasses];
    static STILL: OnceLock<[[svg::Handle; 6]; 3]> = OnceLock::new();
    let dresses = STILL.get_or_init(|| {
        DRESSES.map(|dress| {
            Mood::ALL.map(|m| svg::Handle::from_memory(dress.wear(m.still(), m).into_bytes()))
        })
    });
    let dress = DRESSES.iter().position(|d| *d == dress()).unwrap_or(0);
    dresses[dress][Mood::ALL.iter().position(|m| *m == mood).unwrap_or(0)].clone()
}

/// `mood` at `now`: a frame of `burst` while it plays (and the pandas are
/// lively), else the still drawing.
pub fn drawing(mood: Mood, burst: Option<Burst>, now: Instant) -> svg::Handle {
    drawn(mood, burst, now, None)
}

/// The same, each drawing through `wrap` (the peek's shadow); still
/// drawings wrapped once.
pub fn drawn(
    mood: Mood,
    burst: Option<Burst>,
    now: Instant,
    wrap: Option<fn(&str) -> String>,
) -> svg::Handle {
    played(mood, burst.filter(|_| lively()), now, wrap)
}

/// `mood` at `now`, a frame of `burst` while it plays.
fn played(
    mood: Mood,
    burst: Option<Burst>,
    now: Instant,
    wrap: Option<fn(&str) -> String>,
) -> svg::Handle {
    let dress = dress();
    let frame = burst.and_then(|b| {
        let t = now.saturating_duration_since(b.since);
        // On whole steps: the same moment, the same drawing (and iced's
        // cache of it).
        let t = hocus_pandas::STEP * hocus_pandas::step(t);
        Some((b.window, hocus_pandas::step(t))).zip(hocus_pandas::frame(mood, b.window, t))
    });
    match (frame, wrap) {
        (Some((key, svg)), wrap) => cached(mood, key, dress, wrap, || {
            let svg = dress.wear(&svg, mood);
            match wrap {
                Some(wrap) => wrap(&svg),
                None => svg,
            }
        }),
        (None, None) => still(mood),
        (None, Some(wrap)) => cached(mood, (Duration::MAX, 0), dress, Some(wrap), || {
            wrap(&dress.wear(mood.still(), mood))
        }),
    }
}

type Key = (Mood, (Duration, u32), Dress, Option<usize>);

/// The last frames made, so that several bars drawing the same panda at
/// the same moment make it once.
fn cached(
    mood: Mood,
    at: (Duration, u32),
    dress: Dress,
    wrap: Option<fn(&str) -> String>,
    make: impl FnOnce() -> String,
) -> svg::Handle {
    const KEPT: usize = 24;
    static FRAMES: Mutex<Vec<(Key, svg::Handle)>> = Mutex::new(Vec::new());
    let key = (mood, at, dress, wrap.map(|w| w as usize));
    let mut frames = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, handle)) = frames.iter().find(|(k, _)| *k == key) {
        return handle.clone();
    }
    let handle = svg::Handle::from_memory(make().into_bytes());
    if frames.len() >= KEPT {
        frames.remove(0);
    }
    frames.push((key, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_plays_about_2_5_s_then_the_still_drawing_is_drawn() {
        let now = Instant::now();
        let burst = Burst::of(Mood::Waiting, now);
        assert!(burst.plays(now + Duration::from_millis(2899)));
        assert!(!burst.plays(now + Duration::from_millis(2900)));
        let resting = Burst::of(Mood::Resting, now);
        assert!(resting.plays(now + Duration::from_millis(4299)));
        assert!(!resting.plays(now + Duration::from_millis(4300)));
        let at = |ms| now + Duration::from_millis(ms);
        let mid = played(Mood::Waiting, Some(burst), at(700), None);
        assert_ne!(mid.id(), still(Mood::Waiting).id());
        // The same moment, the same drawing.
        assert_eq!(
            played(Mood::Waiting, Some(burst), at(700), None).id(),
            mid.id()
        );
        let after = played(Mood::Waiting, Some(burst), at(3000), None);
        assert_eq!(after.id(), still(Mood::Waiting).id());
        // Without a burst (no motion), the still drawing.
        assert_eq!(
            played(Mood::Waiting, None, at(700), None).id(),
            still(Mood::Waiting).id()
        );
    }
}
