//! The camera and microphone window's level meter (#619, lot 5): the
//! Hocuspanda microphone read (`hocus_voice::listen`) while the window
//! shows and an application uses the microphone, its voice's level kept
//! for the bars on the preview. The window's reading never turns a
//! microphone on by itself: with no application using one, the bars rest.

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

struct Live {
    _listener: hocus_voice::listen::Listener,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
/// The level, from 0 to 1, as it went: the newest last, a second of it.
static LEVELS: Mutex<VecDeque<(Instant, f32)>> = Mutex::new(VecDeque::new());

/// What is kept of the levels.
const KEPT: Duration = Duration::from_secs(1);
/// The level's floor and top, dBFS: a quiet room, a raised voice.
const FLOOR: f32 = -55.0;
const TOP: f32 = -12.0;

/// The meter reads while `on`, and only then.
pub fn want(on: bool) {
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if on == live.is_some() {
        return;
    }
    LEVELS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    if !on {
        *live = None;
        tracing::debug!("the voice's meter ends");
        return;
    }
    let mut smooth = 0f32;
    let listener = hocus_voice::listen::start(None, move |samples| {
        if samples.is_empty() {
            return;
        }
        let power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        let db = 10.0 * power.max(1e-12).log10();
        let level = ((db - FLOOR) / (TOP - FLOOR)).clamp(0.0, 1.0);
        // Up at once, down over some 150 ms.
        smooth = if level > smooth {
            level
        } else {
            smooth + (level - smooth) * 0.08
        };
        let now = Instant::now();
        let mut levels = LEVELS.lock().unwrap_or_else(PoisonError::into_inner);
        levels.push_back((now, smooth));
        while levels
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > KEPT)
        {
            levels.pop_front();
        }
    });
    tracing::debug!(started = listener.is_some(), "the voice's meter");
    *live = listener.map(|l| Live { _listener: l });
}

/// Whether it reads.
pub fn live() -> bool {
    LIVE.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

/// The level `ago` before `now`, from 0 to 1; 0 when nothing was heard.
pub fn level(now: Instant, ago: Duration) -> f32 {
    let levels = LEVELS.lock().unwrap_or_else(PoisonError::into_inner);
    let at = now.checked_sub(ago).unwrap_or(now);
    levels
        .iter()
        .rev()
        .find(|(when, _)| *when <= at)
        .map_or(0.0, |(_, l)| *l)
}
