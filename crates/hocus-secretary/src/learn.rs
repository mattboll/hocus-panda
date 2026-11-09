//! What she learns (docs/design/secretary.md §9, §10.2, lot 6), in code:
//! the sentences that teach her, read without a model; the rules they
//! compile to; people arriving and leaving; what a source says of the
//! team, and the triage's candidate facts, as proposals; and what she
//! notices in her sources, as observations to ask about.
//!
//! - **Taught** ([`taught`]): "les personnes en congés sont notées dans le
//!   calendrier Congés" (a rule), "Julie est arrivée lundi" (an arrival,
//!   searched in the sources and proposed as a card), "Marc est parti
//!   vendredi" (a departure, kept at once), "Nadia dirige le site web
//!   depuis lundi" (a relation, kept at once: it closes the older lead).
//!   What the user says is kept at once (§9.3); what she finds or guesses
//!   is proposed.
//! - **Rules** are compiled in code ([`Compiled`], §9.1), never by the
//!   model, and kept in the rule's `compiled` column as JSON, for the
//!   lots that act on them (the briefing, the speak gate, the triage).
//!   The leaves' rule computes who is away ([`leaves`]): absences are never
//!   stored.
//! - **Seen in a source**: a Slack account deactivated, someone leaving, a
//!   join in #général ([`scan`]), and the triage's candidate facts
//!   ([`extract`]) are proposals: extracted edges that change people,
//!   projects or rules wait for the user's "Oui" (§7.1); an unknown
//!   person arriving is an observation, no node until accepted.
//! - **Observations** ([`observe`]): a source barely used, a calendar of
//!   birthdays never used for meetings. Each is asked once ("Oui · Non ·
//!   Plus tard") and kept in her memory's `meta` until answered: it drives
//!   nothing before.

use std::{collections::BTreeMap, sync::LazyLock};

use hocus_secretary_link::{Notebook, Provenance};
use jiff::{SignedDuration, Timestamp, ToSpan, civil::Date, tz::TimeZone};
use regex::Regex;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    fold::fold,
    graph::{NewEdge, NodeKind, Origin, Target},
    retrieve::day_of,
    store::{Item, Result, Store, Table, Who, stamp, unstamp},
    triage::Fact,
};

// --- Rules ------------------------------------------------------------------------------

/// A rule compiled in code (§9.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Compiled {
    /// `leave ← calendar X, all-day events, titles matched to the roster`.
    Leave { calendar: String },
    /// `source T: mention only when asked; not in the briefing`.
    Quiet { tool: String },
    /// `files: folder F, name pattern P`.
    Files { folder: String, pattern: String },
    /// `restraint: event "E*" ⇒ held`.
    Restraint { event: String },
    /// `triage: sender S ⇒ urgent, same day`.
    Urgent { sender: String },
    /// `calendar C: ignored`.
    Ignore { calendar: String },
    /// Her way of being (#753): `style S ⇒ every answer`, in her prompt.
    Style { style: crate::style::Style },
    /// `triage: sender S ⇒ not triaged, searched when needed` (#754): its
    /// mails wait apart with their sender, subject and date; the one the
    /// user asks about is triaged first. `sender` is an address, or a
    /// domain for all its addresses.
    Aside { sender: String },
}

impl Compiled {
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn of(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    /// The rule as she restates it, and what she does with it.
    pub fn words(&self, fr: bool) -> (String, String) {
        match (self, fr) {
            (Self::Leave { calendar }, true) => (
                format!("Les congés de l’équipe sont dans le calendrier «\u{a0}{calendar}\u{a0}»."),
                "Ce que j’en fais\u{a0}: je le lis chaque matin, je te dis qui manque parmi les gens avec qui tu travailles, et ce que ça change à tes réunions.".to_owned(),
            ),
            (Self::Leave { calendar }, false) => (
                format!("The team’s leaves are in the “{calendar}” calendar."),
                "What I do with it: I read it every morning, tell you who is away among the people you work with, and what it changes to your meetings.".to_owned(),
            ),
            (Self::Quiet { tool }, true) => (
                format!("{tool}\u{a0}: à peine utilisé."),
                "Compris\u{a0}: je n’en parle plus sauf si tu demandes, et il n’est plus dans ton point du matin.".to_owned(),
            ),
            (Self::Quiet { tool }, false) => (
                format!("{tool}: barely used."),
                "Understood: I only mention it when you ask, and it is out of your morning briefing.".to_owned(),
            ),
            (Self::Files { folder, pattern }, true) => (
                format!("Les documents vont dans {folder}, nommés {pattern}."),
                "Ce que j’en fais\u{a0}: je te propose ces noms et ce dossier, et rien n’est écrit avant ton clic.".to_owned(),
            ),
            (Self::Files { folder, pattern }, false) => (
                format!("Documents go to {folder}, named {pattern}."),
                "What I do with it: I propose these names and this folder; nothing is written before your click.".to_owned(),
            ),
            (Self::Restraint { event }, true) => (
                format!("Jamais de rappel pendant «\u{a0}{event}\u{a0}», même s’il déborde."),
                "Ce que j’en fais\u{a0}: je garde mes rappels pour après.".to_owned(),
            ),
            (Self::Restraint { event }, false) => (
                format!("Never a reminder during “{event}”, even when it runs late."),
                "What I do with it: I keep my reminders for after.".to_owned(),
            ),
            (Self::Urgent { sender }, true) => (
                format!("Les mails de {sender}\u{a0}: urgents, signalés le jour même."),
                "Ce que j’en fais\u{a0}: je te les signale dans la journée.".to_owned(),
            ),
            (Self::Urgent { sender }, false) => (
                format!("Mails from {sender}: urgent, told the same day."),
                "What I do with it: I tell you about them the same day.".to_owned(),
            ),
            (Self::Ignore { calendar }, true) => (
                format!("Le calendrier «\u{a0}{calendar}\u{a0}»\u{a0}: ignoré."),
                "Ce que j’en fais\u{a0}: je ne m’en sers plus pour tes journées.".to_owned(),
            ),
            (Self::Ignore { calendar }, false) => (
                format!("The “{calendar}” calendar: ignored."),
                "What I do with it: I no longer use it for your days.".to_owned(),
            ),
            (Self::Style { style }, true) => (
                style.said(true),
                "Ce que j’en fais\u{a0}: je m’y tiens dans chacune de mes réponses.".to_owned(),
            ),
            (Self::Style { style }, false) => (
                style.said(false),
                "What I do with it: I keep to it in every answer.".to_owned(),
            ),
            (Self::Aside { sender }, true) => (
                format!("Les mails de {sender}\u{a0}: pas triés, cherchés si besoin."),
                "Ce que j’en fais\u{a0}: je ne les lis pas sur cet ordinateur\u{a0}; je garde l’expéditeur, l’objet et la date, et je trie celui dont tu me parles.".to_owned(),
            ),
            (Self::Aside { sender }, false) => (
                format!("Mails from {sender}: not sorted, searched when needed."),
                "What I do with it: I don’t read them on this computer; I keep the sender, subject and date, and sort the one you ask me about.".to_owned(),
            ),
        }
    }

    /// The tool the rule is attached to, by name, when it is about one.
    fn tool(&self) -> Option<&str> {
        match self {
            Self::Quiet { tool } => Some(tool),
            _ => None,
        }
    }
}

/// A rule as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: i64,
    pub node: Option<i64>,
    pub sentence: String,
    pub compiled: Option<Compiled>,
    pub provenance: Provenance,
    pub pointer: String,
    pub recorded_at: Timestamp,
    pub accepted_at: Option<Timestamp>,
    pub valid_to: Option<Date>,
    pub notebook: Notebook,
}

