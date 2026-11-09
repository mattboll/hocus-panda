//! The drawer's conversation (§5.4, lot 2): the thread she keeps, what she
//! makes of a sentence, and the proposals' flow.
//!
//! - **A sentence** becomes the user's bubble, then either one question
//!   computed in code (a word she does not know that one project's items
//!   hold, §2.4: the candidates are code's, not the model's) with its
//!   quick replies, or a call to her model, streamed: her typing and what
//!   she is doing, then her words as they come.
//! - **Her tool calls** never act (§2.1): each outside effect (a message,
//!   a post, a mail, an invitation's answer, an event, a file, a task, an
//!   agent, a job) becomes a card; it happens only after "accept" on that
//!   card, and then only through the effects' recorder (tests) until the
//!   lots that bring each effect. A model asking a question gives a
//!   question with its quick replies.
//! - **"Oui"** to her question keeps the alias as inferred and confirmed
//!   by the user, says so ("Retenu : … · Annuler"), and the sentence goes
//!   on to her model with that fact; a next mention is not asked again.
//!
//! Every connection of the shell's (and the tests') gets the thread's
//! items, her typing and her face; a `hocus` command gets only the answer
//! to its own question. At rest nothing is sent: her face goes out only
//! when it changes.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use hocus_secretary_link::{
    Action, Body, Card, CardKind, Client, Decision, FromSecretary, Item, Look, Mood, Notebook, Tone,
};

use super::{Brain, Event};
use crate::{
    audit,
    effects::{Kind, Proposal},
    fold::fold,
    graph::{NodeKind, Origin},
    model::{Message, Model, Part, Piece, Reply, Role, ToolCall, ToolResult},
    store::{Item as Kept, Who},
    tools,
};

/// The thread's items she keeps (the newest).
const THREAD_MOST: usize = 200;

/// Her model's turns at most for one sentence: read tools, then her
/// answer.
const ROUNDS: u32 = 4;

/// A sentence's conversation with her model so far.
#[derive(Debug)]
struct Turn {
    messages: Vec<Message>,
    facts: Vec<Part>,
    round: u32,
    /// What the call may hold: the pro notebook only for a work draft
    /// (§7.4, lot 6); none, her usual scope.
    scope: Option<crate::context::Scope>,
    /// The model of the whole turn, chosen at its first call (#750): a
    /// change of the settings meanwhile never sends one model's own
    /// blocks to another.
    model: Option<Arc<dyn Model>>,
}

/// The thread's latest items said again to her model, as plain text,
/// before a sentence (§7.3's conversation part, which caps it).
const HISTORY_MOST: usize = 12;

/// A word she does not know, and the one project whose items hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub term: String,
    pub node: i64,
    pub name: String,
    /// The item's digest that holds it, and where it is.
    pub quote: String,
    pub pointer: String,
    pub board: bool,
    pub notebook: Notebook,
}

/// A sentence held until her question about it is answered.
#[derive(Debug)]
struct Pending {
    question: u64,
    sentence: String,
    candidate: Candidate,
}

/// The conversation's state, on her loop.
#[derive(Debug, Default)]
pub(super) struct Talk {
    pub(super) items: Vec<Item>,
    pub(super) next: u64,
    /// The words she makes herself in French (the desktop's language, as
    /// the shell says it).
    english: bool,
    /// The request she composes an answer to.
    composing: Option<u64>,
    pending: Option<Pending>,
    /// Folded words the user said were not the project she asked about.
    declined: HashSet<String>,
    pub(super) proposals: HashMap<u64, Proposal>,
    /// What "Annuler" undoes, by its item: the edits, the last first.
    pub(super) kept: HashMap<u64, Vec<i64>>,
    drawer: bool,
    /// Something just kept: her face says so until the next sentence.
    pub(super) happy: bool,
    /// Her face as last sent.
    told: Option<Look>,
    /// The sentences her model is on, by request.
    turns: HashMap<u64, Turn>,
    /// The cards whose proposal is written with them in her memory.
    pub(super) saved: HashSet<u64>,
}

impl Talk {
    pub(super) fn fr(&self) -> bool {
        !self.english
    }

    pub(super) fn t(&self, fr: &'static str, en: &'static str) -> &'static str {
        if self.english { en } else { fr }
    }
}

impl Brain {
    /// A connection said hello: a shell gets her face, and its language
    /// is hers for the words she makes.
    pub(super) fn greeted(&mut self, conn: u64, client: Client, lang: Option<&str>) {
        if client == Client::Shell {
            self.talk.english = lang.is_some_and(|l| l.starts_with("en"));
        }
        if matches!(client, Client::Shell | Client::Test) {
            let look = self.look();
            self.talk.told = Some(look.clone());
            self.send(conn, &FromSecretary::Look { look });
        }
        if matches!(client, Client::Shell | Client::Test) {
            self.greeted_speaking(conn);
            // Her first run, once she knows her accounts (#750).
            self.first_run_step();
        }
    }

    /// Whether `conn` gets the thread: the shell's, and the tests'.
    pub(super) fn talker(&self, conn: u64) -> bool {
        self.conns
            .get(&conn)
            .is_some_and(|c| matches!(c.client, Some(Client::Shell | Client::Test)))
    }

