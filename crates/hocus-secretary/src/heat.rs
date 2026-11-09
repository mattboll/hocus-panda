//! The triage's heat (#756): a steady reading, not one sample. A laptop's
//! CPU package jumps to about 100 °C on every turbo burst by design (a
//! Meteor Lake read 101 100 100 89 72 65 64 80 64 100 at half-second
//! steps, the model on its graphics card), so one sample over a limit says
//! nothing. A sampler off her loop reads the temperature every five
//! seconds and averages the last minute; the triage pauses once the average
//! reaches the pause point and goes on once it is below the resume point
//! (hysteresis).
//!
//! With the model whole on the graphics card (Ollama's `/api/ps`), the
//! card's temperature is watched instead, at most every ten seconds: its
//! hwmon (amdgpu, nouveau, radeon), else `nvidia-smi` on the sampler's
//! thread (the proprietary driver has no hwmon); unreadable, the package
//! again.
//!
//! Her loop tells the sampler what to watch ([`Watch`]), only while the
//! triage has something to do, and hears back only when the verdict
//! changes ([`Hot`]).

use std::{
    collections::VecDeque,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use jiff::{SignedDuration, Timestamp};

use crate::{
    clock::Clock,
    power::{Sensors, read},
    settings,
};

/// The temperature watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gauge {
    /// The CPU package (`x86_pkg_temp`).
    Cpu,
    /// The graphics card the model is on.
    Gpu,
}

impl Gauge {
    pub fn word(self) -> &'static str {
        match self {
            Self::Cpu => "processor",
            Self::Gpu => "graphics card",
        }
    }
}

/// Too hot for the triage: which temperature, its average over the last
/// minute, in °C, and the average under which it goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hot {
    pub gauge: Gauge,
    pub average: u32,
    pub resume: u32,
}

/// Where a gauge pauses the triage, and where it lets it go on, in °C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub pause: u32,
    pub resume: u32,
}

impl Limits {
    /// The package's pause point, its critical point unknown.
    pub const CPU: u32 = 100;
    /// A card's, its slowdown point unknown.
    pub const GPU: u32 = 87;

    fn of(pause: u32, resume: Option<u32>) -> Self {
        let resume = resume
            .unwrap_or(pause.saturating_sub(10))
            .min(pause.saturating_sub(1));
        Self { pause, resume }
    }

    /// The package's: the settings' `heat`, else its critical point minus
    /// 10, else 100.
    pub fn cpu(settings: &settings::Triage, critical: Option<u32>) -> Self {
        let pause = settings
            .heat
            .or(critical.map(|c| c.saturating_sub(10)))
            .unwrap_or(Self::CPU);
        Self::of(pause, settings.resume)
    }

    /// A card's: the settings' `gpu-heat`, else the slowdown point its
    /// driver says, else 87.
    pub fn gpu(settings: &settings::Triage, slowdown: Option<u32>) -> Self {
        let pause = settings.gpu_heat.or(slowdown).unwrap_or(Self::GPU);
        Self::of(pause, settings.gpu_resume)
    }
}

fn after(at: Timestamp, wait: Duration) -> Timestamp {
    at.checked_add(SignedDuration::try_from(wait).unwrap_or(SignedDuration::MAX))
        .unwrap_or(Timestamp::MAX)
}

fn before(at: Timestamp, wait: Duration) -> Timestamp {
    at.checked_sub(SignedDuration::try_from(wait).unwrap_or(SignedDuration::MAX))
        .unwrap_or(Timestamp::MIN)
}

/// The samples of the last minute.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    samples: VecDeque<(Timestamp, u32)>,
}

impl Window {
    /// What is averaged.
    pub const SPAN: Duration = Duration::from_secs(60);
    /// The samples must cover this much of it before their average says
    /// anything: a first burst alone is not a steady reading.
    pub const STEADY: Duration = Duration::from_secs(30);

