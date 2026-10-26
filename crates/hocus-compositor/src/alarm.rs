//! A moment on the compositor's clock that wakes the loop, once (#88):
//! what falls due is handled after the turn of the loop it wakes, as after
//! any other. A periodic timer would wake a resting desktop for nothing.

use std::time::Duration;

use smithay::reexports::calloop::{
    LoopHandle, RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use crate::App;

/// One wakeup set at a moment, or none.
#[derive(Debug, Default)]
pub struct Alarm {
    set: Option<RegistrationToken>,
    /// The moment asked for, even on a virtual clock, which needs no
    /// timer: the harness sees what falls due as it advances.
    at: Option<Duration>,
}

impl Alarm {
    /// The moment it is set for.
    pub fn at(&self) -> Option<Duration> {
        self.at
    }

    /// Whether its moment has come.
    pub fn due(&self, now: Duration) -> bool {
        self.at.is_some_and(|at| now >= at)
    }

    /// Wakes the loop at `at` (`None`: never) when `handle` is given (a
    /// real clock: see [`crate::State::waker`]); `name` counts the wakeup
    /// in the stats. Setting the same moment again does nothing.
    pub fn set(
        &mut self,
        handle: Option<&LoopHandle<'static, App>>,
        name: &'static str,
        at: Option<Duration>,
        now: Duration,
    ) {
        if self.at == at {
            return;
        }
        self.at = at;
        if let Some(token) = self.set.take()
            && let Some(handle) = handle
        {
            // Gone already when it fired: removing it again is harmless.
            handle.remove(token);
        }
        let (Some(at), Some(handle)) = (at, handle) else {
            return;
        };
        let timer = Timer::from_duration(at.saturating_sub(now));
        let inserted = handle.insert_source(timer, move |_, _, app| {
            // Not a handler of its own: what is due runs after the turn.
            app.state.measures.ran(name);
            TimeoutAction::Drop
        });
        match inserted {
            Ok(token) => self.set = Some(token),
            Err(err) => tracing::warn!(%err.error, alarm = name, "could not set an alarm"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_a_virtual_clock_it_keeps_its_moment_without_a_timer() {
        let mut alarm = Alarm::default();
        let s = Duration::from_secs;
        alarm.set(None, "test", Some(s(30)), s(0));
        assert_eq!(alarm.at(), Some(s(30)));
        assert!(alarm.set.is_none());
        assert!(!alarm.due(s(29)));
        assert!(alarm.due(s(30)));
        alarm.set(None, "test", None, s(30));
        assert!(!alarm.due(s(60)));
    }
}