/// Every rule, accepted or not, still valid on `today` or not.
pub fn rules(store: &Store) -> Result<Vec<Rule>> {
    let mut statement = store.conn.prepare(
        "SELECT id, node, sentence, compiled, provenance, pointer, recorded_at, accepted_at,
                valid_to, notebook FROM rules ORDER BY id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok(Rule {
            id: r.get(0)?,
            node: r.get(1)?,
            sentence: r.get(2)?,
            compiled: r
                .get::<_, Option<String>>(3)?
                .and_then(|c| Compiled::of(&c)),
            provenance: Provenance::of(&r.get::<_, String>(4)?).unwrap_or(Provenance::Inferred),
            pointer: r.get(5)?,
            recorded_at: unstamp(&r.get::<_, String>(6)?).unwrap_or_default(),
            accepted_at: r.get::<_, Option<String>>(7)?.and_then(|t| unstamp(&t)),
            valid_to: r
                .get::<_, Option<String>>(8)?
                .and_then(|d| d.parse::<Date>().ok()),
            notebook: Notebook::of(&r.get::<_, String>(9)?).unwrap_or(Notebook::Pro),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The compiled rules that count on `day`: accepted, not closed.
pub fn compiled(store: &Store, day: Date) -> Result<Vec<(i64, Compiled)>> {
    Ok(rules(store)?
        .into_iter()
        .filter(|r| r.accepted_at.is_some() && r.valid_to.is_none_or(|to| to > day))
        .filter_map(|r| r.compiled.map(|c| (r.id, c)))
        .collect())
}

/// The senders whose mails are set aside on `day` (#754), as their
/// rules say them (lowercase).
pub fn set_aside(store: &Store, day: Date) -> Result<Vec<String>> {
    Ok(compiled(store, day)?
        .into_iter()
        .filter_map(|(_, c)| match c {
            Compiled::Aside { sender } => Some(sender.to_lowercase()),
            _ => None,
        })
        .collect())
}

/// Whether a rule's `sender` (an address, or a domain) covers `address`.
pub fn covers(sender: &str, address: &str) -> bool {
    let (sender, address) = (sender.to_lowercase(), address.to_lowercase());
    if sender.contains('@') {
        return sender == address;
    }
    let sender = sender.trim_start_matches('.');
    address
        .rsplit_once('@')
        .is_some_and(|(_, domain)| domain == sender || domain.ends_with(&format!(".{sender}")))
}

/// A rule kept with its compiled form: told, it counts at once; inferred
/// (an observation), it is a proposal until accepted. Its id and the
/// edits (the last first).
pub fn add_rule(
    store: &mut Store,
    sentence: &str,
    compiled: &Compiled,
    origin: &Origin,
    notebook: Notebook,
) -> Result<(i64, Vec<i64>)> {
    let mut edits = Vec::new();
    // A rule about a tool is attached to its node, made if it is new.
    let node = match compiled.tool() {
        Some(tool) => {
            let (node, made) = tool_node(store, tool, notebook)?;
            edits.extend(made);
            Some(node)
        }
        None => None,
    };
    let kept = origin.provenance == Provenance::Told;
    let mut e = store.edit()?;
    let now = stamp(e.now);
    let accepted = kept.then(|| now.clone());
    let id = e.insert(
        Table::Rules,
        "INSERT INTO rules (node, sentence, compiled, scope, provenance, pointer, source,
             recorded_at, accepted_at, notebook)
         VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, ?7, ?8, ?9)",
        &[
            &node,
            &sentence,
            &compiled.json(),
            &origin.provenance.word(),
            &origin.pointer,
            &origin.source,
            &now,
            &accepted,
            &notebook.word(),
        ],
    )?;
    let who = if kept { Who::User } else { Who::Inference };
    let (edit, changes) = e.commit(&who, "a rule", Some(&origin.pointer), "add-rule")?;
    store.audit_memory(edit, &changes)?;
    if !kept {
        store.audit_proposal(Item::Rule(id))?;
    }
    edits.insert(0, edit);
    Ok((id, edits))
}

/// "N'est plus vrai" on a rule: valid until today (excluded).
pub fn close_rule(store: &mut Store, id: i64, until: Date) -> Result<i64> {
    let mut e = store.edit()?;
    e.update(
        Table::Rules,
        id,
        "UPDATE rules SET valid_to = ?1 WHERE id = ?2",
        &[&until.to_string(), &id],
    )?;
    let (edit, changes) = e.commit(&Who::User, "no longer true", None, "close-rule")?;
    store.audit_memory(edit, &changes)?;
    Ok(edit)
}

/// The tool node named `name`, made (by the user's word) when there is
/// none: its id, and the edit that made it.
fn tool_node(store: &mut Store, name: &str, notebook: Notebook) -> Result<(i64, Option<i64>)> {
    for id in store.find(name)? {
        if store.node(id)?.kind == NodeKind::Tool {
            return Ok((id, None));
        }
    }
    let (id, edit) = store.add_node(NodeKind::Tool, name, notebook, &Who::User, "a tool named")?;
    Ok((id, Some(edit)))
}

// --- Dates ------------------------------------------------------------------------------

/// A past day said in `text` ("lundi", "vendredi", "hier", "le 3
/// novembre"), from `today`: a weekday is the last one, today's included.
pub fn past_day(text: &str, today: Date) -> Option<Date> {
    use hocus_sources::facts::dates::{Resolved, resolve};
    let said = resolve(text, today).into_iter().next()?;
    let day = match said.resolved {
        Resolved::Day(day) | Resolved::By(day) | Resolved::Before(day) => day,
        Resolved::Days { first, .. } => first,
        Resolved::Ambiguous(days) => *days.first()?,
    };
    let weekday = fold(&text[said.start..said.end]);
    let bare = WEEKDAYS.iter().any(|w| weekday.contains(w));
    if day > today && bare {
        day.checked_sub(7.days()).ok()
    } else {
        Some(day)
    }
}

const WEEKDAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];

/// "9 nov.", "9 nov. 2026" in another year than `today`'s.
pub fn short(day: Date, fr: bool) -> String {
    const FR: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = usize::try_from(day.month() - 1).unwrap_or(0);
    if fr {
        format!("{}\u{a0}{}", day.day(), FR[month])
    } else {
        format!("{} {}", EN[month], day.day())
    }
}

/// "lun. 9 nov.".
pub fn weekday_short(day: Date, fr: bool) -> String {
    const FR: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let n = usize::try_from(day.weekday().to_monday_zero_offset()).unwrap_or(0);
    format!("{} {}", if fr { FR[n] } else { EN[n] }, short(day, fr))
}

// --- Sentences --------------------------------------------------------------------------

/// What a sentence teaches her.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taught {
    Rule(Compiled),
    /// Someone arrived on `day`: she searches her sources for them.
    Arrival {
        name: String,
        day: Date,
    },
    /// Someone left on `day`: their tenure closes, their history stays.
    Departure {
        name: String,
        day: Date,
    },
    /// `subject` `kind` `object` from `from`: kept at once, closing what
    /// it contradicts.
    Relation {
        subject: String,
        kind: &'static str,
        object: String,
        from: Date,
    },
}

/// `text` without its trailing punctuation and quotes.
fn trimmed(text: &str) -> String {
    text.trim()
        .trim_end_matches(['.', '!', '?', ';', ' '])
        .trim_matches(['«', '»', '"', '“', '”', '\u{a0}', ' '])
        .trim()
        .to_owned()
}

static LEAVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)cong[ée]s.*\b(?:calendrier|agenda|calendar)\s+(?:nommé\s+|appelé\s+|named\s+)?(?P<cal>.+)$")
        .expect("a pattern")
});
static LEAVE_EN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)leaves?\b.*\bin\s+the\s+(?P<cal>.+?)\s+calendar").expect("a pattern")
});
static QUIET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:on\s+se\s+sert|je\s+me\s+sers|on\s+utilise|j['’]utilise|on\s+n['’]utilise)\s+(?:très\s+|assez\s+|vraiment\s+)?(?:peu|presque\s+pas|à\s+peine)\s+(?:de\s+|d['’]|du\s+)?(?P<tool>.+)$",
    )
    .expect("a pattern")
});
static QUIET_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?P<tool>.+?),?\s+on\s+s['’]en\s+sert\s+(?:très\s+)?peu").expect("a pattern")
});
static QUIET_EN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^we\s+(?:barely|hardly|rarely)\s+use\s+(?P<tool>.+)$").expect("a pattern")
});
static FILES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:justificatifs|documents|factures|fichiers).*?\b(?:vont|sont\s+rangés|se\s+rangent|à\s+ranger)\s+dans\s+(?P<folder>\S+?),?\s+nommés\s+(?P<pattern>\S+)",
    )
    .expect("a pattern")
});
static RESTRAINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:jamais|pas)\s+de\s+rappels?\s+pendant\s+(?:le\s+|la\s+|les\s+|l['’])?(?P<event>[^,.]+?)(?:\s*,|\s+même|\s*$|\.)",
    )
    .expect("a pattern")
});
static URGENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^les\s+mails\s+(?:de\s+|du\s+|des\s+|d['’])(?P<sender>.+?)\s*(?:sont\s+urgents|:\s*(?:me\s+les\s+)?signal|sont\s+à\s+me\s+signaler|à\s+me\s+signaler)",
    )
    .expect("a pattern")
});
static ARRIVED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<name>\p{Lu}[\p{L}-]+(?:\s+\p{Lu}[\p{L}-]+)?)\s+(?:est\s+arrivée?|a\s+commencé|a\s+rejoint\s+l['’]équipe|nous\s+a\s+rejoints?|arrive|commence)\s*(?P<when>.*)$",
    )
    .expect("a pattern")
});
static LEFT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<name>\p{Lu}[\p{L}-]+(?:\s+\p{Lu}[\p{L}-]+)?)\s+(?:est\s+partie?|a\s+quitté\s+l['’](?:équipe|entreprise|agence)|nous\s+a\s+quittés|ne\s+fait\s+plus\s+partie\s+de\s+l['’]équipe)\s*(?P<when>.*)$",
    )
    .expect("a pattern")
});
static RELATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<s>\p{Lu}[\p{L}-]+(?:\s+\p{Lu}[\p{L}-]+)?)\s+(?i:(?P<v>dirige|mène|est\s+(?:la\s+|le\s+)?lead\s+(?:de|sur|du)|travaille\s+sur|relit\s+les\s+PR\s+(?:de|du)|relit|remplace|fait\s+partie\s+de|est\s+dans\s+l['’]équipe\s+(?:de|du)))\s+(?P<o>.+?)(?:\s+(?i:depuis|à\s+partir\s+d[eu']|dès)\s*(?P<d>.+))?$",
    )
    .expect("a pattern")
});

/// What `text` teaches her, if it is one of the sentences she reads in
/// code; else none (her model answers it).
pub fn taught(text: &str, today: Date) -> Option<Taught> {
    if text.trim().ends_with('?') {
        return None;
    }
    let text = trimmed(&text.replace('’', "'"));
    if text.is_empty() {
        return None;
    }
    if let Some(c) = LEAVE.captures(&text).or_else(|| LEAVE_EN.captures(&text)) {
        let calendar = trimmed(&c["cal"]);
        if !calendar.is_empty() && calendar.split_whitespace().count() <= 4 {
            return Some(Taught::Rule(Compiled::Leave { calendar }));
        }
    }
    for pattern in [&*QUIET, &*QUIET_AFTER, &*QUIET_EN] {
        if let Some(c) = pattern.captures(&text) {
            let tool = trimmed(&c["tool"]);
            if !tool.is_empty() && tool.split_whitespace().count() <= 3 {
                return Some(Taught::Rule(Compiled::Quiet {
                    tool: capitalised(&tool),
                }));
            }
        }
    }
    if let Some(c) = FILES.captures(&text) {
        return Some(Taught::Rule(Compiled::Files {
            folder: trimmed(&c["folder"]),
            pattern: trimmed(&c["pattern"]),
        }));
    }
    if let Some(c) = RESTRAINT.captures(&text) {
        let event = trimmed(&c["event"]);
        if !event.is_empty() {
            return Some(Taught::Rule(Compiled::Restraint { event }));
        }
    }
    if let Some(c) = URGENT.captures(&text) {
        let sender = trimmed(&c["sender"]);
        if !sender.is_empty() {
            return Some(Taught::Rule(Compiled::Urgent { sender }));
        }
    }
    if let Some(c) = ARRIVED.captures(&text) {
        let day = past_day(&c["when"], today).unwrap_or(today);
        return Some(Taught::Arrival {
            name: c["name"].to_owned(),
            day,
        });
    }
    if let Some(c) = LEFT.captures(&text) {
        let day = past_day(&c["when"], today).unwrap_or(today);
        return Some(Taught::Departure {
            name: c["name"].to_owned(),
            day,
        });
    }
    if let Some(c) = RELATION.captures(&text) {
        let verb = fold(&c["v"]);
        let kind =
            if verb.starts_with("dirige") || verb.starts_with("mene") || verb.contains("lead") {
                "leads"
            } else if verb.starts_with("travaille") {
                "works_on"
            } else if verb.starts_with("relit") {
                "reviews"
            } else if verb.starts_with("remplace") {
                "replaces"
            } else {
                "member_of"
            };
        let from = c
            .name("d")
            .and_then(|d| past_day(d.as_str(), today))
            .unwrap_or(today);
        return Some(Taught::Relation {
            subject: c["s"].to_owned(),
            kind,
            object: trimmed(&c["o"]),
            from,
        });
    }
    None
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// --- People -----------------------------------------------------------------------------

/// The person nodes `name` names: their full name, an accepted alias, or
/// their first name.
pub fn people_named(store: &Store, name: &str) -> Result<Vec<i64>> {
    let folded = fold(name);
    let mut statement = store.conn.prepare(
        "SELECT id FROM nodes WHERE kind = 'person' AND (folded = ?1 OR folded LIKE ?2)
         UNION SELECT a.node FROM aliases a JOIN nodes n ON n.id = a.node
             WHERE n.kind = 'person' AND a.folded = ?1 AND a.accepted_at IS NOT NULL
         ORDER BY 1",
    )?;
    let ids = statement.query_map(params![folded, format!("{folded} %")], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// The node the words name: by name or accepted alias, else a project or
/// a team whose name holds every one of the words ("le site web",
/// "site web", "#site-web").
pub fn node_named(store: &Store, words: &str) -> Result<Option<i64>> {
    let clean = words.trim_start_matches('#').replace('-', " ");
    if let Some(id) = store.find(&clean)?.into_iter().next() {
        return Ok(Some(id));
    }
    let stripped = clean
        .trim_start_matches("le ")
        .trim_start_matches("la ")
        .trim_start_matches("les ")
        .trim_start_matches("l'");
    if let Some(id) = store.find(stripped)?.into_iter().next() {
        return Ok(Some(id));
    }
    let wanted: Vec<String> = fold(stripped)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut statement = store.conn.prepare(
        "SELECT id, folded FROM nodes WHERE kind IN ('project', 'client', 'topic') ORDER BY id",
    )?;
    let rows: Vec<(i64, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows
        .into_iter()
        .find(|(_, folded)| {
            let have: Vec<&str> = folded.split_whitespace().collect();
            wanted.iter().all(|w| have.contains(&w.as_str()))
        })
        .map(|(id, _)| id))
}

/// Someone found in her sources for an arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub role: Option<String>,
    /// The team guessed: the project's node, its name, and the channel it
    /// was guessed from.
    pub team: Option<(i64, String, String)>,
    pub handle: Option<String>,
    /// The episode it was read in: its pointer, source, digest and day.
    pub pointer: String,
    pub source: Option<String>,
    pub quote: String,
    pub day: Date,
    pub notebook: Notebook,
}

static JOINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:a\s+rejoint|joined|vient\s+d['’]arriver|est\s+arrivée?)")
        .expect("a pattern")
});
static ROLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"«\s*(?P<a>[^»]+?)\s*»|“(?P<b>[^”]+)”|\s[·—–]\s(?P<c>[^·—–#@]+?)\s*$")
        .expect("a pattern")
});
static CHANNEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#(?P<c>[\p{L}\p{N}_-]+)").expect("a pattern"));
static HANDLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(?P<h>[\p{L}\p{N}._-]+)").expect("a pattern"));

