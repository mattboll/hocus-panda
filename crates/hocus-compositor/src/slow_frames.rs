//! Frame callbacks for the windows nobody sees, and for those that show
//! nothing new at rest (#949).
//!
//! A window's frame callbacks are answered at each refresh of the screen
//! it shows on: an application drawing goes at the screen's pace. Two
//! kinds wait longer, answered once a second at most:
//!
//! - **Unseen**: on its desktop, but the last frames drew none of its
//!   surfaces: covered whole by opaque surfaces. Answered at every refresh
//!   of every screen, a browser under another window committed at 165 Hz
//!   for nothing. (A window on a desktop not shown, or minimised, is not
//!   in the space: no refresh answers it, as before.)
//! - **Quiet**: shown, but committing nothing new (#840) for
//!   [`QUIET_AFTER`] with no input, as a browser whose page runs a loop
//!   that draws nothing. Its commits ask for no refresh; a frame its
//!   screen draws anyway still answers it. Any input brings every quiet
//!   window back to its screen's pace at once: the user may be about to
//!   look at it, and its next frame comes at the next refresh.
//!
//! When no screen draws, a timer answers them, every [`SLOW`] while one
//! of them waits for its callback; it is not set when none does. While
//! windows are drawn elsewhere (the overview, Alt+Tab's thumbnails, a
//! window moving), every window goes at the screens' pace as before.

use smithay::{
    backend::renderer::element::{Id, RenderElementPresentationState, RenderElementStates},
    desktop::Window,
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    wayland::compositor::SurfaceAttributes,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use crate::{State, model::WindowId};

/// How often an unseen or quiet window is answered at most.
pub const SLOW: Duration = Duration::from_secs(1);

/// How long a shown window commits nothing new, with no input, before it
/// is quiet.
pub const QUIET_AFTER: Duration = Duration::from_secs(2);

/// The timer's own threshold, a little under [`SLOW`]: set again from
/// when it was due, it may fire a little less than [`SLOW`] after the
/// callback it sent last.
const TIMER_THROTTLE: Duration = Duration::from_millis(900);

#[derive(Debug, Default)]
pub struct SlowFrames {
    /// Since when each window commits nothing new, input aside.
    nothing_new: HashMap<WindowId, Duration>,
    /// Whether the timer is set: a frame that leaves an unseen window
    /// waiting sets it too, with the state borrowed.
    timer: Cell<bool>,
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
        let slow = self.window_quiet(id) || self.window_unseen(&window);
        if slow {
            self.slow_frames_due();
        }
        slow
    }

    /// Window `id` committed something new: back to its screen's pace;
    /// unseen, the timer answers it.
    pub(crate) fn something_new(&mut self, id: WindowId) {
        self.slow_frames.nothing_new.remove(&id);
        if self
            .mapped_window(id)
            .is_some_and(|w| self.window_unseen(&w))
        {
            self.slow_frames_due();
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

    /// The timer set, unless it is.
    pub(crate) fn slow_frames_due(&self) {
        if self.slow_frames.timer.get() || self.locked() {
            return;
        }
        let timer = Timer::from_duration(SLOW);
        let inserted = self
            .loop_handle
            .insert_source(timer, |_, _, app| app.state.slow_frames_fired());
        match inserted {
            Ok(_) => self.slow_frames.timer.set(true),
            Err(err) => tracing::warn!(%err.error, "the unseen windows' frames have no timer"),
        }
    }

    /// The timer fired: the unseen and quiet windows answered, if their
    /// last answer is old enough; again in [`SLOW`] while one still waits.
    fn slow_frames_fired(&mut self) -> TimeoutAction {
        let windows = &self.windows;
        self.slow_frames
            .nothing_new
            .retain(|id, _| windows.contains_key(id));
        // Locked, only the lock's surfaces draw (7.1): the frames after it
        // answer the others.
        let output = self.space.outputs().next().cloned();
        let (Some(output), false) = (output, self.locked()) else {
            self.slow_frames.timer.set(false);
            return TimeoutAction::Drop;
        };
        let now: Duration = self.clock.now().into();
        let mut waiting = false;
        for window in self.space.elements() {
            let quiet = self.id_of(window).is_some_and(|id| self.window_quiet(id));
            if !(quiet || self.window_unseen(window)) {
                continue;
            }
            window.send_frame(&output, now, Some(TIMER_THROTTLE), |_, _| None);
            waiting |= waits_for_frame(window);
        }
        if waiting {
            TimeoutAction::ToDuration(SLOW)
        } else {
            self.slow_frames.timer.set(false);
            TimeoutAction::Drop
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
