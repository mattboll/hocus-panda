//! The bubbles' stack (5.3, variant A) and the history (5.4, variant B):
//! which notifications show, where, how long, what each event does, and
//! what the history keeps. Pure: it takes what happened and the time, and
//! says what to carry out ([`Effect`]).
//!
//! - At most [`MOST`] bubbles, top first, 8 px apart, right of the screen
//!   against the bar (8 px from it, or from the screen's edge without
//!   one), from its top; on the screen of the focused window, taken as the
//!   first bubble shows. The others wait their turn, a "+N" chip under the
//!   stack saying how many.
//! - A normal one counts down its time (6 s, or the application's); a
//!   critical one stays until closed or acted on; a low one makes no
//!   bubble. So does nothing for an agents' tool saying only that its
//!   agent waits or finished.
//! - Every notification goes to the history as it comes. The bell's badge
//!   counts what was not seen: a bubble that went without the pointer
//!   resting on it or a click, a low one, one held while the user is not
//!   to be disturbed; not an agent's. Opening the history sets it to 0;
//!   what it lists is seen as it closes.
//! - While the user is not to be disturbed, bubbles are held (to the
//!   history only), critical ones still showing; it ends by itself at its
//!   time.
//! - While the screen is shared (5.11), bubbles are held as with do not
//!   disturb, critical ones still showing; when the sharing ends, one
//!   bubble sums up what was held, its card opening the history.
//! - Hovering any bubble pauses every countdown.
//! - A notification replacing one shown updates its bubble in place, its
//!   countdown starting again.
//! - A bubble goes over [`BUBBLE_OUT`] (the compositor fades it out),
//!   then its surface; the ones under it move up, and the next waiting
//!   one shows.

use std::{
    collections::{BTreeSet, VecDeque},
    time::{Duration, Instant},
};

use hocus_link::{
    BUBBLE_OUT, BUBBLES_WAITING_NAMESPACE, EDGE, FOOTER_NAMESPACE, Form, bubble_namespace,
};
use iced::{Rectangle, window};

use super::{
    dnd::Dnd,
    draw::{self, Laid},
    history::{History, Record, Sight},
    note::{Note, Timeout, Urgency},
    server::Signal,
};

/// The most bubbles shown at once.
pub const MOST: usize = 3;
/// The id of the bubble that sums up what was held while sharing: the
/// server's ids start from 1, and no signal goes for it.
pub const SUMMARY: u32 = 0;
/// Between two bubbles, and between the last and the chip.
const GAP: f32 = 8.0;

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// A notification, new or replacing one (same id).
    Notify(Box<Note>),
    /// A notification of the shell's own straight to the history, seen,
    /// replacing one of its id: a capture (5.9), a meeting's reminder
    /// (Day.6).
    Keep(Box<Note>),
    /// One of the shell's own in the history says something new, in its
    /// place (a reminder's meeting ended: no action).
    Update(Box<Note>),
    /// `CloseNotification`.
    Close(u32),
    /// The compositor says the focused window's screen.
    Focus(String),
    /// The bars changed (one came, went, or changed form).
    Bars(Vec<(String, Form)>),
    /// The pointer came onto bubble `serial` (`on`), or left it.
    Hover { serial: u64, on: bool },
    /// A click on the card of bubble `serial`.
    Card(u64),
    /// Its ✕.
    Dismiss(u64),
    /// One of its actions.
    Action { serial: u64, key: String },
    /// The history's panel opened (`open`) or closed, the wall clock
    /// reading `wall` (milliseconds).
    History { open: bool, wall: i64 },
    /// A click on a notification of the history, by its key.
    Open(u64),
    /// Its ✕, or "clear all": these go.
    Remove(Vec<u64>),
    /// One of its actions, in the history.
    Act { key: u64, action: String },
    /// Do not disturb, as the user set it.
    Dnd(Dnd),
    /// The screen is shared (5.11), or no longer is.
    Sharing(bool),
    /// The session is locked (7.1), or no longer is: new bubbles wait for
    /// the unlock.
    Locked(bool),
    /// What the history kept before the shell started again, and do not
    /// disturb as it was in this session.
    Restored { notes: Vec<(Note, Sight)>, dnd: Dnd },
    /// The wall clock may have moved apart from the monotonic clock (the
    /// machine resumed, the clock was set, do not disturb's wall-clock
    /// timer rang), reading `wall` (milliseconds): do not disturb's end
    /// read again.
    Clock { wall: i64 },
}

/// What to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A layer surface: a bubble's or the chip's, on `screen`, anchored to
    /// the top right, `size`, its margins (top, right, bottom, left); it
    /// takes the pointer in `region` only (none: nowhere).
    Open {
        id: window::Id,
        screen: String,
        namespace: String,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
        region: Option<Rectangle>,
    },
    /// Surface `id` to this size and these margins.
    Place {
        id: window::Id,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
        region: Option<Rectangle>,
    },
    /// The surfaces of `namespace` draw again.
    Wake(String),
    /// The compositor fades the bubble of `namespace` out.
    Closing(String),
    Remove(window::Id),
    Signal(Signal),
    /// The compositor brings the application's window.
    Raise {
        pid: u32,
        app: String,
    },
    /// The compositor brings the window of the application or of its
    /// agent, else opens its agent's conversation (#823).
    Converse {
        pid: u32,
        app: String,
        conversation: hocus_link::AgentConversation,
    },
    /// What the history lists changed: its panel draws again, at its new
    /// size.
    Changed,
    /// The history to write, as it is now.
    Save(Vec<Record>),
    /// Do not disturb to write, as it is now.
    SaveDnd(Dnd),
    /// The history opens on this screen's bar (the summary's card).
    History(String),
}

/// A bubble's countdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Countdown {
    /// It stays (critical, or never expiring).
    Off,
    Running {
        until: Instant,
        total: Duration,
    },
    Paused {
        left: Duration,
        total: Duration,
    },
}

impl Countdown {
    fn of(timeout: Timeout, paused: bool, now: Instant) -> Self {
        match timeout {
            Timeout::Never => Self::Off,
            Timeout::After(total) if paused => Self::Paused { left: total, total },
            Timeout::After(total) => Self::Running {
                until: now + total,
                total,
            },
        }
    }

    /// What is left of it at `now`, from 1 to 0; `None`: it does not count
    /// down.
    pub fn left(&self, now: Instant) -> Option<f32> {
        match *self {
            Self::Off => None,
            Self::Running { until, total } => Some(
                (until.saturating_duration_since(now).as_secs_f32()
                    / total.as_secs_f32().max(1e-3))
                .clamp(0.0, 1.0),
            ),
            Self::Paused { left, total } => {
                Some((left.as_secs_f32() / total.as_secs_f32().max(1e-3)).clamp(0.0, 1.0))
            }
        }
    }

