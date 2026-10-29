//! BlueZ, faked on the fakes' private bus (`fakes.rs`), never the
//! machine's: an adapter (on, off, or blocked by rfkill), its devices
//! (paired or not, connected or not, with a battery or none, named or not,
//! heard at some strength), the agent manager, and the object manager at
//! `/` that says what comes and goes.
//!
//! It behaves as BlueZ does where the shell looks: a search started brings
//! the devices asked to appear then; a device connects (`Connected`, its
//! `Battery1` appearing), refuses, or does not answer until it is given up
//! (`Disconnect` cancels it); a pairing succeeds without a code (Just
//! Works), fails, or asks the agent registered for a passkey and fails
//! when the agent refuses. Every change is signalled with its values
//! (`PropertiesChanged`, `InterfacesAdded` and `…Removed`), and what the
//! shell asked is recorded, for the tests to compare.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};

use tokio::sync::Notify;
use zbus::{Connection, OwnedObjectPath, Variant, connection::Builder, message::Header};

pub const BLUEZ: &str = "org.bluez";
const ROOT: &str = "/org/bluez";
pub const ADAPTER_PATH: &str = "/org/bluez/hci0";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// How long a connection, a refusal or a pairing takes.
const TAKES: Duration = Duration::from_millis(300);

fn takes(shared: &Shared) -> Duration {
    state(shared, |s| s.takes)
}

/// What a device does when asked to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Connects,
    /// BlueZ's error after a while (off, out of range).
    Refuses,
    /// Nothing, until it is given up.
    Hangs,
}

/// What a device does when asked to pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    /// Without a code.
    JustWorks,
    /// It asks the agent for a passkey to type.
    Passkey,
    /// Not in pairing mode.
    Fails,
}

#[derive(Debug, Clone, Copy)]
pub struct FakeDevice {
    /// None: it never said its name (BlueZ's alias is its address).
    pub name: Option<&'static str>,
    /// BlueZ's icon name, or none.
    pub icon: &'static str,
    pub paired: bool,
    pub connected: bool,
    /// Its battery while connected.
    pub battery: Option<u8>,
    pub rssi: Option<i16>,
    pub answer: Answer,
    pub pairing: Pairing,
}

impl FakeDevice {
    /// A paired device that connects, not connected.
    pub const fn paired(name: &'static str, icon: &'static str, battery: Option<u8>) -> Self {
        Self {
            name: Some(name),
            icon,
            paired: true,
            connected: false,
            battery,
            rssi: None,
            answer: Answer::Connects,
            pairing: Pairing::JustWorks,
        }
    }

    /// A device around, not paired, heard at `rssi`.
    pub const fn around(name: &'static str, icon: &'static str, rssi: i16) -> Self {
        Self {
            name: Some(name),
            icon,
            paired: false,
            connected: false,
            battery: None,
            rssi: Some(rssi),
            answer: Answer::Connects,
            pairing: Pairing::JustWorks,
        }
    }

    pub const fn connected(self) -> Self {
        Self {
            connected: true,
            ..self
        }
    }

    pub const fn answering(self, answer: Answer) -> Self {
        Self { answer, ..self }
    }

    pub const fn pairing(self, pairing: Pairing) -> Self {
        Self { pairing, ..self }
    }
}

/// What BlueZ says as it starts.
#[derive(Debug, Clone)]
pub struct Setup {
    pub adapter: bool,
    pub powered: bool,
    /// Blocked by rfkill: off, and cannot be turned on.
    pub blocked: bool,
    pub devices: Vec<FakeDevice>,
    /// Devices found once a search starts.
    pub later: Vec<FakeDevice>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            adapter: true,
            powered: true,
            blocked: false,
            devices: Vec::new(),
            later: Vec::new(),
        }
    }
}

/// What the shell asked, by device name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    RegisterAgent(String),
    StartDiscovery,
    StopDiscovery,
    Power(bool),
    Connect(String),
    Disconnect(String),
    Pair(String),
    Trust(String),
    CancelPairing(String),
}

#[derive(Debug)]
struct Dev {
    path: String,
    fake: FakeDevice,
    paired: bool,
    trusted: bool,
    connected: bool,
    /// Its battery's interface is served.
    battery: bool,
    /// Wakes a connection or a pairing under way, to give it up.
    cancel: Arc<Notify>,
}

impl Dev {
    fn name(&self) -> String {
        self.fake.name.map_or_else(|| self.address(), str::to_owned)
    }

