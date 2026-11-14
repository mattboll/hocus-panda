//! The notifications' history (5.4, variant B: by time): every
//! notification, newest first, whether it was seen, and whether its sender
//! still holds it; kept 7 days in `$XDG_STATE_HOME/hocuspanda/notifications`.
//!
//! Pure: the stack says what came and went, the history panel reads it.
//! What is written and read back ([`encode`], [`decode`]) is one JSON object
//! a line, the newest first; a notification's actions are not kept, its
//! sender's hold ending with the shell.

use jiff::{Timestamp, Zoned, civil::Date};

use super::note::{IconFrom, Note, Timeout, Urgency};

/// How long the history keeps a notification.
pub const KEEP_MS: i64 = 7 * 24 * 3600 * 1000;
/// The most it keeps, the oldest going first.
pub const MOST: usize = 200;

/// Whether a notification was seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    /// Its bubble shows, or waits its turn: not counted yet.
    Showing,
    /// The badge counts it: its bubble went without the pointer resting on
    /// it or a click, it was of low urgency, or held while the user was not
    /// to be disturbed.
    Unseen,
    Seen,
}

/// A notification in the history.
#[derive(Debug, Clone)]
pub struct Kept {
    /// The history's own, for this run of the shell.
    pub key: u64,
    /// The server's id while its sender holds it: its actions show, and its
    /// default is invoked by a click.
    pub live: Option<u32>,
    pub note: Note,
    pub seen: Sight,
}

/// The history.
#[derive(Debug, Default)]
pub struct History {
    /// Newest first.
    items: Vec<Kept>,
    next: u64,
    /// The panel shows it: the badge says 0, and what it lists is seen as
    /// it closes.
    open: bool,
    /// Changes with what it lists, for the panel to lay it out again.
    version: u64,
    /// The notifications still held by their senders that went for their
    /// age or past the most kept: to close (1).
    dropped: Vec<u32>,
}

