//! Agir sur le bureau (#440, docs/design/mcp.md, scene 5): the agents that
//! act on the desktop through `hocus mcp --agent NAME`, their rights, and
//! how they are plugged in.
//!
//! Every agent is a `[[agents.mcp]]` table of the file, written in place
//! by its name (the store's `["agents", "mcp", NAME, …]` keys), which the
//! compositor follows at its next call. `[agents] confirm-apps` and
//! `show-before-ms` are `[agents]`' keys, their defaults the shipped
//! file's. « Brancher » for Claude Code runs `claude mcp add` (on a
//! thread, only after its click) and « Débrancher » `claude mcp remove`.

use hocus_words::settings::acting as words;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

use crate::{
    lang::{Lang, Words},
    model::{Outside, SectionId},
};

/// The page's own rows, for the search and the lit row.
pub const CARDS: &str = "act.agents";
pub const CONFIRM: &str = "act.confirm-apps";
pub const SHOW: &str = "act.show-before";
pub const PLUG: &str = "act.plug";

/// What Claude Code names the server in its configuration.
pub const SERVER: &str = "hocuspanda";
/// The program each agent's server is.
pub const HOCUS: &str = "hocus";
/// What `client` says of an agent plugged into Claude Code.
pub const CLAUDE_CODE: &str = "claude-code";
/// The name Claude Code's agent takes here.
pub const CLAUDE: &str = "Claude";
/// The name another client's agent takes here (a number after it when
/// taken).
pub const AGENT: &str = "Agent";

/// One of an acting agent's rights: a check box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Right {
    Windows,
    Desktops,
    Settings,
    Input,
}

impl Right {
    pub const ALL: [Self; 4] = [Self::Windows, Self::Desktops, Self::Settings, Self::Input];

    /// Its key in the agent's table.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Desktops => "desktops",
            Self::Settings => "settings",
            Self::Input => "input",
        }
    }

    pub const fn label(self) -> Words {
        match self {
            Self::Windows => words::WINDOWS,
            Self::Desktops => words::DESKTOPS,
            Self::Settings => words::SETTINGS,
            Self::Input => words::INPUT,
        }
    }

    pub const fn line(self) -> Option<Words> {
        match self {
            Self::Windows => Some(words::WINDOWS_LINE),
            Self::Desktops => None,
            Self::Settings => Some(words::SETTINGS_LINE),
            Self::Input => Some(words::INPUT_LINE),
        }
    }

    /// What it is when the table does not say (the compositor's).
    pub const fn default(self) -> bool {
        !matches!(self, Self::Settings)
    }
}

/// An agent of `[[agents.mcp]]`, as its card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Agent {
    pub name: String,
    /// `access = "act"`.
    pub acts: bool,
    pub windows: bool,
    pub desktops: bool,
    pub settings: bool,
    pub input: bool,
    pub apps: Vec<String>,
    pub client: Option<String>,
    /// `"2026-11-12"`.
    pub since: Option<String>,
}

impl Agent {
    pub fn holds(&self, right: Right) -> bool {
        match right {
            Right::Windows => self.windows,
            Right::Desktops => self.desktops,
            Right::Settings => self.settings,
            Right::Input => self.input,
        }
    }

    /// Plugged into Claude Code by the page.
    pub fn in_claude_code(&self) -> bool {
        self.client.as_deref() == Some(CLAUDE_CODE)
    }

    /// Its card's title: its client's name, else its own.
    pub fn title(&self) -> String {
        if self.in_claude_code() {
            words::CLAUDE_CODE.get(Lang::English).to_owned()
        } else {
            self.name.clone()
        }
    }

    /// Its mark's letter.
    pub fn initial(&self) -> String {
        self.title()
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default()
    }

