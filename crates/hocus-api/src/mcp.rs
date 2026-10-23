//! A Model Context Protocol server (#42), behind `hocus mcp`: JSON-RPC 2.0
//! over stdio, one message per line, bridging the control API so that an
//! agent uses the desktop through windows and the shell's elements rather
//! than guessing on pixels.
//!
//! It runs as a program of its own, never on the compositor's loop: it
//! blocks on its client while waiting for a state. With `--agent NAME` it
//! names itself to the compositor, which then holds it to the rights
//! `[[agents.mcp]]` gives that agent; refusals come back as tool errors the
//! agent reads.

use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use serde_json::{Map, Value, json};

use crate::{
    ActTarget, Action, ApiError, Button, Client, ClientError, ErrorCode, Point, Reply, Request,
    Target, UiNode, WindowInfo,
};

/// Protocol revisions spoken, newest first; the newest is offered to a
/// client that asks for one not listed.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a request to the compositor may take: a capture of a large
/// window is encoded on a worker first.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long an action may take: a sensitive one waits for the user, five
/// minutes at most (`[agents] confirm_timeout`), before it is refused.
const ACT_TIMEOUT: Duration = Duration::from_secs(320);
/// How often `wait_for` looks again.
const POLL: Duration = Duration::from_millis(100);
const WAIT_DEFAULT_MS: u64 = 10_000;
const WAIT_MAX_MS: u64 = 120_000;
/// Journal entries given when not asked for a number, and at most: all the
/// compositor keeps.
const JOURNAL_DEFAULT: u64 = 50;
const JOURNAL_MAX: u64 = 500;
/// An intent is one line: longer is cut.
const MAX_INTENT: usize = 120;

/// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What tells applications' windows apart from the shell for an agent.
const NO_APP_TREE: &str = "Applications' own accessibility trees (AT-SPI) are not read: \
     capture the window to see its content, and aim clicks at points of it, relative to its \
     top-left corner in logical pixels.";

/// The server: what the client agreed on, and the connection to the
/// compositor, made at the first tool call and made again if it drops.
#[derive(Debug)]
pub struct Bridge {
    socket: PathBuf,
    agent: Option<String>,
    client: Option<Client>,
    protocol: Option<&'static str>,
}

/// Why a tool call did not do what was asked: told to the agent as the
/// tool's result, so that it can read it and try otherwise.
#[derive(Debug)]
struct Failure(String);

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self(message.to_owned())
    }
}

/// A tool's successful result: what it says, and its data.
#[derive(Debug)]
struct Done {
    content: Vec<Value>,
    structured: Option<Value>,
}

impl Done {
    fn data(value: Value) -> Self {
        Self {
            content: vec![text(
                &serde_json::to_string_pretty(&value).unwrap_or_default(),
            )],
            structured: Some(value),
        }
    }

    fn said(message: &str) -> Self {
        Self {
            content: vec![text(message)],
            structured: None,
        }
    }
}

