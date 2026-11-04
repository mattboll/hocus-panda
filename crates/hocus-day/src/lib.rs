//! "Your day"'s data (docs/design/day.md): calendars, events and tasks,
//! from sources behind adapters ([`source`]), each account read by a
//! worker on a thread of its own ([`Workers`]), never on the shell's loop.
//! The workers send owned [`Snapshot`]s through a channel; the shell keeps
//! the latest of each in a [`Day`], which its views read.
//!
//! No smithay, no iced: the shell's `day` module starts the workers and
//! holds the day; the adapters live here, each in its own module.

pub mod config;
pub mod edit;
pub mod fixture;
pub mod goa;
pub mod google;
mod http;
pub mod local;
pub mod model;
pub mod planka;
pub mod reminders;
pub mod secrets;
pub mod snapshot;
pub mod source;
mod worker;

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};

use jiff::Zoned;

pub use crate::{
    config::Config,
    snapshot::{
        Account, Change, ChangeId, Day, Holds, Kind, Login, MonthState, Pending, Shown, Snapshot,
        State, Update,
    },
    source::{CalendarSource, KanbanSource, SourceError},
};
use crate::{
    model::AccountId,
    secrets::Secrets,
    worker::{Command, Handle, Plan, Source},
};

/// The wall clock the day is read against: the shell's (its time zone,
/// and `HOCUS_CLOCK_AT`'s shift for tests and recordings).
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> Zoned + Send + Sync>);

impl Clock {
    pub fn new(now: impl Fn() -> Zoned + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    /// The system's clock, in the system's time zone.
    pub fn system() -> Self {
        Self::new(Zoned::now)
    }

    pub fn now(&self) -> Zoned {
        (self.0)()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock")
    }
}

impl Kind {
    /// How often an account of this kind is read again, besides on
    /// request and on resume; `None`: only then (a file is read on its
    /// changes, the local list is its own).
    pub fn every(self) -> Option<Duration> {
        match self {
            Self::Google => Some(Duration::from_secs(5 * 60)),
            Self::Planka => Some(Duration::from_secs(2 * 60)),
            Self::Fixture | Self::Local => None,
        }
    }

    /// Whether its data is cached, to show at once on start and offline.
    fn cached(self) -> bool {
        matches!(self, Self::Google | Self::Planka)
    }
}

/// Where the workers keep things, and what they read against.
#[derive(Debug, Clone)]
pub struct Env {
    pub clock: Clock,
    /// The accounts' tokens.
    pub secrets: Secrets,
    /// `$XDG_STATE_HOME/hocuspanda`: the local list's `tasks.json`.
    pub state_dir: Option<PathBuf>,
    /// `$XDG_CACHE_HOME/hocuspanda/day`: each account's cache.
    pub cache_dir: Option<PathBuf>,
    /// The configuration's folder: a fixture's relative file is there.
    pub config_dir: Option<PathBuf>,
    /// The session bus the compositor names (`--session-bus`; "session":
    /// the session's own), where Online Accounts is; `None`: no Google
    /// calendars.
    pub session_bus: Option<String>,
    /// The Calendar API's address: [`google::API`], or a fake's on this
    /// machine for tests ([`google::API_ENV`]).
    pub google_api: String,
}

/// Starts and stops workers, from the thread that keeps the day or
/// Online Accounts' (accounts that come and go).
#[derive(Debug, Clone)]
pub(crate) struct Launcher {
    handles: Arc<Mutex<Vec<Handle>>>,
    clock: Clock,
    cache_dir: Option<PathBuf>,
    out: mpsc::Sender<Update>,
}

impl Launcher {
    fn handles(&self) -> std::sync::MutexGuard<'_, Vec<Handle>> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `plan`'s worker started, with its kind's schedule and cache:
    /// whether it runs.
    pub(crate) fn launch(&self, mut plan: Plan) -> bool {
        if plan.account.kind.cached() {
            plan.cache = self
                .cache_dir
                .as_ref()
                .map(|dir| dir.join(format!("{}.json", plan.account.id)));
        }
        plan.every = plan.account.kind.every();
        match worker::spawn(plan, self.clock.clone(), self.out.clone()) {
            Some(handle) => {
                self.handles().push(handle);
                true
            }
            None => false,
        }
    }

