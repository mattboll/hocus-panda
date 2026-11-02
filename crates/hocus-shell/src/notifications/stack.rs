//! The bubbles' stack (5.3, variant A): which notifications show, where,
//! how long, and what each event does. Pure: it takes what happened and
//! the time, and says what to carry out ([`Effect`]).
//!
//! - At most [`MOST`] bubbles, top first, 8 px apart, right of the screen
//!   against the bar (8 px from it, or from the screen's edge without
//!   one), from its top; on the screen of the focused window, taken as the
//!   first bubble shows. The others wait their turn, a "+N" chip under the
//!   stack saying how many.
//! - A normal one counts down its time (6 s, or the application's); a
//!   critical one stays until closed or acted on; a low one makes no
//!   bubble: it goes to the history, and the dot on the bar counts it. So
//!   does nothing for an agents' tool saying only that its agent waits or
//!   finished: to the history, no bubble, no count.
//! - Hovering any bubble pauses every countdown.
//! - A notification replacing one shown updates its bubble in place, its
//!   countdown starting again.
//! - A bubble goes over [`BUBBLE_OUT`] (the compositor fades it out),
//!   then its surface; the ones under it move up, and the next waiting
//!   one shows.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    time::{Duration, Instant},
};

use hocus_link::{
    BUBBLE_OUT, BUBBLES_WAITING_NAMESPACE, EDGE, Form, UNREAD_NAMESPACE, bubble_namespace,
};
use iced::{Rectangle, window};

use super::{
    draw::{self, Laid},
    note::{Note, Timeout, Urgency},
    server::Signal,
};

/// The most bubbles shown at once.
pub const MOST: usize = 3;
/// Between two bubbles, and between the last and the chip.
const GAP: f32 = 8.0;
/// The history keeps this many, the oldest going first.
const HISTORY: usize = 200;

/// What happened.
#[derive(Debug, Clone)]
pub enum Input {
    /// A notification, new or replacing one (same id).
    Notify(Note),
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
}

/// What to carry out.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A layer surface: a bubble's, the chip's or a dot's, on `screen`,
    /// anchored to the top right (a dot: the bottom right), `size`, its
    /// margins (top, right, bottom, left); it takes the pointer in
    /// `region` only (none: nowhere).
    Open {
        id: window::Id,
        screen: String,
        namespace: String,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
        bottom: bool,
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

/// A surface of the stack's other than a bubble: the chip, a dot.
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
    /// What went straight to the history (low, an agent's), newest last;
    /// for the history (5.4).
    pub history: VecDeque<Note>,
    /// The ids the bar's dot counts.
    unread: BTreeSet<u32>,
    hovered: BTreeSet<u64>,
    /// The focused window's screen, as last told.
    focus: Option<String>,
    /// The bars' screens and forms.
    bars: Vec<(String, Form)>,
    /// The shell draws on the CPU: its surfaces are their cards alone.
    card: bool,
    next: u64,
    chip: Option<(Extra, usize)>,
    dots: HashMap<String, Extra>,
}

