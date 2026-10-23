//! The Wi-Fi networks in reach, through NetworkManager on the system bus:
//! scanned for, listed with their strength and security, joined (a
//! password, when one is asked, goes to NetworkManager and nowhere else),
//! left, and forgotten. The radio itself is switched as quick settings
//! switch it ([`crate::system`]).
//!
//! Everything here blocks: for a worker thread, never a loop.

use std::{collections::HashMap, time::Duration};

use zbus::{
    Connection, OwnedObjectPath, OwnedVariant as OwnedValue, Proxy, Variant as Value,
    fdo::PropertiesProxy, names::InterfaceName,
};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";

/// `NM_DEVICE_TYPE_WIFI`.
const TYPE_WIFI: u32 = 2;
/// `NM_DEVICE_STATE_ACTIVATED`.
const DEVICE_ACTIVATED: u32 = 100;
/// `NM_ACTIVE_CONNECTION_STATE_ACTIVATED` and `…_DEACTIVATED`.
const ACTIVATED: u32 = 2;
const DEACTIVATED: u32 = 4;
/// `NM_802_11_AP_FLAGS_PRIVACY`.
const PRIVACY: u32 = 0x1;
/// `NM_802_11_AP_SEC_KEY_MGMT_*`.
const KEY_PSK: u32 = 0x100;
const KEY_8021X: u32 = 0x200;
const KEY_SAE: u32 = 0x400;
const KEY_OWE: u32 = 0x800;
const KEY_SUITE_B: u32 = 0x2000;
/// How long a network takes to join before it is given up.
const JOIN_TIMEOUT: Duration = Duration::from_secs(40);
const JOIN_POLL: Duration = Duration::from_millis(200);

/// How a network is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// No password (or Enhanced Open, encrypted without one).
    Open,
    /// A password: WPA or WPA2 Personal.
    Personal,
    /// A password: WPA3 Personal only (SAE).
    Wpa3,
    /// An account (802.1X, Enterprise) or WEP: joined with the system's
    /// own tools, not here.
    Other,
}

impl Security {
    /// From an access point's flags.
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        let keys = wpa | rsn;
        if keys & (KEY_8021X | KEY_SUITE_B) != 0 {
            Self::Other
        } else if keys & KEY_PSK != 0 {
            Self::Personal
        } else if keys & KEY_SAE != 0 {
            Self::Wpa3
        } else if keys & KEY_OWE != 0 || flags & PRIVACY == 0 {
            Self::Open
        } else {
            // Privacy without WPA: WEP.
            Self::Other
        }
    }

    /// A password is asked to join it.
    pub fn needs_password(self) -> bool {
        matches!(self, Self::Personal | Self::Wpa3)
    }
}

/// A network in reach, by its name: the strongest of its access points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    /// 0 to 100.
    pub strength: u8,
    pub security: Security,
    /// Joined now.
    pub connected: bool,
    /// NetworkManager keeps a connection for it: joined again without a
    /// password, and forgotten here.
    pub saved: bool,
}

/// What NetworkManager says of Wi-Fi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wifi {
    /// The radio is off.
    Off,
    /// No Wi-Fi device.
    NoDevice,
    /// The networks in reach: the one joined first, then the strongest.
    Networks(Vec<Network>),
}

/// Why a network was not joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinError {
    /// It asked for a password that was not given.
    NeedsPassword,
    /// It was tried and not joined: the password, most likely, when one
    /// was given.
    Failed,
    /// NetworkManager refused, and said why.
    Refused(String),
}

/// NetworkManager on a system bus.
#[derive(Debug, Clone)]
pub struct NetworkManager {
    conn: Connection,
}

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap_or_else(|_| unreachable!("{p}"))
}

fn said(err: zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(name, message, _) => {
            message.unwrap_or_else(|| name.as_str().to_owned())
        }
        other => other.to_string(),
    }
}

