//! The launcher's applications (5.1): the desktop entries read on a thread
//! of their own as the shell starts, read again when the directories that
//! hold them change (watched with inotify: an application installed while
//! the session runs shows up), and how many times each was launched, kept
//! in `$XDG_STATE_HOME/hocuspanda/launches` (one line each: the count, a
//! tab, the application's id), written on a thread of its own too.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};

use super::{
    entries::{self, Entry, Locale},
    search::Folded,
};

/// An application as the launcher shows and searches it.
#[derive(Debug, Clone)]
pub struct App {
    pub entry: Entry,
    pub folded: Folded,
}

/// The applications, from A to Z.
#[derive(Debug, Default)]
pub struct Catalog {
    pub apps: Vec<App>,
}

impl Catalog {
    fn of(entries: Vec<Entry>) -> Self {
        let mut apps: Vec<App> = entries
            .into_iter()
            .map(|entry| App {
                folded: Folded::of(&entry.name, entry.generic.as_deref(), &entry.keywords),
                entry,
            })
            .collect();
        apps.sort_by(|a, b| {
            a.folded
                .name
                .cmp(&b.folded.name)
                .then(a.entry.id.cmp(&b.entry.id))
        });
        Self { apps }
    }
}

static CATALOG: Mutex<Option<Arc<Catalog>>> = Mutex::new(None);
static COUNTS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
static WRITE: Mutex<Option<mpsc::Sender<HashMap<String, u32>>>> = Mutex::new(None);

/// The applications as last read.
pub fn catalog() -> Arc<Catalog> {
    CATALOG
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// How many times each application was launched, by id.
pub fn counts() -> HashMap<String, u32> {
    COUNTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Application `id` was launched: counted, and the counts written off the
/// thread that draws.
pub fn launched(id: &str) {
    let counts = {
        let mut counts = COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
        let counts = counts.get_or_insert_with(HashMap::new);
        *counts.entry(id.to_owned()).or_default() += 1;
        counts.clone()
    };
    if let Some(write) = WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = write.send(counts);
    }
}

/// The launch counts' file: `$XDG_STATE_HOME/hocuspanda/launches`, else
/// `~/.local/state`'s.
fn counts_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let state = var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    state
        .is_absolute()
        .then(|| state.join("hocuspanda/launches"))
}

/// The counts a file holds: `COUNT\tID` lines; others are left out.
pub fn read_counts(text: &str) -> HashMap<String, u32> {
    text.lines()
        .filter_map(|line| {
            let (count, id) = line.split_once('\t')?;
            let id = id.trim();
            (!id.is_empty()).then(|| Some((id.to_owned(), count.trim().parse().ok()?)))?
        })
        .collect()
}

/// The counts as the file holds them, the most launched first.
pub fn write_counts(counts: &HashMap<String, u32>) -> String {
    let mut lines: Vec<(&String, &u32)> = counts.iter().filter(|(_, n)| **n > 0).collect();
    lines.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    lines
        .into_iter()
        .map(|(id, n)| format!("{n}\t{id}\n"))
        .collect()
}

fn save(file: &Path, counts: &HashMap<String, u32>) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let new = file.with_extension("new");
    std::fs::write(&new, write_counts(counts))?;
    std::fs::rename(&new, file)
}

