//! The agents' tools on this machine: whether each is there, and plugging
//! it in, which writes its hooks into its settings (bar.md, 4.0).
//!
//! - **Claude Code**: `~/.claude/settings.json`, its `hooks` (one matcher
//!   group of ours per event, running `hocus agent-hook claude-code`).
//!   Running sessions pick the change up (its settings are watched).
//! - **Codex**: `~/.codex/config.toml` (or `$CODEX_HOME`'s), its inline
//!   `[[hooks.EVENT]]` tables, and `notify` when the user has none. Codex
//!   runs a new hook only once the user trusted it in `/hooks`; `notify`
//!   (the end of each turn) needs no trust.
//! - **herdr** has nothing to plug: its socket is read when it runs.
//!
//! Plugging keeps a copy of the file as it was beside it
//! (`settings.json.before-hocuspanda`), and what was written is
//! remembered (its hash, in the desktop's state directory). Unplugging
//! puts the copy back when the file is still as written; if the user
//! changed it since, only our entries are taken out. A file that does not
//! read (broken JSON or TOML) is never written.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};

pub use crate::hooks::Tool;

/// What a copy of the original is called, beside it.
pub const COPY_SUFFIX: &str = ".before-hocuspanda";
/// The state file, in the desktop's state directory.
pub const MEMORY_FILE: &str = "agents.toml";
/// How long a tool waits for our hook at most, seconds: it takes a few
/// milliseconds, and gives up quietly when the desktop is not there.
const TIMEOUT: i64 = 5;
/// Codex bounds these two to 3 s.
const SHORT_TIMEOUT: i64 = 2;

/// Where the tools are looked for and their settings kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, else `~/.config`: herdr's.
    pub config_home: PathBuf,
    /// `$CODEX_HOME`, else `~/.codex`.
    pub codex_home: PathBuf,
    /// `$PATH`'s directories, where the tools' programs are.
    pub path: Vec<PathBuf>,
    /// herdr's socket: `$HERDR_SOCKET_PATH`, else in its directory.
    pub herdr_socket: PathBuf,
    /// Where herdr's clients are looked for, the processes: `/proc` (a
    /// test's table of its own).
    pub processes: PathBuf,
    /// The `hocus` program the hooks run.
    pub hocus: PathBuf,
    /// Where what was written and what was declined are remembered; none:
    /// for this session only.
    pub state_dir: Option<PathBuf>,
}

impl Places {
    /// From the environment, `hocus` the program the hooks run; none
    /// without a home.
    pub fn from_env(hocus: PathBuf, state_dir: Option<PathBuf>) -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
        let absolute = |var: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let path = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|d| d.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        let config_home = absolute("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        Some(Self {
            herdr_socket: absolute("HERDR_SOCKET_PATH")
                .unwrap_or_else(|| config_home.join("herdr/herdr.sock")),
            processes: PathBuf::from(crate::process::PROC),
            config_home,
            codex_home: absolute("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            home,
            path,
            hocus,
            state_dir,
        })
    }

    /// `tool`'s own directory.
    pub fn dir(&self, tool: Tool) -> PathBuf {
        match tool {
            Tool::ClaudeCode => self.home.join(".claude"),
            Tool::Codex => self.codex_home.clone(),
        }
    }

    /// The file plugging `tool` in changes.
    pub fn settings(&self, tool: Tool) -> PathBuf {
        match tool {
            Tool::ClaudeCode => self.dir(tool).join("settings.json"),
            Tool::Codex => self.dir(tool).join("config.toml"),
        }
    }

    /// That file as the user knows it: `~/` for the home.
    pub fn settings_shown(&self, tool: Tool) -> String {
        let file = self.settings(tool);
        match file.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => file.display().to_string(),
        }
    }

    /// herdr's directory, where its socket is.
    pub fn herdr_dir(&self) -> PathBuf {
        self.config_home.join("herdr")
    }

    /// Whether a program `name` is in `PATH`.
    pub fn has_program(&self, name: &str) -> bool {
        self.path.iter().any(|dir| is_program(&dir.join(name)))
    }

    /// The command our hooks run for `tool`.
    pub fn hook_command(&self, tool: Tool) -> String {
        format!("{} agent-hook {}", quoted(&self.hocus), tool.word())
    }
}

