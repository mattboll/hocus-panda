//! logind for the session menu (7.4), on a thread of its own with its own
//! runtime, never on the loop: what it allows (`CanSuspend`, `CanReboot`,
//! `CanPowerOff`), its `block` inhibitors of `shutdown` while a
//! confirmation shows (read again every second: logind says nothing when
//! one comes or goes but a property change some versions send), the
//! password asked first through polkit when it says `challenge`, and the
//! calls themselves (`Suspend`, `Reboot`, `PowerOff`, `interactive` so
//! that polkit asks through the desktop's dialog, 7.5, when it must).
//!
//! The bus is the one given (`--system-bus`), else the system's on a
//! virtual terminal only: nested or headless without it there is no
//! logind, and the developer's machine is never asked to power off.

use std::time::Duration;

use hocus_link::{Can, LeaveAction, LeaveCan};
use smithay::reexports::calloop::channel::Sender;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{Connection, connection::Builder};

const LOGIN: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const POLKIT: &str = "org.freedesktop.PolicyKit1";
const POLKIT_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY: &str = "org.freedesktop.PolicyKit1.Authority";
/// How often the inhibitors are read again while a confirmation shows.
const WATCH_EVERY: Duration = Duration::from_secs(1);
/// polkit's `AllowUserInteraction`.
const ALLOW_INTERACTION: u32 = 1;

/// What the loop asks of logind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// What it allows, read again.
    Can,
    /// Its `block` inhibitors of `shutdown`, now and every second, until
    /// asked to stop (`false`).
    Watch(bool),
    /// The password for `action`, asked through polkit before the
    /// countdown.
    Authorize(LeaveAction),
    /// The machine sleeps, restarts or powers off.
    Act(Act),
}

/// What the machine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Suspend,
    Restart,
    ShutDown,
}

impl Act {
    const fn method(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::Restart => "Reboot",
            Self::ShutDown => "PowerOff",
        }
    }
}

/// An inhibitor holding the session's end back, as logind lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inhibitor {
    pub who: String,
    pub why: String,
    pub pid: u32,
}

/// What logind said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    Can(LeaveCan),
    Inhibitors(Vec<Inhibitor>),
    /// Whether polkit authorized `action` (the user typed the password).
    Authorized(LeaveAction, bool),
    /// The call made, or why it was not.
    Acted(Act, Result<(), String>),
}

/// The thread's end the loop keeps.
#[derive(Debug)]
pub struct Logind {
    asks: UnboundedSender<Ask>,
}

impl Logind {
    /// On `bus` (an address, or `system`), what it says going to `loop_`.
    pub fn start(bus: String, loop_: Sender<Said>) -> Option<Self> {
        let (asks, asked) = unbounded_channel();
        let started = std::thread::Builder::new()
            .name("session menu: logind".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(run(bus, asked, loop_)),
                    Err(err) => tracing::warn!(%err, "no runtime for logind"),
                }
            });
        match started {
            Ok(_) => Some(Self { asks }),
            Err(err) => {
                tracing::warn!(%err, "cannot reach logind: no power actions");
                None
            }
        }
    }

    pub fn ask(&self, ask: Ask) {
        if self.asks.send(ask).is_err() {
            tracing::warn!(?ask, "logind's thread is gone");
        }
    }
}

async fn run(bus: String, mut asked: UnboundedReceiver<Ask>, loop_: Sender<Said>) {
    let builder = if bus == "system" {
        Builder::system()
    } else {
        Builder::address(bus.as_str())
    };
    let connection = match builder.build().await {
        Ok(connection) => Some(connection),
        Err(err) => {
            tracing::warn!(%err, "cannot reach the system bus: no power actions");
            None
        }
    };
    let mut watching = false;
    loop {
        let next = if watching {
            match tokio::time::timeout(WATCH_EVERY, asked.recv()).await {
                Ok(next) => next.map(Some),
                // A second gone by: the inhibitors again.
                Err(_) => Some(None),
            }
        } else {
            asked.recv().await.map(Some)
        };
        let Some(ask) = next else {
            // The loop is gone.
            return;
        };
        let Some(connection) = &connection else {
            // Nothing to ask: logind allows nothing, and the calls fail.
            match ask {
                Some(Ask::Can) => send(&loop_, Said::Can(LeaveCan::default())),
                Some(Ask::Authorize(action)) => send(&loop_, Said::Authorized(action, false)),
                Some(Ask::Act(act)) => send(&loop_, Said::Acted(act, Err("no system bus".into()))),
                Some(Ask::Watch(_)) | None => {}
            }
            continue;
        };
        match ask {
            Some(Ask::Can) => send(&loop_, Said::Can(can(connection).await)),
            Some(Ask::Watch(on)) => {
                watching = on;
                if on {
                    send(&loop_, Said::Inhibitors(inhibitors(connection).await));
                }
            }
            None => send(&loop_, Said::Inhibitors(inhibitors(connection).await)),
            Some(Ask::Authorize(action)) => {
                let granted = authorize(connection, action).await;
                send(&loop_, Said::Authorized(action, granted));
            }
            Some(Ask::Act(act)) => {
                tracing::info!(method = act.method(), "logind is asked");
                let called = connection
                    .call_method(
                        Some(LOGIN),
                        LOGIN_PATH,
                        Some(MANAGER),
                        act.method(),
                        &(true,),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                send(&loop_, Said::Acted(act, called));
            }
        }
    }
}

fn send(loop_: &Sender<Said>, said: Said) {
    // The loop gone, the compositor ends: nothing to tell.
    let _ = loop_.send(said);
}

/// `CanSuspend`, `CanReboot` and `CanPowerOff`; what does not answer, no.
async fn can(connection: &Connection) -> LeaveCan {
    let ask = |method: &'static str| async move {
        let reply = connection
            .call_method(Some(LOGIN), LOGIN_PATH, Some(MANAGER), method, &())
            .await;
        match reply.and_then(|r| r.body().deserialize::<String>()) {
            Ok(answer) => Can::of_logind(&answer),
            Err(err) => {
                tracing::debug!(method, %err, "logind did not say");
                Can::No
            }
        }
    };
    let can = LeaveCan {
        suspend: ask("CanSuspend").await,
        restart: ask("CanReboot").await,
        shut_down: ask("CanPowerOff").await,
    };
    tracing::debug!(?can, "what logind allows");
    can
}

