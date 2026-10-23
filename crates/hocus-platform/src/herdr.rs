//! herdr (#41), a terminal workspace manager for coding agents: its API
//! socket (JSON lines, protocol 19) says which agents run in which panes and
//! what each is doing; focusing one, answering it and handing it a prompt
//! (#197) go back the same way.
//!
//! One thread lists the agents and follows their status events; another
//! carries out what the user asks, each on a connection of its own. Neither
//! ever blocks the compositor: what they learn goes out through a sink, and
//! a socket that is not there is waited for quietly.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, ErrorKind, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use serde_json::{Value, json};

/// Between looks for a socket that is not there, or after herdr went.
const RETRY: Duration = Duration::from_secs(5);
/// How long a read waits before the thread looks whether it should stop.
const POLL: Duration = Duration::from_millis(500);
/// The list is read again this often, in case an event was missed.
const RESYNC: Duration = Duration::from_secs(60);
/// A request's answer.
const ANSWER: Duration = Duration::from_secs(2);

/// What an agent is doing, as herdr says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Ready for input, and seen.
    Idle,
    Working,
    /// An approval or a question on its screen: it waits for the user.
    Blocked,
    /// Idle after work nobody has looked at yet.
    Done,
    Unknown,
}

impl Status {
    fn parse(s: &str) -> Self {
        match s {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            _ => Self::Unknown,
        }
    }
}

/// An agent in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Its pane, which names it in requests.
    pub pane: String,
    pub name: String,
    pub status: Status,
    /// What its integration said with this status (the question, while it
    /// waits), when it said something.
    pub label: Option<String>,
}

/// What the threads learn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// Every agent, in herdr's order: on connecting, and when panes come
    /// or go.
    Agents(Vec<Agent>),
    /// One agent's status changed.
    Status {
        pane: String,
        status: Status,
        label: Option<String>,
    },
    /// herdr went away: no agents until it comes back.
    Gone,
    /// After a focus: the processes herdr's client runs under, nearest
    /// first, to find its terminal's window.
    Terminal(Vec<u32>),
}

/// What the user asked of an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    Focus(String),
    Keys(String, Vec<String>),
    Prompt(String, String),
}

/// herdr's default socket, from the environment's values.
pub fn default_socket(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    match xdg_config_home.filter(|d| !d.is_empty()) {
        Some(dir) => Some(Path::new(dir).join("herdr/herdr.sock")),
        None => home
            .filter(|h| !h.is_empty())
            .map(|h| Path::new(h).join(".config/herdr/herdr.sock")),
    }
}

/// A connection to herdr: its threads stop when it is dropped, without
/// being waited for.
#[derive(Debug)]
pub struct Herdr {
    asks: mpsc::Sender<Ask>,
    stop: Arc<AtomicBool>,
}

impl Herdr {
    /// Follows the agents behind `socket`, telling `sink`.
    pub fn start(
        socket: PathBuf,
        sink: impl Fn(News) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let sink = Arc::new(sink);
        let stop = Arc::new(AtomicBool::new(false));
        let (asks, asked) = mpsc::channel();
        std::thread::Builder::new()
            .name("herdr events".into())
            .spawn({
                let (socket, sink, stop) = (socket.clone(), sink.clone(), stop.clone());
                move || follow(&socket, &*sink, &stop)
            })?;
        let asking = std::thread::Builder::new()
            .name("herdr asks".into())
            .spawn(move || {
                // Ends when the handle is dropped.
                while let Ok(ask) = asked.recv() {
                    carry_out(&socket, ask, &*sink);
                }
            });
        if let Err(err) = asking {
            // The other thread does not outlive a client that never was.
            stop.store(true, Ordering::Relaxed);
            return Err(err);
        }
        Ok(Self { asks, stop })
    }

    /// Shows the agent in herdr; its terminal's processes follow.
    pub fn focus(&self, pane: &str) {
        let _ = self.asks.send(Ask::Focus(pane.to_owned()));
    }

