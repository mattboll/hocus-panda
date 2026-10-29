//! NetworkManager, faked on the fakes' private bus (`fakes.rs`), never the
//! machine's: the manager (`WirelessEnabled`, `Connectivity` and its check
//! address), a Wi-Fi device and, if asked, a wired one, access points with
//! their strength and security, saved connections, active connections.
//!
//! It behaves as NetworkManager does where the shell looks: a scan updates
//! `LastScan` (and brings the networks asked to appear then); a connection
//! asked for goes through its states and succeeds when the password is the
//! network's (or none is needed), else fails for its secrets (device
//! reason 7, active connection reason 9); `Disconnect` leaves; the Wi-Fi
//! switched off takes the device down. Every change is signalled with its
//! values (`PropertiesChanged`), and what the shell asked is recorded,
//! passwords included, for the tests to compare.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};

use zbus::{Connection, OwnedObjectPath, OwnedVariant, Variant, connection::Builder};

pub const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const WIFI: &str = "/org/freedesktop/NetworkManager/Devices/1";
const WIRED: &str = "/org/freedesktop/NetworkManager/Devices/2";
const SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";

/// How long a connection takes, and a scan.
const TAKES: Duration = Duration::from_millis(400);
const SCAN_TAKES: Duration = Duration::from_millis(1500);

/// What a network asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA2 with this password.
    Psk(&'static str),
    Enterprise,
}

#[derive(Debug, Clone, Copy)]
pub struct FakeAp {
    pub ssid: &'static str,
    pub strength: u8,
    pub security: Security,
}

impl FakeAp {
    pub const fn new(ssid: &'static str, strength: u8, security: Security) -> Self {
        Self {
            ssid,
            strength,
            security,
        }
    }
}

/// What NetworkManager says as it starts.
#[derive(Debug, Clone)]
pub struct Setup {
    pub aps: Vec<FakeAp>,
    /// Networks that appear at the first scan.
    pub later: Vec<FakeAp>,
    /// Saved connections: their SSID and the password saved with them.
    pub saved: Vec<(&'static str, Option<&'static str>)>,
    /// The network connected as it starts.
    pub current: Option<&'static str>,
    /// Networks behind a sign-in page.
    pub portals: Vec<&'static str>,
    pub hardware: bool,
    pub wired: bool,
    pub enabled: bool,
    pub check_uri: &'static str,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            aps: Vec::new(),
            later: Vec::new(),
            saved: Vec::new(),
            current: None,
            portals: Vec::new(),
            hardware: true,
            wired: false,
            enabled: true,
            check_uri: "http://check.hocus.test/portal",
        }
    }
}

/// What the shell asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Scan,
    Enable(bool),
    Activate { ssid: String },
    AddAndActivate { ssid: String, psk: Option<String> },
    Update { ssid: String, psk: Option<String> },
    Delete { ssid: String },
    Disconnect,
    Deactivate,
}

#[derive(Debug)]
struct Ap {
    path: String,
    fake: FakeAp,
}

#[derive(Debug)]
struct Saved {
    path: String,
    ssid: String,
    psk: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    enabled: bool,
    hardware: bool,
    wired: bool,
    connectivity: u32,
    check_uri: String,
    portals: Vec<&'static str>,
    aps: Vec<Ap>,
    later: Vec<FakeAp>,
    saved: Vec<Saved>,
    count: u32,
    /// The Wi-Fi device's state and its access point ("/" for none).
    device: u32,
    active_ap: String,
    /// The active connection: its path, its saved connection, its access
    /// point, its state.
    active: Option<(String, String, String, u32)>,
    last_scan: i64,
    calls: Vec<Call>,
    /// How long a connection takes.
    takes: Duration,
}

