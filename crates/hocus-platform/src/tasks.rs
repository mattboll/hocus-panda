//! Tasks (#255): iCalendar VTODO, the model every source of tasks shares.
//! The local list is a folder of `.ics` files, one task each, as vdir has
//! it (vdirsyncer's `filesystem` storage): the files at its top are the
//! default list, each folder in it a list of its own, named by its
//! `displayname` file or else by the folder. CalDAV task lists (#256) and
//! Google Tasks (#257) come as other [`Source`]s of the same [`Task`].
//!
//! A task written back keeps what it was read with: only the properties
//! the desktop manages (its summary, status, completion, due date and
//! stamps) are written again, every other line stays as it was.
//!
//! Reading and writing block: worker threads call them, never the loop.

use std::{
    io,
    path::{Path, PathBuf},
};

use jiff::{Timestamp, tz::TimeZone};

use crate::calendar::ics::{self, When};

/// One task, from whichever source.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub uid: String,
    pub summary: String,
    pub done: bool,
    /// When it was done, when its source says.
    pub completed: Option<Timestamp>,
    /// A date, or a date and time.
    pub due: Option<When>,
    /// When it was made: newer ones come first among those without a date.
    pub created: Option<Timestamp>,
    /// The list it is in, by name; `None` for its source's default list.
    pub list: Option<String>,
    pub source: Source,
    /// The text it was read from (empty for a new one): written back with
    /// only what changed.
    pub raw: String,
}

/// Where a task comes from, and where it is written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file of the local list's folder.
    Local { file: PathBuf },
}

impl Task {
    /// A new task of the local list in `dir`, in `list` (a folder of it)
    /// or its default list, made at `now`.
    pub fn new_local(
        dir: &Path,
        summary: &str,
        due: Option<When>,
        list: Option<&str>,
        now: Timestamp,
    ) -> io::Result<Self> {
        let uid = new_uid()?;
        let folder = match list {
            Some(name) => dir.join(list_folder(name)?),
            None => dir.to_path_buf(),
        };
        let file = folder.join(format!("{uid}.ics"));
        Ok(Self {
            uid,
            summary: one_line(summary),
            done: false,
            completed: None,
            due,
            created: Some(now),
            list: list.map(|l| l.trim().to_owned()),
            source: Source::Local { file },
            raw: String::new(),
        })
    }

    /// Says `text` from now on, on one line.
    pub fn set_summary(&mut self, text: &str) {
        self.summary = one_line(text);
    }

    /// Done or not, at `now`.
    pub fn set_done(&mut self, done: bool, now: Timestamp) {
        self.done = done;
        self.completed = done.then_some(now);
    }

    /// Its due moment, dates at the start of their day in `tz`.
    pub fn due_at(&self, tz: &TimeZone) -> Option<Timestamp> {
        self.due.as_ref()?.timestamp(tz)
    }

    /// The iCalendar text to write back at `now`: what it was read from,
    /// with the managed properties written again.
    pub fn to_ics(&self, now: Timestamp) -> String {
        let raw = if self.raw.trim().is_empty() {
            skeleton(&self.uid, self.created.unwrap_or(now))
        } else {
            self.raw.clone()
        };
        let managed = self.managed_lines(now);
        let mut out: Vec<String> = Vec::new();
        let mut in_todo = false;
        let mut written = false;
        let mut depth = 0usize;
        for line in ics::unfold(&raw) {
            if line.is_empty() {
                continue;
            }
            let parsed = ics::property(&line);
            let (name, value) = parsed
                .as_ref()
                .map(|(n, _, v)| (n.as_str(), v.as_str()))
                .unwrap_or_default();
            if !in_todo {
                if !written && name == "BEGIN" && value.eq_ignore_ascii_case("VTODO") {
                    in_todo = true;
                }
                out.push(line);
                continue;
            }
            match (name, value.to_ascii_uppercase().as_str()) {
                ("END", "VTODO") if depth == 0 => {
                    out.extend(managed.iter().cloned());
                    out.push(line);
                    in_todo = false;
                    written = true;
                }
                ("BEGIN", _) => {
                    depth += 1;
                    out.push(line);
                }
                ("END", _) => {
                    depth = depth.saturating_sub(1);
                    out.push(line);
                }
                _ if depth == 0 && MANAGED.contains(&name) => {}
                _ => out.push(line),
            }
        }
        let mut text = String::with_capacity(raw.len() + 128);
        for line in out {
            fold_into(&mut text, &line);
        }
        text
    }

