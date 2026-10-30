//! The agents and their states (bar.md §4 group 3, §10; 4.0), kept on the
//! loop and fed by their sources, each off the loop:
//!
//! - **The control socket**, `<socket>.control` (`HOCUS_SOCKET` for the
//!   programs the compositor starts): `hocus agent` and the tools' hooks
//!   (`hocus agent-hook`) write one line each. A thread of its own takes
//!   the connections, reads the agent's process's ancestry in `/proc`, and
//!   hands both to the loop.
//! - **The tools' watcher** ([`hocus_agents::watch`]): which tools are on
//!   the computer and plugged in, as they change; plugging in, unplugging
//!   and declining, which write files, are done there.
//! - **herdr** ([`hocus_agents::herdr`]), when it runs and is not declined.
//!
//! The same agent seen by two sources (a Claude Code in a herdr pane) is
//! one: its process is among the pane's. An agent is forgotten when its
//! session ends, or when its process does (a pidfd on the loop, no
//! polling). Its window is the one whose client is the nearest process
//! above it. The shell is told the tools and the agents in a line each,
//! after the turn of the loop in which they changed: a burst of reports is
//! one line, and a report that changes nothing sends none.

use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use hocus_agents::{
    Report, Source, State,
    herdr::{self, News},
    tools::{Places, Tool},
    watch::{Ask, Snapshot, ToolState as Seen, Watcher},
};
use hocus_link::{AgentShown, AgentTool, ToShell, ToolState};
use smithay::{
    reexports::{
        calloop::{
            Interest, LoopHandle, Mode, PostAction, RegistrationToken,
            channel::{self, Event},
            generic::Generic,
        },
        wayland_server::Resource,
    },
    wayland::seat::WaylandFocus,
};

use crate::{App, State as Compositor, model::WindowId};

/// What a reporting connection may hold, lines, and how long it is read.
const MOST_LINES: usize = 64;
const READ: Duration = Duration::from_secs(1);

/// What the sources hand the loop.
#[derive(Debug)]
pub enum Heard {
    /// A report, and its process's ancestry, nearest first.
    Report(Report, Vec<u32>),
    Tools(Snapshot),
    Herdr(News),
}

/// An agent a hook or a command told about.
#[derive(Debug)]
struct Agent {
    id: u64,
    /// The sources' names for it.
    keys: Vec<(Source, String)>,
    /// Its processes.
    pids: Vec<u32>,
    /// Above its process, where its window is looked for.
    ancestry: Vec<u32>,
    name: String,
    state: State,
    /// Since when, in microseconds of `CLOCK_MONOTONIC` (the shell's).
    since: u64,
}

/// herdr, as last heard.
#[derive(Debug, Default)]
struct Herdr {
    running: bool,
    /// Its agents, by pane: their state, since when.
    agents: Vec<(herdr::Agent, u64)>,
    /// The processes its clients run under (its terminal's).
    terminals: Vec<u32>,
    /// Ids for its panes' agents.
    ids: HashMap<String, u64>,
}

/// The agents' sources and model.
#[derive(Debug, Default)]
pub struct Agents {
    list: Vec<Agent>,
    next_id: u64,
    tools: Option<Snapshot>,
    herdr: Herdr,
    watcher: Option<Watcher>,
    places: Option<Places>,
    /// Each agent process's exit, watched on the loop.
    exits: HashMap<u32, RegistrationToken>,
    /// The control socket.
    pub socket: Option<PathBuf>,
    /// Something changed since the shell was last told.
    dirty: bool,
    told_tools: Option<Vec<AgentTool>>,
    pub told: Option<Vec<AgentShown>>,
    /// Commits of the agents' surfaces, for tests: none at rest.
    pub commits: u64,
}

