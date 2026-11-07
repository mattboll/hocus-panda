//! The recorder: counters the compositor's loop only increments, a ring
//! its events go into, and a thread of its own that wakes every 10 s to
//! read them with `/proc` and append one sample to the day's file.
//!
//! The thread never wakes the loop: it reads atomics and takes the ring's
//! events under a lock the loop holds only to push one. What it raises (a
//! signal's notification, once a day per kind) it says through the
//! callback it was given, on its own thread.

use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use jiff::{civil::Date, tz::TimeZone};

use crate::{
    files::{self, DAY_CAP},
    format::{Event, EventKind, Line, Sample, Who},
    signals::{Signal, SignalKind, Watch},
};

/// The screens counted apart, at most.
pub const SCREENS: usize = 8;
/// The events kept between two samples, at most: the oldest go first.
pub const RING: usize = 128;
/// The recorder's stack: it reads small files and writes a line.
const STACK: usize = 256 * 1024;
/// The loop's first turns after start (the shaders, the wallpapers) are
/// not judged slow.
const SETTLING: Duration = Duration::from_secs(30);

/// The line the Settings say their page on, on the compositor's control
/// socket: their process, and the page's name (none as they close).
pub fn settings_line(pid: u32, page: &str) -> String {
    let page: String = page.chars().filter(|c| !c.is_control()).collect();
    format!("perf settings {pid} {page}")
}

/// What such a line says.
pub fn settings_said(line: &str) -> Option<(u32, &str)> {
    let rest = line.trim_end().strip_prefix("perf settings ")?;
    let (pid, page) = rest.split_once(' ').unwrap_or((rest, ""));
    Some((pid.parse().ok()?, page))
}

/// Now, in Unix seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[derive(Debug, Default)]
struct Screen {
    frames: AtomicU64,
    scenes: AtomicU64,
}

/// What the compositor's loop counts, read by the recorder's thread.
#[derive(Debug, Default)]
pub struct Counters {
    wakeups: AtomicU64,
    frames: AtomicU64,
    scenes: AtomicU64,
    motion_frames: AtomicU64,
    inputs: AtomicU64,
    app_commits: AtomicU64,
    allocations: AtomicU64,
    longest_turn_us: AtomicU64,
    longest_frame_us: AtomicU64,
    screens: [Screen; SCREENS],
    /// The screens' names, each its slot; taken when a screen first
    /// draws, never on each frame.
    names: Mutex<[Option<String>; SCREENS]>,
    events: Mutex<VecDeque<Event>>,
    /// The Settings' process while they run (they say so on the control
    /// socket), else 0.
    settings_pid: AtomicU32,
}

impl Counters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The slot of the screen named `name`, given the first time it is
    /// asked; `None` past [`SCREENS`].
    pub fn screen(&self, name: &str) -> Option<usize> {
        let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = names.iter().position(|n| n.as_deref() == Some(name)) {
            return Some(i);
        }
        let free = names.iter().position(Option::is_none)?;
        names[free] = Some(name.to_owned());
        Some(free)
    }

    /// A turn of the loop, and what its work took.
    pub fn turn(&self, took: Duration) {
        self.wakeups.fetch_add(1, Relaxed);
        self.longest_turn_us
            .fetch_max(took.as_micros() as u64, Relaxed);
    }

    /// A frame drawn on the screen of `slot`, while a motion ran or not,
    /// and what it took.
    pub fn frame(&self, slot: Option<usize>, moving: bool) {
        self.frames.fetch_add(1, Relaxed);
        if moving {
            self.motion_frames.fetch_add(1, Relaxed);
        }
        if let Some(screen) = slot.and_then(|i| self.screens.get(i)) {
            screen.frames.fetch_add(1, Relaxed);
        }
    }

    /// A scene built for the screen of `slot`, and what drawing it took.
    pub fn scene(&self, slot: Option<usize>, took: Duration) {
        self.scenes.fetch_add(1, Relaxed);
        self.longest_frame_us
            .fetch_max(took.as_micros() as u64, Relaxed);
        if let Some(screen) = slot.and_then(|i| self.screens.get(i)) {
            screen.scenes.fetch_add(1, Relaxed);
        }
    }

    pub fn input(&self) {
        self.inputs.fetch_add(1, Relaxed);
    }

    /// An application's window committed.
    pub fn app_commit(&self) {
        self.app_commits.fetch_add(1, Relaxed);
    }

    /// Allocations, from a program that counts them.
    pub fn allocated(&self, n: u64) {
        self.allocations.fetch_add(n, Relaxed);
    }

    /// Something happened; written with the next sample.
    pub fn event(&self, kind: EventKind, what: impl Into<String>) {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() >= RING {
            events.pop_front();
        }
        events.push_back(Event {
            t: unix_now(),
            kind,
            what: what.into(),
        });
    }

    /// The Settings run as process `pid` (0: they ended).
    pub fn settings(&self, pid: u32) {
        self.settings_pid.store(pid, Relaxed);
    }

    fn take(&self) -> Taken {
        let names = self
            .names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Taken {
            wakeups: self.wakeups.load(Relaxed),
            frames: self.frames.load(Relaxed),
            scenes: self.scenes.load(Relaxed),
            motion_frames: self.motion_frames.load(Relaxed),
            inputs: self.inputs.load(Relaxed),
            app_commits: self.app_commits.load(Relaxed),
            allocations: self.allocations.load(Relaxed),
            longest_turn_us: self.longest_turn_us.swap(0, Relaxed),
            longest_frame_us: self.longest_frame_us.swap(0, Relaxed),
            screens: std::array::from_fn(|i| {
                (
                    names[i].clone(),
                    self.screens[i].frames.load(Relaxed),
                    self.screens[i].scenes.load(Relaxed),
                )
            }),
            events: std::mem::take(
                &mut *self.events.lock().unwrap_or_else(PoisonError::into_inner),
            )
            .into(),
        }
    }
}

