//! Bluetooth devices, through BlueZ on the system bus: the paired ones
//! and those in reach while discovery runs, with their kind and battery;
//! paired, connected, disconnected and forgotten. The radio itself is
//! switched as quick settings switch it ([`crate::system`]).
//!
//! Pairing registers an agent with BlueZ for that pairing only. A device
//! that shows a passkey to compare is asked about ([`Ask::Confirm`]); one
//! that shows a code to type on it says it ([`Ask::Show`]); one that asks
//! for a code typed here is refused: that is not offered yet.
//!
//! Everything here blocks: for a worker thread, never a loop.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, interface};

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
/// Where the pairing agent is served, on this connection.
const AGENT_PATH: &str = "/org/hocuspanda/bluetooth/agent";

/// What a device is, from the icon BlueZ gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Headphones,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Other,
}

impl Kind {
    /// From BlueZ's `Icon` (freedesktop icon names).
    pub fn from_icon(icon: &str) -> Self {
        match icon {
            "audio-headset" | "audio-headphones" => Self::Headphones,
            "audio-card" | "audio-speakers" | "multimedia-player" => Self::Speaker,
            "input-keyboard" => Self::Keyboard,
            "input-mouse" | "input-tablet" => Self::Mouse,
            "input-gaming" => Self::Gamepad,
            "phone" => Self::Phone,
            "computer" => Self::Computer,
            _ => Self::Other,
        }
    }
}

/// A device BlueZ knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Its object, which names it to the other calls.
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    /// Its battery, 0 to 100, when it says.
    pub battery: Option<u8>,
}

/// What BlueZ says of Bluetooth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bluetooth {
    /// The radio is off.
    Off,
    /// No adapter.
    NoAdapter,
    /// The paired devices (the connected first), then those in reach, by
    /// name.
    Devices(Vec<Device>),
}

/// What a pairing asks of whoever is at the screen.
#[derive(Debug)]
pub enum Ask {
    /// The device shows this passkey: is it the same?
    Confirm { passkey: u32, answer: Answer },
    /// This code is typed on the device.
    Show { code: String },
    /// The pairing ended: whatever was asked no longer holds.
    Over,
}

#[derive(Debug, Default)]
struct Slot {
    answer: Option<bool>,
    waker: Option<Waker>,
}

fn settle(slot: &Arc<Mutex<Slot>>, yes: bool) {
    if let Ok(mut slot) = slot.lock()
        && slot.answer.is_none()
    {
        slot.answer = Some(yes);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// The answer to [`Ask::Confirm`]; dropped unanswered, it is no.
#[derive(Debug)]
pub struct Answer(Option<Arc<Mutex<Slot>>>);

impl Answer {
    pub fn give(mut self, yes: bool) {
        if let Some(slot) = self.0.take() {
            settle(&slot, yes);
        }
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(slot) = self.0.take() {
            settle(&slot, false);
        }
    }
}

/// Waits for an [`Answer`] without holding a thread.
struct Waiting(Arc<Mutex<Slot>>);

impl Future for Waiting {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let Ok(mut slot) = self.0.lock() else {
            return Poll::Ready(false);
        };
        match slot.answer {
            Some(yes) => Poll::Ready(yes),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
}

type Asks = Arc<dyn Fn(Ask) + Send + Sync>;

/// The agent BlueZ asks while a pairing started here runs.
struct Agent {
    asks: Asks,
    /// The question waiting, which BlueZ may cancel.
    waiting: Mutex<Option<Arc<Mutex<Slot>>>>,
}

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_confirmation(
        &self,
        _device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        let slot = Arc::new(Mutex::new(Slot::default()));
        if let Ok(mut waiting) = self.waiting.lock() {
            *waiting = Some(slot.clone());
        }
        (self.asks)(Ask::Confirm {
            passkey,
            answer: Answer(Some(slot.clone())),
        });
        if Waiting(slot).await {
            Ok(())
        } else {
            Err(AgentError::Rejected("not confirmed".into()))
        }
    }

    /// Pairing without a code ("just works"), started here by a click.
    fn request_authorization(&self, _device: OwnedObjectPath) {}

    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}

    /// A code typed here: not offered yet.
    fn request_pin_code(&self, _device: OwnedObjectPath) -> Result<String, AgentError> {
        Err(AgentError::Rejected(
            "a code typed here is not offered".into(),
        ))
    }

    fn request_passkey(&self, _device: OwnedObjectPath) -> Result<u32, AgentError> {
        Err(AgentError::Rejected(
            "a code typed here is not offered".into(),
        ))
    }

    fn display_passkey(&self, _device: OwnedObjectPath, passkey: u32, _entered: u16) {
        (self.asks)(Ask::Show {
            code: format!("{passkey:06}"),
        });
    }

    fn display_pin_code(&self, _device: OwnedObjectPath, pincode: String) {
        (self.asks)(Ask::Show { code: pincode });
    }

    fn cancel(&self) {
        if let Ok(mut waiting) = self.waiting.lock()
            && let Some(slot) = waiting.take()
        {
            settle(&slot, false);
        }
        (self.asks)(Ask::Over);
    }
}

/// BlueZ's first adapter on a system bus.
#[derive(Debug, Clone)]
pub struct Bluez {
    conn: Connection,
    adapter: OwnedObjectPath,
}

type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>>;

fn said(err: zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(name, message, _) => {
            message.unwrap_or_else(|| name.as_str().to_owned())
        }
        other => other.to_string(),
    }
}

