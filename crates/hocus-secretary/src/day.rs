//! Her day's records (lot 5), beside the graph in her encrypted memory:
//! what the briefing and her reminders are computed from, in code.
//!
//! - **The agenda**: each event a source read, with its end, its
//!   calendar, who is invited, who organises and who presents: today's
//!   meetings, who is on leave (the events of the calendar a rule names),
//!   free slots for every invitee.
//! - **The mails**: each one's headers (who, to whom, its subject through
//!   the first pass, its message id and what it answers, sent or
//!   received) and, once the local triage read it, whether it is for the
//!   user and waits for an answer: "À RÉPONDRE" is those of them nobody
//!   answered in Sent.
//! - **The reminders**: what she will say at a time (a commitment the
//!   user made in a mail sent, a task's time), until when it can still be
//!   acted on, and where it stands: said, done, later, for the next
//!   briefing, never again.
//! - **The plans**: the tasks the user put in To do through her, by day:
//!   "AUJOURD'HUI, TU M'AS DIT" and "HIER, TU VOULAIS".
//!
//! Every row has its notebook, and the source it came from: forgetting a
//! source or a notebook forgets them with the rest ([`forget_source`],
//! [`forget_notebook`]). The tables are made when missing, with the
//! store's; they hold no text but subjects and titles, as the episodes'
//! digests do.

use hocus_secretary_link::Notebook;
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};

use crate::store::{stamp, unstamp};

pub const SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agenda (
    pointer TEXT PRIMARY KEY,
    source TEXT,
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    title TEXT NOT NULL,
    calendar TEXT NOT NULL DEFAULT '',
    attendees TEXT NOT NULL DEFAULT '[]',
    organiser TEXT,
    presenter TEXT,
    busy INTEGER NOT NULL DEFAULT 1,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    mine INTEGER NOT NULL DEFAULT 0,
    declined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS agenda_start ON agenda (start);

CREATE TABLE IF NOT EXISTS mails (
    pointer TEXT PRIMARY KEY,
    source TEXT,
    at TEXT NOT NULL,
    sender TEXT NOT NULL DEFAULT '',
    recipients TEXT NOT NULL DEFAULT '[]',
    subject TEXT NOT NULL DEFAULT '',
    message_id TEXT,
    in_reply_to TEXT,
    sent INTEGER NOT NULL DEFAULT 0,
    for_me INTEGER,
    needs_reply INTEGER,
    urgency TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso'))
);
CREATE INDEX IF NOT EXISTS mails_at ON mails (at);

CREATE TABLE IF NOT EXISTS reminders (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    due TEXT NOT NULL,
    until TEXT,
    next TEXT,
    state TEXT NOT NULL DEFAULT 'open'
        CHECK (state IN ('open', 'asked', 'said', 'done', 'briefing', 'never')),
    pointer TEXT,
    source TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    recorded_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plans (
    id INTEGER PRIMARY KEY,
    day TEXT NOT NULL,
    title TEXT NOT NULL,
    project TEXT,
    source TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    recorded_at TEXT NOT NULL
);
"#;

/// The agenda's columns added since its table was first made (#762):
/// added to a memory that has it without them.
const ADDED: [(&str, &str); 2] = [
    (
        "mine",
        "ALTER TABLE agenda ADD COLUMN mine INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "declined",
        "ALTER TABLE agenda ADD COLUMN declined INTEGER NOT NULL DEFAULT 0",
    ),
];

/// The tables made when missing, and the columns added since.
pub fn make(conn: &Connection) -> Result<()> {
    conn.execute_batch(SQL)?;
    let mut statement = conn.prepare("SELECT name FROM pragma_table_info('agenda')")?;
    let have: Vec<String> = statement
        .query_map([], |r| r.get(0))?
        .collect::<Result<_>>()?;
    for (column, sql) in ADDED {
        if !have.iter().any(|c| c == column) {
            conn.execute_batch(sql)?;
        }
    }
    Ok(())
}

/// An event of the agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub pointer: String,
    pub source: Option<String>,
    pub start: Timestamp,
    pub end: Timestamp,
    pub title: String,
    pub calendar: String,
    pub attendees: Vec<String>,
    pub organiser: Option<String>,
    pub presenter: Option<String>,
    pub busy: bool,
    pub notebook: Notebook,
    /// The user's own: on a calendar of theirs, or accepted (#762).
    pub mine: bool,
    /// Declined by the user, or cancelled.
    pub declined: bool,
}

/// A mail's headers, and what the triage made of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mail {
    pub pointer: String,
    pub source: Option<String>,
    pub at: Timestamp,
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub sent: bool,
    pub for_me: Option<bool>,
    pub needs_reply: Option<bool>,
    pub urgency: Option<String>,
    pub notebook: Notebook,
}

