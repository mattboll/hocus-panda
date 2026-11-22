//! The notifications' journal (#923, docs/design/notifications.md "The
//! journal"): every notification served, written down as it comes, so
//! that nothing is lost; 30 days of them in
//! `$XDG_STATE_HOME/hocuspanda/notifications.log`, one JSON object a line,
//! readable by the user alone.
//!
//! Written and read by the history's thread only (`super::keep`), never
//! the window's: each line appended in one write, the file opened for
//! appending; written again without what is older than 30 days as the
//! shell starts and at the first line of each new day, and without its
//! oldest lines past 64 MB. A line cut by a crash is skipped when read.
//! An agent's search reads it there (`search_notifications`).

use std::{
    io::{BufRead, Write as _},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use hocus_link::{NoteState, NotifyLevel};
use serde_json::{Value, json};

use super::note::Urgency;

/// The journal's file's name, beside the history's.
pub const FILE: &str = "notifications.log";
/// How long a line is kept.
pub const KEEP_DAYS: i64 = 30;
/// Past this size it is written again at once, without its oldest lines
/// down to [`DOWN_TO`].
pub const MOST_BYTES: u64 = 64 * 1024 * 1024;
pub const DOWN_TO: u64 = 48 * 1024 * 1024;

/// Where a notification went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    Bubble,
    /// To the history only (its level, a low urgency).
    History,
    /// Held: do not disturb, the sharing, the lock.
    Held,
    /// An agents' tool saying its agent waits or finished (5.3).
    Agent,
    /// Transient without a bubble.
    Journal,
    /// Its application's level is "none".
    Dropped,
}

impl To {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Bubble => "bubble",
            Self::History => "history",
            Self::Held => "held",
            Self::Agent => "agent",
            Self::Journal => "journal",
            Self::Dropped => "dropped",
        }
    }
}

/// A line of the journal.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The wall clock as it came (milliseconds), written RFC 3339 with its
    /// offset in `zone`.
    pub at: i64,
    pub id: u64,
    pub app: String,
    pub app_id: String,
    pub title: String,
    /// The markup taken out, as shown.
    pub body: String,
    pub urgency: Urgency,
    /// Their labels.
    pub actions: Vec<String>,
    pub transient: bool,
    /// The number of the one it replaces.
    pub replaces: Option<u64>,
    pub level: NotifyLevel,
    pub to: To,
}

/// `at` (milliseconds) on the wall clock of `zone`: RFC 3339, its offset
/// and its milliseconds.
pub fn rfc3339(at: i64, zone: &jiff::tz::TimeZone) -> String {
    let Ok(stamp) = jiff::Timestamp::from_millisecond(at) else {
        return String::new();
    };
    let zoned = stamp.to_zoned(zone.clone());
    format!(
        "{}.{:03}{}",
        zoned.strftime("%Y-%m-%dT%H:%M:%S"),
        at.rem_euclid(1000),
        zoned.strftime("%:z")
    )
}

/// A line's `at` read back, milliseconds.
fn read_at(at: &str) -> Option<i64> {
    at.parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_millisecond)
}

impl Line {
    /// As written, without its line break.
    pub fn text(&self, zone: &jiff::tz::TimeZone) -> String {
        let mut line = json!({
            "at": rfc3339(self.at, zone),
            "id": self.id,
            "app": self.app,
            "app_id": self.app_id,
            "title": self.title,
            "body": self.body,
            "urgency": match self.urgency {
                Urgency::Low => "low",
                Urgency::Normal => "normal",
                Urgency::Critical => "critical",
            },
            "actions": self.actions,
        });
        if self.transient {
            line["transient"] = json!(true);
        }
        if let Some(replaces) = self.replaces {
            line["replaces"] = json!(replaces);
        }
        line["level"] = json!(self.level.word());
        line["to"] = json!(self.to.word());
        line.to_string()
    }
}