    fn pause(&mut self, now: Instant) {
        if let Self::Running { until, total } = *self {
            *self = Self::Paused {
                left: until.saturating_duration_since(now),
                total,
            };
        }
    }

    fn resume(&mut self, now: Instant) {
        if let Self::Paused { left, total } = *self {
            *self = Self::Running {
                until: now + left,
                total,
            };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Shown(Countdown),
    /// Going, its surface removed then.
    Leaving(Instant),
}

/// A bubble, shown or going.
#[derive(Debug, Clone)]
pub struct Bubble {
    pub serial: u64,
    pub id: window::Id,
    pub note: Note,
    pub laid: Laid,
    phase: Phase,
    /// The pointer rested on it: it was seen.
    rested: bool,
    screen: String,
    /// Where it is: its size and margins.
    placed: ((u32, u32), (i32, i32, i32, i32)),
}

impl Bubble {
    pub fn namespace(&self) -> String {
        bubble_namespace(self.serial)
    }

    /// Its countdown, while it shows.
    pub fn countdown(&self) -> Countdown {
        match self.phase {
            Phase::Shown(countdown) => countdown,
            Phase::Leaving(_) => Countdown::Off,
        }
    }

    fn leaving(&self) -> bool {
        matches!(self.phase, Phase::Leaving(_))
    }
}

/// A surface of the stack's other than a bubble: the chip.
#[derive(Debug, Clone, PartialEq)]
struct Extra {
    id: window::Id,
    screen: String,
    placed: ((u32, u32), (i32, i32, i32, i32)),
}

/// The stack.
#[derive(Debug)]
pub struct Stack {
    bubbles: Vec<Bubble>,
    queue: VecDeque<Note>,
    pub history: History,
    pub dnd: Dnd,
    /// While the screen is shared (5.11): the applications of the
    /// notifications held meanwhile, one each, in their order.
    sharing: Option<Vec<String>>,
    /// The session is locked (7.1): bubbles wait in the queue.
    locked: bool,
    /// What was kept before is read back: until then, nothing is written
    /// over it.
    restored: bool,
    hovered: BTreeSet<u64>,
    /// The focused window's screen, as last told.
    focus: Option<String>,
    /// The bars' screens and forms.
    bars: Vec<(String, Form)>,
    /// The shell draws on the CPU: its surfaces are their cards alone.
    card: bool,
    next: u64,
    chip: Option<(Extra, usize)>,
}

/// What a click on `note` asks the compositor: its application's window,
/// or, without a default action of its own, its agent's conversation when
/// it has one (#823).
fn bring(note: &Note) -> Effect {
    let pid = note.pid;
    let app = note.entry.clone().unwrap_or_else(|| note.app.clone());
    match &note.conversation {
        Some(conversation) if !note.default => Effect::Converse {
            pid,
            app,
            conversation: conversation.clone(),
        },
        _ => Effect::Raise { pid, app },
    }
}

/// What, changing, is written or drawn again: the history's version, the
/// badge, do not disturb.
type Marks = (u64, usize, Dnd);

impl Stack {
    pub fn new(card: bool) -> Self {
        Self {
            bubbles: Vec::new(),
            queue: VecDeque::new(),
            history: History::default(),
            dnd: Dnd::Off,
            sharing: None,
            locked: false,
            restored: false,
            hovered: BTreeSet::new(),
            focus: None,
            bars: Vec::new(),
            card,
            next: 1,
            chip: None,
        }
    }

    /// The bubbles showing, from the first.
    pub fn bubbles(&self) -> &[Bubble] {
        &self.bubbles
    }

    /// The bubble whose surface is `id`.
    pub fn bubble(&self, id: window::Id) -> Option<&Bubble> {
        self.bubbles.iter().find(|b| b.id == id)
    }

    /// How many wait, if the chip's surface is `id`.
    pub fn chip(&self, id: window::Id) -> Option<usize> {
        self.chip
            .as_ref()
            .filter(|(chip, _)| chip.id == id)
            .map(|(_, n)| *n)
    }

    /// How many notifications were held since the screen is shared.
    pub fn held_while_sharing(&self) -> usize {
        self.sharing.as_ref().map_or(0, Vec::len)
    }

    /// What the bell's badge counts.
    pub fn badge(&self) -> usize {
        self.history.unseen()
    }

    /// Whether the countdowns are paused (a bubble is hovered).
    pub fn paused(&self) -> bool {
        !self.hovered.is_empty()
    }

    /// When [`Self::tick`] has something to do.
    pub fn deadline(&self) -> Option<Instant> {
        self.bubbles
            .iter()
            .filter_map(|b| match b.phase {
                Phase::Shown(Countdown::Running { until, .. }) => Some(until),
                Phase::Leaving(at) => Some(at),
                Phase::Shown(_) => None,
            })
            .chain(self.dnd.deadline())
            .min()
    }

    fn marks(&self) -> Marks {
        (self.history.version(), self.history.unseen(), self.dnd)
    }

    /// What changed since `before`: the history's panel and the bell drawn
    /// again, what is kept written (once what was kept is read back).
    fn settle(&mut self, before: Marks, effects: &mut Vec<Effect>) {
        // Held, gone from the history for their age (or past the most kept):
        // closed as expired.
        for id in self.history.take_dropped() {
            tracing::debug!(id, "a notification leaves the history");
            effects.push(Effect::Signal(Signal::Closed { id, reason: 1 }));
        }
        let (version, unseen, dnd) = self.marks();
        if version != before.0 {
            effects.push(Effect::Changed);
            if self.restored {
                effects.push(Effect::Save(
                    self.history.items().iter().map(Record::of).collect(),
                ));
            }
        }
        if unseen != before.1 || dnd.is_on() != before.2.is_on() {
            tracing::debug!(unread = unseen, dnd = dnd.is_on(), "the bell");
            effects.push(Effect::Wake(FOOTER_NAMESPACE.to_owned()));
        }
        if dnd != before.2 {
            effects.push(Effect::Changed);
            if self.restored {
                effects.push(Effect::SaveDnd(dnd));
            }
        }
    }

    /// What `input` at `now` does.
    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        let before = self.marks();
        let mut effects = Vec::new();
        self.handle_input(input, now, &mut effects);
        self.settle(before, &mut effects);
        effects
    }

    fn handle_input(&mut self, input: Input, now: Instant, effects: &mut Vec<Effect>) {
        match input {
            Input::Notify(note) => self.notify(*note, now, effects),
            Input::Keep(note) => {
                self.history.prune(note.at);
                self.history.add(*note, Sight::Seen);
            }
            Input::Update(note) => self.history.update(*note),
            Input::Close(id) => self.close(id, now, effects),
            Input::Focus(screen) => self.focus = Some(screen),
            Input::Bars(bars) => {
                self.bars = bars;
                self.relayout(effects);
            }
            Input::Hover { serial, on } => {
                let before = self.paused();
                let bubble = self
                    .bubbles
                    .iter_mut()
                    .find(|b| b.serial == serial && !matches!(b.phase, Phase::Leaving(_)));
                match bubble {
                    Some(bubble) if on => {
                        bubble.rested = true;
                        self.hovered.insert(serial);
                    }
                    _ => {
                        self.hovered.remove(&serial);
                    }
                }
                if before != self.paused() {
                    self.set_paused(self.paused(), now, effects);
                }
            }
            Input::Card(serial) => {
                let Some(bubble) = self.showing(serial) else {
                    return;
                };
                if bubble.note.id == SUMMARY {
                    tracing::debug!("the summary's card opens the history");
                    effects.push(Effect::History(bubble.screen.clone()));
                    self.leave_silently(serial, now, effects);
                    return;
                }
                let note = bubble.note.clone();
                tracing::debug!(id = note.id, "a bubble's card clicked");
                if note.default {
                    effects.push(Effect::Signal(Signal::Action {
                        id: note.id,
                        key: "default".to_owned(),
                    }));
                }
                effects.push(bring(&note));
                self.history.set_seen(note.id, Sight::Seen);
                if !note.resident {
                    self.leave(serial, 2, now, effects);
                }
            }
            Input::Dismiss(serial) => {
                tracing::debug!(serial, "a bubble dismissed");
                self.leave(serial, 2, now, effects);
            }
            Input::Action { serial, key } => {
                let Some(bubble) = self.showing(serial) else {
                    return;
                };
                let (id, resident) = (bubble.note.id, bubble.note.resident);
                tracing::debug!(id, key, "a bubble's action");
                effects.push(Effect::Signal(Signal::Action { id, key }));
                self.history.set_seen(id, Sight::Seen);
                if !resident {
                    self.leave(serial, 2, now, effects);
                }
            }
            Input::History { open, wall } => {
                self.history.prune(wall);
                if self.history.set_open(open) {
                    tracing::debug!(open, "the history");
                }
            }
            Input::Open(key) => {
                let Some(kept) = self.history.get(key) else {
                    return;
                };
                let note = kept.note.clone();
                let live = kept.live;
                tracing::debug!(
                    key,
                    live = live.is_some(),
                    "a notification of the history opened"
                );
                if let Some(id) = live
                    && note.default
                {
                    effects.push(Effect::Signal(Signal::Action {
                        id,
                        key: "default".to_owned(),
                    }));
                }
                // A capture has no window to raise: its editor opens; nor a
                // meeting's reminder: its call does.
                if super::capture_of(note.id).is_none() && super::reminder_of(note.id).is_none() {
                    effects.push(bring(&note));
                }
                self.forget(key, now, effects);
            }
            Input::Remove(keys) => {
                tracing::debug!(count = keys.len(), "notifications taken out of the history");
                for key in keys {
                    self.forget(key, now, effects);
                }
            }
            Input::Act { key, action } => {
                let Some(kept) = self.history.get(key) else {
                    return;
                };
                let Some(id) = kept.live else {
                    return;
                };
                let resident = kept.note.resident;
                tracing::debug!(id, action, "an action of the history");
                effects.push(Effect::Signal(Signal::Action { id, key: action }));
                if !resident {
                    self.forget(key, now, effects);
                }
            }
            Input::Dnd(dnd) => {
                tracing::debug!(on = dnd.is_on(), choice = ?dnd.choice(), "do not disturb");
                self.dnd = dnd;
            }
            Input::Clock { wall } => self.dnd_checked(now, wall),
            Input::Sharing(true) => {
                if self.sharing.is_none() {
                    tracing::debug!("the screen is shared: bubbles are held");
                    self.sharing = Some(Vec::new());
                }
            }
            Input::Locked(locked) => {
                tracing::debug!(locked, "the lock: bubbles wait while locked");
                self.locked = locked;
                if !locked {
                    self.fill(now);
                    self.relayout(effects);
                }
            }
            Input::Sharing(false) => {
                let Some(held) = self.sharing.take() else {
                    return;
                };
                tracing::debug!(held = held.len(), "sharing ended");
                if let Some(note) = summary(&held, super::wall_ms()) {
                    tracing::debug!(
                        title = note.summary,
                        body = note.body,
                        "the summary of what was held"
                    );
                    self.queue.push_back(note);
                    self.fill(now);
                    self.relayout(effects);
                }
            }
            Input::Restored { notes, dnd } => {
                tracing::info!(
                    kept = notes.len(),
                    dnd = dnd.is_on(),
                    "the history read back"
                );
                // Written then, with what came meanwhile (nothing was until
                // then).
                self.history.restore(notes);
                if !self.dnd.is_on() && !dnd.over(now, super::wall_ms()) {
                    self.dnd = dnd;
                }
                self.restored = true;
            }
        }
    }

    /// The notification of the history `key` goes; its sender, if it still
    /// holds it, hears that the user closed it (2), its bubble going too.
    fn forget(&mut self, key: u64, now: Instant, effects: &mut Vec<Effect>) {
        let Some(kept) = self.history.remove(key) else {
            return;
        };
        let Some(id) = kept.live else {
            return;
        };
        if let Some(serial) = self
            .bubbles
            .iter()
            .find(|b| b.note.id == id && !b.leaving())
            .map(|b| b.serial)
        {
            self.leave(serial, 2, now, effects);
            return;
        }
        let before = self.queue.len();
        self.queue.retain(|n| n.id != id);
        if self.queue.len() != before {
            self.relayout(effects);
        }
        self.closed(id, 2, effects);
    }

    /// Notification `id` closed for `reason`: its sender hears it, and holds
    /// it no more.
    fn closed(&mut self, id: u32, reason: u32, effects: &mut Vec<Effect>) {
        if id == SUMMARY {
            return;
        }
        effects.push(Effect::Signal(Signal::Closed { id, reason }));
        self.history.closed(id);
    }

    /// Do not disturb at `now`, the wall clock reading `wall`: over, or its
    /// end read again (the morning's, at the wall clock's 8:00).
    fn dnd_checked(&mut self, now: Instant, wall: i64) {
        let dnd = self.dnd.checked(now, wall);
        if dnd != self.dnd && !dnd.is_on() {
            tracing::debug!("do not disturb ends at its time");
        }
        self.dnd = dnd;
    }

    /// What is due at `now`: do not disturb over, countdowns ended,
    /// bubbles gone.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let before = self.marks();
        let mut effects = Vec::new();
        self.dnd_checked(now, super::wall_ms());
        let expired: Vec<u64> = self
            .bubbles
            .iter()
            .filter(|b| matches!(b.phase, Phase::Shown(Countdown::Running { until, .. }) if until <= now))
            .map(|b| b.serial)
            .collect();
        for serial in expired {
            self.expire(serial, now, &mut effects);
        }
        let gone: Vec<window::Id> = self
            .bubbles
            .iter()
            .filter(|b| matches!(b.phase, Phase::Leaving(at) if at <= now))
            .map(|b| b.id)
            .collect();
        if !gone.is_empty() {
            self.bubbles.retain(|b| !gone.contains(&b.id));
            effects.extend(gone.into_iter().map(Effect::Remove));
            self.fill(now);
            self.relayout(&mut effects);
        }
        self.settle(before, &mut effects);
        effects
    }

