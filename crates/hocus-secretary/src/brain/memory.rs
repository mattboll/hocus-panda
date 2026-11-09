//! What she learns and what she knows, on her loop (lot 6, §8–§10):
//!
//! - the sentences that teach her ([`learn::taught`]): a rule restated,
//!   checked and kept with "Retenu · Annuler"; an arrival searched in her
//!   sources and proposed as a person card; a departure and a relation
//!   kept at once;
//! - "Ce qu'elle sait" ([`crate::knows`]): the view for the drawer's
//!   second tab, sent again while it changes, and what is done there
//!   (forget with five seconds to take it back, "N'est plus vrai", keep,
//!   accept or reject, "Oublier tout le perso", export);
//! - what her sources say of the team and the triage's candidate facts,
//!   as proposals; what she notices, as questions ("Oui · Non · Plus
//!   tard");
//! - the night's compaction ([`crate::compaction`]), on mains power or
//!   above the battery threshold, once a night;
//! - what she kept settling after ten seconds: its "Annuler" goes.
//!
//! A work draft (§7.4) is assembled from the pro notebook only.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::Duration,
};

use hocus_secretary_link::{
    Action, Body, Card, CardKind, Decision, FromSecretary, Notebook,
    knows::{Act, Do, Knowledge, Query},
};
use jiff::{SignedDuration, Timestamp, ToSpan, civil::Date, tz::TimeZone};

use super::{Brain, Event};
use crate::{
    audit,
    compaction::{self, Work},
    context::Scope,
    fold::fold,
    graph::{NewEdge, Origin, Target},
    knows,
    learn::{self, Compiled, Found, Noticed, Seen, Standing, Taught, short, weekday_short},
    model::{Reply, Role},
    retrieve::day_of,
    store::{Item as Kept, Kept as Forgotten},
    triage::Fact,
};

/// How long "Annuler" stays under what she kept (§9.3).
pub const SETTLE: Duration = Duration::from_secs(10);
/// How long a forget can be taken back (§8).
pub const FORGET_UNDO: Duration = Duration::from_secs(5);

/// What a card or a question of hers waits for.
#[derive(Debug, Clone)]
enum Waiting {
    /// "Ajouter" an arrival: who, found or not in her sources.
    Arrival { found: Found, from_source: bool },
    /// Which of these people the user meant, to mark gone on `day`.
    Which { people: Vec<i64>, day: Date },
    /// An observation's question.
    Seen(u64),
}

/// What "Annuler" in a toast takes back.
#[derive(Debug)]
enum Back {
    Forgot(Box<Forgotten>),
    Edits(Vec<i64>),
}

/// Her loop's part for what she learns and knows.
#[derive(Debug, Default)]
pub(super) struct Memory {
    /// The connections watching "Ce qu'elle sait": the request's id, its
    /// query, the view last sent.
    watch: BTreeMap<u64, (u64, Query, Option<Knowledge>)>,
    /// What toasts can take back, by token, and until when.
    backs: BTreeMap<u64, (Back, Timestamp)>,
    next_token: u64,
    waiting: HashMap<u64, Waiting>,
    /// When the night's compaction is due next.
    compaction: Option<Timestamp>,
    /// Summaries asked of her model and not back yet.
    compacting: u32,
    /// A Sunday whose weekly summary waits for its days'.
    weekly: Option<Date>,
    exports: Option<PathBuf>,
}

impl Memory {
    pub(super) fn new(exports: Option<PathBuf>) -> Self {
        Self {
            exports,
            ..Self::default()
        }
    }
}

/// `at` plus `by`.
fn after(at: Timestamp, by: Duration) -> Timestamp {
    at.checked_add(SignedDuration::try_from(by).unwrap_or(SignedDuration::ZERO))
        .unwrap_or(Timestamp::MAX)
}

/// The next time it is `hour` o'clock where the user is, after `now`.
fn next_hour(now: Timestamp, hour: i8) -> Timestamp {
    let zone = TimeZone::system();
    let here = now.to_zoned(zone.clone());
    let today = here
        .date()
        .at(hour.clamp(0, 23), 0, 0, 0)
        .to_zoned(zone.clone())
        .map_or(now, |z| z.timestamp());
    if today > now {
        today
    } else {
        here.date()
            .tomorrow()
            .ok()
            .and_then(|d| d.at(hour.clamp(0, 23), 0, 0, 0).to_zoned(zone).ok())
            .map_or(Timestamp::MAX, |z| z.timestamp())
    }
}

impl Brain {
    fn fr(&self) -> bool {
        self.talk.fr()
    }

    fn today(&self) -> Date {
        day_of(self.clock.now())
    }

    /// The thread's last item's id: the user's sentence just added.
    fn last_item(&self) -> u64 {
        self.talk.items.last().map_or(0, |i| i.id)
    }

    fn her(&mut self, id: u64, text: String, note: Option<String>) -> u64 {
        self.add_item(
            Some(id),
            Body::Her {
                text,
                note,
                assumed: false,
            },
        )
    }

    /// "Retenu : …" with "Annuler" for ten seconds, playing `edits` back.
    fn learned(&mut self, id: u64, text: String, edits: Vec<i64>) -> u64 {
        let item = self.add_item(
            Some(id),
            Body::Learned {
                text,
                undone: false,
                settled: false,
            },
        );
        self.talk.kept.insert(item, edits);
        self.talk.happy = true;
        item
    }

    // --- Sentences ----------------------------------------------------------------------

    /// A sentence that teaches her, read in code: whether it was one (and
    /// she answered it).
    pub(super) fn taught(&mut self, conn: u64, id: u64, text: &str) -> bool {
        let Some(taught) = learn::taught(text, self.today()) else {
            return false;
        };
        let pointer = format!("conversation:{}", self.last_item());
        let done = match taught {
            Taught::Rule(compiled) => self.rule_taught(id, text, &compiled, &pointer),
            Taught::Arrival { name, day } => self.arrival_said(id, &name, day),
            Taught::Departure { name, day } => self.departure_said(id, &name, day, &pointer),
            Taught::Relation {
                subject,
                kind,
                object,
                from,
            } => self.relation_said(id, &subject, kind, &object, from, &pointer),
        };
        if done {
            tracing::info!(id, "a sentence taught her");
            self.done(conn, id);
        }
        done
    }

