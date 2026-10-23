//! The agents' journal and the key that stops them all (#176). Each action
//! an MCP agent asks for is kept, carried out or refused, with the window
//! as it was then and the time as the user reads it; what an agent types
//! only when `[agents] journal_text` says so, since it may be a secret.
//! The control API, `hocus msg agents-journal` and the MCP `journal` tool
//! read it (an agent only its own entries); the column shows the last few
//! once an agent connected. It lives in memory alone: nothing per entry
//! goes to a file or to the log above debug.
//!
//! `stop-agents` (and the column's button, and the settings' through
//! `set_agents_stopped`) refuses every request of every
//! agent connection, reading included, and new agents naming themselves,
//! until allowed again; their events are held too. It lasts the session.

use std::collections::VecDeque;

use hocus_api::{
    ActTarget, Action, Agents, AgentsJournal, ApiError, Button, ConnectionId, ErrorCode,
    JournalAction, JournalEntry, JournalTarget, Request,
};
use hocus_core::WindowId;
use hocus_shell::journal::{Did, JournalLine, JournalView, Pressed};

use crate::{State, surface::title_and_app_id};

/// Entries kept: past these, the oldest go.
pub const KEPT: usize = 500;
/// Entries the column's popover shows.
const SHOWN: usize = 12;

/// What MCP agents did this session.
#[derive(Debug, Default)]
pub struct Journal {
    entries: VecDeque<Kept>,
    /// Whether an agent connected this session: the column shows the
    /// journal from then on.
    met: Met,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Met {
    #[default]
    NoAgent,
    Agent,
}

#[derive(Debug)]
struct Kept {
    /// The time of day the column shows.
    hour: u8,
    minute: u8,
    entry: JournalEntry,
}

/// What agents are told while stopped.
pub(crate) fn stopped() -> ApiError {
    ApiError::new(
        ErrorCode::PermissionDenied,
        "the user stopped every agent: requests are refused until they allow agents again",
    )
}

impl State {
    /// Whether every agent is stopped.
    pub fn agents_stopped(&self) -> bool {
        self.api.agents() == Agents::Stopped
    }

    /// The stop key: stops every agent, or allows them again.
    pub fn toggle_agents(&mut self) {
        self.set_agents_stopped(!self.agents_stopped());
    }

    /// Stops every agent at once, or allows them again; said in a toast,
    /// since the column may be hidden behind a fullscreen window.
    pub fn set_agents_stopped(&mut self, stop: bool) {
        if stop == self.agents_stopped() {
            return;
        }
        self.api.set_agents(if stop {
            Agents::Stopped
        } else {
            Agents::Allowed
        });
        // What agents wait on is refused too.
        if stop {
            self.refuse_pending(&stopped());
        }
        tracing::info!(stopped = stop, "MCP agents stopped or allowed again");
        let (summary, body) = self.shell_lang().agents_toast(stop);
        self.shell_toast(summary, body);
        self.emit(|_| hocus_api::Event::AgentsStopped { stopped: stop });
    }

    /// The refusal of a request while agents are stopped: every request of
    /// an agent's connection, and naming oneself an agent. An action or a
    /// connection refused so is kept in the journal.
    pub(crate) fn agents_gate(
        &mut self,
        connection: ConnectionId,
        request: &Request,
    ) -> Option<ApiError> {
        if !self.agents_stopped() {
            return None;
        }
        let agent = self.api.agent(connection).map(str::to_owned);
        match (agent, request) {
            (Some(agent), Request::Act { target, action, .. }) => {
                let entry = self.journal_entry(&agent, target, action);
                self.journal_keep(entry, Some(&stopped()));
                Some(stopped())
            }
            (None, Request::Agent { name }) => {
                let civil = self.wall_civil();
                let entry = JournalEntry {
                    at: wall(&civil),
                    agent: name.clone(),
                    action: JournalAction::Connect,
                    target: None,
                    refused: None,
                    confirmed: false,
                };
                self.journal_keep(
                    (civil.hour() as u8, civil.minute() as u8, entry),
                    Some(&stopped()),
                );
                Some(stopped())
            }
            (Some(_), _) => Some(stopped()),
            (None, _) => None,
        }
    }

    /// An agent connected: the column shows the journal from now on.
    pub(crate) fn journal_met(&mut self) {
        self.journal.met = Met::Agent;
    }

    /// The user handed `agent` (a coding agent, by its name) a question
    /// and an AI's answer (#197): kept as its action, the text only when
    /// `[agents] journal_text` says so.
    pub(crate) fn journal_prompt(&mut self, agent: &str, text: &str) {
        let civil = self.wall_civil();
        let entry = JournalEntry {
            at: wall(&civil),
            agent: agent.to_owned(),
            action: JournalAction::Prompt {
                characters: text.chars().count(),
                text: self.config.agents.journal_text.then(|| text.to_owned()),
            },
            target: None,
            refused: None,
            confirmed: false,
        };
        self.journal_met();
        self.journal_keep((civil.hour() as u8, civil.minute() as u8, entry), None);
    }

