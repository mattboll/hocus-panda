//! Where the days are kept: one file per local date,
//! `$XDG_STATE_HOME/hocuspanda/perf/YYYY-MM-DD`, 14 of them.

use std::path::{Path, PathBuf};

use jiff::{Timestamp, civil::Date, tz::TimeZone};

use crate::{
    format::{Line, Sample},
    signals::SignalKind,
};

/// How many days are kept, today's included.
pub const KEEP_DAYS: i32 = 14;

/// The most a day's file holds: a day busy enough to reach it is cut
/// there, the rest of it not written.
pub const DAY_CAP: u64 = 1_000_000;

/// The days' folder under the state folder (`$XDG_STATE_HOME/hocuspanda`).
pub fn dir_in(state: &Path) -> PathBuf {
    state.join("perf")
}

/// The days' folder of this user: `$XDG_STATE_HOME/hocuspanda/perf`, else
/// `~/.local/state/hocuspanda/perf`.
pub fn dir() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|h| h.join(".local/state"))
        })?;
    Some(dir_in(&state.join("hocuspanda")))
}

/// The file of `date`.
pub fn path(dir: &Path, date: Date) -> PathBuf {
    dir.join(date.to_string())
}

/// The local date at Unix second `t`.
pub fn date_at(t: i64, tz: &TimeZone) -> Option<Date> {
    Some(Timestamp::from_second(t).ok()?.to_zoned(tz.clone()).date())
}

/// The days kept in `dir`, oldest first.
pub fn days(dir: &Path) -> Vec<Date> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut days: Vec<Date> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<Date>().ok())
        .collect();
    days.sort();
    days
}

/// Removes the days older than [`KEEP_DAYS`] before `today`; what is not a
/// day's file stays. The files removed.
pub fn prune(dir: &Path, today: Date) -> Vec<PathBuf> {
    let Ok(oldest) = today.checked_sub(jiff::Span::new().days(KEEP_DAYS - 1)) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for day in days(dir) {
        if day >= oldest {
            continue;
        }
        let file = path(dir, day);
        match std::fs::remove_file(&file) {
            Ok(()) => removed.push(file),
            Err(err) => tracing::warn!(file = %file.display(), %err, "an old day is not removed"),
        }
    }
    removed
}

/// A day as its file says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Day {
    pub samples: Vec<Sample>,
    /// When the recorder started, Unix seconds.
    pub starts: Vec<i64>,
    /// The signals whose notification was sent.
    pub notified: Vec<SignalKind>,
}

impl Day {
    /// The file's text read; lines that do not read (one cut short by a
    /// crash) are passed over.
    pub fn parse(text: &str) -> Self {
        let mut day = Self::default();
        for line in text.lines() {
            match Line::decode(line) {
                Some(Line::Sample(s)) => day.samples.push(s),
                Some(Line::Start { start, .. }) => day.starts.push(start),
                Some(Line::Notified { notified, .. }) => day.notified.push(notified),
                None => {}
            }
        }
        day
    }

    /// `date`'s file in `dir`; an empty day when there is none.
    pub fn read(dir: &Path, date: Date) -> std::io::Result<Self> {
        match std::fs::read_to_string(path(dir, date)) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_older_than_fourteen_go_and_nothing_else() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let today: Date = "2026-11-07".parse().map_err(std::io::Error::other)?;
        for name in [
            "2026-10-20",
            "2026-10-24",
            "2026-10-25",
            "2026-11-06",
            "2026-11-07",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), "")?;
        }
        let removed = prune(dir.path(), today);
        assert_eq!(
            removed,
            vec![dir.path().join("2026-10-20"), dir.path().join("2026-10-24")]
        );
        let left: Vec<String> = days(dir.path()).iter().map(Date::to_string).collect();
        assert_eq!(left, ["2026-10-25", "2026-11-06", "2026-11-07"]);
        assert!(dir.path().join("notes.txt").exists());
        Ok(())
    }

    #[test]
    fn a_day_cut_by_a_crash_reads_up_to_its_last_whole_line() {
        let day = Day::parse(concat!(
            "{\"start\":100,\"interval\":10000}\n",
            "{\"t\":110,\"c\":[3,1],\"m\":[100,50]}\n",
            "{\"notified\":\"slow-moment\",\"t\":110}\n",
            "{\"t\":120,\"c\":[2,1],\"m\":[100,50]}\n",
            "{\"t\":130,\"c\":[2,"
        ));
        assert_eq!(day.starts, [100]);
        assert_eq!(day.samples.len(), 2);
        assert_eq!(day.notified, [SignalKind::SlowMoment]);
    }

    #[test]
    fn a_day_not_recorded_is_empty() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let date: Date = "2026-11-07".parse().map_err(std::io::Error::other)?;
        assert_eq!(Day::read(dir.path(), date)?, Day::default());
        Ok(())
    }
}
