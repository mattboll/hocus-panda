//! Her loop: one thread that owns her memory and takes, one at a time, the
//! lines of her connections, what her sources read, and what her models
//! answered. It never waits on any of them: sources sync on threads of
//! their own, model calls run on worker threads, and each connection has a
//! writer thread, so a slow reader or a model taking thirty seconds holds
//! nothing up.
//!
//! **Always on** (§3.10): her sources sync on their schedules whatever the
//! session does, locked screen included (she only notes it: she never
//! speaks while locked, which the shell enforces). After a start or a
//! resume, every source syncs at once from its cursor (the catch-up); the
//! time she was away is measured from the last time she was alive (a
//! start) or told (a resume; else `CLOCK_BOOTTIME` against
//! `CLOCK_MONOTONIC`), and over four hours a "Pendant ton absence"
//! briefing is due. At rest, with no source due, she sleeps until the
//! next due sync: no wakeup in between.
//!
//! **The models** (§11, lot 4): an item a source gave with its text is
//! held in her process ([`Inbox`]) and queued for the local triage (the
//! secrets' second pass); her loop steps the queue ([`triage::Queue`]),
//! whose requests run on worker threads. A question's request is
//! assembled within its budget from her memory and what may leave
//! ([`context`]).
//!
//! **Notifications** (lot 7): what the shell forwards of the applications
//! consented to goes to her notifications' source ([`Arrivals`]), due at
//! once; it reads them on its worker like any source. Nothing of hers
//! answers a notification: no line goes back to the shell about one.
//!
//! **The conversation** (§5.4, lot 2) is [`conversation`]'s: the thread,
//! her questions, her model's streamed answer, the proposals' cards.
//!
//! **Her speaking up** (§6, lot 5) is [`speaking`]'s: the briefings at a
//! session's start, her requests to the shell's gate, her peek's answers,
//! her reminders (her loop wakes for the next one), the commitments the
//! triage finds in mails the user sent, and the effects of accepted cards
//! ([`crate::effects`]).

mod consent;
mod conversation;
mod keys;
mod manner;
mod memory;
mod queue;
mod speaking;

pub use conversation::Said;

use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::Duration,
};

use hocus_secretary_link::{
    CatchUp, Client, ConnectionStatus, FromSecretary, Graph, GraphEdge, GraphNode, GraphQuery,
    SourceStatus, Status, ToSecretary, TriageStatus, VERSION, line, read,
};
use hocus_sources::{Quoted, Redactor};
use jiff::{SignedDuration, Timestamp, civil::Date};

use crate::{
    audit,
    clock::{Clock, Sleeps},
    context::{self, Held, Inbox, Scope},
    graph::{NewEpisode, Target},
    model::{Message, Model, Recorder, Reply, Request, Role},
    notifications::{self, Arrivals},
    power::Sensors,
    settings,
    sources::{Source, Synced},
    store::{Store, stamp, unstamp},
    triage::{self, Queue, Step, Triager, Verdict},
};

/// Away longer than this, a "Pendant ton absence" briefing is due.
pub const AWAY_BRIEFING: Duration = Duration::from_secs(4 * 3600);

/// What reaches her loop.
#[derive(Debug)]
pub enum Event {
    /// A connection: what writes to it, and what closes it (none in
    /// tests that talk to her loop directly).
    Connected {
        conn: u64,
        writer: Sender<String>,
        closer: Option<UnixStream>,
    },
    Line {
        conn: u64,
        line: String,
    },
    Closed {
        conn: u64,
    },
    Synced {
        source: usize,
        result: Result<Synced, String>,
    },
    /// A piece of her model's answer to request `id`, as it comes.
    Piece {
        conn: u64,
        id: u64,
        piece: Said,
    },
    Called {
        conn: u64,
        id: u64,
        role: Role,
        items: Vec<String>,
        started: Timestamp,
        result: Result<Reply, String>,
    },
    /// Her model worded a briefing.
    Briefed {
        built: Box<crate::briefing::Built>,
        away: bool,
        items: Vec<String>,
        started: Timestamp,
        result: Result<Reply, String>,
    },
    /// The local triage read an item; where its server holds the model
    /// now, when it says.
    Triaged {
        pointer: String,
        started: Timestamp,
        result: Result<(Verdict, u64, u64), String>,
        placement: Option<crate::providers::openai::Placement>,
    },
    /// The triage's steady heat reading changed: too hot, or not (#756).
    Heat(Option<crate::heat::Hot>),
    /// A summary's words came back from her compaction role's model.
    Compacted {
        work: Box<crate::compaction::Work>,
        started: Timestamp,
        result: Result<Reply, String>,
    },
    /// Whether the keyring gives her answers' provider's key: the
    /// provider missing one, or none.
    Keyed(Option<String>),
    /// A file of hers changed: her consent book, or the configuration
    /// (#752).
    Changed(crate::watch::Watched),
    /// A key given was checked and kept (or not).
    KeyDone {
        conn: u64,
        id: u64,
        provider: String,
        outcome: hocus_secretary_link::KeyOutcome,
        digest: Option<u64>,
    },
    Stop,
}

/// What she is given to run.
#[derive(Debug)]
pub struct Options {
    pub model: Option<Arc<dyn Model>>,
    pub sources: Vec<Box<dyn Source>>,
    /// Where her memory's key is, for her status.
    pub key: String,
    /// Where the outside effects of accepted cards are written (tests):
    /// none, nothing can act yet.
    pub effects: Option<Arc<Recorder>>,
    /// The local triage; none: items' texts stay held, unchecked (only
    /// their metadata may leave).
    pub triage: Option<Arc<dyn Triager>>,
    /// Her settings, read again when they change.
    pub settings: Arc<settings::File>,
    /// Where power and heat are read.
    pub sensors: Sensors,
    /// Her language, and what calls may hold.
    pub scope: Scope,
    /// Where the notifications the shell forwards go: her notifications'
    /// source's, among `sources`. None: they are dropped.
    pub notifications: Option<Arrivals>,
    /// Where "Exporter" writes her memory (her folder's `exports`); none:
    /// no export (tests).
    pub exports: Option<std::path::PathBuf>,
    /// Her consent book (her folder's `consents.json`), which her consent
    /// cards write; none: no consent from the drawer.
    pub consents: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            model: None,
            sources: Vec::new(),
            key: String::new(),
            triage: None,
            settings: Arc::new(settings::File::fixed(settings::Settings::default())),
            sensors: Sensors::default(),
            scope: Scope::default(),
            effects: None,
            notifications: None,
            exports: None,
            consents: None,
        }
    }
}