    /// `account`'s worker stopped and the account said gone; its cache
    /// forgotten if `forget`.
    pub(crate) fn stop(&self, account: &AccountId, forget: bool) {
        let stopped: Vec<Handle> = {
            let mut handles = self.handles();
            let (stopped, kept) = std::mem::take(&mut *handles)
                .into_iter()
                .partition(|h| h.account == *account);
            *handles = kept;
            stopped
        };
        for handle in stopped {
            handle.stop(&self.out, forget);
        }
    }

    /// `account` read again now.
    pub(crate) fn refresh(&self, account: &AccountId) {
        for handle in self.handles().iter().filter(|h| h.account == *account) {
            handle.send(Command::Refresh);
        }
    }
}

/// The running workers, one per account.
#[derive(Debug)]
pub struct Workers {
    launcher: Launcher,
    /// The bar's tasks' account ([`Day::tasks`]).
    tasks: Option<AccountId>,
    next: u64,
    /// What they were started with, for [`Self::reconfigure`].
    env: Env,
    /// Online Accounts followed, while a Google entry is set.
    goa: Option<goa::Watch>,
}

impl Workers {
    /// Starts a worker for each account of `config`, and one for the local
    /// list, each sending what it reads to `out`; the day as it is before
    /// they say anything (each account loading, or failed when it cannot
    /// be read at all). Only the first `[[day.tasks]]` is used. Google
    /// accounts come from Online Accounts as it lists them, later, each
    /// with its worker ([`goa`]).
    pub fn start(config: &Config, env: &Env, out: &mpsc::Sender<Update>) -> (Self, Day) {
        let local = env.state_dir.as_ref().map(|dir| dir.join("tasks.json"));
        let mut plans = plans(config, env);
        plans.push(Ok(Plan {
            account: config::local_account(),
            make: Box::new(move || Source::Tasks {
                source: Box::new(local::LocalList::open(local)),
                names: source::ColumnNames {
                    tasks: local::TODO.to_owned(),
                    done: local::DONE.to_owned(),
                    add: String::new(),
                },
            }),
            every: None,
            cache: None,
            watch: None,
        }));
        let launcher = Launcher {
            handles: Arc::default(),
            clock: env.clock.clone(),
            cache_dir: env.cache_dir.clone(),
            out: out.clone(),
        };
        let mut accounts = Vec::new();
        for plan in plans {
            match plan {
                Ok(plan) => {
                    accounts.push(Snapshot::loading(plan.account.clone()));
                    launcher.launch(plan);
                }
                Err(failed) => accounts.push(*failed),
            }
        }
        let goa = follow_google(config, env, &launcher);
        let day = Day::new(accounts);
        let tasks = day.tasks().map(|s| s.account.id.clone());
        (
            Self {
                launcher,
                tasks,
                next: 0,
                env: env.clone(),
                goa,
            },
            day,
        )
    }