/// The counters as read at a sample.
#[derive(Debug, Default)]
struct Taken {
    wakeups: u64,
    frames: u64,
    scenes: u64,
    motion_frames: u64,
    inputs: u64,
    app_commits: u64,
    allocations: u64,
    longest_turn_us: u64,
    longest_frame_us: u64,
    screens: [(Option<String>, u64, u64); SCREENS],
    events: Vec<Event>,
}

fn delta(now: u64, before: u64) -> u32 {
    u32::try_from(now.saturating_sub(before)).unwrap_or(u32::MAX)
}

/// A process's CPU time (user and system), in clock ticks, from
/// `/proc/PID/stat`.
fn ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Its name may hold spaces and parentheses: the fields follow the
    // last ')'. utime and stime are the 14th and 15th.
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace().skip(11);
    let user: u64 = fields.next()?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    Some(user + system)
}

/// A process's resident memory in kilobytes, from `/proc/PID/statm`.
fn resident_kb(pid: u32) -> Option<u32> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    u32::try_from(pages * rustix::param::page_size() as u64 / 1024).ok()
}

/// The machine's power, milliwatts, while a battery discharges; `None` on
/// AC. Read from `supplies` (`/sys/class/power_supply`): `power_now`, else
/// `current_now` × `voltage_now`.
pub fn power_mw(supplies: &Path) -> Option<u32> {
    let read = |dir: &Path, name: &str| -> Option<String> {
        Some(
            std::fs::read_to_string(dir.join(name))
                .ok()?
                .trim()
                .to_owned(),
        )
    };
    let number = |dir: &Path, name: &str| -> Option<u64> { read(dir, name)?.parse().ok() };
    let mut total_uw: Option<u64> = None;
    for entry in std::fs::read_dir(supplies).ok()?.flatten() {
        let dir = entry.path();
        if read(&dir, "type").as_deref() != Some("Battery")
            || read(&dir, "status").as_deref() != Some("Discharging")
        {
            continue;
        }
        let uw = number(&dir, "power_now").or_else(|| {
            Some(number(&dir, "current_now")? * number(&dir, "voltage_now")? / 1_000_000)
        });
        if let Some(uw) = uw {
            *total_uw.get_or_insert(0) += uw;
        }
    }
    total_uw.and_then(|uw| u32::try_from(uw / 1000).ok())
}

