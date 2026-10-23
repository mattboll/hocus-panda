//! Stand-ins for the machine's services on a private system bus, as far as
//! quick settings and the settings' Hardware pages use them: UPower's
//! battery, NetworkManager (a Wi-Fi device, networks in reach, saved
//! connections), BlueZ (an adapter, paired devices and others found while
//! discovering, a pairing agent asked) and power-profiles-daemon. What
//! they are asked to change is changed (with a PropertiesChanged signal)
//! and written down. Never the machine's.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use zbus::{
    Connection, ObjectServer, OwnedObjectPath, OwnedVariant, interface,
    object_server::SignalEmitter,
};

const BATTERY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const NM: &str = "/org/freedesktop/NetworkManager";
const ACTIVE: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
const WIFI_DEVICE: &str = "/org/freedesktop/NetworkManager/Devices/1";
const NM_SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const ADAPTER: &str = "/org/bluez/hci0";
const PROFILES: &str = "/org/freedesktop/UPower/PowerProfiles";

/// What they were asked, in order: `WirelessEnabled false`, `Powered
/// true`, `ActiveProfile power-saver`, `AddAndActivateConnection Panda den
/// wpa-psk`, `Pair Panda keyboard`.
pub type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: String) {
    if let Ok(mut log) = log.lock() {
        log.push(line);
    }
}

fn object(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).unwrap_or_default()
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

/// The network the stand-in joins.
pub const NETWORK: &str = "Bamboo grove";
/// The password of the secured networks in reach.
pub const WIFI_PASSWORD: &str = "bamboo shoots";

/// How an access point says it is protected: `Flags`, `WpaFlags`,
/// `RsnFlags`.
#[derive(Clone, Copy)]
enum Air {
    Open,
    /// WPA2 Personal (PSK, CCMP).
    Psk,
    /// WPA3 Personal only.
    Sae,
    /// 802.1X.
    Enterprise,
}

impl Air {
    fn flags(self) -> (u32, u32, u32) {
        match self {
            Self::Open => (0, 0, 0),
            Self::Psk => (1, 0, 0x108),
            Self::Sae => (1, 0, 0x408),
            Self::Enterprise => (1, 0, 0x208),
        }
    }
}

/// The access points in reach: a name (one twice, one hidden), strength,
/// security.
const IN_REACH: [(&str, u8, Air); 7] = [
    (NETWORK, 82, Air::Psk),
    ("Panda den", 64, Air::Psk),
    ("", 70, Air::Psk),
    ("Office", 58, Air::Enterprise),
    ("Café du Parc", 47, Air::Open),
    (NETWORK, 35, Air::Psk),
    ("Tea house", 22, Air::Sae),
];

fn point_path(i: usize) -> String {
    format!("/org/freedesktop/NetworkManager/AccessPoint/{i}")
}

fn saved_path(id: u32) -> String {
    format!("/org/freedesktop/NetworkManager/Settings/{id}")
}

/// NetworkManager's state, which its objects share.
struct NmState {
    wifi: bool,
    /// The network joined.
    joined: Option<String>,
    /// The network tried last, for the active connection's name.
    tried: String,
    /// Connections kept: their number, their SSID.
    saved: Vec<(u32, String)>,
    next: u32,
    log: Log,
}

type Nm = Arc<Mutex<NmState>>;

fn nm_read<T>(nm: &Nm, f: impl FnOnce(&NmState) -> T) -> Option<T> {
    nm.lock().ok().map(|s| f(&s))
}

fn nm_write<T>(nm: &Nm, f: impl FnOnce(&mut NmState) -> T) -> Option<T> {
    nm.lock().ok().map(|mut s| f(&mut s))
}

/// The root object's change of `PrimaryConnection`, said from any object.
async fn joined_changed(server: &ObjectServer) -> zbus::Result<()> {
    let root = server.interface::<_, Network>(NM).await?;
    root.get()
        .await
        .primary_connection_changed(root.signal_emitter())
        .await
}

struct Network {
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl Network {
    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        nm_read(&self.nm, |s| s.wifi).unwrap_or(false)
    }

    #[zbus(property)]
    fn set_wireless_enabled(&mut self, on: bool) {
        nm_write(&self.nm, |s| {
            note(&s.log, format!("WirelessEnabled {on}"));
            s.wifi = on;
        });
    }

