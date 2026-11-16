//! What the agents left running after their turn (#831): « en fond »
//! rather than « repos » once the user looked ([`hocus_agents::background`]).
//!
//! Kept on the loop, for each agent of Claude Code's or Codex's hooks.
//! Claude Code's say it: the tasks its `Stop` or `SubagentStop` last
//! listed in flight, until its next turn; and the sub-agents started and
//! not stopped. Its processes are never read. A Codex agent's children
//! found running are read in `/proc` by a worker thread of their own,
//! never on the loop, which sends what it found back by the agents'
//! channel: once as the agent's turn ends (`Stop`), then every [`EVERY`]
//! while one still runs (or one too young to count may), and no more once
//! none does. Nothing is read while no Codex agent's turn has ended.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, SystemTime},
};

use hocus_agents::{
    Source, State,
    background::{self, Found, Ran, Running as Said, Watched},
};
use smithay::reexports::calloop::{
    channel,
    timer::{TimeoutAction, Timer},
};

use super::Heard;
use crate::State as Compositor;

/// How often an agent's children are read again while one runs.
pub const EVERY: Duration = Duration::from_secs(2);

/// What an agent left running, as last known.
#[derive(Debug)]
pub(crate) struct Running {
    /// When it was first heard of, and when its first turn started, in
    /// clock ticks since boot (`/proc`'s): its children started before are
    /// its MCP servers'.
    first_heard: u64,
    first_turn: Option<u64>,
    /// Its children as last read since its turn ended (Codex's).
    processes: Found,
    /// The sub-agents started and not stopped, by id.
    subagents: BTreeSet<String>,
    /// The tasks in flight its tool last listed (Claude Code's), until
    /// its next turn.
    tasks: u32,
}

impl Running {
    pub(crate) fn new() -> Self {
        Self {
            first_heard: background::now_ticks().0,
            first_turn: None,
            processes: Found::default(),
            subagents: BTreeSet::new(),
            tasks: 0,
        }
    }

    /// Whether something it started still runs.
    pub(crate) fn any(&self) -> bool {
        self.processes.runs() || !self.subagents.is_empty() || self.tasks > 0
    }

    /// Its state went from `before` to `now`: its first turn's start is
    /// kept; a new turn forgets its tasks and its children (what ended
    /// brought it back, or its next turn's end lists what still runs).
    pub(crate) fn turned(&mut self, before: State, now: State) {
        if now != State::Working {
            return;
        }
        if self.first_turn.is_none() {
            self.first_turn = Some(background::now_ticks().0);
        }
        if before.ended_its_turn() {
            self.tasks = 0;
            self.processes = Found::default();
        }
    }
}

/// An agent whose children the worker reads, and Codex's `config.toml`,
/// where its MCP servers are configured.
#[derive(Debug)]
pub(crate) struct Job {
    watched: Watched,
    configured: Option<PathBuf>,
}

/// The worker reading the agents' children: what it is given to read goes
/// through the sender; what it found comes back as [`Heard::Running`].
pub(crate) fn start(heard: channel::Sender<Heard>) -> Option<mpsc::Sender<Vec<Job>>> {
    let (jobs, given) = mpsc::channel::<Vec<Job>>();
    match std::thread::Builder::new()
        .name("agents' children".to_owned())
        .spawn(move || work(&given, &heard))
    {
        Ok(_) => Some(jobs),
        Err(err) => {
            tracing::warn!(%err, "the agents' children are not followed");
            None
        }
    }
}

fn work(given: &mpsc::Receiver<Vec<Job>>, heard: &channel::Sender<Heard>) {
    let mut servers = Servers::default();
    while let Ok(mut jobs) = given.recv() {
        // What waited meanwhile, in one reading.
        while let Ok(more) = given.try_recv() {
            jobs.extend(more);
        }
        let procs = background::processes(Path::new(hocus_agents::process::PROC));
        let now = background::now_ticks();
        let mut found: HashMap<u64, Found> = HashMap::new();
        for job in jobs {
            let mut watched = job.watched;
            if let Some(configured) = &job.configured {
                watched.servers = servers.of(configured);
            }
            found.insert(watched.id, background::count(&procs, &watched, now));
        }
        if heard
            .send(Heard::Running(found.into_iter().collect()))
            .is_err()
        {
            return;
        }
    }
}

/// A file's time and length, as last read.
type Stamp = Option<(SystemTime, u64)>;
/// Its servers' command lines.
type Lines = Vec<Vec<String>>;

/// The MCP servers' command lines, by file, kept while it does not change.
#[derive(Default)]
struct Servers {
    read: HashMap<PathBuf, (Stamp, Lines)>,
}