fn is_program(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// A path as a shell word.
fn quoted(path: &Path) -> String {
    let text = path.display().to_string();
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain && !text.is_empty() {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The program's name of a shell command: its first word, unquoted.
fn program_of(command: &str) -> Option<String> {
    let first = command.trim_start();
    let word = if let Some(rest) = first.strip_prefix('\'') {
        rest.split('\'').next()?
    } else if let Some(rest) = first.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        first.split_whitespace().next()?
    };
    Path::new(word)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Whether `command` is our hook for `tool`: `hocus agent-hook TOOL`,
/// wherever `hocus` is.
pub fn is_ours(command: &str, tool: Tool) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    program_of(command).as_deref() == Some("hocus")
        && words
            .windows(2)
            .any(|w| w[0] == "agent-hook" && w[1] == tool.word())
}

// --- Whether a tool is there ---------------------------------------------------------

/// What the desktop knows of a tool that hooks plug in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    /// Its directory or its program is there.
    pub found: bool,
    /// Its settings have our hooks.
    pub plugged: bool,
}

/// `tool` on this machine, as it is now.
pub fn seen(places: &Places, tool: Tool) -> Seen {
    let program = match tool {
        Tool::ClaudeCode => "claude",
        Tool::Codex => "codex",
    };
    let plugged = std::fs::read_to_string(places.settings(tool))
        .ok()
        .is_some_and(|text| has_ours(tool, &text));
    Seen {
        found: plugged || places.dir(tool).is_dir() || places.has_program(program),
        plugged,
    }
}

/// Whether herdr is on this machine: its program, or its directory.
pub fn herdr_found(places: &Places) -> bool {
    places.has_program("herdr") || places.herdr_dir().is_dir() || places.herdr_socket.exists()
}

/// Whether `text`, `tool`'s settings, has our hooks.
pub fn has_ours(tool: Tool, text: &str) -> bool {
    match tool {
        Tool::ClaudeCode => serde_json::from_str::<Value>(text)
            .ok()
            .is_some_and(|v| claude_has(&v)),
        Tool::Codex => text
            .parse::<toml_edit::DocumentMut>()
            .ok()
            .is_some_and(|d| codex_has(&d)),
    }
}

// --- Claude Code's settings.json ---------------------------------------------------

/// Claude Code's events our hook runs on, and their matchers.
const CLAUDE_EVENTS: [(&str, Option<&str>); 10] = [
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("AskUserQuestion|ExitPlanMode")),
    ("PermissionRequest", None),
    ("PostToolUse", None),
    ("PostToolUseFailure", None),
    (
        "Notification",
        Some("permission_prompt|idle_prompt|elicitation_dialog|elicitation_url_dialog"),
    ),
    ("Stop", None),
    ("StopFailure", None),
    ("SessionEnd", None),
];

fn claude_handlers(group: &Value) -> impl Iterator<Item = &Value> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn claude_is_ours(handler: &Value) -> bool {
    handler
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| is_ours(c, Tool::ClaudeCode))
}

/// Whether Claude Code's `settings` have our hooks.
pub fn claude_has(settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|events| events.values())
        .filter_map(Value::as_array)
        .flatten()
        .any(|group| claude_handlers(group).any(claude_is_ours))
}

/// Our hooks added to Claude Code's `settings`, running `command`: a
/// matcher group of ours at the end of each event's.
pub fn claude_add(settings: &mut Value, command: &str) -> Result<(), String> {
    let root = settings
        .as_object_mut()
        .ok_or("its settings are not a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("its \"hooks\" are not an object")?;
    for (event, matcher) in CLAUDE_EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("its \"{event}\" hooks are not a list"))?;
        let mut group = Map::new();
        if let Some(matcher) = matcher {
            group.insert("matcher".into(), json!(matcher));
        }
        group.insert(
            "hooks".into(),
            json!([{ "type": "command", "command": command, "timeout": TIMEOUT }]),
        );
        groups.push(Value::Object(group));
    }
    Ok(())
}