/// The journal's file, beside the history's `history`.
pub fn file_beside(history: &Path) -> PathBuf {
    history.with_file_name(FILE)
}

/// Opened for appending, created readable by the user alone, made so
/// again when found wider.
fn open(file: &Path) -> std::io::Result<std::fs::File> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(file)?;
    let mode = out.metadata()?.permissions().mode();
    if mode & 0o077 != 0 {
        out.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(out)
}

/// `line` appended, in one write; on a line of its own after one a crash
/// cut.
pub fn append(file: &Path, line: &str) -> std::io::Result<()> {
    let mut out = open(file)?;
    let mut bytes = Vec::with_capacity(line.len() + 2);
    if !ends_a_line(file) {
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    out.write_all(&bytes)
}

/// Whether `file` is empty or ends with a line break.
fn ends_a_line(file: &Path) -> bool {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let Ok(mut read) = std::fs::File::open(file) else {
        return true;
    };
    if read.seek(SeekFrom::End(-1)).is_err() {
        return true;
    }
    let mut last = [0u8; 1];
    read.read_exact(&mut last)
        .map_or(true, |()| last[0] == b'\n')
}

/// Each line of `file` that reads, with its `at` (milliseconds); a line cut
/// by a crash, or that does not read, is skipped.
fn lines(file: &Path) -> std::io::Result<Vec<(i64, Value, String)>> {
    let read = match std::fs::File::open(file) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut out = Vec::new();
    for line in std::io::BufReader::new(read).lines() {
        let Ok(line) = line else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(at) = value.get("at").and_then(Value::as_str).and_then(read_at) else {
            continue;
        };
        out.push((at, value, line));
    }
    Ok(out)
}

/// `file` written again without the lines older than 30 days at `now`
/// (milliseconds), and without its oldest past [`MOST_BYTES`] down to
/// [`DOWN_TO`]: a new file, readable by the user alone, renamed over the
/// old. How many lines it keeps and how many went.
pub fn prune(file: &Path, now: i64) -> std::io::Result<(usize, usize)> {
    prune_within(file, now, MOST_BYTES, DOWN_TO)
}

/// [`prune`], past `most` bytes down to `down_to`.
fn prune_within(file: &Path, now: i64, most: u64, down_to: u64) -> std::io::Result<(usize, usize)> {
    let all = lines(file)?;
    let total = std::fs::metadata(file).map_or(0, |m| m.len());
    let since = now - KEEP_DAYS * 24 * 3600 * 1000;
    let mut kept: Vec<&str> = all
        .iter()
        .filter(|(at, _, _)| *at >= since)
        .map(|(_, _, line)| line.as_str())
        .collect();
    let mut size: u64 = kept.iter().map(|l| l.len() as u64 + 1).sum();
    if total > most || size > most {
        let mut cut = 0;
        while size > down_to && cut < kept.len() {
            size -= kept.get(cut).map_or(0, |l| l.len() as u64 + 1);
            cut += 1;
        }
        kept.drain(..cut);
    }
    let gone = all.len() - kept.len();
    // As it is: nothing to write again (a cut line read past the end
    // stays until the next).
    if gone == 0 && total <= most {
        if file.exists() {
            drop(open(file)?);
        }
        return Ok((kept.len(), 0));
    }
    let new = file.with_extension("log.new");
    {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&new)?;
        let mut text = String::with_capacity(size as usize);
        for line in &kept {
            text.push_str(line);
            text.push('\n');
        }
        out.write_all(text.as_bytes())?;
        out.sync_all()?;
    }
    std::fs::rename(&new, file)?;
    Ok((kept.len(), gone))
}

