//! The file's worker: everything that touches the disk, on threads of its
//! own, never the window's.
//!
//! - It reads the configuration file as it starts and each time it changes
//!   on disk: inotify on its folder (editors rename a new file over the
//!   old), on a thread that waits for the kernel.
//! - It writes a change at once: only that line, the file whole or not at
//!   all ([`hocus_config::replace`]), its mode kept.
//! - After each read that reads, it keeps a copy of the file as the last
//!   version that read (`$XDG_STATE_HOME/hocuspanda/config.last-good.toml`);
//!   taking it back keeps the broken file as `config.toml.broken`.
//! - It keeps the last changes, for the home's "Changés récemment"
//!   (`$XDG_STATE_HOME/hocuspanda/settings-recent`).
//!
//! The window hears it through [`Heard`], owned values on a channel.

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::SystemTime,
};

use crate::{
    model::{Source, Value},
    store::{self, FileState, Raw, Reading, Slot, Snapshot},
};

/// What the window asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `raw` written at `slot`; `undoing`: the change is an undo or a redo,
    /// not a new one.
    Write { slot: Slot, raw: Raw, id: u64 },
    /// The last version that read takes the broken file's place.
    Restore,
    /// The file may have changed on disk.
    Changed,
}

/// Why the file was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    Start,
    /// A change of the window's, by its id, written.
    Written(u64),
    /// Changed on disk by something else: the slots whose value changed.
    ByHand(Vec<Slot>),
    Restored,
}

/// A change kept for the home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub slot: Slot,
    pub value: Value,
    /// When, the wall clock's seconds since 1970 (what the user reads).
    pub at: i64,
    pub by_hand: bool,
}

/// What the window hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Read {
        snapshot: Snapshot,
        why: Why,
        /// What the raw values were before, for an undo of each change.
        before: Vec<(Slot, Raw)>,
        /// When the last version that read was kept, if one is.
        last_good: Option<SystemTime>,
    },
    /// A change could not be written.
    Failed {
        id: Option<u64>,
        why: String,
    },
    Recent(Vec<Recent>),
    /// The power source in use, and the battery's charge, once looked at.
    Power {
        source: Source,
        battery: Option<u8>,
    },
}

/// The worker's end the window holds.
#[derive(Debug, Clone)]
pub struct Worker {
    asks: mpsc::Sender<Ask>,
}

impl Worker {
    /// Whether the worker took it.
    pub fn ask(&self, ask: Ask) -> bool {
        let taken = self.asks.send(ask).is_ok();
        if !taken {
            tracing::warn!("the file's worker is gone");
        }
        taken
    }
}

/// Where things are.
#[derive(Debug, Clone)]
pub struct Places {
    /// The configuration file; `None`: the compositor names none.
    pub file: Option<PathBuf>,
    /// `$XDG_STATE_HOME/hocuspanda`; `None`: nothing kept.
    pub state: Option<PathBuf>,
    /// `/sys/class/power_supply` (`HOCUS_POWER_SUPPLY_DIR` in tests).
    pub supplies: PathBuf,
}

/// How many changes the home keeps.
const RECENT: usize = 3;

/// Starts the worker; `heard` takes what it says, and returns `false`
/// once the window is gone.
pub fn start(places: Places, heard: impl FnMut(Heard) -> bool + Send + 'static) -> Worker {
    let (asks, taken) = mpsc::channel();
    let worker = Worker { asks: asks.clone() };
    let started = std::thread::Builder::new()
        .name("settings' file".to_owned())
        .spawn(move || run(&places, &taken, asks, heard));
    if let Err(err) = started {
        tracing::error!(%err, "the file's worker could not start: nothing is read");
    }
    worker
}

struct Run<'a, F> {
    places: &'a Places,
    heard: F,
    /// The text last read or written.
    text: Option<String>,
    snapshot: Option<Snapshot>,
    recent: Vec<Recent>,
}