    fn showing(&self, serial: u64) -> Option<&Bubble> {
        self.bubbles
            .iter()
            .find(|b| b.serial == serial && !b.leaving())
    }

    fn notify(&mut self, note: Note, now: Instant, effects: &mut Vec<Effect>) {
        let id = note.id;
        self.history.prune(note.at);
        let quiet = note.agent || note.urgency == Urgency::Low;
        // A replacement: in place, wherever it is.
        if let Some(bubble) = self
            .bubbles
            .iter_mut()
            .find(|b| b.note.id == id && !b.leaving())
        {
            if quiet {
                let serial = bubble.serial;
                self.leave_silently(serial, now, effects);
                self.keep(note);
                return;
            }
            tracing::debug!(id, "a bubble updated in place");
            let paused = !self.hovered.is_empty();
            bubble.laid = draw::lay(&note);
            bubble.phase = Phase::Shown(Countdown::of(note.timeout, paused, now));
            bubble.note = note.clone();
            effects.push(Effect::Wake(bubble.namespace()));
            self.history.update(note);
            self.relayout(effects);
            return;
        }
        if let Some(waiting) = self.queue.iter_mut().find(|n| n.id == id) {
            *waiting = note.clone();
            self.history.update(note);
            return;
        }
        if quiet {
            tracing::info!(
                id,
                app = note.app,
                low = note.urgency == Urgency::Low,
                agent = note.agent,
                "a notification without a bubble: to the history only"
            );
            self.keep(note);
            return;
        }
        if self.dnd.is_on() && note.urgency != Urgency::Critical {
            tracing::info!(
                id,
                app = note.app,
                "a notification's bubble held: do not disturb"
            );
            tracing::debug!(id, title = note.summary, "held: do not disturb");
            if let Some(held) = &mut self.sharing {
                held.push(note.app.clone());
            }
            self.history.add(note, Sight::Unseen);
            return;
        }
        if let Some(held) = &mut self.sharing
            && note.urgency != Urgency::Critical
        {
            tracing::info!(
                id,
                app = note.app,
                "a notification's bubble held: the screen is shared"
            );
            tracing::debug!(id, title = note.summary, "held: the screen is shared");
            held.push(note.app.clone());
            self.history.add(note, Sight::Unseen);
            return;
        }
        let app = note.app.clone();
        self.history.add(note.clone(), Sight::Showing);
        self.queue.push_back(note);
        self.fill(now);
        self.relayout(effects);
        // Not shown at once: said, so that the journal tells why.
        if let Some(place) = self.queue.iter().position(|n| n.id == id) {
            if self.locked {
                tracing::info!(id, app, "a notification's bubble waits for the unlock");
            } else {
                let behind = self.bubbles.iter().filter(|b| !b.leaving()).count();
                tracing::info!(
                    id,
                    app,
                    behind,
                    place = place + 1,
                    "a notification's bubble waits its turn"
                );
            }
        }
    }

