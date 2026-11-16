//! What an agent left running after its turn (#831, docs/agents.md, "In
//! the background"): its turn over, something it started still runs, and
//! it will come back by itself. The bar then says « en fond » rather than
//! « repos » once the user looked.
//!
//! Claude Code says it; the other tools' agents are looked at:
//!
//! - **Claude Code's hooks**: its `Stop` and `SubagentStop` list the
//!   session's tasks still in flight (`background_tasks`: a command, a
//!   background sub-agent, a monitor, a workflow…), which [`Running::Tasks`]
//!   counts; `SubagentStart` and `SubagentStop` count the sub-agents still
//!   running, for a Claude Code whose events carry no list. Its scheduled
//!   wake-ups (`session_crons`) do not count: nothing of them runs until
//!   they fire, which may be days away. Its processes are never read: a
//!   language server, an MCP server's relay, a shell left over are not
//!   work it waits for.
//! - **Its processes** (Codex): the agent's process's children still
//!   running that are not its MCP servers nor its hooks ([`count`]), read
//!   in `/proc` off the compositor's loop. An MCP server is a child
//!   started before the agent's first turn, or one whose command line is
//!   one of the agent's configured MCP servers' ([`mcp_servers`]). A child
//!   younger than [`YOUNG`] is not counted yet, only noted: a hook of the
//!   user's running at `Stop` is gone a moment later.

use std::{fmt, path::Path, str::FromStr, time::Duration};

use crate::{BadLine, Source};

/// A child younger than this is not counted yet.
pub const YOUNG: Duration = Duration::from_secs(1);

/// What a hook says runs on beside its agent's turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Running {
    /// A sub-agent started: its id.
    SubagentStarted(String),
    /// A sub-agent ended: its id.
    SubagentStopped(String),
    /// The session's tasks still in flight, as its turn or a sub-agent
    /// ended: all there are, sub-agents among them.
    Tasks(u32),
}

/// The line a hook writes for it: `ran SOURCE KEY PID WHAT`, `WHAT` being
/// `subagent-start ID`, `subagent-stop ID` or `tasks N`; `-` for an
/// unknown process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub source: Source,
    /// The agent's session, as its reports name it.
    pub key: String,
    pub pid: Option<u32>,
    pub running: Running,
}

impl fmt::Display for Ran {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pid = self.pid.map_or_else(|| "-".to_owned(), |p| p.to_string());
        write!(
            f,
            "ran {} {} {pid} ",
            self.source.name(),
            crate::key_word(&self.key)
        )?;
        match &self.running {
            Running::SubagentStarted(id) => write!(f, "subagent-start {}", crate::key_word(id)),
            Running::SubagentStopped(id) => write!(f, "subagent-stop {}", crate::key_word(id)),
            Running::Tasks(n) => write!(f, "tasks {n}"),
        }
    }
}

impl FromStr for Ran {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim_end_matches(['\n', '\r']);
        let words: Vec<&str> = line.split(' ').collect();
        let bad = || BadLine(format!("not a ran line: {line:?}"));
        let (source, key, pid, what) = match words.as_slice() {
            ["ran", source, key, pid, what @ ..] if !key.is_empty() => (source, key, pid, what),
            _ => return Err(bad()),
        };
        let pid = match *pid {
            "-" => None,
            pid => Some(
                pid.parse()
                    .map_err(|e| BadLine(format!("a process: {e}")))?,
            ),
        };
        let running = match what {
            ["subagent-start", id] if !id.is_empty() => Running::SubagentStarted((*id).to_owned()),
            ["subagent-stop", id] if !id.is_empty() => Running::SubagentStopped((*id).to_owned()),
            ["tasks", n] => Running::Tasks(
                n.parse()
                    .map_err(|e| BadLine(format!("a number of tasks: {e}")))?,
            ),
            _ => return Err(bad()),
        };
        Ok(Self {
            source: source.parse()?,
            key: (*key).to_owned(),
            pid,
            running,
        })
    }
}

/// A process as the scan reads it: its parent, when it started (clock
/// ticks since boot, `/proc`'s), whether it is a zombie, its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    pub start: u64,
    pub zombie: bool,
    pub argv: Vec<String>,
}

/// `stat`'s parent, state and start: the 4th, 3rd and 22nd fields, after
/// the name in brackets (which may hold spaces and brackets).
fn parse_stat(stat: &str) -> Option<(u32, bool, u64)> {
    let rest = stat.get(stat.rfind(')')? + 1..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let state = fields.first()?;
    let parent = fields.get(1)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    Some((parent, matches!(*state, "Z" | "X"), start))
}

/// The processes `proc` (a `/proc`) shows; those that end meanwhile are
/// left out.
pub fn processes(proc: &Path) -> Vec<Proc> {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let (parent, zombie, start) = parse_stat(&stat)?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
            let argv = cmdline
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            Some(Proc {
                pid,
                parent,
                start,
                zombie,
                argv,
            })
        })
        .collect()
}