/// What her sources say of `name` arriving around `day`: a join seen in
/// the last thirty days, the newest first.
pub fn seek_arrival(store: &Store, name: &str, day: Date) -> Result<Option<Found>> {
    let first = fold(name);
    let since = day
        .checked_sub(30.days())
        .ok()
        .and_then(|d| d.to_zoned(TimeZone::system()).ok())
        .map_or(Timestamp::UNIX_EPOCH, |z| z.timestamp());
    let mut statement = store.conn.prepare(
        "SELECT pointer, source, digest, at, notebook FROM episodes WHERE at >= ?1
         ORDER BY at DESC, id DESC",
    )?;
    let rows: Vec<(String, Option<String>, String, String, String)> = statement
        .query_map([stamp(since)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let full = |digest: &str| -> Option<String> {
        // The name as the digest writes it, from the first name on.
        let words: Vec<&str> = digest.split_whitespace().collect();
        let at = words
            .iter()
            .position(|w| fold(w.trim_matches(|c: char| !c.is_alphanumeric())) == first)?;
        let mut name = vec![words[at].trim_matches(|c: char| !c.is_alphanumeric() && c != '-')];
        if let Some(next) = words.get(at + 1)
            && next.chars().next().is_some_and(char::is_uppercase)
        {
            name.push(next.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'));
        }
        Some(name.join(" "))
    };
    let joined: Vec<&(String, Option<String>, String, String, String)> = rows
        .iter()
        .filter(|(_, _, digest, _, _)| full(digest).is_some())
        .collect();
    let Some((pointer, source, digest, at, notebook)) = joined
        .iter()
        .find(|(_, _, d, _, _)| JOINED.is_match(d))
        .or_else(|| joined.first())
    else {
        return Ok(None);
    };
    let found_name = full(digest).unwrap_or_else(|| name.to_owned());
    let role = ROLE.captures(digest).and_then(|c| {
        c.name("a")
            .or_else(|| c.name("b"))
            .or_else(|| c.name("c"))
            .map(|m| m.as_str().trim().to_owned())
    });
    // The team: a channel of a project's where the person speaks, other
    // than the general one.
    let mut team = None;
    for (_, _, d, _, _) in &joined {
        for c in CHANNEL.captures_iter(d) {
            let channel = &c["c"];
            if matches!(fold(channel).as_str(), "general" | "random" | "annonces") {
                continue;
            }
            if let Some(node) = node_named(store, channel)?
                && let Ok(n) = store.node(node)
                && matches!(
                    n.kind,
                    NodeKind::Project | NodeKind::Client | NodeKind::Topic
                )
            {
                team = Some((node, n.name, format!("#{channel}")));
                break;
            }
        }
        if team.is_some() {
            break;
        }
    }
    let handle = joined
        .iter()
        .find_map(|(_, _, d, _, _)| HANDLE.captures(d).map(|c| format!("@{}", &c["h"])));
    let day = unstamp(at).map_or(day, day_of);
    Ok(Some(Found {
        name: found_name,
        role,
        team,
        handle,
        pointer: pointer.clone(),
        source: source.clone(),
        quote: digest.clone(),
        day,
        notebook: Notebook::of(notebook).unwrap_or(Notebook::Pro),
    }))
}

/// A person added from an arrival accepted: the node, the day they
/// arrived (told), their role and team as read (extracted, accepted by
/// the click). The node's id and the edits, the last first.
pub fn add_person(
    store: &mut Store,
    found: &Found,
    told: &str,
    with_source: bool,
) -> Result<(i64, Vec<i64>)> {
    let mut edits = Vec::new();
    let (node, edit) = store.add_node(
        NodeKind::Person,
        &found.name,
        found.notebook,
        &Who::User,
        "an arrival",
    )?;
    edits.push(edit);
    let source = found.source.clone().unwrap_or_default();
    let item = found
        .pointer
        .strip_prefix(&format!("{source}:"))
        .unwrap_or(&found.pointer)
        .to_owned();
    let read = |quote: &str| {
        if with_source && !source.is_empty() {
            Origin::extracted(&source, &item, quote)
        } else {
            Origin::told(told)
        }
    };
    let mut add = |store: &mut Store, kind: &str, target: Target, origin: Origin| -> Result<()> {
        let (edge, edit) = store.add_edge(&NewEdge {
            src: node,
            kind: kind.to_owned(),
            target,
            valid_from: Some(found.day),
            valid_to: None,
            origin: origin.clone(),
            notebook: found.notebook,
        })?;
        edits.push(edit);
        if store.edge(edge)?.is_proposal() {
            edits.push(store.accept(Item::Edge(edge))?);
        }
        Ok(())
    };
    add(
        store,
        "arrived",
        Target::Value(found.day.to_string()),
        Origin::told(told),
    )?;
    if let Some(role) = &found.role {
        add(
            store,
            "role",
            Target::Value(role.clone()),
            read(&found.quote),
        )?;
    }
    if let Some((team, _, _)) = &found.team {
        add(store, "member_of", Target::Node(*team), read(&found.quote))?;
    }
    if let Some(handle) = &found.handle {
        let origin = read(&found.quote);
        let (alias, edit) = store.add_alias(node, handle, &origin, found.notebook)?;
        edits.push(edit);
        if origin.provenance != Provenance::Told {
            edits.push(store.accept(Item::Alias(alias))?);
        }
    }
    edits.reverse();
    Ok((node, edits))
}

/// The kinds of edge that make someone part of the team now: a departure
/// closes them.
const TENURE: [&str; 8] = [
    "works_on",
    "member_of",
    "leads",
    "reviews",
    "role",
    "reports_to",
    "replaces",
    "client_of",
];

/// Someone left on `day`: every edge of their tenure closed at that day
/// (kept, with its history), and a `left` edge from it. The edits, the
/// last first.
pub fn depart(store: &mut Store, node: i64, day: Date, origin: &Origin) -> Result<Vec<i64>> {
    let mut edits = Vec::new();
    let (_, edit) = store.add_edge(&NewEdge {
        src: node,
        kind: "left".to_owned(),
        target: Target::Value(day.to_string()),
        valid_from: Some(day),
        valid_to: None,
        origin: origin.clone(),
        notebook: store.node(node)?.notebook,
    })?;
    edits.push(edit);
    edits.extend(close_tenure(store, node, day)?);
    edits.reverse();
    Ok(edits)
}

/// Every current edge of `node`'s tenure, closed at `day`: the edits.
pub fn close_tenure(store: &mut Store, node: i64, day: Date) -> Result<Vec<i64>> {
    let mut edits = Vec::new();
    let current: Vec<i64> = store
        .history(node)?
        .into_iter()
        .filter(|e| {
            e.src == node
                && e.superseded_at.is_none()
                && !e.is_proposal()
                && TENURE.contains(&e.kind.as_str())
                && e.valid_to.is_none_or(|to| to > day)
        })
        .map(|e| e.id)
        .collect();
    for id in current {
        let (_, edit) = store.close_edge(id, day, "left")?;
        edits.push(edit);
    }
    Ok(edits)
}

/// The day someone left, if they did.
pub fn left_on(store: &Store, node: i64) -> Result<Option<Date>> {
    Ok(store
        .history(node)?
        .into_iter()
        .filter(|e| e.kind == "left" && e.superseded_at.is_none() && !e.is_proposal())
        .find_map(|e| e.valid_from))
}

// --- Calendars and leaves ---------------------------------------------------------------

/// What she sees of a calendar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Calendar {
    /// Its name as a source gives it.
    pub name: String,
    pub events: usize,
    /// The people of hers its titles name.
    pub people: Vec<String>,
    /// Its titles say only "busy": who is away cannot be read.
    pub busy_only: bool,
}

/// The calendar named `name` in her sources' events, if she has seen it.
pub fn calendar(store: &Store, name: &str) -> Result<Option<Calendar>> {
    let wanted = fold(name);
    let mut statement = store.conn.prepare(
        "SELECT calendar, digest FROM episodes WHERE kind = 'event' AND calendar IS NOT NULL",
    )?;
    let rows: Vec<(String, String)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut seen = Calendar::default();
    let roster = roster(store)?;
    let mut people = std::collections::BTreeSet::new();
    let mut busy = true;
    for (calendar, digest) in rows.iter().filter(|(c, _)| fold(c) == wanted) {
        seen.name.clone_from(calendar);
        seen.events += 1;
        if !matches!(fold(digest).as_str(), "occupe" | "busy" | "") {
            busy = false;
        }
        if let Some(name) = matched(&roster, digest) {
            people.insert(name);
        }
    }
    if seen.events == 0 {
        return Ok(None);
    }
    seen.people = people.into_iter().collect();
    seen.busy_only = busy;
    Ok(Some(seen))
}

/// The people she knows, current or not: their name, and the folded words
/// that name them (their full name, their first name, their aliases).
fn roster(store: &Store) -> Result<Vec<(String, Vec<String>)>> {
    let mut statement = store.conn.prepare(
        "SELECT n.name, n.folded, group_concat(a.folded, '|') FROM nodes n
         LEFT JOIN aliases a ON a.node = n.id AND a.accepted_at IS NOT NULL
         WHERE n.kind = 'person' GROUP BY n.id ORDER BY n.id",
    )?;
    let rows = statement.query_map([], |r| {
        let name: String = r.get(0)?;
        let folded: String = r.get(1)?;
        let aliases: Option<String> = r.get(2)?;
        let mut words = vec![folded.clone()];
        if let Some(first) = folded.split_whitespace().next() {
            words.push(first.to_owned());
        }
        words.extend(
            aliases
                .unwrap_or_default()
                .split('|')
                .filter(|a| !a.is_empty())
                .map(ToOwned::to_owned),
        );
        Ok((name, words))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The person of `roster` a title names, by a whole word.
fn matched(roster: &[(String, Vec<String>)], title: &str) -> Option<String> {
    let folded = format!(
        " {} ",
        fold(title).replace(|c: char| !c.is_alphanumeric(), " ")
    );
    roster
        .iter()
        .find(|(_, words)| words.iter().any(|w| folded.contains(&format!(" {w} "))))
        .map(|(name, _)| name.clone())
}

/// Someone away on a day, and until when (the last day).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leave {
    pub person: String,
    pub last: Option<Date>,
    pub pointer: String,
}

/// Who is away on `day`, computed from the leaves' rules' calendars (§9.1):
/// never stored.
pub fn leaves(store: &Store, day: Date, notebook: Option<Notebook>) -> Result<Vec<Leave>> {
    let calendars: Vec<String> = compiled(store, day)?
        .into_iter()
        .filter_map(|(_, c)| match c {
            Compiled::Leave { calendar } => Some(fold(&calendar)),
            _ => None,
        })
        .collect();
    if calendars.is_empty() {
        return Ok(Vec::new());
    }
    let zone = TimeZone::system();
    let start = day
        .to_zoned(zone.clone())
        .map_or(Timestamp::UNIX_EPOCH, |z| z.timestamp());
    let end = day
        .tomorrow()
        .ok()
        .and_then(|d| d.to_zoned(zone.clone()).ok())
        .map_or(start, |z| z.timestamp());
    let mut statement = store.conn.prepare(
        "SELECT calendar, digest, at, ends, pointer FROM episodes
         WHERE kind = 'event' AND calendar IS NOT NULL AND at < ?1
           AND (?2 IS NULL OR notebook = ?2)",
    )?;
    let rows: Vec<(String, String, String, Option<String>, String)> = statement
        .query_map(params![stamp(end), notebook.map(Notebook::word)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let roster = roster(store)?;
    let mut out: BTreeMap<String, Leave> = BTreeMap::new();
    for (calendar, digest, at, ends, pointer) in rows {
        if !calendars.contains(&fold(&calendar)) {
            continue;
        }
        let Some(at) = unstamp(&at) else { continue };
        let ends = ends
            .as_deref()
            .and_then(unstamp)
            .unwrap_or_else(|| at.checked_add(SignedDuration::from_hours(24)).unwrap_or(at));
        if ends <= start || at >= end {
            continue;
        }
        let Some(person) = matched(&roster, &digest) else {
            continue;
        };
        // An all-day event ends at the midnight after its last day.
        let last = day_of(
            ends.checked_sub(SignedDuration::from_secs(1))
                .unwrap_or(ends),
        );
        out.insert(
            person.clone(),
            Leave {
                person,
                last: Some(last),
                pointer,
            },
        );
    }
    Ok(out.into_values().collect())
}

/// The leaves of today and tomorrow as computed facts for her context.
pub fn leave_lines(store: &Store, today: Date, notebook: Option<Notebook>) -> Vec<String> {
    let mut lines = Vec::new();
    for (day, word) in [
        (today, "aujourd'hui"),
        (today.tomorrow().unwrap_or(today), "demain"),
    ] {
        let away = leaves(store, day, notebook).unwrap_or_default();
        if away.is_empty() {
            continue;
        }
        let names: Vec<String> = away
            .iter()
            .map(|l| match l.last {
                Some(last) if last > day => {
                    format!("{} (jusqu'au {})", l.person, short(last, true))
                }
                _ => l.person.clone(),
            })
            .collect();
        lines.push(format!(
            "[fait calculé] En congés {word} (calendrier de la règle) : {}",
            names.join(", ")
        ));
    }
    lines
}

/// The sources' names a quiet rule keeps out of the day's part: those
/// whose name, or whose name as her status line says it, is the tool's.
pub fn quiet_sources(store: &Store, today: Date) -> Vec<String> {
    let tools: Vec<String> = compiled(store, today)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, c)| match c {
            Compiled::Quiet { tool } => Some(fold(&tool)),
            _ => None,
        })
        .collect();
    if tools.is_empty() {
        return Vec::new();
    }
    let mut statement = match store
        .conn
        .prepare("SELECT DISTINCT source FROM episodes WHERE source IS NOT NULL")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let sources: Vec<String> = statement
        .query_map([], |r| r.get(0))
        .map(|rows| rows.filter_map(std::result::Result::ok).collect())
        .unwrap_or_default();
    sources
        .into_iter()
        .filter(|s| {
            let shown = fold(&shown_source(s));
            let name = fold(s);
            tools
                .iter()
                .any(|t| shown == *t || name.contains(t.as_str()))
        })
        .collect()
}

/// A source's name as the user knows it.
pub fn shown_source(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("slack") {
        "Slack".to_owned()
    } else if lower.contains("mail") {
        "Gmail".to_owned()
    } else if lower.contains("cal") || lower.contains("agenda") {
        "Agenda".to_owned()
    } else if lower.contains("board") || lower.contains("planka") {
        "Planka".to_owned()
    } else {
        name.to_owned()
    }
}

// --- What sources say of the team -------------------------------------------------------

static DEACTIVATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<name>\p{Lu}[\p{L}-]+(?:\s+\p{Lu}[\p{L}-]+)?)\s+(?:a\s+été\s+désactivée?|est\s+désactivée?|has\s+been\s+deactivated|was\s+deactivated|a\s+quitté\s+l['’](?:entreprise|agence|équipe)|ne\s+fait\s+plus\s+partie)",
    )
    .expect("a pattern")
});
static JOINED_GENERAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<name>\p{Lu}[\p{L}-]+(?:\s+\p{Lu}[\p{L}-]+)?)\s+(?:a\s+rejoint|joined)\s+(?:le\s+canal\s+)?#(?:général|general)",
    )
    .expect("a pattern")
});