impl State {
    fn next(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    fn ap(&self, path: &str) -> Option<&Ap> {
        self.aps.iter().find(|a| a.path == path)
    }

    fn actives(&self) -> Vec<String> {
        self.active.iter().map(|(p, ..)| p.clone()).collect()
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

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap_or_else(|_| OwnedObjectPath::default())
}

fn paths(list: &[String]) -> Vec<OwnedObjectPath> {
    list.iter().map(|p| path(p)).collect()
}

fn none() -> OwnedObjectPath {
    path("/")
}

/// The fake, its objects served on `connection`.
pub struct Nm {
    shared: Shared,
    handle: tokio::runtime::Handle,
}

impl Nm {
    /// Serves NetworkManager on the bus at `address` as `setup` says.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        setup: Setup,
    ) -> anyhow::Result<Self> {
        let shared: Shared = Arc::new(Inner {
            state: Mutex::new(State {
                enabled: setup.enabled,
                hardware: setup.hardware,
                wired: setup.wired,
                connectivity: if setup.wired { 4 } else { 1 },
                check_uri: setup.check_uri.to_owned(),
                portals: setup.portals.clone(),
                later: setup.later.clone(),
                device: if setup.enabled { 30 } else { 20 },
                active_ap: "/".to_owned(),
                takes: TAKES,
                ..State::default()
            }),
            connection: OnceLock::new(),
        });
        let address = address.to_owned();
        let served = shared.clone();
        runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(NM)
                .serve_at(ROOT, Manager(served.clone()))
                .serve_at(SETTINGS, Settings(served.clone()))
                .build()
                .await?;
            let _ = served.connection.set(connection.clone());
            let server = connection.object_server();
            if setup.hardware {
                server.at(WIFI, Device(served.clone(), WIFI, 2)).await?;
                server.at(WIFI, Wireless(served.clone())).await?;
            }
            server.at(WIRED, Device(served.clone(), WIRED, 1)).await?;
            for ap in setup.aps {
                add_ap(&served, ap, false).await?;
            }
            for (ssid, psk) in setup.saved {
                add_saved(&served, ssid, psk.map(str::to_owned), false).await?;
            }
            if let Some(current) = setup.current {
                let ap = state(&served, |s| {
                    s.aps
                        .iter()
                        .find(|a| a.fake.ssid == current)
                        .map(|a| a.path.clone())
                });
                let saved = state(&served, |s| {
                    s.saved
                        .iter()
                        .find(|c| c.ssid == current)
                        .map(|c| c.path.clone())
                });
                if let (Some(ap), Some(saved)) = (ap, saved) {
                    let active = add_active(&served, &saved, &ap).await?;
                    state(&served, |s| {
                        s.device = 100;
                        s.active_ap = ap;
                        s.connectivity = if s.portals.contains(&current) { 2 } else { 4 };
                        if let Some(a) = &mut s.active {
                            a.3 = 2;
                        }
                        let _ = active;
                    });
                }
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

    /// The SSID connected now, if any.
    pub fn connected(&self) -> Option<String> {
        state(&self.shared, |s| {
            (s.device == 100)
                .then(|| s.ap(&s.active_ap).map(|a| a.fake.ssid.to_owned()))
                .flatten()
        })
    }

    pub fn enabled(&self) -> bool {
        state(&self.shared, |s| s.enabled)
    }

    /// The saved connections' SSIDs and passwords.
    pub fn saved(&self) -> Vec<(String, Option<String>)> {
        state(&self.shared, |s| {
            s.saved
                .iter()
                .map(|c| (c.ssid.clone(), c.psk.clone()))
                .collect()
        })
    }

    /// Access point `ssid`'s strength changes, as a scan says.
    pub fn set_strength(&self, ssid: &str, strength: u8) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let ssid = ssid.to_owned();
        self.handle.block_on(async move {
            let paths: Vec<String> = state(&shared, |s| {
                s.aps
                    .iter_mut()
                    .filter(|a| a.fake.ssid == ssid)
                    .map(|a| {
                        a.fake.strength = strength;
                        a.path.clone()
                    })
                    .collect()
            });
            for p in paths {
                changed(
                    &shared,
                    &p,
                    "org.freedesktop.NetworkManager.AccessPoint",
                    vec![("Strength", Variant::from(strength))],
                )
                .await?;
            }
            Ok(())
        })
    }

    /// A sign-in page passed (or back): `Connectivity` full (or portal).
    pub fn set_portal(&self, portal: bool) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            let value = if portal { 2u32 } else { 4 };
            state(&shared, |s| s.connectivity = value);
            changed(
                &shared,
                ROOT,
                NM,
                vec![("Connectivity", Variant::from(value))],
            )
            .await
        })
    }

    /// NetworkManager joins `ssid`'s saved connection by itself, as it
    /// does on its own.
    pub fn join(&self, ssid: &str) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let found = state(&shared, |s| {
            let saved = s.saved.iter().find(|c| c.ssid == ssid)?;
            let ap = s.aps.iter().find(|a| a.fake.ssid == ssid)?;
            Some((saved.path.clone(), ap.path.clone(), saved.psk.clone()))
        });
        let (saved, ap, psk) = found.ok_or_else(|| anyhow::anyhow!("no saved {ssid}"))?;
        self.handle
            .block_on(async move { activate(shared, saved, ap, psk).await.map(|_| ()) })
    }

    /// Connections take `takes` from now on.
    pub fn set_takes(&self, takes: Duration) {
        state(&self.shared, |s| s.takes = takes);
    }

    /// The Wi-Fi device leaves its network, as if from elsewhere.
    pub fn disconnect(&self) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            drop_active(&shared, 2).await?;
            device_state(&shared, 30, 39).await
        })
    }

    /// The Wi-Fi switched off or on, as if from elsewhere.
    pub fn set_enabled(&self, on: bool) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            state(&shared, |s| s.enabled = on);
            changed(
                &shared,
                ROOT,
                NM,
                vec![("WirelessEnabled", Variant::from(on))],
            )
            .await?;
            if on {
                device_state(&shared, 30, 0).await
            } else {
                drop_active(&shared, 3).await?;
                device_state(&shared, 20, 0).await
            }
        })
    }

    /// A cable plugged in or out.
    pub fn set_wired(&self, wired: bool) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.handle.block_on(async move {
            let new = if wired { 100u32 } else { 20 };
            state(&shared, |s| s.wired = wired);
            changed(&shared, WIRED, DEVICE, vec![("State", Variant::from(new))]).await?;
            emit(&shared, WIRED, DEVICE, "StateChanged", &(new, 30u32, 0u32)).await
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
    let map: HashMap<&str, Variant<'_>> = values.into_iter().collect();
    emit(
        shared,
        at,
        PROPERTIES,
        "PropertiesChanged",
        &(iface, map, Vec::<&str>::new()),
    )
    .await
}

async fn emit<B>(
    shared: &Shared,
    at: &str,
    iface: &str,
    member: &str,
    body: &B,
) -> anyhow::Result<()>
where
    B: serde::Serialize + zbus::wire::DynamicType,
{
    let Some(connection) = shared.connection.get() else {
        return Ok(());
    };
    connection
        .emit_signal(None::<&str>, at, iface, member, body)
        .await?;
    Ok(())
}

async fn add_ap(shared: &Shared, fake: FakeAp, announce: bool) -> anyhow::Result<String> {
    let p = state(shared, |s| {
        let p = format!("/org/freedesktop/NetworkManager/AccessPoint/{}", s.next());
        s.aps.push(Ap {
            path: p.clone(),
            fake,
        });
        p
    });
    if let Some(connection) = shared.connection.get() {
        connection
            .object_server()
            .at(p.as_str(), AccessPoint(shared.clone(), p.clone()))
            .await?;
    }
    if announce {
        emit(shared, WIFI, WIRELESS, "AccessPointAdded", &(path(&p),)).await?;
        let all: Vec<String> = state(shared, |s| s.aps.iter().map(|a| a.path.clone()).collect());
        changed(
            shared,
            WIFI,
            WIRELESS,
            vec![("AccessPoints", Variant::from(paths(&all)))],
        )
        .await?;
    }
    Ok(p)
}

async fn add_saved(
    shared: &Shared,
    ssid: &str,
    psk: Option<String>,
    announce: bool,
) -> anyhow::Result<String> {
    let p = state(shared, |s| {
        let p = format!("/org/freedesktop/NetworkManager/Settings/{}", s.next());
        s.saved.push(Saved {
            path: p.clone(),
            ssid: ssid.to_owned(),
            psk,
        });
        p
    });
    if let Some(connection) = shared.connection.get() {
        connection
            .object_server()
            .at(p.as_str(), SavedConnection(shared.clone(), p.clone()))
            .await?;
    }
    if announce {
        emit(
            shared,
            SETTINGS,
            SETTINGS_IFACE,
            "NewConnection",
            &(path(&p),),
        )
        .await?;
    }
    Ok(p)
}

async fn add_active(shared: &Shared, saved: &str, ap: &str) -> anyhow::Result<String> {
    let p = state(shared, |s| {
        let p = format!(
            "/org/freedesktop/NetworkManager/ActiveConnection/{}",
            s.next()
        );
        s.active = Some((p.clone(), saved.to_owned(), ap.to_owned(), 1));
        p
    });
    if let Some(connection) = shared.connection.get() {
        connection
            .object_server()
            .at(p.as_str(), Active(shared.clone()))
            .await?;
    }
    Ok(p)
}

/// The Wi-Fi device's new state, said both ways.
async fn device_state(shared: &Shared, new: u32, reason: u32) -> anyhow::Result<()> {
    let old = state(shared, |s| std::mem::replace(&mut s.device, new));
    changed(shared, WIFI, DEVICE, vec![("State", Variant::from(new))]).await?;
    emit(shared, WIFI, DEVICE, "StateChanged", &(new, old, reason)).await
}

/// The active connection goes, the device left without a network.
async fn drop_active(shared: &Shared, reason: u32) -> anyhow::Result<()> {
    let gone = state(shared, |s| {
        s.active_ap = "/".to_owned();
        s.connectivity = if s.wired { 4 } else { 1 };
        s.active.take()
    });
    if let Some((p, ..)) = &gone {
        emit(shared, p, ACTIVE, "StateChanged", &(4u32, reason)).await?;
        if let Some(connection) = shared.connection.get() {
            connection
                .object_server()
                .remove::<Active, _>(p.as_str())
                .await?;
        }
    }
    let connectivity = state(shared, |s| s.connectivity);
    changed(
        shared,
        WIFI,
        WIRELESS,
        vec![("ActiveAccessPoint", Variant::from(none()))],
    )
    .await?;
    changed(
        shared,
        ROOT,
        NM,
        vec![
            (
                "ActiveConnections",
                Variant::from(Vec::<OwnedObjectPath>::new()),
            ),
            ("Connectivity", Variant::from(connectivity)),
        ],
    )
    .await
}

/// A connection to the access point `ap` with the saved connection
/// `saved` and the password `psk`: it ends after a while, connected if the
/// password is right.
async fn activate(
    shared: Shared,
    saved: String,
    ap: String,
    psk: Option<String>,
) -> anyhow::Result<String> {
    if state(&shared, |s| s.active.is_some()) {
        drop_active(&shared, 2).await?;
    }
    let active = add_active(&shared, &saved, &ap).await?;
    device_state(&shared, 40, 0).await?;
    let actives = state(&shared, |s| s.actives());
    changed(
        &shared,
        ROOT,
        NM,
        vec![("ActiveConnections", Variant::from(paths(&actives)))],
    )
    .await?;
    let task = shared.clone();
    let returned = active.clone();
    let takes = state(&shared, |s| s.takes);
    tokio::spawn(async move {
        tokio::time::sleep(takes / 2).await;
        let _ = device_state(&task, 50, 0).await;
        tokio::time::sleep(takes / 2).await;
        let (right, ssid) = state(&task, |s| {
            let fake = s.ap(&ap).map(|a| a.fake);
            let right = match fake.map(|f| f.security) {
                Some(Security::Open) => true,
                Some(Security::Psk(want)) => psk.as_deref() == Some(want),
                _ => false,
            };
            (right, fake.map(|f| f.ssid).unwrap_or_default())
        });
        if right {
            let connectivity = state(&task, |s| {
                s.active_ap.clone_from(&ap);
                s.connectivity = if s.portals.contains(&ssid) { 2 } else { 4 };
                if let Some(a) = &mut s.active {
                    a.3 = 2;
                }
                s.connectivity
            });
            let _ = changed(
                &task,
                WIFI,
                WIRELESS,
                vec![("ActiveAccessPoint", Variant::from(path(&ap)))],
            )
            .await;
            let _ = device_state(&task, 100, 0).await;
            let _ = emit(&task, &active, ACTIVE, "StateChanged", &(2u32, 0u32)).await;
            let _ = changed(
                &task,
                ROOT,
                NM,
                vec![("Connectivity", Variant::from(connectivity))],
            )
            .await;
        } else {
            // As NetworkManager does without an agent to ask: the secrets
            // are missing.
            let _ = device_state(&task, 60, 8).await;
            let _ = device_state(&task, 120, 7).await;
            let _ = drop_active(&task, 9).await;
            let _ = device_state(&task, 30, 0).await;
        }
    });
    Ok(returned)
}

fn ssid_of(settings: &HashMap<String, HashMap<String, OwnedVariant>>) -> Option<String> {
    let value = settings
        .get("802-11-wireless")?
        .get("ssid")?
        .try_clone()
        .ok()?;
    let bytes = Vec::<u8>::try_from(value).ok()?;
    String::from_utf8(bytes).ok()
}

fn psk_of(settings: &HashMap<String, HashMap<String, OwnedVariant>>) -> Option<String> {
    let value = settings.get("802-11-wireless-security")?.get("psk")?;
    value.downcast_ref::<String>().ok()
}

fn failed(err: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(err.to_string())
}

struct Manager(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        state(&self.0, |s| s.enabled)
    }

    #[zbus(property)]
    async fn set_wireless_enabled(&mut self, on: bool) {
        let was = state(&self.0, |s| {
            s.calls.push(Call::Enable(on));
            std::mem::replace(&mut s.enabled, on)
        });
        if was == on {
            return;
        }
        let shared = self.0.clone();
        tokio::spawn(async move {
            if on {
                let _ = device_state(&shared, 30, 0).await;
            } else {
                let _ = drop_active(&shared, 3).await;
                let _ = device_state(&shared, 20, 0).await;
            }
        });
    }

    #[zbus(property)]
    fn wireless_hardware_enabled(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn connectivity(&self) -> u32 {
        state(&self.0, |s| s.connectivity)
    }

    #[zbus(property)]
    fn connectivity_check_uri(&self) -> String {
        state(&self.0, |s| s.check_uri.clone())
    }

    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        state(&self.0, |s| {
            let mut devices = Vec::new();
            if s.hardware {
                devices.push(path(WIFI));
            }
            devices.push(path(WIRED));
            devices
        })
    }

    #[zbus(property)]
    fn active_connections(&self) -> Vec<OwnedObjectPath> {
        paths(&state(&self.0, |s| s.actives()))
    }

    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        state(&self.0, |s| {
            s.actives().first().map_or_else(none, |p| path(p))
        })
    }

    async fn activate_connection(
        &self,
        connection: OwnedObjectPath,
        _device: OwnedObjectPath,
        specific: OwnedObjectPath,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let saved = state(&self.0, |s| {
            let saved = s.saved.iter().find(|c| c.path == connection.as_str())?;
            s.calls.push(Call::Activate {
                ssid: saved.ssid.clone(),
            });
            let ap = if specific.as_str() == "/" {
                s.aps
                    .iter()
                    .find(|a| a.fake.ssid == saved.ssid)
                    .map(|a| a.path.clone())?
            } else {
                specific.as_str().to_owned()
            };
            Some((saved.path.clone(), ap, saved.psk.clone()))
        });
        let (saved, ap, psk) = saved.ok_or_else(|| failed("no such connection"))?;
        let active = activate(self.0.clone(), saved, ap, psk)
            .await
            .map_err(failed)?;
        Ok(path(&active))
    }

    async fn add_and_activate_connection(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
        _device: OwnedObjectPath,
        specific: OwnedObjectPath,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        let ssid = ssid_of(&settings).ok_or_else(|| failed("no SSID"))?;
        let psk = psk_of(&settings);
        state(&self.0, |s| {
            s.calls.push(Call::AddAndActivate {
                ssid: ssid.clone(),
                psk: psk.clone(),
            });
        });
        let saved = add_saved(&self.0, &ssid, psk.clone(), true)
            .await
            .map_err(failed)?;
        let active = activate(
            self.0.clone(),
            saved.clone(),
            specific.as_str().to_owned(),
            psk,
        )
        .await
        .map_err(failed)?;
        Ok((path(&saved), path(&active)))
    }

    async fn deactivate_connection(&self, _active: OwnedObjectPath) -> zbus::fdo::Result<()> {
        state(&self.0, |s| s.calls.push(Call::Deactivate));
        drop_active(&self.0, 2).await.map_err(failed)?;
        device_state(&self.0, 30, 0).await.map_err(failed)
    }
}