/// Our hooks taken out of Claude Code's `settings`, and what they leave
/// empty: whether anything was.
pub fn claude_remove(settings: &mut Value) -> bool {
    let Some(root) = settings.as_object_mut() else {
        return false;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut removed = false;
    for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = handlers.len();
                handlers.retain(|h| !claude_is_ours(h));
                removed |= handlers.len() != before;
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|h| !h.is_empty())
        });
    }
    if removed {
        hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
        if hooks.is_empty() {
            root.remove("hooks");
        }
    }
    removed
}

// --- Codex's config.toml -----------------------------------------------------------

/// Codex's events our hook runs on, and how long it may take.
const CODEX_EVENTS: [(&str, i64); 7] = [
    ("SessionStart", TIMEOUT),
    ("UserPromptSubmit", TIMEOUT),
    ("PermissionRequest", TIMEOUT),
    ("PostToolUse", TIMEOUT),
    ("Stop", TIMEOUT),
    ("Interrupt", SHORT_TIMEOUT),
    ("SessionEnd", SHORT_TIMEOUT),
];

fn codex_is_ours(handler: &dyn toml_edit::TableLike) -> bool {
    handler
        .get("command")
        .and_then(|c| c.as_str())
        .is_some_and(|c| is_ours(c, Tool::Codex))
}

/// The matcher groups of `event`'s item, as tables, written either way.
fn groups_of(item: &toml_edit::Item) -> Vec<&dyn toml_edit::TableLike> {
    match item {
        toml_edit::Item::ArrayOfTables(groups) => groups
            .iter()
            .map(|t| t as &dyn toml_edit::TableLike)
            .collect(),
        toml_edit::Item::Value(toml_edit::Value::Array(groups)) => groups
            .iter()
            .filter_map(|v| v.as_inline_table())
            .map(|t| t as &dyn toml_edit::TableLike)
            .collect(),
        _ => Vec::new(),
    }
}

fn handlers_of(group: &dyn toml_edit::TableLike) -> Vec<&dyn toml_edit::TableLike> {
    group.get("hooks").map(groups_of).unwrap_or_default()
}

fn is_our_notify(item: &toml_edit::Item) -> bool {
    let Some(words) = item.as_array() else {
        return false;
    };
    let words: Vec<&str> = words.iter().filter_map(|w| w.as_str()).collect();
    matches!(words.as_slice(), [program, "agent-hook", "codex", ..]
        if Path::new(program).file_name().is_some_and(|n| n == "hocus"))
}

/// Whether Codex's configuration has our hooks (or our `notify`).
pub fn codex_has(doc: &toml_edit::DocumentMut) -> bool {
    if doc.get("notify").is_some_and(is_our_notify) {
        return true;
    }
    doc.get("hooks")
        .and_then(|h| h.as_table_like())
        .into_iter()
        .flat_map(|events| events.iter().map(|(_, item)| item))
        .flat_map(groups_of)
        .any(|group| handlers_of(group).into_iter().any(codex_is_ours))
}

/// Our hooks added to Codex's configuration: a matcher group of ours at
/// the end of each event's, and `notify` if it has none.
pub fn codex_add(doc: &mut toml_edit::DocumentMut, places: &Places) -> Result<(), String> {
    use toml_edit::{ArrayOfTables, Item, Table, Value as V, value};
    let command = places.hook_command(Tool::Codex);
    if doc.get("notify").is_none() {
        let mut words = toml_edit::Array::new();
        words.push(places.hocus.display().to_string());
        words.push("agent-hook");
        words.push("codex");
        doc.insert("notify", value(words));
    }
    let hooks = doc
        .entry("hooks")
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_like_mut()
        .ok_or("its \"hooks\" are not a table")?;
    for (event, timeout) in CODEX_EVENTS {
        let mut handler = Table::new();
        handler.insert("type", value("command"));
        handler.insert("command", value(command.clone()));
        handler.insert("timeout", value(timeout));
        let item = hooks
            .entry(event)
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
        match item {
            Item::ArrayOfTables(groups) => {
                let mut handlers = ArrayOfTables::new();
                handlers.push(handler);
                let mut group = Table::new();
                group.insert("hooks", Item::ArrayOfTables(handlers));
                groups.push(group);
            }
            Item::Value(V::Array(groups)) => {
                let mut handlers = toml_edit::Array::new();
                handlers.push(handler.into_inline_table());
                let mut group = toml_edit::InlineTable::new();
                group.insert("hooks", V::Array(handlers));
                groups.push(group);
            }
            _ => return Err(format!("its \"{event}\" hooks are not a list")),
        }
    }
    Ok(())
}