async fn proxy(
    conn: &Connection,
    at: impl Into<OwnedObjectPath>,
    interface: &'static str,
) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(NM)
        .path(at.into())
        .interface(interface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// Every property of `interface` at `at`, in one call.
async fn properties(
    conn: &Connection,
    at: &OwnedObjectPath,
    interface: &'static str,
) -> zbus::Result<HashMap<String, OwnedValue>> {
    let props = PropertiesProxy::builder(conn)
        .destination(NM)
        .path(at.clone())
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let name = InterfaceName::try_from(interface)?;
    Ok(props.get_all(name).await?)
}

fn get<T>(props: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    T::try_from(props.get(key)?.try_clone().ok()?).ok()
}

/// A connection's SSID, when it is a Wi-Fi one.
fn ssid_of(settings: &HashMap<String, HashMap<String, OwnedValue>>) -> Option<String> {
    let wireless = settings.get("802-11-wireless")?;
    let bytes: Vec<u8> = get(wireless, "ssid")?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

impl NetworkManager {
    /// Connects to the bus at `system_bus` (the machine's when `None`);
    /// fails when NetworkManager is not on it.
    pub fn connect(system_bus: &Option<String>) -> Result<Self, String> {
        zbus::block_on(async {
            let conn = match system_bus {
                Some(address) => {
                    zbus::connection::Builder::address(address.as_str())
                        .build()
                        .await?
                }
                None => Connection::system().await?,
            };
            let nm = proxy(&conn, path(NM_PATH), NM).await?;
            nm.get_property::<bool>("WirelessEnabled").await?;
            Ok(Self { conn })
        })
        .map_err(said)
    }

    /// The Wi-Fi devices.
    async fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>> {
        let nm = proxy(&self.conn, path(NM_PATH), NM).await?;
        let all: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).await?;
        let mut wifi = Vec::new();
        for device in all {
            let props = properties(&self.conn, &device, DEVICE).await?;
            if get::<u32>(&props, "DeviceType") == Some(TYPE_WIFI) {
                wifi.push(device);
            }
        }
        Ok(wifi)
    }

    /// The saved Wi-Fi connections, by SSID.
    async fn saved(&self) -> zbus::Result<Vec<(String, OwnedObjectPath)>> {
        let settings = proxy(&self.conn, path(SETTINGS_PATH), SETTINGS).await?;
        let all: Vec<OwnedObjectPath> = settings.call("ListConnections", &()).await?;
        let mut saved = Vec::new();
        for connection in all {
            let c = proxy(&self.conn, connection.clone(), CONNECTION).await?;
            // Secrets are not asked for: only what names the network.
            let Ok(s) = c
                .call::<_, _, HashMap<String, HashMap<String, OwnedValue>>>("GetSettings", &())
                .await
            else {
                continue;
            };
            if let Some(ssid) = ssid_of(&s) {
                saved.push((ssid, connection));
            }
        }
        Ok(saved)
    }

    /// Asks every Wi-Fi device to look for networks; the list is read
    /// later. NetworkManager refuses scans too close together: that is
    /// not an error here.
    pub fn scan(&self) {
        let result = zbus::block_on(async {
            for device in self.devices().await? {
                let wireless = proxy(&self.conn, device, WIRELESS).await?;
                let options: HashMap<&str, Value<'_>> = HashMap::new();
                if let Err(err) = wireless.call::<_, _, ()>("RequestScan", &(options,)).await {
                    tracing::debug!(%err, "no scan now");
                }
            }
            zbus::Result::Ok(())
        });
        if let Err(err) = result {
            tracing::debug!(%err, "could not scan for networks");
        }
    }

    /// The radio, and the networks the first Wi-Fi device sees.
    pub fn read(&self) -> Result<Wifi, String> {
        zbus::block_on(async {
            let nm = proxy(&self.conn, path(NM_PATH), NM).await?;
            if !nm.get_property::<bool>("WirelessEnabled").await? {
                return Ok(Wifi::Off);
            }
            let Some(device) = self.devices().await?.into_iter().next() else {
                return Ok(Wifi::NoDevice);
            };
            let state: u32 =
                get(&properties(&self.conn, &device, DEVICE).await?, "State").unwrap_or_default();
            let wireless = properties(&self.conn, &device, WIRELESS).await?;
            let points: Vec<OwnedObjectPath> = get(&wireless, "AccessPoints").unwrap_or_default();
            let active: Option<OwnedObjectPath> = get(&wireless, "ActiveAccessPoint");
            let saved = self.saved().await.unwrap_or_default();
            let mut networks: Vec<Network> = Vec::new();
            for point in points {
                // An access point may go between the list and its read.
                let Ok(p) = properties(&self.conn, &point, ACCESS_POINT).await else {
                    continue;
                };
                let ssid: Vec<u8> = get(&p, "Ssid").unwrap_or_default();
                let ssid = String::from_utf8_lossy(&ssid).trim().to_owned();
                // Hidden networks have no name to show.
                if ssid.is_empty() {
                    continue;
                }
                let flag = |key| get::<u32>(&p, key).unwrap_or_default();
                let network = Network {
                    connected: state == DEVICE_ACTIVATED && active.as_ref() == Some(&point),
                    saved: saved.iter().any(|(s, _)| *s == ssid),
                    strength: get::<u8>(&p, "Strength").unwrap_or_default().min(100),
                    security: Security::from_flags(
                        flag("Flags"),
                        flag("WpaFlags"),
                        flag("RsnFlags"),
                    ),
                    ssid,
                };
                match networks.iter_mut().find(|n| n.ssid == network.ssid) {
                    Some(seen) => {
                        seen.connected |= network.connected;
                        if network.strength > seen.strength {
                            seen.strength = network.strength;
                            seen.security = network.security;
                        }
                    }
                    None => networks.push(network),
                }
            }
            sort(&mut networks);
            zbus::Result::Ok(Wifi::Networks(networks))
        })
        .map_err(said)
    }

    /// Joins `ssid`: through the connection kept for it, or a new one with
    /// `password`, which goes to NetworkManager and nowhere else. Waits
    /// until it is joined or given up; a new connection that did not join
    /// is not kept.
    pub fn join(&self, ssid: &str, password: Option<&str>) -> Result<(), JoinError> {
        zbus::block_on(async {
            let refused = |err: zbus::Error| JoinError::Refused(said(err));
            let device = self
                .devices()
                .await
                .map_err(refused)?
                .into_iter()
                .next()
                .ok_or_else(|| JoinError::Refused("no Wi-Fi device".into()))?;
            let point = self.access_point(&device, ssid).await.map_err(refused)?;
            let saved = self.saved().await.map_err(refused)?;
            let nm = proxy(&self.conn, path(NM_PATH), NM)
                .await
                .map_err(refused)?;
            let none = path("/");
            let kept = saved
                .iter()
                .find(|(s, _)| s == ssid)
                .map(|(_, c)| c.clone());
            let (active, added) = match (kept, password) {
                (Some(connection), None) => {
                    let active: OwnedObjectPath = nm
                        .call(
                            "ActivateConnection",
                            &(connection, device.clone(), point.clone().unwrap_or(none)),
                        )
                        .await
                        .map_err(refused)?;
                    (active, None)
                }
                (kept, password) => {
                    let security = match &point {
                        Some(p) => {
                            let props = properties(&self.conn, p, ACCESS_POINT)
                                .await
                                .map_err(refused)?;
                            let flag = |key| get::<u32>(&props, key).unwrap_or_default();
                            Security::from_flags(flag("Flags"), flag("WpaFlags"), flag("RsnFlags"))
                        }
                        None => Security::Open,
                    };
                    if security.needs_password() && password.is_none() {
                        return Err(JoinError::NeedsPassword);
                    }
                    // A password typed again replaces the one kept.
                    if let Some(old) = kept {
                        let c = proxy(&self.conn, old, CONNECTION).await.map_err(refused)?;
                        c.call::<_, _, ()>("Delete", &()).await.map_err(refused)?;
                    }
                    let mut settings: HashMap<&str, HashMap<&str, Value<'_>>> = HashMap::new();
                    if let (true, Some(password)) = (security.needs_password(), password) {
                        let key = if security == Security::Wpa3 {
                            "sae"
                        } else {
                            "wpa-psk"
                        };
                        settings.insert(
                            "802-11-wireless-security",
                            HashMap::from([
                                ("key-mgmt", Value::from(key)),
                                ("psk", Value::from(password)),
                            ]),
                        );
                    }
                    let (connection, active): (OwnedObjectPath, OwnedObjectPath) = nm
                        .call(
                            "AddAndActivateConnection",
                            &(settings, device.clone(), point.clone().unwrap_or(none)),
                        )
                        .await
                        .map_err(refused)?;
                    (active, Some(connection))
                }
            };
            if self.joined(&active).await {
                return Ok(());
            }
            if let Some(connection) = added
                && let Ok(c) = proxy(&self.conn, connection, CONNECTION).await
                && let Err(err) = c.call::<_, _, ()>("Delete", &()).await
            {
                tracing::debug!(%err, "the connection that did not join stays");
            }
            Err(JoinError::Failed)
        })
    }

    /// The access point of `ssid` the device sees, the strongest.
    async fn access_point(
        &self,
        device: &OwnedObjectPath,
        ssid: &str,
    ) -> zbus::Result<Option<OwnedObjectPath>> {
        let wireless = properties(&self.conn, device, WIRELESS).await?;
        let points: Vec<OwnedObjectPath> = get(&wireless, "AccessPoints").unwrap_or_default();
        let mut best: Option<(u8, OwnedObjectPath)> = None;
        for point in points {
            let Ok(p) = properties(&self.conn, &point, ACCESS_POINT).await else {
                continue;
            };
            let name: Vec<u8> = get(&p, "Ssid").unwrap_or_default();
            let strength: u8 = get(&p, "Strength").unwrap_or_default();
            if String::from_utf8_lossy(&name).trim() == ssid
                && best.as_ref().is_none_or(|(s, _)| strength > *s)
            {
                best = Some((strength, point));
            }
        }
        Ok(best.map(|(_, p)| p))
    }

    /// Waits until the active connection is up, or down for good.
    async fn joined(&self, active: &OwnedObjectPath) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < JOIN_TIMEOUT {
            let state = match proxy(&self.conn, active.clone(), ACTIVE).await {
                Ok(p) => p.get_property::<u32>("State").await,
                Err(err) => Err(err),
            };
            match state {
                Ok(ACTIVATED) => return true,
                // Gone or down: not joined.
                Ok(DEACTIVATED) | Err(_) => return false,
                Ok(_) => {}
            }
            // On the worker's own thread: nothing else waits on it.
            std::thread::sleep(JOIN_POLL);
        }
        false
    }

    /// Leaves the network joined.
    pub fn disconnect(&self) -> Result<(), String> {
        zbus::block_on(async {
            for device in self.devices().await? {
                let d = proxy(&self.conn, device, DEVICE).await?;
                d.call::<_, _, ()>("Disconnect", &()).await?;
            }
            zbus::Result::Ok(())
        })
        .map_err(said)
    }

    /// Forgets every connection kept for `ssid`.
    pub fn forget(&self, ssid: &str) -> Result<(), String> {
        zbus::block_on(async {
            for (name, connection) in self.saved().await? {
                if name == ssid {
                    let c = proxy(&self.conn, connection, CONNECTION).await?;
                    c.call::<_, _, ()>("Delete", &()).await?;
                }
            }
            zbus::Result::Ok(())
        })
        .map_err(said)
    }
}

/// The network joined first, then the strongest, then by name.
fn sort(networks: &mut [Network]) {
    networks.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_is_read_from_the_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(1, 0, KEY_PSK), Security::Personal);
        assert_eq!(
            Security::from_flags(1, 0, KEY_PSK | KEY_SAE),
            Security::Personal,
            "transition mode joins with WPA2"
        );
        assert_eq!(Security::from_flags(1, 0, KEY_SAE), Security::Wpa3);
        assert_eq!(
            Security::from_flags(1, KEY_8021X, KEY_8021X),
            Security::Other
        );
        assert_eq!(Security::from_flags(1, 0, 0), Security::Other, "WEP");
        assert_eq!(Security::from_flags(0, 0, KEY_OWE), Security::Open);
    }

    #[test]
    fn the_network_joined_comes_first_then_the_strongest() {
        let n = |ssid: &str, strength, connected| Network {
            ssid: ssid.into(),
            strength,
            security: Security::Open,
            connected,
            saved: false,
        };
        let mut list = vec![n("b", 40, false), n("a", 90, false), n("c", 20, true)];
        sort(&mut list);
        let names: Vec<_> = list.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }
}