impl Servers {
    fn of(&mut self, file: &Path) -> Lines {
        let stamp = std::fs::metadata(file)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        match self.read.get(file) {
            Some((kept, servers)) if *kept == stamp => servers.clone(),
            _ => {
                let servers = std::fs::read_to_string(file)
                    .map(|text| background::mcp_servers(&text))
                    .unwrap_or_default();
                self.read
                    .insert(file.to_path_buf(), (stamp, servers.clone()));
                servers
            }
        }
    }
}

impl Compositor {
    /// A hook said what runs on beside agent `ran`'s turns.
    pub(crate) fn agent_ran(&mut self, ran: Ran) {
        let key = (ran.source, ran.key.clone());
        let Some(agent) = self.agents.list.iter_mut().find(|a| {
            a.keys.contains(&key)
                || ran
                    .pid
                    .is_some_and(|p| a.pids.contains(&p) && a.keys.iter().any(|k| k.0 == key.0))
        }) else {
            tracing::debug!(?key, "what runs on, of an agent not known");
            return;
        };
        let running = &mut agent.running;
        match ran.running {
            Said::SubagentStarted(id) => {
                running.subagents.insert(id);
            }
            Said::SubagentStopped(id) => {
                running.subagents.remove(&id);
            }
            // All that is in flight, background sub-agents among them: a
            // sub-agent whose end was never said is not counted on.
            Said::Tasks(n) => {
                running.tasks = n;
                running.subagents.clear();
            }
        }
        tracing::debug!(
            id = agent.id,
            subagents = running.subagents.len(),
            tasks = running.tasks,
            "what an agent left running"
        );
        self.agents.dirty = true;
    }

    /// Agent `id`'s turn just ended: its children are read (Codex's).
    pub(crate) fn agent_turn_ended(&mut self, id: u64) {
        self.read_children(&[id]);
    }

    /// The agents among `ids` whose turn ended, of Codex's hooks, have
    /// their children read off the loop. Claude Code's say what they left
    /// running: its language servers, an MCP server's relay, a shell left
    /// over would read as work in `/proc`.
    fn read_children(&mut self, ids: &[u64]) {
        let Some(jobs) = &self.agents.children else {
            return;
        };
        let places = self.agents.places.as_ref();
        let batch: Vec<Job> = self
            .agents
            .list
            .iter()
            .filter(|a| ids.contains(&a.id) && a.state == State::Done)
            .filter(|a| {
                a.keys.iter().any(|k| k.0 == Source::Codex)
                    && !a.keys.iter().any(|k| k.0 == Source::ClaudeCode)
            })
            .filter_map(|a| {
                let pid = *a.pids.first()?;
                Some(Job {
                    watched: Watched {
                        id: a.id,
                        pid,
                        since: a.running.first_turn.unwrap_or(a.running.first_heard),
                        servers: Vec::new(),
                    },
                    configured: places.map(|p| p.codex_home.join("config.toml")),
                })
            })
            .collect();
        if !batch.is_empty() && jobs.send(batch).is_err() {
            tracing::warn!("the agents' children's worker is gone");
            self.agents.children = None;
        }
    }

    /// The worker found these agents' children: kept for those whose turn
    /// is still over; read again in [`EVERY`] while one may run.
    pub(crate) fn agents_children_found(&mut self, found: Vec<(u64, Found)>) {
        for (id, found) in found {
            let Some(agent) = self
                .agents
                .list
                .iter_mut()
                .find(|a| a.id == id && a.state == State::Done)
            else {
                continue;
            };
            if agent.running.processes != found {
                tracing::debug!(
                    id,
                    counted = found.counted,
                    young = found.young,
                    "an agent's children left running"
                );
                agent.running.processes = found;
                self.agents.dirty = true;
            }
        }
        self.children_timer();
    }

    /// The timer that reads the children again, while an agent's turn is
    /// over and one may run; none otherwise.
    fn children_timer(&mut self) {
        let again = self
            .agents
            .list
            .iter()
            .any(|a| a.state == State::Done && a.running.processes.again());
        match (again, self.agents.children_timer.is_some()) {
            (true, false) => {
                let timer =
                    self.loop_handle
                        .insert_source(Timer::from_duration(EVERY), |_, _, app| {
                            app.state.agents.children_timer = None;
                            let ids: Vec<u64> = app
                                .state
                                .agents
                                .list
                                .iter()
                                .filter(|a| a.state == State::Done && a.running.processes.again())
                                .map(|a| a.id)
                                .collect();
                            app.state.read_children(&ids);
                            TimeoutAction::Drop
                        });
                match timer {
                    Ok(token) => self.agents.children_timer = Some(token),
                    Err(err) => tracing::warn!(%err.error, "no timer for the agents' children"),
                }
            }
            (false, true) => {
                if let Some(token) = self.agents.children_timer.take() {
                    self.loop_handle.remove(token);
                }
            }
            _ => {}
        }
    }
}