/// What the recorder needs.
pub struct Setup {
    /// The days' folder.
    pub dir: PathBuf,
    pub interval: Duration,
    pub counters: Arc<Counters>,
    /// The shell's process, while it runs.
    pub shell: Box<dyn Fn() -> Option<u32> + Send>,
    /// `/sys/class/power_supply`, or a test's.
    pub supplies: PathBuf,
    /// A signal raised, the first of its kind today: its notification.
    pub raised: Box<dyn Fn(Signal) + Send>,
    pub tz: TimeZone,
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setup")
            .field("dir", &self.dir)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

/// The recorder's thread, while it runs: it ends once this is dropped.
#[derive(Debug)]
pub struct Recorder {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(setup: Setup) -> std::io::Result<Self> {
        std::fs::create_dir_all(&setup.dir)?;
        // Counted from now: what the loop counted before is no sample's.
        let mut before = setup.counters.take();
        before.events.clear();
        let (stop, stopped) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("perf".to_owned())
            .stack_size(STACK)
            .spawn(move || run(setup, before, &stopped))?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// Stops it and waits for its last line (tests).
    pub fn stop(mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Recorder {
    /// Its thread ends at its next wakeup, or at once if it waits: never
    /// waited for here, so that the loop never waits on it.
    fn drop(&mut self) {
        self.stop.take();
    }
}

/// Today's file, appended to.
struct Today {
    date: Date,
    file: File,
    size: u64,
    /// Its signals already notified.
    notified: Vec<SignalKind>,
}

impl Today {
    fn open(dir: &Path, date: Date) -> std::io::Result<Self> {
        let path = files::path(dir, date);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let size = file.metadata()?.len();
        // The kinds already notified today, by an earlier session: read a
        // line at a time, nothing kept but them.
        let mut notified = Vec::new();
        if size > 0
            && let Ok(read) = File::open(&path)
        {
            for line in BufReader::new(read).lines().map_while(Result::ok) {
                if line.contains("\"notified\"")
                    && let Some(Line::Notified { notified: kind, .. }) = Line::decode(&line)
                {
                    notified.push(kind);
                }
            }
        }
        Ok(Self {
            date,
            file,
            size,
            notified,
        })
    }

    /// Appends `line`, unless the day is full.
    fn write(&mut self, line: &Line) {
        let mut text = line.encode();
        text.push('\n');
        if self.size + text.len() as u64 > DAY_CAP {
            return;
        }
        match self.file.write_all(text.as_bytes()) {
            Ok(()) => self.size += text.len() as u64,
            Err(err) => tracing::warn!(%err, "a sample is not written"),
        }
    }
}

/// Each process's CPU ticks at the last sample, by its pid.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    pid: u32,
    ticks: u64,
}

/// The recorder's thread.
fn run(setup: Setup, mut before: Taken, stopped: &mpsc::Receiver<()>) {
    let Setup {
        dir,
        interval,
        counters,
        shell,
        supplies,
        raised,
        tz,
    } = setup;
    let started = Instant::now();
    let tick_ms = 1000 / rustix::param::clock_ticks_per_second().max(1);
    let date_now = || files::date_at(unix_now(), &tz);
    let Some(mut today) = date_now().and_then(|date| {
        let removed = files::prune(&dir, date);
        if !removed.is_empty() {
            tracing::info!(days = removed.len(), "old days of the performance removed");
        }
        Today::open(&dir, date)
            .inspect_err(|err| tracing::warn!(%err, dir = %dir.display(), "no day to record into"))
            .ok()
    }) else {
        return;
    };
    tracing::info!(dir = %dir.display(), every = ?interval, "the day is recorded");
    today.write(&Line::Start {
        start: unix_now(),
        interval: interval.as_millis() as u32,
    });
    let mut watch = Watch::new();
    let mut seen = [Seen::default(); 3];
    let me = std::process::id();
    // The CPU each process has used so far: the first sample counts from
    // here.
    for (who, pid) in [(Who::Compositor, Some(me)), (Who::Shell, shell())] {
        if let Some(pid) = pid {
            seen[who.index()] = Seen {
                pid,
                ticks: ticks(pid).unwrap_or(0),
            };
        }
    }
    let mut last = Instant::now();
    let mut next = last + interval;
    loop {
        match stopped.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        next += interval;
        if next <= now {
            next = now + interval;
        }
        // The day changed: tomorrow's file, the old days gone.
        if let Some(date) = date_now()
            && date != today.date
        {
            files::prune(&dir, date);
            match Today::open(&dir, date) {
                Ok(new) => today = new,
                Err(err) => tracing::warn!(%err, "no day to record into"),
            }
        }
        let taken = counters.take();
        let settings = counters.settings_pid.load(Relaxed);
        let pids = [Some(me), shell(), (settings != 0).then_some(settings)];
        let mut c = Vec::with_capacity(3);
        let mut m = Vec::with_capacity(3);
        for who in Who::ALL {
            let i = who.index();
            let read = pids[i].and_then(|pid| Some((pid, ticks(pid)?, resident_kb(pid)?)));
            let (cpu, kb) = match read {
                Some((pid, now_ticks, kb)) => {
                    // A process new since the last sample: all of its time
                    // is this interval's.
                    let was = if seen[i].pid == pid { seen[i].ticks } else { 0 };
                    seen[i] = Seen {
                        pid,
                        ticks: now_ticks,
                    };
                    (now_ticks.saturating_sub(was) * tick_ms, kb)
                }
                None => {
                    // The Settings closed: their process is not followed
                    // any more (another may take its number).
                    if let (Who::Settings, Some(pid)) = (who, pids[i]) {
                        let _ = counters
                            .settings_pid
                            .compare_exchange(pid, 0, Relaxed, Relaxed);
                    }
                    seen[i] = Seen::default();
                    (0, 0)
                }
            };
            c.push(u32::try_from(cpu).unwrap_or(u32::MAX));
            m.push(kb);
        }
        // Without the Settings, their place is left out.
        if c.get(2) == Some(&0) && m.get(2) == Some(&0) {
            c.truncate(2);
            m.truncate(2);
        }
        let settled = started.elapsed() >= SETTLING;
        let several = taken.screens.iter().filter(|s| s.0.is_some()).count() > 1;
        let sample = Sample {
            t: unix_now(),
            d: now
                .duration_since(last)
                .as_millis()
                .min(u128::from(u32::MAX)) as u32,
            c,
            m,
            f: delta(taken.frames, before.frames),
            s: delta(taken.scenes, before.scenes),
            w: delta(taken.wakeups, before.wakeups),
            i: delta(taken.inputs, before.inputs),
            a: delta(taken.motion_frames, before.motion_frames),
            k: delta(taken.app_commits, before.app_commits),
            tu: if settled {
                u32::try_from(taken.longest_turn_us).unwrap_or(u32::MAX)
            } else {
                0
            },
            fu: if settled {
                u32::try_from(taken.longest_frame_us).unwrap_or(u32::MAX)
            } else {
                0
            },
            al: taken.allocations.saturating_sub(before.allocations),
            p: power_mw(&supplies),
            o: if several {
                taken
                    .screens
                    .iter()
                    .zip(&before.screens)
                    .filter_map(|(now, was)| {
                        let counts = [delta(now.1, was.1), delta(now.2, was.2)];
                        let name = now.0.clone()?;
                        (counts != [0, 0]).then_some((name, counts))
                    })
                    .collect()
            } else {
                Default::default()
            },
            e: taken.events.clone(),
        };
        last = now;
        before = taken;
        before.events.clear();
        today.write(&Line::Sample(sample.clone()));
        for kind in watch.feed(&sample) {
            if today.notified.contains(&kind) {
                continue;
            }
            today.notified.push(kind);
            today.write(&Line::Notified {
                notified: kind,
                t: sample.t,
            });
            if let Some(signal) = watch.holding(kind) {
                tracing::info!(?kind, "a signal of the performance");
                raised(signal.clone());
            }
        }
        watch.forget_ended();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::Day;

    fn setup(dir: &Path, counters: &Arc<Counters>, every: Duration) -> Setup {
        Setup {
            dir: dir.to_owned(),
            interval: every,
            counters: counters.clone(),
            shell: Box::new(|| Some(std::process::id())),
            supplies: PathBuf::from("/nonexistent"),
            raised: Box::new(|_| {}),
            tz: TimeZone::UTC,
        }
    }

    fn today_of(dir: &Path) -> Day {
        let date = files::date_at(unix_now(), &TimeZone::UTC).expect("today");
        Day::read(dir, date).expect("the day")
    }

    #[test]
    fn a_sample_every_interval_with_the_counts_and_the_events_since_the_last() {
        let dir = tempfile::tempdir().expect("a folder");
        let counters = Counters::new();
        let recorder = Recorder::start(setup(dir.path(), &counters, Duration::from_millis(40)))
            .expect("started");
        let one = counters.screen("eDP-1");
        let two = counters.screen("DP-2");
        assert_eq!(
            (one, two, counters.screen("eDP-1")),
            (Some(0), Some(1), Some(0))
        );
        for _ in 0..7 {
            counters.turn(Duration::from_micros(300));
            counters.input();
        }
        counters.frame(one, false);
        counters.frame(two, true);
        counters.scene(two, Duration::from_millis(2));
        counters.app_commit();
        counters.event(EventKind::WindowOpened, "foot");
        // Five samples or more, then the thread stopped.
        let until = Instant::now() + Duration::from_secs(10);
        while today_of(dir.path()).samples.len() < 5 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        recorder.stop();
        let day = today_of(dir.path());
        assert_eq!(day.starts.len(), 1);
        assert!(day.samples.len() >= 5, "{day:?}");
        // Each count once, whichever samples they fell in.
        let sum = |f: fn(&Sample) -> u32| -> u32 { day.samples.iter().map(f).sum() };
        assert_eq!(
            (
                sum(|s| s.w),
                sum(|s| s.i),
                sum(|s| s.f),
                sum(|s| s.a),
                sum(|s| s.s),
                sum(|s| s.k)
            ),
            (7, 7, 2, 1, 1, 1)
        );
        let screens: Vec<_> = day.samples.iter().flat_map(|s| s.o.clone()).collect();
        assert_eq!(screens.iter().map(|o| o.1[0]).sum::<u32>(), 2);
        let events: Vec<&Event> = day.samples.iter().flat_map(|s| &s.e).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0].kind, events[0].what.as_str()),
            (EventKind::WindowOpened, "foot")
        );
        for s in &day.samples {
            // The compositor and the shell (this process, twice) measured.
            assert_eq!(s.m.len(), 2);
            assert!(s.m.iter().all(|&kb| kb > 0), "{s:?}");
            assert!(s.d >= 30 && s.d < 2000, "{s:?}");
        }
    }