    fn rule_taught(&mut self, id: u64, sentence: &str, compiled: &Compiled, pointer: &str) -> bool {
        let fr = self.fr();
        // She checks what it names first (§9.1).
        if let Compiled::Leave { calendar } = compiled {
            let seen = learn::calendar(&self.store, calendar).ok().flatten();
            let text = match (&seen, fr) {
                (Some(c), true) => {
                    let mut text = format!(
                        "Compris. Je l’ai trouvé\u{a0}: «\u{a0}{}\u{a0}», {} événement{}, {} personne{} de ton équipe.",
                        c.name,
                        c.events,
                        if c.events > 1 { "s" } else { "" },
                        c.people.len(),
                        if c.people.len() > 1 { "s" } else { "" }
                    );
                    if c.busy_only {
                        text.push_str(" Mais je n’y vois que «\u{a0}occupé\u{a0}»\u{a0}: je ne peux pas savoir qui est absent.");
                    }
                    text
                }
                (Some(c), false) => {
                    let mut text = format!(
                        "Got it. I found it: “{}”, {} events, {} people of your team.",
                        c.name,
                        c.events,
                        c.people.len()
                    );
                    if c.busy_only {
                        text.push_str(" But I only see “busy” there: I cannot tell who is away.");
                    }
                    text
                }
                (None, true) => format!(
                    "Je ne trouve pas encore de calendrier «\u{a0}{calendar}\u{a0}» dans tes comptes\u{a0}; je garde la règle et je la vérifierai dès qu’il apparaîtra."
                ),
                (None, false) => format!(
                    "I can’t find a “{calendar}” calendar in your accounts yet; I keep the rule and will check it as soon as it shows."
                ),
            };
            self.her(id, text, None);
        }
        let origin = Origin::told(pointer);
        let kept = learn::add_rule(
            &mut self.store,
            sentence.trim(),
            compiled,
            &origin,
            Notebook::Pro,
        );
        let (rule, edits) = match kept {
            Ok(kept) => kept,
            Err(err) => {
                tracing::warn!(%err, "could not keep a rule");
                return false;
            }
        };
        let (restated, does) = compiled.words(fr);
        self.add_item(
            Some(id),
            Body::Card {
                card: Card {
                    kind: CardKind::Rule,
                    label: self
                        .talk
                        .t("Règle · que je retiens", "Rule · I keep")
                        .to_owned(),
                    text: restated,
                    lines: vec![does],
                    actions: Vec::new(),
                    decided: None,
                    outcome: None,
                },
            },
        );
        let count = learn::compiled(&self.store, self.today())
            .map(|r| r.len())
            .unwrap_or(1);
        let text = match compiled {
            Compiled::Quiet { tool } if fr => format!("Retenu · {tool}\u{a0}: à peine utilisé"),
            Compiled::Quiet { tool } => format!("Kept · {tool}: barely used"),
            _ if fr => format!("Retenu · règle n°\u{a0}{count}"),
            _ => format!("Kept · rule no. {count}"),
        };
        self.learned(id, text, edits);
        tracing::info!(rule, "a rule taught");
        // Its effect today (§9.1).
        if let Compiled::Leave { .. } = compiled {
            let today = self.today();
            let mut parts = Vec::new();
            for (day, word) in [
                (today, self.talk.t("Aujourd’hui", "Today")),
                (
                    today.tomorrow().unwrap_or(today),
                    self.talk.t("Demain", "Tomorrow"),
                ),
            ] {
                let away = learn::leaves(&self.store, day, None).unwrap_or_default();
                if away.is_empty() {
                    continue;
                }
                let names: Vec<String> = away
                    .iter()
                    .map(|l| match l.last {
                        Some(last) if last > day && fr => {
                            format!("{} (jusqu’au {})", first_name(&l.person), last.day())
                        }
                        Some(last) if last > day => {
                            format!("{} (until {})", first_name(&l.person), short(last, false))
                        }
                        _ => first_name(&l.person).to_owned(),
                    })
                    .collect();
                let and = self.talk.t(" et ", " and ");
                parts.push(format!("{word}\u{a0}: {}.", join_and(&names, and)));
            }
            if !parts.is_empty() {
                self.her(id, parts.join(" "), None);
            }
        }
        true
    }

