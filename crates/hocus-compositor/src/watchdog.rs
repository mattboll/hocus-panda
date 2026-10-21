//! The watchdog: a thread that wakes the loop and checks that the loop
//! answered that wakeup. A quiet screen answers at once, so only a loop that
//! is really stuck is taken for one (docs/robustness.md, "The watcher that
//! bites"). When it fires, every thread's backtrace goes to a file first.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use hocus_api::Stall;
use smithay::reexports::calloop::{
    LoopHandle,
    channel::{self, Sender},
    ping,
};

use crate::App;

/// How the watchdog watches.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// How long the loop may take to answer before the watchdog fires.
    pub limit: Duration,
    /// How often the loop is woken.
    pub interval: Duration,
    /// Where backtrace dumps go.
    pub dump_dir: PathBuf,
}

impl WatchdogConfig {
    pub fn new(dump_dir: PathBuf) -> Self {
        Self {
            limit: Duration::from_secs(10),
            interval: Duration::from_secs(1),
            dump_dir,
        }
    }
}

/// The running watchdog; it stops when this is dropped.
#[derive(Debug)]
pub struct Watchdog {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Closing the channel wakes the thread, which then ends.
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What the watchdog thread decides at each tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The last wakeup was answered: send the next one.
    Wake,
    /// Still waiting, within the limit (or already reported).
    Wait,
    /// The loop has not answered within the limit: dump and report, once.
    Fire { waited: Duration },
}

/// The watchdog's bookkeeping, apart from threads and clocks.
#[derive(Debug)]
struct Watch {
    limit: Duration,
    asked: u64,
    asked_at: Instant,
    fired: bool,
}

impl Watch {
    fn new(limit: Duration, now: Instant) -> Self {
        Self {
            limit,
            asked: 0,
            asked_at: now,
            fired: false,
        }
    }

    fn tick(&mut self, now: Instant, answered: u64) -> Action {
        if answered >= self.asked {
            self.asked += 1;
            self.asked_at = now;
            self.fired = false;
            return Action::Wake;
        }
        let waited = now.saturating_duration_since(self.asked_at);
        if !self.fired && waited > self.limit {
            self.fired = true;
            return Action::Fire { waited };
        }
        Action::Wait
    }
}

/// Starts the watchdog thread and the loop's side of it.
pub fn start(
    handle: &LoopHandle<'static, App>,
    config: WatchdogConfig,
) -> anyhow::Result<Watchdog> {
    let asked = Arc::new(AtomicU64::new(0));
    let answered = Arc::new(AtomicU64::new(0));

    let (ping, wakeups) = ping::make_ping().context("could not create the watchdog's wakeup")?;
    let loop_asked = asked.clone();
    let loop_answered = answered.clone();
    handle
        .insert_source(wakeups, move |_, _, _| {
            loop_answered.store(loop_asked.load(Ordering::Acquire), Ordering::Release);
        })
        .map_err(|err| err.error)
        .context("could not watch the watchdog's wakeup")?;

    // Stalls are reported on the loop once it runs again.
    let (reports, stalls) = channel::channel::<Stall>();
    handle
        .insert_source(stalls, |event, _, app| {
            if let channel::Event::Msg(stall) = event {
                app.state.measures.stall(stall);
            }
        })
        .map_err(|err| err.error)
        .context("could not watch the watchdog's reports")?;

    let (stop, stopped) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            let mut watch = Watch::new(config.limit, Instant::now());
            // The thread wakes every interval until the watchdog is dropped.
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(config.interval) {
                match watch.tick(Instant::now(), answered.load(Ordering::Acquire)) {
                    Action::Wake => {
                        asked.store(watch.asked, Ordering::Release);
                        ping.ping();
                    }
                    Action::Wait => {}
                    Action::Fire { waited } => fire(&config, waited, &reports),
                }
            }
        })
        .context("could not start the watchdog")?;
    Ok(Watchdog {
        stop: Some(stop),
        thread: Some(thread),
    })
}

/// Dumps first, then says so: the dump is what tells why.
fn fire(config: &WatchdogConfig, waited: Duration, reports: &Sender<Stall>) {
    let dump = dump(&config.dump_dir, waited, config.limit);
    match &dump {
        Ok(path) => tracing::error!(
            seconds = waited.as_secs_f64(),
            dump = %path.display(),
            "the event loop is not answering"
        ),
        Err(err) => tracing::error!(
            seconds = waited.as_secs_f64(),
            "the event loop is not answering; no dump: {err:#}"
        ),
    }
    let _ = reports.send(Stall {
        seconds: waited.as_secs_f64(),
        dump: dump.ok().map(|p| p.display().to_string()),
    });
}

