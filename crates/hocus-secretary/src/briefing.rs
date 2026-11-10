//! Her briefings (docs/design/secretary.md §6.1, §3.10), computed in code
//! from her day's records ([`crate::day`]), her graph and the facts
//! (§2.4): the morning's, and "Pendant ton absence" after a suspend or a
//! shutdown of over four hours. Her model only words what code found: one
//! sentence on the day, and the rest in three lines ([`Built::facts`] is
//! what it is given); without it, the briefing stands on its own.
//!
//! - **AUJOURD'HUI**: today's events that take time, amber when someone
//!   in them is away.
//! - **EN CONGÉS**: the events of the calendar the leaves rule names
//!   (`{"kind": "leave", "calendar": "Congés"}`, a rule's compiled form, lot
//!   6's) that cover today, for people the user works with (known to her
//!   graph, or invited to their meetings); an absent person invited to,
//!   or presenting, one of today's meetings gives a proposal: the first
//!   slot free for every invitee from the next working day ([`Move`]).
//! - **À RÉPONDRE**: at most three mails for the user, from a person,
//!   waiting for an answer (the triage said so), with none in Sent.
//! - **LE RESTE**: her model's; else what came, counted.
//! - **HIER, TU VOULAIS** and **AUJOURD'HUI, TU M'AS DIT**: the tasks put
//!   in To do through her, by day.
//! - **À NE PAS OUBLIER**: reminders gone by unsaid, or kept for here.
//! - **EN PRIVÉ**: perso events, reminders and birthdays ahead.
//!
//! Every line carries its notebook: the drawer shows no perso title in a
//! public moment, only how many are hidden (§7.4). The agents' night is
//! the shell's to add: it knows them.

use hocus_secretary_link::{Briefing, BriefingKind, Line, Notebook, Section};
use hocus_sources::facts::{self, Hours, Region, Span};
use jiff::{SignedDuration, Timestamp, ToSpan, Zoned, civil::Date, tz::TimeZone};

use crate::{
    day::{self, Event, Mail, State},
    store::Store,
};

/// What a briefing is computed from.
#[derive(Debug, Clone)]
pub struct Input {
    pub now: Timestamp,
    pub zone: TimeZone,
    pub english: bool,
    /// What she read, for the footer ("agenda, Gmail").
    pub read: Vec<String>,
    /// The absence it covers, if one: from, to.
    pub away: Option<(Timestamp, Timestamp)>,
    /// A public moment (§7.4): nothing perso goes to her model.
    pub public: bool,
}

/// A meeting to move: its presenter or an invitee away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub meeting: Event,
    pub absent: String,
    pub until: Option<Date>,
    /// The first slot free for every invitee, as long as the meeting.
    pub slot: Option<Span>,
    pub organiser: Option<String>,
    pub invitees: usize,
}

/// A briefing built, and what her model is given to word it.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub briefing: Briefing,
    pub moves: Vec<Move>,
    /// The computed lines, as her model reads them.
    pub facts: String,
    /// Reminders listed here (no longer to be said).
    pub listed: Vec<i64>,
    /// How many things came while away, and how many matter.
    pub arrived: usize,
    /// What is in it, for her peek (#758): "3 réunions, 2 mails attendent
    /// ta réponse, Sophie est en congés".
    pub peek: String,
}

/// What opens the item `pointer` at its source, if anything does.
fn open_of(store: &Store, pointer: &str) -> Option<String> {
    store
        .conn
        .query_row(
            "SELECT open FROM episodes WHERE pointer = ?1 AND open IS NOT NULL ORDER BY id DESC LIMIT 1",
            [pointer],
            |r| r.get::<_, String>(0),
        )
        .ok()
}

/// The day's free slots of an hour or more between 9 h and 18 h, around
/// `meetings` (#758).
fn free_slots(meetings: &[Event], day: Date, zone: &TimeZone) -> Vec<(Zoned, Zoned)> {
    let at = |h: i8| {
        day.at(h, 0, 0, 0)
            .to_zoned(zone.clone())
            .map(|z| z.timestamp())
            .ok()
    };
    let (Some(open), Some(close)) = (at(9), at(18)) else {
        return Vec::new();
    };
    let mut busy: Vec<(Timestamp, Timestamp)> = meetings
        .iter()
        .filter(|m| m.end > open && m.start < close)
        .map(|m| (m.start.max(open), m.end.min(close)))
        .collect();
    busy.sort();
    let mut free = Vec::new();
    let mut from = open;
    for (start, end) in busy.into_iter().chain([(close, close)]) {
        if start.duration_since(from) >= SignedDuration::from_hours(1) {
            free.push((from.to_zoned(zone.clone()), start.to_zoned(zone.clone())));
        }
        from = from.max(end);
    }
    free
}

fn w<'a>(english: bool, fr: &'a str, en: &'a str) -> &'a str {
    if english { en } else { fr }
}

/// "9 h 30" / "9:30".
pub fn hour(at: &Zoned, english: bool) -> String {
    match (english, at.minute()) {
        (true, m) => format!("{}:{m:02}", at.hour()),
        (false, 0) => format!("{} h", at.hour()),
        (false, m) => format!("{} h {m:02}", at.hour()),
    }
}

const DAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Mardi 10 novembre" / "Tuesday 10 November".
pub fn date(day: Date, english: bool) -> String {
    if english {
        let weekday = DAYS_EN[usize::try_from(day.weekday().to_monday_zero_offset()).unwrap_or(0)];
        let month = MONTHS_EN[usize::try_from(day.month() - 1).unwrap_or(0)];
        format!("{weekday} {} {month}", day.day())
    } else {
        capital(&facts::french_date(day))
    }
}

/// A day in a sentence: "mardi 10 novembre" / "Tuesday 10 November".
pub fn day_in_sentence(day: Date, english: bool) -> String {
    if english {
        date(day, true)
    } else {
        facts::french_date(day)
    }
}

/// "lun. 16" / "Mon 16".
fn short(day: Date, english: bool) -> String {
    let long = if english {
        DAYS_EN[usize::try_from(day.weekday().to_monday_zero_offset()).unwrap_or(0)].to_owned()
    } else {
        facts::french_date(day)
    };
    let weekday: String = long.chars().take(3).collect();
    if english {
        format!("{weekday} {}", day.day())
    } else {
        format!("{weekday}. {}", day.day())
    }
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// When, said short: "9 h 12" today, "hier 17 h 40", "jeu.".
fn when(at: Timestamp, now: &Zoned, english: bool) -> String {
    let at = at.to_zoned(now.time_zone().clone());
    let today = now.date();
    if at.date() == today {
        hour(&at, english)
    } else if Some(at.date()) == today.yesterday().ok() {
        format!("{} {}", w(english, "hier", "yesterday"), hour(&at, english))
    } else {
        short(at.date(), english)
    }
}

/// The day's bounds, in `zone`.
fn bounds(day: Date, zone: &TimeZone) -> (Timestamp, Timestamp) {
    let start = day
        .to_zoned(zone.clone())
        .map_or(Timestamp::UNIX_EPOCH, |z| z.timestamp());
    let end = day
        .tomorrow()
        .ok()
        .and_then(|d| d.to_zoned(zone.clone()).ok())
        .map_or(start, |z| z.timestamp());
    (start, end)
}

/// The calendar the leaves rule names, if one is taught (lot 6 compiles
/// the rule: [`crate::learn::Compiled::Leave`]).
pub fn leaves_calendar(store: &Store) -> Option<String> {
    let mut statement = store
        .conn
        .prepare(
            "SELECT compiled FROM rules WHERE compiled IS NOT NULL AND accepted_at IS NOT NULL
               AND valid_to IS NULL ORDER BY id DESC",
        )
        .ok()?;
    let compiled: Vec<String> = statement
        .query_map([], |r| r.get::<_, String>(0))
        .ok()?
        .filter_map(std::result::Result::ok)
        .collect();
    compiled
        .into_iter()
        .find_map(|text| match serde_json::from_str(&text).ok()? {
            crate::learn::Compiled::Leave { calendar } => Some(calendar),
            _ => None,
        })
}

/// Rule `rule`'s compiled form (lot 6 writes it; tests too).
pub fn compile_rule(store: &Store, rule: i64, compiled: &str) -> crate::store::Result<()> {
    store.conn.execute(
        "UPDATE rules SET compiled = ?2 WHERE id = ?1",
        rusqlite::params![rule, compiled],
    )?;
    Ok(())
}

/// The user's first name, from her graph.
pub(crate) fn first_name(store: &Store) -> Option<String> {
    store
        .conn
        .query_row(
            "SELECT name FROM nodes WHERE kind = 'user' ORDER BY id LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|n| n.split_whitespace().next().map(str::to_owned))
}

fn folded(text: &str) -> String {
    crate::fold::fold(text)
}

/// Whether `name` is someone the user works with: known to her graph as
/// a person, or among `invited`.
fn works_with(store: &Store, name: &str, invited: &[String]) -> bool {
    let name = folded(name);
    invited.iter().any(|i| folded(i) == name)
        || store.find(&name).is_ok_and(|found| {
            found.into_iter().any(|id| {
                store
                    .node(id)
                    .is_ok_and(|n| n.kind == crate::graph::NodeKind::Person)
            })
        })
}

/// Who an event of the leaves calendar says is away: its organiser, else
/// its title less the words of a leave.
fn absent_of(event: &Event) -> String {
    if let Some(who) = event.organiser.as_deref().filter(|o| !o.trim().is_empty()) {
        return who.trim().to_owned();
    }
    let mut title = event.title.clone();
    for word in [
        "Congés", "congés", "Congé", "congé", "Conges", "conges", "Vacances", "vacances", "Absent",
        "absent", "Absente", "absente", "OOO", ":", "-", "–",
    ] {
        title = title.replace(word, " ");
    }
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A sender that is a person, not a robot or a list.
pub fn person(sender: &str) -> bool {
    let lower = sender.to_lowercase();
    ![
        "noreply",
        "no-reply",
        "no_reply",
        "donotreply",
        "notification",
        "newsletter",
        "mailer-daemon",
        "bounce",
        "digest",
        "alerts@",
        "info@",
        "news@",
        "marketing",
    ]
    .iter()
    .any(|robot| lower.contains(robot))
}

/// The display name of a sender: "Léa Martin <lea@…>" → "Léa Martin".
fn who(sender: &str) -> String {
    let name = sender.split('<').next().unwrap_or(sender).trim();
    let name = name.trim_matches('"').trim();
    if name.is_empty() {
        sender.trim().to_owned()
    } else {
        name.to_owned()
    }
}

/// Whether the user answered `mail`: a mail they sent after it that
/// replies to it, or to its sender with its subject.
fn answered(mail: &Mail, sent: &[Mail]) -> bool {
    let subject = folded(
        mail.subject
            .trim_start_matches(['R', 'r', 'e', 'E', ':', ' ']),
    );
    let sender = folded(&who(&mail.sender));
    sent.iter().filter(|s| s.at >= mail.at).any(|s| {
        (mail.message_id.is_some() && s.in_reply_to == mail.message_id)
            || (!subject.is_empty()
                && folded(&s.subject).contains(&subject)
                && s.recipients.iter().any(|r| folded(&who(r)) == sender))
    })
}

/// What waits for the user's answer: mails for them, from a person,
/// needing a reply, unanswered, since `from`; the most urgent first, then
/// the oldest; three at most.
pub fn to_answer(store: &Store, from: Timestamp, now: Timestamp) -> Vec<Mail> {
    let mails = day::mails(&store.conn, from, now).unwrap_or_default();
    let sent = day::sent_since(&store.conn, from).unwrap_or_default();
    let mut waiting: Vec<Mail> = mails
        .into_iter()
        .filter(|m| {
            !m.sent
                && m.for_me == Some(true)
                && m.needs_reply == Some(true)
                && person(&m.sender)
                && !answered(m, &sent)
        })
        .collect();
    let rank = |m: &Mail| match m.urgency.as_deref() {
        Some("high") => 0,
        Some("normal") => 1,
        _ => 2,
    };
    waiting.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.at.cmp(&b.at)));
    waiting.truncate(3);
    waiting
}

/// An episode's digest by its pointer (the triage's, masked, once read).
fn digest_of(store: &Store, pointer: &str) -> Option<String> {
    store
        .conn
        .query_row(
            "SELECT digest FROM episodes WHERE pointer = ?1 ORDER BY id DESC LIMIT 1",
            [pointer],
            |r| r.get::<_, String>(0),
        )
        .ok()
}

fn line(title: impl Into<String>, notebook: Notebook) -> Line {
    Line {
        title: title.into(),
        notebook,
        ..Line::default()
    }
}

/// The meetings of `day` that take time (not the leaves' calendar; the
/// perso notebook's are EN PRIVÉ's).
fn meetings(events: &[Event], leaves: Option<&str>) -> Vec<Event> {
    events
        .iter()
        .filter(|e| e.notebook == Notebook::Pro)
        .filter(|e| e.busy && Some(e.calendar.as_str()) != leaves && e.end > e.start)
        .filter(|e| e.end.duration_since(e.start) < SignedDuration::from_hours(20))
        .cloned()
        .collect()
}

/// Who is away today: name, and their last day away.
fn away_today(
    store: &Store,
    events: &[Event],
    leaves: &str,
    zone: &TimeZone,
) -> Vec<(String, Event, Option<Date>)> {
    let _ = store;
    events
        .iter()
        .filter(|e| folded(&e.calendar) == folded(leaves))
        .map(|e| {
            let last = e
                .end
                .checked_sub(SignedDuration::from_secs(1))
                .ok()
                .map(|t| t.to_zoned(zone.clone()).date());
            (absent_of(e), e.clone(), last)
        })
        .filter(|(name, _, _)| !name.is_empty())
        .collect()
}

/// The first slot free for every invitee of `meeting`, as long as it,
/// from the next working day, within two weeks: computed in code.
fn slot_for(store: &Store, meeting: &Event, today: Date, zone: &TimeZone) -> Option<Span> {
    let from = today.tomorrow().ok()?;
    let to = today.checked_add(14.days()).ok()?;
    let (start, _) = bounds(from, zone);
    let (_, end) = bounds(to, zone);
    let known = day::events(&store.conn, start, end).unwrap_or_default();
    // Each invitee's busy spans: the events they are in that the user's
    // calendars show (the user's own among them).
    let mut people: Vec<String> = meeting.attendees.clone();
    people.extend(meeting.organiser.clone());
    people.sort();
    people.dedup();
    let mut busy: Vec<Vec<Span>> = vec![
        known
            .iter()
            .filter(|e| e.busy && e.pointer != meeting.pointer)
            .map(|e| Span {
                start: e.start,
                end: e.end,
            })
            .collect(),
    ];
    for person in &people {
        let name = folded(person);
        busy.push(
            known
                .iter()
                .filter(|e| {
                    e.pointer != meeting.pointer
                        && (e.attendees.iter().any(|a| folded(a) == name)
                            || e.organiser.as_deref().is_some_and(|o| folded(o) == name))
                })
                .map(|e| Span {
                    start: e.start,
                    end: e.end,
                })
                .collect(),
        );
    }
    let length = meeting.end.duration_since(meeting.start);
    let free = facts::slots::free(
        &busy,
        from,
        to,
        Hours::default(),
        zone,
        Region::default(),
        length,
    );
    // The meeting's own hour if it is free that day, else the first.
    let hour_of = meeting.start.to_zoned(zone.clone()).time();
    free.iter()
        .find_map(|span| {
            let day = span.start.to_zoned(zone.clone()).date();
            let start = day
                .to_datetime(hour_of)
                .to_zoned(zone.clone())
                .ok()?
                .timestamp();
            let end = start.checked_add(length).ok()?;
            (span.start <= start && end <= span.end).then_some(Span { start, end })
        })
        .or_else(|| {
            free.first().map(|s| Span {
                start: s.start,
                end: s.start.checked_add(length).unwrap_or(s.end),
            })
        })
}

/// The morning briefing.
pub fn morning(store: &Store, input: &Input) -> Built {
    build(store, input, BriefingKind::Morning)
}

/// "Pendant ton absence".
pub fn absence(store: &Store, input: &Input) -> Built {
    build(store, input, BriefingKind::Absence)
}

#[allow(clippy::too_many_lines)]
fn build(store: &Store, input: &Input, kind: BriefingKind) -> Built {
    let en = input.english;
    let zone = &input.zone;
    let here = input.now.to_zoned(zone.clone());
    let today = here.date();
    let (day_start, day_end) = bounds(today, zone);
    let mut sections = Vec::new();
    let mut facts_lines = Vec::new();
    let mut moves = Vec::new();
    let mut listed = Vec::new();
    let mut arrived = 0;
    let events = day::events(&store.conn, day_start, day_end).unwrap_or_default();
    let leaves = leaves_calendar(store);
    let meetings_today = meetings(&events, leaves.as_deref());
    // Who is away today, and the meetings that change for it.
    let away = leaves
        .as_deref()
        .map(|l| away_today(store, &events, l, zone))
        .unwrap_or_default();
    let invited: Vec<String> = meetings_today
        .iter()
        .flat_map(|m| m.attendees.iter().chain(m.presenter.iter()).cloned())
        .collect();
    let away: Vec<(String, Event, Option<Date>)> = away
        .into_iter()
        .filter(|(name, _, _)| works_with(store, name, &invited))
        .collect();
    let is_away = |name: &str| away.iter().any(|(n, _, _)| folded(n) == folded(name));

    // "Pendant ton absence": what came while away.
    if let Some((from, to)) = input.away {
        let mails = day::mails(&store.conn, from, to.max(input.now)).unwrap_or_default();
        let episodes =
            crate::retrieve::episodes_between(store, (from, to.max(input.now)), None, 500)
                .unwrap_or_default();
        arrived = episodes
            .iter()
            .filter(|e| e.kind != "catch-up" && e.kind != "event")
            .count();
        let matter = to_answer(store, from, to.max(input.now));
        let mut lines = Vec::new();
        let received = mails.iter().filter(|m| !m.sent).count();
        if received > 0 || arrived > 0 {
            let count = if received > 0 { received } else { arrived };
            let text = match (en, matter.len()) {
                (false, 0) => format!(
                    "{count} {}{s} arrivé{s}, rien qui attende ta réponse",
                    if received > 0 { "mail" } else { "message" },
                    s = if count > 1 { "s" } else { "" }
                ),
                (false, n) => format!(
                    "{count} {}{s} arrivé{s}, dont {n} pour toi",
                    if received > 0 { "mail" } else { "message" },
                    s = if count > 1 { "s" } else { "" }
                ),
                (true, 0) => format!("{count} came, nothing waiting for your answer"),
                (true, n) => format!("{count} came, {n} for you"),
            };
            lines.push(line(text, Notebook::Pro));
        }
        for mail in &matter {
            let mut l = line(
                format!("{} · {}", who(&mail.sender), mail.subject),
                mail.notebook,
            );
            l.detail = digest_of(store, &mail.pointer)
                .filter(|d| *d != mail.subject)
                .unwrap_or_default();
            l.when = when(mail.at, &here, en);
            l.attention = true;
            l.pointer.clone_from(&mail.pointer);
            l.open = open_of(store, &mail.pointer);
            lines.push(l);
        }
        // Perso items that came: their digests.
        for e in episodes
            .iter()
            .filter(|e| e.notebook == Notebook::Perso && e.kind != "event" && e.kind != "catch-up")
            .take(3)
        {
            lines.push(line(e.digest.clone(), Notebook::Perso));
        }
        if !lines.is_empty() {
            for l in lines
                .iter()
                .filter(|l| !(input.public && l.notebook == Notebook::Perso))
            {
                facts_lines.push(format!("[absence] {}", l.title));
            }
            sections.push(Section {
                key: "away".to_owned(),
                label: w(en, "PENDANT TON ABSENCE", "WHILE YOU WERE AWAY").to_owned(),
                note: span_words(from, to, &here, en),
                lines,
                proposal: None,
            });
        }
    }

    // Reminders gone by unsaid, or kept for here.
    let reminders = day::reminders(&store.conn, &[State::Open, State::Asked, State::Briefing])
        .unwrap_or_default();
    let mut remind_lines = Vec::new();
    for r in &reminders {
        let past = r.when() <= input.now && !r.actionable(input.now);
        if r.state == State::Briefing
            || past
            || input
                .away
                .is_some_and(|(f, t)| r.when() >= f && r.when() <= t && !r.actionable(input.now))
        {
            let mut l = line(r.text.clone(), r.notebook);
            let due = r.due.to_zoned(zone.clone());
            l.when = when(r.due, &here, en);
            l.detail = if past {
                w(en, "son heure est passée", "its time has passed").to_owned()
            } else {
                String::new()
            };
            let _ = due;
            l.attention = true;
            remind_lines.push(l);
            listed.push(r.id);
        }
    }

    if kind == BriefingKind::Morning {
        // AUJOURD'HUI.
        let mut lines = Vec::new();
        for m in &meetings_today {
            let start = m.start.to_zoned(zone.clone());
            let minutes = m.end.duration_since(m.start).as_mins();
            let length = if minutes >= 60 && minutes % 60 == 0 {
                format!("{} h", minutes / 60)
            } else if minutes > 60 {
                format!("{} h {:02}", minutes / 60, minutes % 60)
            } else {
                format!("{minutes} min")
            };
            let mut l = line(m.title.clone(), m.notebook);
            l.time = hour(&start, en);
            let absent: Vec<&String> = m
                .attendees
                .iter()
                .chain(m.presenter.iter())
                .filter(|a| is_away(a))
                .collect();
            l.detail = match (m.presenter.as_deref(), absent.first()) {
                (Some(p), Some(_)) if is_away(p) => {
                    if en {
                        format!("{length} · {p} presents it, away today")
                    } else {
                        format!("{length} · {p} y présente, en congés")
                    }
                }
                (_, Some(a)) => {
                    if en {
                        format!("{length} · {a} is away")
                    } else {
                        format!("{length} · {a} est en congés")
                    }
                }
                (Some(p), None) if en => format!("{length} · {p} presents"),
                (Some(p), None) => format!("{length} · {p} présente"),
                (None, None) => length,
            };
            l.attention = !absent.is_empty();
            facts_lines.push(format!("[réunion] {} {} — {}", l.time, l.title, l.detail));
            lines.push(l);
        }
        if !lines.is_empty() {
            let n = lines.len();
            sections.push(Section {
                key: "today".to_owned(),
                label: w(en, "AUJOURD’HUI", "TODAY").to_owned(),
                note: match (en, n) {
                    (false, 1) => "1 réunion".to_owned(),
                    (false, n) => format!("{n} réunions"),
                    (true, 1) => "1 meeting".to_owned(),
                    (true, n) => format!("{n} meetings"),
                },
                lines,
                proposal: None,
            });
        }
        // EN CONGÉS, and what it changes.
        if let Some(calendar) = leaves.as_deref().filter(|_| !away.is_empty()) {
            let mut lines = Vec::new();
            for (name, _, last) in &away {
                let mut l = line(name.clone(), Notebook::Pro);
                l.detail = match last {
                    Some(last) if *last > today => {
                        if en {
                            format!("until {}", short(*last, en))
                        } else {
                            format!("jusqu’au {}", short(*last, en))
                        }
                    }
                    _ => w(en, "aujourd’hui", "today").to_owned(),
                };
                facts_lines.push(format!("[congés, calculé] {name} — {}", l.detail));
                lines.push(l);
                for meeting in meetings_today.iter().filter(|m| {
                    m.attendees.iter().any(|a| folded(a) == folded(name))
                        || m.presenter
                            .as_deref()
                            .is_some_and(|p| folded(p) == folded(name))
                }) {
                    let slot = slot_for(store, meeting, today, zone);
                    moves.push(Move {
                        meeting: meeting.clone(),
                        absent: name.clone(),
                        until: *last,
                        slot,
                        organiser: meeting.organiser.clone(),
                        invitees: meeting.attendees.len(),
                    });
                }
            }
            sections.push(Section {
                key: "leaves".to_owned(),
                label: w(en, "EN CONGÉS", "ON LEAVE").to_owned(),
                note: if en {
                    format!("calendar “{calendar}”")
                } else {
                    format!("calendrier «\u{a0}{calendar}\u{a0}»")
                },
                lines,
                proposal: None,
            });
        }
        // À RÉPONDRE.
        let since = input
            .now
            .checked_sub(SignedDuration::from_hours(24 * 7))
            .unwrap_or(input.now);
        let waiting = to_answer(store, since, input.now);
        if !waiting.is_empty() {
            let lines: Vec<Line> = waiting
                .iter()
                .map(|m| {
                    let mut l = line(format!("{} · {}", who(&m.sender), m.subject), m.notebook);
                    l.detail = digest_of(store, &m.pointer)
                        .filter(|d| *d != m.subject && !d.ends_with(&m.subject))
                        .unwrap_or_default();
                    l.when = when(m.at, &here, en);
                    l.attention = true;
                    l.pointer.clone_from(&m.pointer);
                    l.open = open_of(store, &m.pointer);
                    if !(input.public && m.notebook == Notebook::Perso) {
                        facts_lines.push(format!("[à répondre] {} — {}", l.title, l.detail));
                    }
                    l
                })
                .collect();
            sections.push(Section {
                key: "answer".to_owned(),
                label: w(en, "À RÉPONDRE", "TO ANSWER").to_owned(),
                note: match (en, waiting.len()) {
                    (false, 1) => "1 mail".to_owned(),
                    (false, n) => format!("{n} mails"),
                    (true, 1) => "1 mail".to_owned(),
                    (true, n) => format!("{n} mails"),
                },
                lines,
                proposal: None,
            });
        }
        // LE RESTE: counted here; her model words it.
        let yesterday_start = bounds(today.yesterday().unwrap_or(today), zone).0;
        let others = day::mails(&store.conn, yesterday_start, input.now)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| !m.sent && !waiting.iter().any(|w| w.pointer == m.pointer))
            .count();
        let mut rest = Vec::new();
        if others > 0 {
            rest.push(line(
                match (en, others) {
                    (false, 1) => "1 autre mail".to_owned(),
                    (false, n) => format!("{n} autres mails"),
                    (true, 1) => "1 other mail".to_owned(),
                    (true, n) => format!("{n} other mails"),
                },
                Notebook::Pro,
            ));
        }
        if let Some(holiday) = today
            .tomorrow()
            .ok()
            .and_then(|d| facts::holidays::holiday(d, Region::default()))
        {
            rest.push(line(
                if en {
                    format!("Tomorrow is a public holiday ({})", holiday.name)
                } else {
                    format!("Demain est férié ({})", holiday.name)
                },
                Notebook::Pro,
            ));
        }
        for l in &rest {
            facts_lines.push(format!("[le reste] {}", l.title));
        }
        if !rest.is_empty() {
            sections.push(Section {
                key: "rest".to_owned(),
                label: w(en, "LE RESTE", "THE REST").to_owned(),
                note: String::new(),
                lines: rest,
                proposal: None,
            });
        }
        // The plans.
        for (key, label, day, done) in [
            (
                "yesterday",
                w(en, "HIER, TU VOULAIS", "YESTERDAY, YOU MEANT TO"),
                today.yesterday().unwrap_or(today),
                Some(false),
            ),
            (
                "plan",
                w(en, "AUJOURD’HUI, TU M’AS DIT", "TODAY, YOU TOLD ME"),
                today,
                Some(false),
            ),
        ] {
            let plans = day::plans(&store.conn, day).unwrap_or_default();
            if plans.is_empty() {
                continue;
            }
            sections.push(Section {
                key: key.to_owned(),
                label: label.to_owned(),
                note: String::new(),
                lines: plans
                    .into_iter()
                    .map(|p| {
                        let mut l = line(p.title, p.notebook);
                        l.detail = p.project.unwrap_or_default();
                        l.done = done;
                        l
                    })
                    .collect(),
                proposal: None,
            });
        }
    }
    if !remind_lines.is_empty() {
        sections.push(Section {
            key: "reminders".to_owned(),
            label: w(en, "À NE PAS OUBLIER", "NOT TO FORGET").to_owned(),
            note: String::new(),
            lines: remind_lines,
            proposal: None,
        });
    }
    if kind == BriefingKind::Morning {
        // EN PRIVÉ: today's perso events, perso reminders ahead,
        // birthdays in 10 or 2 days.
        let mut lines = Vec::new();
        for e in events.iter().filter(|e| e.notebook == Notebook::Perso) {
            let mut l = line(e.title.clone(), Notebook::Perso);
            l.time = hour(&e.start.to_zoned(zone.clone()), en);
            lines.push(l);
        }
        for r in reminders
            .iter()
            .filter(|r| r.notebook == Notebook::Perso && !listed.contains(&r.id))
            .filter(|r| r.due < day_end + SignedDuration::from_hours(24 * 2))
        {
            let mut l = line(r.text.clone(), Notebook::Perso);
            l.when = when(r.due, &here, en);
            lines.push(l);
        }
        lines.extend(birthdays(store, today, en));
        if !lines.is_empty() {
            sections.push(Section {
                key: "perso".to_owned(),
                label: w(en, "EN PRIVÉ", "PRIVATE").to_owned(),
                note: String::new(),
                lines,
                proposal: None,
            });
        }
    }
    let footer = if input.read.is_empty() {
        String::new()
    } else if en {
        format!("Read at {}: {}", hour(&here, en), input.read.join(", "))
    } else {
        format!("Lu à {} : {}", hour(&here, en), input.read.join(", "))
    };
    // What is in it, in a line (#758), and its free slot named.
    let (mut summary, mut peek) = (Vec::new(), Vec::new());
    let (mut busy, mut slot) = (String::new(), String::new());
    if kind == BriefingKind::Morning {
        let meetings = meetings_today
            .iter()
            .filter(|m| m.notebook == Notebook::Pro)
            .count();
        if meetings > 0 {
            let said = match (en, meetings) {
                (false, 1) => "1 réunion".to_owned(),
                (false, n) => format!("{n} réunions"),
                (true, 1) => "1 meeting".to_owned(),
                (true, n) => format!("{n} meetings"),
            };
            summary.push(said.clone());
            peek.push(said);
        }
        let answer = sections.iter().find(|s| s.key == "answer").map_or(0, |s| {
            s.lines
                .iter()
                .filter(|l| l.notebook == Notebook::Pro)
                .count()
        });
        if answer > 0 {
            let said = match (en, answer) {
                (false, 1) => "1 mail attend ta réponse".to_owned(),
                (false, n) => format!("{n} mails attendent ta réponse"),
                (true, 1) => "1 mail waits for your answer".to_owned(),
                (true, n) => format!("{n} mails wait for your answer"),
            };
            summary.push(said.clone());
            peek.push(said);
        }
        if let Some((name, _, last)) = away.first() {
            let first = name.split_whitespace().next().unwrap_or(name);
            summary.push(match (en, last) {
                (false, Some(last)) if *last > today => {
                    format!("{first} en congés jusqu’au {}", last.day())
                }
                (true, Some(last)) if *last > today => {
                    format!("{first} on leave until {}", short(*last, en))
                }
                (false, _) => format!("{first} en congés"),
                (true, _) => format!("{first} on leave"),
            });
            peek.push(if en {
                format!("{first} is on leave")
            } else {
                format!("{first} est en congés")
            });
        }
        let free = free_slots(&meetings_today, today, zone);
        let full = meetings_today.len() >= 3;
        let day_word = match (full, en) {
            (true, false) => "une journée chargée ; ",
            (true, true) => "a busy day; ",
            (false, _) => "",
        };
        match free.as_slice() {
            [] if meetings_today.is_empty() => {
                busy = w(en, "une journée sans réunion", "a day without meetings").to_owned();
            }
            [] => {
                busy = w(en, "pas de créneau libre d’une heure", "no free hour").to_owned();
            }
            [(from, to)] => {
                busy = format!(
                    "{day_word}{}",
                    w(en, "ton seul créneau libre", "your only free slot")
                );
                slot = format!("{} – {}", hour(from, en), hour(to, en));
            }
            more => {
                if let Some((from, to)) = more
                    .iter()
                    .max_by_key(|(a, b)| b.timestamp().duration_since(a.timestamp()))
                {
                    busy = format!(
                        "{day_word}{}",
                        w(en, "ton plus long créneau libre", "your longest free slot")
                    );
                    slot = format!("{} – {}", hour(from, en), hour(to, en));
                }
            }
        }
    }
    let (title, hello, sentence_date) = match kind {
        BriefingKind::Morning => (
            if en {
                format!("Your morning briefing · {}", hour(&here, en))
            } else {
                format!("Ton point du matin · {}", hour(&here, en))
            },
            match first_name(store) {
                Some(name) => format!("{} {name}", w(en, "Bonjour", "Good morning")),
                None => w(en, "Bonjour", "Good morning").to_owned(),
            },
            date(today, en),
        ),
        BriefingKind::Absence => (
            match input.away {
                Some((from, to)) => format!(
                    "{} · {}",
                    w(en, "Pendant ton absence", "While you were away"),
                    span_words(from, to, &here, en)
                ),
                None => w(en, "Pendant ton absence", "While you were away").to_owned(),
            },
            String::new(),
            String::new(),
        ),
    };
    Built {
        briefing: Briefing {
            kind,
            title,
            hello,
            date: sentence_date,
            sentence: String::new(),
            busy,
            slot,
            summary: summary.join(" · "),
            sections,
            footer,
        },
        moves,
        // Titles, senders and names are their sources' words (#768): each
        // fact stays its line, and writes no tag.
        facts: facts_lines
            .iter()
            .map(|l| crate::inert::line(l))
            .collect::<Vec<_>>()
            .join("\n"),
        listed,
        arrived,
        peek: peek.join(", "),
    }
}