#[derive(Debug)]
struct Connection {
    writer: Sender<String>,
    closer: Option<UnixStream>,
    client: Option<Client>,
    sent: u64,
}

#[derive(Debug)]
struct Slot {
    name: String,
    notebook: hocus_secretary_link::Notebook,
    every: Duration,
    asks: Sender<Option<String>>,
    due: Timestamp,
    busy: bool,
    /// Asked again as soon as its sync under way ends: something arrived
    /// for it meanwhile.
    again: bool,
    items: u64,
    error: Option<String>,
}

/// A catch-up under way.
#[derive(Debug)]
struct Catching {
    gap: Duration,
    waiting: usize,
    items: u64,
}

#[derive(Debug)]
pub struct Brain {
    store: Store,
    clock: Clock,
    model: Option<Arc<dyn Model>>,
    slots: Vec<Slot>,
    conns: BTreeMap<u64, Connection>,
    events: Receiver<Event>,
    sender: Sender<Event>,
    started: Timestamp,
    locked: bool,
    wakes: u64,
    calls_running: u32,
    catching: Option<Catching>,
    caught_up: Option<CatchUp>,
    sleeps: Sleeps,
    key: String,
    talk: conversation::Talk,
    speaking: speaking::Speaking,
    effectors: crate::effects::Effectors,
    triager: Option<Arc<dyn Triager>>,
    queue: Queue,
    inbox: Inbox,
    settings: Arc<settings::File>,
    sensors: Sensors,
    scope: Scope,
    redactor: Redactor,
    arrivals: Option<Arrivals>,
    /// "Ce qu'elle sait", what she learns, compaction (lot 6).
    memory: memory::Memory,
    /// Her sources' consent in the drawer, her first run (#750).
    consents: consent::Consents,
    /// The provider whose key her answers need and the keyring does not
    /// give; whether it is being asked.
    key_missing: Option<String>,
    key_asked: bool,
    /// The senders whose mails are set aside, as her rules say (#754).
    aside_rules: Vec<String>,
    /// The connections with her queue's view open: its request, and the
    /// view as last sent.
    queue_watch: BTreeMap<u64, (u64, hocus_secretary_link::queue::View)>,
    /// Where the local model was held after its last request (Ollama's
    /// `/api/ps`, asked on the request's thread).
    placement: Option<crate::providers::openai::Placement>,
    /// The heat's sampler (#756): what it was last told to watch, and
    /// its verdict.
    heat_watch: Option<Sender<Option<crate::heat::Watch>>>,
    heat_sent: Option<crate::heat::Watch>,
    heat: Option<crate::heat::Hot>,
}

impl Brain {
    /// Her loop, with her memory, its events' channel, and her sources'
    /// workers started; a catch-up from her last time alive under way.
    pub fn new(store: Store, options: Options) -> Self {
        let (sender, events) = mpsc::channel();
        let clock = store.clock().clone();
        let now = clock.now();
        let mut slots = Vec::new();
        for (index, source) in options.sources.into_iter().enumerate() {
            slots.push(spawn_source(
                index,
                source,
                sender.clone(),
                clock.clone(),
                now,
            ));
        }
        let away = store
            .meta("alive")
            .ok()
            .flatten()
            .and_then(|t| unstamp(&t))
            .map(|alive| Duration::try_from(now.duration_since(alive)).unwrap_or(Duration::ZERO));
        let mut brain = Self {
            store,
            clock,
            model: options.model,
            slots,
            conns: BTreeMap::new(),
            events,
            sender,
            started: now,
            locked: false,
            wakes: 0,
            calls_running: 0,
            catching: None,
            caught_up: None,
            sleeps: Sleeps::now(),
            key: options.key,
            talk: conversation::Talk::default(),
            speaking: speaking::Speaking::default(),
            effectors: crate::effects::Effectors::new(options.effects),
            triager: options.triage,
            queue: Queue::new(now),
            inbox: Inbox::default(),
            settings: options.settings,
            sensors: options.sensors,
            scope: options.scope,
            redactor: Redactor::new(),
            arrivals: options.notifications,
            memory: memory::Memory::new(options.exports),
            consents: consent::Consents {
                book: options.consents,
                ..consent::Consents::default()
            },
            key_missing: None,
            key_asked: false,
            aside_rules: Vec::new(),
            queue_watch: BTreeMap::new(),
            placement: None,
            heat_watch: None,
            heat_sent: None,
            heat: None,
        };
        // The heat sampled off her loop, for the triage only.
        if brain.triager.is_some() {
            let (watches, watched) = mpsc::channel();
            let sender = brain.sender.clone();
            match crate::heat::spawn(
                brain.sensors.clone(),
                brain.clock.clone(),
                watched,
                move |hot| sender.send(Event::Heat(hot)).is_ok(),
            ) {
                Ok(()) => brain.heat_watch = Some(watches),
                Err(err) => tracing::warn!(%err, "no thread to sample the heat"),
            }
        }
        // The senders set aside, before the items waiting come back.
        brain.aside_rules =
            crate::learn::set_aside(&brain.store, brain.store.today()).unwrap_or_default();
        // Her consent book and her settings watched: a change is taken at
        // once (#752).
        let watched: Vec<(crate::watch::Watched, std::path::PathBuf)> = brain
            .consents
            .book
            .clone()
            .map(|b| (crate::watch::Watched::Consents, b))
            .into_iter()
            .chain(
                brain
                    .settings
                    .path()
                    .map(|p| (crate::watch::Watched::Config, p.to_path_buf())),
            )
            .collect();
        crate::watch::start(watched, brain.sender.clone(), Event::Changed);
        // What she kept across a restart: her thread, the items waiting
        // for the triage (#750).
        brain.restore();
        brain.consents_restored();
        // A source's notebook changed while she was away (#752).
        brain.refile();
        brain.ask_key();
        if let Some(away) = away {
            brain.catch_up(away);
        }
        brain.alive();
        brain
    }