fn get<T>(props: &HashMap<String, OwnedVariant>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedVariant>,
{
    T::try_from(props.get(key)?.try_clone().ok()?).ok()
}

async fn proxy(
    conn: &Connection,
    at: impl Into<OwnedObjectPath>,
    interface: &'static str,
) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(BLUEZ)
        .path(at.into())
        .interface(interface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

async fn objects(conn: &Connection) -> zbus::Result<Objects> {
    let root = OwnedObjectPath::try_from("/")?;
    let manager = proxy(conn, root, "org.freedesktop.DBus.ObjectManager").await?;
    manager.call("GetManagedObjects", &()).await
}

fn object(path: &str) -> Result<OwnedObjectPath, String> {
    OwnedObjectPath::try_from(path).map_err(|e| e.to_string())
}

impl Bluez {
    /// Connects to the bus at `system_bus` (the machine's when `None`) and
    /// finds the first adapter; fails when BlueZ is not on it.
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
            let mut adapters: Vec<OwnedObjectPath> = objects(&conn)
                .await?
                .into_iter()
                .filter(|(_, interfaces)| interfaces.contains_key(ADAPTER))
                .map(|(path, _)| path)
                .collect();
            adapters.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            let adapter = adapters.into_iter().next();
            zbus::Result::Ok(adapter.map(|adapter| Self { conn, adapter }))
        })
        .map_err(said)?
        .ok_or_else(|| "no Bluetooth adapter".to_owned())
    }

    /// The radio, and the devices.
    pub fn read(&self) -> Result<Bluetooth, String> {
        zbus::block_on(async {
            let all = objects(&self.conn).await?;
            let Some(adapter) = all.get(&self.adapter).and_then(|i| i.get(ADAPTER)) else {
                return Ok(Bluetooth::NoAdapter);
            };
            if !get::<bool>(adapter, "Powered").unwrap_or(false) {
                return Ok(Bluetooth::Off);
            }
            let mut devices = Vec::new();
            for (path, interfaces) in &all {
                let Some(d) = interfaces.get(DEVICE) else {
                    continue;
                };
                if get::<OwnedObjectPath>(d, "Adapter").as_ref() != Some(&self.adapter) {
                    continue;
                }
                let paired = get::<bool>(d, "Paired").unwrap_or(false);
                // A device in reach without a name is not one to pair.
                let named = d.contains_key("Name");
                if !paired && !named {
                    continue;
                }
                let name = get::<String>(d, "Alias")
                    .or_else(|| get::<String>(d, "Name"))
                    .unwrap_or_default();
                let icon = get::<String>(d, "Icon").unwrap_or_default();
                devices.push(Device {
                    path: path.as_str().to_owned(),
                    name,
                    kind: Kind::from_icon(&icon),
                    paired,
                    connected: get::<bool>(d, "Connected").unwrap_or(false),
                    battery: interfaces
                        .get(BATTERY)
                        .and_then(|b| get::<u8>(b, "Percentage"))
                        .map(|p| p.min(100)),
                });
            }
            sort(&mut devices);
            zbus::Result::Ok(Bluetooth::Devices(devices))
        })
        .map_err(said)
    }

    /// Looks for devices in reach, or stops.
    pub fn discover(&self, on: bool) -> Result<(), String> {
        zbus::block_on(async {
            let adapter = proxy(&self.conn, self.adapter.clone(), ADAPTER).await?;
            let method = if on {
                "StartDiscovery"
            } else {
                "StopDiscovery"
            };
            adapter.call::<_, _, ()>(method, &()).await
        })
        .map_err(said)
    }

    /// Pairs with `device`, asking through `asks` what has to be; then
    /// trusts it, so that it comes back by itself, and connects it.
    pub fn pair(
        &self,
        device: &str,
        asks: impl Fn(Ask) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let device = object(device)?;
        let asks: Asks = Arc::new(asks);
        let result = zbus::block_on(async {
            let server = self.conn.object_server();
            let agent = Agent {
                asks: asks.clone(),
                waiting: Mutex::new(None),
            };
            // One left from a pairing that went wrong goes first.
            server.remove::<Agent, _>(AGENT_PATH).await.ok();
            server.at(AGENT_PATH, agent).await?;
            let agent_path = OwnedObjectPath::try_from(AGENT_PATH)?;
            let root = OwnedObjectPath::try_from("/org/bluez")?;
            let manager = proxy(&self.conn, root, AGENT_MANAGER).await?;
            let paired = match manager
                .call::<_, _, ()>("RegisterAgent", &(agent_path.clone(), "DisplayYesNo"))
                .await
            {
                Ok(()) => {
                    let d = proxy(&self.conn, device.clone(), DEVICE).await?;
                    let paired = d.call::<_, _, ()>("Pair", &()).await;
                    if let Err(err) = manager
                        .call::<_, _, ()>("UnregisterAgent", &(agent_path,))
                        .await
                    {
                        tracing::debug!(%err, "the pairing agent stays registered");
                    }
                    paired
                }
                Err(err) => Err(err),
            };
            server.remove::<Agent, _>(AGENT_PATH).await.ok();
            paired?;
            let d = proxy(&self.conn, device.clone(), DEVICE).await?;
            d.set_property("Trusted", true).await?;
            // Some connect by themselves once paired.
            if let Err(err) = d.call::<_, _, ()>("Connect", &()).await {
                tracing::debug!(%err, "the device paired but did not connect");
            }
            zbus::Result::Ok(())
        });
        asks(Ask::Over);
        result.map_err(said)
    }

    /// Connects a paired device, or disconnects it.
    pub fn connect_device(&self, device: &str, on: bool) -> Result<(), String> {
        let device = object(device)?;
        zbus::block_on(async {
            let d = proxy(&self.conn, device, DEVICE).await?;
            let method = if on { "Connect" } else { "Disconnect" };
            d.call::<_, _, ()>(method, &()).await
        })
        .map_err(said)
    }

    /// Forgets a device: unpaired, and gone from the list until in reach
    /// again.
    pub fn forget(&self, device: &str) -> Result<(), String> {
        let device = object(device)?;
        zbus::block_on(async {
            let adapter = proxy(&self.conn, self.adapter.clone(), ADAPTER).await?;
            adapter.call::<_, _, ()>("RemoveDevice", &(device,)).await
        })
        .map_err(said)
    }
}

