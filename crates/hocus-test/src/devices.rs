//! Stand-ins for the machine's services on a private system bus, as far as
//! quick settings and the settings' Hardware pages use them: UPower's
//! battery, NetworkManager, BlueZ's adapter and power-profiles-daemon.
//! What they are asked to change is changed (with a PropertiesChanged
//! signal) and written down. Never the machine's.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use zbus::{Connection, OwnedObjectPath, interface, object_server::SignalEmitter};

const BATTERY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const NM: &str = "/org/freedesktop/NetworkManager";
const ACTIVE: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
const ADAPTER: &str = "/org/bluez/hci0";
const PROFILES: &str = "/org/freedesktop/UPower/PowerProfiles";

/// What they were asked, in order: `WirelessEnabled false`, `Powered
/// true`, `ActiveProfile power-saver`.
pub type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: String) {
    if let Ok(mut log) = log.lock() {
        log.push(line);
    }
}

struct Battery {
    percent: f64,
    /// On the charger, charging or full; else discharging.
    charger: bool,
}

#[interface(name = "org.freedesktop.UPower.Device")]
impl Battery {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.percent
    }

    /// 1 charging, 2 discharging, 4 fully charged.
    #[zbus(property)]
    fn state(&self) -> u32 {
        match (self.charger, self.percent >= 100.0) {
            (true, true) => 4,
            (true, false) => 1,
            (false, _) => 2,
        }
    }

    /// Three hours a full battery, in seconds.
    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        if self.charger {
            0
        } else {
            (self.percent * 108.0) as i64
        }
    }
}

struct Network {
    wifi: bool,
    log: Log,
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl Network {
    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        self.wifi
    }

    #[zbus(property)]
    fn set_wireless_enabled(&mut self, on: bool) {
        note(&self.log, format!("WirelessEnabled {on}"));
        self.wifi = on;
    }

    /// Joined while the radio is on.
    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        let path = if self.wifi { ACTIVE } else { "/" };
        OwnedObjectPath::try_from(path).unwrap_or_default()
    }
}

struct Active {
    name: String,
}

#[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn id(&self) -> String {
        self.name.clone()
    }
}

struct Adapter {
    powered: bool,
    log: Log,
}

#[interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.powered
    }

    #[zbus(property)]
    fn set_powered(&mut self, on: bool) {
        note(&self.log, format!("Powered {on}"));
        self.powered = on;
    }
}

struct Profiles {
    active: String,
    log: Log,
}

#[interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.active.clone()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) {
        note(&self.log, format!("ActiveProfile {profile}"));
        self.active = profile;
    }

    /// None listed: every profile is offered.
    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, zbus::OwnedVariant>> {
        Vec::new()
    }
}

/// The machine's services, as a laptop on its charger at 80 %, Wi-Fi on
/// and joined, Bluetooth on, the balanced profile.
pub struct FakeDevices {
    upower: Connection,
    _network: Connection,
    _bluez: Connection,
    _profiles: Connection,
    pub log: Log,
}

impl std::fmt::Debug for FakeDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeDevices")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

/// The network the stand-in joins.
pub const NETWORK: &str = "Bamboo grove";

impl FakeDevices {
    /// Takes the services' names on the bus at `address`.
    pub fn start(address: &str) -> anyhow::Result<Self> {
        let log = Log::default();
        let bus = || zbus::connection::Builder::address(address);
        let build = |builder: zbus::connection::Builder<'static>, name: &'static str| {
            zbus::block_on(builder.name(name).build())
                .with_context(|| format!("could not take {name} on the bus"))
        };
        let battery = Battery {
            percent: 80.0,
            charger: true,
        };
        let upower = build(bus().serve_at(BATTERY, battery), "org.freedesktop.UPower")?;
        let wifi = Network {
            wifi: true,
            log: log.clone(),
        };
        let joined = Active {
            name: NETWORK.into(),
        };
        let network = build(
            bus().serve_at(NM, wifi).serve_at(ACTIVE, joined),
            "org.freedesktop.NetworkManager",
        )?;
        let adapter = Adapter {
            powered: true,
            log: log.clone(),
        };
        let bluez = build(bus().serve_at(ADAPTER, adapter), "org.bluez")?;
        let balanced = Profiles {
            active: "balanced".into(),
            log: log.clone(),
        };
        let profiles = build(
            bus().serve_at(PROFILES, balanced),
            "org.freedesktop.UPower.PowerProfiles",
        )?;
        Ok(Self {
            upower,
            _network: network,
            _bluez: bluez,
            _profiles: profiles,
            log,
        })
    }

    /// What they were asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// The charger is plugged in, or pulled out: UPower says so.
    pub fn set_charger(&self, plugged: bool) -> anyhow::Result<()> {
        zbus::block_on(async {
            let iface = self
                .upower
                .object_server()
                .interface::<_, Battery>(BATTERY)
                .await?;
            iface.get_mut().await.charger = plugged;
            let emitter: &SignalEmitter<'_> = iface.signal_emitter();
            let battery = iface.get().await;
            battery.state_changed(emitter).await?;
            battery.time_to_empty_changed(emitter).await
        })?;
        Ok(())
    }
}