    fn of(table: &dyn TableLike) -> Option<Self> {
        let flag = |key: &str, right: Right| {
            table
                .get(key)
                .and_then(Item::as_bool)
                .unwrap_or(right.default())
        };
        let text = |key: &str| table.get(key).and_then(Item::as_str).map(ToOwned::to_owned);
        Some(Self {
            name: text("name")?,
            acts: table.get("access").and_then(Item::as_str) == Some("act"),
            windows: flag("windows", Right::Windows),
            desktops: flag("desktops", Right::Desktops),
            settings: flag("settings", Right::Settings),
            input: flag("input", Right::Input),
            apps: texts(table.get("apps")).unwrap_or_default(),
            client: text("client"),
            since: text("since"),
        })
    }

    /// Its table, as a new one is written: read-only, its client and day.
    pub fn toml(&self) -> String {
        let quoted = |t: &str| toml_edit::Value::from(t).to_string();
        let mut fields = vec![
            format!("name = {}", quoted(&self.name)),
            format!(
                "access = {}",
                quoted(if self.acts { "act" } else { "read" })
            ),
        ];
        if let Some(client) = &self.client {
            fields.push(format!("client = {}", quoted(client)));
        }
        if let Some(since) = &self.since {
            fields.push(format!("since = {}", quoted(since)));
        }
        format!("{{ {} }}", fields.join(", "))
    }
}

fn texts(item: Option<&Item>) -> Option<Vec<String>> {
    item?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(ToOwned::to_owned))
        .collect()
}

/// What the file says of the agents that act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub agents: Vec<Agent>,
    pub confirm_apps: Vec<String>,
    pub show_before_ms: i64,
}

impl Default for File {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            confirm_apps: default_confirm_apps(),
            show_before_ms: default_show_before(),
        }
    }
}