fn run(
    places: &Places,
    taken: &mpsc::Receiver<Ask>,
    asks: mpsc::Sender<Ask>,
    heard: impl FnMut(Heard) -> bool,
) {
    let mut run = Run {
        places,
        heard,
        text: None,
        snapshot: None,
        recent: load_recent(places.state.as_deref()),
    };
    let (source, battery) = power(&places.supplies);
    if !(run.heard)(Heard::Power { source, battery }) {
        return;
    }
    if !(run.heard)(Heard::Recent(run.recent.clone())) {
        return;
    }
    if let Some(file) = &places.file {
        watch(file, asks);
    }
    if !run.read(Why::Start) {
        return;
    }
    while let Ok(ask) = taken.recv() {
        let going = match ask {
            Ask::Write { slot, raw, id } => run.write(slot, &raw, id),
            Ask::Restore => run.restore(),
            Ask::Changed => {
                // An editor's save is a few events: read once for all.
                let mut more = Vec::new();
                while let Ok(next) = taken.try_recv() {
                    if next != Ask::Changed {
                        more.push(next);
                    }
                }
                let mut going = run.reread();
                for ask in more {
                    going &= match ask {
                        Ask::Write { slot, raw, id } => run.write(slot, &raw, id),
                        Ask::Restore => run.restore(),
                        Ask::Changed => true,
                    };
                }
                going
            }
        };
        if !going {
            return;
        }
    }
}

impl<F: FnMut(Heard) -> bool> Run<'_, F> {
    fn read_text(&self) -> Result<String, String> {
        match &self.places.file {
            Some(file) => hocus_config::read(file).map(|r| r.text),
            None => Ok(String::new()),
        }
    }

    /// Reads the file and says what it holds.
    fn read(&mut self, why: Why) -> bool {
        let text = match self.read_text() {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!(%err, "the configuration is not read");
                return (self.heard)(Heard::Failed { id: None, why: err });
            }
        };
        self.said(text, why, Vec::new())
    }

    /// The file changed on disk: read again, unless it is what was last
    /// written or read.
    fn reread(&mut self) -> bool {
        let text = match self.read_text() {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!(%err, "the configuration is not read again");
                return true;
            }
        };
        if self.text.as_ref() == Some(&text) {
            return true;
        }
        let snapshot = store::read(&text);
        let changed: Vec<Slot> = match &self.snapshot {
            Some(before) => store::slots()
                .into_iter()
                .filter(|slot| before.reading(*slot) != snapshot.reading(*slot))
                .collect(),
            None => Vec::new(),
        };
        if snapshot.state == FileState::Reads {
            let now = wall_now();
            for slot in &changed {
                if let Some(value) = match snapshot.reading(*slot) {
                    Reading::Value(value) => Some(value.clone()),
                    _ => None,
                } {
                    self.remember(Recent {
                        slot: *slot,
                        value,
                        at: now,
                        by_hand: true,
                    });
                }
            }
        }
        tracing::info!(
            changed = changed.len(),
            "the configuration changed on disk: read again"
        );
        self.said(text, Why::ByHand(changed), Vec::new())
    }

    fn said(&mut self, text: String, why: Why, before: Vec<(Slot, Raw)>) -> bool {
        let mut snapshot = store::read(&text);
        if self.places.file.is_none() {
            snapshot.state = FileState::NoFile;
        }
        if snapshot.state == FileState::Reads {
            self.keep_good(&text);
        }
        if let FileState::Broken { line, message } = &snapshot.state {
            tracing::warn!(?line, message, "the configuration does not read: read-only");
            // The desktop keeps the last readable configuration: so does
            // the window.
            let kept = match &self.snapshot {
                Some(before) if before.state == FileState::Reads => Some(before.clone()),
                _ => self
                    .last_good()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .map(|t| store::read(&t))
                    .filter(|s| s.state == FileState::Reads),
            };
            if let Some(kept) = kept {
                snapshot.readings = kept.readings;
                snapshot.reduced_motion = kept.reduced_motion;
            }
        }
        self.text = Some(text);
        self.snapshot = Some(snapshot.clone());
        let last_good = self
            .last_good()
            .and_then(|p| p.metadata().ok()?.modified().ok());
        (self.heard)(Heard::Read {
            snapshot,
            why,
            before,
            last_good,
        })
    }

    fn write(&mut self, slot: Slot, raw: &Raw, id: u64) -> bool {
        let Some(file) = self.places.file.clone() else {
            return (self.heard)(Heard::Failed {
                id: Some(id),
                why: "no configuration file".to_owned(),
            });
        };
        let mut before = None;
        let done = hocus_config::edit(
            &file,
            |doc| {
                before = Raw::now(doc, slot);
                store::write(doc, slot, raw)
            },
            |text| store::check(text, slot, raw),
        );
        match done {
            Ok(text) => {
                tracing::info!(slot = slot.word(), ?raw, file = %file.display(), "written");
                if let Raw::Set(value) = raw {
                    self.remember(Recent {
                        slot,
                        value: value.clone(),
                        at: wall_now(),
                        by_hand: false,
                    });
                }
                let before = before.map(|b| vec![(slot, b)]).unwrap_or_default();
                self.said(text, Why::Written(id), before)
            }
            Err(why) => {
                tracing::warn!(slot = slot.word(), why, "not written");
                (self.heard)(Heard::Failed { id: Some(id), why })
            }
        }
    }

    fn last_good(&self) -> Option<PathBuf> {
        Some(self.places.state.as_ref()?.join("config.last-good.toml"))
    }

    /// `text` read: kept as the last version that read, if it changed.
    fn keep_good(&self, text: &str) {
        let Some(path) = self.last_good() else {
            return;
        };
        if text.trim().is_empty() || std::fs::read_to_string(&path).is_ok_and(|kept| kept == text) {
            return;
        }
        if let Err(err) = hocus_config::replace(&path, text.as_bytes(), Some(0o600)) {
            tracing::warn!(%err, path = %path.display(), "the last version that read is not kept");
        }
    }

    /// The broken file kept as `config.toml.broken`, the last version that
    /// read in its place.
    fn restore(&mut self) -> bool {
        let (Some(file), Some(good)) = (self.places.file.clone(), self.last_good()) else {
            return true;
        };
        let done = (|| -> Result<(), String> {
            let kept = std::fs::read_to_string(&good)
                .map_err(|err| format!("{}: {err}", good.display()))?;
            let read = hocus_config::read(&file)?;
            let mut broken = read.real.clone().into_os_string();
            broken.push(".broken");
            if read.mode.is_some() {
                std::fs::rename(&read.real, &broken)
                    .map_err(|err| format!("{}: {err}", read.real.display()))?;
            }
            hocus_config::replace(&read.real, kept.as_bytes(), read.mode)
                .map_err(|err| format!("{}: {err}", read.real.display()))
        })();
        match done {
            Ok(()) => {
                tracing::info!("the last version that read is taken back");
                self.read(Why::Restored)
            }
            Err(why) => {
                tracing::warn!(why, "the last version that read is not taken back");
                (self.heard)(Heard::Failed { id: None, why })
            }
        }
    }

    fn remember(&mut self, recent: Recent) {
        self.recent.retain(|r| r.slot != recent.slot);
        self.recent.insert(0, recent);
        self.recent.truncate(RECENT);
        save_recent(self.places.state.as_deref(), &self.recent);
        let _ = (self.heard)(Heard::Recent(self.recent.clone()));
    }
}

