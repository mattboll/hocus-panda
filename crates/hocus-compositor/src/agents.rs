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
//! one: its process is among the pane's. An agent is named by herdr (its
//! tab's own name, else its workspace's label), else by its project's
//! folder, else by its process's working directory
//! ([`hocus_agents::agent_name`]); never by its tool. An agent is
//! forgotten when its session ends, or when its process does (a pidfd on
//! the loop, no polling). Its window is the one whose client is the
//! nearest process above it; for an agent herdr sees whose own processes
//! reach no window (they run under herdr's server, a daemon), herdr's
//! terminal's: the window of the terminal running herdr's client, its
//! clients looked for again off the loop as the user goes to it (#500). The shell is told
//! the tools and the agents in a line each, after the turn of the loop in
//! which they changed: a burst of reports is one line, and a report that
//! changes nothing sends none.
//!
//! A finished agent stays "not seen yet" (`done` to the shell) until the
//! user has seen it (4.1): its window gets the keyboard after it finished
//! (or has it as it finishes), or its row in the bar is clicked; it is then
//! `seen`, until it finishes again. herdr's terminal's window is shared by
//! all its panes: its getting the keyboard shows only the agent of the
//! pane herdr has focused (herdr also turns `done` into `idle` once a pane
//! is looked at).

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
use hocus_link::{AgentShown, AgentTool, Row, ToShell, ToolState};
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

/// A finished agent's state once seen, as the shell is told it.
const SEEN: &str = "seen";

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
    /// herdr's clients looked for again ([`herdr::terminal_processes`]),
    /// and the go-to that waited for them.
    Terminals(Vec<Vec<u32>>, Option<GoingTo>),
}

/// A go-to waiting for herdr's terminal to be found: the agent, its row,
/// and its row's colour as the click found it.
#[derive(Debug)]
pub struct GoingTo {
    id: u64,
    row: Option<Row>,
    colour: [u8; 3],
}

/// How an agent's window was found, as the journal says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    /// Above its own process.
    Own,
    /// herdr's terminal's, the terminal running herdr's client.
    Herdr(Pick),
    /// No window: why.
    None(&'static str),
}

/// Which of herdr's terminal's windows (a terminal process may have
/// several, as kitty's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Only,
    /// The one whose title names herdr.
    Titled,
    /// None named herdr: the most recently focused.
    Recent,
}

impl How {
    fn says(self) -> &'static str {
        match self {
            Self::Own => "above its process",
            Self::Herdr(Pick::Only) => "herdr's terminal, its only window",
            Self::Herdr(Pick::Titled) => "herdr's terminal, its window titled herdr",
            Self::Herdr(Pick::Recent) => "herdr's terminal, its most recently focused window",
            Self::None(why) => why,
        }
    }
}

/// An agent as the shell is told it, how its window was found, and
/// whether that window getting the keyboard shows it (herdr's terminal
/// shows only herdr's focused pane).
#[derive(Debug)]
struct Listed {
    shown: AgentShown,
    how: How,
    looks: bool,
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
    /// Its project's folder (a hook's first `cwd`, else its process's
    /// working directory), or the command's name; empty while unknown.
    name: String,
    state: State,
    /// Since when, in microseconds of `CLOCK_MONOTONIC` (the shell's).
    since: u64,
    /// What it asks for, or why it failed, as its tool said it in this
    /// state (4.6); never logged but its length.
    note: Option<hocus_agents::Note>,
}