/// Now on `/proc`'s clock: clock ticks since boot (`CLOCK_BOOTTIME`, which
/// a process's start counts from), and how many ticks make a second.
pub fn now_ticks() -> (u64, u64) {
    let per_second = rustix::param::clock_ticks_per_second().max(1);
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let ticks = (now.tv_sec as u64)
        .saturating_mul(per_second)
        .saturating_add(now.tv_nsec as u64 * per_second / 1_000_000_000);
    (ticks, per_second)
}

/// An agent whose processes are looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// The compositor's id for it.
    pub id: u64,
    pub pid: u32,
    /// When its first turn started (else when it was first heard of), in
    /// clock ticks since boot: its children started before are its MCP
    /// servers.
    pub since: u64,
    /// Its configured MCP servers' command lines.
    pub servers: Vec<Vec<String>>,
}

/// What a look at an agent's children found: those that count, and those
/// too young to count yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Found {
    pub counted: usize,
    pub young: usize,
}

impl Found {
    /// Whether something runs that counts.
    pub fn runs(self) -> bool {
        self.counted > 0
    }

    /// Whether it is worth looking again: something runs, or may.
    pub fn again(self) -> bool {
        self.counted + self.young > 0
    }
}

/// `watched`'s children among `procs` that are left running, at `now`
/// (`per_second` ticks a second): neither started before its first turn,
/// nor one of its MCP servers, nor its hooks (`hocus agent-hook`, and the
/// shell running it), nor a zombie.
pub fn count(procs: &[Proc], watched: &Watched, (now, per_second): (u64, u64)) -> Found {
    let young = YOUNG.as_millis() as u64 * per_second / 1000;
    let mut found = Found::default();
    for child in procs.iter().filter(|p| p.parent == watched.pid) {
        let hook = child.argv.iter().any(|a| a.contains("agent-hook"));
        let server = watched.servers.iter().any(|s| matches(&child.argv, s));
        if child.zombie || hook || server || child.start <= watched.since {
            continue;
        }
        if now.saturating_sub(child.start) >= young {
            found.counted += 1;
        } else {
            found.young += 1;
        }
    }
    found
}

/// Whether `argv` runs `server` (a command and its arguments): its command
/// (by its file's name) followed by its arguments, anywhere in it, as
/// `node /usr/bin/npx -y @scope/server` runs `npx -y @scope/server`
/// through its script's interpreter.
pub fn matches(argv: &[String], server: &[String]) -> bool {
    let Some((command, args)) = server.split_first() else {
        return false;
    };
    let name = |word: &str| word.rsplit('/').next().unwrap_or(word).to_owned();
    let command = name(command);
    argv.iter().enumerate().any(|(i, word)| {
        name(word) == command
            && argv
                .get(i + 1..i + 1 + args.len())
                .is_some_and(|a| a == args)
    })
}

