//! A herdr for tests, never the user's: its API socket (JSON lines, protocol
//! 19) serves agents the scenario sets, sends their status changes to the
//! connections that subscribed, and keeps what it was asked to be checked.

use std::{
    io::{BufRead, BufReader, ErrorKind, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct Agent {
    pane: String,
    name: String,
    status: String,
    label: Option<String>,
}

impl Agent {
    fn info(&self) -> Value {
        let mut labels = serde_json::Map::new();
        if let Some(label) = &self.label {
            labels.insert(self.status.clone(), json!(label));
        }
        json!({
            "pane_id": self.pane,
            "terminal_id": format!("term-{}", self.pane),
            "workspace_id": "w1",
            "tab_id": "w1:t1",
            "focused": false,
            "revision": 1,
            "agent_status": self.status,
            "name": self.name,
            "agent": "claude",
            "display_agent": "Claude",
            "state_labels": labels,
        })
    }
}

#[derive(Debug, Default)]
struct Shared {
    agents: Vec<Agent>,
    asked: Vec<String>,
    /// Connections that subscribed, with the panes whose status they follow.
    subscribers: Vec<(UnixStream, Vec<String>)>,
}

#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Serves `agents` (name and status: `working`, `blocked`, `idle`,
    /// `done`) on `herdr.sock` in `dir`; their panes are `w1:p1`, `w1:p2`…
    pub fn start(dir: &Path, agents: &[(String, String)]) -> std::io::Result<Self> {
        let path = dir.join("herdr.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Mutex::new(Shared {
            agents: agents
                .iter()
                .enumerate()
                .map(|(i, (name, status))| Agent {
                    pane: format!("w1:p{}", i + 1),
                    name: name.clone(),
                    status: status.clone(),
                    label: None,
                })
                .collect(),
            ..Shared::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("herdr mock".into())
            .spawn({
                let (shared, stop) = (shared.clone(), stop.clone());
                move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let (shared, stop) = (shared.clone(), stop.clone());
                                let _ = std::thread::Builder::new()
                                    .name("herdr mock connection".into())
                                    .spawn(move || serve(stream, &shared, &stop));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                }
            })?;
        Ok(Self {
            path,
            shared,
            stop,
            thread: Some(thread),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a connection follows this agent's status.
    pub fn followed(&self, name: &str) -> bool {
        let Ok(shared) = self.shared.lock() else {
            return false;
        };
        let pane = shared
            .agents
            .iter()
            .find(|a| a.name == name)
            .map(|a| &a.pane);
        pane.is_some_and(|p| {
            shared
                .subscribers
                .iter()
                .any(|(_, panes)| panes.contains(p))
        })
    }

    /// The agent's status changes, with what its integration says; those
    /// following it hear of it.
    pub fn set_status(&self, name: &str, status: &str, label: Option<&str>) -> anyhow::Result<()> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| anyhow::anyhow!("the mock herdr is broken"))?;
        let agent = shared
            .agents
            .iter_mut()
            .find(|a| a.name == name)
            .ok_or_else(|| anyhow::anyhow!("no agent {name:?} in the mock herdr"))?;
        agent.status = status.to_owned();
        agent.label = label.map(str::to_owned);
        let agent = agent.clone();
        let info = agent.info();
        let line = format!(
            "{}\n",
            json!({
                "event": "pane.agent_status_changed",
                "data": {
                    "pane_id": agent.pane,
                    "workspace_id": "w1",
                    "agent_status": agent.status,
                    "agent": info["agent"],
                    "display_agent": info["display_agent"],
                    "state_labels": info["state_labels"],
                },
            })
        );
        shared.subscribers.retain_mut(|(stream, panes)| {
            !panes.contains(&agent.pane) || stream.write_all(line.as_bytes()).is_ok()
        });
        Ok(())
    }

    /// What it was asked: `agent.focus refacto`, `agent.send_keys refacto
    /// enter`…, agents by name.
    pub fn asked(&self) -> Vec<String> {
        self.shared
            .lock()
            .map(|s| s.asked.clone())
            .unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn serve(stream: UnixStream, shared: &Mutex<Shared>, stop: &AtomicBool) {
    if stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .is_err()
    {
        return;
    }
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return,
            Ok(_) if line.ends_with(b"\n") => {
                let request: Value = serde_json::from_slice(&line).unwrap_or_default();
                line.clear();
                let Some(answer) = answer(&request, shared, &writer) else {
                    return;
                };
                if (&writer)
                    .write_all(format!("{answer}\n").as_bytes())
                    .is_err()
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

/// The answer to a request, as herdr gives it.
fn answer(request: &Value, shared: &Mutex<Shared>, connection: &UnixStream) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(json!(""));
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let mut shared = shared.lock().ok()?;
    let name_of = |shared: &Shared, target: &str| {
        shared
            .agents
            .iter()
            .find(|a| a.pane == target || a.name == target)
            .map(|a| a.name.clone())
    };
    let target = params.get("target").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "ping" => json!({ "type": "pong", "version": "0.8.0", "protocol": 19 }),
        "agent.list" => json!({
            "type": "agent_list",
            "agents": shared.agents.iter().map(Agent::info).collect::<Vec<_>>(),
        }),
        "agent.focus" => {
            let Some(name) = name_of(&shared, target) else {
                return Some(error(id, "agent_not_found"));
            };
            shared.asked.push(format!("agent.focus {name}"));
            let info = shared
                .agents
                .iter()
                .find(|a| a.name == name)
                .map(Agent::info);
            json!({ "type": "agent_info", "agent": info })
        }
        "agent.send_keys" => {
            let Some(name) = name_of(&shared, target) else {
                return Some(error(id, "agent_not_found"));
            };
            let keys: Vec<&str> = params
                .get("keys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            shared
                .asked
                .push(format!("agent.send_keys {name} {}", keys.join(" ")));
            json!({ "type": "ok" })
        }
        "events.subscribe" => {
            let panes = params
                .get("subscriptions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|s| {
                    s.get("type").and_then(Value::as_str) == Some("pane.agent_status_changed")
                })
                .filter_map(|s| s.get("pane_id").and_then(Value::as_str).map(str::to_owned))
                .collect();
            shared
                .subscribers
                .push((connection.try_clone().ok()?, panes));
            json!({ "type": "subscription_started" })
        }
        _ => return Some(error(id, "not_implemented")),
    };
    Some(json!({ "id": id, "result": result }))
}

fn error(id: Value, code: &str) -> Value {
    json!({ "id": id, "error": { "code": code, "message": code } })
}
