//! Her speaking up (§6, lot 5), on her loop: the briefings, her requests
//! to speak, the user's answers to her peek, her reminders, what the user
//! promised in a mail, and the effects the shell carries out for her.
//!
//! - **The morning briefing** comes at the first session of the day
//!   after `[secretary.speak] morning` (07:00): a login (a shell's first
//!   `session` line unlocked) or an unlock. She waits for her catch-up and
//!   her triage to settle, computes it in code ([`crate::briefing`]),
//!   has her model word one sentence and the rest in three lines, keeps
//!   it as the day's first thread item, then asks to speak once: "Ton
//!   point est prêt". A second unlock that day makes none. The drawer
//!   never opens by itself: the shell's gate decides when her peek comes
//!   out.
//! - **"Pendant ton absence"**: after a suspend or a shutdown of over four
//!   hours, at the next unlock, once: what came that matters, what came
//!   due, the perso items; folded into the morning briefing when that is
//!   due at the same unlock (one peek, not two).
//! - **Her requests** ([`Speak`]) go to the shell's gate, which answers
//!   shown, held or dropped; dropped (quiet hours, past its time) goes to
//!   the next briefing. One she is not answered for is asked again when a
//!   shell says hello.
//! - **Her peek's answers**: "Fait" closes a reminder; "Plus tard ▾" asks
//!   again in an hour, after the next meeting, this afternoon, in the
//!   next briefing, or never again (that kind of reminder then goes to the
//!   briefings only).
//! - **Reminders** come due on her clock (her loop wakes for the next
//!   one, and for nothing else); one that can no longer be acted on is
//!   listed in the next briefing instead: after a suspend, never lost.
//! - **Commitments**: what the user promised in a mail they sent (the
//!   triage finds it) becomes a card: "Mettre dans À faire", with a
//!   reminder on the day said.
//! - **Effects**: an accepted card's effect goes to its effector
//!   ([`crate::effects`]); the To do list's is the shell's to carry out,
//!   and the card says so when it answers.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};

use hocus_secretary_link::{
    Action, Body, BriefingKind, Card, CardKind, Client, Decision, FromSecretary, Later, Notebook,
    PeekAnswer, Speak, SpeakKind, Spoken,
};
use jiff::{SignedDuration, Timestamp, tz::TimeZone};

use super::{Brain, Event};
use crate::{
    audit,
    briefing::{self, Built, Input},
    day::{self, Reminder, State},
    effects::{Kind, Outcome, Proposal},
    model::{Message, Part, Reply, Role},
    store::{stamp, unstamp},
};

/// Her day's briefing given: the date, in her memory's meta.
const BRIEFED: &str = "briefing-day";
/// An absence to tell of: `FROM|TO`.
const AWAY: &str = "away";
/// Kinds of reminders the user said never to tell again.
const NEVER: &str = "never-kinds";
/// She waits this long at most for her triage before a briefing.
const SETTLE: SignedDuration = SignedDuration::from_mins(2);

/// What a request to speak was about.
#[derive(Debug, Clone)]
struct About {
    speak: Speak,
    reminder: Option<i64>,
}

/// Her speaking's state, on her loop.
#[derive(Debug, Default)]
pub(super) struct Speaking {
    next: u64,
    /// Asked, not answered yet: asked again to a shell saying hello.
    asked: BTreeMap<u64, About>,
    /// Every request so far, for her peek's answers.
    said: HashMap<u64, About>,
    /// To ask again at a time ("Plus tard").
    later: Vec<(Timestamp, About)>,
    /// A session began: a briefing may be due, since.
    began: Option<Timestamp>,
    /// A briefing's model call under way.
    building: bool,
    /// Connections that said their session once.
    sessioned: HashSet<u64>,
    /// Effects the shell carries out: the card each is for, and what.
    effects: HashMap<u64, (u64, Proposal)>,
    /// The restraint last told the talkers.
    told: Option<hocus_secretary_link::gate::Restraint>,
}

impl Brain {
    fn zone(&self) -> TimeZone {
        TimeZone::system()
    }

    fn english(&self) -> bool {
        !self.talk.fr()
    }

