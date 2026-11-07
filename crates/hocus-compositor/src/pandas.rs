//! The pandas the compositor draws (#649): the overview's badges and its
//! empty desktops' sleepers, the radial menu's hub. The validated drawings
//! (`hocus_pandas`), drawn with resvg on the CPU at the screen's scale, a
//! picture per frame of a burst, each kept by its drawer: at rest the
//! pictures stay and nothing is drawn again.
//!
//! A burst's frames come 30 a second: the compositor's pictures are
//! drawn on the loop, so fewer of them than the shell's.

use std::time::Duration;

use hocus_pandas::Mood;
use resvg::{tiny_skia, usvg};

use crate::overview::AgentState;

/// Frames of a burst a second.
const STEPS: f64 = 30.0;

/// The mood of a window's agents, by the state that needs the user most.
pub fn mood(state: AgentState) -> Mood {
    match state {
        AgentState::Waiting => Mood::Waiting,
        AgentState::Failed => Mood::Stuck,
        AgentState::Done => Mood::Done,
        AgentState::Working => Mood::Working,
        AgentState::Idle => Mood::Resting,
    }
}

/// A frame of a panda: its burst's window and the step it is at; none,
/// the still drawing.
pub type Frame = Option<(Duration, u16)>;

/// The frame of a burst of `window` `t` into it: none once it is over.
pub fn frame(window: Duration, t: Duration) -> Frame {
    (t < hocus_pandas::lasts(window))
        .then(|| (window, (t.as_secs_f64() * STEPS).floor().min(f64::from(u16::MAX)) as u16))
}

/// `mood` at `frame`, `side` pixels square (its drawing as wide, in the
/// middle): premultiplied ARGB.
pub fn draw(mood: Mood, frame: Frame, side: u32) -> Option<Vec<u8>> {
    let drawing = frame
        .and_then(|(window, step)| {
            hocus_pandas::frame(mood, window, Duration::from_secs_f64(f64::from(step) / STEPS))
        })
        .unwrap_or_else(|| mood.still().to_owned());
    let tree = usvg::Tree::from_str(&drawing, &usvg::Options::default())
        .inspect_err(|err| tracing::warn!(%err, mood = mood.name(), "a panda's drawing"))
        .ok()?;
    let side = side.max(1);
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let k = side as f32 / hocus_pandas::VIEW_BOX.0;
    let tall = hocus_pandas::VIEW_BOX.1 * k;
    let at = tiny_skia::Transform::from_scale(k, k).post_translate(0.0, (side as f32 - tall) / 2.0);
    resvg::render(&tree, at, &mut pixmap.as_mut());
    Some(crate::radial_draw::argb_of(pixmap.take()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_has_frames_then_none() {
        let window = Mood::Waiting.burst();
        assert_eq!(frame(window, Duration::ZERO), Some((window, 0)));
        assert_eq!(frame(window, Duration::from_millis(1000)), Some((window, 30)));
        assert_eq!(frame(window, hocus_pandas::lasts(window)), None);
    }

    #[test]
    fn a_panda_is_drawn_still_or_moving() {
        let still = draw(Mood::Waiting, None, 64).expect("drawn");
        assert_eq!(still.len(), 64 * 64 * 4);
        assert!(still.chunks(4).any(|px| px[3] == 255));
        let moving = draw(Mood::Waiting, frame(BURST, Duration::from_millis(650)), 64);
        assert_ne!(moving.as_deref(), Some(still.as_slice()));
    }

    const BURST: Duration = hocus_pandas::BURST;
}