struct Device(Shared, &'static str, u32);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        self.2
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        state(&self.0, |s| match self.2 {
            2 => s.device,
            _ if s.wired => 100,
            _ => 20,
        })
    }

    #[zbus(property)]
    fn interface(&self) -> String {
        if self.2 == 2 { "wlan0" } else { "eth0" }.to_owned()
    }

    async fn disconnect(&self) -> zbus::fdo::Result<()> {
        state(&self.0, |s| s.calls.push(Call::Disconnect));
        drop_active(&self.0, 2).await.map_err(failed)?;
        device_state(&self.0, 30, 39).await.map_err(failed)
    }
}

struct Wireless(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn access_points(&self) -> Vec<OwnedObjectPath> {
        state(&self.0, |s| s.aps.iter().map(|a| path(&a.path)).collect())
    }

    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        state(&self.0, |s| path(&s.active_ap))
    }

    #[zbus(property)]
    fn last_scan(&self) -> i64 {
        state(&self.0, |s| s.last_scan)
    }

    async fn request_scan(&self, _options: HashMap<String, OwnedVariant>) -> zbus::fdo::Result<()> {
        state(&self.0, |s| s.calls.push(Call::Scan));
        let shared = self.0.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SCAN_TAKES).await;
            let later = state(&shared, |s| std::mem::take(&mut s.later));
            for ap in later {
                let _ = add_ap(&shared, ap, true).await;
            }
            let last = state(&shared, |s| {
                s.last_scan += 1000;
                s.last_scan
            });
            let _ = changed(
                &shared,
                WIFI,
                WIRELESS,
                vec![("LastScan", Variant::from(last))],
            )
            .await;
        });
        Ok(())
    }
}

