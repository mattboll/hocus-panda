//! Frame callbacks for the windows nobody sees, and for those that show
//! nothing new at rest (#949).
//!
//! A window's frame callbacks are answered at each refresh of the screen
//! it shows on: an application drawing goes at the screen's pace. Two
//! kinds wait longer:
//!
//! - **Unseen**: on its desktop, but the last frames drew none of its
//!   surfaces: covered whole by opaque surfaces. Answered once a second
//!   at most ([`UNSEEN_EVERY`]); it was answered at every refresh
//!   of every screen, a browser under another window committed at 165 Hz
//!   for nothing. (A window on a desktop not shown, or minimised, is not
//!   in the space: no refresh answers it, as before.)
//! - **Quiet**: shown, but committing nothing new (#840) for
//!   [`QUIET_AFTER`] with no input, as a browser whose page runs a loop
//!   that draws nothing. Answered ten times a second at most
//!   ([`QUIET_EVERY`]): what it shows next comes soon enough to be seen
//!   without input. Its commits ask for no refresh; a frame its
//!   screen draws anyway still answers it. Any input brings every quiet
//!   window back to its screen's pace at once: the user may be about to
//!   look at it, and its next frame comes at the next refresh.
//!
//! When no screen draws, a timer answers them, every [`QUIET_EVERY`]
//! while a quiet one waits for its callback, every [`UNSEEN_EVERY`] while
//! only unseen ones do; it is not set when none does. While
//! windows are drawn elsewhere (the overview, Alt+Tab's thumbnails, a
//! window moving), every window goes at the screens' pace as before.