    /// What sends to her loop.
    pub fn sender(&self) -> Sender<Event> {
        self.sender.clone()
    }

    /// Runs until stopped, or until nothing can reach her anymore.
    pub fn run(mut self) -> Store {
        loop {
            self.ask_due();
            self.triage_step();
            // The triage paused, or going again: her status says it, and
            // her queue's view while open (#754).
            self.tell_look();
            self.queue_refresh();
            self.speak_due();
            self.memory_due();
            let triage = self.settings.current().triage;
            let next = self
                .slots
                .iter()
                .filter(|s| !s.busy)
                .map(|s| s.due)
                .chain(self.queue.next(&triage))
                .chain(self.speaking_next())
                .chain(self.memory_next())
                .min();
            let event = match next.and_then(|at| self.clock.wait_until(at)) {
                Some(wait) => match self.events.recv_timeout(wait) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match self.events.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                },
            };
            self.wakes += 1;
            if let Some(event) = event
                && !self.handle(event)
            {
                break;
            }
        }
        self.alive();
        self.store
    }

    /// Asks every source that is due to sync.
    fn ask_due(&mut self) {
        let now = self.clock.now();
        for slot in self.slots.iter_mut().filter(|s| !s.busy && s.due <= now) {
            let cursor = self
                .store
                .cursor(&slot.name)
                .ok()
                .flatten()
                .map(|(cursor, _)| cursor);
            if slot.asks.send(cursor).is_ok() {
                slot.busy = true;
            } else {
                // Its worker is gone: never due again.
                slot.due = Timestamp::MAX;
            }
        }
    }

    /// Writes down that she is alive now.
    fn alive(&self) {
        if let Err(err) = self.store.set_meta("alive", &stamp(self.clock.now())) {
            tracing::warn!(%err, "could not note the time");
        }
    }

    /// Every source syncs now, after an absence of `away`.
    fn catch_up(&mut self, away: Duration) {
        let now = self.clock.now();
        for slot in &mut self.slots {
            slot.due = slot.due.min(now);
        }
        tracing::info!(?away, sources = self.slots.len(), "catching up");
        self.catching = Some(Catching {
            gap: away,
            waiting: self
                .slots
                .iter()
                .filter(|s| s.due != Timestamp::MAX)
                .count(),
            items: 0,
        });
        self.catch_up_done_if_over();
    }

    fn catch_up_done_if_over(&mut self) {
        let Some(catching) = self.catching.as_ref().filter(|c| c.waiting == 0) else {
            return;
        };
        let away = catching.gap >= AWAY_BRIEFING;
        let caught = CatchUp {
            at: self.clock.now().to_string(),
            gap_seconds: catching.gap.as_secs(),
            items: catching.items,
            away_briefing: away,
        };
        tracing::info!(?caught, "caught up");
        if away {
            // Told at the next session: what arrived meanwhile is in the
            // episodes since.
            let (gap, items) = (catching.gap, catching.items);
            self.away_for(gap);
            let hours = gap.as_secs() / 3600;
            let episode = NewEpisode {
                at: self.clock.now(),
                kind: "catch-up".to_owned(),
                digest: format!("absente {hours} h, {items} éléments lus au retour"),
                source: None,
                pointer: format!("catch-up:{}", stamp(self.clock.now())),
                notebook: hocus_secretary_link::Notebook::Pro,
            };
            if let Err(err) = self.store.add_episode(&episode, &[]) {
                tracing::warn!(%err, "could not note the catch-up");
            }
        }
        self.caught_up = Some(caught);
        self.catching = None;
        self.try_brief();
    }

    /// Takes one event: whether to go on.
    fn handle(&mut self, event: Event) -> bool {
        let changes = matches!(
            event,
            Event::Line { .. }
                | Event::Synced { .. }
                | Event::Triaged { .. }
                | Event::Compacted { .. }
        );
        let line = matches!(event, Event::Line { .. });
        let go_on = self.take(event);
        if line {
            // A rule setting a sender aside, given or taken back (#754).
            self.aside_again();
        }
        if changes {
            self.knows_refresh();
        }
        self.keep_proposals();
        go_on
    }

    /// Takes one event: whether to go on.
    fn take(&mut self, event: Event) -> bool {
        match event {
            Event::Connected {
                conn,
                writer,
                closer,
            } => {
                self.conns.insert(
                    conn,
                    Connection {
                        writer,
                        closer,
                        client: None,
                        sent: 0,
                    },
                );
            }
            Event::Line { conn, mut line } => {
                self.said(conn, &line);
                // A line may hold a key (#752): not kept in memory.
                if line.contains("\"key\"") {
                    hocus_secretary_link::wipe(&mut line);
                }
            }
            Event::Closed { conn } => {
                self.conns.remove(&conn);
            }
            Event::Synced { source, result } => self.synced(source, result),
            Event::Piece { conn, id, piece } => self.piece(conn, id, piece),
            Event::Called {
                conn,
                id,
                role,
                items,
                started,
                result,
            } => self.called(conn, id, role, items, started, result),
            Event::Triaged {
                pointer,
                started,
                result,
                placement,
            } => {
                if placement.is_some() {
                    self.placement = placement;
                }
                self.triaged(&pointer, started, result);
                self.try_brief();
            }
            Event::Briefed {
                built,
                away,
                items,
                started,
                result,
            } => self.worded(*built, away, items, started, result),
            Event::Heat(hot) => {
                // Told before it was told to rest: stale.
                if self.heat_sent.is_some() {
                    if hot != self.heat {
                        tracing::info!(?hot, "the triage's heat");
                    }
                    self.heat = hot;
                    self.queue.heat(self.clock.now(), hot);
                }
            }
            Event::Compacted {
                work,
                started,
                result,
            } => self.compacted(*work, started, result),
            Event::Keyed(missing) => {
                self.key_asked = false;
                if missing != self.key_missing {
                    tracing::info!(missing = ?missing, "her answers' key");
                    self.key_missing = missing;
                    self.tell_look();
                }
                // Her first run waits for this answer: her key first.
                self.first_run_step();
            }
            Event::Changed(crate::watch::Watched::Consents) => self.consents_changed(),
            Event::Changed(crate::watch::Watched::Config) => {
                tracing::info!("her settings changed");
                // Her name, her restraint, at once.
                self.tell_look();
                self.tell_restraint();
            }
            Event::KeyDone {
                conn,
                id,
                provider,
                outcome,
                digest,
            } => self.key_done(conn, id, provider, outcome, digest),
            Event::Stop => return false,
        }
        true
    }

    fn send(&mut self, conn: u64, message: &FromSecretary) {
        if let Some(c) = self.conns.get_mut(&conn) {
            if c.writer.send(line(message)).is_ok() {
                c.sent += 1;
            } else {
                self.conns.remove(&conn);
            }
        }
    }

    fn error(&mut self, conn: u64, id: Option<u64>, message: impl Into<String>) {
        self.send(
            conn,
            &FromSecretary::Error {
                id,
                message: message.into(),
            },
        );
    }

    /// A line from a connection.
    fn said(&mut self, conn: u64, text: &str) {
        let message = match read::<ToSecretary>(text) {
            Ok(message) => message,
            // A key's line never said back, even in part.
            Err(_) if text.contains("\"key\"") => {
                tracing::info!(conn, "a key's line that does not read");
                return self.error(conn, None, "a key's line that does not read");
            }
            Err(err) => {
                tracing::info!(conn, %err, "a line that does not read");
                return self.error(conn, None, format!("a line that does not read: {err}"));
            }
        };
        let greeted = self.conns.get(&conn).is_some_and(|c| c.client.is_some());
        match message {
            ToSecretary::Hello {
                version,
                client,
                lang,
            } => {
                if version != VERSION {
                    self.send(
                        conn,
                        &FromSecretary::Refused {
                            reason: format!("protocol {version}; she speaks {VERSION}"),
                        },
                    );
                    if let Some(c) = self.conns.remove(&conn)
                        && let Some(closer) = c.closer
                    {
                        let _ = closer.shutdown(std::net::Shutdown::Both);
                    }
                    return;
                }
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.client = Some(client);
                }
                tracing::info!(conn, ?client, "a client said hello");
                self.send(conn, &FromSecretary::Hello { version: VERSION });
                self.greeted(conn, client, lang.as_deref());
            }
            _ if !greeted => self.error(conn, None, "hello first"),
            ToSecretary::Status { id } => {
                let status = Box::new(self.status());
                self.send(conn, &FromSecretary::Status { id, status });
            }
            ToSecretary::Graph { id, query } => match self.graph(&query) {
                Ok(graph) => self.send(conn, &FromSecretary::Graph { id, graph }),
                Err(err) => self.error(conn, Some(id), err),
            },
            ToSecretary::Ask { id, text } => self.sentence(conn, id, text),
            ToSecretary::RemindMe { id, item, pointer } => self.remind_me(conn, id, item, &pointer),
            ToSecretary::Choose { id, item, choice } => self.choose(conn, id, item, choice),
            ToSecretary::Decide {
                id,
                item,
                decision,
                notebook,
                left_out,
                off,
            } => {
                if !self.consent_decide(conn, id, item, decision, notebook, left_out, off) {
                    self.decide(conn, id, item, decision);
                }
            }
            ToSecretary::Undo { id, item } => {
                if !self.too_late(conn, id, item) {
                    self.undo(conn, id, item);
                }
            }
            ToSecretary::Thread { id } => self.thread(conn, id),
            ToSecretary::Unfolded { item } => self.unfolded(item),
            ToSecretary::Drawer { open } => self.drawer(open),
            ToSecretary::Session { locked } => {
                tracing::info!(locked, "the session's lock");
                self.session(conn, locked);
            }
            ToSecretary::Resumed { slept } => {
                let now = Sleeps::now();
                let measured = self.sleeps.slept_since(now);
                self.sleeps = now;
                let slept = match slept {
                    Some(seconds) => {
                        let slept = Duration::from_secs(seconds);
                        // A clock of her own sleeps too.
                        self.clock.advance(slept);
                        slept
                    }
                    None => measured,
                };
                self.catch_up(slept);
                self.alive();
            }
            ToSecretary::Clock { advance } => {
                if !self.clock.advance(Duration::from_secs(advance)) {
                    self.error(conn, None, "she runs on the wall clock");
                }
            }
            ToSecretary::Notification(notification) => self.notified(notification),
            ToSecretary::Spoken { speak, outcome } => self.spoken(speak, outcome),
            ToSecretary::Peeked { id, speak, answer } => self.peeked(conn, id, speak, answer),
            ToSecretary::Moment { moment } => {
                tracing::info!(?moment, "the moment");
                self.scope.moment = match moment {
                    hocus_secretary_link::Moment::Quiet => context::Moment::Quiet,
                    hocus_secretary_link::Moment::Meeting => context::Moment::Meeting,
                    hocus_secretary_link::Moment::Sharing => context::Moment::Sharing,
                    hocus_secretary_link::Moment::Camera => context::Moment::Camera,
                };
                // "Ce qu'elle sait" hides, or shows again, the perso.
                self.knows_refresh();
            }
            ToSecretary::Effected { effect, ok, detail } => self.effected(effect, ok, &detail),
            ToSecretary::Knows { id, query } => self.knows(conn, id, query),
            ToSecretary::Memory { id, act } => self.memory_act(conn, id, act),
            ToSecretary::Key { id, provider, key } => self.key_given(conn, id, provider, key),
            ToSecretary::Queue { id, open } => self.queue_open(conn, id, open),
            ToSecretary::QueueAct { id, act } => self.queue_act(conn, id, act),
        }
    }

    /// A notification the shell forwarded: to her notifications' source,
    /// which reads it now.
    fn notified(&mut self, notification: hocus_secretary_link::Notification) {
        let Some(arrivals) = &self.arrivals else {
            tracing::debug!("a notification, and no source to read it: dropped");
            return;
        };
        arrivals.push(notification);
        let now = self.clock.now();
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|s| s.name == notifications::NAME)
        {
            if slot.busy {
                slot.again = true;
            } else {
                slot.due = slot.due.min(now);
            }
        }
    }

    fn status(&self) -> Status {
        let sources = self
            .slots
            .iter()
            .map(|slot| {
                let cursor = self.store.cursor(&slot.name).ok().flatten();
                SourceStatus {
                    name: slot.name.clone(),
                    notebook: Some(slot.notebook),
                    cursor: cursor.as_ref().map(|(c, _)| c.clone()),
                    synced_at: cursor.map(|(_, at)| at),
                    items: slot.items,
                    busy: slot.busy,
                    error: slot.error.clone(),
                }
            })
            .collect();
        Status {
            version: VERSION,
            now: self.clock.now().to_string(),
            started: self.started.to_string(),
            virtual_clock: self.clock.is_virtual(),
            locked: self.locked,
            key: self.key.clone(),
            counts: self.store.counts().unwrap_or_default(),
            sources,
            catch_up: self.caught_up.clone(),
            connections: self
                .conns
                .values()
                .map(|c| ConnectionStatus {
                    client: c.client,
                    sent: c.sent,
                })
                .collect(),
            wakes: self.wakes,
            model: self
                .model
                .as_ref()
                .map(|m| format!("{}/{}", m.provider(), m.model()))
                .unwrap_or_default(),
            calls_running: self.calls_running,
            triage: self.triager.as_ref().map(|t| {
                let (provider, model) = t.endpoint();
                TriageStatus {
                    state: match &self.queue.state {
                        triage::State::Idle { loaded: false, .. } => "idle".to_owned(),
                        triage::State::Idle { loaded: true, .. } => "loaded".to_owned(),
                        triage::State::Running(_) => "running".to_owned(),
                        triage::State::Resting { .. } => "resting".to_owned(),
                        triage::State::Paused { why, .. } => format!("paused: {}", why.word()),
                    },
                    model: format!("{provider}/{model}"),
                    pending: self.queue.pending() as u64,
                    done: self.queue.done,
                    given_up: self.queue.given_up,
                    held: self.inbox.len() as u64,
                    // As last asked, on the triage's thread: never on her
                    // loop.
                    placement: self.placement.map(|p| p.words(self.talk.fr()).to_owned()),
                    set_aside: self.queue.aside_len() as u64,
                }
            }),
            key_provider: self
                .model
                .as_ref()
                .filter(|m| m.needs_key())
                .and_then(|m| m.key_provider()),
            key_missing: self.key_missing.is_some(),
        }
    }

    fn graph(&self, query: &GraphQuery) -> Result<Graph, String> {
        let day = match query.as_of.as_deref() {
            None => self.store.today(),
            Some(text) => as_of(text).ok_or_else(|| format!("{text:?} is not a date"))?,
        };
        let Some(start) = self
            .store
            .find(&query.node)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
        else {
            return Err(format!("she knows no {:?}", query.node));
        };
        let around = self
            .store
            .neighbourhood(start, query.hops, day, query.notebook)
            .map_err(|e| e.to_string())?;
        Ok(Graph {
            as_of: day.to_string(),
            nodes: around
                .nodes
                .into_iter()
                .map(|n| GraphNode {
                    id: n.id,
                    kind: n.kind.word().to_owned(),
                    name: n.name,
                    notebook: n.notebook,
                })
                .collect(),
            edges: around
                .edges
                .into_iter()
                .map(|e| {
                    let (dst, value) = match e.target {
                        Target::Node(id) => (Some(id), None),
                        Target::Value(v) => (None, Some(v)),
                    };
                    GraphEdge {
                        id: e.id,
                        src: e.src,
                        kind: e.kind,
                        dst,
                        value,
                        valid_from: e.valid_from.map(|d| d.to_string()),
                        valid_to: e.valid_to.map(|d| d.to_string()),
                        provenance: e.provenance,
                        pointer: e.pointer,
                        notebook: e.notebook,
                    }
                })
                .collect(),
        })
    }

    fn called(
        &mut self,
        conn: u64,
        id: u64,
        role: Role,
        items: Vec<String>,
        started: Timestamp,
        result: Result<Reply, String>,
    ) {
        self.calls_running = self.calls_running.saturating_sub(1);
        let took = Duration::try_from(self.clock.now().duration_since(started)).unwrap_or_default();
        let (provider, model) = match &result {
            Ok(reply) if !reply.provider.is_empty() => {
                (Some(reply.provider.clone()), Some(reply.model.clone()))
            }
            _ => self
                .model
                .as_ref()
                .map(|m| (m.provider().to_owned(), m.model().to_owned()))
                .unzip(),
        };
        let entry = audit::Entry {
            kind: audit::Kind::Call,
            what: role.word().to_owned(),
            ids: items,
            provider,
            model,
            tokens_in: result.as_ref().ok().map(|r| r.tokens_in),
            tokens_out: result.as_ref().ok().map(|r| r.tokens_out),
            duration_ms: Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX)),
            outcome: Some(if result.is_ok() { "ok" } else { "failed" }.to_owned()),
        };
        if let Err(err) = self.store.audit(entry) {
            tracing::warn!(%err, "could not audit a call");
        }
        self.replied(conn, id, result);
    }

    /// What a source read: each item an episode, in its notebook; the
    /// read audited by ids; its cursor kept.
    fn synced(&mut self, index: usize, result: Result<Synced, String>) {
        let now = self.clock.now();
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        slot.busy = false;
        let every = SignedDuration::try_from(slot.every).unwrap_or(SignedDuration::MAX);
        slot.due = if std::mem::take(&mut slot.again) {
            now
        } else {
            now.checked_add(every).unwrap_or(Timestamp::MAX)
        };
        let (name, notebook) = (slot.name.clone(), slot.notebook);
        let mut read = 0;
        let mut found = None;
        let mut calendars = None;
        match result {
            Ok(mut synced) => {
                slot.error = None;
                slot.items += synced.items.len() as u64;
                read = synced.items.len() as u64;
                found = synced.found.take();
                calendars = synced.calendars.take();
                for gone in &synced.forget {
                    tracing::info!(source = gone, "a source revoked: what came from it goes");
                    if let Err(err) = self.store.forget_source(gone) {
                        tracing::warn!(source = gone, %err, "could not forget a source");
                    }
                    self.inbox.forget_source(gone);
                    self.queue.forget_source(gone);
                }
                let mut ids = Vec::new();
                for item in synced.items {
                    let source = item.from.clone().unwrap_or_else(|| name.clone());
                    let pointer = format!("{source}:{}", item.id);
                    let notebook = item.notebook.unwrap_or(notebook);
                    // A board's card: linked to its project's node.
                    let mentions: Vec<i64> = item
                        .project
                        .as_deref()
                        .and_then(|p| self.project_node(p, notebook, &name))
                        .into_iter()
                        .collect();
                    let episode = NewEpisode {
                        at: item.at,
                        kind: item.kind.clone(),
                        digest: item.digest.clone(),
                        source: Some(source),
                        pointer: pointer.clone(),
                        notebook,
                    };
                    self.keep_day(&item, &pointer, notebook);
                    match self.store.add_episode(&episode, &mentions) {
                        Ok((episode, _)) => {
                            if (item.until.is_some()
                                || item.open.is_some()
                                || item.calendar.is_some())
                                && let Err(err) = self.store.set_episode_extras(
                                    episode,
                                    item.until,
                                    item.open.as_deref(),
                                    item.calendar.as_deref(),
                                )
                            {
                                tracing::warn!(source = name, %err, "could not keep an item's extras");
                            }
                            // What it says of the team (lot 6): proposals.
                            self.seen_in_source(episode, &item.digest, item.at);
                            if let Some(text) = item.text {
                                let from = item.from.as_deref().unwrap_or(&name);
                                self.hold(
                                    pointer.clone(),
                                    from,
                                    *text,
                                    episode,
                                    item.summaries_only,
                                );
                            }
                        }
                        Err(err) => tracing::warn!(source = name, %err, "could not keep an item"),
                    }
                    ids.push(pointer);
                }
                if !ids.is_empty() {
                    let entry = audit::Entry {
                        kind: audit::Kind::Read,
                        what: name.clone(),
                        ids,
                        ..Default::default()
                    };
                    if let Err(err) = self.store.audit(entry) {
                        tracing::warn!(%err, "could not audit a read");
                    }
                }
                if let Err(err) = self.store.set_cursor(&name, &synced.cursor) {
                    tracing::warn!(source = name, %err, "could not keep the cursor");
                }
            }
            Err(err) => {
                tracing::info!(source = name, %err, "a sync failed");
                slot.error = Some(err);
            }
        }
        if let Some(catching) = &mut self.catching {
            catching.waiting = catching.waiting.saturating_sub(1);
            catching.items += read;
        }
        self.catch_up_done_if_over();
        self.try_brief();
        // A sender that looks automated, its mails piling up (#754).
        self.notice_automated();
        if let Some(calendars) = calendars {
            self.calendars_found(calendars);
        }
        if let Some(accounts) = found {
            self.accounts_found(accounts);
        }
        // A source failing, or answering again: her face says it.
        self.tell_look();
        if let Err(err) = self.store.prune_audit() {
            tracing::warn!(%err, "could not prune the audit log");
        }
        self.alive();
    }
}

