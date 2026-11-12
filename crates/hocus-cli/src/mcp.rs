//! `hocus mcp --agent NAME` (docs/design/mcp.md): a Model Context
//! Protocol server on standard input and output, JSON-RPC 2.0, one message
//! per line, for an agent the configuration names (`[[agents.mcp]]`).
//!
//! It runs as a program of its own, never on the compositor's loop: it
//! blocks on its client and on the compositor's answers. It holds one
//! connection to the control socket ([`hocus_agents::mcp`]), made at the
//! first tool call and made again if it drops, and hands each tool call
//! on: the compositor checks the rights, shows the intent, acts, keeps the
//! journal. Its refusals come back as tool errors the agent reads.
//!
//! Ported from v0-first-build's `crates/hocus-api/src/mcp.rs`: its
//! JSON-RPC handling (batches, notifications, error codes), its
//! `initialize` and its revisions, its tools' results and errors. New: the
//! current revision, which says its version on every request.

use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use hocus_agents::mcp::{Answer, Asked, MAX_INTENT};
use serde_json::{Map, Value, json};

/// The revision that says its version on every request (`_meta`).
pub const MODERN: &str = "2026-07-28";
/// The revisions that open with `initialize`, newest first; the newest is
/// offered to a client that asks for one not listed.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Where a modern request says its version.
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";

/// How long the compositor may take: an action is shown first
/// (`[agents] show-before-ms`), a setting written on a thread.
const ANSWER: Duration = Duration::from_secs(30);
const WRITE: Duration = Duration::from_secs(2);

/// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_VERSION: i64 = -32022;

const INSTRUCTIONS: &str = "The windows, desktops, screens and settings of a Hocuspanda \
    desktop. Read them with list_windows, list_desktops, list_outputs and get_settings; when \
    this agent may act, focus and move windows, send them to another desktop, switch desktops, \
    launch an application and change a setting. Say what each action is for with `intent`, in \
    one short line: the user sees it on what you touch before it happens, and then a summary \
    they can undo. The user may stop every agent at once; a refusal says why.";

/// The connection to the compositor.
struct Line {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

/// The server: the client's revision, and the compositor's line.
pub struct Bridge {
    socket: Option<PathBuf>,
    agent: String,
    line: Option<Line>,
    protocol: Option<&'static str>,
}

/// A tool's result: what it says, its data, and whether it failed.
struct Done {
    text: String,
    data: Option<Value>,
    error: bool,
}

impl Bridge {
    pub fn new(socket: Option<PathBuf>, agent: String) -> Self {
        Self {
            socket,
            agent,
            line: None,
            protocol: None,
        }
    }