/// Where a reminder stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Waiting for its time.
    Open,
    /// Asked to be said (the gate has it).
    Asked,
    /// Her peek came out with it.
    Said,
    Done,
    /// For the next briefing: past its time, or asked for there.
    Briefing,
    /// "Ne plus me le dire".
    Never,
}

impl State {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Asked => "asked",
            Self::Said => "said",
            Self::Done => "done",
            Self::Briefing => "briefing",
            Self::Never => "never",
        }
    }

    fn of(word: &str) -> Self {
        match word {
            "asked" => Self::Asked,
            "said" => Self::Said,
            "done" => Self::Done,
            "briefing" => Self::Briefing,
            "never" => Self::Never,
            _ => Self::Open,
        }
    }
}

/// Something she will say at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: i64,
    /// `commitment`, `task`.
    pub kind: String,
    pub text: String,
    pub due: Timestamp,
    /// Past this it can no longer be acted on: listed in a briefing, not
    /// said.
    pub until: Option<Timestamp>,
    /// When to say it next ("Plus tard"); none: at `due`.
    pub next: Option<Timestamp>,
    pub state: State,
    pub pointer: Option<String>,
    pub notebook: Notebook,
}

impl Reminder {
    /// When it is to be said next.
    pub fn when(&self) -> Timestamp {
        self.next.unwrap_or(self.due)
    }

    /// Whether it can still be acted on at `now`.
    pub fn actionable(&self, now: Timestamp) -> bool {
        self.until.is_none_or(|until| now < until)
    }
}

/// A plan's line: a task the user put in To do through her.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: i64,
    pub day: jiff::civil::Date,
    pub title: String,
    pub project: Option<String>,
    pub notebook: Notebook,
}

type Result<T> = rusqlite::Result<T>;

fn notebook(word: &str) -> Notebook {
    Notebook::of(word).unwrap_or(Notebook::Pro)
}

fn time(text: &str) -> Timestamp {
    unstamp(text).unwrap_or(Timestamp::UNIX_EPOCH)
}

fn names(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

fn json(names: &[String]) -> String {
    serde_json::to_string(names).unwrap_or_else(|_| "[]".to_owned())
}

/// An event read (again: the same pointer replaces it).
pub fn keep_event(conn: &Connection, event: &Event) -> Result<()> {
    conn.execute(
        "INSERT INTO agenda (pointer, source, start, end, title, calendar, attendees, organiser,
             presenter, busy, notebook, mine, declined)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (pointer) DO UPDATE SET source = excluded.source, start = excluded.start,
             end = excluded.end, title = excluded.title, calendar = excluded.calendar,
             attendees = excluded.attendees, organiser = excluded.organiser,
             presenter = excluded.presenter, busy = excluded.busy, notebook = excluded.notebook,
             mine = excluded.mine, declined = excluded.declined",
        params![
            event.pointer,
            event.source,
            stamp(event.start),
            stamp(event.end),
            event.title,
            event.calendar,
            json(&event.attendees),
            event.organiser,
            event.presenter,
            event.busy,
            event.notebook.word(),
            event.mine,
            event.declined,
        ],
    )?;
    Ok(())
}