impl Brain {
    /// A call's request for `role`: the conversation `messages`, its
    /// context assembled within the budget (§7.3).
    pub fn request(&self, role: Role, messages: &[Message]) -> Result<Request, String> {
        self.request_in(role, messages, &self.scope)
    }

    /// The same, within `scope` (a work draft: the pro notebook only).
    pub fn request_in(
        &self,
        role: Role,
        messages: &[Message],
        scope: &Scope,
    ) -> Result<Request, String> {
        let mut request = context::assemble(
            &self.store,
            &self.inbox,
            role,
            messages,
            scope,
            self.clock.now(),
        )
        .map_err(|e| format!("her memory: {e}"))?;
        // Her name, as the user gave it (#752): the first line of her
        // persona.
        let name = self.settings.current().name;
        let name = name.trim();
        if !name.is_empty() {
            let line = match scope.language {
                context::Language::French => format!("Tu t'appelles {name}."),
                context::Language::English => format!("Your name is {name}."),
            };
            request.system = format!("{line}\n{}", request.system);
        }
        Ok(request)
    }

    /// An item with its text, held and queued for the triage; its text
    /// kept, encrypted, until the triage checked it (a restart loses
    /// nothing, #750).
    fn hold(
        &mut self,
        pointer: String,
        source: &str,
        text: hocus_sources::Redacted,
        episode: i64,
        only: bool,
    ) {
        let held = Held {
            redacted: text,
            episode,
            summaries_only: only,
            summary: None,
            suspicious: false,
        };
        if held.redacted.check == hocus_sources::Check::FirstPass
            && let Err(err) = crate::kept::keep_pending(&self.store.conn, &pointer, source, &held)
        {
            tracing::warn!(pointer, %err, "could not keep an item waiting for the triage");
        }
        self.inbox.hold(pointer.clone(), held);
        self.enqueue(pointer);
    }