    /// The accounts of `config` from now on (Day.1's choices written): every
    /// worker but the local list's stopped, each account said gone, then
    /// `config`'s started, as [`Self::start`] does. Their caches are kept:
    /// an account that stays shows its last day at once. Blocking (Online
    /// Accounts' thread is waited for): off the loop.
    pub fn reconfigure(&mut self, config: &Config) {
        if let Some(goa) = self.goa.take() {
            goa.end();
        }
        let local = config::local_account().id;
        let running: Vec<AccountId> = self
            .launcher
            .handles()
            .iter()
            .map(|h| h.account.clone())
            .filter(|id| *id != local)
            .collect();
        for id in &running {
            self.launcher.stop(id, false);
        }
        let mut tasks = None;
        for plan in plans(config, &self.env) {
            match plan {
                Ok(plan) => {
                    if plan.account.holds == Holds::Tasks {
                        tasks.get_or_insert_with(|| plan.account.id.clone());
                    }
                    let _ = self
                        .launcher
                        .out
                        .send(Update::Snapshot(Box::new(Snapshot::loading(
                            plan.account.clone(),
                        ))));
                    self.launcher.launch(plan);
                }
                Err(failed) => {
                    if failed.account.holds == Holds::Tasks {
                        tasks.get_or_insert_with(|| failed.account.id.clone());
                    }
                    let _ = self.launcher.out.send(Update::Snapshot(failed));
                }
            }
        }
        self.tasks = Some(tasks.unwrap_or(local));
        self.goa = follow_google(config, &self.env, &self.launcher);
        tracing::info!(
            stopped = running.len(),
            "the day's accounts started again from the configuration"
        );
    }

    /// Online Accounts no longer followed by these workers: the watch, for
    /// the caller to end where waiting for it blocks nothing (its Google
    /// workers stay until [`Self::reconfigure`]).
    pub fn take_online_accounts(&mut self) -> Option<goa::Watch> {
        self.goa.take()
    }

    /// `change` to the bar's tasks' account, at once: its number, or
    /// `None` when that account has no worker.
    pub fn change(&mut self, change: Change) -> Option<ChangeId> {
        let account = self.tasks.as_ref()?;
        let handle = self
            .launcher
            .handles()
            .iter()
            .find(|h| h.account == *account)?
            .clone();
        self.next += 1;
        let id = ChangeId(self.next);
        handle.send(Command::Change(id, change)).then_some(id)
    }

    /// The months of `months` (any of their days) that are outside the
    /// window, asked of every calendars' account: each reads those it has
    /// not read already, off the loop, and sends them in its next
    /// snapshot ([`Snapshot::months`], Day.5b). It never blocks.
    pub fn months(&self, months: &[jiff::civil::Date]) {
        for handle in self
            .launcher
            .handles()
            .iter()
            .filter(|h| h.holds == Holds::Calendars)
        {
            handle.send(Command::Months(months.to_vec()));
        }
    }

    /// Every account read again now (on request, on resume).
    pub fn refresh(&self) {
        for handle in self.launcher.handles().iter() {
            handle.send(Command::Refresh);
        }
    }