    /// Presses keys in the agent's terminal (`enter`, `esc`…), as herdr
    /// names them.
    pub fn keys(&self, pane: &str, keys: &[&str]) {
        let keys = keys.iter().map(|k| (*k).to_owned()).collect();
        let _ = self.asks.send(Ask::Keys(pane.to_owned(), keys));
    }

    /// Submits `text` to the agent as a prompt, as `herdr agent prompt`
    /// does: typed into its input and sent.
    pub fn prompt(&self, pane: &str, text: &str) {
        let _ = self
            .asks
            .send(Ask::Prompt(pane.to_owned(), text.to_owned()));
    }
}

impl Drop for Herdr {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn carry_out(socket: &Path, ask: Ask, sink: &dyn Fn(News)) {
    let (method, params) = match &ask {
        Ask::Focus(pane) => ("agent.focus", json!({ "target": pane })),
        Ask::Keys(pane, keys) => ("agent.send_keys", json!({ "target": pane, "keys": keys })),
        Ask::Prompt(pane, text) => ("agent.prompt", json!({ "target": pane, "text": text })),
    };
    if let Err(err) = call(socket, method, params) {
        tracing::warn!(method, %err, "herdr did not do it");
        return;
    }
    if matches!(ask, Ask::Focus(_)) {
        let processes = terminal_processes();
        if !processes.is_empty() {
            sink(News::Terminal(processes));
        }
    }
}

/// Sleeps `d` in short steps; whether to stop.
fn pause(d: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(POLL.min(until.saturating_duration_since(Instant::now())));
    }
    stop.load(Ordering::Relaxed)
}

/// Lists and follows the agents until told to stop, through herdr's
/// absences.
fn follow(socket: &Path, sink: &dyn Fn(News), stop: &AtomicBool) {
    let mut connected = false;
    while !stop.load(Ordering::Relaxed) {
        let outcome = if socket.exists() {
            session(socket, sink, stop, &mut connected)
        } else {
            Err(String::new())
        };
        match outcome {
            // Panes came or went: listed again at once.
            Ok(()) => continue,
            Err(err) => {
                if connected {
                    sink(News::Gone);
                    connected = false;
                }
                // Not running is no error; anything else is said once.
                if !err.is_empty() {
                    tracing::debug!(%err, "herdr is not reachable");
                }
                if pause(RETRY, stop) {
                    return;
                }
            }
        }
    }
}

/// One listing, then its events until panes come or go (`Ok`) or the
/// connection ends (`Err`).
fn session(
    socket: &Path,
    sink: &dyn Fn(News),
    stop: &AtomicBool,
    connected: &mut bool,
) -> Result<(), String> {
    let listed = call(socket, "agent.list", json!({}))?;
    let agents = parse_agents(&listed);
    let panes: Vec<String> = agents.iter().map(|a| a.pane.clone()).collect();
    sink(News::Agents(agents));
    *connected = true;

    let stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(ANSWER))
        .and_then(|()| stream.set_read_timeout(Some(POLL)))
        .map_err(|e| e.to_string())?;
    let request = json!({
        "id": "hocus-events",
        "method": "events.subscribe",
        "params": { "subscriptions": subscriptions(&panes) },
    });
    (&stream)
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&stream);
    let mut line = Vec::new();
    let since = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) || since.elapsed() > RESYNC {
            return Ok(());
        }
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Err("herdr closed the connection".into()),
            Ok(_) if line.ends_with(b"\n") => {
                let text = std::mem::take(&mut line);
                let Ok(value) = serde_json::from_slice::<Value>(&text) else {
                    continue;
                };
                if let Some(err) = value.get("error") {
                    return Err(format!("herdr refused to subscribe: {err}"));
                }
                match event(&value) {
                    Some(Event::Status {
                        pane,
                        status,
                        label,
                    }) if panes.contains(&pane) => sink(News::Status {
                        pane,
                        status,
                        label,
                    }),
                    Some(Event::Status { .. } | Event::Panes) => return Ok(()),
                    None => {}
                }
            }
            // Part of a line: the rest comes.
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// What the subscription follows: each agent's status, and panes coming
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
    Status {
        pane: String,
        status: Status,
        label: Option<String>,
    },
    /// Panes came, went or got an agent.
    Panes,
}

