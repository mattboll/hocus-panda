//! "Your day"'s data in the shell (docs/design/day.md): the workers of
//! `hocus-day`, one per account of the configuration's `[day]` and one for
//! the local list, started as the shell starts; and the latest [`Day`]
//! they make, which the bar's views read ([`snapshot`]).
//!
//! The workers run on threads of their own and send their snapshots to
//! one more thread, which keeps the day: the shell's loop is never woken
//! for them, and the views only when the day changed ([`on_change`]).
//! Each change is said in the log at debug level ("the day holds …"), for
//! tests. Every account is read again when the machine resumes (logind's
//! `PrepareForSleep`, on a thread of its own), and on request
//! ([`hocus_link::DayAsk::Refresh`]).

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError, mpsc},
    time::Duration,
};

use hocus_day::{
    Change, Clock, Config, Day, Env, Snapshot, Workers, model::TaskId, secrets::Secrets,
};
use hocus_link::DayAsk;

type Waker = Box<dyn FnMut() -> bool + Send>;

struct Service {
    day: Mutex<Arc<Day>>,
    workers: Mutex<Workers>,
    waker: Mutex<Option<Waker>>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

/// How long the shell, ending, waits for each account's worker to finish
/// what it was asked (the local list written).
const FLUSH: Duration = Duration::from_secs(2);

/// Starts the day, once, on a thread of its own (the configuration read
/// there, not where the shell starts): after the notifications, whose wall
/// clock (the system's zone, `HOCUS_CLOCK_AT`) the day is read against.
pub fn start() {
    let kept = std::thread::Builder::new()
        .name("day".to_owned())
        .spawn(|| {
            let Some(snapshots) = start_workers() else {
                return;
            };
            for snapshot in snapshots {
                publish(snapshot);
            }
        });
    if let Err(err) = kept {
        tracing::warn!(%err, "no thread to keep the day");
        return;
    }
    watch_resume();
}

/// The workers started from `[day]`, and what they send.
fn start_workers() -> Option<mpsc::Receiver<Snapshot>> {
    let file = config_file();
    let config = match &file {
        Some(file) => hocus_day::config::read(file).unwrap_or_else(|why| {
            tracing::warn!("[day] refused, no account: {why}");
            Config::default()
        }),
        None => Config::default(),
    };
    let state = crate::launcher::apps::state_file("");
    let bus = std::env::var(hocus_link::NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty());
    let env = Env {
        clock: Clock::new(crate::notifications::wall),
        secrets: Secrets::new(bus, state.as_ref().map(|s| s.join("accounts"))),
        state_dir: state,
        cache_dir: cache_dir(),
        config_dir: file.as_deref().and_then(|f| f.parent()).map(PathBuf::from),
    };
    let (out, snapshots) = mpsc::channel();
    let (workers, day) = Workers::start(&config, &env, &out);
    tracing::info!(
        accounts = day.accounts().len(),
        "the day's accounts started"
    );
    let service = Service {
        day: Mutex::new(Arc::new(day)),
        workers: Mutex::new(workers),
        waker: Mutex::new(None),
    };
    SERVICE.set(service).is_ok().then_some(snapshots)
}

/// The configuration file the compositor names (none when it names an
/// empty one); the shell alone: `$XDG_CONFIG_HOME/hocuspanda/config.toml`.
fn config_file() -> Option<PathBuf> {
    match std::env::var_os(hocus_link::CONFIG_ENV) {
        Some(file) if file.is_empty() => None,
        Some(file) => Some(PathBuf::from(file)),
        None => {
            let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
            let config = var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
            config
                .is_absolute()
                .then(|| config.join("hocuspanda/config.toml"))
        }
    }
}

/// `$XDG_CACHE_HOME/hocuspanda/day`, or under `~/.cache`.
fn cache_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let cache = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    cache.is_absolute().then(|| cache.join("hocuspanda/day"))
}

/// The day as it is now.
pub fn snapshot() -> Arc<Day> {
    SERVICE.get().map_or_else(Arc::default, |s| {
        s.day.lock().unwrap_or_else(PoisonError::into_inner).clone()
    })
}

/// A worker's snapshot: kept, said, and shown if the day changed.
fn publish(snapshot: Snapshot) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    {
        let mut day = service.day.lock().unwrap_or_else(PoisonError::into_inner);
        if !Arc::make_mut(&mut day).update(snapshot) {
            return;
        }
    }
    tracing::debug!(
        "the day holds {}",
        self::snapshot().summary(&crate::notifications::wall())
    );
    let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
    if waker.as_mut().is_some_and(|wake| !wake()) {
        *waker = None;
    }
}

/// From now on, a change of the day calls `wake` (until it returns
/// `false`); it is called once now, for what came before.
#[expect(
    dead_code,
    reason = "the bar's views of the day (Day.2 to Day.4) wake by it"
)]
pub fn on_change(wake: impl FnMut() -> bool + Send + 'static) {
    if let Some(service) = SERVICE.get() {
        let mut wake: Waker = Box::new(wake);
        let alive = wake();
        *service.waker.lock().unwrap_or_else(PoisonError::into_inner) = alive.then_some(wake);
    }
}

/// A change of the bar's tasks, sent at once to their account.
pub fn change(change: Change) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let id = service
        .workers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .change(change.clone());
    match id {
        Some(id) => tracing::debug!(id = id.0, ?change, "a change of the day's tasks"),
        None => tracing::info!(?change, "the day's tasks take no change"),
    }
}

/// Every account read again now.
pub fn refresh() {
    if let Some(service) = SERVICE.get() {
        service
            .workers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh();
    }
}

/// What the compositor's `day` line asks.
pub fn ask(ask: DayAsk) {
    match ask {
        DayAsk::Add(title) => change(Change::Add { title }),
        DayAsk::Tick(task) => change(Change::Tick {
            task: TaskId::new(task),
        }),
        DayAsk::Untick(task) => change(Change::Untick {
            task: TaskId::new(task),
        }),
        DayAsk::Refresh => refresh(),
    }
}

/// Waits for the workers to finish what they were asked: before the shell
/// ends.
pub fn flush() {
    if let Some(service) = SERVICE.get() {
        service
            .workers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .flush(FLUSH);
    }
}

/// Reads every account again as the machine resumes: logind's
/// `PrepareForSleep(false)` on the system bus, on a thread of its own.
/// Without a system bus (tests), nothing is watched, and nothing retried.
fn watch_resume() {
    let started = std::thread::Builder::new()
        .name("day: resume".to_owned())
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(resumes()),
                Err(err) => tracing::warn!(%err, "no runtime to watch for resumes"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch for resumes");
    }
}

async fn resumes() {
    use iced::futures::StreamExt;
    let watched = async {
        let connection = zbus::Connection::system().await?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.login1")
            .interface("org.freedesktop.login1.Manager")
            .member("PrepareForSleep")
            .path("/org/freedesktop/login1")
            .build()?;
        zbus::MessageStream::for_match_rule(rule, &connection, None).await
    };
    let mut stream = match watched.await {
        Ok(stream) => stream,
        Err(err) => {
            tracing::info!(%err, "no logind: the day is not read again on resume");
            return;
        }
    };
    while let Some(message) = stream.next().await {
        let Ok(message) = message else {
            continue;
        };
        if let Ok(false) = message.body().deserialize::<bool>() {
            tracing::info!("resumed: the day read again");
            refresh();
        }
    }
}