/// The MCP servers' command lines Codex's `config.toml` (`text`)
/// configures: its `[mcp_servers.NAME]` run by a `command`.
pub fn mcp_servers(text: &str) -> Vec<Vec<String>> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(servers) = doc.get("mcp_servers").and_then(|s| s.as_table_like()) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(_, server)| {
            let server = server.as_table_like()?;
            let command = server
                .get("command")
                .and_then(|c| c.as_str())
                .filter(|c| !c.is_empty())?;
            let args = server
                .get("args")
                .and_then(|a| a.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned));
            Some(std::iter::once(command.to_owned()).chain(args).collect())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn a_ran_line_reads_back_as_written() {
        let lines = [
            Ran {
                source: Source::ClaudeCode,
                key: "abc123".into(),
                pid: Some(4242),
                running: Running::SubagentStarted("agent-def456".into()),
            },
            Ran {
                source: Source::ClaudeCode,
                key: "abc123".into(),
                pid: None,
                running: Running::SubagentStopped("def456".into()),
            },
            Ran {
                source: Source::ClaudeCode,
                key: "thr_1".into(),
                pid: Some(7),
                running: Running::Tasks(2),
            },
        ];
        for ran in lines {
            assert_eq!(ran.to_string().parse::<Ran>(), Ok(ran.clone()));
            assert!(ran.to_string().parse::<crate::Report>().is_err());
        }
        assert_eq!(
            "ran claude-code s 1 tasks 0\n"
                .parse::<Ran>()
                .map(|r| r.running),
            Ok(Running::Tasks(0))
        );
        for bad in [
            "ran",
            "ran claude-code s 1",
            "ran claude-code s 1 subagent-start",
            "ran claude-code s x tasks 1",
            "ran nobody s 1 tasks 1",
            "ran claude-code s 1 tasks",
            "ran claude-code s 1 tasks -1",
            "ran claude-code s 1 wake-up",
            "ran claude-code s 1 sleeping",
            "agent claude-code s done 1 x",
        ] {
            assert!(bad.parse::<Ran>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_stat_line_says_its_parent_its_state_and_its_start() {
        let stat = "4242 (tmux: server (1)) S 4100 4242 4100 34816 4242 4194560 1 0 0 0 \
                    3 1 0 0 20 0 1 0 987654 1000 100 18446744073709551615";
        assert_eq!(parse_stat(stat), Some((4100, false, 987_654)));
        let zombie = "77 (sleep) Z 4242 77 77 0 -1 4194564 1 0 0 0 0 0 0 0 20 0 1 0 1234 0 0";
        assert_eq!(parse_stat(zombie), Some((4242, true, 1234)));
        assert_eq!(parse_stat("garbage"), None);
    }

    /// The agent, 100; its first turn at 1000 ticks, now 5000, 100 a
    /// second.
    fn agent(servers: &[&str]) -> Watched {
        Watched {
            id: 1,
            pid: 100,
            since: 1000,
            servers: servers.iter().map(|s| argv(s)).collect(),
        }
    }

    fn proc(pid: u32, parent: u32, start: u64, line: &str) -> Proc {
        Proc {
            pid,
            parent,
            start,
            zombie: false,
            argv: argv(line),
        }
    }

    const NOW: (u64, u64) = (5000, 100);

    #[test]
    fn a_child_started_in_a_turn_counts_its_mcp_servers_and_hooks_do_not() {
        let mut procs = vec![
            // Its MCP server, started with it, before its first turn.
            proc(
                101,
                100,
                900,
                "node /usr/bin/npx -y @modelcontextprotocol/server-github",
            ),
            // A server started again later, as configured.
            proc(102, 100, 2000, "node /usr/bin/npx -y @upstash/context7-mcp"),
            // Its hook, through `sh -c`, and another process's child.
            proc(
                103,
                100,
                4990,
                "/bin/sh -c /usr/bin/hocus agent-hook claude-code",
            ),
            proc(201, 200, 3000, "cargo nextest run"),
        ];
        let watched = agent(&["npx -y @upstash/context7-mcp"]);
        assert_eq!(count(&procs, &watched, NOW), Found::default());
        // A command in the background, started in its turn.
        procs.push(proc(104, 100, 3000, "/bin/bash -c cargo nextest run"));
        let found = count(&procs, &watched, NOW);
        assert_eq!(
            found,
            Found {
                counted: 1,
                young: 0
            }
        );
        assert!(found.runs() && found.again());
        // A grandchild is its child's: not counted again.
        procs.push(proc(105, 104, 3001, "cargo nextest run"));
        assert_eq!(count(&procs, &watched, NOW).counted, 1);
        // Ended, not reaped yet: nothing runs.
        procs.retain(|p| p.pid != 105);
        if let Some(p) = procs.iter_mut().find(|p| p.pid == 104) {
            p.zombie = true;
        }
        assert_eq!(count(&procs, &watched, NOW), Found::default());
    }

    #[test]
    fn a_child_younger_than_a_second_is_noted_not_counted() {
        let procs = vec![proc(104, 100, 4950, "statusline.sh")];
        let found = count(&procs, &agent(&[]), NOW);
        assert_eq!(
            found,
            Found {
                counted: 0,
                young: 1
            }
        );
        assert!(!found.runs() && found.again());
        // A second later.
        assert_eq!(count(&procs, &agent(&[]), (5050, 100)).counted, 1);
    }

    #[test]
    fn a_server_runs_by_its_command_and_its_arguments_wherever_they_start() {
        let server = argv("npx -y @upstash/context7-mcp");
        assert!(matches(
            &argv("node /usr/bin/npx -y @upstash/context7-mcp"),
            &server
        ));
        assert!(matches(
            &argv("/usr/local/bin/npx -y @upstash/context7-mcp --x"),
            &server
        ));
        assert!(!matches(&argv("npx -y @other/mcp"), &server));
        assert!(!matches(&argv("npx"), &server));
        assert!(matches(&argv("/opt/servers/docs"), &argv("docs")));
        assert!(!matches(&argv("docs"), &[]));
    }

    #[test]
    fn the_servers_codex_configures() {
        let codex = "model = \"gpt-5\"\n\n[mcp_servers.context7]\ncommand = \"npx\"\n\
                     args = [\"-y\", \"@upstash/context7-mcp\"]\n\n\
                     [mcp_servers.docs]\ncommand = \"/opt/docs-mcp\"\n\n\
                     [mcp_servers.figma]\nurl = \"https://mcp.figma.com/mcp\"\n";
        assert_eq!(
            mcp_servers(codex),
            vec![argv("npx -y @upstash/context7-mcp"), argv("/opt/docs-mcp")]
        );
        assert!(mcp_servers("not = [toml").is_empty());
        assert!(mcp_servers("model = \"gpt-5\"").is_empty());
    }

    #[test]
    fn this_process_is_among_the_machines_with_its_start() {
        let me = std::process::id();
        let procs = processes(Path::new(crate::process::PROC));
        let mine = procs.iter().find(|p| p.pid == me).expect("this process");
        let (now, _) = now_ticks();
        assert!(mine.start > 0 && mine.start <= now, "{mine:?} {now}");
        assert!(!mine.argv.is_empty());
    }
}
