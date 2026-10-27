//! Coding agents (#41): herdr's agents, followed on worker threads
//! (hocus-platform), shown in the column with a panda each; one that waits
//! for an answer asks in a toast, and its answers go back to herdr. A rest
//! shorter than two seconds never shows, so an agent flickering between
//! work and rest does not make the column flicker.
//!
//! The configuration may name herdr's socket, or several herdr each tied
//! to a desktop (#192); otherwise only the session looks for herdr's
//! default one: nested or headless, the developer's own herdr is never
//! reached. Agents of a herdr tied to a desktop show in full on it, and in
//! a line of their own elsewhere; an agent done while its desktop is not
//! shown badges it until it is (`activity`).

use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};

use hocus_core::{
    DesktopId, OutputName,
    config::{Agents as AgentsConfig, HerdrSocket},
};
use hocus_platform::herdr::{self, Herdr, News, Status};
use hocus_shell::agents::{AgentAsk, AgentState, AgentsView, Board, Change, Elsewhere};
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

/// A herdr to follow: its socket, and the desktop its agents work for
/// (its screen's name, none for the primary one, and its number from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    socket: PathBuf,
    desktop: Option<(Option<String>, u32)>,
}

/// News of one herdr: the clients' generation, and which herdr.
type Tidings = (u64, usize, News);

#[derive(Debug, Default)]
pub struct Agents {
    board: Board,
    /// One client per source, in the configuration's order.
    herdr: Vec<Option<Herdr>>,
    /// Bumped with each set of clients: news from an older one is stale.
    generation: u64,
    news: Option<channel::Sender<Tidings>>,
    /// herdr's default socket, for the session only.
    default_socket: Option<PathBuf>,
    /// The configuration the clients follow, and the sources it gives.
    following: Option<(AgentsConfig, Vec<Source>)>,
    /// Toasts of waiting agents: toast id → agent.
    toasts: HashMap<u32, String>,
    /// When the loop is woken for a rest to show.
    waking: Option<Duration>,
    /// The sources the clients were started for.
    tried: Option<Vec<Source>>,
    /// Agents done while their desktop was not shown: agent → desktop.
    pub(crate) done_unseen: HashMap<String, DesktopId>,
    /// Status changes the loop took in, for the test harness: one comes
    /// off a worker thread in its own time (#235).
    pub(crate) statuses_seen: u64,
}

/// An agent's key on the board: its herdr and its pane.
fn key(source: usize, pane: &str) -> String {
    format!("{source}/{pane}")
}