    /// Straight to the history: a low one counted by the badge, an agent's
    /// not.
    fn keep(&mut self, note: Note) {
        let seen = if note.agent {
            Sight::Seen
        } else {
            Sight::Unseen
        };
        let (id, agent) = (note.id, note.agent);
        self.history.add(note, seen);
        tracing::debug!(
            id,
            agent,
            unread = self.history.unseen(),
            "a notification to the history"
        );
    }

    fn close(&mut self, id: u32, now: Instant, effects: &mut Vec<Effect>) {
        // Its application takes it back: from the history too.
        let kept = self.history.remove_live(id);
        if let Some(serial) = self
            .bubbles
            .iter()
            .find(|b| b.note.id == id && !b.leaving())
            .map(|b| b.serial)
        {
            self.leave(serial, 3, now, effects);
            return;
        }
        let before = self.queue.len();
        self.queue.retain(|n| n.id != id);
        if kept || self.queue.len() != before {
            self.closed(id, 3, effects);
            self.relayout(effects);
        }
    }

    /// Bubble `serial`'s time is up: it goes, its notification still held
    /// (the server's `persistence`: its actions work from the history).
    /// Without the pointer having rested on it, it was not seen: the badge
    /// counts it.
    fn expire(&mut self, serial: u64, now: Instant, effects: &mut Vec<Effect>) {
        let Some((id, rested)) = self.showing(serial).map(|b| (b.note.id, b.rested)) else {
            return;
        };
        tracing::debug!(serial, id, "a bubble's time is up");
        let seen = if rested { Sight::Seen } else { Sight::Unseen };
        self.history.set_seen(id, seen);
        self.leave_silently(serial, now, effects);
    }

    /// Bubble `serial` goes, closed for `reason`: seen.
    fn leave(&mut self, serial: u64, reason: u32, now: Instant, effects: &mut Vec<Effect>) {
        let Some(id) = self.showing(serial).map(|b| b.note.id) else {
            return;
        };
        self.history.set_seen(id, Sight::Seen);
        self.closed(id, reason, effects);
        self.leave_silently(serial, now, effects);
    }

    /// Bubble `serial` goes, its notification living on (in the history).
    fn leave_silently(&mut self, serial: u64, now: Instant, effects: &mut Vec<Effect>) {
        let was = self.paused();
        self.hovered.remove(&serial);
        if let Some(bubble) = self.bubbles.iter_mut().find(|b| b.serial == serial) {
            bubble.phase = Phase::Leaving(now + BUBBLE_OUT);
            effects.push(Effect::Closing(bubble.namespace()));
        }
        if was && !self.paused() {
            self.set_paused(false, now, effects);
        }
    }

