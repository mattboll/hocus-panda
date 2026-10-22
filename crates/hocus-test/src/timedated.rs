//! A stand-in for systemd-timedated on a private bus: the system's time
//! zone, which `SetTimezone` changes (with a PropertiesChanged signal), or
//! refuses as polkit would, saying what it was asked. Never the machine's.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use zbus::{Connection, interface, object_server::SignalEmitter};

const SERVICE: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";

/// What it was asked, in order: `SetTimezone ZONE` (with ` interactive`
/// when polkit may ask).
pub type Log = Arc<Mutex<Vec<String>>>;

struct TimeDate {
    zone: String,
    refuse: Arc<Mutex<bool>>,
    log: Log,
}

#[interface(name = "org.freedesktop.timedate1")]
impl TimeDate {
    #[zbus(property)]
    fn timezone(&self) -> String {
        self.zone.clone()
    }

    async fn set_timezone(
        &mut self,
        zone: String,
        interactive: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if let Ok(mut log) = self.log.lock() {
            let how = if interactive { " interactive" } else { "" };
            log.push(format!("SetTimezone {zone}{how}"));
        }
        if self.refuse.lock().is_ok_and(|r| *r) {
            return Err(zbus::fdo::Error::AccessDenied("Permission denied".into()));
        }
        self.zone = zone;
        self.timezone_changed(&emitter).await?;
        Ok(())
    }
}

pub struct FakeTimedated {
    connection: Connection,
    refuse: Arc<Mutex<bool>>,
    pub log: Log,
}

impl std::fmt::Debug for FakeTimedated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTimedated")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl FakeTimedated {
    /// Takes timedated's name on the bus at `address`, in `zone`.
    pub fn start(address: &str, zone: &str) -> anyhow::Result<Self> {
        let log = Log::default();
        let refuse = Arc::new(Mutex::new(false));
        let connection = zbus::block_on(
            zbus::connection::Builder::address(address)
                .serve_at(
                    PATH,
                    TimeDate {
                        zone: zone.to_owned(),
                        refuse: refuse.clone(),
                        log: log.clone(),
                    },
                )
                .name(SERVICE)
                .build(),
        )
        .context("could not take timedated's name on the bus")?;
        Ok(Self {
            connection,
            refuse,
            log,
        })
    }

    /// What it was asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// From now on, setting the zone is refused (or allowed again).
    pub fn refuse(&self, refuse: bool) {
        if let Ok(mut r) = self.refuse.lock() {
            *r = refuse;
        }
    }

    /// The zone changes, as if set elsewhere.
    pub fn set_zone(&self, zone: &str) -> anyhow::Result<()> {
        zbus::block_on(async {
            let iface = self
                .connection
                .object_server()
                .interface::<_, TimeDate>(PATH)
                .await?;
            let mut me = iface.get_mut().await;
            me.zone = zone.to_owned();
            me.timezone_changed(iface.signal_emitter()).await
        })?;
        Ok(())
    }
}