    /// `message` to every talker, and to `asker` (a `hocus` command's
    /// question) besides.
    pub(super) fn tell_talkers(&mut self, message: &FromSecretary, asker: Option<u64>) {
        let mut to: Vec<u64> = self
            .conns
            .keys()
            .copied()
            .filter(|c| self.talker(*c))
            .collect();
        if let Some(asker) = asker
            && !to.contains(&asker)
        {
            to.push(asker);
        }
        for conn in to {
            self.send(conn, message);
        }
    }

    pub(super) fn add_item(&mut self, request: Option<u64>, body: Body) -> u64 {
        self.add_item_in(request, body, None)
    }

    /// A thread item, in `notebook` when it is about one.
    pub(super) fn add_item_in(
        &mut self,
        request: Option<u64>,
        body: Body,
        notebook: Option<Notebook>,
    ) -> u64 {
        self.talk.next += 1;
        let item = Item {
            id: self.talk.next,
            at: self.clock.now().to_string(),
            request,
            notebook,
            body,
        };
        self.talk.items.push(item.clone());
        if self.talk.items.len() > THREAD_MOST {
            let over = self.talk.items.len() - THREAD_MOST;
            for gone in self.talk.items.drain(..over) {
                self.talk.proposals.remove(&gone.id);
                self.talk.kept.remove(&gone.id);
                self.talk.saved.remove(&gone.id);
            }
        }
        self.keep_item(&item);
        self.tell_talkers(&FromSecretary::Item { item }, None);
        self.talk.next
    }

    pub(super) fn change_item(&mut self, id: u64, change: impl FnOnce(&mut Body)) {
        let Some(item) = self.talk.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        change(&mut item.body);
        let item = item.clone();
        self.keep_item(&item);
        self.tell_talkers(&FromSecretary::Item { item }, None);
    }

    /// A thread item written in her memory, encrypted: her thread
    /// survives a restart (#750).
    fn keep_item(&self, item: &Item) {
        if let Err(err) = crate::kept::keep_item(&self.store.conn, item, None) {
            tracing::warn!(%err, "could not keep a thread item");
        }
    }

    /// Her thread before request `id`, as plain text turns: the user's
    /// sentences and her words, never a model's own blocks. None in a
    /// public moment or for a work draft: what was said in private stays
    /// out of it.
    fn history(
        &self,
        id: u64,
        sentence: &str,
        scope: Option<&crate::context::Scope>,
    ) -> Vec<Message> {
        if scope.is_some() || self.scope.moment.public() {
            return Vec::new();
        }
        let said: Vec<Message> = self
            .talk
            .items
            .iter()
            .filter(|i| i.request != Some(id) && i.notebook != Some(Notebook::Perso))
            .filter_map(|i| match &i.body {
                Body::User { text } => Some(Message::user(text.clone())),
                Body::Her { text, .. } => Some(Message::assistant(text.clone())),
                _ => None,
            })
            .collect();
        let mut turns: Vec<Message> = Vec::new();
        for message in said.into_iter().rev().take(HISTORY_MOST).rev() {
            match turns.last_mut() {
                Some(last) if last.from == message.from => {
                    last.text.push_str("\n\n");
                    last.text.push_str(&message.text);
                }
                _ => turns.push(message),
            }
        }
        // It opens with the user and ends with her: the sentence follows.
        while turns.first().is_some_and(|m| m.from != "user") {
            turns.remove(0);
        }
        while turns
            .last()
            .is_some_and(|m| m.from == "user" || m.text.trim() == sentence.trim())
        {
            turns.pop();
        }
        turns
    }

    /// The end of what she says to request `id`.
    pub(super) fn done(&mut self, conn: u64, id: u64) {
        self.tell_talkers(
            &FromSecretary::Answer {
                id,
                text: String::new(),
                done: true,
            },
            Some(conn),
        );
    }

