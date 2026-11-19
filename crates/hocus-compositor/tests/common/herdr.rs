//! A fake herdr (its socket API as `herdr api schema` gives it, protocol
//! 19), on a socket of the test's own: never the user's herdr.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::Path,
};

/// The fake herdr's requests.
pub type Heard = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// What the fake herdr holds: its workspaces, tabs and agents as its API
/// lists them (`herdr api schema`), and the connections subscribed to its
/// events.
#[derive(Default)]
pub struct FakeHerdr {
    pub workspaces: Vec<serde_json::Value>,
    pub tabs: Vec<serde_json::Value>,
    pub agents: Vec<serde_json::Value>,
    /// An older herdr: no `workspace.list`, no `tab.list`.
    pub no_labels: bool,
    pub subscribers: Vec<std::os::unix::net::UnixStream>,
}

pub type Herdr = std::sync::Arc<std::sync::Mutex<FakeHerdr>>;

impl FakeHerdr {
    /// One workspace, `workspace`, with tabs labelled `tabs` (an unnamed
    /// one is labelled with its place), a Claude Code working in the
    /// first: pane w1:p1, its directory `cwd`.
    pub fn with(workspace: &str, tabs: &[&str], cwd: Option<&str>) -> Self {
        let tab = |i: usize, label: &str| {
            serde_json::json!({ "tab_id": format!("w1:t{}", i + 1), "workspace_id": "w1",
                "number": i + 1, "label": label, "focused": i == 0, "pane_count": 1,
                "agent_status": "idle" })
        };
        Self {
            workspaces: vec![serde_json::json!({ "workspace_id": "w1", "number": 1,
                "label": workspace, "focused": true, "pane_count": tabs.len(),
                "tab_count": tabs.len(), "active_tab_id": "w1:t1", "agent_status": "working" })],
            tabs: tabs.iter().enumerate().map(|(i, l)| tab(i, l)).collect(),
            agents: vec![
                serde_json::json!({ "pane_id": "w1:p1", "terminal_id": "w1:term1",
                "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "revision": 1,
                "agent_status": "working", "agent": "claude", "display_agent": null,
                "name": null, "title": "✳ Claude Code", "cwd": cwd, "foreground_cwd": null,
                "interactive_ready": true, "launch_pending": false,
                "screen_detection_skipped": false, "state_change_seq": 1,
                "state_labels": {}, "tokens": {} }),
            ],
            ..Self::default()
        }
    }
}

/// A fake herdr on `socket`, holding `herdr`; each pane's foreground
/// process is `pid`. What it is asked besides listing and subscribing
/// goes into `heard`.
pub fn fake_herdr(
    socket: &Path,
    pid: u32,
    herdr: Herdr,
    heard: Heard,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    std::fs::create_dir_all(socket.parent().unwrap_or(Path::new("/")))?;
    let listener = UnixListener::bind(socket)?;
    Ok(std::thread::spawn(move || {
        let mut kept = Vec::new();
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let id = request["id"].clone();
            let Ok(mut held) = herdr.lock() else {
                return;
            };
            let method = request["method"].as_str();
            let result = match method {
                Some("agent.list") => {
                    serde_json::json!({ "type": "agent_list", "agents": held.agents })
                }
                Some("workspace.list" | "tab.list") if held.no_labels => serde_json::Value::Null,
                Some("workspace.list") => {
                    serde_json::json!({ "type": "workspace_list", "workspaces": held.workspaces })
                }
                Some("tab.list") => serde_json::json!({ "type": "tab_list", "tabs": held.tabs }),
                Some("pane.process_info") => {
                    serde_json::json!({ "type": "pane_process_info", "shell": { "pid": 1 },
                        "foreground": [ { "pid": pid, "name": "claude" } ] })
                }
                Some("events.subscribe") => serde_json::json!({ "type": "subscription_started" }),
                method => {
                    if let Ok(mut heard) = heard.lock() {
                        heard.push(format!("{} {}", method.unwrap_or(""), request["params"]));
                    }
                    serde_json::json!({ "type": "ok" })
                }
            };
            let answer = if result.is_null() {
                serde_json::json!({ "id": id, "error": { "code": "unknown_method",
                    "message": format!("unknown method {}", method.unwrap_or("")) } })
            } else {
                serde_json::json!({ "id": id, "result": result })
            };
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            if method == Some("events.subscribe")
                && let Ok(subscriber) = stream.try_clone()
            {
                held.subscribers.push(subscriber);
            }
            drop(held);
            // The subscription stays open.
            kept.push(stream);
        }
    }))
}