    /// What `agent` asks for, as the journal keeps it: the window as it
    /// is now, before the action changes it. Text and keys are not kept
    /// while the session is locked or a password is asked for.
    pub(crate) fn journal_entry(
        &self,
        agent: &str,
        target: &ActTarget,
        action: &Action,
    ) -> (u8, u8, JournalEntry) {
        let secret = self.locked() || self.auth_open();
        let civil = self.wall_civil();
        let action = match action {
            Action::Click { button, count } => JournalAction::Click {
                button: *button,
                count: *count,
            },
            Action::Scroll { dy } => JournalAction::Scroll { dy: *dy },
            Action::Type { text } => JournalAction::Type {
                characters: text.chars().count(),
                text: (self.config.agents.journal_text && !secret).then(|| text.clone()),
            },
            Action::Keys { keys } => JournalAction::Keys {
                keys: (!secret).then(|| keys.clone()),
            },
        };
        let target = match target {
            ActTarget::Window { window, .. } | ActTarget::AppElement { window, .. } => {
                let (title, app_id) = self
                    .windows
                    .get(&WindowId(*window))
                    .map(|m| title_and_app_id(&m.window))
                    .unwrap_or_default();
                JournalTarget::Window {
                    id: *window,
                    app_id,
                    title: title.filter(|t| !t.is_empty()),
                    element: match target {
                        ActTarget::AppElement { label, .. } => Some(label.clone()),
                        _ => None,
                    },
                }
            }
            ActTarget::Element { label } => JournalTarget::Element {
                label: label.clone(),
            },
        };
        let entry = JournalEntry {
            at: wall(&civil),
            agent: agent.to_owned(),
            action,
            target: Some(target),
            refused: None,
            confirmed: false,
        };
        (civil.hour() as u8, civil.minute() as u8, entry)
    }

    /// Keeps an entry, refused with `refusal` or carried out.
    pub(crate) fn journal_keep(
        &mut self,
        (hour, minute, mut entry): (u8, u8, JournalEntry),
        refusal: Option<&ApiError>,
    ) {
        entry.refused = refusal.map(|e| e.message.clone());
        let entries = &mut self.journal.entries;
        if entries.len() == KEPT {
            entries.pop_front();
        }
        entries.push_back(Kept {
            hour,
            minute,
            entry,
        });
    }

    /// The journal, for the control API: an agent's connection reads its
    /// own entries alone, any other all of them.
    pub fn agents_journal(&self, connection: ConnectionId) -> AgentsJournal {
        let agent = self.api.agent(connection);
        AgentsJournal {
            stopped: self.agents_stopped(),
            entries: self
                .journal
                .entries
                .iter()
                .filter(|k| agent.is_none_or(|a| k.entry.agent == a))
                .map(|k| k.entry.clone())
                .collect(),
        }
    }

    /// What the column shows: once an agent connected, or while stopped.
    pub(crate) fn journal_view(&self) -> Option<JournalView> {
        let stopped = self.agents_stopped();
        if self.journal.met == Met::NoAgent && !stopped {
            return None;
        }
        let format = self.config.clock.format;
        let lines = self
            .journal
            .entries
            .iter()
            .rev()
            .take(SHOWN)
            .map(|k| JournalLine {
                agent: k.entry.agent.clone(),
                did: did(&k.entry.action),
                target: k
                    .entry
                    .target
                    .as_ref()
                    .map(target_words)
                    .unwrap_or_default(),
                time: format.time(k.hour, k.minute),
                refused: k.entry.refused.clone(),
            })
            .collect();
        Some(JournalView {
            stopped,
            seats: self.seat_lines(),
            lines,
            total: self.journal.entries.len(),
        })
    }
}

fn did(action: &JournalAction) -> Did {
    match action {
        JournalAction::Click { button, count } => Did::Click {
            button: match button {
                Button::Left => Pressed::Left,
                Button::Right => Pressed::Right,
                Button::Middle => Pressed::Middle,
            },
            count: *count,
        },
        JournalAction::Scroll { dy } => Did::Scroll { down: *dy >= 0.0 },
        JournalAction::Type { characters, text } => Did::Type {
            characters: *characters,
            text: text.clone(),
        },
        JournalAction::Keys { keys } => Did::Keys(keys.clone()),
        JournalAction::Connect => Did::Connect,
        JournalAction::Prompt { characters, text } => Did::Prompt {
            characters: *characters,
            text: text.clone(),
        },
    }
}

/// The local date and time, as the journal keeps it.
fn wall(civil: &jiff::civil::DateTime) -> String {
    civil.strftime("%Y-%m-%dT%H:%M:%S").to_string()
}

/// A window by its title, else its app id; an element by its label.
fn target_words(target: &JournalTarget) -> String {
    match target {
        JournalTarget::Window {
            id,
            app_id,
            title,
            element,
        } => {
            let window = title
                .clone()
                .or_else(|| app_id.clone())
                .unwrap_or_else(|| format!("#{id}"));
            match element {
                Some(label) => format!("“{label}” · {window}"),
                None => window,
            }
        }
        JournalTarget::Element { label } => label.clone(),
    }
}