    /// Her face now.
    pub(super) fn look(&self) -> Look {
        let asking = self.talk.pending.is_some()
            || self
                .talk
                .items
                .last()
                .is_some_and(|i| matches!(i.body, Body::Question { chosen: None, .. }));
        let failing: Vec<&str> = self
            .slots
            .iter()
            .filter(|s| s.error.is_some())
            .map(|s| s.name.as_str())
            .collect();
        let mood = if self.talk.composing.is_some() {
            Mood::Type
        } else if asking {
            Mood::Ask
        } else if !failing.is_empty() {
            Mood::Worry
        } else if self.talk.happy {
            Mood::Happy
        } else {
            Mood::Listen
        };
        let fr = self.talk.fr();
        let waiting = self.queue.pending();
        let paused = match &self.queue.state {
            crate::triage::State::Paused { why, .. } if waiting > 0 => Some(*why),
            _ => None,
        };
        let unreadable = self.unreadable_calendars();
        let (status, tone, hint) = if let Some(provider) = &self.key_missing {
            // Nothing she answers can be, without it.
            let name = if provider == "anthropic" {
                "Anthropic".to_owned()
            } else {
                provider.clone()
            };
            if fr {
                (
                    format!("Il me manque ma clé {name}"),
                    Tone::Warn,
                    format!(
                        "Dans un terminal : hocus-secretary key {provider}, Entrée, puis colle ta clé."
                    ),
                )
            } else {
                (
                    format!("I’m missing my {name} key"),
                    Tone::Warn,
                    format!(
                        "In a terminal: hocus-secretary key {provider}, Enter, then paste your key."
                    ),
                )
            }
        } else if let Some(name) = failing.first() {
            let name = shown_source(name);
            let status = if fr {
                format!("{name} ne répond plus")
            } else {
                format!("{name} is not answering")
            };
            (status, Tone::Warn, String::new())
        } else if let Some(why) = paused {
            let n = waiting;
            let (status, hint) = match (why, fr) {
                (crate::triage::Pause::Unavailable, true) => (
                    format!("Le tri sur cet ordinateur ne répond pas · {n} en attente"),
                    "Rien n’est perdu : ils attendent. D’ici là, seuls l’expéditeur, l’objet et la date partent vers Claude.".to_owned(),
                ),
                (crate::triage::Pause::Unavailable, false) => (
                    format!("The sorting on this computer is not answering · {n} waiting"),
                    "Nothing is lost: they wait. Until then, only the sender, subject and date go to Claude.".to_owned(),
                ),
                (crate::triage::Pause::Battery(_), true) => (
                    format!("Sur batterie : je trie les {n} nouveaux quand tu rebranches"),
                    "D’ici là, seuls l’expéditeur, l’objet et la date partent vers Claude.".to_owned(),
                ),
                (crate::triage::Pause::Battery(_), false) => (
                    format!("On battery: I’ll sort the {n} new ones once you plug in"),
                    "Until then, only the sender, subject and date go to Claude.".to_owned(),
                ),
                (crate::triage::Pause::Heat(_), true) => (
                    format!("L’ordinateur chauffe : le tri attend · {n} en attente"),
                    "Je reprends quand il a refroidi.".to_owned(),
                ),
                (crate::triage::Pause::Heat(_), false) => (
                    format!("The computer is hot: the sorting waits · {n} waiting"),
                    "I’ll go on once it has cooled down.".to_owned(),
                ),
            };
            (status, Tone::Warn, hint)
        } else if !unreadable.is_empty() {
            // Some calendars could not be read (#752): the others are.
            let n = unreadable.len();
            let names: Vec<&str> = unreadable.iter().map(|c| c.name.as_str()).collect();
            let up: Vec<String> = self.slots.iter().map(|s| shown_source(&s.name)).collect();
            let (status, hint) = if fr {
                (
                    format!(
                        "{} à jour · {n} agenda{} illisible{}",
                        up.join(" · "),
                        if n > 1 { "s" } else { "" },
                        if n > 1 { "s" } else { "" }
                    ),
                    format!(
                        "Je n’arrive pas à lire : {}. Je réessaie à chaque passage ; tu peux les laisser de côté dans Réglages › Agents.",
                        names.join(", ")
                    ),
                )
            } else {
                (
                    format!(
                        "{} up to date · {n} calendar{} unreadable",
                        up.join(" · "),
                        if n > 1 { "s" } else { "" }
                    ),
                    format!(
                        "I can’t read: {}. I try again each time; you can leave them out in Settings › Agents.",
                        names.join(", ")
                    ),
                )
            };
            (status, Tone::Warn, hint)
        } else if self.slots.is_empty() {
            (
                self.talk
                    .t("Rien de branché", "Nothing connected")
                    .to_owned(),
                Tone::Off,
                String::new(),
            )
        } else {
            let names: Vec<String> = self.slots.iter().map(|s| shown_source(&s.name)).collect();
            let up = self.talk.t("à jour", "up to date");
            (
                format!("{} {up}", names.join(" · ")),
                Tone::Ok,
                String::new(),
            )
        };
        Look {
            mood,
            status,
            tone,
            model: self.model.as_ref().map(|m| m.shown()).unwrap_or_default(),
            hint,
            key: self.key_missing.clone(),
            name: self.settings.current().name.trim().to_owned(),
        }
    }

    /// Her face to the talkers, when it changed.
    pub(super) fn tell_look(&mut self) {
        let look = self.look();
        if self.talk.told.as_ref() == Some(&look) {
            return;
        }
        self.talk.told = Some(look.clone());
        self.tell_talkers(&FromSecretary::Look { look }, None);
    }

    pub(super) fn thread(&mut self, conn: u64, id: u64) {
        let items = self.talk.items.clone();
        self.send(conn, &FromSecretary::Thread { id, items });
    }

    pub(super) fn drawer(&mut self, open: bool) {
        tracing::debug!(open, "her drawer");
        self.talk.drawer = open;
        // A key given meanwhile shows as the drawer opens.
        if open && self.key_missing.is_some() {
            self.ask_key();
        }
    }