    /// The talkers' connections (the shell's, the tests').
    fn talkers(&self) -> Vec<u64> {
        self.conns
            .iter()
            .filter(|(_, c)| matches!(c.client, Some(Client::Shell | Client::Test)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// A talker said hello: her restraint, and what she still asks.
    pub(super) fn greeted_speaking(&mut self, conn: u64) {
        let restraint = self.settings.current().speak.restraint();
        self.speaking.told = Some(restraint);
        self.send(conn, &FromSecretary::Restraint { restraint });
        let asked: Vec<Speak> = self
            .speaking
            .asked
            .values()
            .map(|a| a.speak.clone())
            .collect();
        for speak in asked {
            self.send(conn, &FromSecretary::Speak { speak });
        }
    }

    /// Her restraint again, to the talkers, when her settings changed it.
    pub(super) fn tell_restraint(&mut self) {
        let restraint = self.settings.current().speak.restraint();
        if self.speaking.told == Some(restraint) {
            return;
        }
        self.speaking.told = Some(restraint);
        for conn in self.talkers() {
            self.send(conn, &FromSecretary::Restraint { restraint });
        }
    }

    /// The session locked or not, as connection `conn` says: a login (its
    /// first line of it, unlocked) or an unlock begins a session.
    pub(super) fn session(&mut self, conn: u64, locked: bool) {
        let first = self.speaking.sessioned.insert(conn);
        let was = self.locked;
        self.locked = locked;
        if !locked && (was || first) {
            tracing::info!(login = first && !was, "a session begins");
            self.speaking.began = Some(self.clock.now());
            self.try_brief();
        }
    }

    /// Her absence of `gap` to tell of at the next session, from `to`.
    pub(super) fn away_for(&mut self, gap: Duration) {
        let to = self.clock.now();
        let from = to
            .checked_sub(SignedDuration::try_from(gap).unwrap_or(SignedDuration::ZERO))
            .unwrap_or(to);
        let kept = self
            .store
            .set_meta(AWAY, &format!("{}|{}", stamp(from), stamp(to)));
        if let Err(err) = kept {
            tracing::warn!(%err, "could not note the absence");
        }
    }

    fn away(&self) -> Option<(Timestamp, Timestamp)> {
        let text = self.store.meta(AWAY).ok().flatten()?;
        let (from, to) = text.split_once('|')?;
        Some((unstamp(from)?, unstamp(to)?))
    }

    /// A briefing, if one is due now and she is ready to make it.
    pub(super) fn try_brief(&mut self) {
        let Some(began) = self.speaking.began else {
            return;
        };
        if self.locked || self.speaking.building {
            return;
        }
        // Not in the middle of her first run (#758): no briefing of what
        // she does not read yet.
        if self.first_run_under_way() {
            return;
        }
        let now = self.clock.now();
        // Her catch-up, her sources' syncs under way and her triage
        // first: what came is in it.
        let settling = self.catching.is_some()
            || self.slots.iter().any(|s| s.busy)
            || (self.queue.pending() > 0
                && !matches!(self.queue.state, crate::triage::State::Paused { .. }));
        if settling && now.duration_since(began) < SETTLE {
            return;
        }
        let here = now.to_zoned(self.zone());
        let morning = self.settings.current().speak.morning();
        let today = here.date().to_string();
        let morning_due = here.time() >= morning
            && self.store.meta(BRIEFED).ok().flatten().as_deref() != Some(today.as_str());
        let away = self.away();
        self.speaking.began = None;
        if !morning_due && away.is_none() {
            return;
        }
        let input = Input {
            now,
            zone: self.zone(),
            english: self.english(),
            read: self.read_words(),
            away,
            public: self.scope.moment.public(),
        };
        let mut built = if morning_due {
            briefing::morning(&self.store, &input)
        } else {
            briefing::absence(&self.store, &input)
        };
        // Friday: what she learned this week (§9.3), from her memory's
        // weekly digest, to keep or forget in "Ce qu'elle sait".
        if morning_due && here.weekday() == jiff::civil::Weekday::Friday {
            let lines = self.week_learned();
            if !lines.is_empty() {
                let at = built
                    .briefing
                    .sections
                    .iter()
                    .position(|s| s.key == "perso")
                    .unwrap_or(built.briefing.sections.len());
                built.briefing.sections.insert(
                    at,
                    hocus_secretary_link::Section {
                        key: "week".to_owned(),
                        label: self
                            .talk
                            .t(
                                "CE QUE J’AI APPRIS CETTE SEMAINE",
                                "WHAT I LEARNED THIS WEEK",
                            )
                            .to_owned(),
                        note: self
                            .talk
                            .t(
                                "à garder ou oublier dans « Ce qu’elle sait »",
                                "keep or forget in “What she knows”",
                            )
                            .to_owned(),
                        lines,
                        proposal: None,
                    },
                );
            }
        }
        if morning_due && let Err(err) = self.store.set_meta(BRIEFED, &today) {
            tracing::warn!(%err, "could not note the briefing");
        }
        if away.is_some()
            && let Err(err) = self.store.set_meta(AWAY, "")
        {
            tracing::warn!(%err, "could not clear the absence");
        }
        tracing::info!(
            kind = ?built.briefing.kind,
            sections = built.briefing.sections.len(),
            away = away.is_some(),
            "her briefing"
        );
        self.word(built, away.is_some());
    }

    /// The sources she read, as the footer says them.
    fn read_words(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for slot in &self.slots {
            let lower = slot.name.to_lowercase();
            let name = if lower.contains("notification") {
                if slot.items == 0 {
                    continue;
                }
                self.talk.t("notifications", "notifications").to_owned()
            } else if lower.contains("cal") || lower.contains("agenda") {
                self.talk.t("agenda", "calendar").to_owned()
            } else if lower.contains("mail") {
                "Gmail".to_owned()
            } else if lower.contains("google") {
                self.talk.t("agenda, Gmail", "calendar, Gmail").to_owned()
            } else {
                continue;
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Her model words the briefing (role `briefing`), on a worker
    /// thread; without a model, it stands as code made it.
    fn word(&mut self, built: Built, away: bool) {
        let Some(model) = self.model.clone() else {
            return self.briefed(built, away, None);
        };
        let ask = self.talk.t(
            "Fais le point : une phrase sur la journée, puis le reste en trois lignes au plus, \
             une par ligne. Seulement ce que disent les faits ci-dessous.",
            "Brief me: one sentence on the day, then the rest in three lines at most, one a \
             line. Only what the facts below say.",
        );
        let mut request = match self.request(Role::Briefing, &[Message::user(ask)]) {
            Ok(request) => request,
            Err(err) => {
                tracing::warn!(%err, "no request for the briefing");
                return self.briefed(built, away, None);
            }
        };
        request.tools.clear();
        request.context.push(Part {
            label: "briefing".to_owned(),
            text: format!("<briefing>\n{}\n</briefing>", built.facts),
            items: Vec::new(),
        });
        self.speaking.building = true;
        self.calls_running += 1;
        let (sender, clock) = (self.sender.clone(), self.clock.clone());
        let started = clock.now();
        let items = request.items();
        let spawned = std::thread::Builder::new()
            .name("briefing".to_owned())
            .spawn({
                let built = built.clone();
                move || {
                    let result = model.call(&request, &clock);
                    let _ = sender.send(Event::Briefed {
                        built: Box::new(built),
                        away,
                        items,
                        started,
                        result,
                    });
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "no thread for the briefing");
            self.calls_running -= 1;
            self.speaking.building = false;
            self.briefed(built, away, None);
        }
    }

    /// Her model's words back: audited, then the briefing.
    pub(super) fn worded(
        &mut self,
        built: Built,
        away: bool,
        items: Vec<String>,
        started: Timestamp,
        result: Result<Reply, String>,
    ) {
        self.calls_running = self.calls_running.saturating_sub(1);
        self.speaking.building = false;
        let took = Duration::try_from(self.clock.now().duration_since(started)).unwrap_or_default();
        self.audited(audit::Entry {
            kind: audit::Kind::Call,
            what: Role::Briefing.word().to_owned(),
            ids: items,
            provider: result.as_ref().ok().map(|r| r.provider.clone()),
            model: result.as_ref().ok().map(|r| r.model.clone()),
            tokens_in: result.as_ref().ok().map(|r| r.tokens_in),
            tokens_out: result.as_ref().ok().map(|r| r.tokens_out),
            duration_ms: Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX)),
            outcome: Some(if result.is_ok() { "ok" } else { "failed" }.to_owned()),
        });
        let reply = match result {
            Ok(reply) => Some(reply),
            Err(err) => {
                tracing::info!(%err, "her model did not word the briefing: as code made it");
                None
            }
        };
        self.briefed(built, away, reply.as_ref());
    }

    /// The briefing kept as a thread item, its proposals as cards, and
    /// her request to speak.
    fn briefed(&mut self, mut built: Built, away: bool, reply: Option<&Reply>) {
        if let Some(reply) = reply {
            let mut lines = reply
                .text
                .lines()
                .map(|l| l.trim().trim_start_matches(['-', '•', '*']).trim())
                .filter(|l| !l.is_empty());
            if let Some(sentence) = lines.next() {
                built.briefing.sentence = sentence.to_owned();
            }
            let rest: Vec<String> = lines.take(3).map(str::to_owned).collect();
            if !rest.is_empty() && built.briefing.kind == BriefingKind::Morning {
                let section = hocus_secretary_link::Section {
                    key: "rest".to_owned(),
                    label: self.talk.t("LE RESTE", "THE REST").to_owned(),
                    note: String::new(),
                    lines: rest
                        .into_iter()
                        .map(|title| hocus_secretary_link::Line {
                            title,
                            notebook: Notebook::Pro,
                            ..Default::default()
                        })
                        .collect(),
                    proposal: None,
                };
                match built.briefing.sections.iter_mut().find(|s| s.key == "rest") {
                    Some(old) => *old = section,
                    None => {
                        let at = built
                            .briefing
                            .sections
                            .iter()
                            .position(|s| {
                                matches!(
                                    s.key.as_str(),
                                    "yesterday" | "plan" | "reminders" | "perso"
                                )
                            })
                            .unwrap_or(built.briefing.sections.len());
                        built.briefing.sections.insert(at, section);
                    }
                }
            }
        }
        let kind = built.briefing.kind;
        let item = self.add_item_in(
            None,
            Body::Briefing {
                briefing: built.briefing.clone(),
            },
            Some(Notebook::Pro),
        );
        // Each meeting to move: a proposal under EN CONGÉS.
        let zone = self.zone();
        let en = self.english();
        let mut proposal = None;
        for m in &built.moves {
            let (label, text, organiser) = briefing::move_words(m, &zone, en);
            let Some(slot) = m.slot else {
                continue;
            };
            let to = if organiser.is_empty() {
                self.talk.t("l’organisateur", "the organiser").to_owned()
            } else {
                organiser.clone()
            };
            let start = slot.start.to_zoned(zone.clone());
            let when = format!(
                "{} {}",
                briefing::day_in_sentence(start.date(), en),
                briefing::hour(&start, en)
            );
            let draft = if en {
                format!(
                    "Hi {to}, {} is away for “{}”. Could we move it to {when}? Everyone invited is free then.",
                    m.absent, m.meeting.title
                )
            } else {
                format!(
                    "Bonjour {to}, {} est en congés pour «\u{a0}{}\u{a0}». On la déplace à {when}\u{a0}? Tous les invités sont libres.",
                    m.absent, m.meeting.title
                )
            };
            let accept = if en {
                format!("Suggest it to {to}")
            } else {
                format!("Proposer à {to}")
            };
            let card = Card {
                kind: CardKind::Plan,
                label,
                text,
                lines: Vec::new(),
                actions: vec![
                    Action {
                        label: accept,
                        decision: Decision::Accept,
                    },
                    Action {
                        label: self.talk.t("Garder", "Keep it").to_owned(),
                        decision: Decision::Reject,
                    },
                ],
                decided: None,
                outcome: None,
                sheet: None,
            };
            let id = self.add_item_in(None, Body::Card { card }, Some(Notebook::Pro));
            self.talk.proposals.insert(
                id,
                Proposal {
                    kind: Kind::Prepare,
                    tool: "move_meeting".to_owned(),
                    input: serde_json::json!({
                        "to": to,
                        "draft": draft,
                        "meeting": m.meeting.pointer,
                        "slot": stamp(slot.start),
                    }),
                    notebook: Notebook::Pro,
                },
            );
            self.audited(audit::Entry {
                kind: audit::Kind::Proposal,
                what: "move".to_owned(),
                ids: vec![format!("card:{id}"), m.meeting.pointer.clone()],
                ..Default::default()
            });
            proposal.get_or_insert(id);
        }
        if let Some(card) = proposal {
            self.change_item(item, |body| {
                if let Body::Briefing { briefing } = body
                    && let Some(section) = briefing.sections.iter_mut().find(|s| s.key == "leaves")
                {
                    section.proposal = Some(card);
                }
            });
        }
        for id in &built.listed {
            if let Err(err) = day::set_reminder(&self.store.conn, *id, State::Said, None) {
                tracing::warn!(%err, "could not mark a reminder listed");
            }
        }
        let (speak_kind, text, action) = if away {
            let text = match (en, built.arrived) {
                (false, 0) => {
                    "**Pendant ton absence**\u{a0}: rien d’important. Ton point est prêt."
                        .to_owned()
                }
                (false, n) => format!(
                    "**Pendant ton absence**\u{a0}: {n} choses sont arrivées. Je t’ai fait le point."
                ),
                (true, 0) => {
                    "**While you were away**: nothing important. Your briefing is ready.".to_owned()
                }
                (true, n) => format!("**While you were away**: {n} things came. I briefed you."),
            };
            (SpeakKind::Absence, text, self.talk.t("Voir", "See"))
        } else {
            // What is in it, in her bubble (#758).
            let text = match (en, built.peek.is_empty()) {
                (false, true) => "**Ton point du matin est prêt.**".to_owned(),
                (false, false) => format!("**Ton point du matin est prêt.** {}.", built.peek),
                (true, true) => "**Your morning briefing is ready.**".to_owned(),
                (true, false) => format!("**Your morning briefing is ready.** {}.", built.peek),
            };
            (
                SpeakKind::Briefing,
                text,
                self.talk.t("Voir le point", "See the briefing"),
            )
        };
        let _ = kind;
        self.speak(
            speak_kind,
            text,
            Some(action.to_owned()),
            Some(item),
            Notebook::Pro,
            None,
            None,
        );
        self.tell_look();
    }

    /// She asks to speak.
    #[allow(clippy::too_many_arguments)]
    fn speak(
        &mut self,
        kind: SpeakKind,
        text: String,
        action: Option<String>,
        item: Option<u64>,
        notebook: Notebook,
        deadline: Option<Timestamp>,
        reminder: Option<i64>,
    ) {
        self.speaking.next += 1;
        let speak = Speak {
            id: self.speaking.next,
            kind,
            text,
            action,
            item,
            notebook,
            vip: false,
            deadline: deadline.map(|d| d.to_string()),
        };
        self.ask_to_speak(About { speak, reminder });
    }

    fn ask_to_speak(&mut self, about: About) {
        tracing::info!(
            id = about.speak.id,
            kind = about.speak.kind.word(),
            "she asks to speak"
        );
        let speak = about.speak.clone();
        self.speaking.asked.insert(speak.id, about.clone());
        self.speaking.said.insert(speak.id, about);
        for conn in self.talkers() {
            self.send(
                conn,
                &FromSecretary::Speak {
                    speak: speak.clone(),
                },
            );
        }
    }

    /// The gate's answer.
    pub(super) fn spoken(&mut self, speak: u64, outcome: Spoken) {
        tracing::info!(speak, ?outcome, "the gate answered");
        let reminder = self.speaking.said.get(&speak).and_then(|a| a.reminder);
        let state = match outcome {
            Spoken::Held { .. } => return,
            Spoken::Shown => State::Said,
            Spoken::Dropped { .. } => State::Briefing,
        };
        self.speaking.asked.remove(&speak);
        if let Some(id) = reminder
            && let Err(err) = day::set_reminder(&self.store.conn, id, state, None)
        {
            tracing::warn!(%err, "could not mark a reminder");
        }
    }

    /// What the user did with her peek.
    pub(super) fn peeked(&mut self, conn: u64, id: u64, speak: u64, answer: PeekAnswer) {
        let Some(about) = self.speaking.said.get(&speak).cloned() else {
            return self.error(conn, Some(id), "no such peek");
        };
        tracing::info!(speak, ?answer, "her peek answered");
        self.speaking.asked.remove(&speak);
        let now = self.clock.now();
        let set = |brain: &mut Self, state: State, next: Option<Timestamp>| {
            if let Some(r) = about.reminder
                && let Err(err) = day::set_reminder(&brain.store.conn, r, state, next)
            {
                tracing::warn!(%err, "could not change a reminder");
            }
        };
        match answer {
            PeekAnswer::Open => {}
            PeekAnswer::Done => set(self, State::Done, None),
            PeekAnswer::Later { later } => match later {
                Later::Tomorrow => set(self, State::Briefing, None),
                Later::Never => {
                    set(self, State::Never, None);
                    if let Some(r) = about.reminder
                        && let Ok(Some(reminder)) = day::reminder(&self.store.conn, r)
                    {
                        self.never_again(&reminder.kind);
                    }
                }
                later => {
                    let at = self.later_time(later, now);
                    match about.reminder {
                        Some(_) => set(self, State::Open, Some(at)),
                        None => self.speaking.later.push((at, about.clone())),
                    }
                }
            },
        }
        self.done(conn, id);
    }

    /// When "Plus tard" means, from `now`.
    fn later_time(&self, later: Later, now: Timestamp) -> Timestamp {
        let zone = self.zone();
        let hour = now + SignedDuration::from_hours(1);
        match later {
            Later::AfterMeeting => {
                let end = now + SignedDuration::from_hours(12);
                day::events(&self.store.conn, now, end)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| e.busy)
                    .map(|e| e.end)
                    .find(|e| *e > now)
                    .map_or(hour, |e| e + SignedDuration::from_mins(5))
            }
            Later::Afternoon => {
                let here = now.to_zoned(zone.clone());
                let at = |d: jiff::civil::Date| {
                    d.at(14, 30, 0, 0)
                        .to_zoned(zone.clone())
                        .map_or(hour, |z| z.timestamp())
                };
                let today = at(here.date());
                if today > now {
                    today
                } else {
                    here.date().tomorrow().map_or(hour, at)
                }
            }
            _ => hour,
        }
    }

    fn never_again(&mut self, kind: &str) {
        let mut kinds = self.never_kinds();
        if !kinds.iter().any(|k| k == kind) {
            kinds.push(kind.to_owned());
        }
        if let Err(err) = self.store.set_meta(NEVER, &kinds.join(",")) {
            tracing::warn!(%err, "could not keep what never to tell again");
        }
    }

    fn never_kinds(&self) -> Vec<String> {
        self.store
            .meta(NEVER)
            .ok()
            .flatten()
            .map(|t| {
                t.split(',')
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// When her loop wakes next for her speaking: a reminder's time, a
    /// "Plus tard".
    pub(super) fn speaking_next(&self) -> Option<Timestamp> {
        let reminder = day::reminders(&self.store.conn, &[State::Open])
            .ok()?
            .into_iter()
            .map(|r| r.when())
            .min();
        let later = self.speaking.later.iter().map(|(t, _)| *t).min();
        reminder.into_iter().chain(later).min()
    }

    /// What came due: said if it can still be acted on, else kept for the
    /// next briefing.
    pub(super) fn speak_due(&mut self) {
        let now = self.clock.now();
        self.tell_restraint();
        let due: Vec<(Timestamp, About)> = self
            .speaking
            .later
            .extract_if(.., |(t, _)| *t <= now)
            .collect();
        for (_, mut about) in due {
            // The same words, asked anew.
            self.speaking.next += 1;
            about.speak.id = self.speaking.next;
            self.ask_to_speak(about);
        }
        let Ok(reminders) = day::reminders(&self.store.conn, &[State::Open]) else {
            return;
        };
        let never = self.never_kinds();
        for r in reminders.into_iter().filter(|r| r.when() <= now) {
            if !r.actionable(now) || never.contains(&r.kind) {
                if let Err(err) = day::set_reminder(&self.store.conn, r.id, State::Briefing, None) {
                    tracing::warn!(%err, "could not keep a reminder for the briefing");
                }
                continue;
            }
            if let Err(err) = day::set_reminder(&self.store.conn, r.id, State::Asked, None) {
                tracing::warn!(%err, "could not mark a reminder asked");
                continue;
            }
            self.remind(&r);
        }
    }

    /// Reminder `r`: in the thread, and her request to speak.
    fn remind(&mut self, r: &Reminder) {
        let text = if self.english() {
            format!("**Reminder**: {}", r.text)
        } else {
            format!("**Rappel**\u{a0}: {}", r.text)
        };
        let item = self.add_item_in(
            None,
            Body::Her {
                text: text.replace("**", ""),
                note: None,
                assumed: false,
            },
            Some(r.notebook),
        );
        self.speak(
            SpeakKind::Reminder,
            text,
            None,
            Some(item),
            r.notebook,
            r.until,
            Some(r.id),
        );
    }

    /// The triage read mail `pointer`: its verdict kept; what the user
    /// promised in it, if they sent it, proposed as a task.
    pub(super) fn triaged_mail(
        &mut self,
        pointer: &str,
        verdict: &crate::triage::Verdict,
        commitments: &[String],
    ) {
        let urgency = if verdict.urgency.is_empty() {
            "normal"
        } else {
            verdict.urgency.as_str()
        };
        match day::triaged_mail(
            &self.store.conn,
            pointer,
            verdict.for_me,
            verdict.needs_reply,
            urgency,
        ) {
            Ok(false) | Err(_) => return,
            Ok(true) => {}
        }
        let Ok(Some(mail)) = day::mail(&self.store.conn, pointer) else {
            return;
        };
        if !mail.sent {
            return;
        }
        for commitment in commitments.iter().filter(|c| !c.trim().is_empty()) {
            self.commitment(&mail, commitment.trim());
        }
    }

    /// A commitment: a card to put it in To do, with a reminder.
    fn commitment(&mut self, mail: &day::Mail, text: &str) {
        let now = self.clock.now();
        let zone = self.zone();
        let due = briefing::due_of(text, now, &zone);
        let to = mail
            .recipients
            .first()
            .map(|r| r.split('<').next().unwrap_or(r).trim().to_owned())
            .unwrap_or_default();
        let en = self.english();
        let when = due.map(|(due, _)| {
            let z = due.to_zoned(zone.clone());
            format!(
                "{} {}",
                briefing::day_in_sentence(z.date(), en),
                briefing::hour(&z, en)
            )
        });
        let said = match (en, to.is_empty(), &when) {
            (false, false, Some(when)) => format!(
                "Tu as écrit à {to}\u{a0}: «\u{a0}{text}\u{a0}». Je le mets dans À faire, et je te le rappelle {when}\u{a0}?"
            ),
            (false, false, None) => {
                format!(
                    "Tu as écrit à {to}\u{a0}: «\u{a0}{text}\u{a0}». Je le mets dans À faire\u{a0}?"
                )
            }
            (false, true, _) => {
                format!("Tu as promis\u{a0}: «\u{a0}{text}\u{a0}». Je le mets dans À faire\u{a0}?")
            }
            (true, false, Some(when)) => format!(
                "You wrote to {to}: “{text}”. Shall I put it in To do, and remind you {when}?"
            ),
            (true, _, _) => format!("You promised: “{text}”. Shall I put it in To do?"),
        };
        let card = Card {
            kind: CardKind::Plan,
            label: self.talk.t("Ta promesse", "Your promise").to_owned(),
            text: said,
            lines: Vec::new(),
            actions: vec![
                Action {
                    label: self
                        .talk
                        .t("Mettre dans À faire", "Put in To do")
                        .to_owned(),
                    decision: Decision::Accept,
                },
                Action {
                    label: self.talk.t("Non merci", "No thanks").to_owned(),
                    decision: Decision::Reject,
                },
            ],
            decided: None,
            outcome: None,
            sheet: None,
        };
        let item = self.add_item_in(None, Body::Card { card }, Some(mail.notebook));
        let title = if to.is_empty() {
            text.to_owned()
        } else {
            format!("{text} ({to})")
        };
        self.talk.proposals.insert(
            item,
            Proposal {
                kind: Kind::Task,
                tool: "commitment".to_owned(),
                input: serde_json::json!({
                    "title": title,
                    "reminder": text,
                    "due": due.map(|(d, _)| stamp(d)),
                    "until": due.map(|(_, u)| stamp(u)),
                    "pointer": mail.pointer,
                }),
                notebook: mail.notebook,
            },
        );
        self.audited(audit::Entry {
            kind: audit::Kind::Proposal,
            what: "commitment".to_owned(),
            ids: vec![format!("card:{item}"), mail.pointer.clone()],
            ..Default::default()
        });
        tracing::info!(item, "a commitment proposed");
    }

    /// "Rappelle-le-moi" on a briefing's mail to answer (#758): a
    /// reminder two hours from now, and the line says when.
    pub(super) fn remind_me(&mut self, conn: u64, id: u64, item: u64, pointer: &str) {
        let line = self
            .talk
            .items
            .iter()
            .find(|i| i.id == item)
            .and_then(|i| match &i.body {
                Body::Briefing { briefing } => briefing
                    .sections
                    .iter()
                    .flat_map(|s| s.lines.iter())
                    .find(|l| l.pointer == pointer && !pointer.is_empty())
                    .cloned(),
                _ => None,
            });
        let Some(line) = line else {
            return self.error(conn, Some(id), "no such line");
        };
        if !line.reminded.is_empty() {
            return self.done(conn, id);
        }
        let now = self.clock.now();
        let due = now + SignedDuration::from_hours(2);
        let en = self.english();
        let text = if en {
            format!("Answer {}", line.title)
        } else {
            format!("Répondre à {}", line.title)
        };
        let reminder = Reminder {
            id: 0,
            kind: "answer".to_owned(),
            text,
            due,
            until: None,
            next: None,
            state: State::Open,
            pointer: Some(pointer.to_owned()),
            notebook: line.notebook,
        };
        let source = pointer.split_once(':').map(|(s, _)| s.to_owned());
        if let Err(err) = day::remind(&self.store.conn, &reminder, source.as_deref(), now) {
            tracing::warn!(%err, "could not keep a reminder");
            return self.error(conn, Some(id), "the reminder is not kept");
        }
        let at = briefing::hour(&due.to_zoned(TimeZone::system()), en);
        let said = if en {
            format!("I’ll remind you at {at}")
        } else {
            format!("Je te le rappelle à {at}")
        };
        tracing::info!(item, "a mail to answer, to remind");
        self.change_item(item, |body| {
            if let Body::Briefing { briefing } = body {
                for l in briefing
                    .sections
                    .iter_mut()
                    .flat_map(|s| s.lines.iter_mut())
                {
                    if l.pointer == pointer {
                        l.reminded.clone_from(&said);
                    }
                }
            }
        });
        self.done(conn, id);
    }

    /// Card `item`'s proposal accepted: its effect, or what it prepares.
    /// What to say under the card.
    pub(super) fn accepted(&mut self, now: Timestamp, item: u64, proposal: &Proposal) -> String {
        if proposal.kind == Kind::Prepare {
            return self.prepared(item, proposal);
        }
        // A task with a time: her reminder, kept whatever the shell says.
        if proposal.kind == Kind::Task
            && let Some(due) = proposal.input["due"].as_str().and_then(unstamp)
        {
            let text = proposal.input["reminder"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| crate::effects::ToDo::title(&proposal.input))
                .unwrap_or_default();
            let reminder = Reminder {
                id: 0,
                kind: proposal.tool.clone(),
                text,
                due,
                until: proposal.input["until"].as_str().and_then(unstamp),
                next: None,
                state: State::Open,
                pointer: proposal.input["pointer"].as_str().map(str::to_owned),
                notebook: proposal.notebook,
            };
            let source = reminder
                .pointer
                .as_deref()
                .and_then(|p| p.split_once(':').map(|(s, _)| s.to_owned()));
            if let Err(err) = day::remind(&self.store.conn, &reminder, source.as_deref(), now) {
                tracing::warn!(%err, "could not keep a reminder");
            }
        }
        match self
            .effectors
            .carry_out(now, item, proposal, self.english())
        {
            // A message sent says where and when (#758).
            Outcome::Done(_)
                if matches!(proposal.kind, Kind::Message | Kind::Post | Kind::Mail) =>
            {
                let channel = super::conversation::Channel::of(
                    proposal.input["channel"].as_str(),
                    proposal.kind.word(),
                );
                let here = now.to_zoned(jiff::tz::TimeZone::system());
                channel.sent(
                    &crate::briefing::hour(&here, self.english()),
                    !self.english(),
                )
            }
            Outcome::Done(text) | Outcome::Unable(text) => text,
            Outcome::Shell(effect) => {
                let talkers: Vec<u64> = self
                    .conns
                    .iter()
                    .filter(|(_, c)| matches!(c.client, Some(Client::Shell | Client::Test)))
                    .map(|(id, _)| *id)
                    .collect();
                if talkers.is_empty() {
                    return self
                        .talk
                        .t(
                            "Le bureau ne répond pas : rien n’est ajouté.",
                            "The desktop is not answering: nothing was added.",
                        )
                        .to_owned();
                }
                tracing::info!(id = effect.id(), "an effect for the shell");
                self.speaking
                    .effects
                    .insert(effect.id(), (item, proposal.clone()));
                for conn in talkers {
                    self.send(
                        conn,
                        &FromSecretary::Effect {
                            effect: effect.clone(),
                        },
                    );
                }
                self.talk
                    .t("Je le mets dans À faire…", "Putting it in To do…")
                    .to_owned()
            }
        }
    }

    /// A move's proposal accepted: the message, ready, as a draft card
    /// for the user to send; nothing leaves before that.
    fn prepared(&mut self, item: u64, proposal: &Proposal) -> String {
        let to = proposal.input["to"].as_str().unwrap_or_default().to_owned();
        let draft = proposal.input["draft"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let card = Card {
            kind: CardKind::Draft,
            label: if self.english() {
                format!("Message · to {to}")
            } else {
                format!("Message · à {to}")
            },
            text: String::new(),
            lines: vec![draft.clone()],
            actions: vec![
                Action {
                    label: self.talk.t("Envoyer", "Send").to_owned(),
                    decision: Decision::Accept,
                },
                Action {
                    label: self.talk.t("Modifier", "Edit").to_owned(),
                    decision: Decision::Edit,
                },
                Action {
                    label: self.talk.t("Ne pas envoyer", "Don’t send").to_owned(),
                    decision: Decision::Reject,
                },
            ],
            decided: None,
            outcome: None,
            sheet: None,
        };
        let draft_item = self.add_item_in(None, Body::Card { card }, Some(proposal.notebook));
        self.talk.proposals.insert(
            draft_item,
            Proposal {
                kind: Kind::Message,
                tool: "propose_message".to_owned(),
                input: serde_json::json!({ "to": to, "draft": draft, "from": format!("card:{item}") }),
                notebook: proposal.notebook,
            },
        );
        self.audited(audit::Entry {
            kind: audit::Kind::Proposal,
            what: "message".to_owned(),
            ids: vec![format!("card:{draft_item}")],
            ..Default::default()
        });
        self.talk
            .t(
                "Message prêt ci-dessous : rien ne part avant « Envoyer ».",
                "Message ready below: nothing leaves before “Send”.",
            )
            .to_owned()
    }

    /// The shell carried an effect out, or not.
    pub(super) fn effected(&mut self, effect: u64, ok: bool, detail: &str) {
        let Some((card, proposal)) = self.speaking.effects.remove(&effect) else {
            return;
        };
        tracing::info!(effect, ok, "the shell carried an effect out");
        let en = self.english();
        let outcome = match (ok, en) {
            (true, false) => "Mis dans À faire.".to_owned(),
            (true, true) => "Put in To do.".to_owned(),
            (false, false) => format!("Pas mis dans À faire\u{a0}: {detail}"),
            (false, true) => format!("Not put in To do: {detail}"),
        };
        if ok
            && proposal.kind == Kind::Task
            && let Some(title) = crate::effects::ToDo::title(&proposal.input)
        {
            let today = self.store.today();
            let project = proposal.input["project"].as_str();
            if let Err(err) = day::plan(
                &self.store.conn,
                today,
                &title,
                project,
                proposal.notebook,
                self.clock.now(),
            ) {
                tracing::warn!(%err, "could not keep the plan");
            }
        }
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "effect".to_owned(),
            ids: vec![format!("card:{card}")],
            outcome: Some(if ok { "done" } else { "failed" }.to_owned()),
            ..Default::default()
        });
        self.change_item(card, |body| {
            if let Body::Card { card } = body {
                card.outcome = Some(outcome);
            }
        });
    }
}
