//! Context assembly (§7.3): each call gets a budget of tokens, never the
//! history. Four parts, each capped, under 25k tokens in all (AC31):
//!
//! | Part | Holds | Cap | Cache |
//! |---|---|---:|---|
//! | frozen | her persona, the tools, the core card (the user, taught rules, the roster, the close people, the projects) | 8k | an hour |
//! | the day | the date and the moment, computed facts, today's and tomorrow's events, yesterday's summary, what arrived today | 5k | five minutes |
//! | retrieved | what hybrid retrieval found for the question ([`crate::retrieve`]), the items' texts that may leave | 8k | none |
//! | the conversation | the latest turns | 4k | — |
//!
//! What does not fit is dropped, lowest rank first; the model can fetch
//! it with `graph` and `search`.
//!
//! **What leaves** (§14.1): an item's text goes only once both secrets'
//! passes have run (its [`Held`] checked); before that, its metadata and
//! "peut contenir un secret, pas encore vérifié" (fail closed). A source
//! whose items leave only as local summaries gives its triage's summary,
//! never its text. Content stays in `<source>` blocks, out of the system.

use std::collections::{BTreeMap, VecDeque};

use hocus_secretary_link::Notebook;
use hocus_sources::{Content, Outbound, Redacted, facts};
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};

use crate::{
    graph::{NodeKind, Target},
    model::{Message, Part, Request, Role},
    retrieve::{self, day_of},
    store::{Result, Store},
    tools,
};

pub const FROZEN: &str = "frozen";
pub const DAY: &str = "day";
pub const RETRIEVED: &str = "retrieved";

/// A call's whole budget, and each part's (§7.3).
pub const CAP: usize = 25_000;
pub const FROZEN_CAP: usize = 8_000;
pub const DAY_CAP: usize = 5_000;
pub const RETRIEVED_CAP: usize = 8_000;
pub const CONVERSATION_CAP: usize = 4_000;
/// One item's text at most, in tokens.
const ITEM_CAP: usize = 1_500;

/// Tokens a text costs, counted high: a token is about four characters
/// of French or English, and three keeps a margin.
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(3)
}

/// Her language: the desktop's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    French,
    English,
}

impl Language {
    /// From `LC_ALL`, `LC_MESSAGES` or `LANG`: French unless another is
    /// set.
    pub fn of_env() -> Self {
        let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        if lang.is_empty() || lang.starts_with("fr") || lang == "C" || lang.starts_with("C.") {
            Self::French
        } else {
            Self::English
        }
    }
}

/// What the screen shows now (§7.4): in a public moment, perso stays
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Moment {
    #[default]
    Quiet,
    Meeting,
    Sharing,
    Camera,
    Locked,
}

impl Moment {
    fn words(self) -> &'static str {
        match self {
            Self::Quiet => "au calme",
            Self::Meeting => "en réunion",
            Self::Sharing => "partage d'écran",
            Self::Camera => "caméra allumée",
            Self::Locked => "écran verrouillé",
        }
    }

    pub fn public(self) -> bool {
        matches!(self, Self::Meeting | Self::Sharing | Self::Camera)
    }
}

/// What a call may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scope {
    /// One notebook only (pro for work output, §7.4); none: both.
    pub notebook: Option<Notebook>,
    pub moment: Moment,
    pub language: Language,
}

impl Scope {
    fn notebook(&self) -> Option<Notebook> {
        if self.moment.public() {
            Some(Notebook::Pro)
        } else {
            self.notebook
        }
    }
}

/// An item a source gave with its text, held in her process only (never
/// written), between redaction and a model.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub redacted: Redacted,
    pub episode: i64,
    pub summaries_only: bool,
    /// The local triage's summary, once checked (masked).
    pub summary: Option<String>,
    /// The triage took it for instructions to an assistant, or phishing.
    pub suspicious: bool,
}

/// The items held, by pointer, the oldest let go past a bound.
#[derive(Debug, Default)]
pub struct Inbox {
    held: BTreeMap<String, Held>,
    order: VecDeque<String>,
}

impl Inbox {
    pub const MOST: usize = 1_000;