    fn arrival_said(&mut self, id: u64, name: &str, day: Date) -> bool {
        let fr = self.fr();
        let known = learn::people_named(&self.store, name).unwrap_or_default();
        if let Some(node) = known.first()
            && let Ok(person) = self.store.node(*node)
            && known.len() == 1
            && fold(&person.name) != fold(name)
        {
            // A full name she has already: nothing to add.
            let text = if fr {
                format!("Je connais déjà {}.", person.name)
            } else {
                format!("I already know {}.", person.name)
            };
            self.her(id, text, None);
            return true;
        }
        let found = learn::seek_arrival(&self.store, name, day).ok().flatten();
        let first = first_name(name).to_owned();
        let (found, from_source) = match found {
            Some(found) => {
                let source = found
                    .source
                    .as_deref()
                    .map_or_else(String::new, learn::shown_source);
                let text = if fr {
                    format!(
                        "Je n’avais pas de {first} dans l’équipe. Dans {source}, {}\u{a0}: «\u{a0}{}\u{a0}». C’est la bonne personne\u{a0}?",
                        weekday_short(found.day, true),
                        found.quote
                    )
                } else {
                    format!(
                        "I had no {first} in the team. In {source}, {}: “{}”. Is that the right person?",
                        weekday_short(found.day, false),
                        found.quote
                    )
                };
                self.her(id, text, None);
                (found, true)
            }
            None => {
                let text = if fr {
                    format!(
                        "Je n’ai rien trouvé sur {first} dans tes sources. Je l’ajoute quand même\u{a0}?"
                    )
                } else {
                    format!(
                        "I found nothing about {first} in your sources. Shall I add them anyway?"
                    )
                };
                self.her(id, text, None);
                (
                    Found {
                        name: name.to_owned(),
                        role: None,
                        team: None,
                        handle: None,
                        pointer: String::new(),
                        source: None,
                        quote: String::new(),
                        day,
                        notebook: Notebook::Pro,
                    },
                    false,
                )
            }
        };
        let mut lines = vec![format!(
            "{}\u{a0}: {}",
            self.talk.t("Nom", "Name"),
            found.name
        )];
        if let Some(role) = &found.role {
            lines.push(format!("{}\u{a0}: {role}", self.talk.t("Rôle", "Role")));
        }
        lines.push(format!(
            "{}\u{a0}: {} {}",
            self.talk.t("Arrivée", "Arrived"),
            weekday_short(day, fr),
            day.year()
        ));
        if let Some((_, team, channel)) = &found.team {
            lines.push(if fr {
                format!("Équipe\u{a0}: {team} (deviné\u{a0}: {channel})")
            } else {
                format!("Team: {team} (guessed: {channel})")
            });
        }
        if let Some(handle) = &found.handle {
            lines.push(format!(
                "{}\u{a0}: {handle}",
                found
                    .source
                    .as_deref()
                    .map_or("Slack".to_owned(), learn::shown_source)
            ));
        }
        let action = |label: &str, decision| Action {
            label: label.to_owned(),
            decision,
        };
        let card = self.add_item(
            Some(id),
            Body::Card {
                card: Card {
                    kind: CardKind::Person,
                    label: self
                        .talk
                        .t("Nouvelle personne · à confirmer", "New person · to confirm")
                        .to_owned(),
                    // Its fields, one a line (lines would be steps).
                    text: lines.join("\n"),
                    lines: Vec::new(),
                    actions: vec![
                        action(self.talk.t("Ajouter", "Add"), Decision::Accept),
                        action(self.talk.t("Modifier", "Edit"), Decision::Edit),
                        action(self.talk.t("Ce n’est pas ça", "Not them"), Decision::Reject),
                    ],
                    decided: None,
                    outcome: None,
                },
            },
        );
        self.audited(audit::Entry {
            kind: audit::Kind::Proposal,
            what: "person".to_owned(),
            ids: vec![format!("card:{card}")],
            ..Default::default()
        });
        self.memory
            .waiting
            .insert(card, Waiting::Arrival { found, from_source });
        true
    }

    fn departure_said(&mut self, id: u64, name: &str, day: Date, pointer: &str) -> bool {
        let people = learn::people_named(&self.store, name).unwrap_or_default();
        match people[..] {
            [] => {
                let text = if self.fr() {
                    format!("Je ne connais pas de {name}\u{a0}: rien à changer.")
                } else {
                    format!("I know no {name}: nothing to change.")
                };
                self.her(id, text, None);
            }
            [node] => self.depart(id, node, day, pointer),
            _ => {
                let mut replies: Vec<String> = people
                    .iter()
                    .take(3)
                    .filter_map(|p| self.store.node(*p).ok().map(|n| n.name))
                    .collect();
                replies.push(self.talk.t("Aucun·e", "None").to_owned());
                let text = if self.fr() {
                    format!("Quel·le {name}\u{a0}?")
                } else {
                    format!("Which {name}?")
                };
                let question = self.add_item(
                    Some(id),
                    Body::Question {
                        text,
                        note: None,
                        replies,
                        chosen: None,
                    },
                );
                self.memory.waiting.insert(
                    question,
                    Waiting::Which {
                        people: people.into_iter().take(3).collect(),
                        day,
                    },
                );
            }
        }
        true
    }

    /// `node` gone on `day`: kept at once, with "Annuler".
    fn depart(&mut self, id: u64, node: i64, day: Date, pointer: &str) {
        let fr = self.fr();
        let Ok(person) = self.store.node(node) else {
            return;
        };
        let edits = match learn::depart(&mut self.store, node, day, &Origin::told(pointer)) {
            Ok(edits) => edits,
            Err(err) => {
                tracing::warn!(%err, "could not mark a departure");
                return;
            }
        };
        let text = if fr {
            format!(
                "Retenu\u{a0}: {}, parti·e le {}",
                person.name,
                weekday_short(day, true)
            )
        } else {
            format!(
                "Kept: {} left on {}",
                person.name,
                weekday_short(day, false)
            )
        };
        self.learned(id, text, edits);
        let first = first_name(&person.name).to_owned();
        let text = if fr {
            format!(
                "Je garde son historique, marqué «\u{a0}parti\u{a0}»\u{a0}; je ne te propose plus {first} pour une relecture ou une réunion."
            )
        } else {
            format!(
                "I keep their history, marked “left”; I no longer suggest {first} for a review or a meeting."
            )
        };
        self.her(id, text, None);
    }

