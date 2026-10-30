//! Finding the tools, and plugging them in, on a thread of its own: the
//! compositor's loop never touches their files.
//!
//! The thread waits on inotify, without polling: for each place that says
//! a tool is there (its directory, its settings, its program in each of
//! `PATH`'s directories), the nearest directory that exists is watched,
//! for that one name. A tool installed later, a settings file changed by
//! hand, are seen as they happen. What the user asks (plug in, unplug,
//! plug nothing in) comes through a channel, woken by an eventfd. Each time
//! something happened, the tools are looked at again, and a new
//! [`Snapshot`] goes to the sink when it changed.

use std::{
    collections::HashMap,
    ffi::OsString,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    sync::mpsc,
};

use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, eventfd, poll},
    fs::inotify::{self, CreateFlags, WatchFlags},
    io::Errno,
};

use crate::tools::{self, Memory, Places, Tool};

/// A tool that hooks plug in, as the bar shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    /// Not on this computer.
    Absent,
    /// There, not plugged in: the card proposes it.
    Found,
    /// There, not plugged in, and the user said to plug nothing in.
    Declined,
    Plugged,
}

/// The tools as last looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub claude_code: ToolState,
    pub codex: ToolState,
    /// herdr is on this computer (its program or its directory).
    pub herdr_found: bool,
    /// The user said to plug nothing in while herdr was there: its agents
    /// are not followed.
    pub herdr_declined: bool,
}

impl Snapshot {
    pub fn of(&self, tool: Tool) -> ToolState {
        match tool {
            Tool::ClaudeCode => self.claude_code,
            Tool::Codex => self.codex,
        }
    }
}

/// What the user asks of the tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Plug(Tool),
    Unplug(Tool),
    /// "Plug nothing in": every tool there and not plugged in is declined,
    /// herdr's following too.
    Decline,
}

/// The watcher's thread: it ends when this is dropped.
#[derive(Debug)]
pub struct Watcher {
    asks: mpsc::Sender<Ask>,
    wake: std::sync::Arc<OwnedFd>,
}

impl Watcher {
    /// Starts watching the tools in `places`, telling `sink` what it sees
    /// (once at once, then on each change).
    pub fn start(
        places: Places,
        sink: impl Fn(Snapshot) + Send + 'static,
    ) -> std::io::Result<Self> {
        let wake = std::sync::Arc::new(eventfd(0, EventfdFlags::CLOEXEC)?);
        let inotify = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        let (asks, asked) = mpsc::channel();
        let thread_wake = wake.clone();
        std::thread::Builder::new()
            .name("agents' tools".to_owned())
            .spawn(move || run(places, inotify, &thread_wake, &asked, &sink))?;
        Ok(Self { asks, wake })
    }

    pub fn ask(&self, ask: Ask) {
        if self.asks.send(ask).is_ok() {
            let _ = rustix::io::write(self.wake.as_fd(), &1u64.to_ne_bytes());
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // The thread sees the channel closed as it wakes.
        let _ = rustix::io::write(self.wake.as_fd(), &1u64.to_ne_bytes());
    }
}

/// The places that say a tool is there or plugged in.
fn targets(places: &Places) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for tool in [Tool::ClaudeCode, Tool::Codex] {
        targets.push(places.dir(tool));
        targets.push(places.settings(tool));
    }
    targets.push(places.herdr_dir());
    for dir in &places.path {
        for program in ["claude", "codex", "herdr"] {
            targets.push(dir.join(program));
        }
    }
    targets
}

/// The directory to watch for `target`, and the name to watch for in it:
/// its nearest ancestor that exists, and the next part of its path.
fn watch_for(target: &Path) -> Option<(PathBuf, OsString)> {
    let mut at = target;
    loop {
        let parent = at.parent()?;
        if parent.is_dir() {
            return Some((parent.to_path_buf(), at.file_name()?.to_owned()));
        }
        at = parent;
    }
}

/// The watches: by watch descriptor, its directory and the names in it
/// that matter.
#[derive(Default)]
struct Watches {
    by_wd: HashMap<i32, (PathBuf, Vec<OsString>)>,
}