/// Event `pointer`, of `source`, is gone at its source (deleted, #762):
/// off the user's day and no longer busy, its row kept (the episodes keep
/// what it was). One not recorded gets a row that says only that.
pub fn gone(conn: &Connection, pointer: &str, source: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO agenda (pointer, source, start, end, title, busy, notebook, declined)
         VALUES (?1, ?2, '', '', '', 0, 'pro', 1)
         ON CONFLICT (pointer) DO UPDATE SET busy = 0, declined = 1",
        params![pointer, source],
    )?;
    Ok(())
}

/// The events that overlap `[from, to)`, by start.
pub fn events(conn: &Connection, from: Timestamp, to: Timestamp) -> Result<Vec<Event>> {
    let mut statement = conn.prepare(
        "SELECT pointer, source, start, end, title, calendar, attendees, organiser, presenter,
                busy, notebook, mine, declined
         FROM agenda WHERE start < ?2 AND end > ?1 ORDER BY start, pointer",
    )?;
    let rows = statement.query_map(params![stamp(from), stamp(to)], |r| {
        Ok(Event {
            pointer: r.get(0)?,
            source: r.get(1)?,
            start: time(&r.get::<_, String>(2)?),
            end: time(&r.get::<_, String>(3)?),
            title: r.get(4)?,
            calendar: r.get(5)?,
            attendees: names(&r.get::<_, String>(6)?),
            organiser: r.get(7)?,
            presenter: r.get(8)?,
            busy: r.get(9)?,
            notebook: notebook(&r.get::<_, String>(10)?),
            mine: r.get(11)?,
            declined: r.get(12)?,
        })
    })?;
    rows.collect()
}

/// A mail read.
pub fn keep_mail(conn: &Connection, mail: &Mail) -> Result<()> {
    conn.execute(
        "INSERT INTO mails (pointer, source, at, sender, recipients, subject, message_id,
             in_reply_to, sent, notebook)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT (pointer) DO NOTHING",
        params![
            mail.pointer,
            mail.source,
            stamp(mail.at),
            mail.sender,
            json(&mail.recipients),
            mail.subject,
            mail.message_id,
            mail.in_reply_to,
            mail.sent,
            mail.notebook.word(),
        ],
    )?;
    Ok(())
}

/// What the triage made of mail `pointer`.
pub fn triaged_mail(
    conn: &Connection,
    pointer: &str,
    for_me: bool,
    needs_reply: bool,
    urgency: &str,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE mails SET for_me = ?2, needs_reply = ?3, urgency = ?4 WHERE pointer = ?1",
        params![pointer, for_me, needs_reply, urgency],
    )?;
    Ok(changed > 0)
}

pub fn mail(conn: &Connection, pointer: &str) -> Result<Option<Mail>> {
    conn.query_row(
        &format!("SELECT {MAIL} FROM mails WHERE pointer = ?1"),
        [pointer],
        mail_of,
    )
    .optional()
}

const MAIL: &str = "pointer, source, at, sender, recipients, subject, message_id, in_reply_to, \
                    sent, for_me, needs_reply, urgency, notebook";

fn mail_of(r: &rusqlite::Row<'_>) -> Result<Mail> {
    Ok(Mail {
        pointer: r.get(0)?,
        source: r.get(1)?,
        at: time(&r.get::<_, String>(2)?),
        sender: r.get(3)?,
        recipients: names(&r.get::<_, String>(4)?),
        subject: r.get(5)?,
        message_id: r.get(6)?,
        in_reply_to: r.get(7)?,
        sent: r.get(8)?,
        for_me: r.get(9)?,
        needs_reply: r.get(10)?,
        urgency: r.get(11)?,
        notebook: notebook(&r.get::<_, String>(12)?),
    })
}

/// The mails of `[from, to)`, oldest first.
pub fn mails(conn: &Connection, from: Timestamp, to: Timestamp) -> Result<Vec<Mail>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {MAIL} FROM mails WHERE at >= ?1 AND at < ?2 ORDER BY at, pointer"
    ))?;
    let rows = statement.query_map(params![stamp(from), stamp(to)], mail_of)?;
    rows.collect()
}

