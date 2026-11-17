//! The desktop's events, told to the control socket's subscribers
//! (`hocus msg subscribe`, docs/api.md "Events"): a window opened, closed
//! or given the keyboard, a screen showing another desktop, a screen
//! plugged or unplugged, an agent's state. One JSON object a line.
//!
//! Nothing here runs without a subscriber. With one, what they are told
//! is the difference between what was last seen and what is, after each
//! turn of the loop ([`State::flush_events`]): a few comparisons a turn,
//! and the events made only when something they cover changed. The
//! agents' come from the agents' own flush, which already lists them
//! when they change ([`State::events_agents`]).
//!
//! The loop never waits for a subscriber. Each has a queue of at most
//! [`MOST`] events, written as its socket takes them (a calloop source
//! while some are left); the socket itself is given a small buffer, so
//! that the queue is the bound. A subscriber that does not read loses
//! the events past it, and is told how many with one line
//! (`{"event":"lost","count":N}`) in their place, once it has room again.
//! One whose connection ends is dropped as the loop hears it.

use std::{
    collections::{BTreeMap, VecDeque},
    io::{ErrorKind, Read as _, Write as _},
    os::unix::net::UnixStream,
};

use hocus_agents::events::{Kind, Subscription};
use hocus_link::AgentShown;
use serde_json::{Value, json};
use smithay::reexports::calloop::{
    Interest, Mode, PostAction, RegistrationToken, generic::Generic,
};

use crate::State;

/// The events kept for a subscriber that does not read, at most.
pub const MOST: usize = 256;
/// The socket's own buffer asked for a subscriber's connection: small,
/// so that what it holds beside the queue is a few events.
const SOCKET_BUFFER: usize = 4096;
/// What one write takes from the queue at most.
const WRITE_MOST: usize = 4096;

/// The subscribers, and what they were last told of.
#[derive(Debug, Default)]
pub struct Events {
    subscribers: Vec<Subscriber>,
    next: u64,
    /// What was last seen; none without subscribers.
    seen: Option<Seen>,
}

impl Events {
    /// How many subscribers there are.
    pub fn subscribers(&self) -> usize {
        self.subscribers.len()
    }

    fn get(&mut self, id: u64) -> Option<&mut Subscriber> {
        self.subscribers.iter_mut().find(|s| s.id == id)
    }
}

#[derive(Debug)]
struct Subscriber {
    id: u64,
    stream: UnixStream,
    wants: Subscription,
    queue: VecDeque<String>,
    /// What is being written: whole lines taken from the queue, less
    /// what the socket took.
    unsent: Vec<u8>,
    /// Events not kept since it was last told of a loss.
    lost: u64,
    /// Its connection's end, heard.
    hears: Option<RegistrationToken>,
    /// Its socket's room, waited for while something is left to write.
    writing: Option<RegistrationToken>,
}

impl Subscriber {
    /// `line` kept for it, if there is room; a loss told first, once
    /// there is room again.
    fn push(&mut self, line: String) {
        if self.lost > 0 && self.queue.len() < MOST {
            self.queue.push_back(lost_line(self.lost));
            self.lost = 0;
        }
        if self.queue.len() < MOST {
            self.queue.push_back(line);
        } else {
            self.lost += 1;
        }
    }

    /// Writes what its socket takes now: whether everything was.
    fn write(&mut self) -> std::io::Result<bool> {
        loop {
            if self.unsent.is_empty() {
                if self.queue.is_empty() && self.lost > 0 {
                    self.queue.push_back(lost_line(self.lost));
                    self.lost = 0;
                }
                while self.unsent.len() < WRITE_MOST
                    && let Some(line) = self.queue.pop_front()
                {
                    self.unsent.extend_from_slice(line.as_bytes());
                }
                if self.unsent.is_empty() {
                    return Ok(true);
                }
            }
            match (&self.stream).write(&self.unsent) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.unsent.drain(..n.min(self.unsent.len()));
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
    }
}

/// The line telling a subscriber how many events it lost.
fn lost_line(count: u64) -> String {
    line(&json!({ "event": "lost", "count": count }))
}

fn line(value: &Value) -> String {
    format!("{value}\n")
}

/// What the events are the difference from.
#[derive(Debug, Default, Clone, PartialEq)]
struct Seen {
    /// The windows by id: their application's id and their title.
    windows: BTreeMap<u64, (String, String)>,
    focused: Option<u64>,
    /// The screens by name: the desktop each shows.
    screens: BTreeMap<String, usize>,
    /// The agents by id: their name, state, source and window.
    agents: BTreeMap<u64, AgentShown>,
}

fn window_json(id: u64, (app_id, title): &(String, String)) -> Value {
    json!({ "id": id, "app_id": app_id, "title": title })
}

fn agent_json(agent: &AgentShown, state: &str) -> Value {
    json!({
        "id": agent.id,
        "name": agent.name,
        "state": state,
        "source": agent.source,
        "window": (agent.window != 0).then_some(agent.window),
    })
}

impl State {
    /// A connection subscribes: told it is, then each event it wants.
    pub(crate) fn subscribe(&mut self, stream: UnixStream, wants: Subscription) {
        if let Err(err) = stream.set_nonblocking(true) {
            tracing::warn!(%err, "a subscriber is let go");
            return;
        }
        if let Err(err) = rustix::net::sockopt::set_socket_send_buffer_size(&stream, SOCKET_BUFFER)
        {
            tracing::debug!(%err, "a subscriber's socket keeps its buffer");
        }
        let id = self.events.next;
        self.events.next += 1;
        let hears = match stream.try_clone() {
            Ok(heard) => self.loop_handle.insert_source(
                Generic::new(heard, Interest::READ, Mode::Level),
                move |_, _, app| Ok(app.state.subscriber_heard(id)),
            ),
            Err(err) => {
                tracing::warn!(%err, "a subscriber is let go");
                return;
            }
        };
        let hears = match hears {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(err = %err.error, "a subscriber is let go");
                return;
            }
        };
        if self.events.seen.is_none() {
            let mut seen = self.seen_now();
            seen.agents = self.agents_shown().into_iter().map(|a| (a.id, a)).collect();
            self.events.seen = Some(seen);
        }
        let kinds: Vec<&str> = wants.kinds().iter().map(|k| k.word()).collect();
        tracing::info!(id, kinds = kinds.join(","), "a subscriber");
        let mut subscriber = Subscriber {
            id,
            stream,
            wants,
            queue: VecDeque::new(),
            unsent: Vec::new(),
            lost: 0,
            hears: Some(hears),
            writing: None,
        };
        subscriber.push(line(&json!({
            "event": "subscribed",
            "api": hocus_agents::API,
            "kinds": kinds,
        })));
        self.events.subscribers.push(subscriber);
        self.subscriber_write(id);
    }

