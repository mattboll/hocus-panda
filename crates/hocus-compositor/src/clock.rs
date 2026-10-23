//! The compositor's time. Everything that moves on screen or expires reads
//! this clock, so that a test harness can stop and step it.

use std::time::{Duration, Instant};

/// Monotonic time since the compositor started.
#[derive(Debug, Clone, Copy)]
pub enum Clock {
    /// Follows the system's monotonic clock.
    Monotonic { start: Instant },
    /// Moves only when the harness advances it.
    Virtual { now: Duration },
}

impl Clock {
    pub fn monotonic() -> Self {
        Self::Monotonic {
            start: Instant::now(),
        }
    }

    pub fn now(&self) -> Duration {
        match self {
            Self::Monotonic { start } => start.elapsed(),
            Self::Virtual { now } => *now,
        }
    }

    /// Whether only a test harness moves it. Timers on real time are then
    /// of no use and do harm: what falls due is seen as the harness
    /// advances, and a timer firing while a test waits for a client would
    /// redraw at a moment the test did not choose (#212).
    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Virtual { .. })
    }

    /// Moves a virtual clock forward to `to`; a monotonic clock cannot be
    /// moved, and time never goes back.
    pub fn advance_to(&mut self, to: Duration) {
        if let Self::Virtual { now } = self {
            *now = (*now).max(to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_clock_moves_only_when_advanced() {
        let mut clock = Clock::Virtual {
            now: Duration::ZERO,
        };
        assert_eq!(clock.now(), Duration::ZERO);
        clock.advance_to(Duration::from_millis(16));
        assert_eq!(clock.now(), Duration::from_millis(16));
        clock.advance_to(Duration::from_millis(5));
        assert_eq!(clock.now(), Duration::from_millis(16));
    }
}