    pub fn push(&mut self, at: Timestamp, celsius: u32) {
        self.samples.push_back((at, celsius));
        let oldest = before(at, Self::SPAN);
        while self.samples.front().is_some_and(|(t, _)| *t < oldest) {
            self.samples.pop_front();
        }
    }

    /// The mean of the last minute's samples at `now`, rounded; none until
    /// they cover [`Self::STEADY`].
    pub fn average(&self, now: Timestamp) -> Option<u32> {
        let oldest = before(now, Self::SPAN);
        let recent = || self.samples.iter().filter(move |(t, _)| *t >= oldest);
        let first = recent().map(|(t, _)| *t).min()?;
        let last = recent().map(|(t, _)| *t).max()?;
        if Duration::try_from(last.duration_since(first)).ok()? < Self::STEADY {
            return None;
        }
        let (sum, n) = recent().fold((0u64, 0u64), |(s, n), (_, c)| (s + u64::from(*c), n + 1));
        u32::try_from((sum + n / 2) / n).ok()
    }
}

/// One gauge: its samples, and whether it is too hot (its average then).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Reading {
    window: Window,
    hot: Option<u32>,
}

impl Reading {
    /// Hot from the pause point on, until under the resume point; until
    /// the reading is steady, as it was.
    fn judge(&mut self, now: Timestamp, limits: Limits) -> Option<u32> {
        if let Some(average) = self.window.average(now) {
            let hot = if self.hot.is_some() {
                average >= limits.resume
            } else {
                average >= limits.pause
            };
            self.hot = hot.then_some(average);
        }
        self.hot
    }
}

/// What her loop wants watched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    /// The model is whole on the graphics card.
    pub gpu: bool,
    pub triage: settings::Triage,
}

/// Whether `nvidia-smi` is worth asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nvidia {
    /// Asked; `failures` in a row so far; `tlimit`: whether it knows the
    /// margin to the slowdown point (older drivers do not).
    Ask { failures: u32, tlimit: bool },
    /// Not there, or failing: not asked again.
    Gone,
}

/// The sampler's state: one sample at a time ([`Sampler::sample`]).
#[derive(Debug)]
pub struct Sampler {
    sensors: Sensors,
    cpu: Reading,
    gpu: Reading,
    /// When the card was last read, whether it was, and its slowdown
    /// point then.
    gpu_read: Option<Timestamp>,
    gpu_seen: bool,
    slowdown: Option<u32>,
    /// The package's critical point, once looked for.
    critical: Option<Option<u32>>,
    nvidia: Nvidia,
}

impl Sampler {
    /// Between two samples.
    pub const EVERY: Duration = Duration::from_secs(5);
    /// Between two readings of the card (`nvidia-smi` is a process).
    pub const GPU_EVERY: Duration = Duration::from_secs(10);
    /// `nvidia-smi` failing this many times in a row is not asked again.
    const FAILURES: u32 = 3;

    pub fn new(sensors: Sensors) -> Self {
        Self {
            sensors,
            cpu: Reading::default(),
            gpu: Reading::default(),
            gpu_read: None,
            gpu_seen: false,
            slowdown: None,
            critical: None,
            nvidia: Nvidia::Ask {
                failures: 0,
                tlimit: true,
            },
        }
    }

    /// Nothing watched any more: the next watch starts afresh.
    pub fn reset(&mut self) {
        self.cpu = Reading::default();
        self.gpu = Reading::default();
        self.gpu_read = None;
        self.gpu_seen = false;
    }