    /// Waits, up to `within` for each, until what was asked of the workers
    /// is done (the local list written): before the shell ends.
    pub fn flush(&self, within: Duration) {
        let handles = self.launcher.handles().clone();
        for handle in &handles {
            if !handle.flush(within) {
                tracing::info!(account = %handle.account, "an account of the day did not finish in time");
            }
        }
    }
}

/// The workers `config`'s accounts need, the local list's aside, in the
/// configuration's order; those that cannot be read at all as their
/// snapshot. Only the first `[[day.tasks]]` is used; Google's come from
/// Online Accounts ([`follow_google`]).
fn plans(config: &Config, env: &Env) -> Vec<Result<Plan, Box<Snapshot>>> {
    let base = env.config_dir.as_deref();
    let mut taken = vec![config::local_account().id];
    let mut plans: Vec<Result<Plan, Box<Snapshot>>> = Vec::new();
    for entry in &config.calendars {
        if entry.kind == config::CalendarKind::Google {
            continue;
        }
        let account = entry.account(base, &mut taken);
        plans.push(match entry.kind {
            config::CalendarKind::Fixture => {
                let (shown, colours) = (entry.shown.clone(), entry.colors.clone());
                fixture_plan(
                    account,
                    entry.file.as_deref(),
                    env,
                    move |file, id, clock| Source::Calendars {
                        source: Box::new(fixture::FixtureCalendars::new(file, id, clock)),
                        shown,
                        colours,
                    },
                )
            }
            config::CalendarKind::Google => continue,
        });
    }
    for (i, entry) in config.tasks.iter().enumerate() {
        if i > 0 {
            tracing::warn!("only the first [[day.tasks]] is used: the others are left out");
            break;
        }
        let account = entry.account(base, &mut taken);
        plans.push(match entry.kind {
            config::TaskKind::Fixture => {
                let names = entry.columns();
                fixture_plan(
                    account,
                    entry.file.as_deref(),
                    env,
                    move |file, id, clock| Source::Tasks {
                        source: Box::new(fixture::FixtureTasks::new(file, id, clock)),
                        names,
                    },
                )
            }
            config::TaskKind::Planka => planka_plan(account, entry, env),
        });
    }
    plans
}

/// Online Accounts followed for `config`'s Google entries, if it has any.
fn follow_google(config: &Config, env: &Env, launcher: &Launcher) -> Option<goa::Watch> {
    let google: Vec<config::CalendarEntry> = config
        .calendars
        .iter()
        .filter(|e| e.kind == config::CalendarKind::Google)
        .cloned()
        .collect();
    if google.is_empty() {
        return None;
    }
    match &env.session_bus {
        Some(bus) => goa::watch(
            goa::Google {
                entries: google,
                bus: bus.clone(),
                api: env.google_api.clone(),
            },
            launcher.clone(),
        ),
        None => {
            tracing::info!("no session bus, so no Online Accounts: no Google calendars");
            None
        }
    }
}

/// A Planka board's worker: its server and board from the entry, its token
/// from the secrets when it first reads.
fn planka_plan(
    account: Account,
    entry: &config::TaskEntry,
    env: &Env,
) -> Result<Plan, Box<Snapshot>> {
    let (server, board) = match planka::address(&entry.address, &entry.board) {
        Ok(found) => found,
        Err(error) => return Err(Box::new(Snapshot::failed(account, error))),
    };
    let (id, secrets, names) = (account.id.clone(), env.secrets.clone(), entry.columns());
    Ok(Plan {
        account,
        make: Box::new(move || Source::Tasks {
            source: Box::new(planka::Planka::new(server, board, id, secrets)),
            names,
        }),
        every: None,
        cache: None,
        watch: None,
    })
}

/// A fixture's worker, its source made by `make` from its file.
fn fixture_plan(
    account: Account,
    file: Option<&Path>,
    env: &Env,
    make: impl FnOnce(PathBuf, AccountId, Clock) -> Source + Send + 'static,
) -> Result<Plan, Box<Snapshot>> {
    let Some(file) = file else {
        return Err(Box::new(Snapshot::failed(
            account,
            SourceError::Invalid("a fixture names its file (file = …)".to_owned()),
        )));
    };
    let file = config::resolve(file, env.config_dir.as_deref());
    let (id, clock, read) = (account.id.clone(), env.clock.clone(), file.clone());
    Ok(Plan {
        account,
        make: Box::new(move || make(read, id, clock)),
        every: None,
        cache: None,
        watch: fixture::watchable(&file).then_some(file),
    })
}

/// `bytes` in `file`, readable by the user alone, whole or not at all.
pub(crate) fn write_file(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::{io::Write as _, os::unix::fs::OpenOptionsExt};
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut new = file.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&new)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::rename(&new, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(dir: &Path) -> Env {
        let now = jiff::civil::date(2026, 11, 4)
            .at(14, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("a time");
        Env {
            clock: Clock::new(move || now.clone()),
            secrets: Secrets::default(),
            state_dir: Some(dir.join("state")),
            cache_dir: Some(dir.join("cache")),
            config_dir: Some(dir.to_path_buf()),
            session_bus: None,
            google_api: google::API.to_owned(),
        }
    }

    /// Snapshots until `ok` says the day is as expected.
    fn until(
        day: &mut Day,
        updates: &mpsc::Receiver<Update>,
        ok: impl Fn(&Day) -> bool,
    ) -> anyhow::Result<()> {
        while !ok(day) {
            let update = updates.recv_timeout(Duration::from_secs(10))?;
            day.apply(update);
        }
        Ok(())
    }

    #[test]
    fn a_fixture_day_with_the_local_list() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("day.toml"),
            "[[calendars]]\nname = \"Travail\"\n\
             [[events]]\ntitle = \"Point produit\"\nstart = \"now+25m\"\nend = \"now+55m\"\n\
             [tasks]\n[[tasks.cards]]\ntitle = \"Relire\"\n",
        )?;
        let config = config::from_toml(
            "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\
             [[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\
             [[day.tasks]]\nkind = \"planka\"\n",
        )
        .map_err(anyhow::Error::msg)?;
        let env = env(dir.path());
        let (out, snapshots) = mpsc::channel();
        let (mut workers, mut day) = Workers::start(&config, &env, &out);
        let ids: Vec<&str> = day
            .accounts()
            .iter()
            .map(|s| s.account.id.as_str())
            .collect();
        assert_eq!(ids, ["fixture-calendars-day", "fixture-tasks-day", "local"]);
        let now = env.clock.now();
        until(&mut day, &snapshots, |d| {
            d.summary(&now).contains("14:25 «Point produit» (Travail)")
                && d.summary(&now).contains("«Relire»")
        })?;
        // The fixture's board is the bar's tasks: a change goes there.
        assert!(
            workers
                .change(Change::Add {
                    title: "Nouvelle".to_owned()
                })
                .is_some()
        );
        until(&mut day, &snapshots, |d| {
            d.tasks()
                .is_some_and(|t| t.tasks.first().is_some_and(|t| t.title == "Nouvelle"))
        })?;
        // Changing the file shows the change, the fixture's tasks reset.
        std::fs::write(
            dir.path().join("day.toml"),
            "[[calendars]]\nname = \"Travail\"\n\
             [[events]]\ntitle = \"Revue\"\nstart = \"today 16:00\"\n\
             [tasks]\n",
        )?;
        until(&mut day, &snapshots, |d| {
            let s = d.summary(&now);
            s.contains("16:00 «Revue»") && !s.contains("Point produit") && s.contains("tasks=0")
        })?;
        workers.flush(Duration::from_secs(10));
        Ok(())
    }

