//! An agent's process, and the processes above it: its window is the one
//! whose client is among them (a terminal, an editor), as the compositor
//! knows its clients' processes.
//!
//! A hook runs as a child of its agent, often through `sh -c`: the agent
//! is the nearest process above the hook that is not a shell.

use std::path::Path;

/// The shells a hook or a script may run through, by their process name.
const SHELLS: [&str; 8] = [
    "sh", "dash", "bash", "zsh", "fish", "ksh", "mksh", "busybox",
];

/// How far up the ancestry is followed: a loop in a table cannot hold a
/// thread.
const MOST: usize = 64;

/// A process as `/proc/PID/stat` says it: its name and its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub name: String,
    pub parent: u32,
}

/// `stat`'s text: `pid (name) state ppid …`; the name may hold spaces and
/// brackets.
pub fn parse_stat(stat: &str) -> Option<Process> {
    let (open, close) = (stat.find('(')?, stat.rfind(')')?);
    let name = stat.get(open + 1..close)?.to_owned();
    let parent = stat
        .get(close + 1..)?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(Process { name, parent })
}

/// Process `pid` on this machine.
pub fn read(pid: u32) -> Option<Process> {
    let stat =
        std::fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    parse_stat(&stat)
}

/// `pid` and the processes above it, nearest first, up to (not with) init.
pub fn ancestry(pid: u32) -> Vec<u32> {
    ancestry_in(pid, read)
}

pub fn ancestry_in(pid: u32, mut look: impl FnMut(u32) -> Option<Process>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut at = pid;
    while at > 1 && out.len() < MOST && !out.contains(&at) {
        out.push(at);
        match look(at) {
            Some(p) => at = p.parent,
            None => break,
        }
    }
    out
}

/// The agent above a hook whose parent is `parent`: the nearest process
/// from it up that is not a shell (`parent` itself if none is known).
pub fn agent_above(parent: u32) -> u32 {
    agent_above_in(parent, read)
}

pub fn agent_above_in(parent: u32, mut look: impl FnMut(u32) -> Option<Process>) -> u32 {
    let mut at = parent;
    for _ in 0..MOST {
        let Some(p) = look(at) else {
            return at;
        };
        if !SHELLS.contains(&p.name.as_str()) || p.parent <= 1 {
            return at;
        }
        at = p.parent;
    }
    parent
}

/// Process `pid`'s working directory's name: an agent's last resort
/// for one (its project, as it started there).
pub fn folder_of(pid: u32) -> Option<String> {
    let cwd = std::fs::read_link(Path::new("/proc").join(pid.to_string()).join("cwd")).ok()?;
    crate::folder(&cwd.to_string_lossy())
}

/// A descriptor that becomes readable when process `pid` ends (a pidfd),
/// for an event loop to forget its agent without polling.
pub fn exit_of(pid: u32) -> std::io::Result<std::os::fd::OwnedFd> {
    let pid = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    Ok(rustix::process::pidfd_open(
        pid,
        rustix::process::PidfdFlags::empty(),
    )?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn a_stat_line_reads_whatever_its_name_holds() {
        assert_eq!(
            parse_stat("4242 (claude) S 4100 4242 4100 34816 ..."),
            Some(Process {
                name: "claude".into(),
                parent: 4100
            })
        );
        assert_eq!(
            parse_stat("77 (tmux: server (1)) S 1 77 77 0"),
            Some(Process {
                name: "tmux: server (1)".into(),
                parent: 1
            })
        );
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn the_agent_is_above_the_shells_and_its_window_above_it() {
        let p = |name: &str, parent| Process {
            name: name.into(),
            parent,
        };
        let table = HashMap::from([
            (100, p("kitty", 1)),
            (200, p("zsh", 100)),
            (300, p("claude", 200)),
            (400, p("sh", 300)),
        ]);
        let look = |pid| table.get(&pid).cloned();
        // The hook's parent is `sh -c`'s: the agent is claude, above it.
        assert_eq!(agent_above_in(400, look), 300);
        assert_eq!(agent_above_in(300, look), 300);
        assert_eq!(ancestry_in(300, look), vec![300, 200, 100]);
        // A process gone: what is known.
        assert_eq!(ancestry_in(999, look), vec![999]);
        // A loop in the table ends.
        let looped = HashMap::from([(5, p("a", 6)), (6, p("b", 5))]);
        assert_eq!(ancestry_in(5, |pid| looped.get(&pid).cloned()), vec![5, 6]);
    }

    #[test]
    fn this_process_is_in_its_own_ancestry() {
        let me = std::process::id();
        let mine = ancestry(me);
        assert_eq!(mine.first(), Some(&me));
        assert!(mine.len() >= 2, "{mine:?}");
    }

    #[test]
    fn a_process_is_named_by_its_working_directory() {
        let here = std::env::current_dir().expect("a working directory");
        assert_eq!(
            folder_of(std::process::id()),
            crate::folder(&here.to_string_lossy())
        );
        assert_eq!(folder_of(u32::MAX), None);
    }
}