/// Every mail the user sent since `from`.
pub fn sent_since(conn: &Connection, from: Timestamp) -> Result<Vec<Mail>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {MAIL} FROM mails WHERE sent = 1 AND at >= ?1 ORDER BY at"
    ))?;
    let rows = statement.query_map(params![stamp(from)], mail_of)?;
    rows.collect()
}

/// A new reminder; its id.
pub fn remind(
    conn: &Connection,
    reminder: &Reminder,
    source: Option<&str>,
    now: Timestamp,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO reminders (kind, text, due, until, next, state, pointer, source, notebook,
             recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            reminder.kind,
            reminder.text,
            stamp(reminder.due),
            reminder.until.map(stamp),
            reminder.next.map(stamp),
            reminder.state.word(),
            reminder.pointer,
            source,
            reminder.notebook.word(),
            stamp(now),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

const REMINDER: &str = "id, kind, text, due, until, next, state, pointer, notebook";

fn reminder_of(r: &rusqlite::Row<'_>) -> Result<Reminder> {
    Ok(Reminder {
        id: r.get(0)?,
        kind: r.get(1)?,
        text: r.get(2)?,
        due: time(&r.get::<_, String>(3)?),
        until: r.get::<_, Option<String>>(4)?.map(|t| time(&t)),
        next: r.get::<_, Option<String>>(5)?.map(|t| time(&t)),
        state: State::of(&r.get::<_, String>(6)?),
        pointer: r.get(7)?,
        notebook: notebook(&r.get::<_, String>(8)?),
    })
}

pub fn reminder(conn: &Connection, id: i64) -> Result<Option<Reminder>> {
    conn.query_row(
        &format!("SELECT {REMINDER} FROM reminders WHERE id = ?1"),
        [id],
        reminder_of,
    )
    .optional()
}

/// The reminders in `states`, by when they are to be said.
pub fn reminders(conn: &Connection, states: &[State]) -> Result<Vec<Reminder>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {REMINDER} FROM reminders ORDER BY coalesce(next, due), id"
    ))?;
    let rows = statement.query_map([], reminder_of)?;
    let all: Vec<Reminder> = rows.collect::<Result<_>>()?;
    Ok(all
        .into_iter()
        .filter(|r| states.contains(&r.state))
        .collect())
}

/// Reminder `id` now stands at `state`, to be said next at `next`.
pub fn set_reminder(
    conn: &Connection,
    id: i64,
    state: State,
    next: Option<Timestamp>,
) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET state = ?2, next = coalesce(?3, next) WHERE id = ?1",
        params![id, state.word(), next.map(stamp)],
    )?;
    Ok(())
}

/// A task put in To do for `day`.
pub fn plan(
    conn: &Connection,
    day: jiff::civil::Date,
    title: &str,
    project: Option<&str>,
    notebook: Notebook,
    now: Timestamp,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO plans (day, title, project, notebook, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![day.to_string(), title, project, notebook.word(), stamp(now)],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The plan of `day`.
pub fn plans(conn: &Connection, day: jiff::civil::Date) -> Result<Vec<Plan>> {
    let mut statement = conn.prepare(
        "SELECT id, day, title, project, notebook FROM plans WHERE day = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([day.to_string()], |r| {
        Ok(Plan {
            id: r.get(0)?,
            day: r.get::<_, String>(1)?.parse().unwrap_or(day),
            title: r.get(2)?,
            project: r.get(3)?,
            notebook: notebook(&r.get::<_, String>(4)?),
        })
    })?;
    rows.collect()
}

/// A source revoked: what it brought goes.
pub fn forget_source(conn: &Connection, source: &str) -> Result<()> {
    for table in ["agenda", "mails", "reminders", "plans"] {
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE source{}",
                crate::store::OF_SOURCE
            ),
            [source],
        )?;
    }
    Ok(())
}