/// A change of the team a source's item says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A person she knows has gone: a proposal of their departure.
    Left { node: i64, day: Date },
    /// Someone she does not know has joined.
    Joined { name: String, role: Option<String> },
}

/// Words that are never someone (#755): what a calendar or a mail calls
/// things, folded.
const NOT_PEOPLE: &[&str] = &[
    "evenement",
    "evenements",
    "reunion",
    "reunions",
    "rappel",
    "rappels",
    "invitation",
    "rendez-vous",
    "rdv",
    "agenda",
    "calendrier",
    "mail",
    "message",
    "notification",
    "tache",
    "taches",
    "projet",
    "equipe",
    "client",
    "anniversaire",
    "conge",
    "conges",
    "daily",
    "standup",
    "point",
    "demo",
    "revue",
    "sprint",
    "release",
    "livraison",
    "facture",
    "devis",
    "document",
    "fichier",
    "compte",
    "nouveau",
    "nouvelle",
    "quelqu'un",
    "personne",
    "utilisateur",
    "admin",
    "bot",
    "support",
    "contact",
    "service",
    "groupe",
    "canal",
    "general",
    "bonjour",
    "salut",
    "merci",
    "bienvenue",
    "aujourd'hui",
    "demain",
    "lundi",
    "mardi",
    "mercredi",
    "jeudi",
    "vendredi",
    "samedi",
    "dimanche",
    "event",
    "meeting",
    "reminder",
    "invite",
    "update",
    "team",
    "user",
    "someone",
    "new",
    "welcome",
    "call",
    "appel",
    "visio",
    "atelier",
    "formation",
    "seminaire",
    "dejeuner",
    "diner",
    "occupe",
    "busy",
    "absent",
    "absence",
    "ferie",
    "vacances",
    "tableau",
    "carte",
    "ticket",
];