/// Our hooks (and our `notify`) taken out of Codex's configuration, and
/// what they leave empty: whether anything was.
pub fn codex_remove(doc: &mut toml_edit::DocumentMut) -> bool {
    use toml_edit::{Item, Value as V};
    let mut removed = false;
    if doc.get("notify").is_some_and(is_our_notify) {
        doc.remove("notify");
        removed = true;
    }
    let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_table_like_mut()) else {
        return removed;
    };
    let mut empty = Vec::new();
    for (event, item) in hooks.iter_mut() {
        match item {
            Item::ArrayOfTables(groups) => {
                for group in groups.iter_mut() {
                    if let Some(Item::ArrayOfTables(handlers)) = group.get_mut("hooks") {
                        let before = handlers.len();
                        handlers.retain(|h| !codex_is_ours(h));
                        removed |= handlers.len() != before;
                    }
                }
                groups.retain(
                    |g| !matches!(g.get("hooks"), Some(Item::ArrayOfTables(h)) if h.is_empty()),
                );
                if groups.is_empty() {
                    empty.push(event.get().to_owned());
                }
            }
            Item::Value(V::Array(groups)) => {
                for group in groups.iter_mut() {
                    if let Some(V::Array(handlers)) =
                        group.as_inline_table_mut().and_then(|g| g.get_mut("hooks"))
                    {
                        let before = handlers.len();
                        handlers.retain(|h| h.as_inline_table().is_none_or(|t| !codex_is_ours(t)));
                        removed |= handlers.len() != before;
                    }
                }
                groups.retain(|g| {
                    !matches!(g.as_inline_table().and_then(|t| t.get("hooks")),
                        Some(V::Array(h)) if h.is_empty())
                });
                if groups.is_empty() {
                    empty.push(event.get().to_owned());
                }
            }
            _ => {}
        }
    }
    if removed {
        for event in empty {
            hooks.remove(&event);
        }
        if hooks.is_empty() {
            doc.remove("hooks");
        }
    }
    removed
}

// --- What is remembered ------------------------------------------------------------

/// What the desktop remembers between sessions: the tools the user
/// declined ("Ne rien brancher"), and what plugging wrote, by tool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    pub declined: BTreeSet<String>,
    pub written: BTreeMap<String, Written>,
}

/// What plugging a tool in wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    /// The file's hash as written (FNV-1a, 64 bits).
    pub hash: u64,
    /// There was no file before: unplugging removes it.
    pub created: bool,
}

impl Memory {
    pub fn load(places: &Places) -> Self {
        let Some(file) = places.state_dir.as_ref().map(|d| d.join(MEMORY_FILE)) else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Self::default();
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            tracing::warn!(file = %file.display(), "the agents' memory does not read: forgotten");
            return Self::default();
        };
        let declined = doc
            .get("declined")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        let written = doc
            .get("written")
            .and_then(|w| w.as_table_like())
            .into_iter()
            .flat_map(|t| t.iter())
            .filter_map(|(tool, item)| {
                let hash = u64::from_str_radix(item.get("hash")?.as_str()?, 16).ok()?;
                let created = item
                    .get("created")
                    .and_then(|c| c.as_bool())
                    .unwrap_or(false);
                Some((tool.to_owned(), Written { hash, created }))
            })
            .collect();
        Self { declined, written }
    }

    pub fn text(&self) -> String {
        let mut text = String::from(
            "# What Hocuspanda remembers of the agents' tools: those declined in the\n\
             # bar (\"Plug nothing in\"), and what plugging one in wrote.\n",
        );
        let declined: Vec<String> = self.declined.iter().map(|d| format!("{d:?}")).collect();
        text.push_str(&format!("declined = [{}]\n", declined.join(", ")));
        for (tool, written) in &self.written {
            text.push_str(&format!(
                "\n[written.{tool}]\nhash = \"{:016x}\"\ncreated = {}\n",
                written.hash, written.created
            ));
        }
        text
    }

    pub fn save(&self, places: &Places) {
        let Some(dir) = &places.state_dir else {
            return;
        };
        let file = dir.join(MEMORY_FILE);
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| write_whole(&file, self.text().as_bytes(), None));
        if let Err(err) = written {
            tracing::warn!(file = %file.display(), %err, "could not remember the agents' tools");
        }
    }
}

