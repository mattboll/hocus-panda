//! herdr, a terminal workspace manager for coding agents: when it runs,
//! its socket (`$XDG_CONFIG_HOME/herdr/herdr.sock`, JSON lines; its
//! documentation's "Socket API") says which agents run in which panes and
//! what each is doing. Ported from the first build's client
//! (`v0-first-build`, `hocus-platform/src/herdr.rs`), without its polling.
//!
//! One thread: it waits for the socket with inotify, lists the agents
//! (`agent.list`), the workspaces' and tabs' labels (`workspace.list`,
//! `tab.list`) and each pane's foreground processes
//! (`pane.process_info`: the same agent seen by a hook is the same
//! process), then follows their status events and the renames
//! (`events.subscribe`) on a blocking read; panes coming or going list
//! them again. When herdr goes, it says so and waits for the socket again.
//!
//! An agent is named by its tab's own name when the user gave it one
//! ("refacto"), else by its workspace's label (#503); never by its kind
//! ("claude"), which herdr shows under it and which named every agent
//! alike (#497).
//!
//! Going to one of its agents selects its pane ([`select`], 4.3). Its
//! window is the terminal's that runs herdr's client: herdr's server is a
//! daemon, its agents run under it and no terminal, and it outlives its
//! clients; a client attached later is found by looking again
//! ([`terminal_processes`]), not only when the agents are listed (#500).
//! Which of herdr's panes is focused is followed too: herdr's window
//! getting the keyboard shows that pane's agent, not every other.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Value, json};

use crate::State;

/// A request's answer.
const ANSWER: Duration = Duration::from_secs(2);

/// An agent in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Its pane, which names it in herdr's requests.
    pub pane: String,
    /// Its workspace and its tab, whose labels name it.
    pub workspace: String,
    pub tab: String,
    /// Its tab's own name, else its workspace's label, when herdr gives
    /// one.
    pub name: Option<String>,
    /// Its directory's name (herdr's `cwd` for it, else its foreground's,
    /// else its processes'), for when herdr gives no name.
    pub folder: Option<String>,
    /// What it is doing; `None`: herdr does not know.
    pub state: Option<State>,
    /// Its pane's foreground processes.
    pub pids: Vec<u32>,
    /// Its pane is the one herdr shows focused: its window getting the
    /// keyboard shows this agent.
    pub focused: bool,
}

/// What the thread learns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// herdr runs: every agent, and for each of its clients the
    /// processes it runs under (where its terminal's window is), nearest
    /// first ([`terminal_processes`]).
    Agents {
        agents: Vec<Agent>,
        terminals: Vec<Vec<u32>>,
    },
    /// One agent's state changed.
    State { pane: String, state: Option<State> },
    /// herdr focused this pane.
    Focused(String),
    /// A workspace or a tab was renamed (or tabs came, went, moved): the
    /// agents' names now, by pane.
    Names(Vec<(String, Option<String>)>),
    /// herdr went away.
    Gone,
}

/// herdr's states as the desktop's.
pub fn state(status: &str) -> Option<State> {
    match status {
        "idle" => Some(State::Idle),
        "working" => Some(State::Working),
        "blocked" => Some(State::Waiting),
        "done" => Some(State::Done),
        _ => None,
    }
}

/// Follows herdr behind `socket` on a thread of its own, its clients
/// among the processes `proc` shows (`/proc`), telling `sink`; it ends
/// when `sink` says `false`.
pub fn start(
    socket: PathBuf,
    proc: PathBuf,
    sink: impl Fn(News) -> bool + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("herdr".to_owned())
        .spawn(move || follow(&socket, &proc, &sink))
        .map(drop)
}

fn follow(socket: &Path, proc: &Path, sink: &dyn Fn(News) -> bool) {
    let mut connected = false;
    loop {
        match session(socket, proc, sink, &mut connected) {
            // Panes came or went: listed again.
            Ok(true) => continue,
            Ok(false) => return,
            Err(err) => {
                if connected {
                    connected = false;
                    tracing::info!(%err, "herdr went");
                    if !sink(News::Gone) {
                        return;
                    }
                }
                if !wait_for(socket) {
                    return;
                }
            }
        }
    }
}