/// Starts the sources: the control socket always (`hocus agent` works in
/// tests too); the tools' watcher and herdr when the configuration says
/// where they are (never in tests, unless they give a home of their own).
pub fn start(handle: &LoopHandle<'static, App>, state: &mut Compositor) -> anyhow::Result<()> {
    let (sender, heard) = channel::channel::<Heard>();
    handle
        .insert_source(heard, |event, _, app| {
            if let Event::Msg(heard) = event {
                app.state.agents_heard(heard);
            }
        })
        .map_err(|err| err.error)
        .context("could not hear the agents")?;
    let path = hocus_agents::socket_path(&state.socket_path);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("could not remove {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("could not listen on {}", path.display()))?;
    let reports = sender.clone();
    std::thread::Builder::new()
        .name("agents' reports".to_owned())
        .spawn(move || listen(&listener, &reports))
        .context("could not start the agents' reports' thread")?;
    tracing::info!(control = %path.display(), "agents' control socket");
    state.agents.socket = Some(path);
    if let Some(places) = state.config.agent_places.clone() {
        let tools = sender.clone();
        match Watcher::start(places.clone(), move |seen| {
            let _ = tools.send(Heard::Tools(seen));
        }) {
            Ok(watcher) => state.agents.watcher = Some(watcher),
            Err(err) => tracing::warn!(%err, "cannot watch for the agents' tools"),
        }
        let herdr = sender;
        if let Err(err) = herdr::start(places.herdr_socket.clone(), move |news| {
            herdr.send(Heard::Herdr(news)).is_ok()
        }) {
            tracing::warn!(%err, "cannot follow herdr");
        }
        state.agents.places = Some(places);
    }
    state.agents.dirty = true;
    Ok(())
}

/// Takes the reporting connections, one at a time: each is a few lines,
/// read for a second at most.
fn listen(listener: &UnixListener, sender: &channel::Sender<Heard>) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                tracing::warn!(%err, "could not take an agent's report");
                continue;
            }
        };
        if stream.set_read_timeout(Some(READ)).is_err() {
            continue;
        }
        for line in BufReader::new(stream).lines().take(MOST_LINES) {
            let Ok(line) = line else {
                break;
            };
            match line.parse::<Report>() {
                Ok(report) => {
                    let ancestry = report
                        .pid
                        .map(hocus_agents::process::ancestry)
                        .unwrap_or_default();
                    if sender.send(Heard::Report(report, ancestry)).is_err() {
                        // The loop is gone.
                        return;
                    }
                }
                Err(err) => tracing::debug!(%err, "a report that does not read"),
            }
        }
    }
}

impl Compositor {
    fn agents_heard(&mut self, heard: Heard) {
        match heard {
            Heard::Report(report, ancestry) => self.agent_reported(report, ancestry),
            Heard::Tools(seen) => {
                tracing::debug!(?seen, "the agents' tools");
                self.agents.tools = Some(seen);
            }
            Heard::Herdr(news) => self.herdr_said(news),
        }
        self.agents.dirty = true;
    }

    /// Now, in microseconds of `CLOCK_MONOTONIC`.
    fn now_us(&self) -> u64 {
        crate::panel::micros(Duration::from(self.clock.now()))
    }

    fn agent_reported(&mut self, report: Report, ancestry: Vec<u32>) {
        let now = self.now_us();
        let key = (report.source, report.key.clone());
        let agents = &mut self.agents;
        let Some(state) = report.state else {
            // Its session ended.
            for agent in &mut agents.list {
                agent.keys.retain(|k| *k != key);
            }
            let gone: Vec<u64> = agents
                .list
                .iter()
                .filter(|a| a.keys.is_empty())
                .map(|a| a.id)
                .collect();
            tracing::debug!(?key, "an agent's session ended");
            self.forget_agents(&gone);
            return;
        };
        let at = agents
            .list
            .iter()
            .position(|a| a.keys.contains(&key))
            .or_else(|| {
                let pid = report.pid?;
                // The same process seen by another source: the same
                // agent. One source's keys are its own agents.
                agents.list.iter().position(|a| {
                    a.pids.contains(&pid) && !a.keys.iter().any(|(s, _)| *s == report.source)
                })
            });
        let index = match at {
            Some(index) => index,
            None => {
                agents.next_id += 1;
                agents.list.push(Agent {
                    id: agents.next_id,
                    keys: Vec::new(),
                    pids: Vec::new(),
                    ancestry: Vec::new(),
                    name: report.name.clone(),
                    state,
                    since: now,
                });
                agents.list.len() - 1
            }
        };
        let agent = &mut agents.list[index];
        if !agent.keys.contains(&key) {
            agent.keys.push(key);
        }
        if agent.state != state {
            agent.state = state;
            agent.since = now;
        }
        if !report.name.is_empty() {
            agent.name = report.name;
        }
        if !ancestry.is_empty() {
            agent.ancestry = ancestry;
        }
        let new_pid = report.pid.filter(|p| !agent.pids.contains(p));
        if let Some(pid) = new_pid {
            agent.pids.push(pid);
            self.watch_exit(pid);
        }
    }

    /// Process `pid`'s end forgets its agents.
    fn watch_exit(&mut self, pid: u32) {
        if self.agents.exits.contains_key(&pid) {
            return;
        }
        let fd = match hocus_agents::process::exit_of(pid) {
            Ok(fd) => fd,
            Err(err) => {
                tracing::debug!(pid, %err, "cannot watch an agent's process");
                return;
            }
        };
        let token = self.loop_handle.insert_source(
            Generic::new(Arc::new(fd), Interest::READ, Mode::OneShot),
            move |_, _, app| {
                app.state.agent_process_ended(pid);
                Ok(PostAction::Remove)
            },
        );
        match token {
            Ok(token) => {
                self.agents.exits.insert(pid, token);
            }
            Err(err) => tracing::warn!(%err.error, "cannot watch an agent's process"),
        }
    }