    /// The windows, the focus and the screens as they are.
    fn seen_now(&self) -> Seen {
        let windows = self
            .model
            .stack()
            .iter()
            .map(|id| {
                let app_id = self
                    .windows
                    .get(id)
                    .and_then(|m| crate::windows::app_id(&m.window))
                    .unwrap_or_default();
                (id.0, (app_id, crate::windows::title_of(self, *id)))
            })
            .collect();
        let screens = self
            .space
            .outputs()
            .map(|o| (o.name(), self.strip(o).current()))
            .collect();
        Seen {
            windows,
            focused: self.model.focused().map(|w| w.0),
            screens,
            agents: BTreeMap::new(),
        }
    }

    /// Whether the windows, the focus or the screens may differ from
    /// `seen`: comparisons only, nothing made.
    fn differs(&self, seen: &Seen) -> bool {
        let stack = self.model.stack();
        stack.len() != seen.windows.len()
            || stack.iter().any(|id| !seen.windows.contains_key(&id.0))
            || self.model.focused().map(|w| w.0) != seen.focused
            || self.space.outputs().count() != seen.screens.len()
            || self
                .space
                .outputs()
                .any(|o| seen.screens.get(&o.name()) != Some(&self.strip(o).current()))
    }

    /// After each turn of the loop: the subscribers told what changed of
    /// the windows, the desktops and the screens.
    pub(crate) fn flush_events(&mut self) {
        let Some(seen) = &self.events.seen else {
            return;
        };
        if !self.differs(seen) {
            return;
        }
        let mut now = self.seen_now();
        let Some(before) = self.events.seen.take() else {
            return;
        };
        now.agents = before.agents.clone();
        let mut told: Vec<(Kind, Value)> = Vec::new();
        for (name, _) in now
            .screens
            .iter()
            .filter(|(n, _)| !before.screens.contains_key(*n))
        {
            told.push((
                Kind::Screen,
                json!({ "event": "screen-plugged", "screen": { "name": name } }),
            ));
        }
        for (id, window) in now
            .windows
            .iter()
            .filter(|(id, _)| !before.windows.contains_key(id))
        {
            told.push((
                Kind::Window,
                json!({ "event": "window-opened", "window": window_json(*id, window) }),
            ));
        }
        for (id, window) in before
            .windows
            .iter()
            .filter(|(id, _)| !now.windows.contains_key(id))
        {
            told.push((
                Kind::Window,
                json!({ "event": "window-closed", "window": window_json(*id, window) }),
            ));
        }
        for (name, desktop) in &now.screens {
            if before.screens.get(name).is_some_and(|d| d != desktop) {
                let named = self
                    .space
                    .outputs()
                    .find(|o| o.name() == *name)
                    .and_then(|o| self.desktop_name(o, *desktop))
                    .unwrap_or_default();
                told.push((
                    Kind::Desktop,
                    json!({
                        "event": "desktop-switched",
                        "screen": { "name": name },
                        "desktop": { "number": desktop + 1, "name": named },
                    }),
                ));
            }
        }
        if now.focused != before.focused {
            let window = now
                .focused
                .and_then(|id| now.windows.get(&id).map(|w| window_json(id, w)));
            told.push((
                Kind::Window,
                json!({ "event": "window-focused", "window": window }),
            ));
        }
        for name in before
            .screens
            .keys()
            .filter(|n| !now.screens.contains_key(*n))
        {
            told.push((
                Kind::Screen,
                json!({ "event": "screen-unplugged", "screen": { "name": name } }),
            ));
        }
        self.events.seen = Some(now);
        self.tell_subscribers(told);
    }