    #[test]
    fn the_settings_are_measured_while_they_run() {
        let dir = tempfile::tempdir().expect("a folder");
        let counters = Counters::new();
        counters.settings(std::process::id());
        let recorder = Recorder::start(setup(dir.path(), &counters, Duration::from_millis(30)))
            .expect("started");
        let until = Instant::now() + Duration::from_secs(10);
        while today_of(dir.path()).samples.len() < 2 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        counters.settings(0);
        while today_of(dir.path()).samples.len() < 4 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        recorder.stop();
        let day = today_of(dir.path());
        assert_eq!(day.samples[0].m.len(), 3);
        assert_eq!(day.samples.last().map(|s| s.m.len()), Some(2));
    }

    #[test]
    fn a_full_day_stops_at_its_cap() {
        let dir = tempfile::tempdir().expect("a folder");
        let date = jiff::civil::date(2026, 11, 7);
        let mut today = Today::open(dir.path(), date).expect("opened");
        let busy = Line::Sample(Sample {
            t: 1_762_500_620,
            c: vec![812, 345, 120],
            m: vec![138_234, 72_012, 80_123],
            f: 600,
            s: 620,
            w: 900,
            i: 120,
            a: 200,
            k: 400,
            tu: 4_500,
            fu: 3_800,
            p: Some(7_200),
            ..Sample::default()
        });
        for _ in 0..20_000 {
            today.write(&busy);
        }
        let size = std::fs::metadata(files::path(dir.path(), date))
            .expect("written")
            .len();
        assert!(size <= DAY_CAP && size > DAY_CAP - 300, "{size}");
    }

