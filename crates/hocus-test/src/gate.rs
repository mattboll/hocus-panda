//! What a test holds back to show the compositor does not wait for it
//! (#298): a file, a bus service, a program, that answers only once the
//! test lets it. While the gate is closed the test turns the loop, draws
//! frames and asks for things; then it opens the gate. Had the compositor
//! waited on its loop for what is held, the test could not have got there:
//! the loop would have stayed blocked until the gate gave up, and opening
//! it then says so. Nothing is timed: a loaded machine only makes the test
//! slower, never failing.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};

/// How long a closed gate holds what waits on it. Far longer than a test
/// takes between closing and opening it, even on a loaded machine: it
/// only ends a test whose loop waited on the gate.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// Held back until opened, or until its patience runs out. Clones are the
/// same gate: one for the fixture that waits, one for the test.
#[derive(Clone)]
pub struct Gate(Arc<Inner>);

struct Inner {
    shared: Mutex<Shared>,
    changed: Condvar,
    deadline: Instant,
}

#[derive(Debug, Clone, Copy)]
struct Shared {
    phase: Phase,
    /// Something waited on it while it was closed.
    reached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Closed,
    Open,
    /// Its patience ran out before the test opened it.
    GaveUp,
}

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Gate").field(&self.shared()).finish()
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// Closed, with [`PATIENCE`].
    pub fn new() -> Self {
        Self::with_patience(PATIENCE)
    }

    pub fn with_patience(patience: Duration) -> Self {
        Self(Arc::new(Inner {
            shared: Mutex::new(Shared {
                phase: Phase::Closed,
                reached: false,
            }),
            changed: Condvar::new(),
            deadline: Instant::now() + patience,
        }))
    }

    fn shared(&self) -> Shared {
        *self.0.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits until the gate is opened, or gives up at its deadline. For
    /// the fixture, on its own thread.
    pub fn wait(&self) {
        let mut shared = self.0.shared.lock().unwrap_or_else(|e| e.into_inner());
        if shared.phase == Phase::Closed {
            shared.reached = true;
            self.0.changed.notify_all();
        }
        while shared.phase == Phase::Closed {
            let left = self.0.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                shared.phase = Phase::GaveUp;
                self.0.changed.notify_all();
                break;
            }
            shared = self
                .0
                .changed
                .wait_timeout(shared, left)
                .map(|(s, _)| s)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Whether something waited on it while it was closed: what the test
    /// holds back was asked for.
    pub fn reached(&self) -> bool {
        self.shared().reached
    }

    /// Lets what waits go on. False when the gate had given up first:
    /// what it held was waited for where the test could not open it, on
    /// the loop.
    #[must_use = "false means the loop waited for what the gate held"]
    pub fn open(&self) -> bool {
        let mut shared = self.0.shared.lock().unwrap_or_else(|e| e.into_inner());
        if shared.phase == Phase::Closed {
            shared.phase = Phase::Open;
            self.0.changed.notify_all();
        }
        shared.phase == Phase::Open
    }

    /// Makes `path` a FIFO (unless it is one already) whose first reader
    /// waits, as on a slow disk, until the gate opens, then reads
    /// `contents` and the end. Opening it counts as reaching the gate.
    pub fn fifo(&self, path: &Path, contents: impl Into<Vec<u8>>) -> anyhow::Result<()> {
        let made = rustix::fs::mkfifoat(
            rustix::fs::CWD,
            path,
            rustix::fs::Mode::from_raw_mode(0o600),
        );
        match made {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) if is_fifo(path) => {}
            Err(err) => anyhow::bail!("could not make the FIFO {}: {err}", path.display()),
        }
        let gate = Arc::downgrade(&self.0);
        let (path, contents) = (path.to_owned(), contents.into());
        std::thread::Builder::new()
            .name("gate fifo".into())
            .spawn(move || serve_fifo(&gate, &path, &contents))?;
        Ok(())
    }

    /// Lines of shell for a program the compositor runs (a stand-in for
    /// tesseract): they reach the gate, and go on once it opens. Every
    /// run of the program waits on the same gate; `dir` keeps the two
    /// files that say so. A program left behind by a test that ended
    /// first goes on by itself after [`PATIENCE`] and a little more.
    pub fn shell(&self, dir: &Path) -> anyhow::Result<String> {
        let (reached, open) = (dir.join("gate-reached"), dir.join("gate-open"));
        let gate = Arc::downgrade(&self.0);
        let (waiting, opening) = (reached.clone(), open.clone());
        std::thread::Builder::new()
            .name("gate shell".into())
            .spawn(move || {
                while !waiting.exists() {
                    if gate.strong_count() == 0 {
                        let _ = std::fs::write(&opening, "");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if let Some(inner) = gate.upgrade() {
                    Gate(inner).wait();
                }
                let _ = std::fs::write(opening, "");
            })?;
        let turns = (PATIENCE.as_millis() + 10_000) / 10;
        Ok(format!(
            "touch '{}'\nn=0\nwhile [ ! -e '{}' ] && [ $n -lt {turns} ]; do sleep 0.01; n=$((n + 1)); done\n",
            reached.display(),
            open.display()
        ))
    }
}

fn is_fifo(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt as _;
    std::fs::metadata(path).is_ok_and(|m| m.file_type().is_fifo())
}

/// Waits for a reader (an open without one fails at once), holds it
/// until the gate opens, then writes. Ends when the gate is dropped with
/// no reader come.
fn serve_fifo(gate: &Weak<Inner>, path: &PathBuf, contents: &[u8]) {
    use rustix::fs::{Mode, OFlags};
    let fd = loop {
        match rustix::fs::open(
            path,
            OFlags::WRONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => break fd,
            Err(rustix::io::Errno::NXIO) if gate.strong_count() > 0 => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return,
        }
    };
    if let Some(inner) = gate.upgrade() {
        Gate(inner).wait();
    }
    // Blocking again: what is written may be more than the pipe holds.
    let _ = rustix::fs::fcntl_setfl(&fd, OFlags::empty());
    let _ = std::fs::File::from(fd).write_all(contents);
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    #[test]
    fn what_waits_goes_on_once_opened() {
        let gate = Gate::new();
        let waiting = gate.clone();
        let thread = std::thread::spawn(move || waiting.wait());
        while !gate.reached() {
            std::thread::yield_now();
        }
        assert!(gate.open());
        assert!(thread.join().is_ok());
        assert!(gate.open(), "open stays open");
    }

    #[test]
    fn a_gate_that_gave_up_says_so() {
        let gate = Gate::with_patience(Duration::from_millis(10));
        gate.wait();
        assert!(gate.reached());
        assert!(!gate.open());
    }

    #[test]
    fn opened_first_nothing_waits_nor_counts_as_reached() {
        let gate = Gate::with_patience(Duration::from_millis(10));
        assert!(gate.open());
        gate.wait();
        assert!(!gate.reached());
        assert!(gate.open());
    }

    #[test]
    fn a_fifo_is_read_once_the_gate_opens() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("slow");
        let gate = Gate::new();
        gate.fifo(&path, "held back\n")?;
        let reading = path.clone();
        let reader = std::thread::spawn(move || -> std::io::Result<String> {
            let mut text = String::new();
            std::fs::File::open(reading)?.read_to_string(&mut text)?;
            Ok(text)
        });
        while !gate.reached() {
            std::thread::yield_now();
        }
        assert!(!reader.is_finished(), "read before the gate opened");
        assert!(gate.open());
        let text = reader
            .join()
            .map_err(|_| anyhow::anyhow!("the reader panicked"))??;
        assert_eq!(text, "held back\n");
        Ok(())
    }
}
