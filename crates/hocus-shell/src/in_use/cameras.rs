//! The processes holding a camera's device open, for the cameras used
//! without PipeWire (an application opening `/dev/video0` itself).
//!
//! Read from `/proc` (each process's open files) on a thread of its own,
//! never on the one that draws, and only while PipeWire has a camera's
//! node: without one, the thread sleeps. While one exists, the devices
//! (`/dev/video*`, or `HOCUS_VIDEO_DEVICES`'s `video*`) are watched with
//! inotify, which says when one is opened or closed: `/proc` is read then,
//! at most once a second (an open during the second after a reading is
//! read at its end). Nothing is read while nobody opens a device. Where a
//! device cannot be watched, it is read once a second while a camera's
//! node exists.
//!
//! PipeWire's and WirePlumber's own processes are left out: what they
//! open, they open for a stream, which the graph shows with its
//! application.

use std::{
    collections::HashMap,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use rustix::{
    event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll},
    fs::inotify::{self, CreateFlags, WatchFlags},
    io::Errno,
};

use super::Media;

/// `/proc` is read at most this often.
pub const EVERY: Duration = Duration::from_secs(1);

/// The programs whose devices are a stream's.
const PIPEWIRE: [&str; 3] = ["pipewire", "wireplumber", "pipewire-media-"];

/// When `/proc` may be read again: at most once each [`EVERY`].
#[derive(Debug, Default, Clone, Copy)]
pub struct Limiter {
    last: Option<Instant>,
}

impl Limiter {
    /// How long until a reading may be done at `now` (zero: now).
    pub fn wait(&self, now: Instant) -> Duration {
        self.last.map_or(Duration::ZERO, |last| {
            (last + EVERY).saturating_duration_since(now)
        })
    }

    pub fn read(&mut self, now: Instant) {
        self.last = Some(now);
    }
}

/// The thread, once a camera's node was seen: how many exist now, and
/// what wakes it when that changes.
struct Thread {
    nodes: Arc<AtomicUsize>,
    wake: Arc<OwnedFd>,
}

static THREAD: Mutex<Option<Thread>> = Mutex::new(None);

/// PipeWire has `count` cameras' nodes: their devices are listed again
/// when that changes (one plugged in), and none means nothing is read.
pub fn nodes(count: usize) {
    let mut thread = THREAD.lock().unwrap_or_else(PoisonError::into_inner);
    if thread.is_none() {
        if count == 0 {
            return;
        }
        let wake = match eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK) {
            Ok(fd) => Arc::new(fd),
            Err(err) => {
                tracing::warn!(%err, "cannot follow the cameras' devices");
                return;
            }
        };
        let nodes = Arc::new(AtomicUsize::new(0));
        let (them, woken) = (nodes.clone(), wake.clone());
        let started = std::thread::Builder::new()
            .name("in use: cameras".to_owned())
            .spawn(move || run(&them, &woken));
        if let Err(err) = started {
            tracing::warn!(%err, "cannot follow the cameras' devices");
            return;
        }
        *thread = Some(Thread { nodes, wake });
    }
    if let Some(thread) = thread.as_ref()
        && thread.nodes.swap(count, Ordering::SeqCst) != count
    {
        let _ = rustix::io::write(&*thread.wake, &1u64.to_ne_bytes());
    }
}

/// The directory of the camera's devices.
fn devices_dir() -> PathBuf {
    std::env::var_os("HOCUS_VIDEO_DEVICES")
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/dev"), PathBuf::from)
}

/// The cameras' devices there now: `video*`.
fn devices(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut devices: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("video"))
        .map(|e| e.path().canonicalize().unwrap_or_else(|_| e.path()))
        .collect();
    devices.sort();
    devices
}

/// The watches on the devices, while a camera's node exists.
struct Watching {
    devices: Vec<PathBuf>,
    notify: Option<OwnedFd>,
    /// Every device watched: no reading but when one is opened or closed.
    all: bool,
}

impl Watching {
    fn start() -> Self {
        let devices = devices(&devices_dir());
        let notify = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)
            .inspect_err(|err| tracing::debug!(%err, "no inotify for the cameras"))
            .ok();
        let mut all = notify.is_some();
        if let Some(fd) = &notify {
            for device in &devices {
                let watched = inotify::add_watch(
                    fd,
                    device,
                    WatchFlags::OPEN | WatchFlags::CLOSE_WRITE | WatchFlags::CLOSE_NOWRITE,
                );
                if let Err(err) = watched {
                    tracing::debug!(%err, ?device, "a camera's device not watched: read each second");
                    all = false;
                }
            }
        }
        tracing::debug!(?devices, all, "the cameras' devices are watched");
        Self {
            devices,
            notify,
            all,
        }
    }

    /// Whether a device was opened or closed (its events read).
    fn touched(&self) -> bool {
        let Some(fd) = &self.notify else {
            return false;
        };
        let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 1024];
        let mut touched = false;
        loop {
            let mut events = inotify::Reader::new(fd, &mut buffer);
            match events.next() {
                Ok(_) => touched = true,
                Err(Errno::INTR) => {}
                Err(_) => return touched,
            }
        }
    }
}

