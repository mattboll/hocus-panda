//! Coding agents (#41): herdr's agents, followed on worker threads
//! (hocus-platform), shown in the column with a panda each; one that waits
//! for an answer asks in a toast, and its answers go back to herdr. A rest
//! shorter than two seconds never shows, so an agent flickering between
//! work and rest does not make the column flicker.
//!
//! The configuration may name herdr's socket; otherwise only the session
//! looks for herdr's default one: nested or headless, the developer's own
//! herdr is never reached.

use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};

use hocus_core::config::Agents as AgentsConfig;
use hocus_platform::herdr::{self, Herdr, News, Status};
use hocus_shell::agents::{AgentAsk, AgentState, AgentsView, Board, Change};
use smithay::{
    reexports::{
        calloop::{
            LoopHandle, channel,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::Resource,
    },
    utils::SERIAL_COUNTER,
    wayland::seat::WaylandFocus,
};

use crate::{App, State};

/// What herdr's keys are for an answer: Enter takes what the agent
/// proposes, Escape turns it down.
const YES: &[&str] = &["enter"];
const NO: &[&str] = &["esc"];

#[derive(Debug, Default)]
pub struct Agents {
    board: Board,
    herdr: Option<Herdr>,
    /// Bumped with each client: news from an older one is stale.
    generation: u64,
    news: Option<channel::Sender<(u64, News)>>,
    /// herdr's default socket, for the session only.
    default_socket: Option<PathBuf>,
    /// The configuration the client follows, and the socket it gives.
    following: Option<(AgentsConfig, Option<PathBuf>)>,
    /// Toasts of waiting agents: toast id → agent.
    toasts: HashMap<u32, String>,
    /// When the loop is woken for a rest to show.
    waking: Option<Duration>,
    /// The socket the client was started for.
    tried: Option<PathBuf>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (news, received) = channel::channel::<(u64, News)>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg((generation, news)) = event {
            app.timed("agents", |app| app.state.agents_news(generation, news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no agents in the column");
        return;
    }
    state.agents.news = Some(news);
    if state.is_session {
        let var = |name| std::env::var(name).ok();
        state.agents.default_socket =
            herdr::default_socket(var("XDG_CONFIG_HOME").as_deref(), var("HOME").as_deref());
    }
}

/// How the column shows herdr's status.
fn state_of(status: Status) -> AgentState {
    match status {
        Status::Working => AgentState::Work,
        Status::Blocked => AgentState::Wait,
        Status::Done => AgentState::Done,
        // Unknown: an agent is there, doing what herdr cannot tell.
        Status::Idle | Status::Unknown => AgentState::Rest,
    }
}

/// `~` at the start is the home directory.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

impl State {
    /// The socket the configuration asks for, worked out when it changes.
    fn herdr_socket(&mut self) -> Option<PathBuf> {
        let config = &self.config.agents;
        if let Some((followed, socket)) = &self.agents.following
            && followed == config
        {
            return socket.clone();
        }
        let socket = if !config.enabled {
            None
        } else if config.herdr_socket.is_empty() {
            self.agents.default_socket.clone()
        } else {
            Some(expand(&config.herdr_socket))
        };
        self.agents.following = Some((config.clone(), socket.clone()));
        socket
    }

    /// After each dispatch: follows the configuration's socket, and shows
    /// rests that lasted.
    pub fn refresh_agents(&mut self) {
        // Without the shell, nothing shows them.
        if self.agents.news.is_none() {
            return;
        }
        let wanted = self.herdr_socket();
        // Tried once per socket: a client that cannot start is not retried
        // at every dispatch.
        if self.agents.tried != wanted {
            self.agents.tried.clone_from(&wanted);
            self.agents.herdr = None;
            self.agents.generation += 1;
            let changes = self.agents.board.clear();
            self.agent_changes(changes);
            self.needs_redraw = true;
            if let (Some(socket), Some(news)) = (wanted, self.agents.news.clone()) {
                let generation = self.agents.generation;
                let news = Mutex::new(news);
                let sink = move |n| {
                    if let Ok(news) = news.lock() {
                        let _ = news.send((generation, n));
                    }
                };
                match Herdr::start(socket.clone(), sink) {
                    Ok(client) => {
                        tracing::info!(socket = %socket.display(), "following herdr's agents");
                        self.agents.herdr = Some(client);
                    }
                    Err(err) => tracing::warn!(%err, "herdr's agents will not be shown"),
                }
            }
        }
        // The column draws what changed after this.
        let now = self.clock.now();
        let changes = self.agents.board.settle(now);
        self.agent_changes(changes);
        // A rest waits to show: the loop wakes when it is due.
        let due = self.agents.board.deadline();
        if let (Some(at), Some(handle)) = (due, &self.loop_handle)
            && due != self.agents.waking
        {
            let timer = Timer::from_duration(at.saturating_sub(now));
            if let Err(err) = handle.insert_source(timer, |_, _, _| TimeoutAction::Drop) {
                tracing::warn!(%err.error, "a resting agent shows only when something happens");
            }
        }
        self.agents.waking = due;
    }

    fn agents_news(&mut self, generation: u64, news: News) {
        if generation != self.agents.generation {
            return;
        }
        let now = self.clock.now();
        let board = &mut self.agents.board;
        let changes = match news {
            News::Agents(list) => board.replace(
                list.into_iter()
                    .map(|a| (a.pane, a.name, state_of(a.status), a.label))
                    .collect(),
                now,
            ),
            News::Status {
                pane,
                status,
                label,
            } => board.set(&pane, state_of(status), label, now),
            News::Gone => board.clear(),
            News::Terminal(processes) => {
                self.raise_terminal(&processes);
                Vec::new()
            }
        };
        self.agent_changes(changes);
    }

    /// Toasts come for agents that wait, and go once they no longer do.
    fn agent_changes(&mut self, changes: Vec<Change>) {
        for change in changes {
            match change {
                Change::Waits {
                    key,
                    name,
                    question,
                } => {
                    let lang = self.shell_lang();
                    let summary = question.unwrap_or_else(|| lang.agent_waits(&name));
                    let actions = [
                        ("yes", lang.agent_yes()),
                        ("no", lang.agent_no()),
                        ("show", lang.agent_open()),
                    ]
                    .map(|(k, l)| (k.to_owned(), l.to_owned()))
                    .to_vec();
                    if let Some(id) =
                        self.shell_toast_waiting(&format!("herdr · {name}"), &summary, actions)
                    {
                        self.agents.toasts.insert(id, key);
                    }
                }
                Change::Answered(key) => self.close_agent_toasts(&key),
            }
        }
    }

    fn close_agent_toasts(&mut self, key: &str) {
        let ids: Vec<u32> = self
            .agents
            .toasts
            .iter()
            .filter(|(_, k)| *k == key)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.agents.toasts.remove(&id);
            self.close_shell_toast(id);
        }
    }

    /// The column's agents, when they are on.
    pub fn agents_view(&self) -> Option<AgentsView> {
        if !self.config.agents.enabled {
            return None;
        }
        self.agents
            .board
            .view(self.clock.now(), self.pandas_on(), self.power_saver())
    }

    /// A waiting agent's toast was answered: whether it was one.
    pub fn agent_toast_action(&mut self, id: u32, key: &str) -> bool {
        let Some(agent) = self.agents.toasts.remove(&id) else {
            return false;
        };
        let ask = match key {
            "yes" => AgentAsk::Yes,
            "no" => AgentAsk::No,
            "show" | "default" => AgentAsk::Show,
            _ => return true,
        };
        self.agent_ask(&agent, ask);
        true
    }

    /// From the column or a toast. An answer goes only to an agent still
    /// waiting, so that a late click never presses Enter in its work.
    pub fn agent_ask(&mut self, key: &str, ask: AgentAsk) {
        let Some(herdr) = &self.agents.herdr else {
            return;
        };
        match ask {
            AgentAsk::Show => herdr.focus(key),
            AgentAsk::Yes | AgentAsk::No if self.agents.board.waits(key) => {
                herdr.keys(key, if ask == AgentAsk::Yes { YES } else { NO });
                self.close_agent_toasts(key);
            }
            AgentAsk::Yes | AgentAsk::No => {}
        }
    }

    /// Brings forward the window of the nearest of these processes: the
    /// terminal herdr shows in.
    fn raise_terminal(&mut self, processes: &[u32]) {
        let pid_of = |m: &crate::desktops::Managed| {
            m.window
                .wl_surface()
                .and_then(|s| self.display_handle.get_client(s.id()).ok())
                .and_then(|c| c.get_credentials(&self.display_handle).ok())
                .and_then(|c| u32::try_from(c.pid).ok())
        };
        let found = processes.iter().find_map(|pid| {
            self.windows
                .iter()
                .find(|(_, m)| pid_of(m) == Some(*pid))
                .map(|(id, _)| *id)
        });
        if let Some(id) = found {
            self.restore(id, SERIAL_COUNTER.next_serial());
        }
    }
}