    fn address(&self) -> String {
        let id = self.path.rsplit('_').next().unwrap_or("00");
        format!("5C-12-AA-01-02-{id}")
    }
}

#[derive(Debug, Default)]
struct State {
    powered: bool,
    blocked: bool,
    discovering: bool,
    devices: Vec<Dev>,
    later: Vec<FakeDevice>,
    count: u32,
    /// The agent registered: its owner on the bus, and its path.
    agent: Option<(String, String)>,
    calls: Vec<Call>,
    /// How long a connection, a refusal or a pairing takes.
    takes: Duration,
}

impl State {
    fn dev(&self, path: &str) -> Option<&Dev> {
        self.devices.iter().find(|d| d.path == path)
    }

    fn dev_mut(&mut self, path: &str) -> Option<&mut Dev> {
        self.devices.iter_mut().find(|d| d.path == path)
    }
}

struct Inner {
    state: Mutex<State>,
    connection: OnceLock<Connection>,
}

type Shared = Arc<Inner>;

fn state<T>(shared: &Shared, with: impl FnOnce(&mut State) -> T) -> T {
    with(&mut shared.state.lock().unwrap_or_else(PoisonError::into_inner))
}

fn failed(name: &str, message: &str) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{name}: {message}"))
}

/// The fake, its objects served on its own connection.
pub struct Bluez {
    shared: Shared,
    handle: tokio::runtime::Handle,
}

impl Bluez {
    /// Serves BlueZ on the bus at `address` as `setup` says.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        setup: Setup,
    ) -> anyhow::Result<Self> {
        let shared: Shared = Arc::new(Inner {
            state: Mutex::new(State {
                powered: setup.powered && !setup.blocked,
                blocked: setup.blocked,
                later: setup.later.clone(),
                takes: TAKES,
                ..State::default()
            }),
            connection: OnceLock::new(),
        });
        let address = address.to_owned();
        let served = shared.clone();
        runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(BLUEZ)
                .serve_at("/", zbus::fdo::ObjectManager)
                .serve_at(ROOT, AgentManager(served.clone()))
                .build()
                .await?;
            let _ = served.connection.set(connection.clone());
            if setup.adapter {
                connection
                    .object_server()
                    .at(ADAPTER_PATH, Adapter(served.clone()))
                    .await?;
            }
            for device in setup.devices {
                add_device(&served, device).await?;
            }
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(Self {
            shared,
            handle: runtime.handle().clone(),
        })
    }

    /// What the shell asked, in order.
    pub fn calls(&self) -> Vec<Call> {
        state(&self.shared, |s| s.calls.clone())
    }

    /// The devices connected now, by name.
    pub fn connected(&self) -> Vec<String> {
        state(&self.shared, |s| {
            s.devices
                .iter()
                .filter(|d| d.connected)
                .map(Dev::name)
                .collect()
        })
    }

    /// Whether `name` is paired, and trusted.
    pub fn paired(&self, name: &str) -> (bool, bool) {
        state(&self.shared, |s| {
            s.devices
                .iter()
                .find(|d| d.name() == name)
                .map_or((false, false), |d| (d.paired, d.trusted))
        })
    }

    /// Connections, refusals and pairings take `takes` from now on.
    pub fn set_takes(&self, takes: Duration) {
        state(&self.shared, |s| s.takes = takes);
    }

    pub fn powered(&self) -> bool {
        state(&self.shared, |s| s.powered)
    }

    pub fn discovering(&self) -> bool {
        state(&self.shared, |s| s.discovering)
    }

    /// The adapter switched on or off, as if from elsewhere.
    pub fn set_powered(&self, on: bool) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle
            .block_on(async move { power(&shared, on).await.map_err(Into::into) })
    }

    /// Device `name` connects by itself (or leaves).
    pub fn set_connected(&self, name: &str, connected: bool) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let path = state(&shared, |s| {
            s.devices
                .iter()
                .find(|d| d.name() == name)
                .map(|d| d.path.clone())
        })
        .ok_or_else(|| anyhow::anyhow!("no {name}"))?;
        self.handle
            .block_on(async move { connect(&shared, &path, connected).await })
    }

    /// Device `name`'s battery says `percent`.
    pub fn set_battery(&self, name: &str, percent: u8) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let path = state(&shared, |s| {
            let dev = s.devices.iter_mut().find(|d| d.name() == name)?;
            dev.fake.battery = Some(percent);
            Some(dev.path.clone())
        })
        .ok_or_else(|| anyhow::anyhow!("no {name}"))?;
        self.handle.block_on(async move {
            changed(
                &shared,
                &path,
                "org.bluez.Battery1",
                vec![("Percentage", Variant::from(percent))],
            )
            .await
        })
    }

    /// The adapter goes, as when it is unplugged.
    pub fn remove_adapter(&self) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            let Some(connection) = shared.connection.get() else {
                return Ok(());
            };
            let paths: Vec<String> = state(&shared, |s| {
                s.devices.drain(..).map(|d| d.path).collect::<Vec<_>>()
            });
            for path in paths {
                let _ = connection
                    .object_server()
                    .remove::<Battery, _>(path.as_str())
                    .await;
                connection
                    .object_server()
                    .remove::<Device, _>(path.as_str())
                    .await?;
            }
            connection
                .object_server()
                .remove::<Adapter, _>(ADAPTER_PATH)
                .await?;
            Ok(())
        })
    }
}

