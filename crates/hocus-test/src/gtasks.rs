//! A stand-in for Google Tasks' API (tasks/v1, #257) on 127.0.0.1, at
//! Google's paths below `/tasks/v1/`, never Google: the task lists of a
//! JSON fixture, kept in memory, which POST, PATCH and DELETE change as
//! Google would. It takes only the token GNOME Online Accounts' stand-in
//! handed out last ([`crate::goa`]), and, until told otherwise, as one that
//! grants the tasks scope; without it, every request is refused with the
//! 403 Google answers a token without the scope.
//!
//! The fixture is a list of task lists:
//! `[{ "id": "…", "title": "My Tasks", "tasks": [{ "id": "…", "title": "…",
//! "status": "needsAction", "due": "2026-10-21T00:00:00.000Z" }] }]`.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use serde_json::{Map, Value, json};

use crate::caldav::{HttpRequest, Server};

/// Where the API is, as Google's own.
pub const BASE: &str = "/tasks/v1/";
/// When a task changed here says it changed: the scenarios' virtual
/// morning, for the same order on every run.
const NOW: &str = "2026-10-21T14:37:00.000Z";

/// A task list: its id, its title, its tasks.
type List = (String, String, Vec<Map<String, Value>>);

#[derive(Debug, Default)]
pub(crate) struct Tasks {
    lists: Vec<List>,
    /// Ids given to tasks added.
    added: u32,
    /// The token does not grant the tasks scope.
    unshared: bool,
    /// Every request fails, as when Google cannot be reached.
    down: bool,
}

pub(crate) type Shared = Arc<Mutex<Tasks>>;