    fn relation_said(
        &mut self,
        id: u64,
        subject: &str,
        kind: &'static str,
        object: &str,
        from: Date,
        pointer: &str,
    ) -> bool {
        let people = learn::people_named(&self.store, subject).unwrap_or_default();
        let [src] = people[..] else {
            return false;
        };
        let target = match kind {
            "replaces" => learn::people_named(&self.store, object)
                .ok()
                .and_then(|p| (p.len() == 1).then(|| p[0])),
            _ => learn::node_named(&self.store, object).ok().flatten(),
        };
        let Some(dst) = target else {
            return false;
        };
        // What it will close, to say so.
        let before: Vec<String> = if kind == "leads" {
            self.store
                .history(dst)
                .unwrap_or_default()
                .into_iter()
                .filter(|e| {
                    e.kind == "leads"
                        && e.target == Target::Node(dst)
                        && e.src != src
                        && e.superseded_at.is_none()
                        && !e.is_proposal()
                        && e.valid_on(from)
                })
                .filter_map(|e| self.store.node(e.src).ok().map(|n| n.name))
                .collect()
        } else {
            Vec::new()
        };
        let notebook = self.store.node(src).map_or(Notebook::Pro, |n| n.notebook);
        let added = self.store.add_edge(&NewEdge {
            src,
            kind: kind.to_owned(),
            target: Target::Node(dst),
            valid_from: Some(from),
            valid_to: None,
            origin: Origin::told(pointer),
            notebook,
        });
        let edit = match added {
            Ok((_, edit)) => edit,
            Err(err) => {
                tracing::warn!(%err, "could not keep a relation");
                return false;
            }
        };
        let fr = self.fr();
        let (who, what) = (
            self.store.node(src).map(|n| n.name).unwrap_or_default(),
            self.store.node(dst).map(|n| n.name).unwrap_or_default(),
        );
        let verb = match (kind, fr) {
            ("leads", true) => "dirige",
            ("works_on", true) => "travaille sur",
            ("reviews", true) => "relit",
            ("replaces", true) => "remplace",
            ("member_of", true) => "fait partie de",
            ("leads", false) => "leads",
            ("works_on", false) => "works on",
            ("reviews", false) => "reviews",
            ("replaces", false) => "replaces",
            _ => "is in",
        };
        let since = if fr {
            format!("depuis le {}", short(from, true))
        } else {
            format!("since {}", short(from, false))
        };
        let text = if fr {
            format!("Retenu\u{a0}: {who} {verb} {what} {since}")
        } else {
            format!("Kept: {who} {verb} {what} {since}")
        };
        self.learned(id, text, vec![edit]);
        if !before.is_empty() {
            let names = join_and(&before, self.talk.t(" et ", " and "));
            let text = if fr {
                format!("Avant, c’était {names}\u{a0}: je garde l’historique.")
            } else {
                format!("Before, it was {names}: I keep the history.")
            };
            self.her(id, text, None);
        }
        true
    }

    // --- Her cards and questions ---------------------------------------------------------