    /// One sample at `at`, as `watch` says: the verdict.
    pub fn sample(&mut self, at: Timestamp, watch: &Watch) -> Option<Hot> {
        if watch.gpu {
            if self
                .gpu_read
                .is_none_or(|last| at >= after(last, Self::GPU_EVERY))
            {
                self.gpu_read = Some(at);
                match self.card() {
                    Some((celsius, slowdown)) => {
                        self.gpu.window.push(at, celsius);
                        self.slowdown = slowdown;
                        self.gpu_seen = true;
                    }
                    None => self.gpu_seen = false,
                }
            }
            if self.gpu_seen {
                self.cpu = Reading::default();
                let limits = Limits::gpu(&watch.triage, self.slowdown);
                return self.gpu.judge(at, limits).map(|average| Hot {
                    gauge: Gauge::Gpu,
                    average,
                    resume: limits.resume,
                });
            }
        }
        self.gpu = Reading::default();
        if let Some(celsius) = package(&self.sensors.thermal) {
            self.cpu.window.push(at, celsius);
        }
        let critical = *self.critical.get_or_insert_with(|| critical(&self.sensors));
        let limits = Limits::cpu(&watch.triage, critical);
        self.cpu.judge(at, limits).map(|average| Hot {
            gauge: Gauge::Cpu,
            average,
            resume: limits.resume,
        })
    }

    /// The card's temperature and slowdown point: its hwmon, else
    /// `nvidia-smi`.
    fn card(&mut self) -> Option<(u32, Option<u32>)> {
        hwmon_card(&self.sensors.hwmon).or_else(|| self.nvidia())
    }

    fn nvidia(&mut self) -> Option<(u32, Option<u32>)> {
        let Nvidia::Ask { failures, tlimit } = self.nvidia else {
            return None;
        };
        let fields = if tlimit {
            "--query-gpu=temperature.gpu,temperature.gpu.tlimit"
        } else {
            "--query-gpu=temperature.gpu"
        };
        let found = match run(
            &self.sensors.nvidia_smi,
            &[fields, "--format=csv,noheader,nounits"],
        ) {
            Err(()) => {
                // No NVIDIA driver here.
                self.nvidia = Nvidia::Gone;
                return None;
            }
            Ok(out) => out.as_deref().and_then(smi),
        };
        self.nvidia = match (found, tlimit) {
            (Some(_), _) => Nvidia::Ask {
                failures: 0,
                tlimit,
            },
            // An older driver refuses the margin's field: without it.
            (None, true) => Nvidia::Ask {
                failures,
                tlimit: false,
            },
            (None, false) if failures + 1 >= Self::FAILURES => {
                tracing::info!("nvidia-smi gives no temperature: not asked again");
                Nvidia::Gone
            }
            (None, false) => Nvidia::Ask {
                failures: failures + 1,
                tlimit: false,
            },
        };
        found
    }
}

/// The CPU package's temperature: the hottest `x86_pkg_temp` zone's.
fn package(thermal: &Path) -> Option<u32> {
    let mut heat = None;
    for entry in std::fs::read_dir(thermal).ok()?.flatten() {
        let dir = entry.path();
        if read(&dir.join("type")).as_deref() == Some("x86_pkg_temp")
            && let Some(t) = millis(&dir.join("temp"))
        {
            heat = Some(heat.map_or(t, |h: u32| h.max(t)));
        }
    }
    heat
}

/// A sysfs temperature in millidegrees, in °C.
fn millis(path: &Path) -> Option<u32> {
    read(path)?
        .parse::<i64>()
        .ok()
        .and_then(|t| u32::try_from(t / 1000).ok())
}

/// A critical or slowdown point worth believing.
fn plausible(celsius: u32) -> Option<u32> {
    (60..=150).contains(&celsius).then_some(celsius)
}

/// The package's critical point: its zone's critical trip point, else
/// coretemp's `crit`.
fn critical(sensors: &Sensors) -> Option<u32> {
    if let Ok(entries) = std::fs::read_dir(&sensors.thermal) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if read(&dir.join("type")).as_deref() != Some("x86_pkg_temp") {
                continue;
            }
            for k in 0..32 {
                let Some(kind) = read(&dir.join(format!("trip_point_{k}_type"))) else {
                    break;
                };
                if kind == "critical"
                    && let Some(c) =
                        millis(&dir.join(format!("trip_point_{k}_temp"))).and_then(plausible)
                {
                    return Some(c);
                }
            }
        }
    }
    let entries = std::fs::read_dir(&sensors.hwmon).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if read(&dir.join("name")).as_deref() == Some("coretemp")
            && let Some(c) = millis(&dir.join("temp1_crit")).and_then(plausible)
        {
            return Some(c);
        }
    }
    None
}