/// An event line, whichever spelling herdr gives its name
/// (`pane.agent_status_changed` or `pane_agent_status_changed`).
fn event(value: &Value) -> Option<Event> {
    let name = value.get("event")?.as_str()?.replace('.', "_");
    let data = value.get("data")?;
    match name.as_str() {
        "pane_agent_status_changed" => {
            let status = Status::parse(data.get("agent_status")?.as_str()?);
            Some(Event::Status {
                pane: data.get("pane_id")?.as_str()?.to_owned(),
                status,
                label: label(data, status),
            })
        }
        "pane_created" | "pane_closed" | "pane_exited" | "pane_agent_detected" | "pane_moved" => {
            Some(Event::Panes)
        }
        _ => None,
    }
}

/// What the integration said with this status.
fn label(data: &Value, status: Status) -> Option<String> {
    let key = match status {
        Status::Idle => "idle",
        Status::Working => "working",
        Status::Blocked => "blocked",
        Status::Done => "done",
        Status::Unknown => "unknown",
    };
    data.get("state_labels")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
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
            let status = Status::parse(a.get("agent_status")?.as_str()?);
            let name = text(a, "name")
                .or_else(|| text(a, "display_agent"))
                .or_else(|| text(a, "agent"))
                .unwrap_or_else(|| pane.clone());
            Some(Agent {
                pane,
                name,
                status,
                label: label(a, status),
            })
        })
        .collect()
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One request on a connection of its own: its result, or what went wrong.
fn call(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    let stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(ANSWER))
        .and_then(|()| stream.set_read_timeout(Some(ANSWER)))
        .map_err(|e| e.to_string())?;
    let id = format!("hocus-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let request = json!({ "id": id, "method": method, "params": params });
    (&stream)
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + ANSWER;
    let mut reader = BufReader::new(&stream);
    let mut line = Vec::new();
    while Instant::now() < deadline {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
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
    Err(format!("{method}: no answer"))
}

/// A process: its name, parent and arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    name: String,
    parent: u32,
    args: Vec<String>,
}

/// The processes herdr's clients run under, nearest first: a terminal's
/// window among them is the one showing herdr. Its server, started apart,
/// is left out.
fn ancestors_of_clients(table: &HashMap<u32, Process>) -> Vec<u32> {
    let mut clients: Vec<u32> = table
        .iter()
        .filter(|(_, p)| p.name == "herdr" && p.args.get(1).is_none_or(|a| a != "server"))
        .map(|(pid, _)| *pid)
        .collect();
    clients.sort_unstable();
    let mut out = Vec::new();
    for client in clients {
        let mut at = client;
        // A loop in the table cannot hold the thread.
        for _ in 0..64 {
            let Some(p) = table.get(&at) else {
                break;
            };
            if p.parent <= 1 || out.contains(&p.parent) {
                break;
            }
            out.push(p.parent);
            at = p.parent;
        }
    }
    out
}