/// herdr, as last heard.
#[derive(Debug, Default)]
struct Herdr {
    running: bool,
    /// Its agents, by pane: their state, since when.
    agents: Vec<(herdr::Agent, u64)>,
    /// For each of its clients, the processes it runs under (its
    /// terminal's), nearest first.
    terminals: Vec<Vec<u32>>,
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
    /// The finished agents seen, by id: since when they were finished
    /// when seen (finished again later, they are not seen yet).
    seen: HashMap<u64, u64>,
    /// Commits of the agents' surfaces, for tests: none at rest.
    pub commits: u64,
    /// For the threads that look for herdr's clients.
    sender: Option<channel::Sender<Heard>>,
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
    state.agents.sender = Some(sender.clone());
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
        if let Err(err) = herdr::start(
            places.herdr_socket.clone(),
            places.processes.clone(),
            move |news| herdr.send(Heard::Herdr(news)).is_ok(),
        ) {
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
            Heard::Terminals(terminals, going) => self.herdr_terminals(terminals, going),
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
                    note: None,
                });
                agents.list.len() - 1
            }
        };
        let agent = &mut agents.list[index];
        if !agent.keys.contains(&key) {
            agent.keys.push(key);
        }
        let changed = agent.state != state;
        if agent.state != state {
            agent.state = state;
            agent.since = now;
        }
        agent.note = hocus_agents::next_note(agent.note.take(), changed, report.note);
        // A hook's agent keeps the folder it was first named by: a later
        // event's `cwd` may be a directory the agent went down into.
        if !report.name.is_empty() && (agent.name.is_empty() || report.source == Source::Command) {
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
            News::Names(names) => {
                for (agent, _) in &mut herdr.agents {
                    if let Some((_, name)) = names.iter().find(|(pane, _)| *pane == agent.pane) {
                        agent.name.clone_from(name);
                    }
                }
            }
            News::State { pane, state } => {
                if let Some((agent, since)) = herdr.agents.iter_mut().find(|(a, _)| a.pane == pane)
                    && agent.state != state
                {
                    agent.state = state;
                    *since = now;
                }
            }
            News::Focused(pane) => {
                for (agent, _) in &mut herdr.agents {
                    agent.focused = agent.pane == pane;
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
        self.agents_listed().into_iter().map(|l| l.shown).collect()
    }

    /// The same, with how each one's window was found.
    fn agents_listed(&mut self) -> Vec<Listed> {
        let windows = self.windows_by_pid();
        let own = |ancestry: &[u32]| {
            ancestry
                .iter()
                .find_map(|pid| windows.get(pid).and_then(|w| w.last()).copied())
        };
        let (herdr_window, herdr_how) = self.herdr_window(&windows);
        // An agent in one of herdr's panes whose own processes reach no
        // window: herdr's terminal's, which shows it when its pane is
        // the one focused.
        let in_herdr = |focused: bool| match herdr_window {
            Some(w) => (w.0, herdr_how, focused),
            None => (0, herdr_how, false),
        };
        let followed = self.herdr_followed();
        let mut shown: Vec<Listed> = Vec::new();
        let mut merged: Vec<String> = Vec::new();
        for agent in &self.agents.list {
            let mut sources: Vec<Source> = agent.keys.iter().map(|(s, _)| *s).collect();
            let (mut state, mut since) = (agent.state, agent.since);
            // The hook's note, while its state is the one shown.
            let mut note = agent.note.clone();
            let (mut herdr_name, mut herdr_folder) = (None, None);
            let mut pane_focused = None;
            if followed {
                for (pane, herdr_since) in &self.agents.herdr.agents {
                    if pane.pids.iter().any(|p| agent.pids.contains(p)) {
                        merged.push(pane.pane.clone());
                        sources.push(Source::Herdr);
                        herdr_name = pane.name.as_deref();
                        herdr_folder = pane.folder.as_deref();
                        pane_focused = Some(pane.focused);
                        if let Some(herdr_state) = pane.state
                            && *herdr_since > since
                        {
                            state = herdr_state;
                            since = *herdr_since;
                            note = None;
                        }
                    }
                }
            }
            sources.sort_unstable();
            sources.dedup();
            let source: Vec<&str> = sources.iter().map(|s| s.name()).collect();
            let name = hocus_agents::agent_name(herdr_name, Some(&agent.name), herdr_folder)
                .or_else(|| agent.keys.first().map(|(_, key)| key.clone()))
                .unwrap_or_default();
            let (window, how, looks) = match (own(&agent.ancestry), pane_focused) {
                (Some(w), _) => (w.0, How::Own, true),
                (None, Some(focused)) => in_herdr(focused),
                (None, None) => (0, How::None("no window above its process"), false),
            };
            shown.push(Listed {
                shown: AgentShown {
                    id: agent.id,
                    state: state.name().to_owned(),
                    since,
                    source: source.join("+"),
                    window,
                    name,
                    note,
                },
                how,
                looks,
            });
        }
        if followed {
            let pending: Vec<(String, String, State, u64, bool)> = self
                .agents
                .herdr
                .agents
                .iter()
                .filter(|(a, _)| !merged.contains(&a.pane))
                .map(|(a, since)| {
                    let name =
                        hocus_agents::agent_name(a.name.as_deref(), a.folder.as_deref(), None)
                            .unwrap_or_else(|| a.pane.clone());
                    (
                        a.pane.clone(),
                        name,
                        a.state.unwrap_or(State::Idle),
                        *since,
                        a.focused,
                    )
                })
                .collect();
            for (pane, name, state, since, focused) in pending {
                let id = match self.agents.herdr.ids.get(&pane) {
                    Some(id) => *id,
                    None => {
                        self.agents.next_id += 1;
                        self.agents.herdr.ids.insert(pane, self.agents.next_id);
                        self.agents.next_id
                    }
                };
                let (window, how, looks) = in_herdr(focused);
                shown.push(Listed {
                    shown: AgentShown {
                        id,
                        state: state.name().to_owned(),
                        since,
                        source: Source::Herdr.name().to_owned(),
                        window,
                        name,
                        // herdr says no more than its state.
                        note: None,
                    },
                    how,
                    looks,
                });
            }
        }
        shown.sort_by_key(|a| a.shown.id);
        self.mark_seen(&mut shown);
        shown
    }

    /// herdr's terminal's window, and how it was found: of the windows of
    /// the nearest process above each of herdr's clients that has some,
    /// the one whose title names herdr, else the most recently focused.
    fn herdr_window(&self, windows: &BTreeMap<u32, Vec<WindowId>>) -> (Option<WindowId>, How) {
        let terminals = &self.agents.herdr.terminals;
        if terminals.is_empty() {
            return (None, How::None("no herdr client found"));
        }
        let theirs: Vec<WindowId> = terminals
            .iter()
            .filter_map(|above| above.iter().find_map(|pid| windows.get(pid)))
            .flatten()
            .copied()
            .collect();
        let candidates: Vec<(WindowId, bool)> = self
            .model
            .stack()
            .iter()
            .filter(|w| theirs.contains(w))
            .map(|w| (*w, names_herdr(&crate::windows::title_of(self, *w))))
            .collect();
        match pick(&candidates, self.model.recent()) {
            Some((window, how)) => (Some(window), How::Herdr(how)),
            None => (None, How::None("no window above herdr's clients")),
        }
    }

    /// The agents as the halo sees them (4.4): an agent is looked at when
    /// its window has the keyboard and shows it, as a finished one is seen
    /// (herdr's terminal shows only herdr's focused pane).
    fn halo_seen(&self, listed: &[Listed]) -> Vec<crate::halo::Seen> {
        let focused = self.model.focused().map(|w| w.0);
        listed
            .iter()
            .map(
                |Listed {
                     shown: agent,
                     looks,
                     ..
                 }| crate::halo::Seen {
                    id: agent.id,
                    concern: crate::halo::Concern::of_agent(&agent.state),
                    since: Duration::from_micros(agent.since),
                    looked_at: agent.window != 0 && *looks && focused == Some(agent.window),
                },
            )
            .collect()
    }

    /// The finished agents among `listed` whose window has the keyboard
    /// and shows them are seen; those seen say so (`seen`).
    fn mark_seen(&mut self, listed: &mut [Listed]) {
        let focused = self.model.focused().map(|w| w.0);
        let seen = &mut self.agents.seen;
        seen.retain(|id, _| listed.iter().any(|a| a.shown.id == *id));
        for Listed {
            shown: agent,
            looks,
            ..
        } in listed
            .iter_mut()
            .filter(|a| a.shown.state == State::Done.name())
        {
            if agent.window != 0 && *looks && focused == Some(agent.window) {
                seen.insert(agent.id, agent.since);
            }
            if seen.get(&agent.id) == Some(&agent.since) {
                SEEN.clone_into(&mut agent.state);
            }
        }
    }

    /// The user goes to agent `id` from its `row`: its window, if it has
    /// one, on its desktop with the keyboard, the streak running from the
    /// row to it in the row's colour (4.3); herdr's pane selected when it
    /// is herdr's; finished, it is seen.
    pub fn go_to_agent(&mut self, id: u64, row: Option<Row>) {
        let Some(listed) = self.agents_listed().into_iter().find(|a| a.shown.id == id) else {
            tracing::debug!(id, "the shell goes to an agent that is gone");
            return;
        };
        let agent = &listed.shown;
        if agent.state == State::Done.name() {
            self.agents.seen.insert(id, agent.since);
        }
        // Its halo ends (§8 step 5), whatever it waits for or failed at.
        self.halo.acknowledge(id);
        let going = GoingTo {
            id,
            row,
            colour: crate::going::colour(&agent.state),
        };
        match self.herdr_pane(id) {
            Some(pane) if listed.how != How::Own => {
                self.select_herdr_pane(pane);
                // Its window is herdr's terminal's: its clients are looked
                // for again, as one may have attached since its agents
                // were listed (herdr's server outlives them).
                self.look_for_herdr_terminals(Some(going));
            }
            pane => {
                if let Some(pane) = pane {
                    self.select_herdr_pane(pane);
                }
                self.go_to_agent_window(&listed, going);
            }
        }
        self.agents.dirty = true;
    }

    /// The go-to's window, if the agent has one, with the keyboard and
    /// the streak; said in the journal either way (never the agent's
    /// name: its text is the user's).
    fn go_to_agent_window(&mut self, listed: &Listed, going: GoingTo) {
        let agent = &listed.shown;
        let window = WindowId(agent.window);
        if agent.window != 0 && self.windows.contains_key(&window) {
            tracing::info!(
                id = agent.id,
                source = agent.source,
                window = agent.window,
                how = listed.how.says(),
                "going to an agent's window"
            );
            let from = going.row.and_then(|row| self.row_start(&row));
            self.go_to(window, from, going.colour);
        } else {
            tracing::info!(
                id = agent.id,
                source = agent.source,
                why = listed.how.says(),
                "going to an agent: no window"
            );
        }
    }

    /// herdr's clients looked for on a thread (it reads every process),
    /// then `going` goes on.
    fn look_for_herdr_terminals(&mut self, going: Option<GoingTo>) {
        let (Some(sender), Some(proc)) = (
            self.agents.sender.clone(),
            self.agents.places.as_ref().map(|p| p.processes.clone()),
        ) else {
            if let Some(going) = going {
                self.herdr_terminals(self.agents.herdr.terminals.clone(), Some(going));
            }
            return;
        };
        crate::programs::on_a_thread("herdr's clients".to_owned(), move || {
            let terminals = herdr::terminal_processes(&proc);
            let _ = sender.send(Heard::Terminals(terminals, going));
        });
    }

    /// herdr's clients as just found; the go-to that waited for them goes
    /// to their terminal's window.
    fn herdr_terminals(&mut self, terminals: Vec<Vec<u32>>, going: Option<GoingTo>) {
        if self.agents.herdr.running {
            self.agents.herdr.terminals = terminals;
        }
        let Some(going) = going else {
            return;
        };
        match self
            .agents_listed()
            .into_iter()
            .find(|a| a.shown.id == going.id)
        {
            Some(listed) => self.go_to_agent_window(&listed, going),
            None => tracing::debug!(id = going.id, "the agent gone to is gone"),
        }
    }

    /// Where the streak starts from `row`, global: its left edge, at its
    /// middle. The shell knows where the row is in its bar, the compositor
    /// where the bar is.
    fn row_start(&self, row: &Row) -> Option<(f64, f64)> {
        let output = self.space.outputs().find(|o| o.name() == row.output)?;
        let screen = self.space.output_geometry(output)?;
        let edge = hocus_link::EDGE;
        Some((
            f64::from(screen.loc.x + screen.size.w - edge + row.x),
            f64::from(screen.loc.y + edge + row.y) + f64::from(row.height) / 2.0,
        ))
    }

    /// Agent `id`'s pane in herdr, when it is herdr's (its own, or a hook's
    /// agent in one of its panes) and herdr is followed.
    fn herdr_pane(&self, id: u64) -> Option<String> {
        if !self.herdr_followed() {
            return None;
        }
        let herdr = &self.agents.herdr;
        let listed = |pane: &String| herdr.agents.iter().any(|(a, _)| a.pane == *pane);
        if let Some((pane, _)) = herdr.ids.iter().find(|(p, i)| **i == id && listed(p)) {
            return Some(pane.clone());
        }
        let agent = self.agents.list.iter().find(|a| a.id == id)?;
        herdr
            .agents
            .iter()
            .find(|(pane, _)| pane.pids.iter().any(|p| agent.pids.contains(p)))
            .map(|(pane, _)| pane.pane.clone())
    }

    /// herdr selects `pane` (§8 step 3), on a thread: its socket is no
    /// work for the loop.
    fn select_herdr_pane(&self, pane: String) {
        let Some(socket) = self.agents.places.as_ref().map(|p| p.herdr_socket.clone()) else {
            return;
        };
        tracing::debug!(pane, "herdr selects the agent's pane");
        crate::programs::on_a_thread("herdr's pane".to_owned(), move || {
            if let Err(err) = herdr::select(&socket, &pane) {
                tracing::warn!(pane, %err, "herdr did not select the agent's pane");
            }
        });
    }

    /// The keyboard went to another window: a finished agent's may be
    /// seen now. herdr's clients are looked for again (off the loop): the
    /// window may be a terminal herdr attached to since.
    pub fn agents_focus_changed(&mut self) {
        if !self.agents.list.is_empty() || !self.agents.herdr.agents.is_empty() {
            self.agents.dirty = true;
        }
        if self.herdr_followed() && !self.agents.herdr.agents.is_empty() {
            self.look_for_herdr_terminals(None);
        }
    }

    /// For tests: as if every agent had been in its state `by` longer.
    pub fn backdate_agents(&mut self, by: Duration) {
        let by = crate::panel::micros(by);
        for agent in &mut self.agents.list {
            agent.since = agent.since.saturating_sub(by);
        }
        for (_, since) in &mut self.agents.herdr.agents {
            *since = since.saturating_sub(by);
        }
        self.agents.dirty = true;
    }

    /// The mapped windows by their client's process, bottom first: the
    /// last one of each process is its topmost.
    fn windows_by_pid(&self) -> BTreeMap<u32, Vec<WindowId>> {
        let mut out: BTreeMap<u32, Vec<WindowId>> = BTreeMap::new();
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
                out.entry(pid).or_default().push(*id);
            }
        }
        out
    }

    /// The halo and the shell are told what changed, once per turn of the
    /// loop.
    pub fn flush_agents(&mut self) {
        if !self.agents.dirty {
            return;
        }
        self.agents.dirty = false;
        let listed = self.agents_listed();
        let seen = self.halo_seen(&listed);
        let agents: Vec<AgentShown> = listed.into_iter().map(|l| l.shown).collect();
        self.halo_agents(&seen);
        if !self.shell_linked() {
            // Told everything once it connects (`tell_agents`).
            return;
        }
        let tools = self.agent_tools();
        if self.agents.told_tools.as_ref() != Some(&tools) {
            self.agents.told_tools = Some(tools.clone());
            self.tell_shell(ToShell::AgentTools(tools));
        }
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

/// Whether a window's title names herdr: its first word, as the shell
/// titles a terminal with the command it runs ("herdr", "herdr session
/// attach work"); not a file of that name being edited.
fn names_herdr(title: &str) -> bool {
    title
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("herdr"))
}

