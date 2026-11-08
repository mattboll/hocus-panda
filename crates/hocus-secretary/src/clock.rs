//! Her clock: the wall clock, or a clock of her own that moves only when
//! told (tests, `--virtual-clock`), so that ten minutes, a model's thirty
//! seconds or a night's sleep pass at once and the same way every time.
//!
//! Her times are what the user reads (an episode's time, a fact's valid
//! dates): wall time. How long she waits between two syncs is counted on
//! it too, the wait itself on the monotonic clock; a suspend is told her
//! (or measured on `CLOCK_BOOTTIME`), never guessed from a jump.

use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

use jiff::{SignedDuration, Timestamp};

#[derive(Debug, Clone)]
pub enum Clock {
    Wall,
    Virtual(Arc<Virtual>),
}

#[derive(Debug)]
pub struct Virtual {
    now: Mutex<Timestamp>,
    moved: Condvar,
}

impl Clock {
    /// A clock of her own, at `start`.
    pub fn virtual_at(start: Timestamp) -> Self {
        Self::Virtual(Arc::new(Virtual {
            now: Mutex::new(start),
            moved: Condvar::new(),
        }))
    }

    pub fn now(&self) -> Timestamp {
        match self {
            Self::Wall => Timestamp::now(),
            Self::Virtual(v) => *v.now.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }

    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Virtual(_))
    }

    /// Moves a clock of her own on; whether it is one.
    pub fn advance(&self, by: Duration) -> bool {
        match self {
            Self::Wall => false,
            Self::Virtual(v) => {
                let mut now = v.now.lock().unwrap_or_else(PoisonError::into_inner);
                let by = SignedDuration::try_from(by).unwrap_or(SignedDuration::MAX);
                *now = now.checked_add(by).unwrap_or(Timestamp::MAX);
                v.moved.notify_all();
                true
            }
        }
    }

    /// Blocks this thread (a worker's, never her loop's) until `at`.
    pub fn sleep_until(&self, at: Timestamp) {
        match self {
            Self::Wall => {
                let left = at.duration_since(Timestamp::now());
                if let Ok(left) = Duration::try_from(left) {
                    std::thread::sleep(left);
                }
            }
            Self::Virtual(v) => {
                let now = v.now.lock().unwrap_or_else(PoisonError::into_inner);
                let _held = v
                    .moved
                    .wait_while(now, |now| *now < at)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
    }

    /// How long from now until `at`, on the monotonic clock her loop waits
    /// on: `None` on a clock of her own (only a message moves it).
    pub fn wait_until(&self, at: Timestamp) -> Option<Duration> {
        match self {
            Self::Wall => Some(
                Duration::try_from(at.duration_since(Timestamp::now())).unwrap_or(Duration::ZERO),
            ),
            Self::Virtual(_) => None,
        }
    }
}

/// How long the machine slept: `CLOCK_BOOTTIME` counts a suspend,
/// `CLOCK_MONOTONIC` does not.
#[derive(Debug, Clone, Copy)]
pub struct Sleeps {
    boot: Duration,
    mono: Duration,
}

impl Sleeps {
    pub fn now() -> Self {
        let read = |id| {
            let t = rustix::time::clock_gettime(id);
            Duration::new(
                u64::try_from(t.tv_sec).unwrap_or(0),
                u32::try_from(t.tv_nsec).unwrap_or(0),
            )
        };
        Self {
            boot: read(rustix::time::ClockId::Boottime),
            mono: read(rustix::time::ClockId::Monotonic),
        }
    }

    /// The time slept since `self`.
    pub fn slept_since(self, later: Self) -> Duration {
        let boot = later.boot.saturating_sub(self.boot);
        let mono = later.mono.saturating_sub(self.mono);
        boot.saturating_sub(mono)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_of_her_own_moves_only_when_told() {
        let start: Timestamp = "2026-11-09T08:00:00Z".parse().expect("a time");
        let clock = Clock::virtual_at(start);
        assert_eq!(clock.now(), start);
        assert_eq!(clock.wait_until(start), None);
        let waiter = {
            let clock = clock.clone();
            std::thread::spawn(move || {
                clock.sleep_until(start + SignedDuration::from_secs(30));
                clock.now()
            })
        };
        assert!(clock.advance(Duration::from_secs(10)));
        assert!(!waiter.is_finished());
        clock.advance(Duration::from_secs(20));
        let woke = waiter.join().expect("the waiter");
        assert_eq!(woke, start + SignedDuration::from_secs(30));
        assert!(!Clock::Wall.advance(Duration::from_secs(1)));
    }
}