    /// Joined while the radio is on.
    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        let joined = nm_read(&self.nm, |s| s.wifi && s.joined.is_some()).unwrap_or(false);
        object(if joined { ACTIVE } else { "/" })
    }

    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        vec![object(WIFI_DEVICE)]
    }

    async fn activate_connection(
        &self,
        connection: OwnedObjectPath,
        _device: OwnedObjectPath,
        _specific: OwnedObjectPath,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let ssid = nm_write(&self.nm, |s| {
            let ssid = s
                .saved
                .iter()
                .find(|(id, _)| saved_path(*id) == connection.as_str())
                .map(|(_, ssid)| ssid.clone())?;
            note(&s.log, format!("ActivateConnection {ssid}"));
            s.tried = ssid.clone();
            s.joined = Some(ssid.clone());
            Some(ssid)
        })
        .flatten();
        if ssid.is_none() {
            return Err(zbus::fdo::Error::UnknownObject("no such connection".into()));
        }
        joined_changed(server).await?;
        Ok(object(ACTIVE))
    }

    /// Joins the access point asked with these settings: a secured one
    /// with the right password, an open one always.
    async fn add_and_activate_connection(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
        _device: OwnedObjectPath,
        specific: OwnedObjectPath,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        let Some((ssid, _, air)) = (0..IN_REACH.len())
            .find(|i| point_path(*i) == specific.as_str())
            .map(|i| IN_REACH[i])
        else {
            return Err(zbus::fdo::Error::InvalidArgs("no such access point".into()));
        };
        let security = settings.get("802-11-wireless-security");
        let text = |key: &str| {
            security
                .and_then(|s| s.get(key))
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        };
        let key_mgmt = text("key-mgmt").unwrap_or_else(|| "open".into());
        let right = match air {
            Air::Open => true,
            Air::Psk | Air::Sae => text("psk").as_deref() == Some(WIFI_PASSWORD),
            Air::Enterprise => false,
        };
        let id = nm_write(&self.nm, |s| {
            // The password is not written down: only how it is joined.
            note(
                &s.log,
                format!("AddAndActivateConnection {ssid} {key_mgmt}"),
            );
            let id = s.next;
            s.next += 1;
            s.saved.push((id, ssid.to_owned()));
            s.tried = ssid.to_owned();
            s.joined = right.then(|| ssid.to_owned());
            id
        })
        .unwrap_or_default();
        server
            .at(
                saved_path(id),
                Saved {
                    id,
                    nm: self.nm.clone(),
                },
            )
            .await?;
        joined_changed(server).await?;
        Ok((object(&saved_path(id)), object(ACTIVE)))
    }
}

struct Active {
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn id(&self) -> String {
        nm_read(&self.nm, |s| {
            s.joined.clone().unwrap_or_else(|| s.tried.clone())
        })
        .unwrap_or_default()
    }

    /// 2 activated, 4 deactivated.
    #[zbus(property)]
    fn state(&self) -> u32 {
        if nm_read(&self.nm, |s| s.joined.is_some()).unwrap_or(false) {
            2
        } else {
            4
        }
    }
}

struct WifiDevice {
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl WifiDevice {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }

    /// 100 activated, 30 disconnected.
    #[zbus(property)]
    fn state(&self) -> u32 {
        if nm_read(&self.nm, |s| s.wifi && s.joined.is_some()).unwrap_or(false) {
            100
        } else {
            30
        }
    }

    async fn disconnect(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<()> {
        nm_write(&self.nm, |s| {
            note(&s.log, "Disconnect".into());
            s.joined = None;
        });
        joined_changed(server).await?;
        Ok(())
    }
}

struct Wireless {
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn access_points(&self) -> Vec<OwnedObjectPath> {
        if !nm_read(&self.nm, |s| s.wifi).unwrap_or(false) {
            return Vec::new();
        }
        (0..IN_REACH.len())
            .map(|i| object(&point_path(i)))
            .collect()
    }

    /// The strongest access point of the network joined.
    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        let joined = nm_read(&self.nm, |s| s.joined.clone().filter(|_| s.wifi)).flatten();
        let at = joined.and_then(|ssid| IN_REACH.iter().position(|(s, _, _)| *s == ssid));
        at.map_or_else(|| object("/"), |i| object(&point_path(i)))
    }