    fn managed_lines(&self, now: Timestamp) -> Vec<String> {
        let mut lines = vec![
            format!("DTSTAMP:{}", utc(now)),
            format!("LAST-MODIFIED:{}", utc(now)),
            format!("SUMMARY:{}", escape(&self.summary)),
        ];
        if self.done {
            lines.push("STATUS:COMPLETED".into());
            lines.push("PERCENT-COMPLETE:100".into());
            if let Some(at) = self.completed {
                lines.push(format!("COMPLETED:{}", utc(at)));
            }
        } else {
            lines.push("STATUS:NEEDS-ACTION".into());
        }
        if let Some(due) = &self.due {
            lines.push(due_line(due));
        }
        lines
    }
}

/// The properties written again each time; the others stay as read.
const MANAGED: [&str; 7] = [
    "DTSTAMP",
    "LAST-MODIFIED",
    "SUMMARY",
    "STATUS",
    "PERCENT-COMPLETE",
    "COMPLETED",
    "DUE",
];

fn skeleton(uid: &str, created: Timestamp) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Hocuspanda//Tasks//EN\r\nBEGIN:VTODO\r\n\
         UID:{uid}\r\nCREATED:{}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        utc(created)
    )
}

fn utc(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

fn due_line(due: &When) -> String {
    match due {
        When::Date(d) => format!("DUE;VALUE=DATE:{}", d.strftime("%Y%m%d")),
        When::Local(dt, None) => format!("DUE:{}", dt.strftime("%Y%m%dT%H%M%S")),
        When::Local(dt, Some(zone)) => {
            format!("DUE;TZID={zone}:{}", dt.strftime("%Y%m%dT%H%M%S"))
        }
        When::Utc(t) => format!("DUE:{}", utc(*t)),
    }
}

/// A TEXT value, escaped (RFC 5545, 3.3.11).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A task's text on one line, its spaces as typed but at its ends.
fn one_line(text: &str) -> String {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `line` folded at 75 octets, on character boundaries, and its CRLF.
fn fold_into(out: &mut String, line: &str) {
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}

/// The first VTODO of `text`; `None` for none, or a cancelled one.
pub fn parse(text: &str, source: Source, list: Option<String>) -> Option<Task> {
    let mut task: Option<Task> = None;
    let mut depth = 0usize;
    let mut cancelled = false;
    let mut percent_done = false;
    for line in ics::unfold(text) {
        let Some((name, params, value)) = ics::property(&line) else {
            continue;
        };
        let upper = value.to_ascii_uppercase();
        match (name.as_str(), upper.as_str()) {
            ("BEGIN", "VTODO") if task.is_none() => {
                task = Some(Task {
                    uid: String::new(),
                    summary: String::new(),
                    done: false,
                    completed: None,
                    due: None,
                    created: None,
                    list: list.clone(),
                    source: source.clone(),
                    raw: text.to_owned(),
                });
                continue;
            }
            ("END", "VTODO") if depth == 0 && task.is_some() => break,
            ("BEGIN", _) if task.is_some() => depth += 1,
            ("END", _) if task.is_some() => depth = depth.saturating_sub(1),
            _ => {}
        }
        let Some(t) = task.as_mut().filter(|_| depth == 0) else {
            continue;
        };
        match name.as_str() {
            "UID" => t.uid = value.trim().to_owned(),
            "SUMMARY" => t.summary = ics::unescape(&value),
            "STATUS" => {
                t.done |= upper.trim() == "COMPLETED";
                cancelled = upper.trim() == "CANCELLED";
            }
            "COMPLETED" => {
                t.done = true;
                t.completed = moment(&value, &params);
            }
            "PERCENT-COMPLETE" => percent_done = value.trim() == "100",
            "DUE" => t.due = ics::parse_when(&value, &params),
            "CREATED" => t.created = moment(&value, &params).or(t.created),
            "DTSTAMP" if t.created.is_none() => t.created = moment(&value, &params),
            _ => {}
        }
    }
    let mut task = task.filter(|t| !t.uid.is_empty() && !cancelled)?;
    task.done |= percent_done;
    Some(task)
}

fn moment(value: &str, params: &[(String, String)]) -> Option<Timestamp> {
    ics::parse_when(value, params)?.timestamp(&TimeZone::UTC)
}

/// A new task's UID: a random UUID, which also names its file.
fn new_uid() -> io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(e.to_string()))?;
    // Version 4, variant 1.
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// The folder of the list named `name`: its name, kept out of other
/// folders.
fn list_folder(name: &str) -> io::Result<String> {
    let name = name.trim();
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} cannot name a list"),
        ));
    }
    Ok(name.to_owned())
}