impl Watches {
    /// Each target's directory watched (again: a directory made since is
    /// watched for what is under it).
    fn update(&mut self, inotify: &OwnedFd, places: &Places) {
        let flags = WatchFlags::CREATE
            | WatchFlags::DELETE
            | WatchFlags::MOVED_TO
            | WatchFlags::MOVED_FROM
            | WatchFlags::CLOSE_WRITE
            | WatchFlags::ATTRIB;
        let mut wanted: HashMap<PathBuf, Vec<OsString>> = HashMap::new();
        for target in targets(places) {
            if let Some((dir, name)) = watch_for(&target) {
                let names = wanted.entry(dir).or_default();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        self.by_wd.clear();
        for (dir, names) in wanted {
            match inotify::add_watch(inotify, &dir, flags) {
                Ok(wd) => {
                    let entry = self.by_wd.entry(wd).or_insert_with(|| (dir, Vec::new()));
                    entry.1.extend(names);
                }
                Err(err) => {
                    tracing::debug!(dir = %dir.display(), %err, "cannot watch for the agents' tools");
                }
            }
        }
    }

    fn matters(&self, wd: i32, name: Option<&std::ffi::CStr>) -> bool {
        let Some((_, names)) = self.by_wd.get(&wd) else {
            return false;
        };
        let Some(name) = name else {
            return true;
        };
        let name = <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(name.to_bytes());
        names.iter().any(|n| n == name)
    }
}

/// The tools as they are now.
pub fn look(places: &Places, memory: &Memory) -> Snapshot {
    let state = |tool: Tool| {
        let seen = tools::seen(places, tool);
        match (seen.found, seen.plugged) {
            (_, true) => ToolState::Plugged,
            (false, false) => ToolState::Absent,
            (true, false) if memory.declined.contains(tool.word()) => ToolState::Declined,
            (true, false) => ToolState::Found,
        }
    };
    let herdr_found = tools::herdr_found(places);
    Snapshot {
        claude_code: state(Tool::ClaudeCode),
        codex: state(Tool::Codex),
        herdr_found,
        herdr_declined: herdr_found && memory.declined.contains("herdr"),
    }
}

/// What the user asked, done (a failure logged: the tool stays as it is).
fn carry_out(ask: Ask, places: &Places, memory: &mut Memory) {
    let done = match ask {
        Ask::Plug(tool) => tools::plug(tool, places, memory),
        Ask::Unplug(tool) => tools::unplug(tool, places, memory),
        Ask::Decline => {
            let now = look(places, memory);
            for tool in [Tool::ClaudeCode, Tool::Codex] {
                if now.of(tool) == ToolState::Found {
                    memory.declined.insert(tool.word().to_owned());
                }
            }
            if now.herdr_found {
                memory.declined.insert("herdr".to_owned());
            }
            tracing::info!(declined = ?memory.declined, "plug nothing in");
            memory.save(places);
            Ok(())
        }
    };
    if let Err(err) = done {
        tracing::warn!(?ask, %err, "could not do it: the tool stays as it was");
    }
}

fn run(
    places: Places,
    inotify: OwnedFd,
    wake: &OwnedFd,
    asked: &mpsc::Receiver<Ask>,
    sink: &dyn Fn(Snapshot),
) {
    let mut memory = Memory::load(&places);
    let mut watches = Watches::default();
    let mut last = None;
    let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
    loop {
        watches.update(&inotify, &places);
        let now = look(&places, &memory);
        if last != Some(now) {
            tracing::debug!(?now, "the agents' tools");
            sink(now);
            last = Some(now);
        }
        // Wait for a change that matters, or for the user.
        loop {
            let (mut tools_seen, mut woken) = (false, false);
            let mut fds = [
                PollFd::new(&inotify, PollFlags::IN),
                PollFd::new(wake, PollFlags::IN),
            ];
            match poll(&mut fds, None) {
                Ok(_) => {}
                Err(Errno::INTR) => continue,
                Err(err) => {
                    tracing::warn!(%err, "stopped watching the agents' tools");
                    return;
                }
            }
            if fds[0].revents().contains(PollFlags::IN) {
                let mut events = inotify::Reader::new(&inotify, &mut buffer);
                // Drained to the end: the descriptor does not block.
                while let Ok(event) = events.next() {
                    tools_seen |= watches.matters(event.wd(), event.file_name());
                }
            }
            if fds[1].revents().contains(PollFlags::IN) {
                let mut count = [0u8; 8];
                let _ = rustix::io::read(wake, &mut count);
                woken = true;
            }
            if woken {
                loop {
                    match asked.try_recv() {
                        Ok(ask) => {
                            carry_out(ask, &places, &mut memory);
                            tools_seen = true;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
            }
            if tools_seen {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_watched_from_its_nearest_directory() {
        let dir = tempfile::tempdir().expect("a directory");
        let deep = dir.path().join(".claude/settings.json");
        assert_eq!(
            watch_for(&deep),
            Some((dir.path().to_path_buf(), OsString::from(".claude")))
        );
        std::fs::create_dir(dir.path().join(".claude")).expect("made");
        assert_eq!(
            watch_for(&deep),
            Some((dir.path().join(".claude"), OsString::from("settings.json")))
        );
    }
}
