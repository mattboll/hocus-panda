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
