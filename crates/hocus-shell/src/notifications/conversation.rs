//! The conversation a notification came from (#823, notifications.md "An
//! agent's conversation"): its sender's process and the processes above
//! it, up to Claude Code's or Codex's; then that agent's session, without
//! its help: the session's id Claude Code gives the commands it runs, else
//! the transcript the agent holds open, else the newest one of its folder.
//!
//! Read as the notification comes, on the server's thread, while its
//! sender is still there: `/proc` (a table of the test's in the unit
//! tests) and the names and times of the transcripts' files, never what
//! they hold.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use hocus_link::AgentConversation;

/// How far up from the sender the agent is looked for.
const MOST: usize = 32;

/// How long before the notification its transcript was written to, at the
/// most, for the newest of its folder to be taken as its.
const RECENT: Duration = Duration::from_secs(10 * 60);

/// The agents' tools a conversation is resumed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// The tool a process named `comm`, run as `argv`, is: its own program,
    /// or Node running it.
    fn of(comm: &str, argv: &[String]) -> Option<Self> {
        let base = |a: &str| a.rsplit('/').next().unwrap_or(a).to_owned();
        let named = |name: &str| match name {
            "claude" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            _ => None,
        };
        if let Some(tool) = named(comm) {
            return Some(tool);
        }
        if matches!(comm, "node" | "bun") {
            let script = argv.get(1)?;
            if script.contains("@anthropic-ai/claude-code") {
                return Some(Self::ClaudeCode);
            }
            if script.contains("@openai/codex") {
                return Some(Self::Codex);
            }
            return named(&base(script));
        }
        None
    }
}

/// The agent above a notification's sender: its tool, its process, and
/// the processes between the sender (first) and it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Above {
    tool: Tool,
    pid: u32,
    below: Vec<u32>,
}

/// The conversation of the agent above `sender`, in `proc` (`/proc`), its
/// user's home `home`, at `now`; `None` when no agent is above it or its
/// session is not found.
pub fn find(proc: &Path, home: &Path, sender: u32, now: SystemTime) -> Option<AgentConversation> {
    let above = agent_above(proc, sender)?;
    let folder = std::fs::read_link(proc.join(above.pid.to_string()).join("cwd")).ok()?;
    let env = |pid: u32, name: &str| environ(proc, pid, name);
    let (session, transcript) = match above.tool {
        Tool::ClaudeCode => {
            let root = env(above.pid, "CLAUDE_CONFIG_DIR")
                .map_or_else(|| home.join(".claude"), PathBuf::from)
                .join("projects");
            claude_session(proc, &above, &root, &folder, now)?
        }
        Tool::Codex => {
            let root = env(above.pid, "CODEX_HOME")
                .map_or_else(|| home.join(".codex"), PathBuf::from)
                .join("sessions");
            codex_session(proc, &above, &root, now)?
        }
    };
    Some(AgentConversation {
        tool: above.tool.name().to_owned(),
        session,
        folder,
        transcript,
        pid: above.pid,
    })
}

/// The first process from `sender` up that is an agent's tool.
fn agent_above(proc: &Path, sender: u32) -> Option<Above> {
    let mut below = Vec::new();
    let mut at = sender;
    while at > 1 && below.len() < MOST && !below.contains(&at) {
        let dir = proc.join(at.to_string());
        let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
        let (comm, parent) = parse_stat(&stat)?;
        let argv = cmdline(&dir);
        if let Some(tool) = Tool::of(&comm, &argv) {
            return Some(Above {
                tool,
                pid: at,
                below,
            });
        }
        below.push(at);
        at = parent;
    }
    None
}

/// `stat`'s name and parent: `PID (NAME) STATE PPID …`, the name may hold
/// spaces and parentheses, the last `)` ends it.
fn parse_stat(stat: &str) -> Option<(String, u32)> {
    let (open, close) = (stat.find('(')?, stat.rfind(')')?);
    let name = stat.get(open + 1..close)?.to_owned();
    let parent = stat
        .get(close + 1..)?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((name, parent))
}