/// The herdr and the pane of a key.
fn pane_of(key: &str) -> Option<(usize, &str)> {
    let (source, pane) = key.split_once('/')?;
    Some((source.parse().ok()?, pane))
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (news, received) = channel::channel::<Tidings>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg((generation, source, news)) = event {
            app.timed("agents", |app| {
                app.state.agents_news(generation, source, news)
            });
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
    /// The sockets the configuration asks for, worked out when it changes.
    fn herdr_sources(&mut self) -> Vec<Source> {
        let config = &self.config.agents;
        if let Some((followed, sources)) = &self.agents.following
            && followed == config
        {
            return sources.clone();
        }
        let named = config.herdr_sockets();
        let sources: Vec<Source> = if !config.enabled {
            Vec::new()
        } else if named.is_empty() {
            self.agents
                .default_socket
                .iter()
                .map(|socket| Source {
                    socket: socket.clone(),
                    desktop: None,
                })
                .collect()
        } else {
            named
                .iter()
                .map(
                    |HerdrSocket {
                         socket,
                         desktop,
                         output,
                     }| Source {
                        socket: expand(socket),
                        desktop: desktop.map(|d| (output.clone(), d)),
                    },
                )
                .collect()
        };
        self.agents.following = Some((config.clone(), sources.clone()));
        sources
    }

    /// After each dispatch: follows the configuration's sockets, and shows
    /// rests that lasted.
    pub fn refresh_agents(&mut self) {
        // Without the shell, nothing shows them.
        if self.agents.news.is_none() {
            return;
        }
        let wanted = self.herdr_sources();
        // Tried once per set of sockets: a client that cannot start is not
        // retried at every dispatch.
        if self.agents.tried.as_ref() != Some(&wanted) {
            self.agents.tried = Some(wanted.clone());
            self.agents.herdr.clear();
            self.agents.generation += 1;
            let changes = self.agents.board.clear_all();
            self.agent_changes(changes);
            self.agents.done_unseen.clear();
            self.redraw.all();
            for (i, source) in wanted.iter().enumerate() {
                let client = self
                    .agents
                    .news
                    .clone()
                    .and_then(|news| self.start_herdr(i, &source.socket, news));
                self.agents.herdr.push(client);
            }
        }
        // The column draws what changed after this.
        let now = self.clock.now();
        let changes = self.agents.board.settle(now);
        self.agent_changes(changes);
        // A rest waits to show: the loop wakes when it is due.
        let due = self.agents.board.deadline();
        if let (Some(at), Some(handle)) = (due, self.waker())
            && due != self.agents.waking
        {
            let timer = Timer::from_duration(at.saturating_sub(now));
            let woken = handle.insert_source(timer, |_, _, app| {
                app.timed("agents rest", |_| {});
                TimeoutAction::Drop
            });
            if let Err(err) = woken {
                tracing::warn!(%err.error, "a resting agent shows only when something happens");
            }
        }
        self.agents.waking = due;
    }

    /// A client for the herdr `i` at `socket`, telling the loop.
    fn start_herdr(
        &self,
        i: usize,
        socket: &std::path::Path,
        news: channel::Sender<Tidings>,
    ) -> Option<Herdr> {
        let generation = self.agents.generation;
        let news = Mutex::new(news);
        let sink = move |n| {
            if let Ok(news) = news.lock() {
                let _ = news.send((generation, i, n));
            }
        };
        match Herdr::start(socket.to_path_buf(), sink) {
            Ok(client) => {
                tracing::info!(socket = %socket.display(), "following herdr's agents");
                Some(client)
            }
            Err(err) => {
                tracing::warn!(socket = %socket.display(), %err, "herdr's agents will not be shown");
                None
            }
        }
    }

    fn agents_news(&mut self, generation: u64, source: usize, news: News) {
        if generation != self.agents.generation {
            return;
        }
        let now = self.clock.now();
        let changes = match news {
            News::Agents(list) => self.agents.board.replace(
                source,
                list.into_iter()
                    .map(|a| (key(source, &a.pane), a.name, state_of(a.status), a.label))
                    .collect(),
                now,
            ),
            News::Status {
                pane,
                status,
                label,
            } => {
                self.agents.statuses_seen += 1;
                self.agents
                    .board
                    .set(&key(source, &pane), state_of(status), label, now)
            }
            News::Gone => self.agents.board.clear(source),
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
                    // Its desktop, when no screen shows it (#218).
                    let title = match self.unseen_desktop_label(&key) {
                        Some(desktop) => format!("herdr · {name} · {desktop}"),
                        None => format!("herdr · {name}"),
                    };
                    if let Some(id) = self.shell_toast_waiting(&title, &summary, actions) {
                        self.agents.toasts.insert(id, key);
                    }
                }
                Change::Answered(key) => self.close_agent_toasts(&key),
                Change::Done(key) => {
                    // Done where Alex is not looking: its desktop says so.
                    if let Some(desktop) = self.agent_desktop(&key)
                        && !self.desktop_is_shown(desktop)
                    {
                        self.agents.done_unseen.insert(key, desktop);
                    }
                }
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

    /// The desktop the herdr `source` works for, when it has one and it
    /// is there.
    fn source_desktop(&self, source: usize) -> Option<DesktopId> {
        let (output, n) = self.agents.tried.as_ref()?.get(source)?.desktop.clone()?;
        let output = match output {
            Some(name) => OutputName(name),
            None => self.model.primary()?.name.clone(),
        };
        let index = (n as usize).checked_sub(1)?;
        self.model.own_desktops(&output)?.get(index).copied()
    }

    /// The desktop an agent works for, by its key.
    pub(crate) fn agent_desktop(&self, key: &str) -> Option<DesktopId> {
        self.source_desktop(pane_of(key)?.0)
    }

    /// What an agent's desktop is called, when no screen shows it: its
    /// name, else its number.
    fn unseen_desktop_label(&self, key: &str) -> Option<String> {
        let desktop = self.agent_desktop(key)?;
        if self.desktop_is_shown(desktop) {
            return None;
        }
        self.desktop_label(desktop)
    }

    /// What a desktop is called: its name, else its number on its screen.
    pub(crate) fn desktop_label(&self, desktop: DesktopId) -> Option<String> {
        self.desktop_name(desktop).or_else(|| {
            let home = &self.model.desktop(desktop)?.home;
            let n = self
                .model
                .own_desktops(home)?
                .iter()
                .position(|d| *d == desktop)?;
            Some(self.shell_lang().desktop(n + 1))
        })
    }

    /// Whether a screen shows this desktop.
    pub(crate) fn desktop_is_shown(&self, desktop: DesktopId) -> bool {
        self.model.outputs().iter().any(|o| o.shown == desktop)
    }

    /// Whether the agent of this key is done still.
    pub(crate) fn agent_still_done(&self, key: &str) -> bool {
        self.agents.board.state(key) == Some(AgentState::Done)
    }

    /// Desktops with an agent waiting for an answer.
    pub(crate) fn desktops_waiting(&self) -> Vec<DesktopId> {
        self.agents
            .board
            .states()
            .filter(|(_, s)| *s == AgentState::Wait)
            .filter_map(|(source, _)| self.source_desktop(source))
            .collect()
    }

    /// The column's agents, when they are on: those of the desktop its
    /// screen shows in full, every other desktop's in a line.
    pub fn agents_view(&self) -> Option<AgentsView> {
        if !self.config.agents.enabled {
            return None;
        }
        let here = self
            .column_screen()
            .and_then(|o| self.model.output(&OutputName(o.name())))
            .map(|o| o.shown);
        let count = self.agents.tried.as_ref().map_or(0, Vec::len);
        // Every other desktop with a herdr, in the screens' order.
        let mut elsewhere: Vec<(usize, usize, Elsewhere)> = Vec::new();
        for source in 0..count {
            let Some(desktop) = self.source_desktop(source) else {
                continue;
            };
            if Some(desktop) == here {
                continue;
            }
            if let Some((_, _, e)) = elsewhere
                .iter_mut()
                .find(|(_, _, e)| self.source_desktop(e.sources[0]) == Some(desktop))
            {
                e.sources.push(source);
                continue;
            }
            let Some(home) = self.model.desktop(desktop).map(|d| d.home.clone()) else {
                continue;
            };
            let host = self.model.host_of(desktop);
            let screen = host.map_or(usize::MAX, |h| {
                self.model
                    .outputs()
                    .iter()
                    .position(|o| o.name == h.name)
                    .unwrap_or(usize::MAX)
            });
            let index = host
                .and_then(|h| h.desktops.iter().position(|d| *d == desktop))
                .unwrap_or(usize::MAX);
            let own = self
                .model
                .own_desktops(&home)
                .and_then(|own| own.iter().position(|d| *d == desktop))
                .unwrap_or(0);
            let name = self
                .desktop_name(desktop)
                .unwrap_or_else(|| self.shell_lang().desktop(own + 1));
            elsewhere.push((
                screen,
                index,
                Elsewhere {
                    name,
                    output: host.map_or(home.0.clone(), |h| h.name.0.clone()),
                    desktop: index.saturating_add(1),
                    sources: vec![source],
                },
            ));
        }
        elsewhere.sort_by_key(|(screen, index, _)| (*screen, *index));
        let elsewhere: Vec<Elsewhere> = elsewhere.into_iter().map(|(_, _, e)| e).collect();
        self.agents.board.view(
            self.clock.now(),
            self.pandas_on(),
            self.power_saver(),
            &elsewhere,
        )
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
        let Some((source, pane)) = pane_of(key) else {
            return;
        };
        let Some(herdr) = self.agents.herdr.get(source).and_then(Option::as_ref) else {
            return;
        };
        match ask {
            AgentAsk::Show => herdr.focus(pane),
            AgentAsk::Yes | AgentAsk::No if self.agents.board.waits(key) => {
                herdr.keys(pane, if ask == AgentAsk::Yes { YES } else { NO });
                self.close_agent_toasts(key);
            }
            AgentAsk::Yes | AgentAsk::No => {}
        }
    }

    /// The agents a question can be handed to (#197), as key and name:
    /// those working for the desktop the column's screen shows first, then
    /// those of no desktop, then the others.
    pub(crate) fn agent_choices(&self) -> Vec<(String, String)> {
        if !self.config.agents.enabled {
            return Vec::new();
        }
        let here = self
            .column_screen()
            .and_then(|o| self.model.output(&OutputName(o.name())))
            .map(|o| o.shown);
        let mut choices: Vec<(u8, String, String)> = self
            .agents
            .board
            .agents()
            .map(|(key, name, source)| {
                let rank = match self.source_desktop(source) {
                    Some(d) if Some(d) == here => 0,
                    None => 1,
                    Some(_) => 2,
                };
                (rank, key.to_owned(), name.to_owned())
            })
            .collect();
        choices.sort_by_key(|(rank, ..)| *rank);
        choices.into_iter().map(|(_, k, n)| (k, n)).collect()
    }

    /// Hands `text` to the agent of this key as a prompt (#197); whether
    /// it went to its herdr.
    pub(crate) fn prompt_agent(&mut self, key: &str, text: &str) -> bool {
        let Some((source, pane)) = pane_of(key) else {
            return false;
        };
        let Some(herdr) = self.agents.herdr.get(source).and_then(Option::as_ref) else {
            return false;
        };
        herdr.prompt(pane, text);
        true
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