    /// A sentence of the user's.
    pub(super) fn sentence(&mut self, conn: u64, id: u64, text: String) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return self.error(conn, Some(id), "an empty sentence");
        }
        self.talk.happy = false;
        self.add_item(Some(id), Body::User { text: text.clone() });
        // What teaches her is read in code (lot 6): no model call.
        if self.taught(conn, id, &text) {
            self.tell_look();
            return;
        }
        match self.candidate(&text) {
            Some(candidate) => {
                self.ask_about(id, text, candidate);
                self.tell_look();
                self.done(conn, id);
            }
            None => {
                self.compose(conn, id, text, Vec::new());
                self.tell_look();
            }
        }
    }

    /// Her question about a word she does not know.
    fn ask_about(&mut self, id: u64, sentence: String, candidate: Candidate) {
        let fr = self.talk.fr();
        let (term, name) = (&candidate.term, &candidate.name);
        let text = if fr {
            format!(
                "Une question avant de le ranger\u{a0}: «\u{a0}{term}\u{a0}», c’est pour {name}\u{a0}?"
            )
        } else {
            format!("One question before I file it: “{term}”, is that {name}?")
        };
        let quote = &candidate.quote;
        let note = match (fr, candidate.board) {
            (true, true) => format!(
                "Je ne connais pas encore ce mot. Dans le tableau de {name}, il y a «\u{a0}{quote}\u{a0}»."
            ),
            (true, false) => {
                format!("Je ne connais pas encore ce mot. J’ai lu «\u{a0}{quote}\u{a0}» ({name}).")
            }
            (false, true) => {
                format!("I don’t know this word yet. {name}’s board has “{quote}”.")
            }
            (false, false) => format!("I don’t know this word yet. I read “{quote}” ({name})."),
        };
        let replies = if fr {
            vec![
                format!("Oui, {name}"),
                "Non, autre chose".to_owned(),
                "Je t’explique".to_owned(),
            ]
        } else {
            vec![
                format!("Yes, {name}"),
                "No, something else".to_owned(),
                "Let me explain".to_owned(),
            ]
        };
        tracing::info!(
            id,
            node = candidate.node,
            "she asks about a word she does not know"
        );
        let question = self.add_item(
            Some(id),
            Body::Question {
                text,
                note: Some(note),
                replies,
                chosen: None,
            },
        );
        self.talk.pending = Some(Pending {
            question,
            sentence,
            candidate,
        });
    }

    /// Quick reply `choice` of question `item`.
    pub(super) fn choose(&mut self, conn: u64, id: u64, item: u64, choice: u32) {
        let Some(Body::Question {
            replies, chosen, ..
        }) = self
            .talk
            .items
            .iter()
            .find(|i| i.id == item)
            .map(|i| i.body.clone())
        else {
            return self.error(conn, Some(id), "no such question");
        };
        if chosen.is_some() {
            return self.error(conn, Some(id), "answered already");
        }
        let Some(reply) = replies.get(choice as usize).cloned() else {
            return self.error(conn, Some(id), "no such reply");
        };
        self.change_item(item, |body| {
            if let Body::Question { chosen, .. } = body {
                *chosen = Some(choice);
            }
        });
        self.talk.happy = false;
        self.add_item(
            Some(id),
            Body::User {
                text: reply.clone(),
            },
        );
        // Her own questions about her memory (lot 6).
        if self.memory_chose(conn, id, item, choice) {
            self.tell_look();
            return;
        }
        let pending = self.talk.pending.take_if(|p| p.question == item);
        match (pending, choice) {
            (Some(pending), 0) => {
                let fact = self.confirm(id, &pending.candidate);
                self.compose(conn, id, pending.sentence, fact.into_iter().collect());
            }
            (Some(pending), 1) => {
                self.talk.declined.insert(fold(&pending.candidate.term));
                self.compose(conn, id, pending.sentence, Vec::new());
            }
            (Some(_), _) => {
                let listen = self.talk.t("Je t’écoute.", "I’m listening.");
                self.add_item(
                    Some(id),
                    Body::Her {
                        text: listen.to_owned(),
                        note: None,
                        assumed: false,
                    },
                );
                self.tell_look();
                self.done(conn, id);
                return;
            }
            // Her model's own question: the reply is a sentence.
            (None, _) => self.compose(conn, id, reply, Vec::new()),
        }
        self.tell_look();
    }

    /// "Oui": the word kept as an alias of the project, inferred and
    /// confirmed by the user; said with "Annuler". The fact for her model.
    fn confirm(&mut self, id: u64, candidate: &Candidate) -> Option<Part> {
        let origin = Origin::inferred(candidate.pointer.clone(), 0.9);
        let added =
            self.store
                .add_alias(candidate.node, &candidate.term, &origin, candidate.notebook);
        let edits = match added {
            Ok((alias, added)) => match self.store.accept(Kept::Alias(alias)) {
                Ok(accepted) => Some((alias, vec![accepted, added])),
                Err(err) => {
                    tracing::warn!(%err, "could not accept the alias");
                    None
                }
            },
            Err(err) => {
                tracing::warn!(%err, "could not keep the alias");
                None
            }
        };
        let (alias, edits) = edits?;
        let (term, name) = (&candidate.term, &candidate.name);
        let text = if self.talk.fr() {
            format!("Retenu\u{a0}: «\u{a0}{term}\u{a0}» → {name}")
        } else {
            format!("Kept: “{term}” → {name}")
        };
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
        Some(Part {
            label: "fait".to_owned(),
            text: format!("« {term} » désigne {name} (déduit, confirmé par l’utilisateur)"),
            items: vec![Kept::Alias(alias).word()],
        })
    }

    /// "Annuler" under what she kept.
    pub(super) fn undo(&mut self, conn: u64, id: u64, item: u64) {
        let Some(edits) = self.talk.kept.remove(&item) else {
            return self.error(conn, Some(id), "nothing to undo there");
        };
        for edit in edits {
            if let Err(err) = self.store.undo(edit) {
                tracing::warn!(%err, "could not undo");
                return self.error(conn, Some(id), "could not undo");
            }
        }
        self.change_item(item, |body| {
            if let Body::Learned { undone, .. } = body {
                *undone = true;
            }
        });
        self.talk.happy = false;
        self.tell_look();
        self.done(conn, id);
    }

    /// The sentence to her model, `facts` computed in code with it; her
    /// answer comes in pieces ([`Event::Piece`]) on a worker thread.
    fn compose(&mut self, conn: u64, id: u64, sentence: String, facts: Vec<Part>) {
        let scope = self.work_scope(&sentence);
        let mut messages = self.history(id, &sentence, scope.as_ref());
        messages.push(Message::user(sentence));
        self.compose_turn(
            conn,
            id,
            Turn {
                messages,
                facts,
                round: 0,
                scope,
                model: None,
            },
        );
    }

    /// A turn of her model's: the conversation so far (her read tools'
    /// results after their calls), her context assembled for it.
    fn compose_turn(&mut self, conn: u64, id: u64, mut turn: Turn) {
        let Some(base) = self.model.clone() else {
            let none = self.talk.t(
                "Je n’ai pas encore de modèle pour te répondre.",
                "I have no model to answer with yet.",
            );
            self.add_item(
                Some(id),
                Body::Her {
                    text: none.to_owned(),
                    note: None,
                    assumed: false,
                },
            );
            return self.error(conn, Some(id), "she has no model yet");
        };
        // The turn's model, fixed at its first call.
        let model = match turn.model.clone() {
            Some(model) => model,
            None => match base.pin(Role::Answer) {
                Ok(pinned) => pinned.unwrap_or(base),
                Err(err) => return self.replied(conn, id, Err(err)),
            },
        };
        turn.model = Some(model.clone());
        let made = match &turn.scope {
            Some(scope) => self.request_in(Role::Answer, &turn.messages, scope),
            None => self.request(Role::Answer, &turn.messages),
        };
        let mut request = match made {
            Ok(request) => request,
            Err(err) => return self.error(conn, Some(id), err),
        };
        // What code computed for this sentence, and the tools that only
        // propose, beside her read tools.
        request.context.extend(turn.facts.iter().cloned());
        request
            .tools
            .extend(tools::TALK.iter().map(|t| (*t).to_owned()));
        self.talk.turns.insert(id, turn);
        self.talk.composing = Some(id);
        self.tell_talkers(
            &FromSecretary::Typing {
                id,
                doing: String::new(),
            },
            Some(conn),
        );
        let (sender, clock) = (self.sender.clone(), self.clock.clone());
        let started = clock.now();
        self.calls_running += 1;
        let spawned = std::thread::Builder::new()
            .name("model call".to_owned())
            .spawn(move || {
                let pieces = sender.clone();
                let result = model.stream_pieces(&request, &clock, &mut |piece| {
                    let piece = match piece {
                        Piece::Doing(doing) => Said::Doing(doing.to_owned()),
                        Piece::Text(text) => Said::Text(text.to_owned()),
                    };
                    let _ = pieces.send(Event::Piece { conn, id, piece });
                });
                let _ = sender.send(Event::Called {
                    conn,
                    id,
                    role: request.role,
                    items: request.items(),
                    started,
                    result,
                });
            });
        if let Err(err) = spawned {
            self.calls_running -= 1;
            self.talk.composing = None;
            self.error(conn, Some(id), format!("no thread for the call: {err}"));
        }
    }

    /// A piece of her answer as it comes.
    pub(super) fn piece(&mut self, conn: u64, id: u64, piece: Said) {
        let message = match piece {
            Said::Doing(doing) => FromSecretary::Typing { id, doing },
            Said::Text(text) => FromSecretary::Answer {
                id,
                text,
                done: false,
            },
        };
        self.tell_talkers(&message, Some(conn));
    }

    /// Her model's reply whole: her words kept, each tool call a card or a
    /// question, and the end of her answer.
    pub(super) fn replied(&mut self, conn: u64, id: u64, result: Result<Reply, String>) {
        if self.talk.composing == Some(id) {
            self.talk.composing = None;
        }
        if result.is_ok() && self.key_missing.is_some() {
            self.key_missing = None;
        }
        if result.is_err() {
            self.talk.turns.remove(&id);
        }
        match result {
            Ok(reply) => {
                if !reply.text.trim().is_empty() {
                    self.add_item(
                        Some(id),
                        Body::Her {
                            text: reply.text.clone(),
                            note: None,
                            assumed: false,
                        },
                    );
                }
                // Her read tools run here, on her loop, which owns her
                // memory; every other call only shows (a card, a
                // question), and is told so.
                let mut results = Vec::new();
                let mut read = false;
                for call in &reply.tool_calls {
                    if tools::READ.contains(&call.name.as_str()) {
                        read = true;
                        let ran = tools::run(&self.store, call, None);
                        results.push(ToolResult {
                            id: call.id.clone(),
                            error: ran.is_err(),
                            text: ran.unwrap_or_else(|e| e),
                        });
                    } else {
                        self.tool_call(id, call);
                        results.push(ToolResult {
                            id: call.id.clone(),
                            text: "Montré à l'utilisateur ; rien n'est fait avant son clic."
                                .to_owned(),
                            error: false,
                        });
                    }
                }
                let turn = self.talk.turns.remove(&id);
                if let Some(mut turn) = turn.filter(|t| read && t.round < ROUNDS) {
                    turn.messages.push(Message::replied(&reply));
                    turn.messages.push(Message {
                        from: "user".to_owned(),
                        tool_results: results,
                        ..Message::default()
                    });
                    turn.round += 1;
                    let looking = self
                        .talk
                        .t("elle cherche dans sa mémoire", "she looks in her memory");
                    self.tell_talkers(
                        &FromSecretary::Typing {
                            id,
                            doing: looking.to_owned(),
                        },
                        Some(conn),
                    );
                    return self.compose_turn(conn, id, turn);
                }
                self.tell_look();
                self.done(conn, id);
            }
            Err(err) => {
                let no_key = err.starts_with("no key for ");
                if no_key {
                    self.key_missing = err
                        .strip_prefix("no key for ")
                        .and_then(|r| r.split(':').next())
                        .map(str::to_owned);
                }
                let failed = if no_key {
                    self.talk.t(
                        "Je n’ai pas pu te répondre : il me manque ma clé Anthropic. Dans un terminal, colle-la dans le champ en haut de mon tiroir, ou dans un terminal tape « hocus-secretary key anthropic », Entrée, puis colle ta clé ; je la garde dans ton trousseau.",
                        "I couldn’t answer: I’m missing my Anthropic key. In a terminal, paste it in the field at the top of my drawer, or in a terminal type “hocus-secretary key anthropic”, Enter, then paste your key; I keep it in your keyring.",
                    )
                } else {
                    self.talk.t(
                        "Je n’ai pas pu te répondre : le modèle ne répond pas.",
                        "I couldn’t answer: the model is not responding.",
                    )
                };
                self.add_item(
                    Some(id),
                    Body::Her {
                        text: failed.to_owned(),
                        note: None,
                        assumed: false,
                    },
                );
                self.tell_look();
                self.tell_talkers(
                    &FromSecretary::Error {
                        id: Some(id),
                        message: err,
                    },
                    Some(conn),
                );
            }
        }
    }

    /// A tool call: what it shows in the thread; it acts on nothing.
    fn tool_call(&mut self, id: u64, call: &ToolCall) {
        let fr = self.talk.fr();
        match made(call, fr) {
            Some(Made::Question {
                text,
                note,
                replies,
            }) => {
                self.add_item(
                    Some(id),
                    Body::Question {
                        text,
                        note,
                        replies,
                        chosen: None,
                    },
                );
            }
            Some(Made::Card(card, kind)) => {
                let notebook = call
                    .input
                    .get("notebook")
                    .and_then(|n| n.as_str())
                    .and_then(Notebook::of)
                    .unwrap_or(Notebook::Pro);
                let item = self.add_item_in(Some(id), Body::Card { card }, Some(notebook));
                let effect = kind.word();
                tracing::info!(item, effect, tool = call.name, "she proposes");
                self.talk.proposals.insert(
                    item,
                    Proposal {
                        kind,
                        tool: call.name.clone(),
                        input: call.input.clone(),
                        notebook,
                    },
                );
                self.audited(audit::Entry {
                    kind: audit::Kind::Proposal,
                    what: effect.to_owned(),
                    ids: vec![format!("card:{item}")],
                    ..Default::default()
                });
            }
            None => tracing::info!(tool = call.name, "a tool call she does not know: nothing"),
        }
    }

    pub(super) fn audited(&mut self, entry: audit::Entry) {
        if let Err(err) = self.store.audit(entry) {
            tracing::warn!(%err, "could not audit");
        }
    }

    /// A card's button.
    pub(super) fn decide(&mut self, conn: u64, id: u64, item: u64, decision: Decision) {
        let Some(Body::Card { card }) = self
            .talk
            .items
            .iter()
            .find(|i| i.id == item)
            .map(|i| i.body.clone())
        else {
            return self.error(conn, Some(id), "no such card");
        };
        if card.decided.is_some() {
            return self.error(conn, Some(id), "decided already");
        }
        if !card.actions.iter().any(|a| a.decision == decision) {
            return self.error(conn, Some(id), "no such button");
        }
        // Her own cards about her memory (lot 6).
        if self.memory_decide(conn, id, item, decision) {
            return;
        }
        let now = self.clock.now();
        let outcome = match decision {
            Decision::Edit => {
                let ask = self
                    .talk
                    .t("Dis-moi ce que je change.", "Tell me what to change.");
                self.add_item(
                    Some(id),
                    Body::Her {
                        text: ask.to_owned(),
                        note: None,
                        assumed: false,
                    },
                );
                self.tell_look();
                return self.done(conn, id);
            }
            Decision::Reject => self.talk.t("Écarté.", "Dismissed.").to_owned(),
            Decision::Accept => match self.talk.proposals.get(&item).cloned() {
                Some(proposal) => self.accepted(now, item, &proposal),
                None => self.talk.t("Noté.", "Noted.").to_owned(),
            },
        };
        let word = match decision {
            Decision::Accept => "accepted",
            Decision::Reject => "rejected",
            Decision::Edit => "edit",
        };
        tracing::info!(item, word, "a card decided");
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
    }

    /// The word of `sentence` she does not know that exactly one project's
    /// items hold, the first; none when every word is known, common, or
    /// in several projects' items.
    pub(super) fn candidate(&self, sentence: &str) -> Option<Candidate> {
        for word in sentence
            .split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|w| w.chars().count() >= 5 && w.chars().all(char::is_alphabetic))
        {
            let folded = fold(word);
            if COMMON.contains(&folded.as_str()) || self.talk.declined.contains(&folded) {
                continue;
            }
            if self.store.find(word).is_ok_and(|known| !known.is_empty()) {
                continue;
            }
            let Ok(found) = self.store.search(word, None) else {
                continue;
            };
            let mut projects: BTreeMap<i64, (String, String, String, Notebook)> = BTreeMap::new();
            for episode in found.into_iter().filter_map(|i| match i {
                Kept::Episode(id) => Some(id),
                _ => None,
            }) {
                for (node, name, kind, digest, pointer, notebook) in self.projects_of(episode) {
                    let _ = kind;
                    projects
                        .entry(node)
                        .or_insert((name, digest, pointer, notebook));
                }
            }
            if projects.len() != 1 {
                continue;
            }
            let (node, (name, digest, pointer, notebook)) = projects.into_iter().next()?;
            let board = self.episode_kind(&pointer).is_some_and(|k| {
                matches!(k.as_str(), "ticket" | "card" | "task" | "board" | "issue")
            });
            return Some(Candidate {
                term: word.to_lowercase(),
                node,
                name,
                quote: digest,
                pointer,
                board,
                notebook,
            });
        }
        None
    }

    /// The project nodes episode `episode` mentions: their id and name,
    /// the episode's kind, digest, pointer and notebook.
    fn projects_of(&self, episode: i64) -> Vec<(i64, String, String, String, String, Notebook)> {
        let query = "SELECT n.id, n.name, e.kind, e.digest, e.pointer, e.notebook
                     FROM mentions m JOIN nodes n ON n.id = m.node JOIN episodes e ON e.id = m.episode
                     WHERE m.episode = ?1 AND n.kind = 'project'";
        let read = || -> rusqlite::Result<Vec<_>> {
            let mut statement = self.store.conn.prepare(query)?;
            let rows = statement.query_map([episode], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    Notebook::of(&r.get::<_, String>(5)?).unwrap_or(Notebook::Pro),
                ))
            })?;
            rows.collect()
        };
        read().unwrap_or_else(|err| {
            tracing::warn!(%err, "could not read an episode's projects");
            Vec::new()
        })
    }

    fn episode_kind(&self, pointer: &str) -> Option<String> {
        self.store
            .conn
            .query_row(
                "SELECT kind FROM episodes WHERE pointer = ?1",
                [pointer],
                |r| r.get(0),
            )
            .ok()
    }

    /// The node of project `name` a source's item belongs to: hers, or a
    /// new one the source made.
    pub(super) fn project_node(
        &mut self,
        name: &str,
        notebook: Notebook,
        source: &str,
    ) -> Option<i64> {
        let known = self.store.find(name).ok().unwrap_or_default();
        for id in known {
            if self
                .store
                .node(id)
                .is_ok_and(|n| n.kind == NodeKind::Project)
            {
                return Some(id);
            }
        }
        let made = self.store.add_node(
            NodeKind::Project,
            name,
            notebook,
            &Who::Source(source.to_owned()),
            "a project its source names",
        );
        match made {
            Ok((id, _)) => Some(id),
            Err(err) => {
                tracing::warn!(%err, "could not keep a project");
                None
            }
        }
    }
}

