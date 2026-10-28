//! Whether an output still presents its frames. A frame queued for scan-out
//! comes back with a vblank; when the vblanks stop (a lost page flip, a
//! driver hiccup), nothing more is drawn on that output and its clients wait
//! for frame callbacks that never come. The backend looks at an output with
//! a frame out once per [`CHECK`]; after [`LIMIT`] looks in a row it is taken
//! for stuck and recovered.

use std::time::Duration;

/// How often an output with a frame out is looked at.
pub const CHECK: Duration = Duration::from_secs(1);
/// Looks in a row with the same frame still out before recovering.
pub const LIMIT: u32 = 3;

/// One output's frame in flight.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Flip {
    /// Nothing out: the next frame may be drawn.
    #[default]
    Idle,
    /// A frame was queued at `since` and has not been presented; `missed`
    /// looks found it late.
    Queued { since: Duration, missed: u32 },
}

/// What a look at an output found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// No frame out: nothing to look at until the next one.
    Idle,
    /// The frame out is not late yet, or late `missed` times: look again.
    Waiting { missed: u32 },
    /// Stuck: recover the output; the state is back to idle.
    Recover { missed: u32 },
}

impl Flip {
    pub fn queued(&mut self, now: Duration) {
        if *self == Self::Idle {
            *self = Self::Queued {
                since: now,
                missed: 0,
            };
        }
    }

    pub fn presented(&mut self) {
        *self = Self::Idle;
    }

    pub fn is_queued(&self) -> bool {
        matches!(self, Self::Queued { .. })
    }

    /// Looks at the frame out: one more miss when it has been out longer
    /// than [`CHECK`] since the last miss.
    pub fn check(&mut self, now: Duration) -> Health {
        let Self::Queued { since, missed } = *self else {
            return Health::Idle;
        };
        let late = now.saturating_sub(since) >= CHECK * (missed + 1);
        if !late {
            return Health::Waiting { missed };
        }
        let missed = missed + 1;
        if missed >= LIMIT {
            *self = Self::Idle;
            return Health::Recover { missed };
        }
        *self = Self::Queued { since, missed };
        Health::Waiting { missed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_presented_frame_is_never_late() {
        let mut flip = Flip::default();
        flip.queued(Duration::ZERO);
        flip.presented();
        assert_eq!(flip.check(CHECK * 10), Health::Idle);
    }

    #[test]
    fn a_frame_late_three_looks_in_a_row_recovers() {
        let mut flip = Flip::default();
        flip.queued(Duration::ZERO);
        assert_eq!(flip.check(500 * MS), Health::Waiting { missed: 0 });
        assert_eq!(flip.check(CHECK), Health::Waiting { missed: 1 });
        // Looked at again too soon: not one more miss.
        assert_eq!(flip.check(CHECK + MS), Health::Waiting { missed: 1 });
        assert_eq!(flip.check(CHECK * 2), Health::Waiting { missed: 2 });
        assert_eq!(flip.check(CHECK * 3), Health::Recover { missed: 3 });
        assert_eq!(flip, Flip::Idle);
    }

    #[test]
    fn queuing_again_keeps_the_first_time() {
        let mut flip = Flip::default();
        flip.queued(Duration::ZERO);
        flip.queued(CHECK * 2);
        assert_eq!(flip.check(CHECK * 2), Health::Waiting { missed: 1 });
    }
}