    /// The agents as the agents' flush lists them, when they changed: the
    /// subscribers told each new state, and each agent forgotten.
    pub(crate) fn events_agents(&mut self, agents: &[AgentShown]) {
        let Some(seen) = &mut self.events.seen else {
            return;
        };
        let mut told: Vec<(Kind, Value)> = Vec::new();
        let now: BTreeMap<u64, AgentShown> = agents.iter().map(|a| (a.id, a.clone())).collect();
        for (id, agent) in &now {
            let before = seen.agents.get(id);
            if before.is_none_or(|b| b.state != agent.state || b.name != agent.name) {
                told.push((
                    Kind::Agent,
                    json!({ "event": "agent-state", "agent": agent_json(agent, &agent.state) }),
                ));
            }
        }
        for (_, agent) in seen.agents.iter().filter(|(id, _)| !now.contains_key(id)) {
            told.push((
                Kind::Agent,
                json!({ "event": "agent-state", "agent": agent_json(agent, "ended") }),
            ));
        }
        seen.agents = now;
        self.tell_subscribers(told);
    }

    fn tell_subscribers(&mut self, told: Vec<(Kind, Value)>) {
        if told.is_empty() {
            return;
        }
        let lines: Vec<(Kind, String)> = told.into_iter().map(|(k, v)| (k, line(&v))).collect();
        let mut ids = Vec::new();
        for subscriber in &mut self.events.subscribers {
            let before = (subscriber.queue.len(), subscriber.lost);
            for (kind, line) in &lines {
                if subscriber.wants.wants(*kind) {
                    subscriber.push(line.clone());
                }
            }
            if (subscriber.queue.len(), subscriber.lost) != before {
                ids.push(subscriber.id);
            }
        }
        for id in ids {
            self.subscriber_write(id);
        }
    }

    /// Writes what subscriber `id`'s socket takes now; waits for its room
    /// for the rest.
    fn subscriber_write(&mut self, id: u64) {
        let Some(subscriber) = self.events.get(id) else {
            return;
        };
        if subscriber.writing.is_some() {
            // Its room is waited for already.
            return;
        }
        match subscriber.write() {
            Ok(true) => {}
            Ok(false) => {
                let waiting = subscriber.stream.try_clone().map(|writer| {
                    self.loop_handle.insert_source(
                        Generic::new(writer, Interest::WRITE, Mode::Level),
                        move |_, _, app| Ok(app.state.subscriber_writable(id)),
                    )
                });
                match waiting {
                    Ok(Ok(token)) => {
                        if let Some(subscriber) = self.events.get(id) {
                            subscriber.writing = Some(token);
                        }
                    }
                    Ok(Err(err)) => {
                        tracing::warn!(err = %err.error, "a subscriber is let go");
                        self.unsubscribe(id);
                    }
                    Err(err) => {
                        tracing::warn!(%err, "a subscriber is let go");
                        self.unsubscribe(id);
                    }
                }
            }
            Err(err) => {
                tracing::debug!(id, %err, "a subscriber went");
                self.unsubscribe(id);
            }
        }
    }

    /// Subscriber `id`'s socket has room.
    fn subscriber_writable(&mut self, id: u64) -> PostAction {
        let Some(subscriber) = self.events.get(id) else {
            return PostAction::Remove;
        };
        match subscriber.write() {
            Ok(false) => PostAction::Continue,
            Ok(true) => {
                subscriber.writing = None;
                PostAction::Remove
            }
            Err(err) => {
                tracing::debug!(id, %err, "a subscriber went");
                // This source goes as it returns.
                subscriber.writing = None;
                self.unsubscribe(id);
                PostAction::Remove
            }
        }
    }

    /// Subscriber `id`'s connection said something, or ended: what it
    /// says is not read; its end drops it.
    fn subscriber_heard(&mut self, id: u64) -> PostAction {
        let Some(subscriber) = self.events.get(id) else {
            return PostAction::Remove;
        };
        let mut buffer = [0; 512];
        loop {
            match (&subscriber.stream).read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) if err.kind() == ErrorKind::WouldBlock => return PostAction::Continue,
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        tracing::debug!(id, "a subscriber went");
        // This source goes as it returns.
        subscriber.hears = None;
        self.unsubscribe(id);
        PostAction::Remove
    }

    fn unsubscribe(&mut self, id: u64) {
        let Some(at) = self.events.subscribers.iter().position(|s| s.id == id) else {
            return;
        };
        let gone = self.events.subscribers.remove(at);
        for token in [gone.hears, gone.writing].into_iter().flatten() {
            self.loop_handle.remove(token);
        }
        if self.events.subscribers.is_empty() {
            // Nothing is followed without them.
            self.events.seen = None;
        }
    }
}