    /// What she kept across a restart: her thread (its cards' proposals
    /// with it), and the items still waiting for the triage, queued again
    /// with their text.
    fn restore(&mut self) {
        match crate::kept::thread(&self.store.conn) {
            Ok(items) => {
                for (mut item, proposal) in items {
                    // Too late to take back what she kept before.
                    if let hocus_secretary_link::Body::Learned { settled, .. } = &mut item.body {
                        *settled = true;
                    }
                    self.talk.next = self.talk.next.max(item.id);
                    if let Some(proposal) = proposal {
                        self.talk.proposals.insert(item.id, proposal);
                        self.talk.saved.insert(item.id);
                    }
                    self.talk.items.push(item);
                }
                if !self.talk.items.is_empty() {
                    tracing::info!(items = self.talk.items.len(), "her thread kept");
                }
            }
            Err(err) => tracing::warn!(%err, "her thread does not read"),
        }
        match crate::kept::pending(&self.store.conn) {
            Ok(pending) => {
                if !pending.is_empty() {
                    tracing::info!(items = pending.len(), "items still waiting for the triage");
                }
                for (pointer, held) in pending {
                    self.inbox.hold(pointer.clone(), held);
                    self.enqueue(pointer);
                }
            }
            Err(err) => tracing::warn!(%err, "the items waiting for the triage do not read"),
        }
    }