/// Waits until something happens to `socket`'s name (made again as herdr
/// starts), with inotify on its nearest directory: whether to go on.
fn wait_for(socket: &Path) -> bool {
    use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
    if UnixStream::connect(socket).is_ok() {
        return true;
    }
    let mut at = socket;
    let (dir, name) = loop {
        let Some(parent) = at.parent() else {
            return false;
        };
        if parent.is_dir() {
            break (parent, at.file_name().map(|n| n.to_owned()));
        }
        at = parent;
    };
    let Ok(fd) = inotify::init(CreateFlags::CLOEXEC) else {
        return false;
    };
    let flags = WatchFlags::CREATE | WatchFlags::MOVED_TO | WatchFlags::ATTRIB;
    if inotify::add_watch(&fd, dir, flags).is_err() {
        tracing::warn!(dir = %dir.display(), "cannot watch for herdr's socket");
        return false;
    }
    // Made between the look and the watch.
    if UnixStream::connect(socket).is_ok() {
        return true;
    }
    let mut buffer = [std::mem::MaybeUninit::uninit(); 1024];
    let mut events = inotify::Reader::new(&fd, &mut buffer);
    loop {
        match events.next() {
            Ok(event) => {
                let named = event.file_name().map(|n| {
                    <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(n.to_bytes())
                });
                if named.is_some_and(|n| Some(n) == name.as_deref()) {
                    // A directory on the way made: watched from it next.
                    return true;
                }
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return false,
        }
    }
}

/// One listing, then its events: `Ok(true)` when panes came or went,
/// `Ok(false)` when told to stop, `Err` when the connection ends.
fn session(
    socket: &Path,
    proc: &Path,
    sink: &dyn Fn(News) -> bool,
    connected: &mut bool,
) -> Result<bool, String> {
    let listed = call(socket, "agent.list", json!({}))?;
    let mut labels = Labels::of(socket);
    let mut agents = parse_agents(&listed, &labels);
    for agent in &mut agents {
        if let Ok(info) = call(
            socket,
            "pane.process_info",
            json!({ "pane_id": agent.pane }),
        ) {
            agent.pids = pids(&info);
        }
        if agent.folder.is_none() {
            agent.folder = agent
                .pids
                .iter()
                .rev()
                .find_map(|pid| crate::process::folder_in(proc, *pid));
        }
    }
    let panes: Vec<String> = agents.iter().map(|a| a.pane.clone()).collect();
    // Where each is, and its name, for the renames.
    let mut named: Vec<(String, String, String, Option<String>)> = agents
        .iter()
        .map(|a| {
            (
                a.pane.clone(),
                a.workspace.clone(),
                a.tab.clone(),
                a.name.clone(),
            )
        })
        .collect();
    *connected = true;
    if !sink(News::Agents {
        agents,
        terminals: terminal_processes(proc),
    }) {
        return Ok(false);
    }
    let stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(ANSWER))
        .map_err(|e| e.to_string())?;
    let request = json!({
        "id": "hocus-events",
        "method": "events.subscribe",
        "params": { "subscriptions": subscriptions(&panes) },
    });
    (&stream)
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    // Blocking: the thread sleeps until herdr says something.
    let mut reader = BufReader::new(&stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Err("herdr closed the connection".into()),
            Ok(_) => {
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                if let Some(err) = value.get("error") {
                    return Err(format!("herdr refused to subscribe: {err}"));
                }
                match event(&value) {
                    Some(Event::State { pane, state }) if panes.contains(&pane) => {
                        if !sink(News::State { pane, state }) {
                            return Ok(false);
                        }
                    }
                    Some(Event::State { .. } | Event::Panes) => return Ok(true),
                    Some(Event::Focused(pane)) => {
                        if !sink(News::Focused(pane)) {
                            return Ok(false);
                        }
                        continue;
                    }
                    Some(Event::Workspace { id, label }) => {
                        labels.workspaces.insert(id, label);
                    }
                    Some(Event::Tab { id, label }) => {
                        if let Some(tab) = labels.tabs.iter_mut().find(|t| t.id == id) {
                            tab.label = label;
                        }
                    }
                    Some(Event::Tabs) => labels = Labels::of(socket),
                    None => continue,
                }
                let mut changed = false;
                for (_, workspace, tab, name) in &mut named {
                    let now = labels.name(workspace, tab);
                    if *name != now {
                        *name = now;
                        changed = true;
                    }
                }
                if changed {
                    let names = named
                        .iter()
                        .map(|(pane, _, _, name)| (pane.clone(), name.clone()))
                        .collect();
                    if !sink(News::Names(names)) {
                        return Ok(false);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// What the subscription follows: each agent's state, panes coming and
/// going and the focus moving to another tab or workspace (then the
/// agents are listed again), the pane focused, and what renames them.
fn subscriptions(panes: &[String]) -> Vec<Value> {
    let mut all: Vec<Value> = [
        "pane.created",
        "pane.closed",
        "pane.exited",
        "pane.agent_detected",
        "pane.moved",
        "pane.focused",
        "tab.focused",
        "workspace.focused",
        "workspace.renamed",
        "workspace.updated",
        "tab.renamed",
        "tab.created",
        "tab.closed",
        "tab.moved",
    ]
    .iter()
    .map(|t| json!({ "type": t }))
    .collect();
    all.extend(
        panes
            .iter()
            .map(|p| json!({ "type": "pane.agent_status_changed", "pane_id": p })),
    );
    all
}

#[derive(Debug, PartialEq)]
enum Event {
    State {
        pane: String,
        state: Option<State>,
    },
    /// Panes came, went or got an agent, or another tab or workspace
    /// was focused (its pane focused with it).
    Panes,
    /// herdr focused a pane.
    Focused(String),
    /// A workspace's label, renamed or updated (herdr's own label follows
    /// its repository).
    Workspace {
        id: String,
        label: String,
    },
    /// A tab's label, renamed.
    Tab {
        id: String,
        label: String,
    },
    /// Tabs came, went or moved: whether a tab's label shows, and the
    /// numbers herdr labels its unnamed ones with, may change.
    Tabs,
}

/// An event line, whichever spelling herdr gives its name.
fn event(value: &Value) -> Option<Event> {
    let name = value.get("event")?.as_str()?.replace('.', "_");
    let data = value.get("data")?;
    let text = |v: &Value, key: &str| v.get(key)?.as_str().map(str::to_owned);
    match name.as_str() {
        "workspace_renamed" => Some(Event::Workspace {
            id: text(data, "workspace_id")?,
            label: text(data, "label")?,
        }),
        "workspace_updated" => {
            let workspace = data.get("workspace")?;
            Some(Event::Workspace {
                id: text(workspace, "workspace_id")?,
                label: text(workspace, "label")?,
            })
        }
        "tab_renamed" => Some(Event::Tab {
            id: text(data, "tab_id")?,
            label: text(data, "label")?,
        }),
        "tab_created" | "tab_closed" | "tab_moved" => Some(Event::Tabs),
        "pane_agent_status_changed" => Some(Event::State {
            pane: data.get("pane_id")?.as_str()?.to_owned(),
            state: state(data.get("agent_status")?.as_str()?),
        }),
        "pane_focused" => Some(Event::Focused(text(data, "pane_id")?)),
        "pane_created"
        | "pane_closed"
        | "pane_exited"
        | "pane_agent_detected"
        | "pane_moved"
        | "tab_focused"
        | "workspace_focused" => Some(Event::Panes),
        _ => None,
    }
}

/// herdr's workspaces' and tabs' labels, and which are focused.
#[derive(Debug, Default, PartialEq)]
struct Labels {
    workspaces: HashMap<String, String>,
    /// In herdr's order, each workspace's tabs first to last.
    tabs: Vec<Tab>,
    /// The workspace focused, when herdr lists them.
    focused: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Tab {
    id: String,
    workspace: String,
    label: String,
    focused: bool,
}

impl Labels {
    /// Asked of herdr; what it does not answer stays unknown (the agents
    /// are then named by their folder).
    fn of(socket: &Path) -> Self {
        let workspaces = call(socket, "workspace.list", json!({})).unwrap_or_default();
        let tabs = call(socket, "tab.list", json!({})).unwrap_or_default();
        Self::parse(&workspaces, &tabs)
    }

    fn parse(workspaces: &Value, tabs: &Value) -> Self {
        let text = |v: &Value, key: &str| v.get(key)?.as_str().map(str::to_owned);
        let list = |v: &Value, key: &str| {
            v.get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let focused = |v: &Value| v.get("focused").and_then(Value::as_bool) == Some(true);
        let workspaces = list(workspaces, "workspaces");
        Self {
            focused: workspaces
                .iter()
                .find(|w| focused(w))
                .and_then(|w| text(w, "workspace_id")),
            workspaces: workspaces
                .iter()
                .filter_map(|w| Some((text(w, "workspace_id")?, text(w, "label")?)))
                .collect(),
            tabs: list(tabs, "tabs")
                .iter()
                .filter_map(|t| {
                    Some(Tab {
                        id: text(t, "tab_id")?,
                        workspace: text(t, "workspace_id")?,
                        label: text(t, "label")?,
                        focused: focused(t),
                    })
                })
                .collect(),
        }
    }

    /// Whether `workspace`'s `tab` shows, as far as herdr says: a pane
    /// focused in a tab out of sight is not the one looked at.
    fn shows(&self, workspace: &str, tab: &str) -> bool {
        let workspace_shows = self.focused.as_deref().is_none_or(|w| w == workspace);
        let tab_shows = self
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .is_none_or(|t| t.focused);
        workspace_shows && tab_shows
    }

    /// The agent's name in `workspace`'s `tab`: the tab's own name when
    /// it was given one, else the workspace's label (#503). herdr labels
    /// an unnamed tab with its place in its workspace, from 1.
    fn name(&self, workspace: &str, tab: &str) -> Option<String> {
        let own = self
            .tabs
            .iter()
            .filter(|t| t.workspace == workspace)
            .enumerate()
            .find(|(_, t)| t.id == tab)
            .filter(|(at, t)| t.label != (at + 1).to_string())
            .map(|(_, t)| crate::name_text(&t.label))
            .filter(|t| !t.is_empty());
        own.or_else(|| {
            Some(crate::name_text(self.workspaces.get(workspace)?)).filter(|w| !w.is_empty())
        })
    }
}

fn parse_agents(result: &Value, labels: &Labels) -> Vec<Agent> {
    let text = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    result
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let pane = text(a, "pane_id")?;
            let status = a.get("agent_status")?.as_str()?;
            let workspace = text(a, "workspace_id").unwrap_or_default();
            let tab = text(a, "tab_id").unwrap_or_default();
            let folder = text(a, "cwd")
                .or_else(|| text(a, "foreground_cwd"))
                .and_then(|cwd| crate::folder(&cwd));
            let focused = a.get("focused").and_then(Value::as_bool) == Some(true)
                && labels.shows(&workspace, &tab);
            Some(Agent {
                name: labels.name(&workspace, &tab),
                focused,
                pane,
                workspace,
                tab,
                folder,
                state: state(status),
                pids: Vec::new(),
            })
        })
        .collect()
}

/// Every process id `pane.process_info` gives: the pane's shell and its
/// foreground processes, wherever it puts them.
fn pids(info: &Value) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack = vec![info];
    while let Some(v) = stack.pop() {
        match v {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "pid"
                        && let Some(pid) = value.as_u64().and_then(|p| u32::try_from(p).ok())
                        && !out.contains(&pid)
                    {
                        out.push(pid);
                    }
                    stack.push(value);
                }
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    out
}

/// herdr behind `socket` selects `pane` (`pane.focus`, in the API schema
/// herdr bundles: `herdr api schema`). Blocking, for a thread of its own.
pub fn select(socket: &Path, pane: &str) -> Result<(), String> {
    call(socket, "pane.focus", json!({ "pane_id": pane })).map(drop)
}

/// One request on a connection of its own: its result.
fn call(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    let stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(ANSWER))
        .and_then(|()| stream.set_read_timeout(Some(ANSWER)))
        .map_err(|e| e.to_string())?;
    let id = format!("hocus-{method}");
    let request = json!({ "id": id, "method": method, "params": params });
    (&stream)
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err(format!("{method}: no answer"));
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if value.get("id").and_then(Value::as_str) != Some(id.as_str()) {
            continue;
        }
        if let Some(err) = value.get("error") {
            let message = err.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(format!("{method}: {message}"));
        }
        return value
            .get("result")
            .cloned()
            .ok_or_else(|| format!("{method}: no result"));
    }
}

/// For each of herdr's clients, the processes it runs under, nearest
/// first: a terminal's window among them shows herdr. Its server, started
/// apart, is left out, and a client run by another (`herdr` commands in a
/// pane, under the server) leads to no terminal.
fn ancestors_of_clients(table: &HashMap<u32, (crate::process::Process, bool)>) -> Vec<Vec<u32>> {
    let mut clients: Vec<u32> = table
        .iter()
        .filter(|(_, (p, server))| p.name == "herdr" && !server)
        .map(|(pid, _)| *pid)
        .collect();
    clients.sort_unstable();
    clients
        .into_iter()
        .map(|client| {
            crate::process::ancestry_in(client, |pid| table.get(&pid).map(|(p, _)| p.clone()))
                .into_iter()
                .skip(1)
                .collect::<Vec<u32>>()
        })
        .filter(|above| !above.is_empty())
        .collect()
}

/// herdr's clients among the processes `proc` shows (`/proc`), each with
/// the processes above it ([`News::Agents`]'s `terminals`). It reads every
/// process: for a thread, not an event loop.
pub fn terminal_processes(proc: &Path) -> Vec<Vec<u32>> {
    let Ok(dir) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    let mut table = HashMap::new();
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(process) = crate::process::read_in(proc, pid) else {
            continue;
        };
        let server = process.name == "herdr"
            && std::fs::read(entry.path().join("cmdline"))
                .is_ok_and(|raw| raw.split(|b| *b == 0).nth(1) == Some(b"server".as_slice()));
        table.insert(pid, (process, server));
    }
    ancestors_of_clients(&table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Process;

    /// `workspace.list`, `tab.list` and `agent.list` as herdr answers them
    /// (its API schema, `herdr api schema`, protocol 19): two workspaces,
    /// the first with two tabs, the second named after its repository
    /// with one.
    fn listed() -> (Value, Value, Value) {
        let workspaces = json!({ "type": "workspace_list", "workspaces": [
            { "workspace_id": "w1", "number": 1, "label": "hocus-panda", "focused": true,
              "pane_count": 3, "tab_count": 2, "active_tab_id": "w1:t1", "agent_status": "working" },
            { "workspace_id": "w2", "number": 2, "label": "infra", "focused": false,
              "pane_count": 1, "tab_count": 1, "active_tab_id": "w2:t1", "agent_status": "idle" },
            { "workspace_id": "w3", "number": 3, "label": "site", "focused": false,
              "pane_count": 1, "tab_count": 1, "active_tab_id": "w3:t2", "agent_status": "idle" },
        ] });
        let tabs = json!({ "type": "tab_list", "tabs": [
            { "tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "refacto",
              "focused": true, "pane_count": 2, "agent_status": "working" },
            { "tab_id": "w1:t2", "workspace_id": "w1", "number": 2, "label": "2",
              "focused": false, "pane_count": 1, "agent_status": "idle" },
            { "tab_id": "w2:t1", "workspace_id": "w2", "number": 1, "label": "1",
              "focused": false, "pane_count": 1, "agent_status": "idle" },
            // Its first tab closed: the one left is labelled "1", its
            // number 2.
            { "tab_id": "w3:t2", "workspace_id": "w3", "number": 2, "label": "1",
              "focused": false, "pane_count": 1, "agent_status": "idle" },
        ] });
        // herdr's focused pane in each tab; the one shown, w1:p1.
        let agent = |pane: &str, tab: &str, status: &str, cwd: Value| {
            let workspace = &tab[..2];
            let focused = ["w1:p1", "w1:p3", "w2:p1"].contains(&pane);
            json!({ "pane_id": pane, "terminal_id": format!("t-{pane}"), "workspace_id": workspace,
              "tab_id": tab, "focused": focused, "revision": 3, "agent_status": status,
              "agent": "claude", "display_agent": null, "name": null, "title": "✳ Claude Code",
              "cwd": cwd, "foreground_cwd": null, "interactive_ready": true,
              "launch_pending": false, "screen_detection_skipped": false,
              "state_change_seq": 7, "state_labels": {}, "tokens": {} })
        };
        let agents = json!({ "type": "agent_list", "agents": [
            agent("w1:p1", "w1:t1", "blocked", json!("/home/alex/hocus-panda")),
            agent("w1:p3", "w1:t2", "working", json!("/home/alex/hocus-panda")),
            agent("w2:p1", "w2:t1", "idle", json!("/home/alex/infra")),
            agent("w3:p1", "w3:t2", "done", json!(null)),
            agent("w9:p1", "w9:t1", "unknown", json!("/home/alex/notes")),
            { "agent_status": "idle" },
        ] });
        (workspaces, tabs, agents)
    }

    #[test]
    fn agents_are_named_by_their_tab_else_their_workspace_never_by_their_kind() {
        let (workspaces, tabs, listed) = listed();
        let labels = Labels::parse(&workspaces, &tabs);
        let agents = parse_agents(&listed, &labels);
        let seen: Vec<_> = agents
            .iter()
            .map(|a| {
                (
                    a.pane.as_str(),
                    a.name.as_deref(),
                    a.folder.as_deref(),
                    a.state,
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                // A named tab: its name only.
                (
                    "w1:p1",
                    Some("refacto"),
                    Some("hocus-panda"),
                    Some(State::Waiting)
                ),
                // An unnamed one, among two: the workspace's label.
                (
                    "w1:p3",
                    Some("hocus-panda"),
                    Some("hocus-panda"),
                    Some(State::Working)
                ),
                // Alone and unnamed: the workspace's label.
                ("w2:p1", Some("infra"), Some("infra"), Some(State::Idle)),
                // Unnamed, labelled by its place, not its number.
                ("w3:p1", Some("site"), None, Some(State::Done)),
                // A workspace herdr did not list: no name from herdr, its
                // folder.
                ("w9:p1", None, Some("notes"), None),
            ]
        );
        // The pane looked at: focused in the tab and the workspace shown,
        // not in a tab or a workspace out of sight.
        let focused: Vec<&str> = agents
            .iter()
            .filter(|a| a.focused)
            .map(|a| a.pane.as_str())
            .collect();
        assert_eq!(focused, ["w1:p1"]);
        // An older herdr, without the lists: its folder only, and herdr's
        // focus as it says it.
        let bare = parse_agents(&listed, &Labels::default());
        assert!(bare.iter().all(|a| a.name.is_none()), "{bare:?}");
        assert_eq!(bare.iter().filter(|a| a.focused).count(), 3);
        assert!(agents.iter().all(|a| a.name.as_deref() != Some("claude")));
    }

    #[test]
    fn renames_in_herdr_are_heard() {
        let (workspaces, tabs, _) = listed();
        let mut labels = Labels::parse(&workspaces, &tabs);
        let renamed = json!({ "event": "tab_renamed",
            "data": { "type": "tab_renamed", "tab_id": "w2:t1", "workspace_id": "w2", "label": "deploy" } });
        let Some(Event::Tab { id, label }) = event(&renamed) else {
            panic!("{renamed}");
        };
        if let Some(tab) = labels.tabs.iter_mut().find(|t| t.id == id) {
            tab.label = label;
        }
        assert_eq!(labels.name("w2", "w2:t1").as_deref(), Some("deploy"));
        let workspace = json!({ "event": "workspace.renamed",
            "data": { "type": "workspace_renamed", "workspace_id": "w2", "label": "ops" } });
        assert_eq!(
            event(&workspace),
            Some(Event::Workspace {
                id: "w2".into(),
                label: "ops".into()
            })
        );
        let updated = json!({ "event": "workspace_updated", "data": { "type": "workspace_updated",
            "workspace": { "workspace_id": "w3", "number": 3, "label": "site-web", "focused": false,
              "pane_count": 1, "tab_count": 1, "active_tab_id": "w3:t2", "agent_status": "idle" } } });
        assert_eq!(
            event(&updated),
            Some(Event::Workspace {
                id: "w3".into(),
                label: "site-web".into()
            })
        );
        for kind in ["tab_created", "tab.closed", "tab_moved"] {
            let tabs = json!({ "event": kind, "data": { "type": kind } });
            assert_eq!(event(&tabs), Some(Event::Tabs), "{kind}");
        }
        let subscribed = subscriptions(&[]);
        for kind in [
            "tab.renamed",
            "workspace.renamed",
            "workspace.updated",
            "tab.created",
        ] {
            assert!(subscribed.contains(&json!({ "type": kind })), "{kind}");
        }
    }

    #[test]
    fn events_read_in_both_spellings_and_processes_wherever_they_are() {
        let dotted = json!({ "event": "pane.agent_status_changed",
            "data": { "pane_id": "w1:p1", "workspace_id": "w1", "agent_status": "done" } });
        assert_eq!(
            event(&dotted),
            Some(Event::State {
                pane: "w1:p1".into(),
                state: Some(State::Done)
            })
        );
        let closed = json!({ "event": "pane_closed", "data": { "pane_id": "w1:p2" } });
        assert_eq!(event(&closed), Some(Event::Panes));
        // The focus: a pane's said, another tab's or workspace's listed
        // again (the pane focused there is herdr's to say).
        let focused = json!({ "event": "pane.focused",
            "data": { "type": "pane_focused", "pane_id": "w1:p3", "workspace_id": "w1" } });
        assert_eq!(event(&focused), Some(Event::Focused("w1:p3".into())));
        let tab = json!({ "event": "tab_focused",
            "data": { "type": "tab_focused", "tab_id": "w1:t2", "workspace_id": "w1" } });
        assert_eq!(event(&tab), Some(Event::Panes));
        assert_eq!(
            event(&json!({ "event": "layout_updated", "data": {} })),
            None
        );
        let info = json!({ "type": "pane_process_info", "shell": { "pid": 300 },
            "foreground_pgid": 400,
            "foreground": [ { "pid": 400, "name": "claude", "argv": ["claude"] } ] });
        let mut found = pids(&info);
        found.sort_unstable();
        assert_eq!(found, [300, 400]);
        assert!(
            subscriptions(&["w1:p1".into()])
                .contains(&json!({ "type": "pane.agent_status_changed", "pane_id": "w1:p1" }))
        );
    }

    /// The maintainer's tree (#500): the agents under herdr's server, a
    /// daemon; herdr's client in a shell of the terminal's.
    #[test]
    fn a_terminal_is_found_above_herdrs_client_not_its_server() {
        let p = |name: &str, parent| Process {
            name: name.into(),
            parent,
        };
        let mut table = HashMap::from([
            (100, (p("kitty", 1), false)),
            (200, (p("zsh", 100), false)),
            (300, (p("herdr", 200), false)),
            (400, (p("herdr", 1), true)),
            (410, (p("zsh", 400), false)),
            (500, (p("claude", 410), false)),
            // A `herdr` command an agent runs in its pane: under the
            // server, no terminal.
            (510, (p("herdr", 500), false)),
        ]);
        assert_eq!(
            ancestors_of_clients(&table),
            vec![vec![200, 100], vec![500, 410, 400]]
        );
        // A second client, in another terminal: each its own.
        table.insert(700, (p("foot", 1), false));
        table.insert(800, (p("herdr", 700), false));
        assert_eq!(ancestors_of_clients(&table)[2], vec![700]);
        // Its server alone, before any client attached: none.
        let server = HashMap::from([(400, (p("herdr", 1), true))]);
        assert!(ancestors_of_clients(&server).is_empty());
    }

    /// The table read from a `/proc` of the test's own: its server known
    /// by its command line, never the machine's processes (#494).
    #[test]
    fn herdrs_clients_are_read_in_the_proc_given() -> std::io::Result<()> {
        let proc = tempfile::tempdir()?;
        let process = |pid: u32, stat: &str, cmdline: &[u8]| -> std::io::Result<()> {
            let dir = proc.path().join(pid.to_string());
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("stat"), stat)?;
            std::fs::write(dir.join("cmdline"), cmdline)
        };
        process(100, "100 (kitty) S 1 100 100 0", b"kitty\0")?;
        process(200, "200 (zsh) S 100 200 200 0", b"/usr/bin/zsh\0")?;
        process(300, "300 (herdr) S 200 300 200 0", b"herdr\0")?;
        process(
            400,
            "400 (herdr) S 1 400 400 0",
            b"/home/alex/.local/bin/herdr\0server\0",
        )?;
        process(500, "500 (claude) S 400 500 400 0", b"claude\0")?;
        assert_eq!(terminal_processes(proc.path()), vec![vec![200, 100]]);
        assert!(terminal_processes(&proc.path().join("none")).is_empty());
        Ok(())
    }
}