    fn set_paused(&mut self, paused: bool, now: Instant, effects: &mut Vec<Effect>) {
        tracing::debug!(paused, "the bubbles' countdowns");
        for bubble in &mut self.bubbles {
            if let Phase::Shown(countdown) = &mut bubble.phase {
                if paused {
                    countdown.pause(now);
                } else {
                    countdown.resume(now);
                }
                if *countdown != Countdown::Off {
                    effects.push(Effect::Wake(bubble.namespace()));
                }
            }
        }
    }

    /// Waiting ones show while there is room.
    fn fill(&mut self, now: Instant) {
        // Locked: they wait for the unlock (7.1).
        if self.locked {
            return;
        }
        let screen = match self.bubbles.first() {
            Some(first) => first.screen.clone(),
            None => self.focus_screen(),
        };
        let paused = self.paused();
        while self.bubbles.iter().filter(|b| !b.leaving()).count() < MOST {
            let Some(note) = self.queue.pop_front() else {
                break;
            };
            let serial = self.next;
            self.next += 1;
            let laid = draw::lay(&note);
            // Its application and id, never what it says: the journal is
            // read by others.
            tracing::info!(
                id = note.id,
                app = note.app,
                serial,
                screen,
                "a notification's bubble shows"
            );
            tracing::debug!(
                id = note.id,
                serial,
                screen,
                title = note.summary,
                actions = note.actions.len(),
                critical = note.urgency == Urgency::Critical,
                progress = ?note.progress,
                "a bubble shows"
            );
            let bubble = Bubble {
                serial,
                id: window::Id::unique(),
                rested: false,
                phase: Phase::Shown(Countdown::of(note.timeout, paused, now)),
                note,
                laid,
                screen: screen.clone(),
                placed: ((0, 0), (0, 0, 0, 0)),
            };
            // Opened by the layout, where it goes.
            self.bubbles.push(bubble);
        }
    }

    /// Where the bubbles go now: the focused window's screen, else the
    /// first bar's.
    fn focus_screen(&self) -> String {
        self.focus
            .clone()
            .or_else(|| self.bars.first().map(|(s, _)| s.clone()))
            .unwrap_or_else(|| crate::ALONE.to_owned())
    }

    /// From the screen's right edge to the cards': past the bar and its
    /// gap, or the edge's gap without a bar there (none on the left: the
    /// bubbles stay in the right corner).
    fn right(&self, screen: &str) -> f32 {
        let bar = self
            .bars
            .iter()
            .filter(|_| !crate::side::left())
            .find(|(s, _)| s == screen)
            .map_or(0, |(_, form)| form.width() + EDGE);
        (EDGE + bar) as f32
    }

    /// Each bubble where it goes, opened the first time; the chip under
    /// them.
    fn relayout(&mut self, effects: &mut Vec<Effect>) {
        let Some(screen) = self.bubbles.first().map(|b| b.screen.clone()) else {
            self.place_chip(None, effects);
            return;
        };
        let right = self.right(&screen);
        let mut top = EDGE as f32;
        let card = self.card;
        for bubble in &mut self.bubbles {
            let (size, margin, region) = draw::placed(bubble.laid.size, top, right, card);
            top += bubble.laid.size.height + GAP;
            if bubble.placed == ((0, 0), (0, 0, 0, 0)) {
                effects.push(Effect::Open {
                    id: bubble.id,
                    screen: bubble.screen.clone(),
                    namespace: bubble.namespace(),
                    size,
                    margin,
                    region: Some(region),
                });
            } else if bubble.placed != (size, margin) {
                effects.push(Effect::Place {
                    id: bubble.id,
                    size,
                    margin,
                    region: Some(region),
                });
            }
            bubble.placed = (size, margin);
        }
        let waiting = (!self.queue.is_empty()).then_some((screen, top, right));
        self.place_chip(waiting, effects);
    }

    /// The chip saying how many wait, its top at `top` on `screen`, `right`
    /// from its edge; or none.
    fn place_chip(&mut self, at: Option<(String, f32, f32)>, effects: &mut Vec<Effect>) {
        let n = self.queue.len();
        let Some((screen, top, right)) = at else {
            if let Some((chip, _)) = self.chip.take() {
                effects.push(Effect::Remove(chip.id));
            }
            return;
        };
        let size = draw::chip_size(n);
        let margin = (top.round() as i32, right.round() as i32, 0, 0);
        match &mut self.chip {
            Some((chip, shown)) if chip.screen == screen => {
                if chip.placed != (size, margin) {
                    chip.placed = (size, margin);
                    effects.push(Effect::Place {
                        id: chip.id,
                        size,
                        margin,
                        region: None,
                    });
                }
                if *shown != n {
                    *shown = n;
                    tracing::debug!(n, "the waiting chip");
                    effects.push(Effect::Wake(BUBBLES_WAITING_NAMESPACE.to_owned()));
                }
            }
            _ => {
                if let Some((chip, _)) = self.chip.take() {
                    effects.push(Effect::Remove(chip.id));
                }
                let id = window::Id::unique();
                tracing::debug!(n, "the waiting chip");
                effects.push(Effect::Open {
                    id,
                    screen: screen.clone(),
                    namespace: BUBBLES_WAITING_NAMESPACE.to_owned(),
                    size,
                    margin,
                    region: None,
                });
                self.chip = Some((
                    Extra {
                        id,
                        screen,
                        placed: (size, margin),
                    },
                    n,
                ));
            }
        }
    }
}

/// The bubble that sums up the notifications of applications `held`
/// (one each, in their order) while the screen was shared: none when
/// nothing was held.
pub fn summary(held: &[String], at: i64) -> Option<Note> {
    if held.is_empty() {
        return None;
    }
    let fr = crate::clock::time::Lang::of_messages() == crate::clock::time::Lang::French;
    let (summary, body) = summary_words(held, fr);
    Some(Note {
        id: SUMMARY,
        app: "Hocuspanda".to_owned(),
        entry: None,
        icon: Some(crate::levels::icons::Icon::Vector(summary_icon())),
        summary,
        body,
        actions: Vec::new(),
        default: false,
        urgency: Urgency::Normal,
        timeout: Timeout::of(-1, Urgency::Normal),
        progress: None,
        resident: false,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: super::note::IconFrom::default(),
        at,
    })
}