use smithay::{
    backend::renderer::element::{Id, RenderElementPresentationState, RenderElementStates},
    desktop::Window,
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    wayland::compositor::SurfaceAttributes,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use crate::{State, model::WindowId};

/// How often an unseen window is answered at most.
pub const UNSEEN_EVERY: Duration = Duration::from_secs(1);

/// How often a quiet window is answered at most.
pub const QUIET_EVERY: Duration = Duration::from_millis(100);

/// How long a shown window commits nothing new, with no input, before it
/// is quiet.
pub const QUIET_AFTER: Duration = Duration::from_secs(2);

/// The timer's own threshold for a pace, a little under it: set again
/// from when it was due, it may fire a little less than its pace after the
/// callback it sent last.
fn timer_throttle(every: Duration) -> Duration {
    every * 9 / 10
}

#[derive(Debug, Default)]
pub struct SlowFrames {
    /// Since when each window commits nothing new, input aside.
    nothing_new: HashMap<WindowId, Duration>,
    /// The timer while it is set, and its pace: a frame that leaves an
    /// unseen window waiting sets it too, with the state borrowed.
    timer: Cell<Option<(RegistrationToken, Duration)>>,
}

/// The screens whose last frame drew a window's surfaces; `None` until a
/// frame judged it.
#[derive(Debug, Default)]
struct ShownOn(RefCell<Option<Vec<Output>>>);

impl State {
    /// Whether the last frames drew none of `window`'s surfaces, on any
    /// screen still plugged.
    pub(crate) fn window_unseen(&self, window: &Window) -> bool {
        window
            .user_data()
            .get::<ShownOn>()
            .and_then(|shown| {
                shown
                    .0
                    .borrow()
                    .as_ref()
                    .map(|on| !on.iter().any(|o| self.is_plugged(o)))
            })
            .unwrap_or(false)
    }

    /// Notes whether the frame of `output` with `states` drew `window`:
    /// one of its surfaces drawn, or, none of them in the scene, the
    /// window drawn as a picture (raining, bent) where it is.
    pub(crate) fn judge_shown(
        &self,
        window: &Window,
        output: &Output,
        states: &RenderElementStates,
    ) {
        let (mut stated, mut drawn) = (false, false);
        window.with_surfaces(|surface, _| {
            if let Some(state) = states.element_render_state(Id::from_wayland_resource(surface)) {
                stated = true;
                drawn |= state.presentation_state != RenderElementPresentationState::Skipped;
            }
        });
        let here = drawn || (!stated && self.drawn_as_picture(window, output));
        let data = window.user_data();
        data.insert_if_missing(ShownOn::default);
        let Some(shown) = data.get::<ShownOn>() else {
            return;
        };
        let mut shown = shown.0.borrow_mut();
        let on = shown.get_or_insert_with(Vec::new);
        on.retain(|o| o != output);
        if here {
            on.push(output.clone());
        }
    }

    /// Whether `window`, none of its surfaces in a frame of `output`, is
    /// in sight there all the same: drawn as a picture.
    fn drawn_as_picture(&self, window: &Window, output: &Output) -> bool {
        // Not one of the desktop's windows: told as before.
        let Some(id) = self.id_of(window) else {
            return true;
        };
        let Some(area) = self.window_area(id, self.now()) else {
            return false;
        };
        self.space
            .output_geometry(output)
            .is_some_and(|g| g.to_f64().overlaps(area))
    }

    /// Whether window `id` is quiet: shown, committing nothing new for
    /// [`QUIET_AFTER`] with no input.
    fn window_quiet(&self, id: WindowId) -> bool {
        self.slow_frames
            .nothing_new
            .get(&id)
            .is_some_and(|since| self.now().saturating_sub(*since) >= QUIET_AFTER)
    }

    /// Window `id` committed nothing new: whether its frame callbacks wait
    /// for the slow pace, unseen or quiet. The timer is set then.
    pub(crate) fn nothing_new(&mut self, id: WindowId) -> bool {
        let now = self.now();
        self.slow_frames.nothing_new.entry(id).or_insert(now);
        let Some(window) = self.mapped_window(id) else {
            return false;
        };
        // Covered whole, it keeps the unseen windows' pace, quiet or not.
        if self.window_unseen(&window) {
            self.slow_frames_due(UNSEEN_EVERY);
            true
        } else if self.window_quiet(id) {
            self.slow_frames_due(QUIET_EVERY);
            true
        } else {
            false
        }
    }

    /// Window `id` committed something new: back to its screen's pace;
    /// unseen, the timer answers it.
    pub(crate) fn something_new(&mut self, id: WindowId) {
        self.slow_frames.nothing_new.remove(&id);
        if self
            .mapped_window(id)
            .is_some_and(|w| self.window_unseen(&w))
        {
            self.slow_frames_due(UNSEEN_EVERY);
        }
    }

    /// Window `id`, if it is in the space: on its desktop, not minimised.
    fn mapped_window(&self, id: WindowId) -> Option<Window> {
        let window = &self.windows.get(&id)?.window;
        self.space
            .elements()
            .any(|w| w == window)
            .then(|| window.clone())
    }

    /// The user's input: every quiet window back to its screen's pace,
    /// its waiting callback answered at the next refresh.
    pub(crate) fn slow_frames_input(&mut self) {
        if self.slow_frames.nothing_new.is_empty() {
            return;
        }
        let quiet: Vec<WindowId> = self
            .slow_frames
            .nothing_new
            .keys()
            .copied()
            .filter(|id| self.window_quiet(*id))
            .collect();
        self.slow_frames.nothing_new.clear();
        for id in quiet {
            self.owe_frames_of(Some(id));
        }
    }

    /// The timer set at `every`, unless it is at that pace or faster.
    pub(crate) fn slow_frames_due(&self, every: Duration) {
        if self.locked() {
            return;
        }
        match self.slow_frames.timer.get() {
            Some((_, pace)) if pace <= every => return,
            // Too slow for a quiet window: set again at its pace.
            Some((token, _)) => self.loop_handle.remove(token),
            None => {}
        }
        self.slow_frames.timer.set(None);
        let timer = Timer::from_duration(every);
        let inserted = self
            .loop_handle
            .insert_source(timer, |_, _, app| app.state.slow_frames_fired());
        match inserted {
            Ok(token) => self.slow_frames.timer.set(Some((token, every))),
            Err(err) => tracing::warn!(%err.error, "the unseen windows' frames have no timer"),
        }
    }

    /// The timer fired: the unseen and quiet windows answered, if their
    /// last answer is old enough; again while one still waits, at the
    /// quiet windows' pace while one of them does.
    fn slow_frames_fired(&mut self) -> TimeoutAction {
        let windows = &self.windows;
        self.slow_frames
            .nothing_new
            .retain(|id, _| windows.contains_key(id));
        // Locked, only the lock's surfaces draw (7.1): the frames after it
        // answer the others.
        let output = self.space.outputs().next().cloned();
        let (Some(output), false) = (output, self.locked()) else {
            self.slow_frames.timer.set(None);
            return TimeoutAction::Drop;
        };
        let now: Duration = self.clock.now().into();
        let mut next: Option<Duration> = None;
        for window in self.space.elements() {
            let every = if self.window_unseen(window) {
                UNSEEN_EVERY
            } else if self.id_of(window).is_some_and(|id| self.window_quiet(id)) {
                QUIET_EVERY
            } else {
                continue;
            };
            window.send_frame(&output, now, Some(timer_throttle(every)), |_, _| None);
            if waits_for_frame(window) {
                next = Some(next.map_or(every, |n| n.min(every)));
            }
        }
        match (next, self.slow_frames.timer.get()) {
            (Some(every), Some((token, _))) => {
                self.slow_frames.timer.set(Some((token, every)));
                TimeoutAction::ToDuration(every)
            }
            _ => {
                self.slow_frames.timer.set(None);
                TimeoutAction::Drop
            }
        }
    }
}

/// Whether one of `window`'s surfaces has a frame callback not answered.
pub(crate) fn waits_for_frame(window: &Window) -> bool {
    let mut waits = false;
    window.with_surfaces(|_, data| {
        waits |= !data
            .cached_state
            .get::<SurfaceAttributes>()
            .current()
            .frame_callbacks
            .is_empty();
    });
    waits
}