/// The graphics cards with a hwmon: the one nearest its slowdown point
/// (`temp1_max`, else `temp1_crit` minus 10), its temperature and that
/// point.
fn hwmon_card(hwmon: &Path) -> Option<(u32, Option<u32>)> {
    let mut found: Option<(u32, Option<u32>)> = None;
    for entry in std::fs::read_dir(hwmon).ok()?.flatten() {
        let dir = entry.path();
        if !matches!(
            read(&dir.join("name")).as_deref(),
            Some("amdgpu" | "nouveau" | "radeon")
        ) {
            continue;
        }
        let Some(celsius) = millis(&dir.join("temp1_input")) else {
            continue;
        };
        let slowdown = millis(&dir.join("temp1_max"))
            .and_then(plausible)
            .or_else(|| {
                millis(&dir.join("temp1_crit"))
                    .and_then(plausible)
                    .map(|c| c - 10)
            });
        found = Some(match found {
            Some(other) if margin(other) <= margin((celsius, slowdown)) => other,
            _ => (celsius, slowdown),
        });
    }
    found
}

/// How far a card is from its slowdown point.
fn margin((celsius, slowdown): (u32, Option<u32>)) -> i64 {
    i64::from(slowdown.unwrap_or(Limits::GPU)) - i64::from(celsius)
}

/// `nvidia-smi`'s answer (`temperature.gpu[, temperature.gpu.tlimit]`, a
/// line a card): the card nearest its slowdown point, its temperature and
/// that point (the temperature plus the margin `tlimit` says).
pub fn smi(out: &str) -> Option<(u32, Option<u32>)> {
    let mut found: Option<(u32, Option<u32>)> = None;
    for line in out.lines() {
        let mut fields = line.split(',').map(str::trim);
        let Some(celsius) = fields.next().and_then(|t| t.parse::<u32>().ok()) else {
            continue;
        };
        let slowdown = fields
            .next()
            .and_then(|m| m.parse::<i64>().ok())
            .and_then(|m| u32::try_from(i64::from(celsius) + m).ok())
            .and_then(plausible);
        found = Some(match found {
            Some(other) if margin(other) <= margin((celsius, slowdown)) => other,
            _ => (celsius, slowdown),
        });
    }
    found
}

/// `program` run with `args`, on this thread, for five seconds at most:
/// its output when it succeeded; `Err` when it is not there.
fn run(program: &Path, args: &[&str]) -> Result<Option<String>, ()> {
    const LONGEST: Duration = Duration::from_secs(5);
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            return Err(());
        }
        Err(err) => {
            tracing::info!(%err, "nvidia-smi did not start");
            return Ok(None);
        }
    };
    let started = Instant::now();
    // It answers in a few milliseconds: looked at soon, then less often.
    let mut pause = Duration::from_millis(1);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < LONGEST => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(50));
            }
            _ => {
                tracing::info!("nvidia-smi did not answer in time");
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
        }
    };
    let mut out = String::new();
    if !status.success()
        || child
            .stdout
            .take()
            .is_none_or(|mut o| o.read_to_string(&mut out).is_err())
    {
        return Ok(None);
    }
    Ok(Some(out))
}

/// The sampler on its own thread: it waits for a [`Watch`], samples every
/// five seconds while it has one, and tells `told` each new verdict (the
/// first after each watch too). It ends once her loop is gone.
pub fn spawn(
    sensors: Sensors,
    clock: Clock,
    watches: Receiver<Option<Watch>>,
    told: impl Fn(Option<Hot>) -> bool + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("heat".to_owned())
        .spawn(move || sampling(Sampler::new(sensors), &clock, &watches, &told))
        .map(|_| ())
}