/// Reads the counts and the applications, then follows the directories,
/// on threads of their own.
pub fn start() {
    let file = counts_file();
    let (write, writes) = mpsc::channel::<HashMap<String, u32>>();
    *WRITE.lock().unwrap_or_else(PoisonError::into_inner) = Some(write);
    let started = std::thread::Builder::new()
        .name("launcher's applications".to_owned())
        .spawn(move || {
            if let Some(file) = &file {
                match std::fs::read_to_string(file) {
                    Ok(text) => {
                        let read = read_counts(&text);
                        let mut counts = COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
                        let counts = counts.get_or_insert_with(HashMap::new);
                        // Launches made while it was read count too.
                        for (id, n) in read {
                            *counts.entry(id).or_default() += n;
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => tracing::warn!(file = %file.display(), %err, "launch counts not read"),
                }
            }
            if let Some(file) = file {
                let saved = std::thread::Builder::new()
                    .name("launcher's counts".to_owned())
                    .spawn(move || {
                        while let Ok(mut counts) = writes.recv() {
                            // The latest of those sent meanwhile.
                            counts = writes.try_iter().last().unwrap_or(counts);
                            if let Err(err) = save(&file, &counts) {
                                tracing::warn!(file = %file.display(), %err, "launch counts not written");
                            }
                        }
                    });
                if let Err(err) = saved {
                    tracing::warn!(%err, "cannot keep the launch counts");
                }
            }
            follow();
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the applications");
    }
}

/// Reads the applications, publishes them, and reads them again each time
/// their directories change.
fn follow() {
    let locale = Locale::from_env();
    let desktops = entries::current_desktops();
    let mut last: Option<Vec<Entry>> = None;
    loop {
        let dirs = entries::data_dirs();
        let watch = Watch::new(&dirs);
        let found = entries::scan(&dirs, &locale, &desktops);
        if last.as_ref() != Some(&found) {
            tracing::info!(count = found.len(), "the launcher's applications");
            tracing::debug!(
                apps = ?found.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
                "the launcher's applications are"
            );
            let again = last.is_some();
            *CATALOG.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Arc::new(Catalog::of(found.clone())));
            last = Some(found);
            if again {
                super::icons::again();
            }
            super::changed();
        }
        match watch {
            Some(watch) => watch.wait(),
            None => return,
        }
    }
}

/// The directories the applications are in, watched: each `applications`
/// directory and those under it; for one not there yet, the nearest
/// directory above it that is.
struct Watch {
    fd: rustix::fd::OwnedFd,
}

/// What a change in a watched directory may be.
fn flags() -> rustix::fs::inotify::WatchFlags {
    use rustix::fs::inotify::WatchFlags;
    WatchFlags::CREATE
        | WatchFlags::DELETE
        | WatchFlags::MOVED_TO
        | WatchFlags::MOVED_FROM
        | WatchFlags::CLOSE_WRITE
        | WatchFlags::DELETE_SELF
        | WatchFlags::MOVE_SELF
}

impl Watch {
    fn new(dirs: &[PathBuf]) -> Option<Self> {
        use rustix::fs::inotify::{self, CreateFlags};
        let fd = inotify::init(CreateFlags::CLOEXEC)
            .inspect_err(|err| {
                tracing::warn!(%err, "cannot watch the applications: they are read at start only");
            })
            .ok()?;
        for dir in dirs {
            let applications = dir.join("applications");
            if applications.is_dir() {
                let mut stack = vec![applications];
                while let Some(dir) = stack.pop() {
                    let _ = inotify::add_watch(&fd, &dir, flags());
                    if let Ok(read) = std::fs::read_dir(&dir) {
                        stack.extend(read.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
                    }
                }
            } else if let Some(near) = applications.ancestors().skip(1).find(|a| a.is_dir()) {
                let _ = inotify::add_watch(&fd, near, flags());
            }
        }
        Some(Self { fd })
    }

    /// Until something changed, then until it has been quiet a moment (an
    /// install writes several files).
    fn wait(&self) {
        use rustix::{
            event::{PollFd, PollFlags, Timespec, poll},
            io::Errno,
        };
        let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4096];
        let quiet = Timespec {
            tv_sec: 0,
            tv_nsec: QUIET.as_nanos() as i64,
        };
        let mut changed = false;
        loop {
            let timeout = changed.then_some(&quiet);
            let mut fds = [PollFd::new(&self.fd, PollFlags::IN)];
            match poll(&mut fds, timeout) {
                Ok(0) if changed => return,
                Ok(_) => {
                    let mut events = rustix::fs::inotify::Reader::new(&self.fd, &mut buffer);
                    match events.next() {
                        Ok(_) => changed = true,
                        Err(Errno::INTR | Errno::AGAIN) => {}
                        Err(err) => {
                            tracing::warn!(%err, "stopped watching the applications");
                            std::thread::sleep(Duration::from_secs(5));
                            return;
                        }
                    }
                }
                Err(Errno::INTR) => {}
                Err(err) => {
                    tracing::warn!(%err, "stopped watching the applications");
                    std::thread::sleep(Duration::from_secs(5));
                    return;
                }
            }
        }
    }
}

/// How long the directories stay quiet before they are read again.
const QUIET: Duration = Duration::from_millis(200);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counts_read_back_as_written() {
        let counts: HashMap<String, u32> = [
            ("org.mozilla.firefox".to_owned(), 12),
            ("kitty".to_owned(), 30),
        ]
        .into_iter()
        .collect();
        let text = write_counts(&counts);
        assert_eq!(text, "30\tkitty\n12\torg.mozilla.firefox\n");
        assert_eq!(read_counts(&text), counts);
        assert_eq!(read_counts("x\tfoo\n3\t\nnope\n4\tbar\n").len(), 1);
    }
}