/// Whether `name`, said to have arrived in an item of `kind` whose digest
/// is `digest`, can be a person (#755): never a common word nor a
/// calendar's title; a name in the roster's form ("Prénom Nom"), a mail's
/// sender, or a member a Slack join names (`joined`).
pub fn person_like(name: &str, kind: &str, digest: &str, joined: bool) -> bool {
    let words: Vec<&str> = name.split_whitespace().collect();
    let shaped = !words.is_empty()
        && words.len() <= 3
        && words.iter().all(|w| {
            w.chars().next().is_some_and(char::is_uppercase)
                && w.chars()
                    .all(|c| c.is_alphabetic() || c == '-' || c == '\'')
        });
    if !shaped || words.iter().any(|w| NOT_PEOPLE.contains(&fold(w).as_str())) {
        return false;
    }
    if kind == "event" {
        return false;
    }
    let sender = digest
        .split([':', '<'])
        .next()
        .map(fold)
        .unwrap_or_default();
    joined || words.len() >= 2 || (!sender.is_empty() && sender.contains(&fold(name)))
}

/// What an item's digest says of the team, if anything.
pub fn scan(store: &Store, digest: &str, at: Timestamp) -> Result<Option<Change>> {
    let digest = digest.replace('’', "'");
    if let Some(c) = DEACTIVATED.captures(&digest) {
        let people = people_named(store, &c["name"])?;
        if let [node] = people[..]
            && left_on(store, node)?.is_none()
        {
            return Ok(Some(Change::Left {
                node,
                day: day_of(at),
            }));
        }
        return Ok(None);
    }
    if let Some(c) = JOINED_GENERAL.captures(&digest)
        && people_named(store, &c["name"])?.is_empty()
        && person_like(&c["name"], "message", &digest, true)
    {
        let role = ROLE.captures(&digest).and_then(|r| {
            r.name("a")
                .or_else(|| r.name("b"))
                .or_else(|| r.name("c"))
                .map(|m| m.as_str().trim().to_owned())
        });
        return Ok(Some(Change::Joined {
            name: c["name"].to_owned(),
            role,
        }));
    }
    Ok(None)
}

/// An edge type for a triage's relation, in French or English words.
fn relation_kind(relation: &str) -> Option<&'static str> {
    let r = fold(relation);
    let has = |words: &[&str]| words.iter().any(|w| r.contains(w));
    Some(if has(&["dirige", "lead", "responsable", "mene"]) {
        "leads"
    } else if has(&["remplace", "replace"]) {
        "replaces"
    } else if has(&["relit", "review", "relecture"]) {
        "reviews"
    } else if has(&["membre", "fait partie", "equipe", "team", "member"]) {
        "member_of"
    } else if has(&["arrive", "rejoint", "join", "commence", "nouvel", "nouveau"]) {
        "arrived"
    } else if has(&["quitte", "depart", "left", "leave", "parti"])
        || r == "part"
        || r.starts_with("part ")
    {
        "left"
    } else if has(&["travaille", "works", "work on"]) {
        "works_on"
    } else if has(&["client"]) {
        "client_of"
    } else if has(&["role", "poste", "titre", "is a", "est "]) {
        "role"
    } else {
        return None;
    })
}

/// Someone a fact names that she does not know: their name, role, and
/// the quoted words.
pub type Newcomer = (String, Option<String>, String);

/// The triage's candidate facts on an episode, as proposals (§7.1): an
/// edge for a person she knows (extracted, so a change to people or
/// projects waits for the user), an arrival to ask about for someone she
/// does not. Never a confirmed edge. The edges proposed, and the arrivals.
pub fn extract(
    store: &mut Store,
    episode: i64,
    facts: &[Fact],
) -> Result<(Vec<i64>, Vec<Newcomer>)> {
    type Row = (String, Option<String>, String, String, String, String);
    let row: Option<Row> = store
        .conn
        .query_row(
            "SELECT pointer, source, notebook, at, kind, digest FROM episodes WHERE id = ?1",
            [episode],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((pointer, source, notebook, at, kind_of, digest)) = row else {
        return Ok((Vec::new(), Vec::new()));
    };
    let notebook = Notebook::of(&notebook).unwrap_or(Notebook::Pro);
    let day = unstamp(&at).map_or_else(|| store.today(), day_of);
    let source = source.unwrap_or_default();
    let item = pointer
        .strip_prefix(&format!("{source}:"))
        .unwrap_or(&pointer)
        .to_owned();
    let mut edges = Vec::new();
    let mut arrivals = Vec::new();
    for fact in facts {
        let Some(kind) = relation_kind(&fact.relation) else {
            continue;
        };
        let subject = people_named(store, &fact.subject)?
            .into_iter()
            .next()
            .or_else(|| node_named(store, &fact.subject).ok().flatten());
        let Some(subject) = subject else {
            if matches!(kind, "arrived" | "member_of" | "role")
                && person_like(&fact.subject, &kind_of, &digest, false)
            {
                arrivals.push((fact.subject.clone(), None, fact.quote.clone()));
            } else if kind == "arrived" {
                tracing::info!(
                    subject = fact.subject,
                    kind = kind_of,
                    "not someone: no arrival"
                );
            }
            continue;
        };
        let target = match kind {
            "arrived" | "left" => Target::Value(day.to_string()),
            "role" => Target::Value(fact.object.clone()),
            _ => match node_named(store, &fact.object)? {
                Some(node) => Target::Node(node),
                None => continue,
            },
        };
        let mut origin = Origin::extracted(&source, &item, fact.quote.clone());
        if source.is_empty() {
            origin.pointer.clone_from(&pointer);
        }
        let (edge, _) = store.add_edge(&NewEdge {
            src: subject,
            kind: kind.to_owned(),
            target,
            valid_from: Some(day),
            valid_to: None,
            origin,
            notebook,
        })?;
        edges.push(edge);
    }
    Ok((edges, arrivals))
}

// --- Observations -----------------------------------------------------------------------

/// What she noticed, to ask about once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Noticed {
    /// A source barely used: `count` items in `days` days.
    Quiet {
        source: String,
        tool: String,
        count: u32,
        days: u32,
    },
    /// A calendar of birthdays or holidays never used for meetings.
    Ignore { calendar: String, events: u32 },
    /// A sender whose mails look sent by a machine, `count` of them
    /// waiting for the triage (#754): set aside?
    Automated {
        sender: String,
        count: u32,
        why: String,
        notebook: Notebook,
    },
    /// Someone she does not know joined, as a source says.
    Arrival {
        name: String,
        role: Option<String>,
        pointer: String,
        source: Option<String>,
        quote: String,
        day: Date,
        notebook: Notebook,
    },
    /// A way of being her own records suggest (#753).
    Style {
        style: crate::style::Style,
        cause: crate::style::Cause,
        count: u32,
    },
}