fn sampling(
    mut sampler: Sampler,
    clock: &Clock,
    watches: &Receiver<Option<Watch>>,
    told: &dyn Fn(Option<Hot>) -> bool,
) {
    let mut watch: Option<Watch> = None;
    // The verdict last told; none since the watch began.
    let mut said: Option<Option<Hot>> = None;
    let mut next = clock.now();
    loop {
        loop {
            match watches.try_recv() {
                Ok(w) => watch = w,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let Some(current) = watch.as_ref() else {
            sampler.reset();
            said = None;
            match watches.recv() {
                Ok(w) => {
                    watch = w;
                    next = clock.now();
                    continue;
                }
                Err(_) => return,
            }
        };
        let now = clock.now();
        // A clock of her own jumps: each tick of the jump is sampled, as
        // the wall clock would have been. Late on the wall clock (the
        // machine slept), now only.
        let mut at = if clock.is_virtual() {
            next.max(before(now, Window::SPAN)).min(now)
        } else {
            now
        };
        let mut hot = None;
        while at <= now {
            hot = sampler.sample(at, current);
            at = after(at, Sampler::EVERY);
        }
        if said != Some(hot) {
            if !told(hot) {
                return;
            }
            said = Some(hot);
        }
        next = at;
        clock.sleep_until(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().expect("a time")
    }

    /// A machine whose package and cards are files of `dir`; no
    /// `nvidia-smi`.
    fn sensors(dir: &Path) -> Sensors {
        let zone = dir.join("thermal/thermal_zone0");
        std::fs::create_dir_all(&zone).expect("a folder");
        std::fs::write(zone.join("type"), "x86_pkg_temp").expect("written");
        std::fs::create_dir_all(dir.join("hwmon")).expect("a folder");
        Sensors {
            supplies: dir.join("power_supply"),
            thermal: dir.join("thermal"),
            hwmon: dir.join("hwmon"),
            nvidia_smi: dir.join("no-nvidia-smi"),
        }
    }

    fn package_at(dir: &Path, celsius: u32) {
        std::fs::write(
            dir.join("thermal/thermal_zone0/temp"),
            format!("{}\n", celsius * 1000),
        )
        .expect("written");
    }

    fn cpu() -> Watch {
        Watch {
            gpu: false,
            triage: settings::Triage::default(),
        }
    }

    #[test]
    fn the_laptops_swing_never_pauses_a_steady_hundred_does_then_resumes_under_ninety() {
        let dir = tempfile::tempdir().expect("a folder");
        let mut sampler = Sampler::new(sensors(dir.path()));
        let watch = cpu();
        let mut now = at("2026-11-09T08:00:00Z");
        // The package of #756, a sample each tick, for five minutes.
        let swing = [101, 100, 100, 89, 72, 65, 64, 80, 64, 100];
        for celsius in swing.iter().cycle().take(60) {
            package_at(dir.path(), *celsius);
            assert_eq!(sampler.sample(now, &watch), None, "{celsius} °C, swinging");
            now = after(now, Sampler::EVERY);
        }
        // A steady 100 °C: paused once the minute's average reaches it.
        let mut ticks = 0;
        let hot = loop {
            package_at(dir.path(), 100);
            if let Some(hot) = sampler.sample(now, &watch) {
                break hot;
            }
            ticks += 1;
            assert!(ticks < 20, "never paused");
            now = after(now, Sampler::EVERY);
        };
        assert_eq!(
            hot,
            Hot {
                gauge: Gauge::Cpu,
                average: 100,
                resume: 90
            }
        );
        // 85 °C: still hot while the average is 90 or more, then cool.
        let mut ticks = 0;
        loop {
            now = after(now, Sampler::EVERY);
            package_at(dir.path(), 85);
            match sampler.sample(now, &watch) {
                Some(hot) => assert!(hot.average >= 90, "{hot:?}"),
                None => break,
            }
            ticks += 1;
            assert!(ticks < 20, "never resumed");
        }
        assert!(ticks > 3, "not on the first cool sample");
    }

    #[test]
    fn a_first_burst_alone_is_not_a_steady_reading() {
        let dir = tempfile::tempdir().expect("a folder");
        let mut sampler = Sampler::new(sensors(dir.path()));
        let mut now = at("2026-11-09T08:00:00Z");
        package_at(dir.path(), 105);
        for _ in 0..6 {
            assert_eq!(sampler.sample(now, &cpu()), None);
            now = after(now, Sampler::EVERY);
        }
        assert!(sampler.sample(now, &cpu()).is_some(), "30 s of it");
    }

    #[test]
    fn the_critical_point_and_the_settings_set_the_limits() {
        let dir = tempfile::tempdir().expect("a folder");
        let sensors = sensors(dir.path());
        assert_eq!(critical(&sensors), None);
        let zone = dir.path().join("thermal/thermal_zone0");
        std::fs::write(zone.join("trip_point_0_type"), "passive").expect("written");
        std::fs::write(zone.join("trip_point_0_temp"), "95000").expect("written");
        std::fs::write(zone.join("trip_point_1_type"), "critical").expect("written");
        std::fs::write(zone.join("trip_point_1_temp"), "110000").expect("written");
        assert_eq!(critical(&sensors), Some(110));
        let mut triage = settings::Triage::default();
        assert_eq!(
            Limits::cpu(&triage, Some(110)),
            Limits {
                pause: 100,
                resume: 90
            }
        );
        assert_eq!(
            Limits::cpu(&triage, Some(100)),
            Limits {
                pause: 90,
                resume: 80
            }
        );
        triage.heat = Some(95);
        triage.resume = Some(99);
        assert_eq!(
            Limits::cpu(&triage, Some(100)),
            Limits {
                pause: 95,
                resume: 94
            },
            "the settings', the resume point under the pause point"
        );
        assert_eq!(
            Limits::gpu(&settings::Triage::default(), None),
            Limits {
                pause: 87,
                resume: 77
            }
        );
        assert_eq!(
            Limits::gpu(&settings::Triage::default(), Some(92)),
            Limits {
                pause: 92,
                resume: 82
            }
        );
    }

    #[test]
    fn the_card_on_its_hwmon_is_watched_instead_of_the_package() {
        let dir = tempfile::tempdir().expect("a folder");
        let mut sampler = Sampler::new(sensors(dir.path()));
        let card = dir.path().join("hwmon/hwmon3");
        std::fs::create_dir_all(&card).expect("a folder");
        std::fs::write(card.join("name"), "amdgpu").expect("written");
        std::fs::write(card.join("temp1_input"), "90000").expect("written");
        package_at(dir.path(), 60);
        let gpu = Watch {
            gpu: true,
            triage: settings::Triage::default(),
        };
        let mut now = at("2026-11-09T08:00:00Z");
        let mut hot = None;
        for _ in 0..8 {
            hot = sampler.sample(now, &gpu);
            now = after(now, Sampler::EVERY);
        }
        assert_eq!(
            hot,
            Some(Hot {
                gauge: Gauge::Gpu,
                average: 90,
                resume: 77
            })
        );
        // The same machine, the model on the processor: the package's 60.
        sampler.reset();
        for _ in 0..8 {
            hot = sampler.sample(now, &cpu());
            now = after(now, Sampler::EVERY);
        }
        assert_eq!(hot, None);
    }

    #[test]
    fn nvidia_smi_says_each_cards_temperature_and_margin() {
        assert_eq!(smi("45, 42\n"), Some((45, Some(87))));
        assert_eq!(smi("45, [N/A]\n"), Some((45, None)));
        assert_eq!(smi("50\n"), Some((50, None)));
        assert_eq!(smi("40, 50\n88, -1\n"), Some((88, Some(87))));
        assert_eq!(smi("No devices were found\n"), None);
    }
}