/// Every task of the local list in `dir`, by file; `None` when the folder
/// is not there: the list was never used.
pub fn load(dir: &Path) -> io::Result<Option<Vec<Task>>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut tasks = Vec::new();
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if hidden(&path) {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => folders.push(path),
            Ok(_) if is_ics(&path) => files.push(path),
            _ => {}
        }
    }
    files.sort();
    tasks.extend(files.into_iter().filter_map(|f| read_one(f, None)));
    folders.sort();
    for folder in folders {
        let name = std::fs::read_to_string(folder.join("displayname"))
            .ok()
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()));
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| !hidden(p) && is_ics(p) && p.is_file())
            .collect();
        files.sort();
        tasks.extend(files.into_iter().filter_map(|f| read_one(f, name.clone())));
    }
    Ok(Some(tasks))
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn is_ics(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ics"))
}

fn read_one(file: PathBuf, list: Option<String>) -> Option<Task> {
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text, Source::Local { file }, list),
        Err(err) => {
            tracing::warn!(%err, file = %file.display(), "a task does not read");
            None
        }
    }
}

/// Writes `text` as the file of a local task, whole or not at all.
pub fn write_local(file: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = file.with_file_name(format!(".{name}.new"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)
}

/// Removes a local task's file; one already gone is gone.
pub fn remove_local(file: &Path) -> io::Result<()> {
    match std::fs::remove_file(file) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::{date, datetime};

    use super::*;

    const THUNDERBIRD: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nCREATED:20261019T080000Z\r\nLAST-MODIFIED:20261019T080000Z\r\nDTSTAMP:20261019T080000Z\r\nUID:7f1c\r\nSUMMARY:Send the quote\\, at last\r\nPRIORITY:1\r\nSTATUS:NEEDS-ACTION\r\nDUE;VALUE=DATE:20261020\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:Reminder\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    fn local() -> Source {
        Source::Local {
            file: PathBuf::from("/tasks/7f1c.ics"),
        }
    }

    #[test]
    fn a_vtodo_reads_with_its_due_date_and_status() {
        let task = parse(THUNDERBIRD, local(), None).expect("a task");
        assert_eq!(task.uid, "7f1c");
        assert_eq!(task.summary, "Send the quote, at last");
        assert!(!task.done);
        assert_eq!(task.due, Some(When::Date(date(2026, 10, 20))));
        assert!(task.created.is_some());
    }

    #[test]
    fn written_back_it_keeps_what_it_does_not_manage() {
        let mut task = parse(THUNDERBIRD, local(), None).expect("a task");
        let now = Timestamp::from_second(1_792_588_620).expect("a time");
        task.set_done(true, now);
        task.set_summary("Send the quote; really");
        let text = task.to_ics(now);
        assert!(text.contains("PRIORITY:1\r\n"), "{text}");
        assert!(
            text.contains("TRIGGER:-PT15M\r\n"),
            "the alarm stays: {text}"
        );
        assert!(
            text.contains("SUMMARY:Reminder\r\n"),
            "the alarm's own summary: {text}"
        );
        assert!(
            text.contains("SUMMARY:Send the quote\\; really\r\n"),
            "{text}"
        );
        assert!(text.contains("STATUS:COMPLETED\r\n"), "{text}");
        assert_eq!(text.matches("STATUS:").count(), 1, "{text}");
        assert!(text.contains("COMPLETED:2026"), "{text}");
        assert!(text.contains("DUE;VALUE=DATE:20261020\r\n"), "{text}");
        let again = parse(&text, local(), None).expect("read again");
        assert!(again.done);
        assert_eq!(again.summary, "Send the quote; really");
        assert_eq!(again.completed, Some(now));
        // Undone, its completion goes.
        task.set_done(false, now);
        let text = task.to_ics(now);
        assert!(!text.contains("COMPLETED:"), "{text}");
        assert!(text.contains("STATUS:NEEDS-ACTION\r\n"), "{text}");
    }

    #[test]
    fn a_new_task_is_a_whole_calendar_with_long_lines_folded() {
        let dir = Path::new("/tasks");
        let now = Timestamp::from_second(1_792_588_620).expect("a time");
        let long = "Buy milk ".repeat(12);
        let due = When::Local(datetime(2026, 10, 21, 15, 0, 0, 0), None);
        let task = Task::new_local(dir, &long, Some(due.clone()), Some("Home"), now).expect("new");
        let Source::Local { file } = &task.source;
        assert_eq!(file.parent(), Some(Path::new("/tasks/Home")));
        assert_eq!(task.uid.len(), 36);
        let text = task.to_ics(now);
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n"), "{text}");
        assert!(text.ends_with("END:VCALENDAR\r\n"), "{text}");
        assert!(text.lines().all(|l| l.len() <= 76), "{text}");
        assert!(text.contains("DUE:20261021T150000\r\n"), "{text}");
        let again = parse(&text, task.source.clone(), None).expect("read again");
        assert_eq!(again.summary, long.trim());
        assert_eq!(again.due, Some(due));
        assert!(Task::new_local(dir, "x", None, Some("../out"), now).is_err());
    }

    #[test]
    fn cancelled_or_broken_ones_are_left_out_and_percent_done_is_done() {
        let cancelled = THUNDERBIRD.replace("STATUS:NEEDS-ACTION", "STATUS:CANCELLED");
        assert!(parse(&cancelled, local(), None).is_none());
        assert!(parse("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n", local(), None).is_none());
        let done = THUNDERBIRD.replace("PRIORITY:1", "PERCENT-COMPLETE:100");
        assert!(parse(&done, local(), None).expect("a task").done);
    }

    #[test]
    fn the_folder_is_the_default_list_and_its_folders_named_lists() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(load(&dir.path().join("none"))?, None);
        std::fs::write(dir.path().join("a.ics"), THUNDERBIRD)?;
        std::fs::write(dir.path().join("notes.txt"), "not a task")?;
        std::fs::create_dir(dir.path().join("work"))?;
        std::fs::write(dir.path().join("work/displayname"), "Work\n")?;
        std::fs::write(
            dir.path().join("work/b.ics"),
            THUNDERBIRD.replace("UID:7f1c", "UID:b"),
        )?;
        let tasks = load(dir.path())?.unwrap_or_default();
        let lists: Vec<_> = tasks
            .iter()
            .map(|t| (t.uid.as_str(), t.list.as_deref()))
            .collect();
        assert_eq!(lists, [("7f1c", None), ("b", Some("Work"))]);
        let file = dir.path().join("c.ics");
        write_local(&file, THUNDERBIRD)?;
        assert_eq!(load(dir.path())?.map(|t| t.len()), Some(3));
        remove_local(&file)?;
        remove_local(&file)?;
        assert_eq!(load(dir.path())?.map(|t| t.len()), Some(2));
        Ok(())
    }
}