impl History {
    pub fn items(&self) -> &[Kept] {
        &self.items
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// What the bell's badge counts: none while the history shows.
    pub fn unseen(&self) -> usize {
        if self.open {
            return 0;
        }
        self.items
            .iter()
            .filter(|k| k.seen == Sight::Unseen)
            .count()
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    /// What is past the most kept goes.
    fn trim(&mut self) {
        if self.items.len() > MOST {
            let gone = self.items.split_off(MOST);
            self.dropped.extend(gone.iter().filter_map(|k| k.live));
        }
    }

    /// The notifications still held by their senders that went since last
    /// asked (for their age, or past the most kept).
    pub fn take_dropped(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.dropped)
    }

    /// `note` comes, live, `seen` so: the newest. A notification of the
    /// same id held by its sender is taken out first (replaced).
    pub fn add(&mut self, note: Note, seen: Sight) -> u64 {
        self.items.retain(|k| k.live != Some(note.id));
        self.next += 1;
        let key = self.next;
        self.items.insert(
            0,
            Kept {
                key,
                live: Some(note.id),
                note,
                seen,
            },
        );
        self.trim();
        self.changed();
        key
    }

    /// The live notification `id` now says what `note` does, in its place.
    pub fn update(&mut self, note: Note) {
        if let Some(kept) = self.items.iter_mut().find(|k| k.live == Some(note.id)) {
            kept.note = note;
            self.version += 1;
        }
    }

    /// The live notification `id` is now `seen` so.
    pub fn set_seen(&mut self, id: u32, seen: Sight) {
        if let Some(kept) = self
            .items
            .iter_mut()
            .find(|k| k.live == Some(id) && k.seen != seen)
        {
            kept.seen = seen;
            self.version += 1;
        }
    }

    /// Notification `id`'s sender holds it no more (it was closed): its
    /// actions go.
    pub fn closed(&mut self, id: u32) {
        if let Some(kept) = self.items.iter_mut().find(|k| k.live == Some(id)) {
            kept.live = None;
            self.version += 1;
        }
    }

    /// The live notification `id` taken out (its application took it
    /// back): whether it was there.
    pub fn remove_live(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|k| k.live != Some(id));
        let gone = self.items.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    /// The one of `key` taken out.
    pub fn remove(&mut self, key: u64) -> Option<Kept> {
        let at = self.items.iter().position(|k| k.key == key)?;
        self.changed();
        Some(self.items.remove(at))
    }

    pub fn get(&self, key: u64) -> Option<&Kept> {
        self.items.iter().find(|k| k.key == key)
    }

    /// The panel opens, or closes: as it closes, what it listed is seen.
    /// Whether the badge or the dots change.
    pub fn set_open(&mut self, open: bool) -> bool {
        if self.open == open {
            return false;
        }
        self.open = open;
        if !open {
            for kept in &mut self.items {
                if kept.seen == Sight::Unseen {
                    kept.seen = Sight::Seen;
                }
            }
        }
        self.changed();
        true
    }

    /// What is older than 7 days at `now` (wall clock, milliseconds) goes:
    /// whether any did.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.items.len();
        let (kept, old): (Vec<Kept>, Vec<Kept>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|k| now - k.note.at < KEEP_MS);
        self.items = kept;
        self.dropped.extend(old.iter().filter_map(|k| k.live));
        let gone = self.items.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    /// What was kept before the shell started again, read back: after what
    /// came since, by time, newest first.
    pub fn restore(&mut self, notes: Vec<(Note, Sight)>) {
        for (note, seen) in notes {
            self.next += 1;
            self.items.push(Kept {
                key: self.next,
                live: None,
                note,
                seen,
            });
        }
        self.items.sort_by_key(|k| std::cmp::Reverse(k.note.at));
        self.trim();
        self.changed();
    }
}

// --- On disk ----------------------------------------------------------------------

/// What is written of a notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub app: String,
    pub entry: Option<String>,
    pub icon_from: IconFrom,
    pub summary: String,
    pub body: String,
    pub pid: u32,
    pub agent: bool,
    pub conversation: Option<hocus_link::AgentConversation>,
    pub at: i64,
    pub unseen: bool,
}

impl Record {
    pub fn of(kept: &Kept) -> Self {
        let note = &kept.note;
        Self {
            app: note.app.clone(),
            entry: note.entry.clone(),
            icon_from: note.icon_from.clone(),
            summary: note.summary.clone(),
            body: note.body.clone(),
            pid: note.pid,
            agent: note.agent,
            conversation: note.conversation.clone(),
            at: note.at,
            // A bubble on screen as the shell ends was not seen.
            unseen: kept.seen != Sight::Seen,
        }
    }

    /// The notification it was, its sender holding it no more.
    pub fn note(self) -> (Note, Sight) {
        let note = Note {
            id: 0,
            app: self.app,
            entry: self.entry,
            icon: None,
            summary: self.summary,
            body: self.body,
            actions: Vec::new(),
            default: false,
            urgency: Urgency::Normal,
            timeout: Timeout::Never,
            progress: None,
            resident: false,
            pid: self.pid,
            agent: self.agent,
            conversation: self.conversation,
            icon_from: self.icon_from,
            at: self.at,
        };
        let seen = if self.unseen {
            Sight::Unseen
        } else {
            Sight::Seen
        };
        (note, seen)
    }
}