/// FNV-1a, 64 bits: what was written, recognised again.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

// --- Plugging in and out ------------------------------------------------------------

/// The file a tool's settings are in: what a link points to, so that a
/// settings file kept elsewhere (a dotfiles repository) stays a link.
fn real(file: &Path) -> PathBuf {
    match std::fs::symlink_metadata(file) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
        }
        _ => file.to_path_buf(),
    }
}

fn copy_of(file: &Path) -> PathBuf {
    let mut copy = file.as_os_str().to_owned();
    copy.push(COPY_SUFFIX);
    PathBuf::from(copy)
}

/// `bytes` into `file` whole, through a file beside it renamed over it, so
/// that the tool never reads it half written; with `mode`'s permissions.
fn write_whole(file: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let mut temp = file.as_os_str().to_owned();
    temp.push(".hocuspanda-new");
    let temp = PathBuf::from(temp);
    let written = (|| {
        let mut out = std::fs::File::create(&temp)?;
        out.write_all(bytes)?;
        out.sync_all()?;
        if let Some(mode) = mode {
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode))?;
        }
        std::fs::rename(&temp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// `tool`'s settings with our hooks added to `original` (none: no file).
pub fn plugged_text(tool: Tool, original: Option<&str>, places: &Places) -> Result<String, String> {
    let original = original.unwrap_or_default();
    match tool {
        Tool::ClaudeCode => {
            let mut settings = if original.trim().is_empty() {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(original).map_err(|e| format!("it does not read: {e}"))?
            };
            claude_add(&mut settings, &places.hook_command(tool))?;
            let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
            Ok(format!("{text}\n"))
        }
        Tool::Codex => {
            let mut doc = original
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("it does not read: {e}"))?;
            codex_add(&mut doc, places)?;
            Ok(doc.to_string())
        }
    }
}

/// `tool`'s settings with our hooks taken out of `text`; `None` when there
/// were none, or it does not read.
pub fn unplugged_text(tool: Tool, text: &str) -> Option<String> {
    match tool {
        Tool::ClaudeCode => {
            let mut settings: Value = serde_json::from_str(text).ok()?;
            claude_remove(&mut settings).then_some(())?;
            serde_json::to_string_pretty(&settings)
                .ok()
                .map(|t| format!("{t}\n"))
        }
        Tool::Codex => {
            let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
            codex_remove(&mut doc).then_some(())?;
            Some(doc.to_string())
        }
    }
}

/// Plugs `tool` in: its hooks into its settings, a copy of the file as it
/// was kept beside it. Nothing is written if the file does not read.
pub fn plug(tool: Tool, places: &Places, memory: &mut Memory) -> Result<(), String> {
    let file = real(&places.settings(tool));
    let original = match std::fs::read(&file) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("{}: {err}", file.display())),
    };
    let text = original
        .as_deref()
        .map(|b| {
            String::from_utf8(b.to_vec()).map_err(|_| format!("{}: not UTF-8", file.display()))
        })
        .transpose()?;
    if text.as_deref().is_some_and(|t| has_ours(tool, t)) {
        memory.declined.remove(tool.word());
        return Ok(());
    }
    let new = plugged_text(tool, text.as_deref(), places)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    let mode = std::fs::metadata(&file)
        .ok()
        .map(|m| m.permissions().mode());
    if let Some(original) = &original {
        write_whole(&copy_of(&file), original, mode).map_err(io)?;
    } else if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    write_whole(&file, new.as_bytes(), mode).map_err(io)?;
    tracing::info!(tool = tool.word(), file = %file.display(), "plugged in");
    memory.declined.remove(tool.word());
    memory.written.insert(
        tool.word().to_owned(),
        Written {
            hash: hash(new.as_bytes()),
            created: original.is_none(),
        },
    );
    memory.save(places);
    Ok(())
}

