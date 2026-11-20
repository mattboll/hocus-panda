//! The battery's charge limit (6.10, issue #896, docs/design/settings.md
//! "Énergie"): `[power] charge-limit`, the battery's own threshold at
//! 80 %, written by the package's helper as root ([`hocus_charge`]), off
//! the loop.
//!
//! - **Applied** as the session starts while the file says on (80 %, or
//!   100 % while lifted), and as the file changes (on: 80 %, off: 100 %).
//!   The firmware keeps it, so applying it again changes nothing: a value
//!   the batteries already hold is not written, and pkexec is not run.
//! - **"Charger à 100 % une fois"** (the Settings' line `charge-full-once`):
//!   100 % until the next unplug (UPower's `OnBattery`), then 80 % again.
//!   The lift is kept in `$XDG_STATE_HOME/hocuspanda/charge-lifted` while
//!   it holds, so that a restart does not forget it.
//! - **Told**: the shell, the limit that holds (the battery gauge's mark,
//!   the popover's "Branché, limité à 80 %"); the Settings, whether it is
//!   lifted.
//! - **Where**: on a virtual terminal, the session's battery, through
//!   `pkexec` and the package's helper; else only with a helper given
//!   (`HOCUS_CHARGE_HELPER`, a test's [`Helper`]), run as itself. A
//!   nested or headless compositor never touches the machine's battery.

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
};

use hocus_charge::Target;
use hocus_link::ToShell;
use smithay::reexports::calloop::channel::{self, Event};

use crate::State;

/// The file that says a lift waits for the next unplug.
const LIFTED_FILE: &str = "charge-lifted";

/// What sets the thresholds, and where the kernel's supplies are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    /// The helper (`/usr/libexec/hocuspanda/charge-limit`), given `80` or
    /// `100`.
    pub program: PathBuf,
    /// Run through `pkexec` (the session's), else as is (a test's).
    pub pkexec: bool,
    /// `/sys/class/power_supply`, or a test's.
    pub supplies: PathBuf,
}

impl Helper {
    /// The package's helper, beside the compositor's own folder:
    /// `/usr/bin/hocuspanda` → `/usr/libexec/hocuspanda/charge-limit`, the
    /// path polkit's action names.
    pub fn packaged(exe: &Path, supplies: PathBuf) -> Option<Self> {
        let prefix = exe.parent()?.parent()?;
        Some(Self {
            program: prefix.join("libexec/hocuspanda/charge-limit"),
            pkexec: true,
            supplies,
        })
    }
}

/// What the loop keeps of it.
#[derive(Debug, Default)]
pub struct Charge {
    to_worker: Option<mpsc::Sender<Ask>>,
    /// 100 % until the next unplug.
    lifted: bool,
    /// A battery has the threshold, as the worker last found; `None`
    /// until it looked.
    supported: Option<bool>,
    /// What the shell was told last.
    told: Option<Option<u8>>,
}

/// What the loop asks of the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Set(Target),
    /// The lift kept (`true`) or forgotten.
    Keep(bool),
}

/// What the worker says back: whether a battery has the threshold.
#[derive(Debug, Clone, Copy)]
pub struct Heard {
    supported: bool,
}

