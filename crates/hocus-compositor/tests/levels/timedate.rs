//! systemd's timedated, faked on a test's private bus, never the
//! machine's: its zone, its automatic time and whether it is right,
//! whether a time service is there; what it was asked is recorded, and it
//! refuses everything while told to (polkit saying no). It says its
//! changes as timedated does (`PropertiesChanged`), all but
//! `NTPSynchronized`, which timedated never says.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use zbus::{Connection, Variant, connection::Builder};

pub const NAME: &str = "org.freedesktop.timedate1";
pub const PATH: &str = "/org/freedesktop/timedate1";

/// What it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Ntp(bool),
    Zone(String),
    /// The time, in microseconds since the epoch.
    Time(i64),
}

/// What it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub zone: String,
    pub ntp: bool,
    pub synced: bool,
    pub can_ntp: bool,
    /// Every call refused, as polkit refuses it.
    pub refuse: bool,
    pub calls: Vec<Call>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            zone: "Europe/Paris".to_owned(),
            ntp: true,
            synced: true,
            can_ntp: true,
            refuse: false,
            calls: Vec::new(),
        }
    }
}

struct Inner {
    state: Mutex<State>,
    connection: OnceLock<Connection>,
}

type Shared = Arc<Inner>;

fn with<T>(shared: &Shared, f: impl FnOnce(&mut State) -> T) -> T {
    f(&mut shared.state.lock().unwrap_or_else(PoisonError::into_inner))
}

/// `PropertiesChanged` for `names`, their values left to be read again.
async fn changed(shared: &Shared, names: &[&str]) {
    let Some(connection) = shared.connection.get() else {
        return;
    };
    let _ = connection
        .emit_signal(
            None::<&str>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(NAME, HashMap::<&str, Variant<'_>>::new(), names.to_vec()),
        )
        .await;
}

fn refused(shared: &Shared, call: Call) -> zbus::fdo::Result<()> {
    with(shared, |s| {
        s.calls.push(call);
        if s.refuse {
            Err(zbus::fdo::Error::AccessDenied(
                "Interactive authentication required.".to_owned(),
            ))
        } else {
            Ok(())
        }
    })
}

struct Service(Shared);

#[zbus::interface(name = "org.freedesktop.timedate1")]
impl Service {
    #[zbus(property)]
    fn timezone(&self) -> String {
        with(&self.0, |s| s.zone.clone())
    }

    #[zbus(property, name = "NTP")]
    fn ntp(&self) -> bool {
        with(&self.0, |s| s.ntp)
    }

    #[zbus(property, name = "NTPSynchronized")]
    fn ntp_synchronized(&self) -> bool {
        with(&self.0, |s| s.synced)
    }

    #[zbus(property, name = "CanNTP")]
    fn can_ntp(&self) -> bool {
        with(&self.0, |s| s.can_ntp)
    }

    #[zbus(property, name = "LocalRTC")]
    fn local_rtc(&self) -> bool {
        false
    }

    #[zbus(name = "SetTimezone")]
    async fn set_timezone(&self, zone: String, _interactive: bool) -> zbus::fdo::Result<()> {
        refused(&self.0, Call::Zone(zone.clone()))?;
        with(&self.0, |s| s.zone = zone);
        changed(&self.0, &["Timezone"]).await;
        Ok(())
    }

    #[zbus(name = "SetNTP")]
    async fn set_ntp(&self, on: bool, _interactive: bool) -> zbus::fdo::Result<()> {
        refused(&self.0, Call::Ntp(on))?;
        with(&self.0, |s| s.ntp = on);
        changed(&self.0, &["NTP"]).await;
        Ok(())
    }

    #[zbus(name = "SetTime")]
    async fn set_time(
        &self,
        usec: i64,
        _relative: bool,
        _interactive: bool,
    ) -> zbus::fdo::Result<()> {
        refused(&self.0, Call::Time(usec))?;
        Ok(())
    }
}

/// The fake, served on its own connection.
pub struct TimeDate {
    shared: Shared,
}

impl TimeDate {
    /// timedated on the bus at `address`, holding `state`.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        state: State,
    ) -> anyhow::Result<Self> {
        let shared: Shared = Arc::new(Inner {
            state: Mutex::new(state),
            connection: OnceLock::new(),
        });
        let address = address.to_owned();
        let served = shared.clone();
        runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(NAME)
                .serve_at(PATH, Service(served.clone()))
                .build()
                .await?;
            let _ = served.connection.set(connection);
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(Self { shared })
    }

    pub fn now(&self) -> State {
        with(&self.shared, |s| s.clone())
    }

    pub fn calls(&self) -> Vec<Call> {
        with(&self.shared, |s| s.calls.clone())
    }

    /// Every call refused from now on, or not.
    pub fn refuse(&self, refuse: bool) {
        with(&self.shared, |s| s.refuse = refuse);
    }

    /// The zone changed by someone else (`timedatectl`), said.
    pub fn set_zone(&self, runtime: &tokio::runtime::Runtime, zone: &str) {
        with(&self.shared, |s| s.zone = zone.to_owned());
        let shared = self.shared.clone();
        runtime.block_on(async move { changed(&shared, &["Timezone"]).await });
    }
}