    /// A button of one of her cards: whether it was hers.
    pub(super) fn memory_decide(
        &mut self,
        conn: u64,
        id: u64,
        item: u64,
        decision: Decision,
    ) -> bool {
        let Some(Waiting::Arrival { found, from_source }) = self.memory.waiting.get(&item).cloned()
        else {
            return false;
        };
        let fr = self.fr();
        let outcome = match decision {
            Decision::Edit => {
                let ask = self
                    .talk
                    .t("Dis-moi ce que je change.", "Tell me what to change.");
                self.her(id, ask.to_owned(), None);
                self.done(conn, id);
                return true;
            }
            Decision::Reject => {
                self.memory.waiting.remove(&item);
                self.talk.t("Écarté.", "Dismissed.").to_owned()
            }
            Decision::Accept => {
                self.memory.waiting.remove(&item);
                let told = format!("card:{item}");
                match learn::add_person(&mut self.store, &found, &told, from_source) {
                    Ok((node, edits)) => {
                        let text = match (&found.team, fr) {
                            (Some((_, team, _)), true) => {
                                format!("{} ajouté·e à l’équipe {team}", found.name)
                            }
                            (None, true) => format!("{} ajouté·e à l’équipe", found.name),
                            (Some((_, team, _)), false) => {
                                format!("{} added to the {team} team", found.name)
                            }
                            (None, false) => format!("{} added to the team", found.name),
                        };
                        self.learned(id, text, edits);
                        tracing::info!(node, "an arrival kept");
                        self.talk.t("Ajouté·e.", "Added.").to_owned()
                    }
                    Err(err) => {
                        tracing::warn!(%err, "could not add a person");
                        self.talk
                            .t("Je n’ai pas pu l’ajouter.", "I could not add them.")
                            .to_owned()
                    }
                }
            }
        };
        let word = match decision {
            Decision::Accept => "accepted",
            Decision::Reject => "rejected",
            Decision::Edit => "edit",
        };
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "card".to_owned(),
            ids: vec![format!("card:{item}")],
            outcome: Some(word.to_owned()),
            ..Default::default()
        });
        self.change_item(item, |body| {
            if let Body::Card { card } = body {
                card.decided = Some(decision);
                card.outcome = Some(outcome);
            }
        });
        self.tell_look();
        self.done(conn, id);
        true
    }

    /// A quick reply to one of her questions: whether it was hers.
    pub(super) fn memory_chose(&mut self, conn: u64, id: u64, item: u64, choice: u32) -> bool {
        let Some(waiting) = self.memory.waiting.remove(&item) else {
            return false;
        };
        match waiting {
            Waiting::Which { people, day } => {
                if let Some(node) = people.get(choice as usize) {
                    let pointer = format!("conversation:{item}");
                    self.depart(id, *node, day, &pointer);
                }
            }
            Waiting::Seen(seen) => {
                let act = match choice {
                    0 => Act::Accept,
                    1 => Act::Reject,
                    _ => Act::Later,
                };
                let said = self.answer_seen(seen, act, Some(id));
                if let Some(text) = said {
                    self.her(id, text, None);
                }
            }
            Waiting::Arrival { .. } => {}
        }
        self.done(conn, id);
        true
    }

    /// "Annuler" ten seconds after: too late, and said so. Whether it was.
    pub(super) fn too_late(&mut self, conn: u64, id: u64, item: u64) -> bool {
        let settled = self
            .talk
            .items
            .iter()
            .find(|i| i.id == item)
            .is_some_and(|i| matches!(i.body, Body::Learned { settled: true, .. }));
        if settled {
            self.error(
                conn,
                Some(id),
                "too late: correct it in \u{ab} Ce qu\u{2019}elle sait \u{bb}",
            );
        }
        settled
    }

    // --- Sources and the triage -----------------------------------------------------------

    /// What an item a source read says of the team: a departure of
    /// someone she knows, a proposal; someone she does not know joining,
    /// an observation to ask about.
    pub(super) fn seen_in_source(&mut self, episode: i64, digest: &str, at: Timestamp) {
        let change = match learn::scan(&self.store, digest, at) {
            Ok(Some(change)) => change,
            Ok(None) => return,
            Err(err) => {
                tracing::warn!(%err, "could not read an item for the team");
                return;
            }
        };
        let (pointer, source, notebook) = match self.store.conn.query_row(
            "SELECT pointer, source, notebook FROM episodes WHERE id = ?1",
            [episode],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        ) {
            Ok(row) => row,
            Err(_) => return,
        };
        let notebook = Notebook::of(&notebook).unwrap_or(Notebook::Pro);
        match change {
            learn::Change::Left { node, day } => {
                let source = source.unwrap_or_default();
                let item = pointer
                    .strip_prefix(&format!("{source}:"))
                    .unwrap_or(&pointer)
                    .to_owned();
                let proposed = self.store.add_edge(&NewEdge {
                    src: node,
                    kind: "left".to_owned(),
                    target: Target::Value(day.to_string()),
                    valid_from: Some(day),
                    valid_to: None,
                    origin: Origin::extracted(&source, &item, digest),
                    notebook,
                });
                match proposed {
                    Ok((edge, _)) => tracing::info!(edge, "a departure seen in a source: proposed"),
                    Err(err) => tracing::warn!(%err, "could not propose a departure"),
                }
            }
            learn::Change::Joined { name, role } => self.notice(Noticed::Arrival {
                name,
                role,
                pointer,
                source,
                quote: digest.to_owned(),
                day: day_of(at),
                notebook,
            }),
        }
    }

    /// The triage's candidate facts on `episode`: proposals.
    pub(super) fn extracted(&mut self, episode: i64, facts: &[Fact]) {
        match learn::extract(&mut self.store, episode, facts) {
            Ok((edges, arrivals)) => {
                tracing::info!(edges = edges.len(), "the triage's facts proposed");
                if arrivals.is_empty() {
                    return;
                }
                let row = self.store.conn.query_row(
                    "SELECT pointer, source, notebook, at FROM episodes WHERE id = ?1",
                    [episode],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                );
                let Ok((pointer, source, notebook, at)) = row else {
                    return;
                };
                for (name, role, quote) in arrivals {
                    self.notice(Noticed::Arrival {
                        name,
                        role,
                        pointer: pointer.clone(),
                        source: source.clone(),
                        quote,
                        day: crate::store::unstamp(&at).map_or_else(|| self.today(), day_of),
                        notebook: Notebook::of(&notebook).unwrap_or(Notebook::Pro),
                    });
                }
            }
            Err(err) => tracing::warn!(%err, "could not propose the triage's facts"),
        }
    }

    /// Something noticed: kept and asked once.
    fn notice(&mut self, noticed: Noticed) {
        let mut seen = learn::load_seen(&self.store);
        let key = noticed.key();
        if seen.iter().any(|s| s.noticed.key() == key) {
            return;
        }
        let id = seen.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        let (text, note) = noticed.question(self.fr());
        let replies = vec![
            self.talk.t("Oui", "Yes").to_owned(),
            self.talk.t("Non", "No").to_owned(),
            self.talk.t("Plus tard", "Later").to_owned(),
        ];
        let question = self.add_item(
            None,
            Body::Question {
                text,
                note: Some(note),
                replies,
                chosen: None,
            },
        );
        self.memory.waiting.insert(question, Waiting::Seen(id));
        seen.push(Seen {
            id,
            noticed,
            at: self.clock.now(),
            standing: Standing::Open,
            question: Some(question),
        });
        if let Err(err) = learn::save_seen(&self.store, &seen) {
            tracing::warn!(%err, "could not keep an observation");
        }
        self.audited(audit::Entry {
            kind: audit::Kind::Proposal,
            what: "observation".to_owned(),
            ids: vec![format!("seen:{id}")],
            ..Default::default()
        });
        tracing::info!(id, key, "she noticed something");
    }

    /// An observation answered, from her question or from "Ce qu'elle
    /// sait": what she says of it.
    fn answer_seen(&mut self, id: u64, act: Act, _request: Option<u64>) -> Option<String> {
        let mut all = learn::load_seen(&self.store);
        let seen = all.iter_mut().find(|s| s.id == id)?;
        let now = self.clock.now();
        let fr = self.talk.fr();
        let said = match act {
            Act::Accept => {
                match learn::apply(&mut self.store, seen) {
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(%err, "could not apply an observation");
                        return None;
                    }
                }
                seen.standing = Standing::Accepted;
                if fr { "Retenu." } else { "Kept." }
            }
            Act::Reject => {
                seen.standing = Standing::Declined;
                if fr {
                    "D’accord, je laisse comme ça."
                } else {
                    "All right, I leave it."
                }
            }
            _ => {
                seen.standing = Standing::Later {
                    until: after(now, Duration::from_secs(7 * 24 * 3600)),
                };
                if fr {
                    "Je t’en reparle dans une semaine."
                } else {
                    "I’ll ask again in a week."
                }
            }
        };
        let question = seen.question;
        if let Err(err) = learn::save_seen(&self.store, &all) {
            tracing::warn!(%err, "could not keep an observation's answer");
        }
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "observation".to_owned(),
            ids: vec![format!("seen:{id}")],
            outcome: Some(format!("{act:?}").to_lowercase()),
            ..Default::default()
        });
        // Her question in the thread, answered from the tab: its pill.
        if let Some(question) = question {
            self.memory.waiting.remove(&question);
            let choice = match act {
                Act::Accept => 0,
                Act::Reject => 1,
                _ => 2,
            };
            self.change_item(question, |body| {
                if let Body::Question { chosen, .. } = body
                    && chosen.is_none()
                {
                    *chosen = Some(choice);
                }
            });
        }
        Some(said.to_owned())
    }

    // --- "Ce qu'elle sait" ----------------------------------------------------------------

    /// The view for `query`, sent now and while it changes; none stops it.
    pub(super) fn knows(&mut self, conn: u64, id: u64, query: Option<Query>) {
        match query {
            None => {
                self.memory.watch.remove(&conn);
            }
            Some(query) => {
                self.memory.watch.insert(conn, (id, query, None));
                self.knows_send(conn);
            }
        }
    }

    fn view(&self, query: &Query) -> Result<Knowledge, String> {
        let context = knows::Context {
            now: self.clock.now(),
            fr: self.talk.fr(),
            sources: self
                .slots
                .iter()
                .map(|s| (s.name.clone(), s.notebook))
                .collect(),
            seen: learn::load_seen(&self.store),
        };
        knows::view(&self.store, query, &context).map_err(|e| e.to_string())
    }

    /// The view to `conn` if it changed since it was last sent.
    fn knows_send(&mut self, conn: u64) {
        let Some((id, query, last)) = self.memory.watch.get(&conn).cloned() else {
            return;
        };
        match self.view(&query) {
            Ok(view) => {
                if last.as_ref() == Some(&view) {
                    return;
                }
                if let Some(w) = self.memory.watch.get_mut(&conn) {
                    w.2 = Some(view.clone());
                }
                self.send(
                    conn,
                    &FromSecretary::Knows {
                        id,
                        view: Box::new(view),
                    },
                );
            }
            Err(err) => {
                tracing::warn!(%err, "could not make what she knows");
                self.error(conn, Some(id), err);
            }
        }
    }

    /// Every watcher's view, when it changed.
    pub(super) fn knows_refresh(&mut self) {
        self.memory.watch.retain(|c, _| self.conns.contains_key(c));
        let conns: Vec<u64> = self.memory.watch.keys().copied().collect();
        for conn in conns {
            self.knows_send(conn);
        }
    }

    fn toast(&mut self, conn: u64, id: u64, text: &str, back: Option<Back>) {
        let undo = back.map(|back| {
            self.memory.next_token += 1;
            let token = self.memory.next_token;
            let until = after(self.clock.now(), FORGET_UNDO + Duration::from_secs(1));
            self.memory.backs.insert(token, (back, until));
            token
        });
        self.send(
            conn,
            &FromSecretary::Toast {
                id,
                text: text.to_owned(),
                undo,
            },
        );
    }

    /// Something done in "Ce qu'elle sait".
    pub(super) fn memory_act(&mut self, conn: u64, id: u64, act: Do) {
        let fr = self.fr();
        let result: Result<(), String> = match act {
            Do::Item { item, act } => self.act_on(conn, id, &item, act),
            Do::Restore { token } => match self.memory.backs.remove(&token) {
                Some((_, until)) if until < self.clock.now() => {
                    Err("too late to take it back".to_owned())
                }
                Some((Back::Forgot(kept), _)) => self
                    .store
                    .restore(&kept)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Some((Back::Edits(edits), _)) => {
                    let mut out = Ok(());
                    for edit in edits {
                        if let Err(err) = self.store.undo(edit) {
                            out = Err(err.to_string());
                            break;
                        }
                    }
                    out
                }
                None => Err("nothing to take back".to_owned()),
            },
            Do::ForgetNotebook { notebook } => {
                let done = self
                    .store
                    .forget_notebook(notebook)
                    .map_err(|e| e.to_string());
                if done.is_ok() {
                    // Her observations of that notebook go with it.
                    let mut seen = learn::load_seen(&self.store);
                    seen.retain(|s| {
                        !matches!(&s.noticed, Noticed::Arrival { notebook: n, .. } if *n == notebook)
                    });
                    let _ = learn::save_seen(&self.store, &seen);
                    let text = match (notebook, fr) {
                        (Notebook::Perso, true) => "Tout le perso est oublié. Le pro reste.",
                        (Notebook::Pro, true) => "Tout le pro est oublié. Le perso reste.",
                        (Notebook::Perso, false) => "Everything perso is forgotten. Work stays.",
                        (Notebook::Pro, false) => "Everything work is forgotten. Perso stays.",
                    };
                    self.toast(conn, id, text, None);
                }
                done
            }
            Do::Export { notebook } => self.export(conn, id, notebook),
        };
        if let Err(err) = result {
            tracing::info!(%err, "an act on her memory refused");
            self.error(conn, Some(id), err);
        }
        // Answered with the view, changed or not.
        self.memory.watch.entry(conn).and_modify(|w| {
            w.0 = id;
            w.2 = None;
        });
        self.knows_send(conn);
    }

    fn act_on(&mut self, conn: u64, id: u64, item: &str, act: Act) -> Result<(), String> {
        let fr = self.fr();
        let (kind, number) = item
            .split_once(':')
            .ok_or_else(|| format!("{item:?} is no item"))?;
        let today = self.today();
        let err = |e: crate::store::Error| e.to_string();
        if kind == "seen" {
            let seen: u64 = number.parse().map_err(|_| "no such observation")?;
            self.answer_seen(seen, act, None)
                .ok_or_else(|| "no such observation".to_owned())?;
            return Ok(());
        }
        let n: i64 = number.parse().map_err(|_| format!("{item:?} is no item"))?;
        let kept = match kind {
            "node" => Kept::Node(n),
            "edge" => Kept::Edge(n),
            "rule" => Kept::Rule(n),
            "alias" => Kept::Alias(n),
            "edit" => {
                // A merge of the night's, taken back, or kept.
                return match act {
                    Act::Reject | Act::Forget => self.store.undo(n).map(|_| ()).map_err(err),
                    Act::Keep => knows::keep(&self.store, item).map_err(err),
                    _ => Err("no such act on a merge".to_owned()),
                };
            }
            _ => return Err(format!("{item:?} is no item")),
        };
        match act {
            Act::Forget => {
                // A proposal is rejected: it never was a fact.
                let proposal = matches!(kept, Kept::Edge(_) | Kept::Alias(_) | Kept::Rule(_))
                    && self.store.proposals().map_err(err)?.contains(&kept);
                if proposal {
                    return self.store.reject(kept).map(|_| ()).map_err(err);
                }
                let forgotten = self.store.forget_kept(kept).map_err(err)?;
                tracing::info!(item, "forgotten from Ce qu'elle sait");
                self.toast(
                    conn,
                    id,
                    self.talk.t("Oublié", "Forgotten"),
                    Some(Back::Forgot(Box::new(forgotten))),
                );
                Ok(())
            }
            Act::Close => {
                let edits = match kept {
                    Kept::Edge(edge) => vec![
                        self.store
                            .close_edge(edge, today, "no longer true")
                            .map_err(err)?
                            .1,
                    ],
                    Kept::Rule(rule) => {
                        vec![learn::close_rule(&mut self.store, rule, today).map_err(err)?]
                    }
                    Kept::Node(node) => learn::depart(
                        &mut self.store,
                        node,
                        today,
                        &Origin::told(format!("knows:{item}")),
                    )
                    .map_err(err)?,
                    _ => return Err("it cannot end".to_owned()),
                };
                let text = if fr {
                    format!(
                        "Plus vrai depuis le {}\u{a0}: gardé dans l’historique",
                        short(today, true)
                    )
                } else {
                    format!(
                        "No longer true from {}: kept in the history",
                        short(today, false)
                    )
                };
                self.toast(conn, id, &text, Some(Back::Edits(edits)));
                Ok(())
            }
            Act::Keep => knows::keep(&self.store, item).map_err(err),
            Act::Accept => {
                self.store.accept(kept).map_err(err)?;
                // A departure accepted closes the rest of the tenure.
                if let Kept::Edge(edge) = kept
                    && let Ok(edge) = self.store.edge(edge)
                    && edge.kind == "left"
                    && let Some(day) = edge.valid_from
                {
                    learn::close_tenure(&mut self.store, edge.src, day).map_err(err)?;
                }
                Ok(())
            }
            Act::Reject => self.store.reject(kept).map(|_| ()).map_err(err),
            Act::Edit | Act::Later => Err("done in the conversation".to_owned()),
        }
    }

    /// "Exporter": Markdown and JSON in her folder's `exports`.
    fn export(&mut self, conn: u64, id: u64, notebook: Option<Notebook>) -> Result<(), String> {
        let Some(dir) = self.memory.exports.clone() else {
            return Err("no folder to export to".to_owned());
        };
        let markdown = self
            .store
            .export_markdown(notebook)
            .map_err(|e| e.to_string())?;
        let json = self
            .store
            .export_json(notebook)
            .map_err(|e| e.to_string())?;
        let stem = format!(
            "ce-qu-elle-sait-{}{}",
            self.today(),
            notebook.map_or(String::new(), |n| format!("-{}", n.word()))
        );
        // Written on a thread: her loop never waits on the disk.
        let path = dir.join(format!("{stem}.md"));
        let shown = path.display().to_string();
        let written = std::thread::Builder::new()
            .name("export".to_owned())
            .spawn(move || {
                use std::os::unix::fs::OpenOptionsExt;
                let write = |name: String, text: &str| -> std::io::Result<()> {
                    std::fs::create_dir_all(&dir)?;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .mode(0o600)
                        .open(dir.join(name))?;
                    std::io::Write::write_all(&mut file, text.as_bytes())
                };
                if let Err(err) = write(format!("{stem}.md"), &markdown)
                    .and_then(|()| write(format!("{stem}.json"), &json.to_string()))
                {
                    tracing::warn!(%err, "the export could not be written");
                }
            });
        if let Err(err) = written {
            return Err(err.to_string());
        }
        self.audited(audit::Entry {
            kind: audit::Kind::Memory,
            what: "export".to_owned(),
            ..Default::default()
        });
        let text = if self.fr() {
            format!("Exporté\u{a0}: {shown}")
        } else {
            format!("Exported: {shown}")
        };
        self.toast(conn, id, &text, None);
        Ok(())
    }

    // --- A work draft from the pro notebook only ------------------------------------------

    /// A sentence asking for a work draft or a delegation (§7.4): its
    /// call holds the pro notebook only, unless it names someone of the
    /// perso one.
    pub(super) fn work_scope(&self, sentence: &str) -> Option<Scope> {
        let folded = fold(sentence);
        let drafting = [
            "redige",
            "ecris",
            "reponds",
            "envoie",
            "prepare un mail",
            "prepare un message",
            "fais un mail",
            "un brouillon",
            "confie",
            "delegue",
            "draft",
            "write to",
            "reply to",
            "send ",
            "delegate",
        ]
        .iter()
        .any(|w| folded.contains(w));
        if !drafting {
            return None;
        }
        let linked = crate::retrieve::link(&self.store, sentence).unwrap_or_default();
        let perso = linked.iter().any(|l| {
            self.store
                .node(l.node)
                .is_ok_and(|n| n.notebook == Notebook::Perso)
        });
        if perso {
            return None;
        }
        Some(Scope {
            notebook: Some(Notebook::Pro),
            ..self.scope
        })
    }

    // --- Timers: settling, toasts, the night ------------------------------------------------

    /// When something of hers is due next.
    pub(super) fn memory_next(&self) -> Option<Timestamp> {
        let settle = self
            .talk
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.body,
                    Body::Learned {
                        undone: false,
                        settled: false,
                        ..
                    }
                )
            })
            .filter_map(|i| i.at.parse::<Timestamp>().ok())
            .map(|at| after(at, SETTLE))
            .min();
        let backs = self.memory.backs.values().map(|(_, until)| *until).min();
        let compaction = self.memory.compaction.or_else(|| {
            Some(next_hour(
                self.clock.now(),
                self.settings.current().compaction.hour,
            ))
        });
        [settle, backs, compaction].into_iter().flatten().min()
    }

    /// What is due now.
    pub(super) fn memory_due(&mut self) {
        let now = self.clock.now();
        let due: Vec<u64> = self
            .talk
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.body,
                    Body::Learned {
                        undone: false,
                        settled: false,
                        ..
                    }
                ) && i
                    .at
                    .parse::<Timestamp>()
                    .is_ok_and(|at| after(at, SETTLE) <= now)
            })
            .map(|i| i.id)
            .collect();
        for item in due {
            self.talk.kept.remove(&item);
            self.change_item(item, |body| {
                if let Body::Learned { settled, .. } = body {
                    *settled = true;
                }
            });
        }
        // What a toast could take back, gone after its seconds.
        self.memory.backs.retain(|_, (_, until)| *until > now);
        let settings = self.settings.current().compaction;
        let at = *self
            .memory
            .compaction
            .get_or_insert_with(|| next_hour(now, settings.hour));
        if at <= now {
            self.night(now);
        }
    }

    /// The night's work, if power allows: compaction, then what she
    /// notices.
    fn night(&mut self, now: Timestamp) {
        let settings = self.settings.current();
        let today = day_of(now);
        let done = self
            .store
            .meta("compacted")
            .ok()
            .flatten()
            .and_then(|d| d.parse::<Date>().ok());
        if done == Some(today) {
            self.memory.compaction = Some(next_hour(now, settings.compaction.hour));
            return;
        }
        let power = self.sensors.read();
        let afford = power.on_ac || power.battery.is_some_and(|b| b >= settings.triage.battery);
        if !afford {
            tracing::info!(?power, "the night's compaction waits for power");
            self.memory.compaction = Some(after(now, Duration::from_secs(3600)));
            return;
        }
        if let Err(err) = self.store.set_meta("compacted", &today.to_string()) {
            tracing::warn!(%err, "could not note the compaction");
        }
        self.memory.compaction = Some(next_hour(now, settings.compaction.hour));
        tracing::info!(%today, "the night's compaction");
        match compaction::merge_duplicates(&mut self.store) {
            Ok(merged) if !merged.is_empty() => {
                tracing::info!(merged = merged.len(), "duplicates merged")
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(%err, "could not merge duplicates"),
        }
        // Each day since the last night, a week at most.
        let yesterday = today.yesterday().unwrap_or(today);
        let first = done
            .map_or(yesterday, |d| {
                d.max(today.checked_sub(7.days()).unwrap_or(d))
            })
            .min(yesterday);
        let mut works = Vec::new();
        let mut day = first;
        while day <= yesterday {
            works.extend(compaction::daily(&self.store, day).unwrap_or_default());
            if day.weekday() == jiff::civil::Weekday::Sunday {
                self.memory.weekly = Some(day);
            }
            day = match day.tomorrow() {
                Ok(next) => next,
                Err(_) => break,
            };
        }
        works.extend(compaction::stale(&self.store).unwrap_or_default());
        for work in works {
            self.summarise(work);
        }
        self.weekly_if_ready();
        // What she notices in her sources.
        let sources: Vec<String> = self.slots.iter().map(|s| s.name.clone()).collect();
        match learn::observe(&self.store, &sources, now) {
            Ok(noticed) => {
                for n in noticed {
                    self.notice(n);
                }
            }
            Err(err) => tracing::warn!(%err, "could not look at her sources"),
        }
        self.audited(audit::Entry {
            kind: audit::Kind::Memory,
            what: "compaction".to_owned(),
            ..Default::default()
        });
    }

    fn weekly_if_ready(&mut self) {
        if self.memory.compacting > 0 {
            return;
        }
        if let Some(sunday) = self.memory.weekly.take() {
            for work in compaction::weekly(&self.store, sunday).unwrap_or_default() {
                self.summarise(work);
            }
        }
    }

    /// A summary made: by her compaction role's model on a worker
    /// thread, or in code without one.
    fn summarise(&mut self, work: Work) {
        let Some(model) = self.model.clone() else {
            let text = work.in_code();
            if let Err(err) = work.write(&mut self.store, &text) {
                tracing::warn!(%err, "could not write a summary");
            }
            return;
        };
        let request = work.request();
        let (sender, clock) = (self.sender.clone(), self.clock.clone());
        let started = clock.now();
        self.memory.compacting += 1;
        self.calls_running += 1;
        let spawned = std::thread::Builder::new()
            .name("compaction".to_owned())
            .spawn(move || {
                let result = model.call(&request, &clock);
                let _ = sender.send(Event::Compacted {
                    work: Box::new(work),
                    started,
                    result,
                });
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "no thread for the compaction");
            self.memory.compacting -= 1;
            self.calls_running -= 1;
        }
    }

    /// A summary's words back from her model: written (in code when the
    /// call failed).
    pub(super) fn compacted(
        &mut self,
        work: Work,
        started: Timestamp,
        result: Result<Reply, String>,
    ) {
        self.memory.compacting = self.memory.compacting.saturating_sub(1);
        self.calls_running = self.calls_running.saturating_sub(1);
        let took = Duration::try_from(self.clock.now().duration_since(started)).unwrap_or_default();
        self.audited(audit::Entry {
            kind: audit::Kind::Call,
            what: Role::Compaction.word().to_owned(),
            ids: vec![format!("summary:{}:{}", work.period, work.start)],
            provider: result.as_ref().ok().map(|r| r.provider.clone()),
            model: result.as_ref().ok().map(|r| r.model.clone()),
            tokens_in: result.as_ref().ok().map(|r| r.tokens_in),
            tokens_out: result.as_ref().ok().map(|r| r.tokens_out),
            duration_ms: Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX)),
            outcome: Some(if result.is_ok() { "ok" } else { "failed" }.to_owned()),
        });
        let text = match result {
            Ok(reply) if !reply.text.trim().is_empty() => reply.text.trim().to_owned(),
            Ok(_) => work.in_code(),
            Err(err) => {
                tracing::info!(%err, "the compaction's call failed: a summary in code");
                work.in_code()
            }
        };
        if let Err(err) = work.write(&mut self.store, &text) {
            tracing::warn!(%err, "could not write a summary");
        }
        self.weekly_if_ready();
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

fn join_and(names: &[String], and: &str) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
    }
}