impl State {
    /// As the loop starts: the worker, the lift kept, and the limit set
    /// while the file says on.
    pub(crate) fn start_charge(&mut self) {
        let Some(helper) = self.config.charge.clone() else {
            tracing::debug!("no charge limit here: the session's battery is not this one's");
            return;
        };
        let lifted_file = self.config.state_dir.as_ref().map(|d| d.join(LIFTED_FILE));
        let lifted = lifted_file.as_deref().is_some_and(Path::exists);
        let (sender, receiver) = channel::channel::<Heard>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(heard) = event {
                app.state.charge_heard(heard);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "no charge limit: its worker cannot answer");
            return;
        }
        let (to_worker, asks) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("charge limit".to_owned())
            .spawn(move || work(&helper, lifted_file.as_deref(), &asks, &sender));
        if let Err(err) = started {
            tracing::warn!(%err, "no charge limit: its worker does not start");
            return;
        }
        let charge = &mut self.away.charge;
        charge.to_worker = Some(to_worker);
        if self.config.power.charge_limit {
            charge.lifted = lifted;
            tracing::info!(lifted, "the battery's charge limit, as the session starts");
            self.charge_ask(Ask::Set(if lifted { Target::Full } else { Target::Limit }));
        } else if lifted {
            self.charge_ask(Ask::Keep(false));
        }
    }

    fn charge_ask(&self, ask: Ask) {
        if let Some(to) = &self.away.charge.to_worker {
            let _ = to.send(ask);
        }
    }

    /// `[power] charge-limit` was `was` and changed: on, 80 %; off, 100 %;
    /// a lift forgotten either way.
    pub(crate) fn charge_config_changed(&mut self, was: bool) {
        let now = self.config.power.charge_limit;
        if was == now || self.away.charge.to_worker.is_none() {
            return;
        }
        tracing::info!(on = now, "the battery's charge limit");
        if self.away.charge.lifted {
            self.away.charge.lifted = false;
            self.charge_ask(Ask::Keep(false));
        }
        self.charge_ask(Ask::Set(if now { Target::Limit } else { Target::Full }));
        self.charge_tell();
    }

    /// "Charger à 100 % une fois": the limit lifted until the next
    /// unplug. Refused, why.
    pub(crate) fn charge_lift(&mut self) -> Result<(), &'static str> {
        if self.away.charge.to_worker.is_none() {
            return Err("no charge limit in this session");
        }
        if !self.config.power.charge_limit {
            return Err("the charge limit is off");
        }
        if self.away.charge.supported == Some(false) {
            return Err("no battery here has a charge threshold");
        }
        if !self.away.charge.lifted {
            tracing::info!("the battery's charge limit lifted until the next unplug");
            self.away.charge.lifted = true;
            self.charge_ask(Ask::Keep(true));
            self.charge_ask(Ask::Set(Target::Full));
        }
        self.charge_tell();
        Ok(())
    }

    /// Unplugged (UPower's `OnBattery`): a lift ends, the limit back.
    pub(crate) fn charge_unplugged(&mut self) {
        if !self.away.charge.lifted {
            return;
        }
        tracing::info!("unplugged: the battery's charge limit is back");
        self.away.charge.lifted = false;
        self.charge_ask(Ask::Keep(false));
        if self.config.power.charge_limit {
            self.charge_ask(Ask::Set(Target::Limit));
        }
        self.charge_tell();
    }

    fn charge_heard(&mut self, heard: Heard) {
        if self.away.charge.supported != Some(heard.supported) {
            tracing::info!(
                supported = heard.supported,
                "whether the battery's charge can be limited"
            );
        }
        self.away.charge.supported = Some(heard.supported);
        self.charge_tell();
    }

    /// The limit that holds now, for the shell: 80 while the file says
    /// on, it is not lifted and a battery has the threshold.
    pub fn charge_limit_shown(&self) -> Option<u8> {
        let charge = &self.away.charge;
        (self.config.power.charge_limit && !charge.lifted && charge.supported == Some(true))
            .then_some(Target::Limit.end())
    }

    /// Whether the limit is lifted until the next unplug.
    pub fn charge_lifted(&self) -> bool {
        self.away.charge.lifted
    }

    /// What the Settings' line says of it.
    pub(crate) fn charge_line(&self) -> String {
        serde_json::json!({ "charge": { "lifted": self.away.charge.lifted } }).to_string()
    }

    fn charge_tell(&mut self) {
        let shown = self.charge_limit_shown();
        if self.away.charge.told != Some(shown) {
            self.away.charge.told = Some(shown);
            self.tell_shell(ToShell::ChargeLimit(shown));
        }
        let line = self.charge_line();
        self.displays_send(None, &line);
    }

    /// The shell connected: the limit that holds.
    pub(crate) fn charge_shell_connected(&mut self) {
        self.away.charge.told = None;
        let shown = self.charge_limit_shown();
        self.away.charge.told = Some(shown);
        self.tell_shell(ToShell::ChargeLimit(shown));
    }
}

/// The worker: the lift kept or forgotten, the thresholds set (the last
/// of a burst), whether a battery has them said back.
fn work(
    helper: &Helper,
    lifted_file: Option<&Path>,
    asks: &mpsc::Receiver<Ask>,
    to_loop: &channel::Sender<Heard>,
) {
    while let Ok(ask) = asks.recv() {
        let mut target = None;
        for ask in std::iter::once(ask).chain(asks.try_iter()) {
            match ask {
                Ask::Set(t) => target = Some(t),
                Ask::Keep(on) => keep(lifted_file, on),
            }
        }
        if let Some(target) = target
            && to_loop
                .send(Heard {
                    supported: set(helper, target),
                })
                .is_err()
        {
            return;
        }
    }
}

fn keep(file: Option<&Path>, on: bool) {
    let Some(file) = file else {
        return;
    };
    let done = if on {
        file.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(file, "100\n"))
    } else {
        match std::fs::remove_file(file) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    };
    if let Err(err) = done {
        tracing::warn!(%err, file = %file.display(), on, "the charge limit's lift is not kept");
    }
}

/// Sets `target`, unless every battery holds it already; whether a
/// battery has the threshold.
fn set(helper: &Helper, target: Target) -> bool {
    if !hocus_charge::supported(&helper.supplies) {
        tracing::info!("no battery has a charge threshold: no charge limit");
        return false;
    }
    if hocus_charge::holds(&helper.supplies, target) {
        tracing::debug!(
            end = target.end(),
            "the battery holds its charge limit already"
        );
        return true;
    }
    let mut command = if helper.pkexec {
        let mut command = Command::new("pkexec");
        command.arg(&helper.program);
        command
    } else {
        Command::new(&helper.program)
    };
    command
        .arg(target.arg())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    match command.output() {
        Ok(out) if out.status.success() => {
            tracing::info!(end = target.end(), "the battery's charge limit set");
        }
        Ok(out) => tracing::warn!(
            end = target.end(),
            status = %out.status,
            said = %String::from_utf8_lossy(&out.stderr).trim(),
            program = %helper.program.display(),
            "the battery's charge limit was not set"
        ),
        Err(err) => tracing::warn!(
            %err,
            program = %helper.program.display(),
            pkexec = helper.pkexec,
            "the battery's charge limit's helper does not run"
        ),
    }
    true
}