    fn request_scan(&self, _options: HashMap<String, OwnedVariant>) {
        nm_read(&self.nm, |s| note(&s.log, "RequestScan".into()));
    }
}

struct AccessPoint {
    index: usize,
}

#[interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl AccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> Vec<u8> {
        IN_REACH[self.index].0.as_bytes().to_vec()
    }

    #[zbus(property)]
    fn strength(&self) -> u8 {
        IN_REACH[self.index].1
    }

    #[zbus(property)]
    fn flags(&self) -> u32 {
        IN_REACH[self.index].2.flags().0
    }

    #[zbus(property)]
    fn wpa_flags(&self) -> u32 {
        IN_REACH[self.index].2.flags().1
    }

    #[zbus(property)]
    fn rsn_flags(&self) -> u32 {
        IN_REACH[self.index].2.flags().2
    }
}

struct Settings {
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Settings {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        nm_read(&self.nm, |s| {
            s.saved
                .iter()
                .map(|(id, _)| object(&saved_path(*id)))
                .collect()
        })
        .unwrap_or_default()
    }
}

/// A connection kept; once deleted, only no longer listed.
struct Saved {
    id: u32,
    nm: Nm,
}

#[interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl Saved {
    fn get_settings(&self) -> zbus::fdo::Result<HashMap<String, HashMap<String, OwnedVariant>>> {
        let ssid = nm_read(&self.nm, |s| {
            s.saved
                .iter()
                .find(|(id, _)| *id == self.id)
                .map(|(_, ssid)| ssid.clone())
        })
        .flatten()
        .ok_or_else(|| zbus::fdo::Error::UnknownObject("deleted".into()))?;
        let value = |v: zbus::Variant<'_>| OwnedVariant::try_from(v).ok();
        let mut connection = HashMap::new();
        connection.extend(value(ssid.as_str().into()).map(|v| ("id".to_owned(), v)));
        connection.extend(value("802-11-wireless".into()).map(|v| ("type".to_owned(), v)));
        let mut wireless = HashMap::new();
        wireless.extend(value(ssid.as_bytes().into()).map(|v| ("ssid".to_owned(), v)));
        Ok(HashMap::from([
            ("connection".to_owned(), connection),
            ("802-11-wireless".to_owned(), wireless),
        ]))
    }

    fn delete(&self) {
        nm_write(&self.nm, |s| {
            if let Some(at) = s.saved.iter().position(|(id, _)| *id == self.id) {
                let (_, ssid) = s.saved.remove(at);
                note(&s.log, format!("Delete {ssid}"));
            }
        });
    }
}

/// The pairing agent registered last: its connection and object.
type AgentAt = Arc<Mutex<Option<(String, OwnedObjectPath)>>>;

struct Adapter {
    powered: bool,
    discovering: bool,
    agent: AgentAt,
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

    #[zbus(property)]
    fn discovering(&self) -> bool {
        self.discovering
    }

    /// The devices in reach show up.
    async fn start_discovery(
        &mut self,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<()> {
        note(&self.log, "StartDiscovery".into());
        self.discovering = true;
        for spec in &FOUND {
            add_device(server, spec, &self.agent, &self.log).await?;
        }
        // One without a name, as many are.
        let unnamed = format!("{ADAPTER}/dev_5C_12_00_AA_BB_CC");
        server
            .at(
                unnamed.as_str(),
                Unnamed {
                    address: "5C:12:00:AA:BB:CC".into(),
                },
            )
            .await?;
        Ok(())
    }

    fn stop_discovery(&mut self) {
        note(&self.log, "StopDiscovery".into());
        self.discovering = false;
    }

    async fn remove_device(
        &self,
        device: OwnedObjectPath,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<()> {
        let name = match server.interface::<_, Device>(device.as_str()).await {
            Ok(d) => d.get().await.spec.name,
            Err(_) => return Err(zbus::fdo::Error::UnknownObject("no such device".into())),
        };
        note(&self.log, format!("RemoveDevice {name}"));
        server.remove::<Battery1, _>(device.as_str()).await.ok();
        server.remove::<Device, _>(device.as_str()).await?;
        Ok(())
    }
}

struct AgentManager {
    agent: AgentAt,
    log: Log,
}

#[interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        note(&self.log, format!("RegisterAgent {capability}"));
        if let (Ok(mut slot), Some(sender)) = (self.agent.lock(), header.sender()) {
            *slot = Some((sender.to_string(), agent));
        }
    }