    fn agent_process_ended(&mut self, pid: u32) {
        self.agents.exits.remove(&pid);
        let gone: Vec<u64> = self
            .agents
            .list
            .iter()
            .filter(|a| a.pids.contains(&pid))
            .map(|a| a.id)
            .collect();
        tracing::debug!(pid, ?gone, "an agent's process ended");
        self.forget_agents(&gone);
        self.agents.dirty = true;
    }

    fn forget_agents(&mut self, ids: &[u64]) {
        let (gone, kept): (Vec<Agent>, Vec<Agent>) = std::mem::take(&mut self.agents.list)
            .into_iter()
            .partition(|a| ids.contains(&a.id));
        self.agents.list = kept;
        for pid in gone.iter().flat_map(|a| a.pids.iter()) {
            let used = self.agents.list.iter().any(|a| a.pids.contains(pid));
            if !used && let Some(token) = self.agents.exits.remove(pid) {
                self.loop_handle.remove(token);
            }
        }
    }

    fn herdr_said(&mut self, news: News) {
        let now = self.now_us();
        let herdr = &mut self.agents.herdr;
        match news {
            News::Agents { agents, terminals } => {
                herdr.running = true;
                let before: HashMap<String, (Option<State>, u64)> = herdr
                    .agents
                    .drain(..)
                    .map(|(a, since)| (a.pane.clone(), (a.state, since)))
                    .collect();
                herdr.agents = agents
                    .into_iter()
                    .map(|a| {
                        let since = match before.get(&a.pane) {
                            Some((state, since)) if *state == a.state => *since,
                            _ => now,
                        };
                        (a, since)
                    })
                    .collect();
                herdr.terminals = terminals;
            }
            News::State { pane, state } => {
                if let Some((agent, since)) = herdr.agents.iter_mut().find(|(a, _)| a.pane == pane)
                    && agent.state != state
                {
                    agent.state = state;
                    *since = now;
                }
            }
            News::Gone => {
                herdr.running = false;
                herdr.agents.clear();
                herdr.terminals.clear();
            }
        }
    }

    /// Whether herdr's agents are followed: it runs, and was not declined.
    fn herdr_followed(&self) -> bool {
        self.agents.herdr.running && !self.agents.tools.is_some_and(|t| t.herdr_declined)
    }

    /// The tools as the shell shows them.
    pub fn agent_tools(&self) -> Vec<AgentTool> {
        let shown = |seen: Seen| match seen {
            Seen::Absent => ToolState::Absent,
            Seen::Found => ToolState::Found,
            Seen::Declined => ToolState::Declined,
            Seen::Plugged => ToolState::Plugged,
        };
        let tools = self.agents.tools;
        let file = |tool: Tool| {
            self.agents
                .places
                .as_ref()
                .map(|p| p.settings_shown(tool))
                .unwrap_or_default()
        };
        let mut out: Vec<AgentTool> = [Tool::ClaudeCode, Tool::Codex]
            .into_iter()
            .map(|tool| AgentTool {
                tool: tool.word().to_owned(),
                state: tools.map_or(ToolState::Absent, |t| shown(t.of(tool))),
                file: file(tool),
            })
            .collect();
        let herdr = match (
            self.agents.herdr.running,
            tools.is_some_and(|t| t.herdr_declined),
        ) {
            (true, false) => ToolState::Running,
            (true, true) => ToolState::Ignored,
            (false, _) => ToolState::Stopped,
        };
        out.push(AgentTool {
            tool: "herdr".to_owned(),
            state: herdr,
            file: String::new(),
        });
        out
    }