    /// The proposals of cards not written with them yet, written.
    fn keep_proposals(&mut self) {
        let unsaved: Vec<u64> = self
            .talk
            .proposals
            .keys()
            .copied()
            .filter(|i| !self.talk.saved.contains(i))
            .collect();
        for id in unsaved {
            if let Some(item) = self.talk.items.iter().find(|i| i.id == id)
                && let Err(err) =
                    crate::kept::keep_item(&self.store.conn, item, self.talk.proposals.get(&id))
            {
                tracing::warn!(%err, "could not keep a card's proposal");
            }
            self.talk.saved.insert(id);
        }
    }

    /// Whether the keyring gives her answers' key, asked on a thread.
    fn ask_key(&mut self) {
        let Some(model) = self.model.clone().filter(|m| m.needs_key()) else {
            return;
        };
        if self.key_asked {
            return;
        }
        self.key_asked = true;
        let sender = self.sender.clone();
        let spawned = std::thread::Builder::new()
            .name("key".to_owned())
            .spawn(move || {
                let _ = sender.send(Event::Keyed(model.missing_key()));
            });
        if let Err(err) = spawned {
            self.key_asked = false;
            tracing::warn!(%err, "no thread to ask the keyring");
        }
    }

    /// The triage's queue, one step: an item sent, the model unloaded,
    /// or nothing.
    fn triage_step(&mut self) {
        let Some(triager) = self.triager.clone() else {
            return;
        };
        let settings = self.settings.current().triage;
        let now = self.clock.now();
        self.watch_heat(&settings);
        // Power is read only when there is something to do; the heat is
        // the sampler's.
        let power = if self.queue.pending() > 0 {
            crate::power::Power {
                heat: self.heat,
                ..self.sensors.read()
            }
        } else {
            crate::power::Power {
                on_ac: true,
                ..Default::default()
            }
        };
        match self.queue.step(now, power, &settings) {
            Step::Nothing => {}
            Step::Unload => {
                let spawned = std::thread::Builder::new()
                    .name("triage unload".to_owned())
                    .spawn(move || {
                        if let Err(err) = triager.unload() {
                            tracing::info!(%err, "the local model was not unloaded");
                        }
                    });
                if let Err(err) = spawned {
                    tracing::warn!(%err, "no thread to unload the local model");
                }
            }
            Step::Send(pointer) => {
                let Some(held) = self.inbox.get(&pointer) else {
                    // Forgotten meanwhile.
                    self.queue.finished(&pointer, now, true, &settings);
                    return;
                };
                let input = triage::input(&held.redacted.item);
                let sender = self.sender.clone();
                let spawned = std::thread::Builder::new()
                    .name("triage".to_owned())
                    .spawn({
                        let pointer = pointer.clone();
                        move || {
                            let result = triager.triage(&input);
                            // Where the model is held, while it is.
                            let placement = triager.placement();
                            let _ = sender.send(Event::Triaged {
                                pointer,
                                started: now,
                                result,
                                placement,
                            });
                        }
                    });
                if let Err(err) = spawned {
                    tracing::warn!(%err, "no thread for the triage");
                    self.queue.finished(&pointer, now, false, &settings);
                }
            }
        }
    }