/// "4 notifications pendant le partage" and "Slack (2), Signal ·
/// dans l'historique".
pub fn summary_words(held: &[String], fr: bool) -> (String, String) {
    let mut apps: Vec<(String, usize)> = Vec::new();
    for app in held {
        let app = if app.trim().is_empty() {
            if fr {
                "Une application"
            } else {
                "An application"
            }
            .to_owned()
        } else {
            app.trim().to_owned()
        };
        match apps.iter_mut().find(|(a, _)| *a == app) {
            Some((_, n)) => *n += 1,
            None => apps.push((app, 1)),
        }
    }
    let n = held.len();
    let summary = match (n, fr) {
        (1, true) => "1 notification pendant le partage".to_owned(),
        (1, false) => "1 notification while sharing".to_owned(),
        (n, true) => format!("{n} notifications pendant le partage"),
        (n, false) => format!("{n} notifications while sharing"),
    };
    let list: Vec<String> = apps
        .iter()
        .map(|(app, n)| {
            if *n > 1 {
                format!("{app} ({n})")
            } else {
                app.clone()
            }
        })
        .collect();
    let body = format!(
        "{} · {}",
        list.join(", "),
        if fr {
            "dans l'historique"
        } else {
            "in the history"
        }
    );
    (summary, body)
}