/// `[agents] confirm-apps`' default, the shipped file's.
pub fn default_confirm_apps() -> Vec<String> {
    hocus_config::defaults::value(&["agents", "confirm-apps"])
        .and_then(|v| {
            v.as_array()?
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `[agents] show-before-ms`' default, the shipped file's.
pub fn default_show_before() -> i64 {
    hocus_config::defaults::value(&["agents", "show-before-ms"])
        .and_then(toml_edit::Value::as_integer)
        .unwrap_or(600)
}

impl File {
    pub fn of(doc: &DocumentMut) -> Self {
        let agents_table = doc.get("agents");
        let agents = match agents_table.and_then(|a| a.get("mcp")) {
            Some(Item::ArrayOfTables(tables)) => {
                tables.iter().filter_map(|t| Agent::of(t)).collect()
            }
            _ => Vec::new(),
        };
        Self {
            agents,
            confirm_apps: texts(agents_table.and_then(|a| a.get("confirm-apps")))
                .unwrap_or_else(default_confirm_apps),
            show_before_ms: agents_table
                .and_then(|a| a.get("show-before-ms"))
                .and_then(Item::as_integer)
                .unwrap_or_else(default_show_before),
        }
    }

    pub fn agent(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// The name another client's agent takes: "Agent", else "Agent 2"…
    pub fn new_name(&self) -> String {
        (1..)
            .map(|n| {
                if n == 1 {
                    AGENT.to_owned()
                } else {
                    format!("{AGENT} {n}")
                }
            })
            .find(|name| self.agent(name).is_none())
            .unwrap_or_else(|| AGENT.to_owned())
    }
}

/// A key of agent `name`'s table: `["agents", "mcp", NAME, KEY]`.
pub fn key(name: &str, field: &str) -> Vec<String> {
    vec!["agents".into(), "mcp".into(), name.into(), field.into()]
}

/// Agent `name`'s whole table.
pub fn table_key(name: &str) -> Vec<String> {
    vec!["agents".into(), "mcp".into(), name.into()]
}

/// A list of texts as the file writes it.
pub fn list_toml(items: &[String]) -> String {
    toml_edit::Value::Array(items.iter().map(String::as_str).collect()).to_string()
}

/// What Claude Code is asked to add: its server, in all the user's
/// projects (`--scope user`).
pub fn claude_add(name: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "mcp", "add", "--scope", "user", SERVER, "--", HOCUS, "mcp", "--agent",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    args.push(name.to_owned());
    args
}

/// What Claude Code is asked to remove.
pub fn claude_remove() -> Vec<String> {
    ["mcp", "remove", "--scope", "user", SERVER]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// The line to paste in a terminal.
pub fn claude_line(name: &str) -> String {
    let mut line = vec!["claude".to_owned()];
    line.extend(claude_add(name).into_iter().map(|a| shell_word(&a)));
    line.join(" ")
}

/// `word` as a shell reads it back.
fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=".contains(c))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The server, as an MCP client's configuration names it.
pub fn json_snippet(name: &str) -> String {
    let quoted = |t: &str| serde_json::Value::from(t).to_string();
    format!(
        "{{\n  \"mcpServers\": {{\n    {}: {{\n      \"command\": {},\n      \"args\": [\"mcp\", \"--agent\", {}]\n    }}\n  }}\n}}",
        quoted(SERVER),
        quoted(HOCUS),
        quoted(name)
    )
}

/// `claude` on `path` (the `PATH`'s directories), if it is there.
pub fn find_claude(path: &std::ffi::OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    std::env::split_paths(path)
        .map(|dir| dir.join("claude"))
        .find(|file| {
            std::fs::metadata(file)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// `claude` run with `args`, on the calling thread (never the window's):
/// what went wrong, in its words, if it did. 20 s at most.
pub fn run_claude(claude: &Path, args: &[String]) -> Result<(), String> {
    use std::{process::Stdio, time::Duration};
    let mut child = std::process::Command::new(claude)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut said = String::new();
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read as _;
                    let _ = err.read_to_string(&mut said);
                }
                let said = said
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or_default();
                return Err(if said.is_empty() {
                    status.to_string()
                } else {
                    said.trim().to_owned()
                });
            }
            Ok(None) if started.elapsed() > Duration::from_secs(20) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("no answer in 20 s".to_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(err.to_string()),
        }
    }
}

/// Today, as `since` writes it.
pub fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

/// "12 novembre", from `since`.
pub fn since_said(since: &str, lang: Lang) -> Option<String> {
    let date: jiff::civil::Date = since.parse().ok()?;
    let month = usize::try_from(date.month() - 1).ok()?;
    let month = hocus_words::date::MONTHS.get(lang).get(month)?;
    Some(hocus_words::settings::agents::day_month(
        lang,
        hocus_words::settings::agents::DayMonth {
            day: date.day(),
            month,
        },
    ))
}

/// The line under a card's title.
pub fn card_line(agent: &Agent, lang: Lang) -> String {
    let mut parts = Vec::new();
    if let Some(day) = agent.since.as_deref().and_then(|s| since_said(s, lang)) {
        parts.push(words::plugged_on(lang, day));
    }
    if agent.in_claude_code() {
        parts.push(words::EVERYWHERE.get(lang).to_owned());
    }
    if parts.is_empty() {
        let mut by_hand = words::BY_HAND.get(lang).to_owned();
        if let Some(first) = by_hand.get(..1) {
            by_hand = first.to_uppercase() + by_hand.get(1..).unwrap_or_default();
        }
        parts.push(by_hand);
    }
    parts.join(" · ")
}

/// `ms` as the show-before row says it: "0,6 s".
pub fn seconds_said(ms: i64, lang: Lang) -> String {
    let text = format!("{:.1}", ms as f64 / 1000.0);
    let text = text.strip_suffix(".0").unwrap_or(&text).to_owned();
    let text = if lang == Lang::French {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} s")
}

/// The page's rows, for the search.
pub fn outside() -> &'static [Outside] {
    const ALL: [Outside; 4] = [
        Outside {
            id: CARDS,
            section: SectionId::Acting,
            group: words::WHAT_IT_MAY_DO,
            name: words::WHAT_IT_MAY_DO,
            keywords: (
                &[
                    "mcp",
                    "agent",
                    "droits",
                    "regarder",
                    "agir",
                    "fenêtres",
                    "claude",
                    "débrancher",
                ],
                &[
                    "mcp", "agent", "rights", "look", "act", "windows", "claude", "unplug",
                ],
            ),
        },
        Outside {
            id: CONFIRM,
            section: SectionId::Acting,
            group: words::CONFIRM_LABEL,
            name: words::CONFIRM_LABEL,
            keywords: (
                &["demander", "confirmer", "mots de passe", "confirm-apps"],
                &["ask", "confirm", "passwords", "confirm-apps"],
            ),
        },
        Outside {
            id: SHOW,
            section: SectionId::Acting,
            group: words::SHOW_BEFORE,
            name: words::SHOW_BEFORE,
            keywords: (
                &["montrer", "pointeur", "intention", "show-before-ms"],
                &["show", "pointer", "intent", "show-before-ms"],
            ),
        },
        Outside {
            id: PLUG,
            section: SectionId::Acting,
            group: words::PLUG_ANOTHER,
            name: words::PLUG_ANOTHER,
            keywords: (
                &["brancher", "claude code", "codex", "client mcp", "serveur"],
                &["plug in", "claude code", "codex", "mcp client", "server"],
            ),
        },
    ];
    &ALL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_s_agents_read_with_the_compositor_s_defaults() {
        let doc = hocus_config::parse(
            "[agents]\nshow-before-ms = 0\n[[agents.mcp]]\nname = \"Claude\"\naccess = \"act\"\n\
             settings = true\napps = [\"kitty\"]\nclient = \"claude-code\"\nsince = \"2026-11-12\"\n\
             [[agents.mcp]]\nname = \"look\"\n",
        )
        .expect("reads");
        let file = File::of(&doc);
        assert_eq!(file.show_before_ms, 0);
        assert!(file.confirm_apps.contains(&"hocus-settings".to_owned()));
        let claude = file.agent("Claude").expect("Claude");
        assert!(claude.acts && claude.windows && claude.settings && claude.input);
        assert_eq!(claude.title(), "Claude Code");
        assert_eq!(
            card_line(claude, Lang::French),
            "Branché le 12 novembre · dans tous tes projets"
        );
        let look = file.agent("look").expect("look");
        assert!(!look.acts && !look.settings && look.windows);
        assert_eq!(card_line(look, Lang::French), "Nommé dans ta configuration");
        assert_eq!(file.new_name(), "Agent");
        assert_eq!(File::default().show_before_ms, 600);
        assert_eq!(seconds_said(600, Lang::French), "0,6 s");
        assert_eq!(seconds_said(1000, Lang::English), "1 s");
    }

    #[test]
    fn claude_code_is_asked_for_its_user_s_scope() {
        assert_eq!(
            claude_line("Claude"),
            "claude mcp add --scope user hocuspanda -- hocus mcp --agent Claude"
        );
        assert_eq!(
            claude_line("Mon agent"),
            "claude mcp add --scope user hocuspanda -- hocus mcp --agent 'Mon agent'"
        );
        let agent = Agent {
            name: "Claude".into(),
            client: Some(CLAUDE_CODE.into()),
            since: Some("2026-11-12".into()),
            ..Agent::default()
        };
        assert_eq!(
            agent.toml(),
            "{ name = \"Claude\", access = \"read\", client = \"claude-code\", since = \"2026-11-12\" }"
        );
        let snippet: serde_json::Value =
            serde_json::from_str(&json_snippet("Mon \"agent\"")).expect("JSON");
        assert_eq!(
            snippet["mcpServers"]["hocuspanda"]["args"],
            serde_json::json!(["mcp", "--agent", "Mon \"agent\""])
        );
    }
}