    /// Answers every message of `input` on `output` until `input` ends.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(answer) = self.handle_line(&line) {
                writeln!(output, "{answer}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// The answer to one line, if it calls for one.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let answer = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) if batch.is_empty() => {
                Some(error(Value::Null, INVALID_REQUEST, "an empty batch".into()))
            }
            // Batches came and went with revision 2025-03-26: still
            // answered, in one array.
            Ok(Value::Array(batch)) => {
                let answers: Vec<Value> =
                    batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!answers.is_empty()).then_some(Value::Array(answers))
            }
            Ok(message) => self.handle(message),
            Err(err) => Some(error(Value::Null, PARSE_ERROR, format!("not JSON: {err}"))),
        };
        answer.map(|a| a.to_string())
    }

    /// The answer to one message: none for notifications and responses.
    fn handle(&mut self, message: Value) -> Option<Value> {
        let Value::Object(message) = message else {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "a message is an object".into(),
            ));
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response: this server asks the client nothing.
            return match id {
                Some(_) if message.contains_key("result") || message.contains_key("error") => None,
                id => Some(error(
                    id.unwrap_or(Value::Null),
                    INVALID_REQUEST,
                    "no method".into(),
                )),
            };
        };
        // Notifications need nothing: each request is answered before the
        // next is read.
        let id = id?;
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\"".into()));
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // A modern request says its version; one this server does not
        // speak is told those it does.
        let modern = match params
            .get("_meta")
            .and_then(|m| m.get(VERSION_KEY))
            .and_then(Value::as_str)
        {
            Some(MODERN) => true,
            Some(other) => {
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": UNSUPPORTED_VERSION,
                        "message": "Unsupported protocol version",
                        "data": { "supported": supported(), "requested": other },
                    },
                }));
            }
            None => false,
        };
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "server/discover" => Ok(discover()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(&params),
            other => Err((METHOD_NOT_FOUND, format!("no method {other:?}"))),
        };
        Some(match result {
            Ok(mut result) => {
                if modern && let Value::Object(fields) = &mut result {
                    fields.insert("resultType".into(), json!("complete"));
                }
                json!({ "jsonrpc": "2.0", "id": id, "result": result })
            }
            Err((code, message)) => error(id, code, message),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = LEGACY
            .iter()
            .find(|v| Some(**v) == asked)
            .unwrap_or(&"2025-11-25");
        self.protocol = Some(version);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": server_info(),
            "instructions": INSTRUCTIONS,
        })
    }

    /// The revision agreed at `initialize`, if a client opened with it.
    #[cfg(test)]
    pub fn protocol(&self) -> Option<&'static str> {
        self.protocol
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_owned()))?;
        if !tools().iter().any(|t| t["name"] == name) {
            return Err((INVALID_PARAMS, format!("unknown tool {name:?}")));
        }
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(args @ Value::Object(_)) => args.clone(),
            Some(_) => return Err((INVALID_PARAMS, "arguments are an object".into())),
        };
        let done = match name {
            "launch" => self.launch(&args),
            _ => self.ask(name, args),
        };
        let mut result = json!({
            "content": [{ "type": "text", "text": done.text }],
            "isError": done.error,
        });
        if let (Some(data), Some(fields)) = (done.data, result.as_object_mut()) {
            fields.insert("structuredContent".into(), data);
        }
        Ok(result)
    }

    /// `launch`: the desktop entry the agent names, never a command of its
    /// own, handed on.
    fn launch(&mut self, args: &Value) -> Done {
        let Some(app) = args.get("app").and_then(Value::as_str) else {
            return failed(
                "launch: say which application with `app`, a desktop entry's id or name",
            );
        };
        if let Some(odd) = args
            .as_object()
            .and_then(|a| a.keys().find(|k| !matches!(k.as_str(), "app" | "intent")))
        {
            return failed(&format!("launch: unknown field `{odd}`"));
        }
        let locale = hocus_xdg::desktop::Locale::from_env();
        let entries = hocus_xdg::desktop::scan(
            &hocus_xdg::desktop::data_dirs(),
            &locale,
            &hocus_xdg::desktop::current_desktops(),
        );
        let Some(entry) = find_entry(&entries, app) else {
            return failed(&format!(
                "launch: no application {app:?} among the desktop's ({} of them)",
                entries.len()
            ));
        };
        self.ask(
            "launch",
            json!({
                "id": entry.id,
                "name": entry.name,
                "argv": entry.argv,
                "terminal": entry.terminal,
                "dir": entry.dir,
                "intent": args.get("intent").cloned().unwrap_or(Value::Null),
            }),
        )
    }

    /// Hands `tool` on to the compositor, and its answer back.
    fn ask(&mut self, tool: &str, args: Value) -> Done {
        match self.exchange(tool, args) {
            Ok(Answer {
                error: Some(why), ..
            }) => failed(&why),
            Ok(Answer { ok, .. }) => {
                let data = ok.unwrap_or(Value::Null);
                Done {
                    text: serde_json::to_string_pretty(&data).unwrap_or_default(),
                    data: data.is_object().then_some(data),
                    error: false,
                }
            }
            Err(why) => {
                // Made again at the next call: the compositor may be back.
                self.line = None;
                failed(&why)
            }
        }
    }

    fn exchange(&mut self, tool: &str, args: Value) -> Result<Answer, String> {
        if self.line.is_none() {
            self.line = Some(self.connect()?);
        }
        let line = self.line.as_mut().ok_or("no connection")?;
        line.next += 1;
        let id = line.next;
        let asked = Asked {
            id,
            tool: tool.to_owned(),
            args,
        };
        let text = serde_json::to_string(&asked).map_err(|e| e.to_string())?;
        line.stream
            .write_all(format!("{text}\n").as_bytes())
            .map_err(|e| format!("the compositor's line: {e}"))?;
        loop {
            let mut answer = String::new();
            match line.reader.read_line(&mut answer) {
                Ok(0) => return Err("the compositor closed its line".into()),
                Ok(_) => {}
                Err(err) => return Err(format!("the compositor did not answer: {err}")),
            }
            let answer: Answer = serde_json::from_str(&answer)
                .map_err(|e| format!("the compositor's answer does not read: {e}"))?;
            // An answer to a call given up on comes late: left.
            if answer.id == id {
                return Ok(answer);
            }
        }
    }

    fn connect(&self) -> Result<Line, String> {
        let path = crate::socket_path(self.socket.clone())
            .ok_or("no desktop: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
        let at =
            |e: io::Error| format!("the compositor is not reachable at {}: {e}", path.display());
        let mut stream = UnixStream::connect(&path).map_err(at)?;
        stream.set_write_timeout(Some(WRITE)).map_err(at)?;
        stream.set_read_timeout(Some(ANSWER)).map_err(at)?;
        stream
            .write_all(format!("{}\n", hocus_agents::mcp::opening(&self.agent)).as_bytes())
            .map_err(at)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(at)?);
        // The compositor's hello first: what was written before it could
        // be read by another of its threads.
        let mut hello = String::new();
        reader.read_line(&mut hello).map_err(at)?;
        match serde_json::from_str::<Answer>(&hello) {
            Ok(Answer {
                id: 0, ok: Some(_), ..
            }) => {}
            _ => return Err(format!("{} does not serve agents", path.display())),
        }
        Ok(Line {
            stream,
            reader,
            next: 0,
        })
    }
}