/// The history as written: a JSON object a line.
pub fn encode(records: &[Record]) -> String {
    let mut out = String::new();
    for r in records {
        let line = serde_json::json!({
            "app": r.app,
            "entry": r.entry,
            "app_icon": r.icon_from.app_icon,
            "image_path": r.icon_from.image_path,
            "summary": r.summary,
            "body": r.body,
            "pid": r.pid,
            "agent": r.agent,
            "conversation": r.conversation,
            "at": r.at,
            "unseen": r.unseen,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}

/// What `text` keeps that is younger than 7 days at `now` (milliseconds),
/// and how many it dropped for their age; lines that do not read are left
/// out.
pub fn decode(text: &str, now: i64) -> (Vec<Record>, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let text = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_owned);
        let Some(at) = value.get("at").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if now - at >= KEEP_MS {
            dropped += 1;
            continue;
        }
        kept.push(Record {
            app: text("app").unwrap_or_default(),
            entry: text("entry"),
            icon_from: IconFrom {
                app_icon: text("app_icon").unwrap_or_default(),
                image_path: text("image_path"),
            },
            summary: text("summary").unwrap_or_default(),
            body: text("body").unwrap_or_default(),
            pid: value
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or(0),
            agent: value
                .get("agent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            conversation: value
                .get("conversation")
                .and_then(|c| serde_json::from_value(c.clone()).ok()),
            at,
            unseen: value
                .get("unseen")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        });
        if kept.len() == MOST {
            break;
        }
    }
    (kept, dropped)
}

// --- Its sections ----------------------------------------------------------------

/// The list's sections, in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    /// Under an hour.
    Recent,
    /// Today, an hour ago or more.
    Today,
    Yesterday,
    Older,
}

/// The section of a notification that came at `at` (milliseconds), at
/// `now` on the wall clock: by the hour, then by the days of the calendar.
pub fn section(at: i64, now: &Zoned) -> Section {
    let ago = now.timestamp().as_millisecond() - at;
    if ago < 3600 * 1000 {
        return Section::Recent;
    }
    let day = day_of(at, now);
    let today = now.date();
    if day == Some(today) {
        Section::Today
    } else if day.is_some() && day == today.yesterday().ok() {
        Section::Yesterday
    } else {
        Section::Older
    }
}

/// The calendar day of `at` (milliseconds) in `now`'s time zone.
fn day_of(at: i64, now: &Zoned) -> Option<Date> {
    let at = Timestamp::from_millisecond(at).ok()?;
    Some(at.to_zoned(now.time_zone().clone()).date())
}

/// How long ago `at` was at `now`: whole minutes under an hour, whole hours
/// today, else whole days (yesterday is a day).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ago {
    Now,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Days(i64),
}