    pub fn hold(&mut self, pointer: String, held: Held) {
        if self.held.insert(pointer.clone(), held).is_none() {
            self.order.push_back(pointer);
        }
        while self.order.len() > Self::MOST {
            if let Some(old) = self.order.pop_front() {
                self.held.remove(&old);
            }
        }
    }

    pub fn get(&self, pointer: &str) -> Option<&Held> {
        self.held.get(pointer)
    }

    pub fn get_mut(&mut self, pointer: &str) -> Option<&mut Held> {
        self.held.get_mut(pointer)
    }

    /// What came from `source` goes (a revocation).
    pub fn forget_source(&mut self, source: &str) {
        let prefix = format!("{source}:");
        self.held.retain(|p, _| !p.starts_with(&prefix));
        self.order.retain(|p| !p.starts_with(&prefix));
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

fn cut(text: &str, most: usize) -> String {
    if tokens(text) <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most * 3).collect();
    out.push_str(" […]");
    out
}

/// A held item as it may leave now.
pub fn outbound(pointer: &str, held: &Held) -> String {
    match held.redacted.outbound() {
        Outbound::MetadataOnly {
            from,
            subject,
            at,
            note,
            ..
        } => format!(
            "<source id=\"{pointer}\" checked=\"no\">\nDe : {}\nObjet : {subject}\nDate : {}\n({note})\n</source>",
            from.unwrap_or("—"),
            day_of(at)
        ),
        Outbound::Checked(_) if held.summaries_only => format!(
            "<source id=\"{pointer}\" checked=\"yes\" text=\"résumé local seulement\">\n{}\n</source>",
            held.summary.as_deref().unwrap_or("(pas de résumé)")
        ),
        Outbound::Checked(item) => {
            let text = match &item.content {
                Content::Mail(mail) => format!(
                    "De : {}\nÀ : {}\nObjet : {}\nDate : {}\nPièces jointes : {}\n\n{}",
                    mail.from,
                    mail.to.join(", "),
                    mail.subject,
                    day_of(item.at),
                    if mail.attachments.is_empty() {
                        "aucune".to_owned()
                    } else {
                        mail.attachments.join(", ")
                    },
                    mail.body.as_deref().unwrap_or_default()
                ),
                Content::Event(event) => format!(
                    "Événement : {}\nAgenda : {}\nLieu : {}\n\n{}",
                    event.title,
                    event.calendar_name,
                    event.place.as_deref().unwrap_or("—"),
                    event.description.as_deref().unwrap_or_default()
                ),
                Content::Birthday(b) => {
                    format!("Anniversaire de {} : {}/{}", b.name, b.day, b.month)
                }
                Content::Notification(n) => format!(
                    "Notification de {}\nTitre : {}\nDate : {}\n\n{}",
                    n.app,
                    n.title,
                    day_of(item.at),
                    n.body
                ),
            };
            format!(
                "<source id=\"{pointer}\" checked=\"yes\">\n{}\n</source>",
                cut(&text, ITEM_CAP)
            )
        }
    }
}

/// Lines added while they fit `most` tokens; how many were left out.
fn fill(out: &mut String, lines: impl IntoIterator<Item = String>, most: usize) -> usize {
    let mut left = 0;
    for line in lines {
        if left == 0 && tokens(out) + tokens(&line) < most {
            out.push_str(&line);
            out.push('\n');
        } else {
            left += 1;
        }
    }
    left
}

/// Her persona, for the user, in her language.
pub fn persona(store: &Store, language: Language) -> String {
    let user = user_name(store);
    let text = match language {
        Language::French => include_str!("persona/fr.txt"),
        Language::English => include_str!("persona/en.txt"),
    };
    text.replace("{user_name}", &user)
}

fn user_name(store: &Store) -> String {
    store
        .conn
        .query_row(
            "SELECT name FROM nodes WHERE kind = 'user' ORDER BY id LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "l'utilisateur".to_owned())
}

/// The core card (frozen): who the user is, what they taught, the people
/// and projects. Deterministic, so the cache holds it until it changes.
fn card(store: &Store, today: Date, scope: &Scope, most: usize) -> Result<(String, Vec<String>)> {
    let notebook = scope.notebook();
    let filter = notebook.map(Notebook::word);
    let mut out = String::from("<card>\n");
    let mut items = Vec::new();
    let mut lines = vec![format!("Utilisateur : {}", user_name(store))];
    let mut statement = store.conn.prepare(
        "SELECT r.id, r.sentence, n.name FROM rules r LEFT JOIN nodes n ON n.id = r.node
         WHERE r.accepted_at IS NOT NULL AND (?1 IS NULL OR r.notebook = ?1)
           AND (r.valid_to IS NULL OR r.valid_to > ?2) ORDER BY r.id",
    )?;
    let rules: Vec<(i64, String, Option<String>)> = statement
        .query_map(rusqlite::params![filter, today.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if !rules.is_empty() {
        lines.push("Règles apprises :".to_owned());
        for (id, sentence, node) in rules {
            items.push(format!("rule:{id}"));
            lines.push(match node {
                Some(node) => format!("- {sentence} ({node})"),
                None => format!("- {sentence}"),
            });
        }
    }
    for (kind, title) in [
        (NodeKind::Person, "Personnes :"),
        (NodeKind::Project, "Projets :"),
        (NodeKind::Client, "Clients :"),
        (NodeKind::Tool, "Outils :"),
    ] {
        let mut statement = store.conn.prepare(
            "SELECT id, name FROM nodes WHERE kind = ?1 AND (?2 IS NULL OR notebook = ?2)
             ORDER BY folded",
        )?;
        let nodes: Vec<(i64, String)> = statement
            .query_map(rusqlite::params![kind.word(), filter], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        if nodes.is_empty() {
            continue;
        }
        lines.push(title.to_owned());
        for (id, name) in nodes {
            items.push(format!("node:{id}"));
            let mut facts = Vec::new();
            for edge in store.edges_of(id, today, notebook)?.iter().take(6) {
                if edge.src != id {
                    continue;
                }
                let target = match &edge.target {
                    Target::Node(dst) => store.node(*dst).map(|n| n.name).unwrap_or_default(),
                    Target::Value(v) => v.clone(),
                };
                facts.push(format!("{} {target}", edge.kind));
            }
            let mut aliases = store.conn.prepare(
                "SELECT alias FROM aliases WHERE node = ?1 AND accepted_at IS NOT NULL
                   AND folded != (SELECT folded FROM nodes WHERE id = ?1) ORDER BY id",
            )?;
            let aliases: Vec<String> = aliases
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut line = format!("- {name}");
            if !facts.is_empty() {
                line.push_str(" — ");
                line.push_str(&facts.join(" ; "));
            }
            if !aliases.is_empty() {
                line.push_str(" (aussi : ");
                line.push_str(&aliases.join(", "));
                line.push(')');
            }
            lines.push(line);
        }
    }
    let left = fill(&mut out, lines, most.saturating_sub(4));
    if left > 0 {
        out.push_str(&format!("… {left} lignes de plus : `graph` et `search`\n"));
    }
    out.push_str("</card>");
    Ok((out, items))
}

/// The day's part.
fn day(
    store: &Store,
    inbox: &Inbox,
    now: Timestamp,
    scope: &Scope,
    most: usize,
) -> Result<(String, Vec<String>)> {
    let zone = TimeZone::system();
    let here = now.to_zoned(zone.clone());
    let today = here.date();
    let notebook = scope.notebook();
    let mut items = Vec::new();
    let visible = match notebook {
        Some(Notebook::Pro) => "pro seulement",
        Some(Notebook::Perso) => "perso seulement",
        None => "pro et perso",
    };
    let mut lines = vec![
        "<today>".to_owned(),
        format!(
            "Aujourd'hui : {} {}, {:02}:{:02} ({}). En ce moment : {}. Carnets visibles : {visible}.",
            facts::french_date(today),
            today.year(),
            here.hour(),
            here.minute(),
            zone.iana_name().unwrap_or("heure locale"),
            scope.moment.words()
        ),
    ];
    for date in [today, today.tomorrow().unwrap_or(today)] {
        let facts = facts::DayFacts {
            date,
            holiday: facts::holidays::holiday(date, facts::Region::default()),
            school_holiday: None,
            birthdays: Vec::new(),
            free: Vec::new(),
        };
        lines.extend(facts.lines(&zone, scope.moment.public()));
    }
    let start = today.to_zoned(zone.clone()).map_or(now, |z| z.timestamp());
    let after = today
        .checked_add(2.days())
        .ok()
        .and_then(|d| d.to_zoned(zone.clone()).ok())
        .map_or(now, |z| z.timestamp());
    let events: Vec<_> = retrieve::episodes_between(store, (start, after), notebook, 60)?
        .into_iter()
        .filter(|e| e.kind == "event")
        .collect();
    if !events.is_empty() {
        lines.push("Agenda (aujourd'hui et demain) :".to_owned());
        lines.push("<source kind=\"calendar\">".to_owned());
        for e in events {
            items.push(format!("episode:{}", e.id));
            lines.push(retrieve::episode_line(&e));
        }
        lines.push("</source>".to_owned());
    }
    let yesterday = today.yesterday().unwrap_or(today).to_string();
    let summary: Option<(i64, String)> = store
        .conn
        .query_row(
            "SELECT id, text FROM summaries WHERE period = 'daily' AND start = ?1 AND stale = 0
               AND (?2 IS NULL OR notebook = ?2) ORDER BY id DESC LIMIT 1",
            rusqlite::params![yesterday, notebook.map(Notebook::word)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if let Some((id, text)) = summary {
        items.push(format!("summary:{id}"));
        lines.push(format!("Hier : {text}"));
    }
    let arrived: Vec<_> = retrieve::episodes_between(store, (start, now), notebook, 40)?
        .into_iter()
        .filter(|e| e.kind != "event" && e.kind != "catch-up")
        .collect();
    if !arrived.is_empty() {
        lines.push("Arrivé aujourd'hui :".to_owned());
        lines.push("<source kind=\"digests\">".to_owned());
        for e in arrived {
            items.push(format!("episode:{}", e.id));
            let mut line = retrieve::episode_line(&e);
            if inbox
                .get(&e.pointer)
                .is_some_and(|h| matches!(h.redacted.outbound(), Outbound::MetadataOnly { .. }))
            {
                line.push_str(" — ");
                line.push_str(hocus_sources::redact::NOT_CHECKED);
            }
            lines.push(line);
        }
        lines.push("</source>".to_owned());
    }
    let mut out = String::new();
    let close = lines.pop_if(|l| l == "</source>");
    let left = fill(&mut out, lines, most.saturating_sub(10));
    if let Some(close) = close {
        out.push_str(&close);
        out.push('\n');
    }
    if left > 0 {
        out.push_str(&format!("… {left} de plus : `search`\n"));
    }
    out.push_str("</today>");
    Ok((out, items))
}

/// The retrieved part, for `question`.
fn retrieved(
    store: &Store,
    inbox: &Inbox,
    question: &str,
    today: Date,
    scope: &Scope,
    most: usize,
) -> Result<(String, Vec<String>)> {
    let found = retrieve::retrieve(store, question, today, scope.notebook())?;
    let mut out = String::from("<memory");
    let mut items = Vec::new();
    if let Some(as_of) = &found.as_of {
        out.push_str(&format!(" as_of=\"{}\">\n", as_of.day));
        out.push_str(&format!(
            "[fait calculé] La question porte sur le {} ({}) : le graphe ci-dessous est lu à cette date.\n",
            as_of.day, as_of.why
        ));
        if let Some(edge) = as_of.edge {
            items.push(format!("edge:{edge}"));
        }
    } else {
        out.push_str(">\n");
    }
    if !found.linked.is_empty() {
        let names: Vec<String> = found
            .linked
            .iter()
            .filter_map(|l| store.node(l.node).ok())
            .map(|n| format!("{} ({})", n.name, n.kind.word()))
            .collect();
        out.push_str(&format!("Liés à la question : {}\n", names.join(", ")));
    }
    let mut sources = String::new();
    let mut left = 0;
    for ranked in &found.lines {
        let held = ranked
            .pointer
            .as_deref()
            .and_then(|p| inbox.get(p).map(|h| (p, h)));
        let line = match held {
            // A source item held with its text: as it may leave now.
            Some((pointer, held)) => outbound(pointer, held),
            None => ranked.line.clone(),
        };
        if tokens(&out) + tokens(&sources) + tokens(&line) + 20 > most {
            left += 1;
            continue;
        }
        items.push(ranked.item.clone());
        if let Some((pointer, _)) = held {
            items.push(pointer.to_owned());
            sources.push_str(&line);
            sources.push('\n');
        } else if ranked.pointer.is_some() {
            sources.push_str(&format!("<source kind=\"digest\">{line}</source>\n"));
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if left > 0 {
        out.push_str(&format!("… {left} de plus : `graph` et `search`\n"));
    }
    out.push_str("</memory>\n");
    out.push_str(&sources);
    Ok((out, items))
}

/// The latest turns that fit; the last one always, cut if it must be.
fn conversation(messages: &[Message], most: usize) -> Vec<Message> {
    let mut kept = Vec::new();
    let mut used = 0;
    for (i, m) in messages.iter().enumerate().rev() {
        let cost = tokens(&m.text)
            + m.tool_results
                .iter()
                .map(|r| tokens(&r.text))
                .sum::<usize>()
            + m.native.as_ref().map_or(0, |n| tokens(&n.to_string()));
        if i + 1 == messages.len() {
            let mut last = m.clone();
            last.text = cut(&m.text, most);
            used += tokens(&last.text);
            kept.push(last);
            continue;
        }
        if used + cost > most {
            break;
        }
        used += cost;
        kept.push(m.clone());
    }
    kept.reverse();
    // A turn that answers tool calls cannot open the conversation.
    while kept
        .first()
        .is_some_and(|m| m.from == "assistant" || !m.tool_results.is_empty())
    {
        kept.remove(0);
    }
    kept
}

/// A call's request for `role`, the conversation `messages` (its last
/// the user's question), within the budget.
pub fn assemble(
    store: &Store,
    inbox: &Inbox,
    role: Role,
    messages: &[Message],
    scope: &Scope,
    now: Timestamp,
) -> Result<Request> {
    let today = day_of(now);
    let system = persona(store, scope.language);
    let tools: Vec<String> = match role {
        Role::Answer | Role::Briefing => tools::READ.iter().map(|t| (*t).to_owned()).collect(),
        _ => Vec::new(),
    };
    let tool_cost: usize = tools
        .iter()
        .filter_map(|t| tools::spec(t))
        .map(|s| tokens(s.description) + tokens(&s.schema.to_string()))
        .sum();
    let card_most = FROZEN_CAP.saturating_sub(tokens(&system) + tool_cost);
    let (card, card_items) = card(store, today, scope, card_most)?;
    let (day, day_items) = day(store, inbox, now, scope, DAY_CAP)?;
    let question = messages
        .iter()
        .rev()
        .find(|m| m.from == "user" && !m.text.is_empty())
        .map(|m| m.text.clone())
        .unwrap_or_default();
    let (found, found_items) = retrieved(store, inbox, &question, today, scope, RETRIEVED_CAP)?;
    Ok(Request {
        role,
        system,
        context: vec![
            Part {
                label: FROZEN.to_owned(),
                text: card,
                items: card_items,
            },
            Part {
                label: DAY.to_owned(),
                text: day,
                items: day_items,
            },
            Part {
                label: RETRIEVED.to_owned(),
                text: found,
                items: found_items,
            },
        ],
        messages: conversation(messages, CONVERSATION_CAP),
        tools,
    })
}

/// A request's cost in tokens, as [`tokens`] counts them.
pub fn cost(request: &Request) -> usize {
    tokens(&request.system)
        + request
            .tools
            .iter()
            .filter_map(|t| tools::spec(t))
            .map(|s| tokens(s.description) + tokens(&s.schema.to_string()))
            .sum::<usize>()
        + request
            .context
            .iter()
            .map(|p| tokens(&p.text))
            .sum::<usize>()
        + request
            .messages
            .iter()
            .map(|m| tokens(&m.text))
            .sum::<usize>()
}
