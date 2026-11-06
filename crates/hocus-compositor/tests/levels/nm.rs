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
//!
//! For the Settings' Réseau page (6.6) too: saved connections with their
//! UUID, when they were last used and whether they join by themselves;
//! their secrets read (`GetSecrets`), changed (`Update2`, keeping those an
//! update leaves out, as NetworkManager does) and given back
//! (`AddConnection`); a connection deleted while in use is left first;
//! the Wi-Fi device's IPv4 configuration. Changes can be refused as polkit
//! would ([`Nm::refuse`]).
#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashMap},
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
const IP4: &str = "/org/freedesktop/NetworkManager/IP4Config/1";
const IP4_WIRED: &str = "/org/freedesktop/NetworkManager/IP4Config/2";
const IP4_VPN: &str = "/org/freedesktop/NetworkManager/IP4Config/3";

/// How long a connection takes, and a scan.
const TAKES: Duration = Duration::from_millis(400);
const SCAN_TAKES: Duration = Duration::from_millis(1500);

/// What a network asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA2 with this password.
    Psk(&'static str),
    /// 802.1X: joined with any identity.
    Enterprise,
    /// WEP with this key.
    Wep(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct FakeAp {
    pub ssid: &'static str,
    pub strength: u8,
    pub security: Security,
    /// In MHz: 5 GHz unless said.
    pub frequency: u32,
}

impl FakeAp {
    pub const fn new(ssid: &'static str, strength: u8, security: Security) -> Self {
        Self {
            ssid,
            strength,
            security,
            frequency: 5180,
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
    /// When saved networks were last used, in seconds before now; the
    /// others never were.
    pub used: Vec<(&'static str, u64)>,
    /// Networks that do not say their name: joined by it, they appear.
    pub hidden: Vec<FakeAp>,
    /// VPN connections: their name, their kind (`wireguard`, `openvpn`),
    /// whether turning them on fails.
    pub vpns: Vec<(&'static str, &'static str, bool)>,
    /// NetworkManager's OpenVPN plugin is missing: such files do not
    /// import.
    pub no_openvpn: bool,
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
            used: Vec::new(),
            hidden: Vec::new(),
            vpns: Vec::new(),
            no_openvpn: false,
        }
    }
}

/// What the shell asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Scan,
    Enable(bool),
    Activate {
        ssid: String,
    },
    AddAndActivate {
        ssid: String,
        psk: Option<String>,
    },
    Update {
        ssid: String,
        psk: Option<String>,
    },
    /// The Settings' change of a saved connection: the password and
    /// `autoconnect` it sends, if it does.
    Update2 {
        ssid: String,
        psk: Option<String>,
        autoconnect: Option<bool>,
    },
    GetSecrets {
        ssid: String,
    },
    /// A connection given back: its UUID, its password.
    AddConnection {
        ssid: String,
        uuid: String,
        psk: Option<String>,
    },
    Delete {
        ssid: String,
    },
    Disconnect,
    Deactivate,
    /// A VPN imported with its user name and password.
    VpnCredentials {
        name: String,
        user: String,
        password: String,
    },
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
    /// What a connection is right with: the password, the WEP key, or
    /// `8021x:IDENTITY`.
    secret: Option<String>,
    uuid: String,
    /// Seconds since the epoch; 0: never used.
    timestamp: u64,
    autoconnect: bool,
    /// A VPN: its kind, whether it fails.
    vpn: Option<(String, bool)>,
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
    /// Changes to saved connections refused, as polkit would.
    refuse: bool,
    hidden: Vec<FakeAp>,
    /// The settings each `AddAndActivateConnection` sent, flattened
    /// (`802-1x.identity` → its value).
    added: Vec<(String, BTreeMap<String, String>)>,
    /// The VPN on, or on its way: its active connection, its saved one,
    /// its state.
    vpn_active: Vec<(String, String, u32)>,
    no_openvpn: bool,
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
        self.active
            .iter()
            .map(|(p, ..)| p.clone())
            .chain(self.vpn_active.iter().map(|(p, ..)| p.clone()))
            .collect()
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
                hidden: setup.hidden.clone(),
                no_openvpn: setup.no_openvpn,
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
            // Its name taken once every object is served, as
            // NetworkManager's: a client that reads it at once finds them.
            let connection = Builder::address(address.as_str())
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
            server.at(WIRED, Wired(served.clone())).await?;
            server.at(IP4_WIRED, Ip4Wired).await?;
            for ap in setup.aps {
                add_ap(&served, ap, false).await?;
            }
            server.at(IP4, Ip4Config).await?;
            for (ssid, psk) in setup.saved {
                let p = add_saved(&served, ssid, psk.map(str::to_owned), None, false).await?;
                let ago = setup.used.iter().find(|(s, _)| *s == ssid).map(|(_, a)| *a);
                state(&served, |s| {
                    if let Some(saved) = s.saved.iter_mut().find(|c| c.path == p) {
                        saved.timestamp = match (ago, setup.current) {
                            (_, Some(current)) if current == ssid => now(),
                            (Some(ago), _) => now().saturating_sub(ago),
                            (None, _) => 0,
                        };
                    }
                });
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
            for (name, kind, fails) in &setup.vpns {
                add_vpn(&served, name, kind, *fails, None, false).await?;
            }
            server.at(IP4_VPN, Ip4Vpn).await?;
            server.at(SETTINGS, Test(served.clone())).await?;
            connection.request_name(NM).await?;
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

    /// Changes to saved connections refused from now on, as polkit would
    /// (or no longer).
    pub fn refuse(&self, refuse: bool) {
        state(&self.shared, |s| s.refuse = refuse);
    }

    /// The saved connections' UUIDs, SSIDs and `autoconnect`.
    pub fn saved_uuids(&self) -> Vec<(String, String, bool)> {
        state(&self.shared, |s| {
            s.saved
                .iter()
                .map(|c| (c.uuid.clone(), c.ssid.clone(), c.autoconnect))
                .collect()
        })
    }

    /// The settings the last `AddAndActivateConnection` for `ssid` sent,
    /// flattened: `802-1x.eap` → `peap`.
    pub fn added(&self, ssid: &str) -> Option<BTreeMap<String, String>> {
        state(&self.shared, |s| {
            s.added
                .iter()
                .rev()
                .find(|(n, _)| n == ssid)
                .map(|(_, v)| v.clone())
        })
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
            Some((saved.path.clone(), ap.path.clone(), saved.secret.clone()))
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
            let ip4 = if wired { path(IP4_WIRED) } else { none() };
            changed(
                &shared,
                WIRED,
                DEVICE,
                vec![
                    ("State", Variant::from(new)),
                    ("Ip4Config", Variant::from(ip4)),
                ],
            )
            .await?;
            changed(
                &shared,
                WIRED,
                "org.freedesktop.NetworkManager.Device.Wired",
                vec![("Carrier", Variant::from(wired))],
            )
            .await?;
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

/// Now, in seconds since the epoch.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

async fn add_saved(
    shared: &Shared,
    ssid: &str,
    psk: Option<String>,
    uuid: Option<String>,
    announce: bool,
) -> anyhow::Result<String> {
    let p = state(shared, |s| {
        let n = s.next();
        let p = format!("/org/freedesktop/NetworkManager/Settings/{n}");
        s.saved.push(Saved {
            path: p.clone(),
            ssid: ssid.to_owned(),
            secret: psk.clone(),
            psk,
            uuid: uuid.unwrap_or_else(|| format!("{n:08x}-7e57-4000-8000-000000000000")),
            timestamp: 0,
            autoconnect: true,
            vpn: None,
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
            .at(p.as_str(), Active(shared.clone(), p.clone()))
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
    let (connectivity, actives) = state(shared, |s| (s.connectivity, s.actives()));
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
            ("ActiveConnections", Variant::from(paths(&actives))),
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
                Some(Security::Psk(want) | Security::Wep(want)) => psk.as_deref() == Some(want),
                Some(Security::Enterprise) => psk
                    .as_deref()
                    .and_then(|p| p.strip_prefix("8021x:"))
                    .is_some_and(|identity| !identity.is_empty()),
                _ => false,
            };
            (right, fake.map(|f| f.ssid).unwrap_or_default())
        });
        if right {
            let connectivity = state(&task, |s| {
                let saved = s.active.as_ref().map(|a| a.1.clone());
                if let Some(c) = s.saved.iter_mut().find(|c| Some(&c.path) == saved.as_ref()) {
                    c.timestamp = now();
                }
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
        let vpn = state(&self.0, |s| {
            let saved = s.saved.iter().find(|c| c.path == connection.as_str())?;
            let (_, fails) = saved.vpn.clone()?;
            s.calls.push(Call::Activate {
                ssid: saved.ssid.clone(),
            });
            Some(fails)
        });
        if let Some(fails) = vpn {
            return activate_vpn(self.0.clone(), connection.as_str().to_owned(), fails)
                .await
                .map(|p| path(&p))
                .map_err(failed);
        }
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
            Some((saved.path.clone(), ap, saved.secret.clone()))
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
        let flat = flatten(&settings);
        let secret = psk
            .clone()
            .or_else(|| flat.get("802-11-wireless-security.wep-key0").cloned())
            .or_else(|| flat.get("802-1x.identity").map(|i| format!("8021x:{i}")));
        state(&self.0, |s| {
            s.calls.push(Call::AddAndActivate {
                ssid: ssid.clone(),
                psk: psk.clone(),
            });
            s.added.push((ssid.clone(), flat));
        });
        // A hidden network, joined by its name: its access point appears.
        let mut ap = specific.as_str().to_owned();
        if ap == "/" {
            let (known, hidden) = state(&self.0, |s| {
                let known = s
                    .aps
                    .iter()
                    .find(|a| a.fake.ssid == ssid)
                    .map(|a| a.path.clone());
                let hidden = s
                    .hidden
                    .iter()
                    .position(|h| h.ssid == ssid)
                    .map(|at| s.hidden.remove(at));
                (known, hidden)
            });
            if let Some(known) = known {
                ap = known;
            } else if let Some(hidden) = hidden {
                ap = add_ap(&self.0, hidden, true).await.map_err(failed)?;
            }
        }
        let saved = add_saved(&self.0, &ssid, psk.clone(), None, true)
            .await
            .map_err(failed)?;
        state(&self.0, |s| {
            if let Some(c) = s.saved.iter_mut().find(|c| c.path == saved) {
                c.secret.clone_from(&secret);
            }
        });
        let active = activate(self.0.clone(), saved.clone(), ap, secret)
            .await
            .map_err(failed)?;
        Ok((path(&saved), path(&active)))
    }

    async fn deactivate_connection(&self, active: OwnedObjectPath) -> zbus::fdo::Result<()> {
        state(&self.0, |s| s.calls.push(Call::Deactivate));
        let vpn = state(&self.0, |s| {
            s.vpn_active.iter().any(|v| v.0 == active.as_str())
        });
        if vpn {
            return drop_vpn(&self.0, active.as_str(), 2).await.map_err(failed);
        }
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

    /// The Wi-Fi device's IPv4 configuration, the same whatever its
    /// network; the cable's while it is plugged.
    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        match self.2 {
            2 => path(IP4),
            _ if state(&self.0, |s| s.wired) => path(IP4_WIRED),
            _ => none(),
        }
    }

    /// Where it sits: the cable on a USB adapter.
    #[zbus(property)]
    fn path(&self) -> String {
        if self.2 == 2 {
            "pci-0000:00:14.3"
        } else {
            "pci-0000:00:14.0-usb-0:2:1.0"
        }
        .to_owned()
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

    #[zbus(property)]
    fn hw_address(&self) -> String {
        "3C:A9:F4:12:5B:7E".to_owned()
    }

    /// Kb/s.
    #[zbus(property)]
    fn bitrate(&self) -> u32 {
        866_000
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
            Some(Security::Wep(_)) => 0,
            _ => 0,
        }
    }

    #[zbus(property)]
    fn frequency(&self) -> u32 {
        self.fake().map_or(2412, |f| f.frequency)
    }
}

/// The cable's side of a wired device.
struct Wired(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wired")]
impl Wired {
    /// Mb/s.
    #[zbus(property)]
    fn speed(&self) -> u32 {
        1000
    }

    #[zbus(property)]
    fn carrier(&self) -> bool {
        state(&self.0, |s| s.wired)
    }
}

/// The cable's IPv4 configuration.
struct Ip4Wired;

#[zbus::interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4Wired {
    #[zbus(property)]
    fn address_data(&self) -> Dicts {
        vec![dict(&[("address", "192.168.1.30")])]
    }

    #[zbus(property)]
    fn gateway(&self) -> String {
        "192.168.1.1".to_owned()
    }
}

/// Settings flattened for the tests: `group.key` → its value as text
/// (bytes as text, without their trailing NUL; lists joined by commas).
fn flatten(settings: &HashMap<String, HashMap<String, OwnedVariant>>) -> BTreeMap<String, String> {
    let mut flat = BTreeMap::new();
    for (group, values) in settings {
        for (key, value) in values {
            let said = if let Ok(s) = value.downcast_ref::<String>() {
                s
            } else if let Ok(b) = value.downcast_ref::<bool>() {
                b.to_string()
            } else if let Ok(n) = value.downcast_ref::<u32>() {
                n.to_string()
            } else if let Some(bytes) = value
                .try_clone()
                .ok()
                .and_then(|v| Vec::<u8>::try_from(v).ok())
            {
                String::from_utf8_lossy(&bytes)
                    .trim_end_matches('\0')
                    .to_owned()
            } else if let Some(list) = value
                .try_clone()
                .ok()
                .and_then(|v| Vec::<String>::try_from(v).ok())
            {
                list.join(",")
            } else {
                format!("{value:?}")
            };
            flat.insert(format!("{group}.{key}"), said);
        }
    }
    flat
}

/// The Wi-Fi device's IPv4 configuration.
struct Ip4Config;

type Dicts = Vec<HashMap<String, OwnedVariant>>;

fn dict(values: &[(&str, &str)]) -> HashMap<String, OwnedVariant> {
    values
        .iter()
        .filter_map(|(k, v)| {
            Some((
                (*k).to_owned(),
                OwnedVariant::try_from(Variant::from(*v)).ok()?,
            ))
        })
        .collect()
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4Config {
    #[zbus(property)]
    fn address_data(&self) -> Dicts {
        vec![dict(&[("address", "192.168.1.24")])]
    }

    #[zbus(property)]
    fn gateway(&self) -> String {
        "192.168.1.1".to_owned()
    }

    #[zbus(property)]
    fn nameserver_data(&self) -> Dicts {
        vec![dict(&[("address", "192.168.1.1")])]
    }
}

struct Settings(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Settings {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        state(&self.0, |s| s.saved.iter().map(|c| path(&c.path)).collect())
    }

    fn get_connection_by_uuid(&self, uuid: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.uuid == uuid)
                .map(|c| path(&c.path))
        })
        .ok_or_else(|| failed("no such connection"))
    }

    async fn add_connection(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        if state(&self.0, |s| s.refuse) {
            return Err(refused());
        }
        let text_of = |group: &str, key: &str| {
            settings
                .get(group)
                .and_then(|c| c.get(key))
                .and_then(|v| v.downcast_ref::<String>().ok())
        };
        // A VPN given back.
        if let Some(kind) = text_of("connection", "type").filter(|t| t == "vpn" || t == "wireguard")
        {
            let name = text_of("connection", "id").unwrap_or_default();
            let uuid = text_of("connection", "uuid");
            let kind = if kind == "wireguard" {
                "wireguard"
            } else {
                "openvpn"
            };
            state(&self.0, |s| {
                s.calls.push(Call::AddConnection {
                    ssid: name.clone(),
                    uuid: uuid.clone().unwrap_or_default(),
                    psk: None,
                });
            });
            let p = add_vpn(&self.0, &name, kind, false, uuid, true)
                .await
                .map_err(failed)?;
            return Ok(path(&p));
        }
        let ssid = ssid_of(&settings).ok_or_else(|| failed("no SSID"))?;
        let psk = psk_of(&settings);
        let uuid = settings
            .get("connection")
            .and_then(|c| c.get("uuid"))
            .and_then(|v| v.downcast_ref::<String>().ok())
            .ok_or_else(|| failed("no UUID"))?;
        let autoconnect = settings
            .get("connection")
            .and_then(|c| c.get("autoconnect"))
            .and_then(|v| v.downcast_ref::<bool>().ok())
            .unwrap_or(true);
        let timestamp = settings
            .get("connection")
            .and_then(|c| c.get("timestamp"))
            .and_then(|v| v.downcast_ref::<u64>().ok())
            .unwrap_or(0);
        state(&self.0, |s| {
            s.calls.push(Call::AddConnection {
                ssid: ssid.clone(),
                uuid: uuid.clone(),
                psk: psk.clone(),
            });
        });
        let p = add_saved(&self.0, &ssid, psk, Some(uuid), true)
            .await
            .map_err(failed)?;
        state(&self.0, |s| {
            if let Some(saved) = s.saved.iter_mut().find(|c| c.path == p) {
                saved.autoconnect = autoconnect;
                saved.timestamp = timestamp;
            }
        });
        Ok(path(&p))
    }
}

/// What polkit says to a change it does not allow.
fn refused() -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied("Not authorized to modify system connections".to_owned())
}

struct SavedConnection(Shared, String);

#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl SavedConnection {
    fn get_settings(&self) -> HashMap<String, HashMap<String, OwnedVariant>> {
        let found = state(&self.0, |s| {
            s.saved.iter().find(|c| c.path == self.1).map(|c| {
                (
                    c.ssid.clone(),
                    c.uuid.clone(),
                    c.timestamp,
                    c.autoconnect,
                    c.psk.is_some(),
                )
            })
        });
        let (ssid, uuid, timestamp, autoconnect, protected) = found.unwrap_or_default();
        let vpn = state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.path == self.1)
                .and_then(|c| c.vpn.clone())
        });
        if let Some((kind, _)) = vpn {
            return vpn_settings(&ssid, &uuid, autoconnect, &kind);
        }
        let owned = |v: Variant<'_>| OwnedVariant::try_from(v).ok();
        let mut settings = HashMap::new();
        let mut connection = HashMap::new();
        connection.extend(owned(Variant::from(ssid.as_str())).map(|v| ("id".to_owned(), v)));
        connection.extend(owned(Variant::from(uuid.as_str())).map(|v| ("uuid".to_owned(), v)));
        connection.extend(owned(Variant::from("802-11-wireless")).map(|v| ("type".to_owned(), v)));
        connection.extend(owned(Variant::from(timestamp)).map(|v| ("timestamp".to_owned(), v)));
        connection.extend(owned(Variant::from(autoconnect)).map(|v| ("autoconnect".to_owned(), v)));
        settings.insert("connection".to_owned(), connection);
        let mut wifi = HashMap::new();
        wifi.extend(owned(Variant::from(ssid.as_bytes())).map(|v| ("ssid".to_owned(), v)));
        settings.insert("802-11-wireless".to_owned(), wifi);
        if protected {
            let mut security = HashMap::new();
            security.extend(owned(Variant::from("wpa-psk")).map(|v| ("key-mgmt".to_owned(), v)));
            settings.insert("802-11-wireless-security".to_owned(), security);
        }
        settings
    }

    fn get_secrets(
        &self,
        setting: &str,
    ) -> zbus::fdo::Result<HashMap<String, HashMap<String, OwnedVariant>>> {
        let (ssid, psk) = state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.path == self.1)
                .map(|c| (c.ssid.clone(), c.psk.clone()))
        })
        .ok_or_else(|| failed("no such connection"))?;
        state(&self.0, |s| s.calls.push(Call::GetSecrets { ssid }));
        let mut values = HashMap::new();
        if setting == "802-11-wireless-security"
            && let Some(psk) = psk
            && let Ok(v) = OwnedVariant::try_from(Variant::from(psk.as_str()))
        {
            values.insert("psk".to_owned(), v);
        }
        Ok([(setting.to_owned(), values)].into_iter().collect())
    }

    async fn update2(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
        _flags: u32,
        _args: HashMap<String, OwnedVariant>,
    ) -> zbus::fdo::Result<HashMap<String, OwnedVariant>> {
        if state(&self.0, |s| s.refuse) {
            return Err(refused());
        }
        let psk = psk_of(&settings);
        let autoconnect = settings
            .get("connection")
            .and_then(|c| c.get("autoconnect"))
            .and_then(|v| v.downcast_ref::<bool>().ok());
        let dict_of = |key: &str| {
            settings
                .get("vpn")
                .and_then(|v| v.get(key))
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| HashMap::<String, String>::try_from(v).ok())
                .unwrap_or_default()
        };
        let (data, secrets) = (dict_of("data"), dict_of("secrets"));
        let id = settings
            .get("connection")
            .and_then(|c| c.get("id"))
            .and_then(|v| v.downcast_ref::<String>().ok());
        if let Some(user) = data.get("username") {
            state(&self.0, |s| {
                let name = s
                    .saved
                    .iter()
                    .find(|c| c.path == self.1)
                    .map(|c| c.ssid.clone())
                    .unwrap_or_default();
                s.calls.push(Call::VpnCredentials {
                    name,
                    user: user.clone(),
                    password: secrets.get("password").cloned().unwrap_or_default(),
                });
            });
        }
        state(&self.0, |s| {
            if let Some(saved) = s.saved.iter_mut().find(|c| c.path == self.1) {
                // The secrets an update leaves out are kept.
                if psk.is_some() {
                    saved.psk.clone_from(&psk);
                    saved.secret.clone_from(&psk);
                }
                if let Some(on) = autoconnect {
                    saved.autoconnect = on;
                }
                // A VPN renamed.
                if saved.vpn.is_some()
                    && let Some(id) = id.clone()
                {
                    saved.ssid = id;
                }
                s.calls.push(Call::Update2 {
                    ssid: saved.ssid.clone(),
                    psk,
                    autoconnect,
                });
            }
        });
        emit(&self.0, &self.1, CONNECTION, "Updated", &())
            .await
            .map_err(failed)?;
        Ok(HashMap::new())
    }

    async fn update(
        &self,
        settings: HashMap<String, HashMap<String, OwnedVariant>>,
    ) -> zbus::fdo::Result<()> {
        let psk = psk_of(&settings);
        state(&self.0, |s| {
            if let Some(saved) = s.saved.iter_mut().find(|c| c.path == self.1) {
                saved.psk.clone_from(&psk);
                saved.secret.clone_from(&psk);
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
        if state(&self.0, |s| s.refuse) {
            return Err(refused());
        }
        // In use: left first, as NetworkManager does.
        let vpn_on = state(&self.0, |s| {
            s.vpn_active
                .iter()
                .find(|v| v.1 == self.1)
                .map(|v| v.0.clone())
        });
        if let Some(on) = vpn_on {
            drop_vpn(&self.0, &on, 11).await.map_err(failed)?;
        }
        let in_use = state(&self.0, |s| {
            s.active.as_ref().is_some_and(|a| a.1 == self.1)
        });
        if in_use {
            drop_active(&self.0, 2).await.map_err(failed)?;
            device_state(&self.0, 30, 0).await.map_err(failed)?;
        }
        state(&self.0, |s| {
            if let Some(at) = s.saved.iter().position(|c| c.path == self.1) {
                let gone = s.saved.remove(at);
                s.calls.push(Call::Delete { ssid: gone.ssid });
            }
        });
        if let Some(connection) = self.0.connection.get() {
            let _ = connection
                .object_server()
                .remove::<SavedConnection, _>(self.1.as_str())
                .await;
        }
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

struct Active(Shared, String);

impl Active {
    /// The VPN's, if it is the VPN's: its saved connection, its state.
    fn vpn(&self) -> Option<(String, u32)> {
        state(&self.0, |s| {
            s.vpn_active
                .iter()
                .find(|v| v.0 == self.1)
                .map(|v| (v.1.clone(), v.2))
        })
    }

    fn saved(&self) -> String {
        match self.vpn() {
            Some((saved, _)) => saved,
            None => state(&self.0, |s| {
                s.active.as_ref().map(|a| a.1.clone()).unwrap_or_default()
            }),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn state(&self) -> u32 {
        match self.vpn() {
            Some((_, s)) => s,
            None => state(&self.0, |s| s.active.as_ref().map_or(4, |a| a.3)),
        }
    }

    #[zbus(property)]
    fn connection(&self) -> OwnedObjectPath {
        let saved = self.saved();
        if saved.is_empty() {
            none()
        } else {
            path(&saved)
        }
    }

    #[zbus(property)]
    fn specific_object(&self) -> OwnedObjectPath {
        if self.vpn().is_some() {
            return none();
        }
        state(&self.0, |s| {
            s.active.as_ref().map_or_else(none, |a| path(&a.2))
        })
    }

    #[zbus(property)]
    fn uuid(&self) -> String {
        let saved = self.saved();
        state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.path == saved)
                .map(|c| c.uuid.clone())
                .unwrap_or_default()
        })
    }

    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        let saved = self.saved();
        state(&self.0, |s| {
            match s
                .saved
                .iter()
                .find(|c| c.path == saved)
                .and_then(|c| c.vpn.clone())
            {
                Some((kind, _)) if kind == "wireguard" => "wireguard".to_owned(),
                Some(_) => "vpn".to_owned(),
                None => "802-11-wireless".to_owned(),
            }
        })
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        match self.vpn() {
            Some((_, 2)) => path(IP4_VPN),
            Some(_) => none(),
            None => path(IP4),
        }
    }
}

/// A VPN's IPv4 configuration.
struct Ip4Vpn;

#[zbus::interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4Vpn {
    #[zbus(property)]
    fn address_data(&self) -> Dicts {
        vec![dict(&[("address", "10.8.0.6")])]
    }
}

/// The tests' own door: a VPN's file imported, as nmcli's importers would
/// (the fake `nmcli` calls it with `dbus-send`).
struct Test(Shared);

#[zbus::interface(name = "org.hocus.Test")]
impl Test {
    async fn import(&self, kind: String, file: String) -> zbus::fdo::Result<String> {
        if kind == "openvpn" && state(&self.0, |s| s.no_openvpn) {
            return Err(failed("Error: failed to find VPN plugin for openvpn."));
        }
        let name = std::path::Path::new(&file)
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = add_vpn(&self.0, &name, &kind, false, None, true)
            .await
            .map_err(failed)?;
        let uuid = state(&self.0, |s| {
            s.saved
                .iter()
                .find(|c| c.path == path)
                .map(|c| c.uuid.clone())
                .unwrap_or_default()
        });
        Ok(format!("Connection '{name}' ({uuid}) successfully added."))
    }
}

/// A VPN connection saved: its path.
async fn add_vpn(
    shared: &Shared,
    name: &str,
    kind: &str,
    fails: bool,
    uuid: Option<String>,
    announce: bool,
) -> anyhow::Result<String> {
    let p = add_saved(shared, name, None, uuid, false).await?;
    state(shared, |s| {
        if let Some(c) = s.saved.iter_mut().find(|c| c.path == p) {
            c.vpn = Some((kind.to_owned(), fails));
        }
    });
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

/// A VPN's settings, as NetworkManager's importers write them.
fn vpn_settings(
    name: &str,
    uuid: &str,
    autoconnect: bool,
    kind: &str,
) -> HashMap<String, HashMap<String, OwnedVariant>> {
    let owned = |v: Variant<'_>| OwnedVariant::try_from(v).ok();
    let wireguard = kind == "wireguard";
    let mut settings = HashMap::new();
    let mut connection = HashMap::new();
    connection.extend(owned(Variant::from(name)).map(|v| ("id".to_owned(), v)));
    connection.extend(owned(Variant::from(uuid)).map(|v| ("uuid".to_owned(), v)));
    connection.extend(
        owned(Variant::from(if wireguard { "wireguard" } else { "vpn" }))
            .map(|v| ("type".to_owned(), v)),
    );
    connection.extend(owned(Variant::from(autoconnect)).map(|v| ("autoconnect".to_owned(), v)));
    settings.insert("connection".to_owned(), connection);
    if !wireguard {
        let mut vpn = HashMap::new();
        vpn.extend(
            owned(Variant::from("org.freedesktop.NetworkManager.openvpn"))
                .map(|v| ("service-type".to_owned(), v)),
        );
        let data: HashMap<String, String> = [("remote".to_owned(), "vpn.example.org".to_owned())]
            .into_iter()
            .collect();
        vpn.extend(owned(Variant::from(data)).map(|v| ("data".to_owned(), v)));
        settings.insert("vpn".to_owned(), vpn);
    }
    settings
}

/// The VPN `saved` turned on: on its way, then on (or failed: its server
/// does not answer, reason 6).
async fn activate_vpn(shared: Shared, saved: String, fails: bool) -> anyhow::Result<String> {
    let p = state(&shared, |s| {
        let p = format!(
            "/org/freedesktop/NetworkManager/ActiveConnection/{}",
            s.next()
        );
        s.vpn_active.push((p.clone(), saved.clone(), 1));
        p
    });
    if let Some(connection) = shared.connection.get() {
        connection
            .object_server()
            .at(p.as_str(), Active(shared.clone(), p.clone()))
            .await?;
    }
    let actives = state(&shared, |s| s.actives());
    changed(
        &shared,
        ROOT,
        NM,
        vec![("ActiveConnections", Variant::from(paths(&actives)))],
    )
    .await?;
    let task = shared.clone();
    let active = p.clone();
    let takes = state(&shared, |s| s.takes);
    tokio::spawn(async move {
        tokio::time::sleep(takes).await;
        if fails {
            let _ = drop_vpn(&task, &active, 6).await;
            return;
        }
        state(&task, |s| {
            if let Some(v) = s.vpn_active.iter_mut().find(|v| v.0 == active) {
                v.2 = 2;
            }
        });
        let _ = changed(
            &task,
            &active,
            ACTIVE,
            vec![
                ("State", Variant::from(2u32)),
                ("Ip4Config", Variant::from(path(IP4_VPN))),
            ],
        )
        .await;
        let _ = emit(&task, &active, ACTIVE, "StateChanged", &(2u32, 0u32)).await;
    });
    Ok(p)
}

/// The VPN on at `active` off, for `reason`.
async fn drop_vpn(shared: &Shared, active: &str, reason: u32) -> anyhow::Result<()> {
    let gone = state(shared, |s| {
        let at = s.vpn_active.iter().position(|v| v.0 == active)?;
        Some(s.vpn_active.remove(at))
    });
    if let Some((p, ..)) = gone {
        emit(shared, &p, ACTIVE, "StateChanged", &(4u32, reason)).await?;
        if let Some(connection) = shared.connection.get() {
            connection
                .object_server()
                .remove::<Active, _>(p.as_str())
                .await?;
        }
    }
    let actives = state(shared, |s| s.actives());
    changed(
        shared,
        ROOT,
        NM,
        vec![("ActiveConnections", Variant::from(paths(&actives)))],
    )
    .await
}