/// The largest number its last lines hold (0: none), for the numbers to
/// go on after it.
pub fn largest(file: &Path) -> u64 {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let Ok(mut read) = std::fs::File::open(file) else {
        return 0;
    };
    // The end of the file: its last lines, whole or cut.
    let size = read.metadata().map_or(0, |m| m.len());
    let from = size.saturating_sub(64 * 1024);
    if read.seek(SeekFrom::Start(from)).is_err() {
        return 0;
    }
    let mut tail = Vec::new();
    if read.read_to_end(&mut tail).is_err() {
        return 0;
    }
    String::from_utf8_lossy(&tail)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
}

/// What a search asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// Its words, every one of them, in the title, the body or the
    /// application's name, ignoring case and accents.
    pub text: String,
    /// An application's id or name, ignoring case.
    pub app: Option<String>,
    /// Milliseconds.
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: usize,
}

/// `file`'s lines that `search` finds, newest first, as written, each with
/// its `state` while the bell holds it (`state_of`, by number).
pub fn search(
    file: &Path,
    search: &Search,
    state_of: impl Fn(u64) -> Option<NoteState>,
) -> std::io::Result<Vec<Value>> {
    let fold = crate::launcher::search::fold;
    let words: Vec<String> = fold(&search.text)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let app = search.app.as_deref().map(|a| a.trim().to_lowercase());
    let mut found = Vec::new();
    for (at, mut value, _) in lines(file)?.into_iter().rev() {
        if found.len() >= search.limit {
            break;
        }
        if search.since.is_some_and(|since| at < since)
            || search.until.is_some_and(|until| at > until)
        {
            continue;
        }
        let text = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if let Some(app) = &app
            && text("app").to_lowercase() != *app
            && text("app_id").to_lowercase() != *app
        {
            continue;
        }
        let haystack = fold(&format!(
            "{} {} {}",
            text("title"),
            text("body"),
            text("app")
        ));
        if !words.iter().all(|w| haystack.contains(w.as_str())) {
            continue;
        }
        if let Some(state) = value.get("id").and_then(Value::as_u64).and_then(&state_of) {
            value["state"] = json!(state.word());
        }
        found.push(value);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 3600 * 1000;
    /// 22 November 2026, 14:32:07.250 UTC.
    const NOW: i64 = 1_795_357_927_250;

    fn line(id: u64, at: i64, app: &str, title: &str) -> Line {
        Line {
            at,
            id,
            app: app.to_owned(),
            app_id: format!("org.example.{app}"),
            title: title.to_owned(),
            body: "Tu as vu la maquette ?".to_owned(),
            urgency: Urgency::Normal,
            actions: vec!["Répondre".to_owned()],
            transient: false,
            replaces: None,
            level: NotifyLevel::All,
            to: To::Bubble,
        }
    }

    fn paris() -> jiff::tz::TimeZone {
        jiff::tz::TimeZone::get("Europe/Paris").unwrap_or(jiff::tz::TimeZone::UTC)
    }

    #[test]
    fn a_line_says_its_time_with_its_offset_and_milliseconds() {
        let mut transient = line(1201, NOW, "Musique", "Morceau suivant");
        transient.transient = true;
        transient.to = To::Journal;
        transient.replaces = Some(1200);
        let text = transient.text(&jiff::tz::TimeZone::UTC);
        let value: Value = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(value["at"], "2026-11-22T14:32:07.250+00:00");
        assert_eq!(value["id"], 1201);
        assert_eq!(value["app_id"], "org.example.Musique");
        assert_eq!(value["transient"], true);
        assert_eq!(value["replaces"], 1200);
        assert_eq!(value["to"], "journal");
        assert_eq!(value["level"], "all");
        assert_eq!(value["actions"], json!(["Répondre"]));
        let plain = line(1, NOW, "Chat", "Camille");
        let value: Value = serde_json::from_str(&plain.text(&paris())).unwrap_or_default();
        if paris() != jiff::tz::TimeZone::UTC {
            assert_eq!(value["at"], "2026-11-22T15:32:07.250+01:00");
        }
        assert!(value.get("transient").is_none() && value.get("replaces").is_none());
        assert_eq!(read_at("2026-11-22T15:32:07.250+01:00"), Some(NOW));
    }

    #[test]
    fn it_is_the_users_alone_keeps_30_days_and_skips_a_cut_line() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("hocuspanda").join(FILE);
        let zone = jiff::tz::TimeZone::UTC;
        append(&file, &line(1, NOW - 31 * DAY, "Chat", "Vieux").text(&zone))?;
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        // Found wider: made the user's alone again.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
        append(
            &file,
            &line(2, NOW - 29 * DAY, "Chat", "Camille").text(&zone),
        )?;
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        // A crash cut the last line.
        let cut = line(3, NOW, "Courriel", "Facture").text(&zone);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)?
            .write_all(&cut.as_bytes()[..cut.len() / 2])?;
        assert_eq!(largest(&file), 2);
        // A line after the cut one is whole.
        append(&file, &line(4, NOW, "Chat", "Après").text(&zone))?;
        assert_eq!(largest(&file), 4);
        assert_eq!(prune(&file, NOW)?, (2, 1));
        let text = std::fs::read_to_string(&file)?;
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("Camille") && text.contains("Après"));
        assert!(!text.contains("Facture"), "the cut line skipped");
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn past_its_most_its_oldest_lines_go_down_to_less() -> std::io::Result<()> {
        // 64 MB and 48 MB as 64 kB and 48 kB.
        let (most, down_to) = (MOST_BYTES / 1024, DOWN_TO / 1024);
        let dir = tempfile::tempdir()?;
        let file = dir.path().join(FILE);
        let zone = jiff::tz::TimeZone::UTC;
        let mut big = line(0, NOW, "Chat", "x");
        big.body = "a".repeat(1024);
        let mut text = String::new();
        for id in 1..=70 {
            big.id = id;
            text.push_str(&big.text(&zone));
            text.push('\n');
        }
        std::fs::write(&file, text)?;
        let (kept, gone) = prune_within(&file, NOW, most, down_to)?;
        assert!(std::fs::metadata(&file)?.len() <= down_to);
        assert!(gone > 0);
        assert_eq!(kept + gone, 70);
        assert_eq!(largest(&file), 70, "the newest kept");
        Ok(())
    }

    #[test]
    fn a_search_finds_every_word_ignoring_case_and_accents_newest_first() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join(FILE);
        let zone = jiff::tz::TimeZone::UTC;
        append(
            &file,
            &line(1, NOW - 3 * DAY, "Chat", "Réunion d’équipe").text(&zone),
        )?;
        append(
            &file,
            &line(2, NOW - DAY, "Courriel", "Facture").text(&zone),
        )?;
        append(
            &file,
            &line(3, NOW, "Chat", "La reunion est déplacée").text(&zone),
        )?;
        let found = |s: Search| -> Vec<u64> {
            search(&file, &s, |id| (id == 3).then_some(NoteState::New))
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v["id"].as_u64())
                .collect()
        };
        let all = |text: &str| Search {
            text: text.to_owned(),
            limit: 50,
            ..Search::default()
        };
        assert_eq!(found(all("REUNION")), [3, 1]);
        assert_eq!(found(all("réunion déplacée")), [3]);
        assert_eq!(found(all("maquette")), [3, 2, 1], "in the body too");
        assert_eq!(
            found(Search {
                app: Some("org.example.courriel".to_owned()),
                ..all("")
            }),
            [2]
        );
        assert_eq!(
            found(Search {
                since: Some(NOW - 2 * DAY),
                ..all("")
            }),
            [3, 2]
        );
        assert_eq!(
            found(Search {
                limit: 1,
                ..all("")
            }),
            [3]
        );
        let first = search(&file, &all("reunion"), |id| {
            (id == 3).then_some(NoteState::New)
        })?;
        assert_eq!(first[0]["state"], "new");
        assert!(first[1].get("state").is_none(), "no longer in the bell");
        Ok(())
    }
}
