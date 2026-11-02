//! Do not disturb (5.4): while on, bubbles are held (they go straight to
//! the history, the badge counting them), critical ones still showing. It
//! ends by itself at the time chosen: an hour on, the next 8:00, or never.
//!
//! The end is on the monotonic clock, its time read once from the wall
//! clock as it starts (for an hour, an hour on both; for the morning, the
//! wall clock's time until 8:00). It survives a restart of the shell, not a
//! new session: it is written in the session's runtime directory with the
//! compositor's process id, which starts the shell again.

use std::time::{Duration, Instant};

use jiff::{Timestamp, Zoned, civil::Time};

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// For an hour: the default.
    Hour,
    /// Until 8:00 next.
    Morning,
    /// Until it is turned off.
    Always,
}

impl Choice {
    pub const ALL: [Self; 3] = [Self::Hour, Self::Morning, Self::Always];

    fn word(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Morning => "morning",
            Self::Always => "always",
        }
    }

    fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.word() == word)
    }
}

/// When it ends: on the monotonic clock, and the wall clock's time then
/// (milliseconds), which the panel says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Until {
    pub at: Instant,
    pub wall: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dnd {
    #[default]
    Off,
    On {
        choice: Choice,
        /// None: until it is turned off.
        until: Option<Until>,
    },
}

/// The morning it ends at.
const MORNING: Time = Time::constant(8, 0, 0, 0);

impl Dnd {
    pub fn is_on(self) -> bool {
        matches!(self, Self::On { .. })
    }

    pub fn choice(self) -> Option<Choice> {
        match self {
            Self::On { choice, .. } => Some(choice),
            Self::Off => None,
        }
    }

    pub fn until(self) -> Option<Until> {
        match self {
            Self::On { until, .. } => until,
            Self::Off => None,
        }
    }

    /// `choice`, starting at `now`, the wall clock reading `wall`.
    pub fn starting(choice: Choice, now: Instant, wall: &Zoned) -> Self {
        let target = match choice {
            Choice::Hour => Some(wall.timestamp() + jiff::SignedDuration::from_hours(1)),
            Choice::Morning => next_morning(wall).map(|z| z.timestamp()),
            Choice::Always => None,
        };
        let until = target.map(|target| {
            let left = target.duration_since(wall.timestamp());
            let left = Duration::try_from(left).unwrap_or_default();
            Until {
                at: now + left,
                wall: target.as_millisecond(),
            }
        });
        Self::On { choice, until }
    }

    /// When it ends by itself.
    pub fn deadline(self) -> Option<Instant> {
        self.until().map(|u| u.at)
    }

    /// Whether it is over at `now`.
    pub fn over(self, now: Instant) -> bool {
        self.deadline().is_some_and(|at| at <= now)
    }
}

/// The next 8:00 after `wall`, in its time zone.
fn next_morning(wall: &Zoned) -> Option<Zoned> {
    let today = wall
        .date()
        .to_datetime(MORNING)
        .to_zoned(wall.time_zone().clone())
        .ok()?;
    if today > *wall {
        return Some(today);
    }
    let tomorrow = wall.date().tomorrow().ok()?;
    tomorrow
        .to_datetime(MORNING)
        .to_zoned(wall.time_zone().clone())
        .ok()
}

// --- Kept for the session ---------------------------------------------------------

