//! The busy pointer (docs/design/windows.md §4, #966): the user opened
//! something (a link or a file from a click in an application, an
//! application from the launcher or a shortcut) and nothing has shown
//! yet. The pointer shows the theme's `progress` shape until a window
//! appears or comes forward, 5 s at most; nothing else shows.

use std::time::Duration;

use smithay::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use crate::State;

/// The longest the pointer stays busy: an application that never shows a
/// window (a link opened in a browser that ignores its token, a program
/// that failed) leaves it as it was.
pub const AT_MOST: Duration = Duration::from_secs(5);

/// What the pointer waits for.
#[derive(Debug, Default)]
pub enum Busy {
    #[default]
    Idle,
    /// Something opened, its window not shown yet; the timer that ends
    /// the wait.
    Opening(RegistrationToken),
}

impl State {
    /// The user opened something: the pointer is busy until a window shows
    /// or comes forward, [`AT_MOST`] at most. Opened again, the wait starts
    /// over.
    pub fn opening(&mut self, what: &'static str) {
        if let Busy::Opening(timer) = std::mem::take(&mut self.pointer.busy) {
            self.loop_handle.remove(timer);
        }
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(AT_MOST), |_, _, app| {
                    // The timer is spent: it goes with this call.
                    app.state.pointer.busy = Busy::Idle;
                    tracing::debug!("the pointer is no longer busy: nothing came in 5 s");
                    app.state.cursor_changed();
                    TimeoutAction::Drop
                });
        match inserted {
            Ok(timer) => {
                tracing::debug!(what, "the pointer is busy");
                self.pointer.busy = Busy::Opening(timer);
                self.cursor_changed();
            }
            Err(err) => tracing::warn!(%err.error, "no timer for the busy pointer"),
        }
    }

    /// A window appeared or came forward (or pulsed, asked for without
    /// the user's token): the pointer is no longer busy.
    pub fn opened(&mut self) {
        if let Busy::Opening(timer) = std::mem::take(&mut self.pointer.busy) {
            self.loop_handle.remove(timer);
            tracing::debug!("the pointer is no longer busy");
            self.cursor_changed();
        }
    }

    /// Whether the pointer shows busy.
    pub fn pointer_busy(&self) -> bool {
        matches!(self.pointer.busy, Busy::Opening(_))
    }

    fn cursor_changed(&mut self) {
        if !self.nested {
            self.redraw = true;
        }
    }
}