/// A source's records move to `notebook` (#752).
pub fn refile_source(conn: &Connection, source: &str, notebook: Notebook) -> Result<()> {
    for table in ["agenda", "mails", "reminders", "plans"] {
        conn.execute(
            &format!(
                "UPDATE {table} SET notebook = ?2 WHERE source{}",
                crate::store::OF_SOURCE
            ),
            rusqlite::params![source, notebook.word()],
        )?;
    }
    Ok(())
}

/// A notebook forgotten.
pub fn forget_notebook(conn: &Connection, notebook: Notebook) -> Result<()> {
    for table in ["agenda", "mails", "reminders", "plans"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE notebook = ?1"),
            [notebook.word()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: &str) -> Timestamp {
        t.parse().expect("a time")
    }

    #[test]
    fn the_days_records_keep_and_go_with_their_source() {
        let conn = Connection::open_in_memory().expect("a database");
        make(&conn).expect("the tables");
        make(&conn).expect("made again: nothing changes");
        let event = Event {
            pointer: "cal:e1".to_owned(),
            source: Some("cal".to_owned()),
            start: at("2026-11-10T13:00:00Z"),
            end: at("2026-11-10T14:00:00Z"),
            title: "Revue de sprint".to_owned(),
            calendar: "Travail".to_owned(),
            attendees: vec!["Sophie Marchand".to_owned(), "Karim".to_owned()],
            organiser: Some("Karim".to_owned()),
            presenter: Some("Sophie Marchand".to_owned()),
            busy: true,
            notebook: Notebook::Pro,
            mine: true,
            declined: false,
        };
        keep_event(&conn, &event).expect("kept");
        keep_event(&conn, &event).expect("kept again");
        let found = events(
            &conn,
            at("2026-11-10T00:00:00Z"),
            at("2026-11-11T00:00:00Z"),
        )
        .expect("read");
        assert_eq!(found, vec![event]);
        let id = remind(
            &conn,
            &Reminder {
                id: 0,
                kind: "commitment".to_owned(),
                text: "le devis à Léa".to_owned(),
                due: at("2026-11-10T09:00:00Z"),
                until: Some(at("2026-11-10T17:00:00Z")),
                next: None,
                state: State::Open,
                pointer: None,
                notebook: Notebook::Pro,
            },
            Some("cal"),
            at("2026-11-09T09:00:00Z"),
        )
        .expect("a reminder");
        set_reminder(&conn, id, State::Said, None).expect("said");
        assert_eq!(
            reminder(&conn, id).expect("read").map(|r| r.state),
            Some(State::Said)
        );
        forget_source(&conn, "cal").expect("forgotten");
        assert!(
            events(
                &conn,
                at("2026-11-10T00:00:00Z"),
                at("2026-11-11T00:00:00Z")
            )
            .expect("read")
            .is_empty()
        );
        assert_eq!(reminder(&conn, id).expect("read"), None);
    }

    #[test]
    fn an_agenda_made_before_its_columns_has_them_added() {
        let conn = Connection::open_in_memory().expect("a database");
        conn.execute_batch(
            "CREATE TABLE agenda (pointer TEXT PRIMARY KEY, source TEXT, start TEXT NOT NULL,
                 end TEXT NOT NULL, title TEXT NOT NULL, calendar TEXT NOT NULL DEFAULT '',
                 attendees TEXT NOT NULL DEFAULT '[]', organiser TEXT, presenter TEXT,
                 busy INTEGER NOT NULL DEFAULT 1,
                 notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')));
             INSERT INTO agenda (pointer, start, end, title, notebook)
                 VALUES ('cal:e0', '2026-11-10T08:00:00Z', '2026-11-10T09:00:00Z', 'Daily', 'pro');",
        )
        .expect("an older agenda");
        make(&conn).expect("its columns added");
        let found = events(
            &conn,
            at("2026-11-10T00:00:00Z"),
            at("2026-11-11T00:00:00Z"),
        )
        .expect("read");
        assert_eq!(found.len(), 1);
        assert!(!found[0].mine && !found[0].declined);
    }
}