impl Stack {
    pub fn new(card: bool) -> Self {
        Self {
            bubbles: Vec::new(),
            queue: VecDeque::new(),
            history: VecDeque::new(),
            unread: BTreeSet::new(),
            hovered: BTreeSet::new(),
            focus: None,
            bars: Vec::new(),
            card,
            next: 1,
            chip: None,
            dots: HashMap::new(),
        }
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

    /// The count, and the bar's form, if a dot's surface is `id`.
    pub fn dot(&self, id: window::Id) -> Option<(usize, String)> {
        self.dots
            .iter()
            .find(|(_, d)| d.id == id)
            .map(|(bar, _)| (self.unread.len(), bar.clone()))
    }

    /// How many the dot counts.
    #[cfg(test)]
    pub fn unread(&self) -> usize {
        self.unread.len()
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
            .min()
    }

    /// What `input` at `now` does.
    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            Input::Notify(note) => self.notify(note, now, &mut effects),
            Input::Close(id) => self.close(id, now, &mut effects),
            Input::Focus(screen) => self.focus = Some(screen),
            Input::Bars(bars) => {
                self.bars = bars;
                self.dots(&mut effects);
                self.relayout(&mut effects);
            }
            Input::Hover { serial, on } => {
                let before = self.paused();
                if on
                    && self
                        .bubbles
                        .iter()
                        .any(|b| b.serial == serial && !b.leaving())
                {
                    self.hovered.insert(serial);
                } else {
                    self.hovered.remove(&serial);
                }
                if before != self.paused() {
                    self.set_paused(self.paused(), now, &mut effects);
                }
            }
            Input::Card(serial) => {
                let Some(bubble) = self.showing(serial) else {
                    return effects;
                };
                let note = bubble.note.clone();
                tracing::debug!(id = note.id, "a bubble's card clicked");
                if note.default {
                    effects.push(Effect::Signal(Signal::Action {
                        id: note.id,
                        key: "default".to_owned(),
                    }));
                }
                effects.push(Effect::Raise {
                    pid: note.pid,
                    app: note.entry.clone().unwrap_or_else(|| note.app.clone()),
                });
                if !note.resident {
                    self.leave(serial, 2, now, &mut effects);
                }
            }
            Input::Dismiss(serial) => {
                tracing::debug!(serial, "a bubble dismissed");
                self.leave(serial, 2, now, &mut effects);
            }
            Input::Action { serial, key } => {
                let Some(bubble) = self.showing(serial) else {
                    return effects;
                };
                let (id, resident) = (bubble.note.id, bubble.note.resident);
                tracing::debug!(id, key, "a bubble's action");
                effects.push(Effect::Signal(Signal::Action { id, key }));
                if !resident {
                    self.leave(serial, 2, now, &mut effects);
                }
            }
        }
        effects
    }

    /// What is due at `now`: countdowns ended, bubbles gone.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let expired: Vec<u64> = self
            .bubbles
            .iter()
            .filter(|b| matches!(b.phase, Phase::Shown(Countdown::Running { until, .. }) if until <= now))
            .map(|b| b.serial)
            .collect();
        for serial in expired {
            tracing::debug!(serial, "a bubble's time is up");
            self.leave(serial, 1, now, &mut effects);
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
        effects
    }

    fn showing(&self, serial: u64) -> Option<&Bubble> {
        self.bubbles
            .iter()
            .find(|b| b.serial == serial && !b.leaving())
    }

    fn notify(&mut self, note: Note, now: Instant, effects: &mut Vec<Effect>) {
        let id = note.id;
        // A replacement: in place, wherever it is.
        if let Some(bubble) = self
            .bubbles
            .iter_mut()
            .find(|b| b.note.id == id && !b.leaving())
        {
            if note.agent || note.urgency == Urgency::Low {
                let serial = bubble.serial;
                self.leave_silently(serial, now, effects);
                self.keep_in_history(note, effects);
                return;
            }
            tracing::debug!(id, "a bubble updated in place");
            let paused = !self.hovered.is_empty();
            bubble.laid = draw::lay(&note);
            bubble.phase = Phase::Shown(Countdown::of(note.timeout, paused, now));
            bubble.note = note;
            effects.push(Effect::Wake(bubble.namespace()));
            self.relayout(effects);
            return;
        }
        if let Some(waiting) = self.queue.iter_mut().find(|n| n.id == id) {
            *waiting = note;
            return;
        }
        if let Some(at) = self.history.iter().position(|n| n.id == id) {
            self.history.remove(at);
            if self.unread.remove(&id) {
                self.dots(effects);
            }
        }
        if note.agent || note.urgency == Urgency::Low {
            self.keep_in_history(note, effects);
            return;
        }
        self.queue.push_back(note);
        self.fill(now);
        self.relayout(effects);
    }

    /// Straight to the history: a low one counted by the dot, an agent's
    /// not.
    fn keep_in_history(&mut self, note: Note, effects: &mut Vec<Effect>) {
        if !note.agent {
            self.unread.insert(note.id);
        }
        tracing::debug!(
            id = note.id,
            agent = note.agent,
            unread = self.unread.len(),
            "a notification to the history"
        );
        self.history.push_back(note);
        while self.history.len() > HISTORY {
            if let Some(old) = self.history.pop_front() {
                self.unread.remove(&old.id);
            }
        }
        self.dots(effects);
    }

    fn close(&mut self, id: u32, now: Instant, effects: &mut Vec<Effect>) {
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
        let mut found = self.queue.len() != before;
        if let Some(at) = self.history.iter().position(|n| n.id == id) {
            self.history.remove(at);
            if self.unread.remove(&id) {
                self.dots(effects);
            }
            found = true;
        }
        if found {
            effects.push(Effect::Signal(Signal::Closed { id, reason: 3 }));
            self.relayout(effects);
        }
    }

    /// Bubble `serial` goes, closed for `reason`.
    fn leave(&mut self, serial: u64, reason: u32, now: Instant, effects: &mut Vec<Effect>) {
        let Some(id) = self.showing(serial).map(|b| b.note.id) else {
            return;
        };
        effects.push(Effect::Signal(Signal::Closed { id, reason }));
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
    /// gap, or the edge's gap without a bar.
    fn right(&self, screen: &str) -> f32 {
        let bar = self
            .bars
            .iter()
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
                    bottom: false,
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
                    bottom: false,
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

    /// A dot on each bar while the history holds unread ones, where its
    /// form has its notifications' place.
    fn dots(&mut self, effects: &mut Vec<Effect>) {
        let count = self.unread.len();
        if count == 0 {
            effects.extend(self.dots.drain().map(|(_, d)| Effect::Remove(d.id)));
            return;
        }
        let gone: Vec<String> = self
            .dots
            .keys()
            .filter(|bar| !self.bars.iter().any(|(b, _)| b == *bar))
            .cloned()
            .collect();
        for bar in gone {
            if let Some(dot) = self.dots.remove(&bar) {
                effects.push(Effect::Remove(dot.id));
            }
        }
        let mut woken = false;
        for (bar, form) in &self.bars {
            let (size, margin) = draw::dot_placed(*form, count);
            match self.dots.get_mut(bar) {
                Some(dot) => {
                    if dot.placed != (size, margin) {
                        dot.placed = (size, margin);
                        effects.push(Effect::Place {
                            id: dot.id,
                            size,
                            margin,
                            region: None,
                        });
                    }
                    woken = true;
                }
                None => {
                    let id = window::Id::unique();
                    effects.push(Effect::Open {
                        id,
                        screen: bar.clone(),
                        namespace: UNREAD_NAMESPACE.to_owned(),
                        size,
                        margin,
                        bottom: true,
                        region: None,
                    });
                    self.dots.insert(
                        bar.clone(),
                        Extra {
                            id,
                            screen: bar.clone(),
                            placed: (size, margin),
                        },
                    );
                }
            }
        }
        if woken {
            effects.push(Effect::Wake(UNREAD_NAMESPACE.to_owned()));
        }
    }
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
        }
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
        let effects = s.handle(Input::Notify(note(1)), t0);
        let open = opened(&effects);
        assert_eq!(open.len(), 1);
        // Its top 8 px from the screen's, its right 8 px from the full bar.
        assert_eq!(open[0].1, (8, 8 + 196 + 8, 0, 0));
        assert_eq!(s.deadline(), Some(t0 + LIFE));
        assert!(s.tick(t0 + LIFE - MS(1)).is_empty());
        let effects = s.tick(t0 + LIFE);
        assert_eq!(signals(&effects), [Signal::Closed { id: 1, reason: 1 }]);
        assert!(effects.contains(&Effect::Closing(bubble_namespace(1))));
        // Its surface goes once it faded out.
        let effects = s.tick(t0 + LIFE + BUBBLE_OUT);
        assert!(matches!(effects[..], [Effect::Remove(_)]), "{effects:?}");
        assert_eq!(s.deadline(), None);
    }

    #[test]
    fn hovering_pauses_every_countdown() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(Input::Notify(note(1)), t0);
        s.handle(Input::Notify(note(2)), t0 + MS(1000));
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
        s.handle(Input::Notify(critical), t0);
        assert_eq!(s.deadline(), None);
        let effects = s.handle(Input::Dismiss(1), t0 + MS(90_000));
        assert_eq!(signals(&effects), [Signal::Closed { id: 1, reason: 2 }]);
    }

    #[test]
    fn a_low_one_goes_to_the_history_and_the_dot_counts_it() {
        let mut s = stack();
        let mut low = note(1);
        low.urgency = Urgency::Low;
        let effects = s.handle(Input::Notify(low), Instant::now());
        let open = opened(&effects);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].0, UNREAD_NAMESPACE);
        assert_eq!((s.unread(), s.history.len()), (1, 1));
        // An agent's: to the history, no bubble, no count.
        let mut agent = note(2);
        agent.agent = true;
        let effects = s.handle(Input::Notify(agent), Instant::now());
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert_eq!((s.unread(), s.history.len()), (1, 2));
    }

    #[test]
    fn a_replacement_updates_its_bubble_in_place_and_starts_its_countdown_again() {
        let mut s = stack();
        let t0 = Instant::now();
        s.handle(Input::Notify(note(1)), t0);
        let mut later = note(1);
        later.body = "Téléchargement : 38 %".to_owned();
        later.progress = Some(38);
        let effects = s.handle(Input::Notify(later), t0 + MS(3000));
        assert!(opened(&effects).is_empty(), "{effects:?}");
        assert!(effects.contains(&Effect::Wake(bubble_namespace(1))));
        assert_eq!(s.deadline(), Some(t0 + MS(3000) + LIFE));
    }

    #[test]
    fn more_than_three_wait_their_turn_under_a_chip() {
        let mut s = stack();
        let t0 = Instant::now();
        for id in 1..=5 {
            s.handle(Input::Notify(note(id)), t0);
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
    fn a_click_invokes_the_default_and_raises_an_action_invokes_and_closes() {
        let mut s = stack();
        let t0 = Instant::now();
        let mut first = note(1);
        first.default = true;
        first.entry = Some("slack".to_owned());
        first.pid = 42;
        s.handle(Input::Notify(first), t0);
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
        s.handle(Input::Notify(second), t0);
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
}