    fn unregister_agent(&self, _agent: OwnedObjectPath) {
        note(&self.log, "UnregisterAgent".into());
        if let Ok(mut slot) = self.agent.lock() {
            *slot = None;
        }
    }
}

/// A device BlueZ knows, as it starts.
struct DeviceSpec {
    name: &'static str,
    address: &'static str,
    icon: &'static str,
    paired: bool,
    connected: bool,
    battery: Option<u8>,
    /// Pairing shows this passkey, to confirm; `None`: "just works".
    passkey: Option<u32>,
}

/// The passkey the keyboard shows when paired.
pub const PASSKEY: u32 = 123_456;

/// Paired before the scenario starts.
const PAIRED: [DeviceSpec; 2] = [
    DeviceSpec {
        name: "Bamboo headphones",
        address: "A4:C1:38:10:20:30",
        icon: "audio-headset",
        paired: true,
        connected: true,
        battery: Some(80),
        passkey: None,
    },
    DeviceSpec {
        name: "Panda mouse",
        address: "D0:2B:20:40:50:60",
        icon: "input-mouse",
        paired: true,
        connected: false,
        battery: Some(35),
        passkey: None,
    },
];

/// Found while discovering.
const FOUND: [DeviceSpec; 2] = [
    DeviceSpec {
        name: "Panda keyboard",
        address: "E8:9F:6D:70:80:90",
        icon: "input-keyboard",
        paired: false,
        connected: false,
        battery: None,
        passkey: Some(PASSKEY),
    },
    DeviceSpec {
        name: "Speaker by the window",
        address: "F4:4E:FD:01:02:03",
        icon: "audio-card",
        paired: false,
        connected: false,
        battery: None,
        passkey: None,
    },
];

fn device_path(spec: &DeviceSpec) -> String {
    format!("{ADAPTER}/dev_{}", spec.address.replace(':', "_"))
}

async fn add_device(
    server: &ObjectServer,
    spec: &'static DeviceSpec,
    agent: &AgentAt,
    log: &Log,
) -> zbus::Result<()> {
    let path = device_path(spec);
    // Found again: as it was.
    if server.interface::<_, Device>(path.as_str()).await.is_ok() {
        return Ok(());
    }
    if let Some(percent) = spec.battery {
        server.at(path.as_str(), Battery1 { percent }).await?;
    }
    let device = Device {
        spec,
        path: path.clone(),
        state: Mutex::new(DeviceState {
            paired: spec.paired,
            connected: spec.connected,
            trusted: spec.paired,
        }),
        agent: agent.clone(),
        log: log.clone(),
    };
    server.at(path.as_str(), device).await?;
    Ok(())
}

struct DeviceState {
    paired: bool,
    connected: bool,
    trusted: bool,
}

struct Device {
    spec: &'static DeviceSpec,
    path: String,
    state: Mutex<DeviceState>,
    agent: AgentAt,
    log: Log,
}

impl Device {
    fn read<T>(&self, f: impl FnOnce(&DeviceState) -> T) -> Option<T> {
        self.state.lock().ok().map(|s| f(&s))
    }

    fn write(&self, f: impl FnOnce(&mut DeviceState)) {
        if let Ok(mut s) = self.state.lock() {
            f(&mut s);
        }
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum BluezError {
    #[zbus(error)]
    ZBus(zbus::Error),
    AuthenticationRejected(String),
    AlreadyExists(String),
}

#[interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn address(&self) -> String {
        self.spec.address.into()
    }

    #[zbus(property)]
    fn name(&self) -> String {
        self.spec.name.into()
    }

    #[zbus(property)]
    fn alias(&self) -> String {
        self.spec.name.into()
    }