/// Writes what every thread is doing to a new file in `dir`.
fn dump(dir: &Path, waited: Duration, limit: Duration) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let pid = std::process::id();
    // Wall-clock time only names the file, for people to find it.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let path = dir.join(format!("stall-{stamp}-{pid}.txt"));
    let mut file = std::fs::File::create(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    writeln!(
        file,
        "Hocuspanda {}: the event loop has not answered for {:.1} s (limit {:.1} s).\npid {pid}\n",
        env!("CARGO_PKG_VERSION"),
        waited.as_secs_f64(),
        limit.as_secs_f64()
    )?;
    writeln!(file, "== threads")?;
    file.write_all(threads().as_bytes())?;
    writeln!(file, "\n== backtraces")?;
    file.write_all(backtraces(pid).as_bytes())?;
    file.sync_all()?;
    Ok(path)
}

/// One line per thread: id, name, state, where it sleeps in the kernel.
#[cfg(target_os = "linux")]
fn threads() -> String {
    let read = |path: PathBuf| {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return "(no /proc/self/task)\n".into();
    };
    let mut lines: Vec<String> = tasks
        .filter_map(Result::ok)
        .map(|task| {
            let dir = task.path();
            let stat = read(dir.join("stat"));
            // The state follows the name, which is in parentheses.
            let state = stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .unwrap_or("?")
                .to_string();
            format!(
                "{} {} state={state} wchan={}",
                task.file_name().to_string_lossy(),
                read(dir.join("comm")),
                read(dir.join("wchan")),
            )
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

#[cfg(not(target_os = "linux"))]
fn threads() -> String {
    run_for("procstat", &["-t", &std::process::id().to_string()], None)
}

/// User-space backtraces of every thread, from gdb when it is installed.
fn backtraces(pid: u32) -> String {
    let pid = pid.to_string();
    let gdb = run_for(
        "gdb",
        &["-p", &pid, "-batch", "-nx", "-ex", "thread apply all bt"],
        Some(pid.as_str()),
    );
    if cfg!(target_os = "freebsd") {
        gdb + &run_for("procstat", &["-kk", &pid], None)
    } else {
        gdb
    }
}

/// Runs a debugging tool for at most 20 s and returns what it printed.
///
/// With `attach`, the tool is allowed to trace this process: Linux's Yama
/// only lets a process be traced by its ancestors otherwise. The tool starts
/// held behind a shell that waits for a line, so that the permission names
/// its pid before it attaches.
fn run_for(program: &str, args: &[&str], attach: Option<&str>) -> String {
    const TIMEOUT: Duration = Duration::from_secs(20);
    let mut command = if attach.is_some() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("read go && exec \"$0\" \"$@\"")
            .arg(program)
            .args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    };
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return format!("({program}: {err})\n"),
    };
    if attach.is_some() {
        allow_tracing_by(child.id());
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"go\n");
        }
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return format!("({program} did not finish in {} s)\n", TIMEOUT.as_secs());
            }
        }
    }
    match child.wait_with_output() {
        Ok(output) => {
            String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr)
        }
        Err(err) => format!("({program}: {err})\n"),
    }
}

#[cfg(target_os = "linux")]
fn allow_tracing_by(pid: u32) {
    let pid = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw);
    if let Some(pid) = pid
        && let Err(err) = rustix::process::set_ptracer(rustix::process::PTracer::ProcessID(pid))
    {
        tracing::warn!(%err, "could not let the debugger attach");
    }
}

#[cfg(not(target_os = "linux"))]
fn allow_tracing_by(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_loop_that_answers_is_woken_again() {
        let start = Instant::now();
        let mut watch = Watch::new(LIMIT, start);
        assert_eq!(watch.tick(start, 0), Action::Wake);
        // Answered: a quiet loop answers each wakeup.
        for s in 1..100 {
            let now = start + Duration::from_secs(s);
            assert_eq!(watch.tick(now, watch.asked), Action::Wake);
        }
    }

    #[test]
    fn a_loop_that_does_not_answer_fires_once_past_the_limit() {
        let start = Instant::now();
        let mut watch = Watch::new(LIMIT, start);
        assert_eq!(watch.tick(start, 0), Action::Wake);
        let at = |s| start + Duration::from_secs(s);
        assert_eq!(watch.tick(at(5), 0), Action::Wait);
        assert_eq!(
            watch.tick(at(11), 0),
            Action::Fire {
                waited: Duration::from_secs(11)
            }
        );
        assert_eq!(watch.tick(at(30), 0), Action::Wait);
        // Once it answers, it is watched afresh.
        assert_eq!(watch.tick(at(31), 1), Action::Wake);
        assert_eq!(
            watch.tick(at(45), 1),
            Action::Fire {
                waited: Duration::from_secs(14)
            }
        );
    }

    #[test]
    fn a_dump_lists_the_threads() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dump(dir.path(), Duration::from_secs(12), LIMIT)?;
        let text = std::fs::read_to_string(path)?;
        assert!(text.contains("has not answered for 12.0 s"), "{text}");
        assert!(text.contains("== threads"), "{text}");
        assert!(text.contains("== backtraces"), "{text}");
        Ok(())
    }
}