/// The entry `app` names: its id (case aside, `.desktop` or not), else
/// its name, else the one name that holds it.
fn find_entry<'a>(
    entries: &'a [hocus_xdg::desktop::Entry],
    app: &str,
) -> Option<&'a hocus_xdg::desktop::Entry> {
    let app = app.trim();
    let id = app.strip_suffix(".desktop").unwrap_or(app);
    let low = app.to_lowercase();
    entries
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
        .or_else(|| hocus_xdg::desktop::entry_for_app(entries, id))
        .or_else(|| entries.iter().find(|e| e.name.to_lowercase() == low))
        .or_else(|| {
            let mut holding = entries
                .iter()
                .filter(|e| e.name.to_lowercase().contains(&low));
            match (holding.next(), holding.next()) {
                (Some(one), None) => Some(one),
                _ => None,
            }
        })
}

fn failed(why: &str) -> Done {
    Done {
        text: why.to_owned(),
        data: None,
        error: true,
    }
}

fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN)
        .chain(LEGACY.iter().copied())
        .collect()
}

fn server_info() -> Value {
    json!({
        "name": "hocuspanda",
        "title": "Hocuspanda",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn discover() -> Value {
    json!({
        "supportedVersions": supported(),
        "capabilities": { "tools": { "listChanged": false } },
        "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
        "instructions": INSTRUCTIONS,
    })
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The tools, as `tools/list` describes them.
fn tools() -> Vec<Value> {
    let window = json!({ "type": "integer", "minimum": 0, "description": "A window's id, from list_windows." });
    let intent = json!({ "type": "string", "maxLength": MAX_INTENT, "description": "What this does, in one short line for the user, shown on what it touches before it happens: \"Puts the two terminals on the left\"." });
    let none = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    let read = json!({ "readOnlyHint": true, "openWorldHint": false });
    let act = json!({ "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false });
    vec![
        json!({
            "name": "list_windows",
            "title": "List windows",
            "description": "Every window: id, title, app id, its screen (output), its desktop (from 1), \
                its geometry in the global logical space, whether it has the keyboard, is minimized, \
                and its state (floating, maximized, snapped, fullscreen).",
            "inputSchema": none,
            "annotations": read,
        }),
        json!({
            "name": "list_desktops",
            "title": "List desktops",
            "description": "Each screen's desktops: the one it shows (current, from 1), and each \
                desktop's number, name and how many windows it holds.",
            "inputSchema": none,
            "annotations": read,
        }),
        json!({
            "name": "list_outputs",
            "title": "List screens",
            "description": "The screens: name, geometry in the global logical space, the work area \
                windows go in (without the bar), scale.",
            "inputSchema": none,
            "annotations": read,
        }),
        json!({
            "name": "get_settings",
            "title": "Read settings",
            "description": "The desktop's settings as they are now, by dotted key \
                (appearance.theme): the configuration file's value, else the default.",
            "inputSchema": {
                "type": "object",
                "properties": { "section": { "type": "string", "description": "One section: appearance, windows, desktops…" } },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
        json!({
            "name": "journal",
            "title": "Journal",
            "description": "What this agent did and was refused this session, oldest first: the \
                time, the tool, what it touched, its intent, why it was refused.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 50 } },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
        json!({
            "name": "focus_window",
            "title": "Focus a window",
            "description": "Brings a window forward with the keyboard, on its desktop; a minimized \
                one comes back. Shown to the user first.",
            "inputSchema": {
                "type": "object",
                "properties": { "window": window, "intent": intent },
                "required": ["window", "intent"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "move_window",
            "title": "Move a window",
            "description": "Moves and sizes a window (global logical pixels; what is left out \
                stays), a snapped or maximized one floating first, and/or sends it to another \
                desktop of its screen. Shown to the user first; the user can undo it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "width": { "type": "integer", "minimum": 1 },
                    "height": { "type": "integer", "minimum": 1 },
                    "desktop": { "type": "integer", "minimum": 1, "description": "A desktop of its screen, from 1." },
                    "intent": intent,
                },
                "required": ["window", "intent"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "switch_desktop",
            "title": "Switch desktop",
            "description": "A screen shows another of its desktops. Shown to the user first; the \
                user can undo it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "desktop": { "type": "integer", "minimum": 1 },
                    "output": { "type": "string", "description": "The screen, from list_outputs; the focused window's by default." },
                    "intent": intent,
                },
                "required": ["desktop", "intent"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "launch",
            "title": "Launch an application",
            "description": "Starts an installed application, by its desktop entry's id \
                (org.mozilla.firefox) or its name (Firefox), as the launcher does. Shown to the \
                user first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app": { "type": "string", "description": "A desktop entry's id or name." },
                    "intent": intent,
                },
                "required": ["app", "intent"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": true },
        }),
        json!({
            "name": "set_setting",
            "title": "Change a setting",
            "description": "Writes one setting into the configuration file, as the Settings do; \
                the desktop follows it at once. Only keys get_settings lists, with a value of \
                the same type; never [agents], [shortcuts] or [developer]. Shown to the user \
                first; the user can undo it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Dotted: appearance.theme." },
                    "value": { "description": "Of the type get_settings shows for it." },
                    "intent": intent,
                },
                "required": ["key", "value", "intent"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge() -> Bridge {
        Bridge::new(
            Some(PathBuf::from("/nonexistent/hocus-0.control")),
            "t".into(),
        )
    }

    fn ask(bridge: &mut Bridge, line: &str) -> Value {
        let answer = bridge.handle_line(line).expect("an answer");
        serde_json::from_str(&answer).expect("JSON")
    }

    #[test]
    fn initialize_agrees_on_the_version_asked_or_offers_the_newest() {
        let mut b = bridge();
        let a = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        );
        assert_eq!(a["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(a["result"]["serverInfo"]["name"], "hocuspanda");
        assert_eq!(b.protocol(), Some("2025-06-18"));
        let a = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(a["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn a_modern_request_says_its_version_and_gets_told_those_spoken() {
        let mut b = bridge();
        let a = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
        );
        assert_eq!(a["result"]["resultType"], "complete");
        assert_eq!(a["result"]["supportedVersions"][0], MODERN);
        let a = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}"#,
        );
        assert_eq!(a["error"]["code"], UNSUPPORTED_VERSION);
        assert_eq!(a["error"]["data"]["requested"], "1900-01-01");
        // Without `_meta`: the handshake's revisions, no resultType.
        let a = ask(&mut b, r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#);
        assert!(a["result"].get("resultType").is_none());
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let mut b = bridge();
        assert_eq!(
            b.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(
            b.handle_line(r#"{"jsonrpc":"2.0","id":5,"result":{}}"#),
            None
        );
    }

    #[test]
    fn errors_follow_json_rpc() {
        let mut b = bridge();
        assert_eq!(ask(&mut b, "{nope")["error"]["code"], PARSE_ERROR);
        assert_eq!(ask(&mut b, "[]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(
            ask(&mut b, r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            ask(&mut b, r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#)["error"]["code"],
            METHOD_NOT_FOUND
        );
        assert_eq!(
            ask(
                &mut b,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"close_window"}}"#
            )["error"]["code"],
            INVALID_PARAMS
        );
    }

    #[test]
    fn a_batch_is_answered_in_one_array() {
        let mut b = bridge();
        let a = ask(
            &mut b,
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
        );
        assert_eq!(a.as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn every_acting_tool_asks_for_an_intent() {
        for tool in tools() {
            let acts = tool["annotations"]["readOnlyHint"] == false;
            let required = tool["inputSchema"]["required"]
                .as_array()
                .is_some_and(|r| r.contains(&json!("intent")));
            assert_eq!(acts, required, "{}", tool["name"]);
        }
    }

    #[test]
    fn a_tool_that_cannot_reach_the_compositor_says_so_as_its_result() {
        let mut b = bridge();
        let a = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_windows"}}"#,
        );
        assert_eq!(a["result"]["isError"], true);
        let said = a["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(said.contains("not reachable"), "{said}");
    }

    #[test]
    fn an_application_is_found_by_its_id_else_its_name() {
        use hocus_xdg::desktop::Entry;
        let entry = |id: &str, name: &str| Entry {
            id: id.into(),
            name: name.into(),
            ..Entry::default()
        };
        let entries = [
            entry("org.mozilla.firefox", "Firefox"),
            entry("kitty", "kitty"),
            entry("org.gnome.TextEditor", "Text Editor"),
        ];
        let id = |app: &str| find_entry(&entries, app).map(|e| e.id.as_str());
        assert_eq!(
            id("org.mozilla.firefox.desktop"),
            Some("org.mozilla.firefox")
        );
        assert_eq!(id("firefox"), Some("org.mozilla.firefox"));
        assert_eq!(id("text editor"), Some("org.gnome.TextEditor"));
        assert_eq!(id("edit"), Some("org.gnome.TextEditor"));
        assert_eq!(id("t"), None, "several hold it");
        assert_eq!(id("nope"), None);
    }
}