fn wall_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The recent changes as kept: `SECONDS<TAB>SLOT<TAB>settings|hand<TAB>VALUE`
/// (the value as TOML).
fn load_recent(state: Option<&Path>) -> Vec<Recent> {
    let Some(text) = state.and_then(|s| std::fs::read_to_string(s.join("settings-recent")).ok())
    else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let at = parts.next()?.parse().ok()?;
            let slot = Slot::from_word(parts.next()?)?;
            let by_hand = parts.next()? == "hand";
            let value = parts.next()?.parse::<toml_edit::Value>().ok()?;
            let value = slot.place()?.codec.read(&value)?;
            Some(Recent {
                slot,
                value,
                at,
                by_hand,
            })
        })
        .take(RECENT)
        .collect()
}

fn save_recent(state: Option<&Path>, recent: &[Recent]) {
    let Some(state) = state else {
        return;
    };
    let text: String = recent
        .iter()
        .filter_map(|r| {
            let value = r.slot.place()?.codec.write(&r.value)?;
            Some(format!(
                "{}\t{}\t{}\t{}\n",
                r.at,
                r.slot.word(),
                if r.by_hand { "hand" } else { "settings" },
                value.to_string().trim()
            ))
        })
        .collect();
    if let Err(err) = hocus_config::replace(&state.join("settings-recent"), text.as_bytes(), None) {
        tracing::warn!(%err, "the recent changes are not kept");
    }
}