/// Unplugs `tool`: the file as it was when it is still as written, else
/// only our entries taken out.
pub fn unplug(tool: Tool, places: &Places, memory: &mut Memory) -> Result<(), String> {
    let file = real(&places.settings(tool));
    let copy = copy_of(&file);
    let written = memory.written.remove(tool.word());
    memory.save(places);
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    let current = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(io(err)),
    };
    let mode = std::fs::metadata(&file)
        .ok()
        .map(|m| m.permissions().mode());
    match written {
        Some(w) if w.hash == hash(&current) && w.created => {
            std::fs::remove_file(&file).map_err(io)?;
            tracing::info!(tool = tool.word(), file = %file.display(), "unplugged: the file we made removed");
            return Ok(());
        }
        Some(w) if w.hash == hash(&current) && copy.exists() => {
            let original = std::fs::read(&copy).map_err(io)?;
            write_whole(&file, &original, mode).map_err(io)?;
            std::fs::remove_file(&copy).map_err(io)?;
            tracing::info!(tool = tool.word(), file = %file.display(), "unplugged: the file as it was");
            return Ok(());
        }
        _ => {}
    }
    let text = String::from_utf8_lossy(&current);
    if let Some(new) = unplugged_text(tool, &text) {
        write_whole(&file, new.as_bytes(), mode).map_err(io)?;
        tracing::info!(
            tool = tool.word(),
            file = %file.display(),
            "unplugged: the file changed since, only our hooks taken out"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(dir: &Path) -> Places {
        Places {
            home: dir.to_path_buf(),
            config_home: dir.join(".config"),
            codex_home: dir.join(".codex"),
            path: vec![dir.join("bin")],
            herdr_socket: dir.join(".config/herdr/herdr.sock"),
            processes: dir.join("proc"),
            hocus: PathBuf::from("/usr/bin/hocus"),
            state_dir: Some(dir.join("state")),
        }
    }

    #[test]
    fn our_hook_is_known_wherever_hocus_is() {
        assert!(is_ours(
            "/usr/bin/hocus agent-hook claude-code",
            Tool::ClaudeCode
        ));
        assert!(is_ours("'/opt/my dir/hocus' agent-hook codex", Tool::Codex));
        assert!(!is_ours(
            "/usr/bin/hocus agent-hook codex",
            Tool::ClaudeCode
        ));
        assert!(!is_ours("notify-send done", Tool::ClaudeCode));
        assert!(!is_ours(
            "hocus-other agent-hook claude-code",
            Tool::ClaudeCode
        ));
        assert_eq!(
            quoted(Path::new("/opt/my dir/hocus")),
            "'/opt/my dir/hocus'"
        );
    }

    #[test]
    fn claude_codes_hooks_go_beside_the_users_and_come_out_alone() {
        let p = places(Path::new("/home/alex"));
        let original = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "command": "notify-send done" } ] }
    ]
  }
}"#;
        let text = plugged_text(Tool::ClaudeCode, Some(original), &p).expect("plugged");
        let v: Value = serde_json::from_str(&text).expect("JSON");
        // The user's keys first, in their order; their hook kept.
        assert_eq!(
            v.as_object().map(|o| o.keys().next().cloned()),
            Some(Some("model".into()))
        );
        let stop = v["hooks"]["Stop"].as_array().expect("Stop");
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "notify-send done");
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            "/usr/bin/hocus agent-hook claude-code"
        );
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["matcher"],
            "AskUserQuestion|ExitPlanMode"
        );
        assert!(has_ours(Tool::ClaudeCode, &text));
        assert!(!has_ours(Tool::ClaudeCode, original));
        // Out again: the user's alone, the events we added gone.
        let back = unplugged_text(Tool::ClaudeCode, &text).expect("unplugged");
        let back: Value = serde_json::from_str(&back).expect("JSON");
        let before: Value = serde_json::from_str(original).expect("JSON");
        assert_eq!(back, before);
        // No file: ours alone; out, an empty object.
        let fresh = plugged_text(Tool::ClaudeCode, None, &p).expect("plugged");
        let out = unplugged_text(Tool::ClaudeCode, &fresh).expect("unplugged");
        assert_eq!(out.trim(), "{}");
        // Broken: never written.
        assert!(plugged_text(Tool::ClaudeCode, Some("{ \"model\": "), &p).is_err());
        assert!(plugged_text(Tool::ClaudeCode, Some("[1, 2]"), &p).is_err());
    }

    #[test]
    fn codexs_hooks_and_notify_keep_the_users_comments() {
        let p = places(Path::new("/home/alex"));
        let original =
            "# my settings\nmodel = \"gpt-5\" # the best\n\n[tui]\nnotifications = true\n";
        let text = plugged_text(Tool::Codex, Some(original), &p).expect("plugged");
        assert!(
            text.starts_with("# my settings\nmodel = \"gpt-5\" # the best\n"),
            "{text}"
        );
        assert!(
            text.contains("notify = [\"/usr/bin/hocus\", \"agent-hook\", \"codex\"]"),
            "{text}"
        );
        assert!(text.contains("[[hooks.PermissionRequest]]"), "{text}");
        assert!(text.contains("[[hooks.PermissionRequest.hooks]]"), "{text}");
        assert!(
            text.contains("command = \"/usr/bin/hocus agent-hook codex\""),
            "{text}"
        );
        let doc: toml_edit::DocumentMut = text.parse().expect("TOML");
        assert!(codex_has(&doc));
        let back = unplugged_text(Tool::Codex, &text).expect("unplugged");
        assert_eq!(
            back.parse::<toml_edit::DocumentMut>()
                .expect("TOML")
                .to_string(),
            back
        );
        assert!(back.contains("# the best"));
        assert!(
            !back.contains("hooks") && !back.contains("notify"),
            "{back}"
        );
        // The user's own notify stays, and theirs in inline hooks too.
        let theirs = "notify = [\"notify-send\"]\nhooks = { Stop = [ { hooks = [ { type = \"command\", command = \"say done\" } ] } ] }\n";
        let text = plugged_text(Tool::Codex, Some(theirs), &p).expect("plugged");
        assert!(text.contains("notify = [\"notify-send\"]"));
        assert!(has_ours(Tool::Codex, &text));
        let back = unplugged_text(Tool::Codex, &text).expect("unplugged");
        assert!(
            back.contains("say done") && !back.contains("agent-hook"),
            "{back}"
        );
        assert!(plugged_text(Tool::Codex, Some("model = "), &p).is_err());
    }

    #[test]
    fn plugging_in_keeps_a_copy_and_unplugging_puts_it_back() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let p = places(dir.path());
        let file = p.settings(Tool::ClaudeCode);
        std::fs::create_dir_all(file.parent().ok_or("no dir")?).map_err(|e| e.to_string())?;
        let original = "{\n    \"model\": \"opus\"\n}";
        std::fs::write(&file, original).map_err(|e| e.to_string())?;
        let mut memory = Memory::default();
        memory.declined.insert("claude-code".into());
        plug(Tool::ClaudeCode, &p, &mut memory)?;
        assert!(seen(&p, Tool::ClaudeCode).plugged);
        assert!(memory.declined.is_empty());
        assert_eq!(
            std::fs::read_to_string(copy_of(&file)).map_err(|e| e.to_string())?,
            original
        );
        // Remembered across sessions.
        assert_eq!(Memory::load(&p), memory);
        unplug(Tool::ClaudeCode, &p, &mut memory)?;
        assert_eq!(
            std::fs::read_to_string(&file).map_err(|e| e.to_string())?,
            original
        );
        assert!(!copy_of(&file).exists());
        // Changed since: only ours out, the user's change kept.
        plug(Tool::ClaudeCode, &p, &mut memory)?;
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        std::fs::write(&file, text.replace("\"opus\"", "\"sonnet\"")).map_err(|e| e.to_string())?;
        unplug(Tool::ClaudeCode, &p, &mut memory)?;
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        assert!(
            text.contains("sonnet") && !text.contains("agent-hook"),
            "{text}"
        );
        // No file before: none after.
        let codex = p.settings(Tool::Codex);
        plug(Tool::Codex, &p, &mut memory)?;
        assert!(codex.exists());
        unplug(Tool::Codex, &p, &mut memory)?;
        assert!(!codex.exists());
        // A file that does not read is left alone.
        std::fs::write(&file, "{ broken").map_err(|e| e.to_string())?;
        assert!(plug(Tool::ClaudeCode, &p, &mut memory).is_err());
        assert_eq!(
            std::fs::read_to_string(&file).map_err(|e| e.to_string())?,
            "{ broken"
        );
        Ok(())
    }
}