impl Noticed {
    /// The notebook it is about.
    pub fn notebook(&self) -> Notebook {
        match self {
            Self::Arrival { notebook, .. } | Self::Automated { notebook, .. } => *notebook,
            _ => Notebook::Pro,
        }
    }

    /// What makes two the same, asked once.
    pub fn key(&self) -> String {
        match self {
            Self::Quiet { source, .. } => format!("quiet:{source}"),
            Self::Ignore { calendar, .. } => format!("ignore:{}", fold(calendar)),
            Self::Arrival { name, .. } => format!("arrival:{}", fold(name)),
            Self::Style { style, .. } => format!("style:{}", style.aspect()),
            Self::Automated { sender, .. } => format!("aside:{}", sender.to_lowercase()),
        }
    }

    /// Her question, with its evidence.
    pub fn question(&self, fr: bool) -> (String, String) {
        match (self, fr) {
            (
                Self::Style {
                    style,
                    cause,
                    count,
                },
                fr,
            ) => crate::style::question(style, *cause, *count, fr),
            (
                Self::Quiet {
                    tool, count, days, ..
                },
                true,
            ) => (
                format!(
                    "{tool}\u{a0}: {count} élément{} en {days} jours. Je n’en parle que si tu demandes\u{a0}?",
                    if *count > 1 { "s" } else { "" }
                ),
                "Tu t’en sers très peu\u{a0}; je le sortirais de ton point du matin.".to_owned(),
            ),
            (
                Self::Quiet {
                    tool, count, days, ..
                },
                false,
            ) => (
                format!(
                    "{tool}: {count} item{} in {days} days. Shall I mention it only when you ask?",
                    if *count > 1 { "s" } else { "" }
                ),
                "You barely use it; I would leave it out of your morning briefing.".to_owned(),
            ),
            (Self::Ignore { calendar, events }, true) => (
                format!(
                    "Le calendrier «\u{a0}{calendar}\u{a0}» n’a jamais servi à tes réunions. Je l’ignore\u{a0}?"
                ),
                format!(
                    "{events} événement{}, aucun avec quelqu’un.",
                    if *events > 1 { "s" } else { "" }
                ),
            ),
            (Self::Ignore { calendar, events }, false) => (
                format!(
                    "The “{calendar}” calendar never held a meeting of yours. Shall I ignore it?"
                ),
                format!(
                    "{events} event{}, none with anyone.",
                    if *events > 1 { "s" } else { "" }
                ),
            ),
            (
                Self::Automated {
                    sender, count, why, ..
                },
                true,
            ) => (
                format!(
                    "{sender}\u{a0}: {count} mail{s} attend{e} le tri, envoyé{s} par une machine. Je ne les trie pas, et je les cherche si besoin\u{a0}?",
                    s = if *count > 1 { "s" } else { "" },
                    e = if *count > 1 { "ent" } else { "" },
                ),
                format!(
                    "Vu à «\u{a0}{why}\u{a0}». Je garde l’expéditeur, l’objet et la date\u{a0}; celui dont tu me parles, je le trie d’abord."
                ),
            ),
            (
                Self::Automated {
                    sender, count, why, ..
                },
                false,
            ) => (
                format!(
                    "{sender}: {count} mail{s} waiting for the sorting, sent by a machine. Shall I leave them unsorted and search them when needed?",
                    s = if *count > 1 { "s" } else { "" },
                ),
                format!(
                    "Seen in “{why}”. I keep the sender, subject and date; the one you ask me about, I sort first."
                ),
            ),
            (
                Self::Arrival {
                    name,
                    role,
                    source,
                    day,
                    ..
                },
                true,
            ) => (
                format!("{name} est arrivé·e\u{a0}? Je l’ajoute à l’équipe\u{a0}?"),
                format!(
                    "Vu dans {}, {}{}.",
                    shown_source(source.as_deref().unwrap_or("une source")),
                    weekday_short(*day, true),
                    role.as_ref()
                        .map(|r| format!(", «\u{a0}{r}\u{a0}»"))
                        .unwrap_or_default()
                ),
            ),
            (
                Self::Arrival {
                    name,
                    role,
                    source,
                    day,
                    ..
                },
                false,
            ) => (
                format!("Has {name} joined? Shall I add them to the team?"),
                format!(
                    "Seen in {}, {}{}.",
                    shown_source(source.as_deref().unwrap_or("a source")),
                    weekday_short(*day, false),
                    role.as_ref()
                        .map(|r| format!(", “{r}”"))
                        .unwrap_or_default()
                ),
            ),
        }
    }
}

/// Where an observation stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Standing {
    Open,
    /// "Plus tard": asked again from then.
    Later {
        until: Timestamp,
    },
    Accepted,
    Declined,
}

/// An observation, as kept in her memory's `meta` until answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub id: u64,
    pub noticed: Noticed,
    pub at: Timestamp,
    pub standing: Standing,
    /// Her question about it in the thread.
    #[serde(default)]
    pub question: Option<u64>,
}

impl Seen {
    /// Waiting for the user now.
    pub fn open(&self, now: Timestamp) -> bool {
        match self.standing {
            Standing::Open => true,
            Standing::Later { until } => until <= now,
            Standing::Accepted | Standing::Declined => false,
        }
    }
}

const SEEN: &str = "observations";