/// The summary's icon: the mockup's bell, `#B9CEFF`, on
/// `rgba(91,141,239,0.25)`.
fn summary_icon() -> iced::advanced::svg::Handle {
    static ICON: std::sync::OnceLock<iced::advanced::svg::Handle> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        iced::advanced::svg::Handle::from_memory(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\" width=\"32\" \
             height=\"32\"><rect width=\"32\" height=\"32\" rx=\"9\" fill=\"#5B8DEF\" \
             fill-opacity=\"0.25\"/><g transform=\"translate(7 7) scale(0.75)\"><path d=\"M6 \
             16.5 V11 a6 6 0 0 1 12 0 V16.5 L19.5 18 H4.5 Z M10 20.5 a2 2 0 0 0 4 0\" \
             fill=\"none\" stroke=\"#B9CEFF\" stroke-width=\"1.8\" \
             stroke-linejoin=\"round\"/></g></svg>"
                .as_bytes()
                .to_vec(),
        )
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::note::LIFE;

    const SCREEN: &str = "eDP-1";
    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn note(id: u32) -> Note {
        Note {
            id,
            app: "Slack".to_owned(),
            entry: None,
            icon: None,
            summary: format!("Message {id}"),
            body: "Bonjour".to_owned(),
            actions: Vec::new(),
            default: false,
            urgency: Urgency::Normal,
            timeout: Timeout::After(LIFE),
            progress: None,
            resident: false,
            pid: 0,
            agent: false,
            conversation: None,
            icon_from: crate::notifications::note::IconFrom::default(),
            at: 1_000_000,
        }
    }

    fn notify(note: Note) -> Input {
        Input::Notify(Box::new(note))
    }

    fn stack() -> Stack {
        let mut stack = Stack::new(true);
        let now = Instant::now();
        stack.handle(Input::Bars(vec![(SCREEN.to_owned(), Form::Full)]), now);
        stack.handle(Input::Focus(SCREEN.to_owned()), now);
        stack
    }

    fn opened(effects: &[Effect]) -> Vec<(String, (i32, i32, i32, i32))> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Open {
                    namespace, margin, ..
                } => Some((namespace.clone(), *margin)),
                _ => None,
            })
            .collect()
    }

    fn signals(effects: &[Effect]) -> Vec<Signal> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Signal(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_bubble_shows_against_the_bar_and_goes_after_its_time() {
        let mut s = stack();
        let t0 = Instant::now();
        let effects = s.handle(notify(note(1)), t0);
        let open = opened(&effects);
        assert_eq!(open.len(), 1);
        // Its top 8 px from the screen's, its right 8 px from the full bar.
        assert_eq!(open[0].1, (8, 8 + 196 + 8, 0, 0));
        assert_eq!(s.deadline(), Some(t0 + LIFE));
        assert!(s.tick(t0 + LIFE - MS(1)).is_empty());
        // Its time up, it goes; its notification is still held (nothing
        // said to its sender), in the history.
        let effects = s.tick(t0 + LIFE);
        assert!(signals(&effects).is_empty(), "{effects:?}");
        assert!(effects.contains(&Effect::Closing(bubble_namespace(1))));
        assert_eq!(s.history.items()[0].live, Some(1));
        // Its surface goes once it faded out.
        let effects = s.tick(t0 + LIFE + BUBBLE_OUT);
        assert!(matches!(effects[..], [Effect::Remove(_)]), "{effects:?}");
        assert_eq!(s.deadline(), None);
    }

    #[test]
    fn hovering_pauses_every_countdown() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(notify(note(1)), t0);
        s.handle(notify(note(2)), t0 + MS(1000));
        s.handle(
            Input::Hover {
                serial: 2,
                on: true,
            },
            t0 + MS(2000),
        );
        assert!(s.paused());
        assert_eq!(s.deadline(), None);
        assert!(s.tick(t0 + MS(60_000)).is_empty());
        // Left at 62 s: the first had 4 s left, the second 5 s.
        let t1 = t0 + MS(62_000);
        s.handle(
            Input::Hover {
                serial: 2,
                on: false,
            },
            t1,
        );
        assert_eq!(s.deadline(), Some(t1 + MS(4000)));
    }

    #[test]
    fn a_critical_one_stays_until_closed() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut critical = note(1);
        critical.urgency = Urgency::Critical;
        critical.timeout = Timeout::of(2000, Urgency::Critical);
        s.handle(notify(critical), t0);
        assert_eq!(s.deadline(), None);
        let effects = s.handle(Input::Dismiss(1), t0 + MS(90_000));
        assert_eq!(signals(&effects), [Signal::Closed { id: 1, reason: 2 }]);
    }

    #[test]
    fn a_low_one_goes_to_the_history_and_the_badge_counts_it() {
        let mut s = stack();
        let mut low = note(1);
        low.urgency = Urgency::Low;
        let effects = s.handle(notify(low), Instant::now());
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(effects.contains(&Effect::Wake(FOOTER_NAMESPACE.to_owned())));
        assert_eq!((s.badge(), s.history.items().len()), (1, 1));
        // An agent's: to the history, no bubble, no count.
        let mut agent = note(2);
        agent.agent = true;
        let effects = s.handle(notify(agent), Instant::now());
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!((s.badge(), s.history.items().len()), (1, 2));
    }

    #[test]
    fn a_replacement_updates_its_bubble_in_place_and_starts_its_countdown_again() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(notify(note(1)), t0);
        let mut later = note(1);
        later.body = "Téléchargement : 38 %".to_owned();
        later.progress = Some(38);
        let effects = s.handle(notify(later), t0 + MS(3000));
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(effects.contains(&Effect::Wake(bubble_namespace(1))));
        assert_eq!(s.deadline(), Some(t0 + MS(3000) + LIFE));
    }

    #[test]
    fn more_than_three_wait_their_turn_under_a_chip() {
        let mut s = stack();
        let t0 = Instant::now();
        for id in 1..=5 {
            s.handle(notify(note(id)), t0);
        }
        assert_eq!(s.bubbles.len(), MOST);
        assert_eq!(s.chip.as_ref().map(|(_, n)| *n), Some(2));
        // One goes: the next shows once its surface is gone.
        s.handle(Input::Dismiss(1), t0 + MS(100));
        let effects = s.tick(t0 + MS(100) + BUBBLE_OUT);
        assert_eq!(opened(&effects).len(), 1);
        assert_eq!(s.chip.as_ref().map(|(_, n)| *n), Some(1));
        // An application closes a waiting one.
        let effects = s.handle(Input::Close(5), t0 + MS(500));
        assert_eq!(signals(&effects), [Signal::Closed { id: 5, reason: 3 }]);
        assert!(s.chip.is_none());
    }

    #[test]
    fn an_agent_s_notification_without_a_default_opens_its_conversation() {
        let mut s = stack();
        let t0 = Instant::now();
        let conversation = hocus_link::AgentConversation {
            tool: "claude-code".to_owned(),
            session: "3f6c2a1e".to_owned(),
            folder: "/home/alex/site".into(),
            transcript: "/home/alex/.claude/projects/-home-alex-site/3f6c2a1e.jsonl".into(),
            pid: 40,
        };
        let mut first = note(1);
        first.app = "Ada".to_owned();
        first.pid = 42;
        first.conversation = Some(conversation.clone());
        s.handle(notify(first), t0);
        let effects = s.handle(Input::Card(1), t0);
        assert!(effects.contains(&Effect::Converse {
            pid: 42,
            app: "Ada".to_owned(),
            conversation: conversation.clone(),
        }));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Raise { .. })));
        // Its own default kept: invoked, its window raised.
        let mut second = note(2);
        second.default = true;
        second.conversation = Some(conversation);
        s.handle(notify(second), t0);
        let effects = s.handle(Input::Card(2), t0);
        assert!(effects.iter().any(|e| matches!(e, Effect::Raise { .. })));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Converse { .. })));
    }

    #[test]
    fn a_click_invokes_the_default_and_raises_an_action_invokes_and_closes() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut first = note(1);
        first.default = true;
        first.entry = Some("slack".to_owned());
        first.pid = 42;
        s.handle(notify(first), t0);
        let effects = s.handle(Input::Card(1), t0);
        assert_eq!(
            signals(&effects),
            [
                Signal::Action {
                    id: 1,
                    key: "default".to_owned()
                },
                Signal::Closed { id: 1, reason: 2 }
            ]
        );
        assert!(effects.contains(&Effect::Raise {
            pid: 42,
            app: "slack".to_owned()
        }));
        let mut second = note(2);
        second.actions = vec![("reply".to_owned(), "Répondre".to_owned())];
        s.handle(notify(second), t0);
        let effects = s.handle(
            Input::Action {
                serial: 2,
                key: "reply".to_owned(),
            },
            t0,
        );
        assert_eq!(
            signals(&effects),
            [
                Signal::Action {
                    id: 2,
                    key: "reply".to_owned()
                },
                Signal::Closed { id: 2, reason: 2 }
            ]
        );
    }

    fn keys(s: &Stack) -> Vec<u64> {
        s.history.items().iter().map(|k| k.key).collect()
    }

    #[test]
    fn the_history_lists_everything_newest_first_and_counts_what_was_not_seen() {
        let mut s = stack();
        let t0 = Instant::now();
        // Shown and gone after its time, the pointer never on it: counted.
        s.handle(notify(note(1)), t0);
        assert_eq!(s.badge(), 0);
        s.tick(t0 + LIFE);
        assert_eq!(s.badge(), 1);
        // Shown, the pointer resting on it before it went: seen.
        s.handle(notify(note(2)), t0 + LIFE);
        let serial = s.bubbles.last().map(|b| b.serial).expect("a bubble");
        s.handle(Input::Hover { serial, on: true }, t0 + LIFE);
        s.handle(Input::Hover { serial, on: false }, t0 + LIFE + MS(10));
        s.tick(t0 + LIFE * 3);
        assert_eq!(s.badge(), 1);
        let mut low = note(3);
        low.urgency = Urgency::Low;
        s.handle(notify(low), t0 + LIFE * 3);
        assert_eq!(s.badge(), 2);
        let ids: Vec<u32> = s.history.items().iter().map(|k| k.note.id).collect();
        assert_eq!(ids, [3, 2, 1]);
        // Opened: the badge says 0; closed, all of it is seen.
        let effects = s.handle(
            Input::History {
                open: true,
                wall: 1_000_000,
            },
            t0 + LIFE * 3,
        );
        assert_eq!(s.badge(), 0);
        assert!(effects.contains(&Effect::Wake(FOOTER_NAMESPACE.to_owned())));
        assert!(s.history.items().iter().any(|k| k.seen == Sight::Unseen));
        s.handle(
            Input::History {
                open: false,
                wall: 1_000_000,
            },
            t0 + LIFE * 3,
        );
        assert_eq!(s.badge(), 0);
        assert!(s.history.items().iter().all(|k| k.seen == Sight::Seen));
    }

    #[test]
    fn a_click_in_the_history_invokes_raises_and_removes_and_the_close_removes() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut low = note(1);
        low.urgency = Urgency::Low;
        low.default = true;
        low.pid = 42;
        low.entry = Some("slack".to_owned());
        s.handle(notify(low), t0);
        let mut other = note(2);
        other.urgency = Urgency::Low;
        s.handle(notify(other), t0);
        let [second, first] = keys(&s)[..] else {
            panic!("{:?}", keys(&s));
        };
        let effects = s.handle(Input::Open(first), t0);
        assert_eq!(
            signals(&effects),
            [
                Signal::Action {
                    id: 1,
                    key: "default".to_owned()
                },
                Signal::Closed { id: 1, reason: 2 }
            ]
        );
        assert!(effects.contains(&Effect::Raise {
            pid: 42,
            app: "slack".to_owned()
        }));
        assert!(effects.contains(&Effect::Changed));
        assert_eq!(keys(&s), [second]);
        // Its ✕: closed, no action.
        let effects = s.handle(Input::Remove(vec![second]), t0);
        assert_eq!(signals(&effects), [Signal::Closed { id: 2, reason: 2 }]);
        assert!(s.history.items().is_empty());
        // A bubble gone after its time: still held, its action works from
        // the history, then it is closed (2).
        let mut expired = note(3);
        expired.actions = vec![("reply".to_owned(), "Répondre".to_owned())];
        s.handle(notify(expired), t0);
        s.tick(t0 + LIFE);
        let key = keys(&s)[0];
        let effects = s.handle(
            Input::Act {
                key,
                action: "reply".to_owned(),
            },
            t0 + LIFE,
        );
        assert_eq!(
            signals(&effects),
            [
                Signal::Action {
                    id: 3,
                    key: "reply".to_owned()
                },
                Signal::Closed { id: 3, reason: 2 }
            ]
        );
        // One read back after a restart is held by no one: raised, nothing
        // said.
        s.handle(
            Input::Restored {
                notes: vec![(note(9), Sight::Seen)],
                dnd: Dnd::Off,
            },
            t0,
        );
        let key = keys(&s)[0];
        let effects = s.handle(Input::Open(key), t0 + LIFE);
        assert!(signals(&effects).is_empty(), "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Raise { .. })));
    }

    #[test]
    fn a_held_one_leaving_the_history_after_seven_days_is_closed_as_expired() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut low = note(1);
        low.urgency = Urgency::Low;
        s.handle(notify(low), t0);
        let week = crate::notifications::history::KEEP_MS;
        let effects = s.handle(
            Input::History {
                open: true,
                wall: 1_000_000 + week,
            },
            t0,
        );
        assert_eq!(signals(&effects), [Signal::Closed { id: 1, reason: 1 }]);
        assert!(s.history.items().is_empty());
    }

    #[test]
    fn clear_all_empties_the_history_and_closes_what_is_held() {
        let mut s = stack();
        let t0 = Instant::now();
        for id in 1..=3 {
            let mut low = note(id);
            low.urgency = Urgency::Low;
            s.handle(notify(low), t0);
        }
        s.handle(notify(note(4)), t0);
        let effects = s.handle(Input::Remove(keys(&s)), t0);
        assert!(s.history.items().is_empty());
        assert_eq!(s.badge(), 0);
        // The bubble on screen goes too.
        assert!(effects.contains(&Effect::Closing(bubble_namespace(1))));
        assert_eq!(signals(&effects).len(), 4);
    }

    #[test]
    fn locked_bubbles_wait_for_the_unlock() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(Input::Locked(true), t0);
        assert!(
            opened(&s.handle(notify(note(1)), t0)).is_empty(),
            "it waits"
        );
        let effects = s.handle(Input::Locked(false), t0);
        assert_eq!(opened(&effects).len(), 1, "shown once unlocked");
    }

    #[test]
    fn sharing_holds_bubbles_and_one_sums_them_up_at_its_end() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(Input::Sharing(true), t0);
        let mut slack = note(1);
        slack.app = "Slack".into();
        let mut slack_again = note(2);
        slack_again.app = "Slack".into();
        let mut signal = note(3);
        signal.app = "Signal".into();
        for n in [slack, slack_again, signal] {
            let effects = s.handle(notify(n), t0);
            assert!(opened(&effects).is_empty(), "held, no bubble");
        }
        let mut urgent = note(4);
        urgent.urgency = Urgency::Critical;
        assert_eq!(opened(&s.handle(notify(urgent), t0)).len(), 1);
        assert_eq!(s.held_while_sharing(), 3);
        // Its end: one bubble sums them up.
        let effects = s.handle(Input::Sharing(false), t0);
        assert_eq!(opened(&effects).len(), 1);
        assert_eq!(s.held_while_sharing(), 0);
        let shown: Vec<&Bubble> = s.bubbles.iter().filter(|b| b.note.id == SUMMARY).collect();
        assert_eq!(shown.len(), 1);
        let (fr, fr_body) = summary_words(&["Slack".into(), "Slack".into(), "Signal".into()], true);
        assert_eq!(fr, "3 notifications pendant le partage");
        assert_eq!(fr_body, "Slack (2), Signal · dans l'historique");
        let (en, en_body) = summary_words(&["Thunderbird".into()], false);
        assert_eq!(en, "1 notification while sharing");
        assert_eq!(en_body, "Thunderbird · in the history");
        // Its card opens the history, and says nothing to the bus.
        let serial = shown[0].serial;
        let effects = s.handle(Input::Card(serial), t0);
        assert!(effects.iter().any(|e| matches!(e, Effect::History(_))));
        assert!(signals(&effects).is_empty());
        // Nothing held: no summary.
        s.handle(Input::Sharing(true), t0);
        assert!(opened(&s.handle(Input::Sharing(false), t0)).is_empty());
    }

    #[test]
    fn do_not_disturb_holds_bubbles_but_critical_ones_and_ends_at_its_time() {
        let mut s = stack();
        let t0 = Instant::now();
        let wall: jiff::Zoned = "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time");
        let dnd = Dnd::starting(super::super::dnd::Choice::Hour, t0, &wall);
        let effects = s.handle(Input::Dnd(dnd), t0);
        assert!(effects.contains(&Effect::Wake(FOOTER_NAMESPACE.to_owned())));
        assert_eq!(s.deadline(), Some(t0 + Duration::from_secs(3600)));
        // Held: no bubble, to the history, counted.
        let effects = s.handle(notify(note(1)), t0 + MS(10));
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!((s.badge(), s.history.items().len()), (1, 1));
        // A critical one shows.
        let mut critical = note(2);
        critical.urgency = Urgency::Critical;
        critical.timeout = Timeout::Never;
        let effects = s.handle(notify(critical), t0 + MS(20));
        assert_eq!(opened(&effects).len(), 1);
        // At its time, on the clock it is given, it ends: bubbles again.
        assert!(s.tick(t0 + Duration::from_secs(3599)).is_empty());
        let effects = s.tick(t0 + Duration::from_secs(3600));
        assert!(!s.dnd.is_on());
        assert!(effects.contains(&Effect::Wake(FOOTER_NAMESPACE.to_owned())));
        let effects = s.handle(notify(note(3)), t0 + Duration::from_secs(3601));
        assert_eq!(opened(&effects).len(), 1);
    }

    #[test]
    fn nothing_is_written_until_what_was_kept_is_read_back() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut low = note(1);
        low.urgency = Urgency::Low;
        let effects = s.handle(notify(low), t0);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Save(_))));
        let mut old = note(9);
        old.at = 500;
        let effects = s.handle(
            Input::Restored {
                notes: vec![(old, Sight::Unseen)],
                dnd: Dnd::Off,
            },
            t0,
        );
        let saved = effects.iter().find_map(|e| match e {
            Effect::Save(records) => Some(records.len()),
            _ => None,
        });
        assert_eq!(saved, Some(2));
        assert_eq!(s.badge(), 2);
        assert_eq!(s.history.items()[1].note.at, 500);
    }
}