#[cfg(target_os = "linux")]
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
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `pid (name) state ppid …`; the name may hold spaces and brackets.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let name = stat.get(open + 1..close).unwrap_or("").to_owned();
        let parent = stat
            .get(close + 1..)
            .and_then(|rest| rest.split_whitespace().nth(1))
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        let args = if name == "herdr" {
            std::fs::read(entry.path().join("cmdline"))
                .map(|raw| {
                    raw.split(|b| *b == 0)
                        .map(|a| String::from_utf8_lossy(a).into_owned())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        table.insert(pid, Process { name, parent, args });
    }
    ancestors_of_clients(&table)
}

#[cfg(not(target_os = "linux"))]
fn terminal_processes() -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_socket_follows_xdg_then_home() {
        assert_eq!(
            default_socket(Some("/x/config"), Some("/home/alex")),
            Some(PathBuf::from("/x/config/herdr/herdr.sock"))
        );
        assert_eq!(
            default_socket(Some(""), Some("/home/alex")),
            Some(PathBuf::from("/home/alex/.config/herdr/herdr.sock"))
        );
        assert_eq!(default_socket(None, None), None);
    }

    #[test]
    fn agents_are_named_by_their_name_else_their_kind() {
        let listed = json!({
            "type": "agent_list",
            "agents": [
                { "pane_id": "w1:p1", "terminal_id": "t1", "workspace_id": "w1", "tab_id": "w1:t1",
                  "focused": false, "revision": 3, "agent_status": "blocked", "name": "refacto",
                  "agent": "claude", "state_labels": { "blocked": "Push the branch?" } },
                { "pane_id": "w1:p2", "agent_status": "working", "agent": "codex", "name": null },
                { "pane_id": "w1:p3", "agent_status": "idle" },
                { "agent_status": "idle" },
            ]
        });
        let agents = parse_agents(&listed);
        assert_eq!(
            agents,
            vec![
                Agent {
                    pane: "w1:p1".into(),
                    name: "refacto".into(),
                    status: Status::Blocked,
                    label: Some("Push the branch?".into()),
                },
                Agent {
                    pane: "w1:p2".into(),
                    name: "codex".into(),
                    status: Status::Working,
                    label: None,
                },
                Agent {
                    pane: "w1:p3".into(),
                    name: "w1:p3".into(),
                    status: Status::Idle,
                    label: None,
                },
            ]
        );
    }

    #[test]
    fn status_events_are_read_in_both_spellings() {
        let dotted = json!({ "event": "pane.agent_status_changed",
            "data": { "pane_id": "w1:p1", "workspace_id": "w1", "agent_status": "done" } });
        assert_eq!(
            event(&dotted),
            Some(Event::Status {
                pane: "w1:p1".into(),
                status: Status::Done,
                label: None
            })
        );
        let snake = json!({ "event": "pane_agent_status_changed",
            "data": { "type": "pane_agent_status_changed", "pane_id": "w1:p2",
                      "workspace_id": "w1", "agent_status": "weird" } });
        assert!(matches!(
            event(&snake),
            Some(Event::Status {
                status: Status::Unknown,
                ..
            })
        ));
        let closed = json!({ "event": "pane_closed", "data": { "pane_id": "w1:p2" } });
        assert_eq!(event(&closed), Some(Event::Panes));
        let other = json!({ "event": "tab_focused", "data": {} });
        assert_eq!(event(&other), None);
        assert_eq!(event(&json!({ "id": "x", "result": {} })), None);
    }

    #[test]
    fn statuses_are_followed_pane_by_pane() {
        let subs = subscriptions(&["w1:p1".into()]);
        assert!(subs.contains(&json!({ "type": "pane.agent_detected" })));
        assert!(subs.contains(&json!({ "type": "pane.agent_status_changed", "pane_id": "w1:p1" })));
    }

    #[test]
    fn a_terminal_is_found_above_herdrs_client_not_its_server() {
        let p = |name: &str, parent, args: &[&str]| Process {
            name: name.into(),
            parent,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        let table = HashMap::from([
            (1, p("systemd", 0, &[])),
            (100, p("kitty", 1, &[])),
            (200, p("zsh", 100, &[])),
            (300, p("herdr", 200, &["herdr"])),
            (400, p("herdr", 1, &["herdr", "server"])),
            (500, p("claude", 400, &[])),
        ]);
        assert_eq!(ancestors_of_clients(&table), vec![200, 100]);
    }

    #[test]
    fn a_missing_socket_is_waited_for_quietly() {
        let dir = tempfile::tempdir().expect("a directory");
        let (sender, received) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        let herdr = Herdr::start(dir.path().join("herdr.sock"), move |news| {
            let _ = sender.lock().map(|s| s.send(news));
        })
        .expect("its threads");
        herdr.focus("w1:p1");
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err());
        drop(herdr);
    }
}