pub fn ago(at: i64, now: &Zoned) -> Ago {
    let ms = (now.timestamp().as_millisecond() - at).max(0);
    let minutes = ms / 60_000;
    match section(at, now) {
        Section::Recent if minutes < 1 => Ago::Now,
        Section::Recent => Ago::Minutes(minutes),
        Section::Today => Ago::Hours(minutes / 60),
        Section::Yesterday => Ago::Yesterday,
        Section::Older => {
            let days = match (day_of(at, now), Some(now.date())) {
                (Some(then), Some(today)) => (today - then).get_days().into(),
                _ => ms / (24 * 3600 * 1000),
            };
            Ago::Days(days.max(2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::note::LIFE;

    pub(crate) fn note(id: u32, at: i64) -> Note {
        Note {
            id,
            app: "Slack".to_owned(),
            entry: Some("slack".to_owned()),
            icon: None,
            summary: format!("Message {id}"),
            body: "Bonjour\ttout le monde\n« ça va ? »".to_owned(),
            actions: vec![("reply".to_owned(), "Répondre".to_owned())],
            default: true,
            urgency: Urgency::Normal,
            timeout: Timeout::After(LIFE),
            progress: None,
            resident: false,
            pid: 42,
            agent: false,
            conversation: None,
            icon_from: IconFrom {
                app_icon: "slack".to_owned(),
                image_path: None,
            },
            at,
        }
    }

    const DAY: i64 = 24 * 3600 * 1000;

    #[test]
    fn the_newest_comes_first_and_a_replacement_takes_its_old_place_out() {
        let mut h = History::default();
        h.add(note(1, 1000), Sight::Showing);
        h.add(note(2, 2000), Sight::Unseen);
        h.add(note(1, 3000), Sight::Unseen);
        let ids: Vec<Option<u32>> = h.items().iter().map(|k| k.live).collect();
        assert_eq!(ids, [Some(1), Some(2)]);
        assert_eq!(h.unseen(), 2);
        // Open, the badge says nothing; closed, all is seen.
        h.set_open(true);
        assert_eq!(h.unseen(), 0);
        h.set_open(false);
        assert_eq!(h.unseen(), 0);
        assert!(h.items().iter().all(|k| k.seen == Sight::Seen));
        h.closed(2);
        assert_eq!(h.items()[1].live, None);
        assert!(h.remove_live(1));
        assert_eq!(h.items().len(), 1);
    }

    #[test]
    fn it_is_written_and_read_back_without_what_is_older_than_seven_days() {
        let now = 100 * DAY;
        let mut h = History::default();
        h.add(note(1, now - 8 * DAY), Sight::Seen);
        // An agent's conversation (#823) kept: a click after a restart
        // opens it.
        let conversation = hocus_link::AgentConversation {
            tool: "claude-code".to_owned(),
            session: "3f6c2a1e".to_owned(),
            folder: "/home/alex/site".into(),
            transcript: "/home/alex/.claude/projects/-home-alex-site/3f6c2a1e.jsonl".into(),
            pid: 40,
        };
        let mut two = note(2, now - DAY);
        two.conversation = Some(conversation.clone());
        h.add(two, Sight::Unseen);
        h.add(note(3, now - 1000), Sight::Showing);
        let records: Vec<Record> = h.items().iter().map(Record::of).collect();
        let text = encode(&records);
        assert_eq!(text.lines().count(), 3);
        let (read, dropped) = decode(&format!("{text}not json\n"), now);
        assert_eq!(dropped, 1);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], records[0]);
        // A bubble on screen as it was written is not seen.
        assert!(read[0].unseen);
        assert_eq!(read[1].body, "Bonjour\ttout le monde\n« ça va ? »");
        assert_eq!(read[1].conversation, Some(conversation));
        assert_eq!(read[0].conversation, None);
        let mut again = History::default();
        again.add(note(1, now), Sight::Showing);
        again.restore(read.into_iter().map(Record::note).collect());
        let ats: Vec<i64> = again.items().iter().map(|k| k.note.at).collect();
        assert_eq!(ats, [now, now - 1000, now - DAY]);
        assert_eq!(again.items()[1].live, None);
        assert!(again.items()[1].note.actions.is_empty());
        assert_eq!(again.unseen(), 2);
        // Seven days on, the older goes.
        assert!(again.prune(now + 7 * DAY - DAY + 1));
        assert_eq!(again.items().len(), 2);
    }

    #[test]
    fn its_sections_are_by_the_hour_then_by_the_days_of_the_calendar() {
        let now: Zoned = "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time");
        let at = |s: &str| -> i64 {
            s.parse::<Zoned>()
                .expect("a time")
                .timestamp()
                .as_millisecond()
        };
        let ms = now.timestamp().as_millisecond();
        assert_eq!(section(ms - 9 * 60_000, &now), Section::Recent);
        assert_eq!(ago(ms - 9 * 60_000, &now), Ago::Minutes(9));
        assert_eq!(ago(ms - 20_000, &now), Ago::Now);
        let morning = at("2026-11-02T08:10:00+01:00[Europe/Paris]");
        assert_eq!(section(morning, &now), Section::Today);
        assert_eq!(ago(morning, &now), Ago::Hours(6));
        let late = at("2026-11-01T23:50:00+01:00[Europe/Paris]");
        assert_eq!(section(late, &now), Section::Yesterday);
        let before = at("2026-10-30T12:00:00+01:00[Europe/Paris]");
        assert_eq!(section(before, &now), Section::Older);
        assert_eq!(ago(before, &now), Ago::Days(3));
    }
}