    /// The agents as the shell shows them: the hooks' and the commands',
    /// and herdr's when it is followed, one agent where they share a
    /// process; the most recent state wins.
    pub fn agents_shown(&mut self) -> Vec<AgentShown> {
        let windows = self.windows_by_pid();
        let window_of = |ancestry: &[u32]| {
            ancestry
                .iter()
                .find_map(|pid| windows.get(pid).copied())
                .map_or(0, |w: WindowId| w.0)
        };
        let followed = self.herdr_followed();
        let mut shown: Vec<AgentShown> = Vec::new();
        let mut merged: Vec<String> = Vec::new();
        for agent in &self.agents.list {
            let mut sources: Vec<Source> = agent.keys.iter().map(|(s, _)| *s).collect();
            let (mut state, mut since, mut name) = (agent.state, agent.since, agent.name.clone());
            if followed {
                for (pane, herdr_since) in &self.agents.herdr.agents {
                    if pane.pids.iter().any(|p| agent.pids.contains(p)) {
                        merged.push(pane.pane.clone());
                        sources.push(Source::Herdr);
                        name.clone_from(&pane.name);
                        if let Some(herdr_state) = pane.state
                            && *herdr_since > since
                        {
                            state = herdr_state;
                            since = *herdr_since;
                        }
                    }
                }
            }
            sources.sort_unstable();
            sources.dedup();
            let source: Vec<&str> = sources.iter().map(|s| s.name()).collect();
            shown.push(AgentShown {
                id: agent.id,
                state: state.name().to_owned(),
                since,
                source: source.join("+"),
                window: window_of(&agent.ancestry),
                name,
            });
        }
        if followed {
            let terminals = self.agents.herdr.terminals.clone();
            let window = window_of(&terminals);
            let pending: Vec<(String, String, State, u64)> = self
                .agents
                .herdr
                .agents
                .iter()
                .filter(|(a, _)| !merged.contains(&a.pane))
                .map(|(a, since)| {
                    (
                        a.pane.clone(),
                        a.name.clone(),
                        a.state.unwrap_or(State::Idle),
                        *since,
                    )
                })
                .collect();
            for (pane, name, state, since) in pending {
                let id = match self.agents.herdr.ids.get(&pane) {
                    Some(id) => *id,
                    None => {
                        self.agents.next_id += 1;
                        self.agents.herdr.ids.insert(pane, self.agents.next_id);
                        self.agents.next_id
                    }
                };
                shown.push(AgentShown {
                    id,
                    state: state.name().to_owned(),
                    since,
                    source: Source::Herdr.name().to_owned(),
                    window,
                    name,
                });
            }
        }
        shown.sort_by_key(|a| a.id);
        shown
    }

    /// The mapped windows by their client's process: the topmost one of
    /// each process.
    fn windows_by_pid(&self) -> BTreeMap<u32, WindowId> {
        let mut out = BTreeMap::new();
        for id in self.model.stack() {
            let Some(managed) = self.windows.get(id) else {
                continue;
            };
            let pid = match managed.window.x11_surface() {
                Some(x11) => x11.pid(),
                None => managed.window.wl_surface().and_then(|surface| {
                    let client = self.display_handle.get_client(surface.id()).ok()?;
                    let credentials = client.get_credentials(&self.display_handle).ok()?;
                    u32::try_from(credentials.pid).ok()
                }),
            };
            if let Some(pid) = pid {
                // Bottom first: the last one is on top.
                out.insert(pid, *id);
            }
        }
        out
    }

    /// The shell is told what changed, once per turn of the loop.
    pub fn flush_agents(&mut self) {
        if !self.agents.dirty || !self.shell_linked() {
            return;
        }
        self.agents.dirty = false;
        let tools = self.agent_tools();
        if self.agents.told_tools.as_ref() != Some(&tools) {
            self.agents.told_tools = Some(tools.clone());
            self.tell_shell(ToShell::AgentTools(tools));
        }
        let agents = self.agents_shown();
        if self.agents.told.as_ref() != Some(&agents) {
            tracing::debug!(?agents, "the agents");
            self.agents.told = Some(agents.clone());
            self.tell_shell(ToShell::Agents(agents));
        }
    }

    /// The shell connected: it is told everything again.
    pub fn tell_agents(&mut self) {
        self.agents.told_tools = None;
        self.agents.told = None;
        self.agents.dirty = true;
    }

    /// Windows came or went: the agents' windows may have.
    pub fn agents_windows_changed(&mut self) {
        if !self.agents.list.is_empty() || !self.agents.herdr.agents.is_empty() {
            self.agents.dirty = true;
        }
    }

    /// The shell copies `text` (the Sources popover's command): the
    /// clipboard offers it, as a client's selection would be.
    pub fn copy_text(&mut self, text: String) {
        let types = [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "TEXT",
            "STRING",
        ]
        .map(str::to_owned)
        .to_vec();
        tracing::debug!(text, "the shell copies");
        smithay::wayland::selection::data_device::set_data_device_selection(
            &self.display_handle,
            &self.seat,
            types,
            Some(Arc::from(text)),
        );
    }

    /// The user plugs a tool in, unplugs it, or declines them.
    pub fn agent_tools_asked(&mut self, ask: Ask) {
        match &self.agents.watcher {
            Some(watcher) => watcher.ask(ask),
            None => tracing::warn!(?ask, "no agents' tools here"),
        }
    }
}

/// A tool by the shell's word for it.
pub fn tool(word: &str) -> Option<Tool> {
    Tool::from_word(word)
}