/// `PropertiesChanged` for `iface` at `at`, with the values.
async fn changed(
    shared: &Shared,
    at: &str,
    iface: &str,
    values: Vec<(&str, Variant<'_>)>,
) -> anyhow::Result<()> {
    let Some(connection) = shared.connection.get() else {
        return Ok(());
    };
    let map: HashMap<&str, Variant<'_>> = values.into_iter().collect();
    connection
        .emit_signal(
            None::<&str>,
            at,
            PROPERTIES,
            "PropertiesChanged",
            &(iface, map, Vec::<&str>::new()),
        )
        .await?;
    Ok(())
}

async fn add_device(shared: &Shared, fake: FakeDevice) -> anyhow::Result<String> {
    let path = state(shared, |s| {
        s.count += 1;
        let path = format!("{ADAPTER_PATH}/dev_5C_12_AA_01_02_{:02}", s.count);
        s.devices.push(Dev {
            path: path.clone(),
            fake,
            paired: fake.paired,
            trusted: fake.paired,
            connected: fake.connected,
            battery: false,
            cancel: Arc::new(Notify::new()),
        });
        path
    });
    if let Some(connection) = shared.connection.get() {
        connection
            .object_server()
            .at(path.as_str(), Device(shared.clone(), path.clone()))
            .await?;
    }
    if fake.connected {
        battery(shared, &path, true).await?;
    }
    Ok(path)
}

/// Device `path`'s battery's interface, served while it is connected.
async fn battery(shared: &Shared, path: &str, on: bool) -> anyhow::Result<()> {
    let Some(connection) = shared.connection.get() else {
        return Ok(());
    };
    let (has, served) = state(shared, |s| {
        s.dev(path)
            .map_or((false, false), |d| (d.fake.battery.is_some(), d.battery))
    });
    let server = connection.object_server();
    if on && has && !served {
        state(shared, |s| {
            if let Some(d) = s.dev_mut(path) {
                d.battery = true;
            }
        });
        server
            .at(path, Battery(shared.clone(), path.to_owned()))
            .await?;
    } else if !on && served {
        state(shared, |s| {
            if let Some(d) = s.dev_mut(path) {
                d.battery = false;
            }
        });
        server.remove::<Battery, _>(path).await?;
    }
    Ok(())
}

/// Device `path` connected, or not, said.
async fn connect(shared: &Shared, path: &str, connected: bool) -> anyhow::Result<()> {
    let was = state(shared, |s| {
        s.dev_mut(path)
            .map(|d| std::mem::replace(&mut d.connected, connected))
    });
    if was == Some(connected) {
        return Ok(());
    }
    if connected {
        changed(
            shared,
            path,
            DEVICE,
            vec![("Connected", Variant::from(true))],
        )
        .await?;
        battery(shared, path, true).await
    } else {
        battery(shared, path, false).await?;
        changed(
            shared,
            path,
            DEVICE,
            vec![("Connected", Variant::from(false))],
        )
        .await
    }
}

async fn power(shared: &Shared, on: bool) -> zbus::fdo::Result<()> {
    let blocked = state(shared, |s| s.blocked);
    if on && blocked {
        return Err(zbus::fdo::Error::Failed(
            "org.bluez.Error.Blocked: blocked through rfkill".to_owned(),
        ));
    }
    let was = state(shared, |s| std::mem::replace(&mut s.powered, on));
    if was == on {
        return Ok(());
    }
    let mut values = vec![("Powered", Variant::from(on))];
    if !on {
        // BlueZ ends a search and the connections with the adapter.
        let connected: Vec<String> = state(shared, |s| {
            s.discovering = false;
            s.devices
                .iter()
                .filter(|d| d.connected)
                .map(|d| d.path.clone())
                .collect()
        });
        for path in connected {
            connect(shared, &path, false)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        values.push(("Discovering", Variant::from(false)));
    }
    changed(shared, ADAPTER_PATH, ADAPTER, values)
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

struct AgentManager(Shared);

#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: Header<'_>,
    ) {
        let owner = header.sender().map(|s| s.to_string()).unwrap_or_default();
        state(&self.0, |s| {
            s.calls.push(Call::RegisterAgent(capability));
            s.agent = Some((owner, agent.as_str().to_owned()));
        });
    }

    fn unregister_agent(&self, _agent: OwnedObjectPath) {
        state(&self.0, |s| s.agent = None);
    }
}

struct Adapter(Shared);

#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property)]
    fn powered(&self) -> bool {
        state(&self.0, |s| s.powered)
    }

    #[zbus(property)]
    async fn set_powered(&mut self, on: bool) -> zbus::fdo::Result<()> {
        let blocked = state(&self.0, |s| {
            s.calls.push(Call::Power(on));
            s.blocked
        });
        if on && blocked {
            return Err(zbus::fdo::Error::Failed(
                "org.bluez.Error.Blocked: blocked through rfkill".to_owned(),
            ));
        }
        // Aside: the connections it ends change the object server, which
        // this setter holds.
        let shared = self.0.clone();
        tokio::spawn(async move {
            let _ = power(&shared, on).await;
        });
        Ok(())
    }

    #[zbus(property)]
    fn power_state(&self) -> String {
        state(&self.0, |s| match (s.powered, s.blocked) {
            (true, _) => "on",
            (false, true) => "off-blocked",
            (false, false) => "off",
        })
        .to_owned()
    }

    #[zbus(property)]
    fn discovering(&self) -> bool {
        state(&self.0, |s| s.discovering)
    }

    #[zbus(property)]
    fn address(&self) -> String {
        "5C:12:AA:00:00:01".to_owned()
    }

    async fn start_discovery(&self) -> zbus::fdo::Result<()> {
        let later = state(&self.0, |s| {
            s.calls.push(Call::StartDiscovery);
            if !s.powered {
                return None;
            }
            s.discovering = true;
            Some(std::mem::take(&mut s.later))
        });
        let Some(later) = later else {
            return Err(zbus::fdo::Error::Failed(
                "org.bluez.Error.NotReady".to_owned(),
            ));
        };
        let shared = self.0.clone();
        tokio::spawn(async move {
            let _ = changed(
                &shared,
                ADAPTER_PATH,
                ADAPTER,
                vec![("Discovering", Variant::from(true))],
            )
            .await;
            tokio::time::sleep(takes(&shared)).await;
            for device in later {
                let _ = add_device(&shared, device).await;
            }
        });
        Ok(())
    }

    async fn stop_discovery(&self) -> zbus::fdo::Result<()> {
        let was = state(&self.0, |s| {
            s.calls.push(Call::StopDiscovery);
            std::mem::replace(&mut s.discovering, false)
        });
        if !was {
            return Err(zbus::fdo::Error::Failed(
                "org.bluez.Error.Failed: No discovery started".to_owned(),
            ));
        }
        changed(
            &self.0,
            ADAPTER_PATH,
            ADAPTER,
            vec![("Discovering", Variant::from(false))],
        )
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

struct Device(Shared, String);

impl Device {
    fn with<T>(&self, with: impl FnOnce(&Dev) -> T) -> Option<T> {
        state(&self.0, |s| s.dev(&self.1).map(with))
    }
}

#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn address(&self) -> String {
        self.with(Dev::address).unwrap_or_default()
    }

    #[zbus(property)]
    fn alias(&self) -> String {
        self.with(Dev::name).unwrap_or_default()
    }

    // `Name` only when the device said it.
    #[zbus(property)]
    fn name(&self) -> zbus::fdo::Result<String> {
        self.with(|d| d.fake.name.map(str::to_owned))
            .flatten()
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("Name".to_owned()))
    }

    #[zbus(property)]
    fn icon(&self) -> zbus::fdo::Result<String> {
        self.with(|d| d.fake.icon)
            .filter(|i| !i.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("Icon".to_owned()))
    }

    #[zbus(property)]
    fn paired(&self) -> bool {
        self.with(|d| d.paired).unwrap_or(false)
    }

    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.with(|d| d.trusted).unwrap_or(false)
    }

    #[zbus(property)]
    async fn set_trusted(&mut self, trusted: bool) {
        let name = state(&self.0, |s| {
            let d = s.dev_mut(&self.1)?;
            d.trusted = trusted;
            Some(d.name())
        });
        if let Some(name) = name {
            state(&self.0, |s| s.calls.push(Call::Trust(name)));
        }
    }

    #[zbus(property)]
    fn connected(&self) -> bool {
        self.with(|d| d.connected).unwrap_or(false)
    }

    #[zbus(property, name = "RSSI")]
    fn rssi(&self) -> zbus::fdo::Result<i16> {
        self.with(|d| d.fake.rssi)
            .flatten()
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty("RSSI".to_owned()))
    }

    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(ADAPTER_PATH).unwrap_or_default()
    }

    async fn connect(&self) -> zbus::fdo::Result<()> {
        let Some((name, answer, cancel)) =
            self.with(|d| (d.name(), d.fake.answer, d.cancel.clone()))
        else {
            return Err(zbus::fdo::Error::UnknownObject(self.1.clone()));
        };
        state(&self.0, |s| s.calls.push(Call::Connect(name.clone())));
        match answer {
            Answer::Connects => {
                tokio::time::sleep(takes(&self.0)).await;
                connect(&self.0, &self.1, true)
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }
            Answer::Refuses => {
                tokio::time::sleep(takes(&self.0)).await;
                Err(failed(
                    "org.bluez.Error.Failed",
                    "br-connection-page-timeout",
                ))
            }
            Answer::Hangs => {
                cancel.notified().await;
                Err(failed("org.bluez.Error.Failed", "br-connection-canceled"))
            }
        }
    }

    async fn disconnect(&self) -> zbus::fdo::Result<()> {
        let Some((name, cancel)) = self.with(|d| (d.name(), d.cancel.clone())) else {
            return Err(zbus::fdo::Error::UnknownObject(self.1.clone()));
        };
        state(&self.0, |s| s.calls.push(Call::Disconnect(name)));
        cancel.notify_waiters();
        connect(&self.0, &self.1, false)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn pair(&self) -> zbus::fdo::Result<()> {
        let Some((name, pairing)) = self.with(|d| (d.name(), d.fake.pairing)) else {
            return Err(zbus::fdo::Error::UnknownObject(self.1.clone()));
        };
        state(&self.0, |s| s.calls.push(Call::Pair(name)));
        tokio::time::sleep(takes(&self.0)).await;
        match pairing {
            Pairing::JustWorks => {}
            Pairing::Fails => {
                return Err(failed(
                    "org.bluez.Error.AuthenticationFailed",
                    "Authentication Failed",
                ));
            }
            Pairing::Passkey => {
                // BlueZ asks the agent of whoever asked for the pairing.
                let agent = state(&self.0, |s| s.agent.clone());
                let connection = self.0.connection.get().cloned();
                let (Some((owner, path)), Some(connection)) = (agent, connection) else {
                    return Err(failed("org.bluez.Error.AuthenticationFailed", "no agent"));
                };
                let device = OwnedObjectPath::try_from(self.1.as_str()).unwrap_or_default();
                let asked = connection
                    .call_method(
                        Some(owner.as_str()),
                        path.as_str(),
                        Some("org.bluez.Agent1"),
                        "RequestPasskey",
                        &(device,),
                    )
                    .await;
                if asked.is_err() {
                    return Err(failed(
                        "org.bluez.Error.AuthenticationRejected",
                        "Authentication Rejected",
                    ));
                }
            }
        }
        state(&self.0, |s| {
            if let Some(d) = s.dev_mut(&self.1) {
                d.paired = true;
            }
        });
        changed(
            &self.0,
            &self.1,
            DEVICE,
            vec![("Paired", Variant::from(true))],
        )
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    fn cancel_pairing(&self) {
        if let Some((name, cancel)) = self.with(|d| (d.name(), d.cancel.clone())) {
            state(&self.0, |s| s.calls.push(Call::CancelPairing(name)));
            cancel.notify_waiters();
        }
    }
}

struct Battery(Shared, String);

#[zbus::interface(name = "org.bluez.Battery1")]
impl Battery {
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        state(&self.0, |s| s.dev(&self.1).and_then(|d| d.fake.battery)).unwrap_or(0)
    }
}