    #[test]
    fn notified_kinds_are_remembered_for_the_day() {
        let dir = tempfile::tempdir().expect("a folder");
        let date = jiff::civil::date(2026, 11, 7);
        let mut today = Today::open(dir.path(), date).expect("opened");
        today.write(&Line::Notified {
            notified: SignalKind::Wakeups,
            t: 1,
        });
        drop(today);
        let again = Today::open(dir.path(), date).expect("opened");
        assert_eq!(again.notified, [SignalKind::Wakeups]);
    }

    #[test]
    fn the_settings_line_goes_and_comes_back() {
        let line = settings_line(4321, "Fond d’écran\n");
        assert_eq!(line, "perf settings 4321 Fond d’écran");
        assert_eq!(settings_said(&line), Some((4321, "Fond d’écran")));
        assert_eq!(settings_said("perf settings 0"), Some((0, "")));
        assert_eq!(settings_said("msg idle"), None);
    }

    #[test]
    fn the_power_on_battery_and_none_on_ac() {
        let dir = tempfile::tempdir().expect("a folder");
        let bat = dir.path().join("BAT0");
        std::fs::create_dir_all(&bat).expect("made");
        let ac = dir.path().join("AC");
        std::fs::create_dir_all(&ac).expect("made");
        std::fs::write(ac.join("type"), "Mains\n").expect("written");
        std::fs::write(bat.join("type"), "Battery\n").expect("written");
        std::fs::write(bat.join("status"), "Discharging\n").expect("written");
        std::fs::write(bat.join("power_now"), "6800000\n").expect("written");
        assert_eq!(power_mw(dir.path()), Some(6_800));
        std::fs::remove_file(bat.join("power_now")).expect("removed");
        std::fs::write(bat.join("current_now"), "500000\n").expect("written");
        std::fs::write(bat.join("voltage_now"), "12000000\n").expect("written");
        assert_eq!(power_mw(dir.path()), Some(6_000));
        std::fs::write(bat.join("status"), "Charging\n").expect("written");
        assert_eq!(power_mw(dir.path()), None);
    }
}