/// The power source in use and the battery's charge, from the kernel's
/// supplies: on AC when a mains supply is online or there is no battery.
fn power(dir: &Path) -> (Source, Option<u8>) {
    let mut online = false;
    let mut battery = None;
    let mut mains = false;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let read = |name: &str| std::fs::read_to_string(path.join(name)).unwrap_or_default();
        match read("type").trim() {
            "Mains" | "USB" => {
                mains = true;
                online |= read("online").trim() == "1";
            }
            "Battery" => {
                battery = battery.or_else(|| read("capacity").trim().parse::<u8>().ok());
            }
            _ => {}
        }
    }
    let source = if battery.is_some() && mains && !online {
        Source::Battery
    } else {
        Source::Ac
    };
    (source, battery)
}

/// Watches `file`'s folder (and its link's target's) for changes: a
/// thread blocked in inotify's read, which wakes the worker.
fn watch(file: &Path, asks: mpsc::Sender<Ask>) {
    use rustix::fs::inotify;
    let Some(name) = file.file_name().map(ToOwned::to_owned) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = file.parent().map(Path::to_path_buf).into_iter().collect();
    let mut names = vec![name];
    if let Ok(real) = std::fs::canonicalize(file)
        && real != file
    {
        if let Some(parent) = real.parent() {
            dirs.push(parent.to_path_buf());
        }
        if let Some(name) = real.file_name() {
            names.push(name.to_owned());
        }
    }
    let fd = match inotify::init(inotify::CreateFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "the file is not watched: a change by hand is not seen");
            return;
        }
    };
    let flags = inotify::WatchFlags::CLOSE_WRITE
        | inotify::WatchFlags::MOVED_TO
        | inotify::WatchFlags::MOVED_FROM
        | inotify::WatchFlags::CREATE
        | inotify::WatchFlags::DELETE;
    for dir in &dirs {
        // A folder that is not there yet is made: the first change makes
        // it all the same.
        let _ = std::fs::create_dir_all(dir);
        if let Err(err) = inotify::add_watch(&fd, dir, flags) {
            tracing::warn!(%err, dir = %dir.display(), "a folder is not watched");
        }
    }
    let started = std::thread::Builder::new()
        .name("settings' watch".to_owned())
        .spawn(move || {
            let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4096];
            let mut reader = inotify::Reader::new(&fd, &mut buffer);
            loop {
                match reader.next() {
                    Ok(event) => {
                        let ours = event.file_name().is_some_and(|n| {
                            names
                                .iter()
                                .any(|name| name.as_encoded_bytes() == n.to_bytes())
                        });
                        if ours && asks.send(Ask::Changed).is_err() {
                            return;
                        }
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(err) => {
                        tracing::warn!(%err, "the file's watch stopped");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "the file is not watched");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_power_source_from_the_kernels_supplies() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(power(dir.path()), (Source::Ac, None));
        std::fs::create_dir_all(dir.path().join("BAT0"))?;
        std::fs::write(dir.path().join("BAT0/type"), "Battery\n")?;
        std::fs::write(dir.path().join("BAT0/capacity"), "82\n")?;
        std::fs::create_dir_all(dir.path().join("AC"))?;
        std::fs::write(dir.path().join("AC/type"), "Mains\n")?;
        std::fs::write(dir.path().join("AC/online"), "0\n")?;
        assert_eq!(power(dir.path()), (Source::Battery, Some(82)));
        std::fs::write(dir.path().join("AC/online"), "1\n")?;
        assert_eq!(power(dir.path()), (Source::Ac, Some(82)));
        Ok(())
    }

    #[test]
    fn recent_changes_are_kept_between_sessions() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let slot = Slot::from_word("power.suspend-after@battery").context("a slot")?;
        let kept = vec![Recent {
            slot,
            value: Value::Seconds(Some(1800)),
            at: 1_790_000_000,
            by_hand: true,
        }];
        save_recent(Some(dir.path()), &kept);
        assert_eq!(load_recent(Some(dir.path())), kept);
        Ok(())
    }

    use anyhow::Context as _;
}