struct AccessPoint(Shared, String);

impl AccessPoint {
    fn fake(&self) -> Option<FakeAp> {
        state(&self.0, |s| s.ap(&self.1).map(|a| a.fake))
    }
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl AccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> Vec<u8> {
        self.fake()
            .map(|f| f.ssid.as_bytes().to_vec())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn strength(&self) -> u8 {
        self.fake().map_or(0, |f| f.strength)
    }

    #[zbus(property)]
    fn flags(&self) -> u32 {
        match self.fake().map(|f| f.security) {
            Some(Security::Open) | None => 0,
            _ => 1,
        }
    }

    #[zbus(property)]
    fn wpa_flags(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn rsn_flags(&self) -> u32 {
        match self.fake().map(|f| f.security) {
            Some(Security::Psk(_)) => 0x188,
            Some(Security::Enterprise) => 0x288,
            _ => 0,
        }
    }
}

struct Settings(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Settings {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        state(&self.0, |s| s.saved.iter().map(|c| path(&c.path)).collect())
    }
}

struct SavedConnection(Shared, String);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl SavedConnection {
    fn get_settings(&self) -> HashMap<String, HashMap<String, OwnedVariant>> {
        let ssid = state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.path == self.1)
                .map(|c| c.ssid.clone())
        })
        .unwrap_or_default();
        let owned = |v: Variant<'_>| OwnedVariant::try_from(v).ok();
        let mut settings = HashMap::new();
        let mut connection = HashMap::new();
        connection.extend(owned(Variant::from(ssid.as_str())).map(|v| ("id".to_owned(), v)));
        connection.extend(owned(Variant::from("802-11-wireless")).map(|v| ("type".to_owned(), v)));
        settings.insert("connection".to_owned(), connection);
        let mut wifi = HashMap::new();
        wifi.extend(owned(Variant::from(ssid.as_bytes())).map(|v| ("ssid".to_owned(), v)));
        settings.insert("802-11-wireless".to_owned(), wifi);
        settings
    }

    async fn update(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
    ) -> zbus::fdo::Result<()> {
        let psk = psk_of(&settings);
        state(&self.0, |s| {
            if let Some(saved) = s.saved.iter_mut().find(|c| c.path == self.1) {
                saved.psk.clone_from(&psk);
                s.calls.push(Call::Update {
                    ssid: saved.ssid.clone(),
                    psk,
                });
            }
        });
        emit(&self.0, &self.1, CONNECTION, "Updated", &())
            .await
            .map_err(failed)
    }

    async fn delete(&self) -> zbus::fdo::Result<()> {
        state(&self.0, |s| {
            if let Some(at) = s.saved.iter().position(|c| c.path == self.1) {
                let gone = s.saved.remove(at);
                s.calls.push(Call::Delete { ssid: gone.ssid });
            }
        });
        emit(
            &self.0,
            SETTINGS,
            SETTINGS_IFACE,
            "ConnectionRemoved",
            &(path(&self.1),),
        )
        .await
        .map_err(failed)
    }
}

struct Active(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn state(&self) -> u32 {
        state(&self.0, |s| s.active.as_ref().map_or(4, |a| a.3))
    }

    #[zbus(property)]
    fn connection(&self) -> OwnedObjectPath {
        state(&self.0, |s| {
            s.active.as_ref().map_or_else(none, |a| path(&a.1))
        })
    }

    #[zbus(property)]
    fn specific_object(&self) -> OwnedObjectPath {
        state(&self.0, |s| {
            s.active.as_ref().map_or_else(none, |a| path(&a.2))
        })
    }
}