    /// The heat's sampler told what to watch, when that changed: the
    /// graphics card with the model whole on it, else the package; only
    /// while the triage has something to do.
    fn watch_heat(&mut self, settings: &settings::Triage) {
        let Some(watches) = &self.heat_watch else {
            return;
        };
        let busy = self.queue.pending() > 0 || self.queue.running().is_some();
        let watch = busy.then(|| crate::heat::Watch {
            gpu: self.placement == Some(crate::providers::openai::Placement::Gpu),
            triage: settings.clone(),
        });
        if watch == self.heat_sent {
            return;
        }
        if watches.send(watch.clone()).is_err() {
            tracing::warn!("the heat's sampler is gone");
            self.heat_watch = None;
        }
        if watch.is_none() {
            self.heat = None;
        }
        self.heat_sent = watch;
    }

    /// The triage's verdict on an item: the second pass applied (its
    /// spans masked in every field), its digest kept, masked too.
    fn triaged(
        &mut self,
        pointer: &str,
        started: Timestamp,
        result: Result<(Verdict, u64, u64), String>,
    ) {
        let now = self.clock.now();
        let settings = self.settings.current().triage;
        let took = Duration::try_from(now.duration_since(started)).unwrap_or_default();
        let (provider, model) = self
            .triager
            .as_ref()
            .map(|t| t.endpoint())
            .unwrap_or_default();
        let entry = audit::Entry {
            kind: audit::Kind::Call,
            what: Role::Triage.word().to_owned(),
            ids: vec![pointer.to_owned()],
            provider: Some(provider),
            model: Some(model),
            tokens_in: result.as_ref().ok().map(|r| r.1),
            tokens_out: result.as_ref().ok().map(|r| r.2),
            duration_ms: Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX)),
            outcome: Some(if result.is_ok() { "ok" } else { "failed" }.to_owned()),
        };
        if let Err(err) = self.store.audit(entry) {
            tracing::warn!(%err, "could not audit the triage");
        }
        let verdict = match result {
            Ok((verdict, _, _)) => verdict,
            Err(err) => {
                tracing::info!(pointer, %err, "the triage failed: the item stays unchecked");
                self.queue.finished(pointer, now, false, &settings);
                return;
            }
        };
        self.queue.finished(pointer, now, true, &settings);
        let Some(held) = self.inbox.get_mut(pointer) else {
            return;
        };
        let mut quoted = Quoted::new(verdict.secrets.iter().map(|s| s.text.clone()));
        if let Err(err) = held.redacted.second_pass(&mut quoted) {
            tracing::warn!(pointer, %err, "the second pass failed: the item stays unchecked");
            return;
        }
        // Checked: its text is no longer kept on disk.
        if let Err(err) = crate::kept::checked(&self.store.conn, pointer) {
            tracing::warn!(pointer, %err, "could not let a checked item's text go");
        }
        let label = held.redacted.item.pointer.label.clone();
        let digest = self.redactor.mask_with(&verdict.digest, &quoted, &label);
        held.summary = Some(digest.clone());
        held.suspicious = verdict.suspicious;
        let episode = held.episode;
        if !digest.trim().is_empty()
            && let Err(err) = triage::keep_digest(&mut self.store, episode, &digest)
        {
            tracing::warn!(pointer, %err, "could not keep the triage's digest");
        }
        // Its candidate facts: proposals, never confirmed (lot 6, §7.1).
        if !verdict.facts.is_empty() {
            self.extracted(episode, &verdict.facts);
        }
        let commitments: Vec<String> = verdict
            .commitments
            .iter()
            .map(|c| self.redactor.mask_with(c, &quoted, &label))
            .collect();
        self.triaged_mail(pointer, &verdict, &commitments);
    }

    /// A source item's day record: an event's times and people, a mail's
    /// headers (lot 5).
    fn keep_day(
        &mut self,
        item: &crate::sources::Item,
        pointer: &str,
        notebook: hocus_secretary_link::Notebook,
    ) {
        let source = item
            .from
            .clone()
            .or_else(|| pointer.split_once(':').map(|(s, _)| s.to_owned()));
        if let Some(event) = &item.event {
            let kept = crate::day::keep_event(
                &self.store.conn,
                &crate::day::Event {
                    pointer: pointer.to_owned(),
                    source,
                    start: item.at,
                    end: event.end,
                    title: item.digest.clone(),
                    calendar: event.calendar.clone(),
                    attendees: event.attendees.clone(),
                    organiser: event.organiser.clone(),
                    presenter: event.presenter.clone(),
                    busy: event.busy,
                    notebook,
                },
            );
            if let Err(err) = kept {
                tracing::warn!(pointer, %err, "could not keep an event");
            }
        } else if item.kind == "mail" {
            let info = item.mail.clone().unwrap_or_default();
            let subject = item
                .text
                .as_ref()
                .and_then(|t| match &t.item.content {
                    hocus_sources::Content::Mail(m) => Some(m.subject.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    item.digest
                        .split_once(" : ")
                        .map_or(item.digest.clone(), |(_, s)| s.to_owned())
                });
            let kept = crate::day::keep_mail(
                &self.store.conn,
                &crate::day::Mail {
                    pointer: pointer.to_owned(),
                    source,
                    at: item.at,
                    sender: item.sender.clone().unwrap_or_default(),
                    recipients: info.to,
                    subject,
                    message_id: info.message_id,
                    in_reply_to: info.in_reply_to,
                    sent: info.sent,
                    notebook,
                    ..Default::default()
                },
            );
            if let Err(err) = kept {
                tracing::warn!(pointer, %err, "could not keep a mail");
            }
        }
    }
}

/// A source's worker: syncs each time it is asked, with the cursor given.
fn spawn_source(
    index: usize,
    mut source: Box<dyn Source>,
    events: Sender<Event>,
    clock: Clock,
    now: Timestamp,
) -> Slot {
    let (asks, asked) = mpsc::channel::<Option<String>>();
    let slot = Slot {
        name: source.name().to_owned(),
        notebook: source.notebook(),
        every: source.every(),
        asks,
        due: now,
        busy: false,
        again: false,
        items: 0,
        error: None,
    };
    let spawned = std::thread::Builder::new()
        .name(format!("source {}", source.name()))
        .spawn(move || {
            while let Ok(cursor) = asked.recv() {
                let result = source.sync(cursor.as_deref(), clock.now());
                if events
                    .send(Event::Synced {
                        source: index,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "no thread for a source");
    }
    slot
}

/// A date the user means: `YYYY-MM-DD`, or an instant's date in their
/// time zone.
pub fn as_of(text: &str) -> Option<Date> {
    text.parse::<Date>().ok().or_else(|| {
        text.parse::<Timestamp>()
            .ok()
            .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date())
    })
}