fn run(nodes: &AtomicUsize, wake: &OwnedFd) {
    let mut watching: Option<Watching> = None;
    let mut watched = 0;
    let mut limiter = Limiter::default();
    // A reading is owed: the devices were opened or closed, or are new.
    let mut due = false;
    loop {
        let count = nodes.load(Ordering::SeqCst);
        if count != watched {
            watched = count;
            if count == 0 {
                watching = None;
                due = false;
                super::set_camera_holders(Vec::new());
            } else {
                watching = Some(Watching::start());
                due = true;
            }
        }
        let now = Instant::now();
        if let Some(watch) = &watching
            && due
            && limiter.wait(now).is_zero()
        {
            limiter.read(now);
            due = false;
            let holders = holders(&watch.devices);
            tracing::debug!(holders = holders.len(), "the cameras' holders read");
            super::set_camera_holders(holders);
        }
        // Asleep until told, a device touched, or the reading owed.
        let timeout = match &watching {
            Some(_) if due => Some(limiter.wait(Instant::now())),
            Some(watch) if !watch.all => Some(EVERY),
            _ => None,
        };
        let timespec = timeout.map(|t| Timespec {
            tv_sec: t.as_secs() as i64,
            tv_nsec: i64::from(t.subsec_nanos()),
        });
        let mut fds = vec![PollFd::new(wake, PollFlags::IN)];
        if let Some(notify) = watching.as_ref().and_then(|w| w.notify.as_ref()) {
            fds.push(PollFd::new(notify, PollFlags::IN));
        }
        match poll(&mut fds, timespec.as_ref()) {
            Ok(0) => {
                // The reading owed, or each second where not watched.
                if watching.as_ref().is_some_and(|w| !w.all) {
                    due = true;
                }
            }
            Ok(_) => {
                let mut counter = [0u8; 8];
                let _ = rustix::io::read(wake.as_fd(), &mut counter);
                if watching.as_ref().is_some_and(Watching::touched) {
                    due = true;
                }
            }
            Err(Errno::INTR) => {}
            Err(err) => {
                tracing::warn!(%err, "stopped following the cameras' devices");
                return;
            }
        }
    }
}

/// The processes holding one of `devices` open, by program: not this one,
/// not PipeWire's.
fn holders(devices: &[PathBuf]) -> Vec<Media> {
    let me = std::process::id();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found: HashMap<String, Media> = HashMap::new();
    for entry in procs.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let dir = entry.path();
        let Ok(fds) = std::fs::read_dir(dir.join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .any(|target| devices.contains(&target));
        if !holds {
            continue;
        }
        let Some(program) = program(&dir) else {
            continue;
        };
        if PIPEWIRE.iter().any(|p| program.starts_with(p)) {
            continue;
        }
        found
            .entry(program.clone())
            .or_insert_with(|| media_of(&program, pid));
    }
    let mut holders: Vec<Media> = found.into_values().collect();
    holders.sort_by(|a, b| a.key.cmp(&b.key));
    holders
}

/// Process `dir`'s program: its executable's name, else its `comm`.
fn program(dir: &Path) -> Option<String> {
    std::fs::read_link(dir.join("exe"))
        .ok()
        .and_then(|exe| Some(exe.file_name()?.to_string_lossy().into_owned()))
        .or_else(|| {
            std::fs::read_to_string(dir.join("comm"))
                .ok()
                .map(|c| c.trim().to_owned())
        })
        .filter(|p| !p.is_empty())
}

/// Program `program`'s use: named and drawn as its desktop entry says.
fn media_of(program: &str, pid: u32) -> Media {
    let entry = crate::share::entry_of(program);
    let app = entry
        .as_ref()
        .map_or_else(|| program.to_owned(), |e| e.name.clone());
    let mut names = vec![program.to_owned()];
    if let Some(entry) = &entry {
        names.insert(0, entry.id.clone());
        if let Some(icon) = &entry.icon {
            names.insert(0, icon.clone());
        }
    }
    tracing::debug!(program, pid, app, "a camera's device is held open");
    Media {
        key: format!("program {program}"),
        app,
        icon: crate::levels::icons::find(&names),
        ids: vec![program.to_lowercase()],
        media: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_is_read_at_most_once_a_second() {
        let t0 = Instant::now();
        let mut limiter = Limiter::default();
        // The first reading at once.
        assert_eq!(limiter.wait(t0), Duration::ZERO);
        limiter.read(t0);
        // Another touch within the second waits for its end.
        assert_eq!(
            limiter.wait(t0 + Duration::from_millis(300)),
            Duration::from_millis(700)
        );
        assert_eq!(limiter.wait(t0 + EVERY), Duration::ZERO);
        // However often the devices are touched: one reading a second.
        let mut readings = 0;
        let mut at = t0 + EVERY;
        let end = t0 + Duration::from_secs(10);
        while at < end {
            if limiter.wait(at).is_zero() {
                limiter.read(at);
                readings += 1;
            }
            at += Duration::from_millis(50);
        }
        assert_eq!(readings, 9);
    }

    #[test]
    fn a_process_holding_a_device_is_found_by_its_program() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let device = dir.path().join("video0");
        std::fs::write(&device, b"")?;
        std::fs::write(dir.path().join("vbi0"), b"")?;
        let listed = devices(dir.path());
        assert_eq!(listed, [device.canonicalize()?]);
        // This test's own process is left out; a child holding it is not.
        let held = std::fs::File::open(&device)?;
        assert!(holders(&listed).iter().all(|m| !m.key.contains("sleep")));
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exec sleep 30 3<\"$0\"")
            .arg(&device)
            .spawn()?;
        let found = (0..100).find_map(|_| {
            let found = holders(&listed);
            if found.iter().any(|m| m.key == "program sleep") {
                return Some(found);
            }
            std::thread::sleep(Duration::from_millis(20));
            None
        });
        let _ = child.kill();
        let _ = child.wait();
        drop(held);
        let found = found.unwrap_or_default();
        let sleep = found.iter().find(|m| m.key == "program sleep");
        assert_eq!(sleep.map(|m| m.ids.clone()), Some(vec!["sleep".to_owned()]));
        Ok(())
    }
}
