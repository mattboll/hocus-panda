//! Leaving the session and sleeping (#72), through logind: what the machine
//! can do, which applications hold shutting down back and why, and the
//! calls themselves. Each runs on a thread of its own and answers through
//! a callback; nothing waits on the caller's loop.

use zbus::Proxy;

use crate::session::{manager, system};

/// What logind is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Suspend,
    Hibernate,
    Reboot,
    PowerOff,
}

impl Power {
    fn method(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::Hibernate => "Hibernate",
            Self::Reboot => "Reboot",
            Self::PowerOff => "PowerOff",
        }
    }

    fn can(self) -> &'static str {
        match self {
            Self::Suspend => "CanSuspend",
            Self::Hibernate => "CanHibernate",
            Self::Reboot => "CanReboot",
            Self::PowerOff => "CanPowerOff",
        }
    }
}

/// What the machine can do; unknown ones are taken as possible, except
/// hibernating, which needs swap and is often not set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub suspend: bool,
    pub hibernate: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            suspend: true,
            hibernate: false,
        }
    }
}

/// An application holding something back, as logind lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inhibitor {
    /// `shutdown`, `sleep`, `idle`… joined by colons.
    pub what: String,
    pub who: String,
    pub why: String,
    /// `block` or `delay`.
    pub mode: String,
}

impl Inhibitor {
    /// Whether it blocks shutting down (and restarting).
    pub fn blocks_shutdown(&self) -> bool {
        self.mode == "block" && self.what.split(':').any(|w| w == "shutdown")
    }
}

/// Asks logind what the machine can do, on a thread.
pub fn capabilities(bus: Option<String>, done: impl FnOnce(Capabilities) + Send + 'static) {
    spawn("logind can", move || {
        let caps = zbus::block_on(async {
            let conn = system(&bus).await?;
            let manager = manager(&conn).await?;
            let mut caps = Capabilities::default();
            for (power, slot) in [
                (Power::Suspend, &mut caps.suspend),
                (Power::Hibernate, &mut caps.hibernate),
            ] {
                let answer: String = manager.call(power.can(), &()).await?;
                *slot = answer == "yes" || answer == "challenge";
            }
            zbus::Result::Ok(caps)
        });
        match caps {
            Ok(caps) => done(caps),
            Err(err) => {
                tracing::debug!(%err, "logind is not there");
                done(Capabilities::default());
            }
        }
    });
}

/// The applications holding something back, on a thread; none when logind
/// is not there.
pub fn inhibitors(bus: Option<String>, done: impl FnOnce(Vec<Inhibitor>) + Send + 'static) {
    spawn("logind inhibitors", move || {
        let listed = zbus::block_on(async {
            let conn = system(&bus).await?;
            let manager = manager(&conn).await?;
            let rows: Vec<(String, String, String, String, u32, u32)> =
                manager.call("ListInhibitors", &()).await?;
            zbus::Result::Ok(rows)
        });
        let rows = listed.unwrap_or_else(|err| {
            tracing::debug!(%err, "could not list inhibitors");
            Vec::new()
        });
        done(
            rows.into_iter()
                .map(|(what, who, why, mode, _, _)| Inhibitor {
                    what,
                    who,
                    why,
                    mode,
                })
                .collect(),
        );
    });
}

/// Asks logind to suspend, hibernate, restart or shut down, on a thread;
/// the outcome through `done`.
pub fn act(
    bus: Option<String>,
    power: Power,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) {
    spawn("logind power", move || {
        let result = zbus::block_on(async {
            let conn = system(&bus).await?;
            let manager: Proxy<'_> = manager(&conn).await?;
            // Not interactive: the user already chose, in the session menu.
            manager.call::<_, _, ()>(power.method(), &(false,)).await
        });
        done(result.map_err(|err| err.to_string()));
    });
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_block_inhibitors_of_shutdown_hold_it_back() {
        let i = |what: &str, mode: &str| Inhibitor {
            what: what.into(),
            who: "Editor".into(),
            why: "Unsaved document".into(),
            mode: mode.into(),
        };
        assert!(i("shutdown", "block").blocks_shutdown());
        assert!(i("sleep:shutdown", "block").blocks_shutdown());
        assert!(!i("shutdown", "delay").blocks_shutdown());
        assert!(!i("sleep", "block").blocks_shutdown());
    }
}