/// "ven. 18 h → lun. 8 h 40".
fn span_words(from: Timestamp, to: Timestamp, now: &Zoned, en: bool) -> String {
    let zone = now.time_zone().clone();
    let a = from.to_zoned(zone.clone());
    let b = to.to_zoned(zone);
    let side = |z: &Zoned| {
        if z.date() == now.date() {
            hour(z, en)
        } else {
            format!("{} {}", short(z.date(), en), hour(z, en))
        }
    };
    format!("{} → {}", side(&a), side(&b))
}

/// Birthdays 10 days and 2 days ahead, from the episodes her contacts'
/// source made ("anniversaire de Claire : 12/11").
fn birthdays(store: &Store, today: Date, en: bool) -> Vec<Line> {
    let mut statement = match store
        .conn
        .prepare("SELECT DISTINCT digest FROM episodes WHERE kind = 'birthday'")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let digests: Vec<String> = statement
        .query_map([], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(std::result::Result::ok).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for digest in digests {
        let Some(rest) = digest.strip_prefix("anniversaire de ") else {
            continue;
        };
        let Some((name, date)) = rest.rsplit_once(" : ") else {
            continue;
        };
        let Some((d, m)) = date.split_once('/') else {
            continue;
        };
        let (Ok(d), Ok(m)) = (d.trim().parse::<i8>(), m.trim().parse::<i8>()) else {
            continue;
        };
        for ahead in [10_i64, 2] {
            let Ok(day) = today.checked_add(ahead.days()) else {
                continue;
            };
            if day.day() == d && day.month() == m {
                let mut l = line(
                    if en {
                        format!("{name}’s birthday")
                    } else {
                        format!("L’anniversaire de {name}")
                    },
                    Notebook::Perso,
                );
                l.when = if en {
                    format!("in {ahead} days")
                } else {
                    format!("dans {ahead} jours")
                };
                out.push(l);
            }
        }
    }
    out
}

/// A reminder's due time and until when it can be acted on, from what a
/// commitment says ("avant jeudi", "demain", "avant 14 h"), in code:
/// the morning of its day, until its end (before a day: the day before).
pub fn due_of(text: &str, now: Timestamp, zone: &TimeZone) -> Option<(Timestamp, Timestamp)> {
    use hocus_sources::facts::dates::Resolved;
    let here = now.to_zoned(zone.clone());
    let today = here.date();
    let said = facts::dates::resolve(text, today);
    let hour_said = hour_in(text);
    let day = said.into_iter().find_map(|s| match s.resolved {
        Resolved::Day(d) | Resolved::By(d) => Some(d),
        Resolved::Before(d) => d.yesterday().ok(),
        Resolved::Days { first, .. } => Some(first),
        Resolved::Ambiguous(ds) => ds.first().copied(),
    });
    let day = match (day, hour_said) {
        (Some(d), _) => d,
        (None, Some(_)) => today,
        (None, None) => return None,
    };
    let at = |h: i8, m: i8| {
        day.at(h, m, 0, 0)
            .to_zoned(zone.clone())
            .ok()
            .map(|z| z.timestamp())
    };
    match hour_said {
        Some((h, m)) => {
            let until = at(h, m)?;
            let due = until
                .checked_sub(SignedDuration::from_hours(2))
                .ok()
                .filter(|d| *d > now)
                .unwrap_or(now.max(until.checked_sub(SignedDuration::from_mins(30)).ok()?));
            Some((due, until))
        }
        None => Some((at(9, 0)?, at(18, 0)?)),
    }
}

/// "14 h", "14h30", "à 9 h 15": an hour said.
fn hour_in(text: &str) -> Option<(i8, i8)> {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_digit()) {
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        let hours: String = chars[i..j].iter().collect();
        let mut k = j;
        while k < chars.len() && chars[k] == ' ' {
            k += 1;
        }
        if k < chars.len() && (chars[k] == 'h' || chars[k] == 'H') {
            let after = chars.get(k + 1).copied();
            if after.is_some_and(char::is_alphabetic) {
                continue;
            }
            let mut m = k + 1;
            while m < chars.len() && chars[m] == ' ' {
                m += 1;
            }
            let mut n = m;
            while n < chars.len() && chars[n].is_ascii_digit() {
                n += 1;
            }
            let minutes: String = chars[m..n].iter().collect();
            let h: i8 = hours.parse().ok()?;
            let mi: i8 = if minutes.is_empty() {
                0
            } else {
                minutes.parse().ok()?
            };
            if (0..24).contains(&h) && (0..60).contains(&mi) {
                return Some((h, mi));
            }
        }
    }
    None
}

