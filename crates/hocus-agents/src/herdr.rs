//! herdr, a terminal workspace manager for coding agents: when it runs,
//! its socket (`$XDG_CONFIG_HOME/herdr/herdr.sock`, JSON lines; its
//! documentation's "Socket API") says which agents run in which panes and
//! what each is doing. Ported from the first build's client
//! (`v0-first-build`, `hocus-platform/src/herdr.rs`), without its polling.
//!
//! One thread: it waits for the socket with inotify, lists the agents
//! (`agent.list`) and each pane's foreground processes
//! (`pane.process_info`: the same agent seen by a hook is the same
//! process), then follows their status events (`events.subscribe`) on a
//! blocking read; panes coming or going list them again. When herdr goes,
//! it says so and waits for the socket again.
//!
//! Going to one of its agents selects its pane ([`select`], 4.3).

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
    pub name: String,
    /// What it is doing; `None`: herdr does not know.
    pub state: Option<State>,
    /// Its pane's foreground processes.
    pub pids: Vec<u32>,
}

/// What the thread learns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// herdr runs: every agent, and the processes its clients run under
    /// (where its terminal's window is), nearest first.
    Agents {
        agents: Vec<Agent>,
        terminals: Vec<u32>,
    },
    /// One agent's state changed.
    State { pane: String, state: Option<State> },
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

/// Follows herdr behind `socket` on a thread of its own, telling `sink`;
/// it ends when `sink` says `false`.
pub fn start(socket: PathBuf, sink: impl Fn(News) -> bool + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("herdr".to_owned())
        .spawn(move || follow(&socket, &sink))
        .map(drop)
}

fn follow(socket: &Path, sink: &dyn Fn(News) -> bool) {
    let mut connected = false;
    loop {
        match session(socket, sink, &mut connected) {
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
    sink: &dyn Fn(News) -> bool,
    connected: &mut bool,
) -> Result<bool, String> {
    let listed = call(socket, "agent.list", json!({}))?;
    let mut agents = parse_agents(&listed);
    for agent in &mut agents {
        if let Ok(info) = call(
            socket,
            "pane.process_info",
            json!({ "pane_id": agent.pane }),
        ) {
            agent.pids = pids(&info);
        }
    }
    let panes: Vec<String> = agents.iter().map(|a| a.pane.clone()).collect();
    *connected = true;
    if !sink(News::Agents {
        agents,
        terminals: terminal_processes(),
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
                    None => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// What the subscription follows: each agent's state, and panes coming
/// and going (then the agents are listed again).
fn subscriptions(panes: &[String]) -> Vec<Value> {
    let mut all: Vec<Value> = [
        "pane.created",
        "pane.closed",
        "pane.exited",
        "pane.agent_detected",
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
    /// Panes came, went or got an agent.
    Panes,
}

/// An event line, whichever spelling herdr gives its name.
fn event(value: &Value) -> Option<Event> {
    let name = value.get("event")?.as_str()?.replace('.', "_");
    let data = value.get("data")?;
    match name.as_str() {
        "pane_agent_status_changed" => Some(Event::State {
            pane: data.get("pane_id")?.as_str()?.to_owned(),
            state: state(data.get("agent_status")?.as_str()?),
        }),
        "pane_created" | "pane_closed" | "pane_exited" | "pane_agent_detected" | "pane_moved" => {
            Some(Event::Panes)
        }
        _ => None,
    }
}

fn parse_agents(result: &Value) -> Vec<Agent> {
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
            let name = text(a, "name")
                .or_else(|| text(a, "display_agent"))
                .or_else(|| text(a, "agent"))
                .unwrap_or_else(|| pane.clone());
            Some(Agent {
                pane,
                name,
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

/// The processes herdr's clients run under, nearest first: a terminal's
/// window among them shows herdr. Its server, started apart, is left out.
fn ancestors_of_clients(table: &HashMap<u32, (crate::process::Process, bool)>) -> Vec<u32> {
    let mut clients: Vec<u32> = table
        .iter()
        .filter(|(_, (p, server))| p.name == "herdr" && !server)
        .map(|(pid, _)| *pid)
        .collect();
    clients.sort_unstable();
    let mut out = Vec::new();
    for client in clients {
        let above =
            crate::process::ancestry_in(client, |pid| table.get(&pid).map(|(p, _)| p.clone()));
        for pid in above.into_iter().skip(1) {
            if !out.contains(&pid) {
                out.push(pid);
            }
        }
    }
    out
}

fn terminal_processes() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
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
        let Some(process) = crate::process::read(pid) else {
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

    #[test]
    fn agents_are_named_by_their_name_else_their_kind() {
        let listed = json!({
            "type": "agent_list",
            "agents": [
                { "pane_id": "w1:p1", "terminal_id": "t1", "workspace_id": "w1", "tab_id": "w1:t1",
                  "focused": false, "revision": 3, "agent_status": "blocked", "name": "refacto",
                  "agent": "claude" },
                { "pane_id": "w1:p2", "agent_status": "working", "agent": "codex", "name": null },
                { "pane_id": "w1:p3", "agent_status": "unknown" },
                { "agent_status": "idle" },
            ]
        });
        let agents = parse_agents(&listed);
        let seen: Vec<(&str, &str, Option<State>)> = agents
            .iter()
            .map(|a| (a.pane.as_str(), a.name.as_str(), a.state))
            .collect();
        assert_eq!(
            seen,
            [
                ("w1:p1", "refacto", Some(State::Waiting)),
                ("w1:p2", "codex", Some(State::Working)),
                ("w1:p3", "w1:p3", None),
            ]
        );
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
        assert_eq!(event(&json!({ "event": "tab_focused", "data": {} })), None);
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

    #[test]
    fn a_terminal_is_found_above_herdrs_client_not_its_server() {
        let p = |name: &str, parent| Process {
            name: name.into(),
            parent,
        };
        let table = HashMap::from([
            (100, (p("kitty", 1), false)),
            (200, (p("zsh", 100), false)),
            (300, (p("herdr", 200), false)),
            (400, (p("herdr", 1), true)),
            (500, (p("claude", 400), false)),
        ]);
        assert_eq!(ancestors_of_clients(&table), vec![200, 100]);
    }
}