    #[test]
    fn reconfigured_the_new_accounts_start_and_the_local_list_stays() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("day.toml"),
            "[tasks]\n[[tasks.cards]]\ntitle = \"Relire\"\n",
        )?;
        let env = env(dir.path());
        let (out, snapshots) = mpsc::channel();
        let (mut workers, mut day) = Workers::start(&Config::default(), &env, &out);
        let now = env.clock.now();
        until(&mut day, &snapshots, |d| {
            d.summary(&now).contains("local fresh")
        })?;
        // Day.1's choice written: the board's account starts, and is the
        // bar's tasks; the local list goes on.
        let config = config::from_toml("[[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n")
            .map_err(anyhow::Error::msg)?;
        workers.reconfigure(&config);
        until(&mut day, &snapshots, |d| {
            let s = d.summary(&now);
            s.contains("fixture-tasks-day fresh")
                && s.contains("local fresh")
                && s.contains("«Relire»")
        })?;
        assert!(
            workers
                .change(Change::Add {
                    title: "Nouvelle".to_owned()
                })
                .is_some()
        );
        until(&mut day, &snapshots, |d| {
            d.tasks().is_some_and(|t| {
                t.account.id.as_str() == "fixture-tasks-day"
                    && t.tasks.first().is_some_and(|t| t.title == "Nouvelle")
            })
        })?;
        // And back to none: the board's account is gone.
        workers.reconfigure(&Config::default());
        until(&mut day, &snapshots, |d| {
            d.accounts()
                .iter()
                .map(|s| s.account.id.as_str())
                .collect::<Vec<_>>()
                == ["local"]
        })?;
        workers.flush(Duration::from_secs(10));
        Ok(())
    }
}