/// A piece of her answer, owned, on its way to her loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    Doing(String),
    Text(String),
}

/// What a tool call shows in the thread.
#[derive(Debug, Clone, PartialEq)]
enum Made {
    Question {
        text: String,
        note: Option<String>,
        replies: Vec<String>,
    },
    Card(Card, Kind),
}

/// A tool call's card or question; none for a tool she does not know.
/// The model has no tool that acts: a call named as one that would
/// (`send_message`, `write_file`, `start_job`…) is a proposal all the
/// same.
fn made(call: &ToolCall, fr: bool) -> Option<Made> {
    let input = &call.input;
    let field = |name: &str| input.get(name).and_then(|v| v.as_str()).map(str::to_owned);
    let list = |name: &str| -> Vec<String> {
        input
            .get(name)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let w = |f: &str, e: &str| if fr { f.to_owned() } else { e.to_owned() };
    let name = call.name.as_str();
    if name == "ask_user" {
        return Some(Made::Question {
            text: field("question").or_else(|| field("text"))?,
            note: field("note"),
            replies: list("replies").into_iter().take(4).collect(),
        });
    }
    let (kind_of, kind) = match name {
        "propose_message" | "send_message" | "reply" => (Kind::Message, CardKind::Draft),
        "propose_post" | "post" | "post_message" => (Kind::Post, CardKind::Draft),
        "propose_mail" | "send_mail" | "send_email" => (Kind::Mail, CardKind::Draft),
        "propose_invitation_answer" | "answer_invitation" | "respond_to_event" => {
            (Kind::Invitation, CardKind::Plan)
        }
        "propose_event" | "create_event" => (Kind::Event, CardKind::Plan),
        "propose_file" | "write_file" | "save_file" => (Kind::File, CardKind::Plan),
        "propose_task" | "add_task" | "create_task" | "propose_plan" => {
            (Kind::Task, CardKind::Plan)
        }
        "propose_agent" | "start_agent" | "start_job" => (Kind::Agent, CardKind::Delegation),
        "propose_delegation" | "delegate" => (Kind::Delegation, CardKind::Delegation),
        _ => return None,
    };
    let effect = kind_of.word();
    let to = field("to");
    let default_label = match (effect, to.as_deref()) {
        ("message", Some(to)) => w(&format!("Message à {to}"), &format!("Message to {to}")),
        ("message", None) => w("Message", "Message"),
        ("post", _) => w("Publication", "Post"),
        ("mail", Some(to)) => w(&format!("Mail à {to}"), &format!("Mail to {to}")),
        ("mail", None) => w("Mail", "Mail"),
        ("invitation", _) => w("Réponse à l’invitation", "Answer to the invitation"),
        ("event", _) => w("Un événement", "An event"),
        ("file", _) => w("Un fichier", "A file"),
        ("task", _) => w("À faire", "To do"),
        _ => w("Confier à Claude Code", "Hand to Claude Code"),
    };
    let accept = match effect {
        "message" | "post" | "mail" => w("Envoyer", "Send"),
        "invitation" => w("Répondre", "Answer"),
        "event" => w("Ajouter", "Add"),
        "file" => w("Enregistrer", "Save"),
        "task" => w("Mettre dans À faire", "Put in To do"),
        _ => w("Lancer", "Start"),
    };
    let mut lines = list("lines");
    if lines.is_empty()
        && let Some(text) = field("draft").or_else(|| field("body"))
    {
        lines.push(text);
    }
    let action = |label: String, decision| Action { label, decision };
    Some(Made::Card(
        Card {
            kind,
            label: field("label").unwrap_or(default_label),
            text: field("text").unwrap_or_default(),
            lines,
            actions: vec![
                action(accept, Decision::Accept),
                action(w("Modifier", "Edit"), Decision::Edit),
                action(w("Annuler", "Cancel"), Decision::Reject),
            ],
            decided: None,
            outcome: None,
            sheet: None,
        },
        kind_of,
    ))
}

/// A source's name as her status line says it.
fn shown_source(name: &str) -> String {
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

/// Common words of five letters or more, folded, that are never a
/// project's own word: what entity linking (lot 4) will know better.
const COMMON: &[&str] = &[
    "aujourd",
    "demain",
    "toujours",
    "quelque",
    "comment",
    "pourquoi",
    "avant",
    "apres",
    "cette",
    "votre",
    "notre",
    "entre",
    "aussi",
    "encore",
    "faire",
    "finir",
    "integrer",
    "ticket",
    "tickets",
    "article",
    "blog",
    "reunion",
    "reunions",
    "matin",
    "soir",
    "semaine",
    "journee",
    "projet",
    "projets",
    "client",
    "clients",
    "equipe",
    "point",
    "lundi",
    "mardi",
    "mercredi",
    "jeudi",
    "vendredi",
    "samedi",
    "dimanche",
    "merci",
    "bonjour",
    "bonsoir",
    "rappelle",
    "rappeler",
    "envoie",
    "envoyer",
    "message",
    "messages",
    "mails",
    "prepare",
    "preparer",
    "regarde",
    "regarder",
    "besoin",
    "plutot",
    "depuis",
    "pendant",
    "parce",
    "alors",
    "ensuite",
    "today",
    "tomorrow",
    "morning",
    "evening",
    "meeting",
    "meetings",
    "please",
    "thanks",
    "should",
    "would",
    "could",
    "about",
    "after",
    "before",
    "their",
    "there",
    "these",
    "those",
    "which",
    "where",
    "while",
    "write",
    "remind",
    "finish",
    "project",
    "projects",
    "people",
    "review",
    "relire",
    "relecture",
    "tarifs",
    "arrondis",
    "travail",
    "travaille",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_acting_tool_is_a_proposal_all_the_same() {
        for (name, effect) in [
            ("send_message", "message"),
            ("post", "post"),
            ("write_file", "file"),
            ("add_task", "task"),
            ("start_job", "agent"),
            ("propose_delegation", "delegation"),
        ] {
            let call = ToolCall {
                name: name.to_owned(),
                input: serde_json::json!({"to": "Léa", "draft": "Je regarde ça."}),
                ..ToolCall::default()
            };
            let Some(Made::Card(card, made_effect)) = made(&call, true) else {
                panic!("{name}: no card");
            };
            assert_eq!(made_effect.word(), effect);
            assert!(card.actions.len() <= 3);
            assert_eq!(card.actions[0].decision, Decision::Accept);
        }
        let unknown = ToolCall {
            name: "rm_rf".to_owned(),
            input: serde_json::Value::Null,
            ..ToolCall::default()
        };
        assert_eq!(made(&unknown, true), None);
    }
}