/// logind's inhibitors in `block` mode holding `shutdown`: their `Who`,
/// `Why` and process.
async fn inhibitors(connection: &Connection) -> Vec<Inhibitor> {
    let reply = connection
        .call_method(
            Some(LOGIN),
            LOGIN_PATH,
            Some(MANAGER),
            "ListInhibitors",
            &(),
        )
        .await;
    let listed = reply.and_then(|r| {
        r.body()
            .deserialize::<Vec<(String, String, String, String, u32, u32)>>()
    });
    match listed {
        Ok(listed) => holding(listed),
        Err(err) => {
            tracing::debug!(%err, "logind's inhibitors not read");
            Vec::new()
        }
    }
}

/// Of `listed` (`what`, `who`, `why`, `mode`, `uid`, `pid`), those that
/// hold the machine's shutdown back: `block`, `shutdown` among what they
/// hold. `delay` ones logind waits for itself.
fn holding(listed: Vec<(String, String, String, String, u32, u32)>) -> Vec<Inhibitor> {
    listed
        .into_iter()
        .filter(|(what, _, _, mode, _, _)| {
            mode == "block" && what.split(':').any(|w| w == "shutdown")
        })
        .map(|(_, who, why, _, _, pid)| Inhibitor { who, why, pid })
        .collect()
}

/// polkit asked for `action` as logind will check it, the user asked for
/// the password (7.5's dialog): whether it was given.
async fn authorize(connection: &Connection, action: LeaveAction) -> bool {
    let Some(name) = connection.unique_name().map(|n| n.to_string()) else {
        return false;
    };
    let others = other_sessions(connection).await;
    let id = action_id(action, others);
    tracing::info!(id, "polkit asks for the password first");
    let subject = (
        "system-bus-name",
        std::collections::HashMap::from([("name", zbus::Variant::from(name.as_str()))]),
    );
    let details: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let reply = connection
        .call_method(
            Some(POLKIT),
            POLKIT_PATH,
            Some(AUTHORITY),
            "CheckAuthorization",
            &(subject, id, details, ALLOW_INTERACTION, ""),
        )
        .await;
    let answer = reply.and_then(|r| {
        r.body()
            .deserialize::<(bool, bool, std::collections::HashMap<String, String>)>()
    });
    match answer {
        Ok((authorized, _, _)) => authorized,
        Err(err) => {
            tracing::warn!(%err, "polkit did not answer");
            false
        }
    }
}

/// The polkit action logind checks for `action`: the one for several
/// users' sessions when another user has one open.
const fn action_id(action: LeaveAction, others: bool) -> &'static str {
    match (action, others) {
        (LeaveAction::Restart, false) => "org.freedesktop.login1.reboot",
        (LeaveAction::Restart, true) => "org.freedesktop.login1.reboot-multiple-sessions",
        (LeaveAction::ShutDown | LeaveAction::LogOut, false) => "org.freedesktop.login1.power-off",
        (LeaveAction::ShutDown | LeaveAction::LogOut, true) => {
            "org.freedesktop.login1.power-off-multiple-sessions"
        }
    }
}

/// Whether another user has a session open (`ListSessions`).
async fn other_sessions(connection: &Connection) -> bool {
    let me = rustix::process::getuid().as_raw();
    let reply = connection
        .call_method(Some(LOGIN), LOGIN_PATH, Some(MANAGER), "ListSessions", &())
        .await;
    let sessions = reply.and_then(|r| {
        r.body()
            .deserialize::<Vec<(String, u32, String, String, zbus::OwnedObjectPath)>>()
    });
    sessions.is_ok_and(|s| s.iter().any(|(_, uid, _, _, _)| *uid != me))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_block_inhibitors_of_shutdown_hold_it() {
        let row = |what: &str, mode: &str, who: &str| {
            (
                what.to_owned(),
                who.to_owned(),
                format!("{who}'s reason"),
                mode.to_owned(),
                1000,
                42,
            )
        };
        let held = holding(vec![
            row("shutdown:sleep", "block", "Firefox"),
            row("sleep", "block", "Music"),
            row("shutdown", "delay", "Unattended Upgrades"),
            row("idle:shutdown", "block", "Backup"),
            row("handle-power-key", "block", "GNOME"),
        ]);
        let who: Vec<&str> = held.iter().map(|i| i.who.as_str()).collect();
        assert_eq!(who, ["Firefox", "Backup"]);
        assert_eq!(held[0].why, "Firefox's reason");
        assert_eq!(held[0].pid, 42);
    }

    #[test]
    fn polkit_is_asked_for_what_logind_checks() {
        assert_eq!(
            action_id(LeaveAction::Restart, false),
            "org.freedesktop.login1.reboot"
        );
        assert_eq!(
            action_id(LeaveAction::ShutDown, true),
            "org.freedesktop.login1.power-off-multiple-sessions"
        );
    }
}
