//! The agents' journal and the key that stops them all (#176). Each action
//! an MCP agent asks for is kept, carried out or refused, with the window
//! as it was then and the time as the user reads it; what an agent types
//! only when `[agents] journal_text` says so, since it may be a secret.
//! The control API, `hocus msg agents-journal` and the MCP `journal` tool
//! read it; the column shows the last few once an agent connected.
//!
//! `stop-agents` (and the column's button) refuses every request of every
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
fn stopped() -> ApiError {
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
        tracing::info!(stopped = stop, "MCP agents stopped or allowed again");
        let (summary, body) = self.shell_lang().agents_toast(stop);
        self.shell_toast(summary, body);
    }

    /// The refusal of a request while agents are stopped: every request of
    /// an agent's connection, and naming oneself an agent. An action
    /// refused so is kept in the journal.
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
            (Some(agent), Request::Act { target, action }) => {
                let entry = self.journal_entry(&agent, target, action);
                self.journal_keep(entry, Some(&stopped()));
                Some(stopped())
            }
            (Some(_), _) | (None, Request::Agent { .. }) => Some(stopped()),
            (None, _) => None,
        }
    }

    /// An agent connected: the column shows the journal from now on.
    pub(crate) fn journal_met(&mut self) {
        self.journal.met = Met::Agent;
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
            ActTarget::Window { window, .. } => {
                let (title, app_id) = self
                    .windows
                    .get(&WindowId(*window))
                    .map(|m| title_and_app_id(&m.window))
                    .unwrap_or_default();
                JournalTarget::Window {
                    id: *window,
                    app_id,
                    title: title.filter(|t| !t.is_empty()),
                }
            }
            ActTarget::Element { label } => JournalTarget::Element {
                label: label.clone(),
            },
        };
        let entry = JournalEntry {
            at: civil.strftime("%Y-%m-%dT%H:%M:%S").to_string(),
            agent: agent.to_owned(),
            action,
            target,
            refused: None,
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

    /// The journal, for the control API.
    pub fn agents_journal(&self) -> AgentsJournal {
        AgentsJournal {
            stopped: self.agents_stopped(),
            entries: self
                .journal
                .entries
                .iter()
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
                target: target_words(&k.entry.target),
                time: format.time(k.hour, k.minute),
                refused: k.entry.refused.clone(),
            })
            .collect();
        Some(JournalView {
            stopped,
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
    }
}

/// A window by its title, else its app id; an element by its label.
fn target_words(target: &JournalTarget) -> String {
    match target {
        JournalTarget::Window { id, app_id, title } => title
            .clone()
            .or_else(|| app_id.clone())
            .unwrap_or_else(|| format!("#{id}")),
        JournalTarget::Element { label } => label.clone(),
    }
}