impl Tasks {
    /// The lists of the fixture at `file`, in place of those known.
    pub(crate) fn load(&mut self, file: &Path) -> anyhow::Result<()> {
        let text = std::fs::read_to_string(file)?;
        let lists: Vec<Value> = serde_json::from_str(&text)?;
        self.lists = lists
            .into_iter()
            .map(|l| {
                let tasks = l["tasks"]
                    .as_array()
                    .map(|tasks| {
                        tasks
                            .iter()
                            .filter_map(|t| t.as_object().cloned())
                            .map(stamped)
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    l["id"].as_str().unwrap_or_default().to_owned(),
                    l["title"].as_str().unwrap_or_default().to_owned(),
                    tasks,
                )
            })
            .collect();
        Ok(())
    }

    pub(crate) fn share(&mut self, granted: bool) {
        self.unshared = !granted;
    }

    pub(crate) fn set_down(&mut self, down: bool) {
        self.down = down;
    }

    /// The task titled `title`, as Google keeps it now.
    pub(crate) fn find(&self, title: &str) -> Option<String> {
        self.lists
            .iter()
            .flat_map(|(_, _, tasks)| tasks)
            .find(|t| t.get("title").and_then(Value::as_str) == Some(title))
            .map(|t| Value::Object(t.clone()).to_string())
    }
}

/// A task with what Google adds to it: its kind and its ETag.
fn stamped(mut task: Map<String, Value>) -> Map<String, Value> {
    task.remove("etag");
    task.insert("kind".into(), json!("tasks#task"));
    task.entry("status").or_insert(json!("needsAction"));
    task.entry("updated").or_insert(json!(NOW));
    let text = Value::Object(task.clone()).to_string();
    task.insert("etag".into(), json!(etag(&text)));
    task
}

fn etag(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("\"{hash:016x}\"")
}

fn error(code: u16, message: &str, reason: &str) -> String {
    json!({
        "error": {
            "code": code,
            "message": message,
            "errors": [{ "message": message, "domain": "global", "reason": reason }],
        }
    })
    .to_string()
}

/// Serves `tasks` on 127.0.0.1 to the requests `accepted` says carry the
/// right token; `note` hears each request: `GET /tasks/v1/users/@me/lists
/// ok`, `… refused` or `… unshared`.
pub(crate) fn serve(
    tasks: Shared,
    accepted: impl Fn(&HttpRequest) -> bool + Send + 'static,
    note: impl Fn(String) + Send + 'static,
) -> std::io::Result<Server> {
    Server::serve("google tasks", BASE, move |request| {
        let path = request.path.split('?').next().unwrap_or_default();
        let said = |what: &str| note(format!("{} {path} {what}", request.method));
        if !accepted(request) {
            said("refused");
            return (
                "401 Unauthorized",
                error(
                    401,
                    "Request had invalid authentication credentials.",
                    "authError",
                ),
            );
        }
        let Ok(mut tasks) = tasks.lock() else {
            return ("500 Internal Server Error", String::new());
        };
        if tasks.down {
            said("down");
            return (
                "503 Service Unavailable",
                error(503, "The service is currently unavailable.", "backendError"),
            );
        }
        if tasks.unshared {
            said("unshared");
            return (
                "403 Forbidden",
                error(
                    403,
                    "Request had insufficient authentication scopes.",
                    "insufficientPermissions",
                ),
            );
        }
        said("ok");
        answer(&mut tasks, &request.method, path, &request.body)
    })
}

fn answer(tasks: &mut Tasks, method: &str, path: &str, body: &str) -> (&'static str, String) {
    let missing = || ("404 Not Found", error(404, "Not Found", "notFound"));
    let rest = path.strip_prefix(BASE).unwrap_or_default();
    let parts: Vec<&str> = rest.split('/').collect();
    match (method, parts.as_slice()) {
        ("GET", ["users", "@me", "lists"]) => {
            let items: Vec<Value> = tasks
                .lists
                .iter()
                .map(|(id, title, _)| {
                    json!({ "kind": "tasks#taskList", "id": id, "title": title, "updated": NOW })
                })
                .collect();
            (
                "200 OK",
                json!({ "kind": "tasks#taskLists", "items": items }).to_string(),
            )
        }
        (_, ["lists", list, "tasks", ..]) => {
            let Some((_, _, found)) = tasks.lists.iter_mut().find(|(id, _, _)| id == list) else {
                return missing();
            };
            match (method, &parts[3..]) {
                ("GET", []) => {
                    let items: Vec<Value> = found.iter().cloned().map(Value::Object).collect();
                    (
                        "200 OK",
                        json!({ "kind": "tasks#tasks", "items": items }).to_string(),
                    )
                }
                ("POST", []) => {
                    let Ok(Value::Object(mut task)) = serde_json::from_str(body) else {
                        return ("400 Bad Request", error(400, "Invalid JSON", "parseError"));
                    };
                    tasks.added += 1;
                    task.insert("id".into(), json!(format!("added-{}", tasks.added)));
                    task.insert("updated".into(), json!(NOW));
                    let task = stamped(task);
                    let text = Value::Object(task.clone()).to_string();
                    if let Some((_, _, found)) =
                        tasks.lists.iter_mut().find(|(id, _, _)| id == list)
                    {
                        found.push(task);
                    }
                    ("200 OK", text)
                }
                ("PATCH", [id]) => {
                    let Some(task) = found
                        .iter_mut()
                        .find(|t| t.get("id").and_then(Value::as_str) == Some(id))
                    else {
                        return missing();
                    };
                    let Ok(Value::Object(fields)) = serde_json::from_str(body) else {
                        return ("400 Bad Request", error(400, "Invalid JSON", "parseError"));
                    };
                    for (name, value) in fields {
                        if value.is_null() {
                            task.remove(&name);
                        } else {
                            task.insert(name, value);
                        }
                    }
                    // As Google: a task not done has no completion.
                    if task.get("status").and_then(Value::as_str) == Some("needsAction") {
                        task.remove("completed");
                    }
                    task.insert("updated".into(), json!(NOW));
                    *task = stamped(std::mem::take(task));
                    ("200 OK", Value::Object(task.clone()).to_string())
                }
                ("DELETE", [id]) => {
                    let before = found.len();
                    found.retain(|t| t.get("id").and_then(Value::as_str) != Some(id));
                    if found.len() == before {
                        return missing();
                    }
                    ("204 No Content", String::new())
                }
                _ => ("405 Method Not Allowed", String::new()),
            }
        }
        _ => missing(),
    }
}