/// What the morning's proposal card says of a move.
pub fn move_words(m: &Move, zone: &TimeZone, en: bool) -> (String, String, String) {
    let start = m.meeting.start.to_zoned(zone.clone());
    let label = format!("{} · {}", m.meeting.title, hour(&start, en));
    let slot = m.slot.map(|s| {
        let z = s.start.to_zoned(zone.clone());
        if en {
            format!("{} {}", date(z.date(), en), hour(&z, en))
        } else {
            format!("{} {}", facts::french_date(z.date()), hour(&z, en))
        }
    });
    let organiser = m.organiser.clone().unwrap_or_default();
    let presents = m
        .meeting
        .presenter
        .as_deref()
        .is_some_and(|p| folded(p) == folded(&m.absent));
    let text = match (en, slot, presents) {
        (false, Some(slot), true) => format!(
            "{} présente «\u{a0}{}\u{a0}» de {}. Je propose de la déplacer à **{slot}**\u{a0}: les {} invités sont libres.",
            m.absent,
            m.meeting.title,
            hour(&start, en),
            m.invitees
        ),
        (false, Some(slot), false) => format!(
            "{} est invitée à «\u{a0}{}\u{a0}» de {}. Je propose de la déplacer à **{slot}**\u{a0}: les {} invités sont libres.",
            m.absent,
            m.meeting.title,
            hour(&start, en),
            m.invitees
        ),
        (false, None, _) => format!(
            "{} est en congés pour «\u{a0}{}\u{a0}» de {}. Je n’ai pas trouvé de créneau libre pour tous dans les deux semaines.",
            m.absent,
            m.meeting.title,
            hour(&start, en)
        ),
        (true, Some(slot), _) => format!(
            "{} is away for “{}” at {}. I suggest moving it to **{slot}**: all {} invitees are free.",
            m.absent,
            m.meeting.title,
            hour(&start, en),
            m.invitees
        ),
        (true, None, _) => format!(
            "{} is away for “{}” at {}. I found no slot free for everyone within two weeks.",
            m.absent,
            m.meeting.title,
            hour(&start, en)
        ),
    };
    (label, text, organiser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_said_are_read() {
        assert_eq!(hour_in("avant 14 h"), Some((14, 0)));
        assert_eq!(hour_in("à 9h30 au plus tard"), Some((9, 30)));
        assert_eq!(hour_in("le devis avant jeudi"), None);
        assert_eq!(hour_in("12 heures"), None);
    }

    #[test]
    fn robots_are_not_people() {
        assert!(person("Léa Martin <lea@atelier-lune.fr>"));
        assert!(!person("GitLab <noreply@gitlab.com>"));
        assert!(!person("Scaleway <newsletter@scaleway.com>"));
        assert_eq!(who("Léa Martin <lea@atelier-lune.fr>"), "Léa Martin");
    }

    #[test]
    fn a_leave_names_who_is_away() {
        let event = Event {
            pointer: "c:1".to_owned(),
            source: None,
            start: Timestamp::UNIX_EPOCH,
            end: Timestamp::UNIX_EPOCH,
            title: "Congés Sophie Marchand".to_owned(),
            calendar: "Congés".to_owned(),
            attendees: Vec::new(),
            organiser: None,
            presenter: None,
            busy: false,
            notebook: Notebook::Pro,
            mine: false,
            declined: false,
        };
        assert_eq!(absent_of(&event), "Sophie Marchand");
    }
}