pub fn load_seen(store: &Store) -> Vec<Seen> {
    store
        .meta(SEEN)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_seen(store: &Store, seen: &[Seen]) -> Result<()> {
    store.set_meta(
        SEEN,
        &serde_json::to_string(seen).unwrap_or_else(|_| "[]".to_owned()),
    )
}

/// Calendars that hold birthdays or public holidays: never meetings.
fn ignorable(calendar: &str) -> bool {
    let c = fold(calendar);
    [
        "anniversaire",
        "birthday",
        "jours feries",
        "holidays",
        "fetes",
        "vacances scolaires",
    ]
    .iter()
    .any(|w| c.contains(w))
}

/// What she notices in her sources now (§10.2): a source connected for
/// three weeks or more with three items or fewer in the last thirty
/// days; a calendar of birthdays or holidays. Those already asked are
/// left out by the caller.
pub fn observe(store: &Store, sources: &[String], now: Timestamp) -> Result<Vec<Noticed>> {
    let mut out = Vec::new();
    let month = now
        .checked_sub(SignedDuration::from_hours(24 * 30))
        .unwrap_or(now);
    let weeks = now
        .checked_sub(SignedDuration::from_hours(24 * 21))
        .unwrap_or(now);
    for source in sources {
        let (first, recent): (Option<String>, i64) = store.conn.query_row(
            "SELECT min(at), sum(at >= ?2) FROM episodes WHERE source = ?1 OR source LIKE ?3",
            params![source, stamp(month), format!("{source}:%")],
            |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
        )?;
        let Some(first) = first.as_deref().and_then(unstamp) else {
            continue;
        };
        if first <= weeks && recent <= 3 {
            out.push(Noticed::Quiet {
                source: source.clone(),
                tool: shown_source(source),
                count: u32::try_from(recent).unwrap_or(0),
                days: 30,
            });
        }
    }
    let mut statement = store.conn.prepare(
        "SELECT calendar, count(*) FROM episodes WHERE kind = 'event' AND calendar IS NOT NULL
         GROUP BY calendar ORDER BY calendar",
    )?;
    let calendars: Vec<(String, i64)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let ignored: Vec<String> = compiled(store, day_of(now))?
        .into_iter()
        .filter_map(|(_, c)| match c {
            Compiled::Ignore { calendar } => Some(fold(&calendar)),
            _ => None,
        })
        .collect();
    for (calendar, events) in calendars {
        if ignorable(&calendar) && !ignored.contains(&fold(&calendar)) {
            out.push(Noticed::Ignore {
                calendar,
                events: u32::try_from(events).unwrap_or(0),
            });
        }
    }
    Ok(out)
}

/// An observation accepted: what it becomes in her memory (a rule, a
/// person), inferred from her sources and accepted by the click. The
/// edits.
pub fn apply(store: &mut Store, seen: &Seen) -> Result<Vec<i64>> {
    let pointer = format!("seen:{}", seen.id);
    match &seen.noticed {
        Noticed::Quiet { tool, .. } => {
            let compiled = Compiled::Quiet { tool: tool.clone() };
            let (id, mut edits) = add_rule(
                store,
                &format!("{tool} : à peine utilisé"),
                &compiled,
                &Origin::inferred(pointer, 0.8),
                Notebook::Pro,
            )?;
            edits.insert(0, store.accept(Item::Rule(id))?);
            Ok(edits)
        }
        Noticed::Ignore { calendar, .. } => {
            let compiled = Compiled::Ignore {
                calendar: calendar.clone(),
            };
            let (id, mut edits) = add_rule(
                store,
                &format!("le calendrier « {calendar} » : ignoré"),
                &compiled,
                &Origin::inferred(pointer, 0.8),
                Notebook::Pro,
            )?;
            edits.insert(0, store.accept(Item::Rule(id))?);
            Ok(edits)
        }
        Noticed::Automated {
            sender, notebook, ..
        } => {
            let compiled = Compiled::Aside {
                sender: sender.to_lowercase(),
            };
            let (id, mut edits) = add_rule(
                store,
                &format!("les mails de {sender} : pas triés, cherchés si besoin"),
                &compiled,
                &Origin::inferred(pointer, 0.8),
                *notebook,
            )?;
            edits.insert(0, store.accept(Item::Rule(id))?);
            Ok(edits)
        }
        Noticed::Arrival {
            name,
            role,
            pointer,
            source,
            quote,
            day,
            notebook,
        } => {
            let found = Found {
                name: name.clone(),
                role: role.clone(),
                team: None,
                handle: None,
                pointer: pointer.clone(),
                source: source.clone(),
                quote: quote.clone(),
                day: *day,
                notebook: *notebook,
            };
            Ok(add_person(store, &found, pointer, true)?.1)
        }
        Noticed::Style { style, .. } => {
            let sentence = style.said(true);
            match crate::style::keep(
                store,
                style,
                &sentence,
                &Origin::inferred(pointer, 0.8),
                store.today(),
            )? {
                crate::style::Kept::New { edits, .. } => Ok(edits),
                crate::style::Kept::Already { .. } => Ok(Vec::new()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    /// #755: an arrival needs someone, never a common word or a
    /// calendar's title.
    #[test]
    fn common_words_and_calendar_titles_are_not_arrivals() -> anyhow::Result<()> {
        for word in ["Événement", "Réunion", "Rappel"] {
            assert!(!person_like(word, "event", word, false), "{word}");
            assert!(
                !person_like(word, "mail", &format!("{word} : demain 10 h"), false),
                "{word}"
            );
            assert!(!person_like(word, "message", "#général", true), "{word}");
        }
        assert!(!person_like(
            "Julie Berthier",
            "event",
            "Julie Berthier",
            false
        ));
        assert!(person_like(
            "Julie Berthier",
            "mail",
            "Léa : nouvelle",
            false
        ));
        assert!(person_like(
            "Julie",
            "mail",
            "Julie <julie@x> : bonjour",
            false
        ));
        assert!(!person_like("Julie", "mail", "Léa : Julie arrive", false));
        assert!(person_like("Julie", "message", "#général", true));

        let start: Timestamp = "2026-11-09T08:00:00Z".parse()?;
        let mut store = Store::in_memory(crate::clock::Clock::virtual_at(start))?;
        let (event, _) = store.add_episode(
            &crate::graph::NewEpisode {
                at: start,
                kind: "event".to_owned(),
                digest: "Événement".to_owned(),
                source: Some("fixture-calendar".to_owned()),
                pointer: "fixture-calendar:e1".to_owned(),
                notebook: Notebook::Pro,
            },
            &[],
        )?;
        let facts: Vec<Fact> = ["Événement", "Réunion", "Rappel"]
            .iter()
            .map(|w| Fact {
                subject: (*w).to_owned(),
                relation: "est arrivé".to_owned(),
                object: String::new(),
                quote: (*w).to_owned(),
            })
            .collect();
        let (_, arrivals) = extract(&mut store, event, &facts)?;
        assert!(arrivals.is_empty(), "{arrivals:?}");
        assert_eq!(
            scan(&store, "Réunion a rejoint le canal #général", start)?,
            None
        );
        Ok(())
    }

    #[test]
    fn sentences_that_teach_her_are_read_in_code() {
        let monday = date(2026, 11, 9);
        assert_eq!(
            taught(
                "les personnes en congés sont notées dans le calendrier Congés",
                monday
            ),
            Some(Taught::Rule(Compiled::Leave {
                calendar: "Congés".to_owned()
            }))
        );
        assert_eq!(
            taught("on se sert très peu de Planka", monday),
            Some(Taught::Rule(Compiled::Quiet {
                tool: "Planka".to_owned()
            }))
        );
        assert_eq!(
            taught(
                "les justificatifs de compta vont dans Compta/AAAA-MM, nommés AAAA-MM-JJ_Sujet_AM",
                monday
            ),
            Some(Taught::Rule(Compiled::Files {
                folder: "Compta/AAAA-MM".to_owned(),
                pattern: "AAAA-MM-JJ_Sujet_AM".to_owned()
            }))
        );
        assert_eq!(
            taught(
                "jamais de rappel pendant le daily, même s'il déborde",
                monday
            ),
            Some(Taught::Rule(Compiled::Restraint {
                event: "daily".to_owned()
            }))
        );
        assert_eq!(
            taught(
                "les mails du cabinet Morel sont urgents le jour même",
                monday
            ),
            Some(Taught::Rule(Compiled::Urgent {
                sender: "cabinet Morel".to_owned()
            }))
        );
        assert_eq!(
            taught("Julie est arrivée lundi", monday),
            Some(Taught::Arrival {
                name: "Julie".to_owned(),
                day: monday
            })
        );
        assert_eq!(
            taught("Marc est parti vendredi", monday),
            Some(Taught::Departure {
                name: "Marc".to_owned(),
                day: date(2026, 11, 6)
            })
        );
        assert_eq!(
            taught("Nadia dirige le site web depuis lundi", date(2026, 11, 11)),
            Some(Taught::Relation {
                subject: "Nadia".to_owned(),
                kind: "leads",
                object: "le site web".to_owned(),
                from: monday
            })
        );
        assert_eq!(taught("qui dirige le site web ?", monday), None);
        assert_eq!(taught("aujourd'hui je finis le devis", monday), None);
    }

    #[test]
    fn a_sender_or_a_domain_covers_its_addresses() {
        assert!(covers(
            "notifications@github.com",
            "Notifications@GitHub.com"
        ));
        assert!(!covers("notifications@github.com", "noreply@github.com"));
        assert!(covers("gitlab.example.org", "gitlab@gitlab.example.org"));
        assert!(covers("example.org", "bot@ci.example.org"));
        assert!(!covers("example.org", "lea@notexample.org"));
    }

    #[test]
    fn a_weekday_said_in_the_past_is_the_last_one() {
        let wednesday = date(2026, 11, 11);
        assert_eq!(past_day("lundi", wednesday), Some(date(2026, 11, 9)));
        assert_eq!(past_day("vendredi", wednesday), Some(date(2026, 11, 6)));
        assert_eq!(past_day("mercredi", wednesday), Some(wednesday));
        assert_eq!(past_day("hier", wednesday), Some(date(2026, 11, 10)));
        assert_eq!(past_day("", wednesday), None);
    }
}