fn text(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

impl Bridge {
    /// A server for the compositor at `socket`, as agent `agent` of the
    /// configuration, or as a plain client (which reads, and acts only on
    /// a compositor started for tests).
    pub fn new(socket: PathBuf, agent: Option<String>) -> Self {
        Self {
            socket,
            agent,
            client: None,
            protocol: None,
        }
    }

    /// The protocol revision agreed at initialization.
    pub fn protocol(&self) -> Option<&'static str> {
        self.protocol
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

    /// The answer to one message: `None` for notifications and responses.
    pub fn handle(&mut self, message: Value) -> Option<Value> {
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
        // Notifications (`notifications/initialized`, `…/cancelled`) need
        // nothing: each request is answered before the next is read.
        let id = id?;
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\"".into()));
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(&params),
            other => Err((METHOD_NOT_FOUND, format!("no method {other:?}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id, code, message),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = PROTOCOL_VERSIONS
            .iter()
            .find(|v| Some(**v) == asked)
            .unwrap_or(&PROTOCOL_VERSIONS[0]);
        self.protocol = Some(version);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "hocuspanda",
                "title": "Hocuspanda",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "The windows of a Hocuspanda desktop. List them, read the shell's \
                interface, capture a window to see it, and, when this agent may act, click, type \
                and scroll aimed at a window (points relative to its top-left corner, in logical \
                pixels) or at a labelled element of the shell, with a pointer and a keyboard of \
                your own: the user keeps theirs. Say what each action is for with `intent`, \
                shown to the user before it happens; sensitive ones wait for the user's \
                confirmation. Wait for a window or a label rather than sleeping. What agents do \
                is kept in a journal the user reads (and the journal tool gives); the user may \
                stop every agent at once, or take over from one.",
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_owned()))?;
        let empty = Map::new();
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(args)) => args,
            Some(_) => return Err((INVALID_PARAMS, "arguments are an object".into())),
        };
        let args = Args(args);
        let outcome = match name {
            "list_windows" => self.list_windows(),
            "accessibility_tree" => self.accessibility_tree(&args),
            "capture" => self.capture(&args),
            "click" => self.click(&args),
            "scroll" => self.scroll(&args),
            "type" => self.type_text(&args),
            "press_keys" => self.press_keys(&args),
            "wait_for" => self.wait_for(&args),
            "journal" => self.journal(&args),
            other => return Err((INVALID_PARAMS, format!("unknown tool {other:?}"))),
        };
        Ok(match outcome {
            Ok(done) => {
                let mut result = json!({ "content": done.content, "isError": false });
                if let Some(data) = done.structured {
                    result["structuredContent"] = data;
                }
                result
            }
            Err(Failure(message)) => json!({ "content": [text(&message)], "isError": true }),
        })
    }

    /// The connection to the compositor, named as the agent.
    fn client(&mut self) -> Result<&mut Client, Failure> {
        if self.client.is_none() {
            let mut client = Client::connect(&self.socket).map_err(|err| {
                format!(
                    "the compositor is not reachable at {}: {err}",
                    self.socket.display()
                )
            })?;
            client
                .set_timeout(Some(REQUEST_TIMEOUT))
                .map_err(|err| err.to_string())?;
            if let Some(name) = &self.agent {
                client
                    .request(Request::Agent { name: name.clone() })
                    .map_err(refusal)?;
            }
            self.client = Some(client);
        }
        self.client
            .as_mut()
            .ok_or_else(|| Failure::from("no connection"))
    }

    fn request(&mut self, request: Request) -> Result<Reply, Failure> {
        let result = self.client()?.request(request);
        if let Err(ClientError::Io(_) | ClientError::Closed | ClientError::Protocol(_)) = &result {
            // Made again at the next call: the compositor may be back.
            self.client = None;
        }
        result.map_err(refusal)
    }

    fn windows(&mut self) -> Result<Vec<WindowInfo>, Failure> {
        match self.request(Request::Windows)? {
            Reply::Windows(windows) => Ok(windows),
            other => Err(unexpected(&other)),
        }
    }

    fn ui(&mut self) -> Result<Vec<UiNode>, Failure> {
        match self.request(Request::Ui)? {
            Reply::Ui(nodes) => Ok(nodes),
            other => Err(unexpected(&other)),
        }
    }

    fn list_windows(&mut self) -> Result<Done, Failure> {
        let outputs = match self.request(Request::Outputs)? {
            Reply::Outputs(outputs) => outputs,
            other => return Err(unexpected(&other)),
        };
        let windows = self.windows()?;
        Ok(Done::data(json!({
            "outputs": outputs.iter().map(|o| json!({
                "name": o.name,
                "geometry": o.geometry,
                "scale": o.scale,
            })).collect::<Vec<_>>(),
            "windows": windows.iter().map(window_json).collect::<Vec<_>>(),
        })))
    }

    fn accessibility_tree(&mut self, args: &Args) -> Result<Done, Failure> {
        match args.u64("window")? {
            None => {
                let nodes = self.ui()?;
                Ok(Done::data(json!({
                    "source": "shell",
                    "elements": nodes,
                })))
            }
            Some(id) => {
                let window = self.window(id)?;
                Ok(Done::data(json!({
                    "source": "window",
                    "window": window_json(&window),
                    "elements": [],
                    "note": NO_APP_TREE,
                })))
            }
        }
    }

    fn window(&mut self, id: u64) -> Result<WindowInfo, Failure> {
        self.windows()?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| Failure(format!("no window with id {id}")))
    }

    fn capture(&mut self, args: &Args) -> Result<Done, Failure> {
        let (target, logical) = match (args.u64("window")?, args.str("output")?) {
            (Some(id), None) => {
                let window = self.window(id)?;
                (Target::Window(id), window.geometry)
            }
            (None, Some(name)) => {
                let outputs = match self.request(Request::Outputs)? {
                    Reply::Outputs(outputs) => outputs,
                    other => return Err(unexpected(&other)),
                };
                let output = outputs
                    .into_iter()
                    .find(|o| o.name == name)
                    .ok_or_else(|| Failure(format!("no output named {name}")))?;
                (Target::Output(name.to_owned()), output.geometry)
            }
            _ => return Err("say which: window (an id) or output (a name), one of them".into()),
        };
        let shot = match self.request(Request::Screenshot {
            target,
            cursor: false,
        })? {
            Reply::Screenshot(shot) => shot,
            other => return Err(unexpected(&other)),
        };
        let scale = if logical.width > 0 {
            f64::from(shot.width) / f64::from(logical.width)
        } else {
            1.0
        };
        let about = format!(
            "{}×{} pixels for {}×{} logical: divide by {scale} for coordinates to aim at",
            shot.width, shot.height, logical.width, logical.height
        );
        Ok(Done {
            content: vec![
                json!({ "type": "image", "data": shot.png_base64, "mimeType": "image/png" }),
                text(&about),
            ],
            structured: None,
        })
    }

    /// What `click` and `scroll` aim at.
    fn target(&self, args: &Args) -> Result<ActTarget, Failure> {
        match (args.u64("window")?, args.str("element")?) {
            (Some(window), None) => {
                let at = match (args.f64("x")?, args.f64("y")?) {
                    (Some(x), Some(y)) => Some(Point { x, y }),
                    (None, None) => None,
                    _ => return Err("x and y go together".into()),
                };
                Ok(ActTarget::Window { window, at })
            }
            (None, Some(label)) => Ok(ActTarget::Element {
                label: label.to_owned(),
            }),
            _ => Err(
                "say which: window (an id, with x and y or its centre) or element (a \
                      label of the shell), one of them"
                    .into(),
            ),
        }
    }

    /// Acts, with the intent the agent gave (shown before it acts), and
    /// waits as long as the user may take to confirm it.
    fn act(&mut self, target: ActTarget, action: Action, args: &Args) -> Result<(), Failure> {
        let intent = args
            .str("intent")?
            .map(str::trim)
            .filter(|i| !i.is_empty())
            .map(|i| i.chars().take(MAX_INTENT).collect());
        self.client()?
            .set_timeout(Some(ACT_TIMEOUT))
            .map_err(|err| err.to_string())?;
        let result = self.request(Request::Act {
            target,
            action,
            intent,
        });
        if let Some(client) = self.client.as_ref() {
            client
                .set_timeout(Some(REQUEST_TIMEOUT))
                .map_err(|err| err.to_string())?;
        }
        match result? {
            Reply::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    fn click(&mut self, args: &Args) -> Result<Done, Failure> {
        let target = self.target(args)?;
        let button = match args.str("button")?.unwrap_or("left") {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            other => return Err(format!("no button {other:?}: left, right or middle").into()),
        };
        let count = args.u64("count")?.unwrap_or(1).clamp(1, 3) as u32;
        self.act(target, Action::Click { button, count }, args)?;
        Ok(Done::said("clicked"))
    }

    fn scroll(&mut self, args: &Args) -> Result<Done, Failure> {
        let target = self.target(args)?;
        let dy = args.f64("dy")?.ok_or("dy: how far, in logical pixels")?;
        self.act(target, Action::Scroll { dy }, args)?;
        Ok(Done::said("scrolled"))
    }

    fn type_text(&mut self, args: &Args) -> Result<Done, Failure> {
        let window = args.u64("window")?.ok_or("window: which one, by id")?;
        let text = args.str("text")?.ok_or("text: what to type")?;
        self.act(
            ActTarget::Window { window, at: None },
            Action::Type {
                text: text.to_owned(),
            },
            args,
        )?;
        Ok(Done::said("typed"))
    }

    fn press_keys(&mut self, args: &Args) -> Result<Done, Failure> {
        let window = args.u64("window")?.ok_or("window: which one, by id")?;
        let keys = args.str("keys")?.ok_or("keys: a combination, as Ctrl+A")?;
        self.act(
            ActTarget::Window { window, at: None },
            Action::Keys {
                keys: keys.to_owned(),
            },
            args,
        )?;
        Ok(Done::said("pressed"))
    }

    fn wait_for(&mut self, args: &Args) -> Result<Done, Failure> {
        let app_id = args.str("app_id")?.map(str::to_owned);
        let title = args.str("title")?.map(str::to_owned);
        let label = args.str("label")?.map(str::to_owned);
        let gone = args.bool("gone")?.unwrap_or(false);
        if app_id.is_none() && title.is_none() && label.is_none() {
            return Err("wait for what: app_id, title or label".into());
        }
        let timeout = args
            .u64("timeout_ms")?
            .unwrap_or(WAIT_DEFAULT_MS)
            .min(WAIT_MAX_MS);
        let deadline = Instant::now() + Duration::from_millis(timeout);
        let window_wanted = app_id.is_some() || title.is_some();
        loop {
            let windows: Vec<WindowInfo> = if window_wanted {
                self.windows()?
                    .into_iter()
                    .filter(|w| {
                        app_id.as_ref().is_none_or(|a| w.app_id.as_ref() == Some(a))
                            && title.as_ref().is_none_or(|t| {
                                w.title.as_deref().is_some_and(|wt| wt.contains(t.as_str()))
                            })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let elements: Vec<UiNode> = match &label {
                Some(label) => self
                    .ui()?
                    .into_iter()
                    .filter(|n| n.label.as_deref() == Some(label.as_str()))
                    .collect(),
                None => Vec::new(),
            };
            let shown = (!window_wanted || !windows.is_empty())
                && (label.is_none() || !elements.is_empty());
            let none_left = windows.is_empty() && elements.is_empty();
            if (!gone && shown) || (gone && none_left) {
                return Ok(Done::data(json!({
                    "windows": windows.iter().map(window_json).collect::<Vec<_>>(),
                    "elements": elements,
                })));
            }
            if Instant::now() >= deadline {
                return Err(Failure(format!(
                    "timed out after {timeout} ms: {} {}",
                    if gone { "still there:" } else { "not seen:" },
                    describe(app_id.as_deref(), title.as_deref(), label.as_deref())
                )));
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Bridge {
    /// What agents did, oldest first: one line each for the agent to
    /// read, and the entries as data.
    fn journal(&mut self, args: &Args) -> Result<Done, Failure> {
        let limit = args
            .u64("limit")?
            .unwrap_or(JOURNAL_DEFAULT)
            .clamp(1, JOURNAL_MAX) as usize;
        let agent = args.str("agent")?.map(str::to_owned);
        let journal = match self.request(Request::AgentsJournal)? {
            Reply::AgentsJournal(journal) => journal,
            other => return Err(unexpected(&other)),
        };
        let mut entries: Vec<_> = journal
            .entries
            .into_iter()
            .filter(|e| agent.as_ref().is_none_or(|a| e.agent == *a))
            .collect();
        entries.drain(..entries.len().saturating_sub(limit));
        let mut lines: Vec<String> = entries.iter().map(ToString::to_string).collect();
        if lines.is_empty() {
            lines.push("Nothing yet.".into());
        }
        Ok(Done {
            content: vec![text(&lines.join("\n"))],
            structured: Some(json!({ "stopped": journal.stopped, "entries": entries })),
        })
    }
}

fn describe(app_id: Option<&str>, title: Option<&str>, label: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(a) = app_id {
        parts.push(format!("a window of {a:?}"));
    }
    if let Some(t) = title {
        parts.push(format!("a window titled with {t:?}"));
    }
    if let Some(l) = label {
        parts.push(format!("an element labelled {l:?}"));
    }
    parts.join(" and ")
}

fn window_json(w: &WindowInfo) -> Value {
    json!({
        "id": w.id,
        "title": w.title,
        "app_id": w.app_id,
        "output": w.output,
        "geometry": w.geometry,
        "focused": w.focused,
        "visible": w.visible,
        "state": w.state,
        "desktop": w.desktop,
    })
}

fn refusal(err: ClientError) -> Failure {
    match err {
        ClientError::Api(ApiError { code, message }) => Failure(match code {
            ErrorCode::PermissionDenied => format!("refused: {message}"),
            ErrorCode::NotFound => format!("not found: {message}"),
            ErrorCode::BadRequest => format!("bad request: {message}"),
            ErrorCode::Failed => format!("failed: {message}"),
        }),
        other => Failure(format!("the compositor: {other}")),
    }
}

fn unexpected(reply: &Reply) -> Failure {
    Failure(format!("unexpected answer from the compositor: {reply:?}"))
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A tool's arguments, read with their expected types.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn get<T>(
        &self,
        key: &str,
        kind: &str,
        read: impl FnOnce(&Value) -> Option<T>,
    ) -> Result<Option<T>, Failure> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => read(value)
                .map(Some)
                .ok_or_else(|| Failure(format!("{key}: expected {kind}, got {value}"))),
        }
    }

    fn u64(&self, key: &str) -> Result<Option<u64>, Failure> {
        self.get(key, "a whole number", Value::as_u64)
    }

    fn f64(&self, key: &str) -> Result<Option<f64>, Failure> {
        self.get(key, "a number", Value::as_f64)
    }

    fn str(&self, key: &str) -> Result<Option<&str>, Failure> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(value) => Err(Failure(format!("{key}: expected text, got {value}"))),
        }
    }

    fn bool(&self, key: &str) -> Result<Option<bool>, Failure> {
        self.get(key, "true or false", Value::as_bool)
    }
}

/// The tools, as `tools/list` describes them.
fn tools() -> Vec<Value> {
    let window = json!({ "type": "integer", "minimum": 0, "description": "A window's id, from list_windows." });
    let element = json!({ "type": "string", "description": "The label of an element of the shell, from accessibility_tree." });
    let x = json!({ "type": "number", "description": "From the window's left edge, logical pixels; its centre when x and y are left out." });
    let y =
        json!({ "type": "number", "description": "From the window's top edge, logical pixels." });
    let intent = json!({ "type": "string", "maxLength": MAX_INTENT, "description": "What this does, in one short line for the user, shown before it happens: \"Sends the message\"." });
    let read = json!({ "readOnlyHint": true, "openWorldHint": false });
    let act = json!({ "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false });
    vec![
        json!({
            "name": "list_windows",
            "title": "List windows",
            "description": "The screens (name, logical geometry, scale) and every window: id, title, app id, \
                the output showing most of it, geometry in the global logical space, whether it has \
                the keyboard, is visible, its state and desktop.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": read,
        }),
        json!({
            "name": "accessibility_tree",
            "title": "Accessibility tree",
            "description": format!("Without a window: the shell's own interface as screen readers see it \
                (the column, dock, launcher, panels, toasts), depth first: role, label, rectangle in \
                the global logical space, depth, focused, disabled, selected; labels are what click \
                and scroll aim at with `element`. With a window: that window only. {NO_APP_TREE}"),
            "inputSchema": {
                "type": "object",
                "properties": { "window": window },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
        json!({
            "name": "capture",
            "title": "Capture a window",
            "description": "A PNG of a window (or of a whole output) as the screen shows it, at the \
                output's scale; the text says how its pixels map to logical coordinates. A window \
                on a desktop no screen shows, or minimized, as its application last drew it: it \
                draws on while this agent acts in it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "output": { "type": "string", "description": "An output's name, instead of a window." },
                },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
        json!({
            "name": "click",
            "title": "Click",
            "description": "Clicks a point of a window, with this agent's own pointer (the user's \
                pointer and keyboard stay theirs), or the centre of a labelled element of the \
                shell. The window may be on a desktop no screen shows; a minimized one comes back \
                on its desktop first. A ring and the intent show at the point a moment before the \
                click; a sensitive one (a password manager, the settings…) waits until the user \
                confirms it. Refused when the agent may not act on that window's app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "x": x,
                    "y": y,
                    "element": element,
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "default": "left" },
                    "count": { "type": "integer", "minimum": 1, "maximum": 3, "default": 1, "description": "2 for a double click." },
                    "intent": intent,
                },
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "scroll",
            "title": "Scroll",
            "description": "Scrolls the wheel over a point of a window, or over a labelled element of \
                the shell.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "x": x,
                    "y": y,
                    "element": element,
                    "dy": { "type": "number", "description": "Logical pixels, 15 per notch, down positive." },
                    "intent": intent,
                },
                "required": ["dy"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "type",
            "title": "Type text",
            "description": "Gives a window this agent's own keyboard (the user's stays where it is) \
                and types text into it with the keyboard layout in effect (line breaks are Return), \
                wherever the window is. The keys go to the window alone, never to the desktop's \
                shortcuts. Typing into a password field waits until the user confirms it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "text": { "type": "string", "maxLength": 4096 },
                    "intent": intent,
                },
                "required": ["window", "text"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "press_keys",
            "title": "Press keys",
            "description": "Gives a window this agent's own keyboard and presses a combination in \
                it: `Ctrl+A`, `Return`, `Shift+Tab`, `Ctrl+Shift+T`, `Escape` (modifiers Ctrl, Shift, \
                Alt, Super; keys by their XKB names). Never a shortcut of the desktop's.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "keys": { "type": "string" },
                    "intent": intent,
                },
                "required": ["window", "keys"],
                "additionalProperties": false,
            },
            "annotations": act,
        }),
        json!({
            "name": "wait_for",
            "title": "Wait for a state",
            "description": "Waits until a window (by exact app id, and/or a part of its title) or an \
                element of the shell (by exact label) is shown, or with `gone` until none is; \
                answers what matched, or fails after the timeout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app_id": { "type": "string" },
                    "title": { "type": "string", "description": "A part of the title." },
                    "label": { "type": "string" },
                    "gone": { "type": "boolean", "default": false },
                    "timeout_ms": { "type": "integer", "minimum": 0, "maximum": WAIT_MAX_MS, "default": WAIT_DEFAULT_MS },
                },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
        json!({
            "name": "journal",
            "title": "Agents' journal",
            "description": "What agents did on this desktop this session, oldest first, one line \
                each: when (the user's local time), which agent, the action (click, scroll, type, \
                keys), on which window or element of the shell, and why it was refused if it was. \
                Typed text is kept only when the user's configuration asks for it; otherwise, how \
                many characters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": JOURNAL_MAX, "default": JOURNAL_DEFAULT, "description": "The last so many entries." },
                    "agent": { "type": "string", "description": "Only this agent's entries." },
                },
                "additionalProperties": false,
            },
            "annotations": read,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bridge to nowhere: what is answered without the compositor.
    fn bridge() -> Bridge {
        Bridge::new(PathBuf::from("/nonexistent/hocus-0.api"), None)
    }

    fn ask(bridge: &mut Bridge, line: &str) -> Value {
        let answer = bridge.handle_line(line).expect("an answer");
        serde_json::from_str(&answer).expect("JSON")
    }

    #[test]
    fn initialize_agrees_on_the_version_asked_or_offers_the_newest() {
        let mut b = bridge();
        let answer = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        );
        assert_eq!(answer["id"], 1);
        assert_eq!(answer["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(answer["result"]["serverInfo"]["name"], "hocuspanda");
        assert!(answer["result"]["capabilities"]["tools"].is_object());
        assert_eq!(b.protocol(), Some("2024-11-05"));

        let answer = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
        );
        assert_eq!(answer["id"], "a");
        assert_eq!(answer["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let mut b = bridge();
        assert_eq!(
            b.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(
            b.handle_line(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#
            ),
            None
        );
        assert_eq!(
            b.handle_line(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
            None
        );
    }

    #[test]
    fn errors_follow_json_rpc() {
        let mut b = bridge();
        assert_eq!(ask(&mut b, "{not json")["error"]["code"], PARSE_ERROR);
        assert_eq!(ask(&mut b, "{not json")["id"], Value::Null);
        assert_eq!(
            ask(
                &mut b,
                r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#
            )["error"]["code"],
            METHOD_NOT_FOUND
        );
        assert_eq!(
            ask(
                &mut b,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"format_disk"}}"#
            )["error"]["code"],
            INVALID_PARAMS
        );
        assert_eq!(
            ask(&mut b, r#"{"id":4,"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(ask(&mut b, "[]")["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn a_batch_is_answered_in_one_array() {
        let mut b = bridge();
        let answer = ask(
            &mut b,
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#,
        );
        let answers = answer.as_array().expect("an array");
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[1]["id"], 2);
    }

    #[test]
    fn tools_are_listed_with_their_schemas() {
        let mut b = bridge();
        let answer = ask(&mut b, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let tools = answer["result"]["tools"].as_array().expect("tools");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(
            names,
            [
                "list_windows",
                "accessibility_tree",
                "capture",
                "click",
                "scroll",
                "type",
                "press_keys",
                "wait_for",
                "journal"
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        }
    }

    #[test]
    fn a_tool_that_cannot_reach_the_compositor_says_so_as_its_result() {
        let mut b = bridge();
        let answer = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_windows","arguments":{}}}"#,
        );
        assert_eq!(answer["result"]["isError"], true);
        let message = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(message.contains("not reachable"), "{message}");
    }

    #[test]
    fn bad_arguments_are_told_to_the_agent() {
        let mut b = bridge();
        let answer = ask(
            &mut b,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"click","arguments":{"window":1,"x":3}}}"#,
        );
        assert_eq!(answer["result"]["isError"], true);
        assert_eq!(
            answer["result"]["content"][0]["text"],
            "x and y go together"
        );
    }

    #[test]
    fn serve_answers_line_by_line() -> io::Result<()> {
        let mut b = bridge();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n\
                     {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut output = Vec::new();
        b.serve(input.as_bytes(), &mut output)?;
        let text = String::from_utf8_lossy(&output);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("\"id\":2"));
        Ok(())
    }
}