/// `CLOCK_MONOTONIC` now, microseconds: the clock `Instant` reads on Linux.
fn monotonic_us() -> u64 {
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (clock.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(clock.tv_nsec as u64 / 1_000)
}

/// What is written for `dnd` in session `session`: `None` when it is off.
pub fn encode(dnd: Dnd, session: u32) -> Option<String> {
    let Dnd::On { choice, until } = dnd else {
        return None;
    };
    let mut text = format!("session {session}\nchoice {}\n", choice.word());
    if let Some(until) = until {
        let now = Instant::now();
        let left = until.at.saturating_duration_since(now).as_micros();
        let at = monotonic_us().saturating_add(u64::try_from(left).unwrap_or(u64::MAX));
        text.push_str(&format!("until {at}\nwall {}\n", until.wall));
    }
    Some(text)
}

/// What `text` says, in session `session` at `now`: off when it was another
/// session's, or it ended meanwhile.
pub fn decode(text: &str, session: u32, now: Instant) -> Dnd {
    let mut fields = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(' ') {
            fields.insert(name, value.trim());
        }
    }
    if fields.get("session").and_then(|s| s.parse::<u32>().ok()) != Some(session) {
        return Dnd::Off;
    }
    let Some(choice) = fields.get("choice").and_then(|c| Choice::of(c)) else {
        return Dnd::Off;
    };
    let until = match (
        fields.get("until").and_then(|u| u.parse::<u64>().ok()),
        fields.get("wall").and_then(|w| w.parse::<i64>().ok()),
    ) {
        (Some(at), Some(wall)) => {
            let left = Duration::from_micros(at.saturating_sub(monotonic_us()));
            if left.is_zero() {
                return Dnd::Off;
            }
            Some(Until {
                at: now + left,
                wall,
            })
        }
        _ if choice == Choice::Always => None,
        _ => return Dnd::Off,
    };
    Dnd::On { choice, until }
}

/// The session this shell runs in: the compositor that started it.
pub fn session() -> u32 {
    std::os::unix::process::parent_id()
}

/// The wall clock's `ms` in `like`'s time zone.
pub fn zoned(ms: i64, like: &Zoned) -> Option<Zoned> {
    Some(
        Timestamp::from_millisecond(ms)
            .ok()?
            .to_zoned(like.time_zone().clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(at: &str) -> Zoned {
        at.parse().expect("a time")
    }

    #[test]
    fn it_ends_an_hour_on_or_at_the_next_morning_or_never() {
        let now = Instant::now();
        let at = wall("2026-11-02T14:32:00+01:00[Europe/Paris]");
        let hour = Dnd::starting(Choice::Hour, now, &at);
        assert_eq!(hour.deadline(), Some(now + Duration::from_secs(3600)));
        let until = hour.until().expect("an end");
        let said = zoned(until.wall, &at).expect("a time");
        assert_eq!((said.hour(), said.minute()), (15, 32));
        assert!(!hour.over(now + Duration::from_secs(3599)));
        assert!(hour.over(now + Duration::from_secs(3600)));
        let morning = Dnd::starting(Choice::Morning, now, &at);
        // 14:32 to 8:00 the next day.
        let left = Duration::from_secs((24 * 60 - (14 * 60 + 32) + 8 * 60) * 60);
        assert_eq!(morning.deadline(), Some(now + left));
        let early = Dnd::starting(
            Choice::Morning,
            now,
            &wall("2026-11-02T03:00:00+01:00[Europe/Paris]"),
        );
        assert_eq!(early.deadline(), Some(now + Duration::from_secs(5 * 3600)));
        let always = Dnd::starting(Choice::Always, now, &at);
        assert_eq!(always.deadline(), None);
        assert!(always.is_on() && !always.over(now + Duration::from_secs(1 << 30)));
    }

    #[test]
    fn it_lasts_through_a_restart_of_the_shell_not_into_another_session() {
        let now = Instant::now();
        let at = wall("2026-11-02T14:32:00+01:00[Europe/Paris]");
        let hour = Dnd::starting(Choice::Hour, now, &at);
        let text = encode(hour, 4242).expect("on");
        let back = decode(&text, 4242, Instant::now());
        assert_eq!(back.choice(), Some(Choice::Hour));
        let left = back
            .deadline()
            .expect("an end")
            .saturating_duration_since(now);
        assert!(left > Duration::from_secs(3590) && left <= Duration::from_secs(3601));
        assert_eq!(back.until().map(|u| u.wall), hour.until().map(|u| u.wall));
        assert_eq!(decode(&text, 7, now), Dnd::Off);
        let always = encode(Dnd::starting(Choice::Always, now, &at), 4242).expect("on");
        assert_eq!(
            decode(&always, 4242, now),
            Dnd::On {
                choice: Choice::Always,
                until: None
            }
        );
        assert_eq!(encode(Dnd::Off, 4242), None);
    }
}