/// A process's arguments.
fn cmdline(dir: &Path) -> Vec<String> {
    std::fs::read(dir.join("cmdline"))
        .map(|raw| {
            raw.split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The variable `name` of process `pid`'s environment, the other ones not
/// kept.
fn environ(proc: &Path, pid: u32, name: &str) -> Option<String> {
    let raw = std::fs::read(proc.join(pid.to_string()).join("environ")).ok()?;
    let prefix = format!("{name}=");
    raw.split(|b| *b == 0).find_map(|entry| {
        let value = entry.strip_prefix(prefix.as_bytes())?;
        String::from_utf8(value.to_vec())
            .ok()
            .filter(|v| !v.is_empty())
    })
}

/// Claude Code's folder of a working directory's transcripts in `root`:
/// each character but letters and digits as `-`; else, as older versions
/// wrote it, `/` and `.` alone.
fn claude_folder(root: &Path, folder: &Path) -> PathBuf {
    let text = folder.to_string_lossy();
    let all: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let some: String = text
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    let first = root.join(all);
    if first.is_dir() {
        first
    } else {
        root.join(some)
    }
}

/// Claude Code's session: the id it gives the commands it runs, else the
/// transcript it holds open, else the newest of its folder's.
fn claude_session(
    proc: &Path,
    above: &Above,
    root: &Path,
    folder: &Path,
    now: SystemTime,
) -> Option<(String, PathBuf)> {
    let dir = claude_folder(root, folder);
    let said = above
        .below
        .iter()
        .find_map(|pid| environ(proc, *pid, "CLAUDE_CODE_SESSION_ID"))
        .filter(|id| id_ok(id));
    if let Some(id) = said {
        let file = format!("{id}.jsonl");
        let transcript = Some(dir.join(&file))
            .filter(|t| t.is_file())
            .or_else(|| {
                // Another folder's name than this one's: looked for by its
                // name.
                std::fs::read_dir(root)
                    .ok()?
                    .flatten()
                    .map(|e| e.path().join(&file))
                    .find(|t| t.is_file())
            })
            .unwrap_or_else(|| dir.join(&file));
        return Some((id, transcript));
    }
    let transcript =
        held_open(proc, above.pid, root).or_else(|| newest(std::slice::from_ref(&dir), now))?;
    let id = transcript.file_stem()?.to_str()?.to_owned();
    id_ok(&id).then_some((id, transcript))
}

/// Codex's session: the rollout it holds open, else the newest of its
/// last two days'; its id at the end of its name.
fn codex_session(
    proc: &Path,
    above: &Above,
    root: &Path,
    now: SystemTime,
) -> Option<(String, PathBuf)> {
    let transcript = held_open(proc, above.pid, root).or_else(|| {
        let days = last_days(root, 2);
        newest(&days, now)
    })?;
    let stem = transcript.file_stem()?.to_str()?;
    let id = stem.get(stem.len().checked_sub(36)?..)?.to_owned();
    (stem.starts_with("rollout-") && id_ok(&id)).then_some((id, transcript))
}

/// The newest `count` day folders of Codex's `sessions` (`YYYY/MM/DD`).
fn last_days(root: &Path, count: usize) -> Vec<PathBuf> {
    let sorted = |dir: &Path| -> Vec<PathBuf> {
        let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.reverse();
        names
    };
    let mut out = Vec::new();
    for year in sorted(root) {
        for month in sorted(&year) {
            for day in sorted(&month) {
                out.push(day);
                if out.len() == count {
                    return out;
                }
            }
        }
    }
    out
}

/// A transcript (`*.jsonl` under `root`) process `pid` holds open.
fn held_open(proc: &Path, pid: u32, root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(proc.join(pid.to_string()).join("fd"))
        .ok()?
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok())
        .find(|p| p.starts_with(root) && p.extension().is_some_and(|e| e == "jsonl"))
}

/// The newest `*.jsonl` of `dirs`, written within [`RECENT`] before `now`
/// (or a little after: the clock read before the file was).
fn newest(dirs: &[PathBuf], now: SystemTime) -> Option<PathBuf> {
    let from = now.checked_sub(RECENT)?;
    let until = now + Duration::from_secs(60);
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|d| d.flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(at, _)| *at >= from && *at <= until)
        .max_by_key(|(at, _)| *at)
        .map(|(_, path)| path)
}

/// Whether an id may be given on a command line: letters, digits and `-`,
/// not first (as the compositor checks it again, `resume::id_ok`).
fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "3f6c2a1e-0b7d-4c55-9a51-2f0e8d1c7b42";

    /// A process of the test's `/proc`: its name, parent, arguments,
    /// environment and working directory.
    fn process(
        proc: &Path,
        pid: u32,
        name: &str,
        parent: u32,
        argv: &[&str],
        env: &[&str],
        cwd: Option<&Path>,
    ) -> std::io::Result<()> {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(dir.join("fd"))?;
        std::fs::write(
            dir.join("stat"),
            format!("{pid} ({name}) S {parent} {pid} {pid} 0 -1 4194304"),
        )?;
        std::fs::write(dir.join("cmdline"), argv.join("\0") + "\0")?;
        std::fs::write(dir.join("environ"), env.join("\0") + "\0")?;
        if let Some(cwd) = cwd {
            std::os::unix::fs::symlink(cwd, dir.join("cwd"))?;
        }
        Ok(())
    }

    #[test]
    fn the_walk_goes_up_to_claude_through_the_shells_or_node() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let proc = dir.path();
        // notify-send ← bash ← claude ← systemd.
        process(proc, 40, "notify-send", 30, &["notify-send"], &[], None)?;
        process(proc, 30, "bash", 20, &["/bin/bash", "-c"], &[], None)?;
        process(proc, 20, "claude", 1, &["claude", "-p"], &[], None)?;
        assert_eq!(
            agent_above(proc, 40),
            Some(Above {
                tool: Tool::ClaudeCode,
                pid: 20,
                below: vec![40, 30]
            })
        );
        // Node running Claude Code's package; Codex's own program.
        process(
            proc,
            21,
            "node",
            1,
            &[
                "node",
                "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
            ],
            &[],
            None,
        )?;
        process(proc, 41, "sh", 21, &["sh"], &[], None)?;
        assert_eq!(
            agent_above(proc, 41).map(|a| (a.tool, a.pid)),
            Some((Tool::ClaudeCode, 21))
        );
        process(proc, 22, "codex", 1, &["codex"], &[], None)?;
        process(proc, 42, "notify-send", 22, &["notify-send"], &[], None)?;
        assert_eq!(
            agent_above(proc, 42).map(|a| (a.tool, a.pid)),
            Some((Tool::Codex, 22))
        );
        // No agent above: none; a loop in the table ends.
        process(proc, 50, "firefox", 1, &["firefox"], &[], None)?;
        assert_eq!(agent_above(proc, 50), None);
        process(proc, 60, "a", 61, &["a"], &[], None)?;
        process(proc, 61, "b", 60, &["b"], &[], None)?;
        assert_eq!(agent_above(proc, 60), None);
        Ok(())
    }

    #[test]
    fn the_session_is_the_one_claude_said_else_the_newest_transcript_of_its_folder()
    -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let proc = dir.path().join("proc");
        let home = dir.path().join("home");
        let folder = dir.path().join("work/hocus.site");
        std::fs::create_dir_all(&folder)?;
        let projects = home.join(".claude/projects");
        let mangled: String = folder
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let transcripts = projects.join(mangled);
        std::fs::create_dir_all(&transcripts)?;
        let now = SystemTime::now();
        let write = |name: &str, ago: u64| -> std::io::Result<PathBuf> {
            let file = transcripts.join(name);
            std::fs::write(&file, "{}\n")?;
            std::fs::File::options()
                .append(true)
                .open(&file)?
                .set_modified(now - Duration::from_secs(ago))?;
            Ok(file)
        };
        let older = write("0a0a0a0a-0000-4000-8000-000000000001.jsonl", 120)?;
        let newer = write(&format!("{ID}.jsonl"), 5)?;
        write("1b1b1b1b-0000-4000-8000-000000000002.jsonl", 3600)?;
        process(
            &proc,
            20,
            "claude",
            1,
            &["claude", "-p"],
            &[],
            Some(&folder),
        )?;
        process(&proc, 30, "bash", 20, &["bash"], &["PATH=/bin"], None)?;
        process(
            &proc,
            40,
            "notify-send",
            30,
            &["notify-send"],
            &["PATH=/bin"],
            None,
        )?;

        // Nothing said: the newest written of its folder's.
        let found = find(&proc, &home, 40, now).expect("a conversation");
        assert_eq!(found.tool, "claude-code");
        assert_eq!((found.session.as_str(), &found.transcript), (ID, &newer));
        assert_eq!((found.folder.as_path(), found.pid), (folder.as_path(), 20));
        // Written over 10 minutes ago: not taken.
        assert!(find(&proc, &home, 40, now + Duration::from_secs(700)).is_none());

        // The id Claude Code gave its command, whichever is newest.
        let said = "0a0a0a0a-0000-4000-8000-000000000001";
        std::fs::write(
            proc.join("40/environ"),
            format!("PATH=/bin\0CLAUDE_CODE_SESSION_ID={said}\0"),
        )?;
        let found = find(&proc, &home, 40, now).expect("a conversation");
        assert_eq!((found.session.as_str(), &found.transcript), (said, &older));
        // An id that is not one: not taken.
        std::fs::write(
            proc.join("40/environ"),
            "CLAUDE_CODE_SESSION_ID=; rm -rf ~\0",
        )?;
        assert_eq!(
            find(&proc, &home, 40, now).map(|f| f.session),
            Some(ID.to_owned())
        );
        Ok(())
    }

    #[test]
    fn codex_s_rollout_is_the_one_it_holds_else_its_last_days_newest() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let proc = dir.path().join("proc");
        let home = dir.path().join("home");
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder)?;
        let day = home.join(".codex/sessions/2026/11/14");
        std::fs::create_dir_all(&day)?;
        let id = "019a8b2c-1d2e-7f30-a1b2-c3d4e5f60718";
        let rollout = day.join(format!("rollout-2026-11-14T09-12-00-{id}.jsonl"));
        std::fs::write(&rollout, "{}\n")?;
        process(&proc, 22, "codex", 1, &["codex"], &[], Some(&folder))?;
        process(&proc, 42, "notify-send", 22, &["notify-send"], &[], None)?;
        let found = find(&proc, &home, 42, SystemTime::now()).expect("a conversation");
        assert_eq!(found.tool, "codex");
        assert_eq!((found.session.as_str(), &found.transcript), (id, &rollout));
        // Held open by it: that one, however old.
        let other =
            day.join("rollout-2026-11-13T08-00-00-019a8b2c-1d2e-7f30-a1b2-000000000000.jsonl");
        std::fs::write(&other, "{}\n")?;
        std::fs::File::options()
            .append(true)
            .open(&other)?
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 3600))?;
        std::os::unix::fs::symlink(&other, proc.join("22/fd/7"))?;
        let found = find(&proc, &home, 42, SystemTime::now()).expect("a conversation");
        assert_eq!(found.transcript, other);
        Ok(())
    }
}