/// Among herdr's terminal's windows (`candidates`, bottom first, with
/// whether each one's title names herdr), the one to go to: the only one;
/// else, of those named herdr if any, the most recently focused
/// (`recent`, the most recent first), the topmost of those never focused.
fn pick(candidates: &[(WindowId, bool)], recent: &[WindowId]) -> Option<(WindowId, Pick)> {
    let titled: Vec<WindowId> = candidates
        .iter()
        .filter(|(_, titled)| *titled)
        .map(|(w, _)| *w)
        .collect();
    let all: Vec<WindowId> = candidates.iter().map(|(w, _)| *w).collect();
    let (among, how) = match (all.len(), titled.len()) {
        (0, _) => return None,
        (1, _) => (all, Pick::Only),
        (_, 0) => (all, Pick::Recent),
        _ => (titled, Pick::Titled),
    };
    let rank = |w: &WindowId| recent.iter().position(|r| r == w).unwrap_or(usize::MAX);
    // The first of equals: the topmost.
    among
        .iter()
        .rev()
        .min_by_key(|w| rank(w))
        .map(|w| (*w, how))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_names_herdr_by_its_first_word() {
        assert!(names_herdr("herdr"));
        assert!(names_herdr("  HERDR session attach work"));
        assert!(!names_herdr("vim herdr.rs"));
        assert!(!names_herdr("herdr.rs"));
        assert!(!names_herdr("zsh"));
        assert!(!names_herdr(""));
    }

    #[test]
    fn herdrs_window_is_the_one_titled_herdr_else_the_last_focused() {
        let w = WindowId;
        // Two of kitty's windows: the one titled herdr, though behind and
        // focused longer ago.
        assert_eq!(
            pick(&[(w(1), true), (w(2), false)], &[w(2), w(1)]),
            Some((w(1), Pick::Titled))
        );
        // None titled: the most recently focused, not the topmost.
        assert_eq!(
            pick(&[(w(1), false), (w(2), false)], &[w(1), w(2)]),
            Some((w(1), Pick::Recent))
        );
        // Never focused: the topmost.
        assert_eq!(
            pick(&[(w(1), false), (w(2), false), (w(3), false)], &[]),
            Some((w(3), Pick::Recent))
        );
        // Two titled (two terminals attached): the last focused of them.
        assert_eq!(
            pick(&[(w(1), true), (w(2), false), (w(3), true)], &[w(2), w(1)]),
            Some((w(1), Pick::Titled))
        );
        assert_eq!(pick(&[(w(4), false)], &[]), Some((w(4), Pick::Only)));
        assert_eq!(pick(&[], &[w(1)]), None);
    }
}