/// Paired first, the connected among them first; then by name.
fn sort(devices: &mut [Device]) {
    devices.sort_by(|a, b| {
        b.paired
            .cmp(&a.paired)
            .then(b.connected.cmp(&a.connected))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_the_icon() {
        assert_eq!(Kind::from_icon("audio-headset"), Kind::Headphones);
        assert_eq!(Kind::from_icon("input-keyboard"), Kind::Keyboard);
        assert_eq!(Kind::from_icon("something-new"), Kind::Other);
    }

    #[test]
    fn an_answer_dropped_is_no() {
        let slot = Arc::new(Mutex::new(Slot::default()));
        drop(Answer(Some(slot.clone())));
        assert_eq!(slot.lock().map(|s| s.answer).ok().flatten(), Some(false));
        let slot = Arc::new(Mutex::new(Slot::default()));
        Answer(Some(slot.clone())).give(true);
        assert!(zbus::block_on(Waiting(slot)));
    }

    #[test]
    fn paired_and_connected_come_first() {
        let d = |name: &str, paired, connected| Device {
            path: format!("/{name}"),
            name: name.into(),
            kind: Kind::Other,
            paired,
            connected,
            battery: None,
        };
        let mut list = vec![
            d("a", false, false),
            d("b", true, false),
            d("c", true, true),
        ];
        sort(&mut list);
        let names: Vec<_> = list.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["c", "b", "a"]);
    }
}