    #[zbus(property)]
    fn icon(&self) -> String {
        self.spec.icon.into()
    }

    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        object(ADAPTER)
    }

    #[zbus(property)]
    fn paired(&self) -> bool {
        self.read(|s| s.paired).unwrap_or(false)
    }

    #[zbus(property)]
    fn connected(&self) -> bool {
        self.read(|s| s.connected).unwrap_or(false)
    }

    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.read(|s| s.trusted).unwrap_or(false)
    }

    #[zbus(property)]
    fn set_trusted(&self, on: bool) {
        note(&self.log, format!("Trusted {} {on}", self.spec.name));
        self.write(|s| s.trusted = on);
    }

    /// With a passkey, the agent registered is asked to confirm it.
    async fn pair(
        &self,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), BluezError> {
        note(&self.log, format!("Pair {}", self.spec.name));
        if self.read(|s| s.paired).unwrap_or(false) {
            return Err(BluezError::AlreadyExists("Already paired".into()));
        }
        if let Some(passkey) = self.spec.passkey {
            let agent = self.agent.lock().ok().and_then(|a| a.clone());
            let Some((sender, at)) = agent else {
                return Err(BluezError::AuthenticationRejected("no agent".into()));
            };
            let asked = conn
                .call_method(
                    Some(sender.as_str()),
                    at.as_str(),
                    Some("org.bluez.Agent1"),
                    "RequestConfirmation",
                    &(object(&self.path), passkey),
                )
                .await;
            if asked.is_err() {
                note(&self.log, format!("Pair {} rejected", self.spec.name));
                return Err(BluezError::AuthenticationRejected(
                    "Authentication Rejected".into(),
                ));
            }
        }
        self.write(|s| s.paired = true);
        self.paired_changed(&emitter).await?;
        Ok(())
    }

    async fn connect(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        note(&self.log, format!("Connect {}", self.spec.name));
        self.write(|s| s.connected = true);
        self.connected_changed(&emitter).await?;
        Ok(())
    }

    async fn disconnect(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        note(&self.log, format!("Disconnect {}", self.spec.name));
        self.write(|s| s.connected = false);
        self.connected_changed(&emitter).await?;
        Ok(())
    }
}

/// A device in reach that says no name: BlueZ names it by its address.
struct Unnamed {
    address: String,
}

#[interface(name = "org.bluez.Device1")]
impl Unnamed {
    #[zbus(property)]
    fn address(&self) -> String {
        self.address.clone()
    }

    #[zbus(property)]
    fn alias(&self) -> String {
        self.address.replace(':', "-")
    }

    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        object(ADAPTER)
    }

    #[zbus(property)]
    fn paired(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn connected(&self) -> bool {
        false
    }
}

struct Battery1 {
    percent: u8,
}

#[interface(name = "org.bluez.Battery1")]
impl Battery1 {
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        self.percent
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
    fn profiles(&self) -> Vec<HashMap<String, OwnedVariant>> {
        Vec::new()
    }
}

/// The machine's services, as a laptop on its charger at 80 %, Wi-Fi on
/// and joined to a saved network with others in reach, Bluetooth on with
/// headphones connected and a mouse paired, the balanced profile.
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

        let nm: Nm = Arc::new(Mutex::new(NmState {
            wifi: true,
            joined: Some(NETWORK.into()),
            tried: NETWORK.into(),
            saved: vec![(1, NETWORK.into())],
            next: 2,
            log: log.clone(),
        }));
        let mut builder = bus()
            .serve_at(NM, Network { nm: nm.clone() })
            .serve_at(ACTIVE, Active { nm: nm.clone() })
            .serve_at(WIFI_DEVICE, WifiDevice { nm: nm.clone() })
            .serve_at(WIFI_DEVICE, Wireless { nm: nm.clone() })
            .serve_at(NM_SETTINGS, Settings { nm: nm.clone() })
            .serve_at(
                saved_path(1),
                Saved {
                    id: 1,
                    nm: nm.clone(),
                },
            );
        for i in 0..IN_REACH.len() {
            builder = builder.serve_at(point_path(i), AccessPoint { index: i });
        }
        let network = build(builder, "org.freedesktop.NetworkManager")?;

        let agent = AgentAt::default();
        let adapter = Adapter {
            powered: true,
            discovering: false,
            agent: agent.clone(),
            log: log.clone(),
        };
        let manager = AgentManager {
            agent: agent.clone(),
            log: log.clone(),
        };
        let bluez = build(
            bus()
                .serve_at("/", zbus::fdo::ObjectManager)
                .serve_at(ADAPTER, adapter)
                .serve_at("/org/bluez", manager),
            "org.bluez",
        )?;
        zbus::block_on(async {
            for spec in &PAIRED {
                add_device(bluez.object_server(), spec, &agent, &log).await?;
            }
            zbus::Result::Ok(())
        })?;

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
