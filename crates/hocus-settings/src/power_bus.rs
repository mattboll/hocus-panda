//! The Énergie page's services (6.10), on the system bus, on a thread of
//! its own with its runtime, never the window's: UPower's battery (its
//! display device, the battery's health, whether on battery, whether the
//! computer has a lid), power-profiles-daemon's modes, and whether logind
//! is there (the compositor takes the lid and the power button from it).
//!
//! Each is read as the thread starts and again when its properties change
//! or its owner does (no polling); the window is told only what changed.
//! The power mode is set as the window asks. The bus is the one
//! `DBUS_SYSTEM_BUS_ADDRESS` names, else the system's.

use std::{collections::HashMap, time::Duration};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{Connection, MatchRule, MessageStream, OwnedVariant, fdo::DBusProxy};

const UPOWER: &str = "org.freedesktop.UPower";
const ROOT: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const LOGIN: &str = "org.freedesktop.login1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// UPower's `Type` of a laptop's battery.
const BATTERY_TYPE: u32 = 2;

type Properties = HashMap<String, OwnedVariant>;

/// How the battery charges, as UPower's `State` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    /// Plugged in, not charging.
    Held,
    Unknown,
}

impl Charge {
    fn of(state: u32, on_battery: bool) -> Self {
        match (state, on_battery) {
            (1, _) => Self::Charging,
            (2 | 3 | 6, true) => Self::Discharging,
            (4, _) => Self::Full,
            (5 | 2 | 3 | 6, false) => Self::Held,
            (_, true) => Self::Discharging,
            _ => Self::Unknown,
        }
    }
}

/// The battery, as UPower's display device says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// Its level, in whole percent.
    pub percent: u8,
    pub charge: Charge,
    /// Until empty (discharging) or full (charging), when UPower knows.
    pub left: Option<Duration>,
    /// What it holds of its original capacity, in percent, when known.
    pub health: Option<u8>,
}

/// power-profiles-daemon's modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modes {
    pub active: String,
    /// The profiles the machine has.
    pub available: Vec<String>,
    /// `PerformanceDegraded`: why performance is held back, or empty.
    pub degraded: String,
}

/// What the services say now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Now {
    /// UPower answers.
    pub upower: bool,
    pub battery: Option<Battery>,
    pub on_battery: bool,
    /// UPower's `LidIsPresent`.
    pub lid: bool,
    /// `None`: no power-profiles-daemon.
    pub modes: Option<Modes>,
    /// logind is on the bus.
    pub logind: bool,
    /// The bus answered at all (else nothing is known).
    pub bus: bool,
}

/// What the window asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// power-profiles-daemon's mode set so.
    Mode(String),
}

/// The thread's end the window keeps.
#[derive(Debug, Clone)]
pub struct Bus {
    asks: UnboundedSender<Ask>,
}

impl Bus {
    pub fn ask(&self, ask: Ask) {
        if self.asks.send(ask).is_err() {
            tracing::warn!("the power services' thread has stopped");
        }
    }
}

/// Starts the thread; `heard` takes each change, and returns `false` once
/// the window is gone.
pub fn start(heard: impl FnMut(Now) -> bool + Send + 'static) -> Bus {
    let (asks, asked) = unbounded_channel();
    let started = std::thread::Builder::new()
        .name("power services".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(asked, heard)),
                Err(err) => tracing::warn!(%err, "no runtime for the power services"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "the power services are not read");
    }
    Bus { asks }
}

/// What wakes the thread.
enum Wake {
    Changed,
    Ask(Ask),
}

async fn run(mut asked: UnboundedReceiver<Ask>, mut heard: impl FnMut(Now) -> bool) {
    let connection = match zbus::connection::Builder::system().build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::info!(%err, "no system bus: no battery, no modes, no logind");
            let _ = heard(Now::default());
            // Asked for a mode, nothing to set: wait for the window to go.
            while asked.recv().await.is_some() {}
            return;
        }
    };
    let (wake, mut woken) = unbounded_channel::<Wake>();
    if let Err(err) = listen(&connection, wake.clone()).await {
        tracing::warn!(%err, "the power services' changes are not followed");
    }
    let asks = wake;
    tokio::spawn(async move {
        while let Some(ask) = asked.recv().await {
            if asks.send(Wake::Ask(ask)).is_err() {
                return;
            }
        }
    });
    let mut said: Option<Now> = None;
    loop {
        let now = read(&connection).await;
        if said.as_ref() != Some(&now) {
            tracing::debug!(?now, "the power services");
            if !heard(now.clone()) {
                return;
            }
            said = Some(now);
        }
        let Some(first) = woken.recv().await else {
            return;
        };
        let mut wakes = vec![first];
        while let Ok(more) = woken.try_recv() {
            wakes.push(more);
        }
        for wake in wakes {
            if let Wake::Ask(Ask::Mode(mode)) = wake {
                set_mode(&connection, &mode).await;
            }
        }
    }
}

/// Every change of UPower's, power-profiles-daemon's and logind's
/// presence wakes the thread.
async fn listen(connection: &Connection, wake: UnboundedSender<Wake>) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace(ROOT)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .build()?;
    let changes = MessageStream::for_match_rule(rule, connection, None).await?;
    let proxy = DBusProxy::new(connection).await?;
    let mut owners = Vec::new();
    for name in [UPOWER, PROFILES, LOGIN] {
        owners.push(proxy.receive_name_owner_changed_with_args(&[(0, name)]).await?);
    }
    let on_change = wake.clone();
    tokio::spawn(async move {
        let mut changes = std::pin::pin!(changes);
        while next(&mut changes).await.is_some() {
            if on_change.send(Wake::Changed).is_err() {
                return;
            }
        }
    });
    for owner in owners {
        let wake = wake.clone();
        tokio::spawn(async move {
            let mut owner = std::pin::pin!(owner);
            while next(&mut owner).await.is_some() {
                if wake.send(Wake::Changed).is_err() {
                    return;
                }
            }
        });
    }
    Ok(())
}

/// The next item of `stream`, without a futures crate.
async fn next<S: zbus::export::futures_core::Stream>(
    stream: &mut std::pin::Pin<&mut S>,
) -> Option<S::Item> {
    std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
}

async fn get_all(
    connection: &Connection,
    name: &str,
    path: &str,
    interface: &str,
) -> Option<Properties> {
    let reply = connection
        .call_method(Some(name), path, Some(PROPERTIES), "GetAll", &(interface,))
        .await
        .ok()?;
    reply.body().deserialize::<Properties>().ok()
}

fn get<'a, T>(properties: &'a Properties, name: &str) -> Option<T>
where
    T: TryFrom<&'a zbus::Variant<'a>>,
{
    let value: &zbus::Variant<'_> = properties.get(name)?;
    T::try_from(value).ok()
}

async fn read(connection: &Connection) -> Now {
    let root = get_all(connection, UPOWER, ROOT, UPOWER).await;
    let on_battery = root
        .as_ref()
        .and_then(|r| get::<bool>(r, "OnBattery"))
        .unwrap_or(false);
    let lid = root
        .as_ref()
        .and_then(|r| get::<bool>(r, "LidIsPresent"))
        .unwrap_or(false);
    let display = get_all(connection, UPOWER, DISPLAY_DEVICE, DEVICE).await;
    let mut battery = display.as_ref().and_then(|d| battery(d, on_battery));
    if let Some(battery) = &mut battery {
        battery.health = health(connection).await;
    }
    let modes = get_all(connection, PROFILES, PROFILES_PATH, PROFILES)
        .await
        .and_then(|p| modes(&p));
    let logind = match DBusProxy::new(connection).await {
        Ok(proxy) => proxy
            .name_has_owner(LOGIN.try_into().expect("a bus name"))
            .await
            .unwrap_or(false),
        Err(_) => false,
    };
    Now {
        upower: root.is_some(),
        battery,
        on_battery,
        lid,
        modes,
        logind,
        bus: true,
    }
}

/// UPower's display device as a battery; `None` without one.
fn battery(properties: &Properties, on_battery: bool) -> Option<Battery> {
    if !get::<bool>(properties, "IsPresent")? {
        return None;
    }
    let charge = Charge::of(get::<u32>(properties, "State").unwrap_or(0), on_battery);
    let seconds = match charge {
        Charge::Discharging => get::<i64>(properties, "TimeToEmpty"),
        Charge::Charging => get::<i64>(properties, "TimeToFull"),
        _ => None,
    };
    Some(Battery {
        percent: get::<f64>(properties, "Percentage")
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 100.0) as u8,
        charge,
        left: seconds
            .and_then(|s| u64::try_from(s).ok())
            .filter(|s| *s > 0)
            .map(Duration::from_secs),
        health: None,
    })
}

/// The laptop's battery's `Capacity`, from UPower's devices.
async fn health(connection: &Connection) -> Option<u8> {
    let reply = connection
        .call_method(Some(UPOWER), ROOT, Some(UPOWER), "EnumerateDevices", &())
        .await
        .ok()?;
    let paths: Vec<zbus::OwnedObjectPath> = reply.body().deserialize().ok()?;
    for path in paths {
        let Some(device) = get_all(connection, UPOWER, path.as_str(), DEVICE).await else {
            continue;
        };
        let battery = get::<u32>(&device, "Type") == Some(BATTERY_TYPE)
            && get::<bool>(&device, "PowerSupply").unwrap_or(false);
        if battery {
            return get::<f64>(&device, "Capacity")
                .filter(|c| *c > 0.0)
                .map(|c| c.round().clamp(0.0, 100.0) as u8);
        }
    }
    None
}

fn modes(properties: &Properties) -> Option<Modes> {
    let active = get::<&str>(properties, "ActiveProfile")?.to_owned();
    let available = properties
        .get("Profiles")
        .and_then(|list| list.try_clone().ok())
        .and_then(|list| Vec::<HashMap<String, OwnedVariant>>::try_from(list).ok())
        .map(|list| {
            list.iter()
                .filter_map(|p| get::<&str>(p, "Profile").map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(Modes {
        active,
        available,
        degraded: get::<&str>(properties, "PerformanceDegraded")
            .unwrap_or_default()
            .to_owned(),
    })
}

async fn set_mode(connection: &Connection, mode: &str) {
    let set = connection
        .call_method(
            Some(PROFILES),
            PROFILES_PATH,
            Some(PROPERTIES),
            "Set",
            &(PROFILES, "ActiveProfile", zbus::Variant::from(mode)),
        )
        .await;
    if let Err(err) = set {
        tracing::warn!(%err, mode, "power-profiles-daemon did not take the mode");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upowers_states_on_battery_and_on_ac() {
        assert_eq!(Charge::of(1, false), Charge::Charging);
        assert_eq!(Charge::of(2, true), Charge::Discharging);
        assert_eq!(Charge::of(4, false), Charge::Full);
        assert_eq!(Charge::of(5, false), Charge::Held);
        assert_eq!(Charge::of(2, false), Charge::Held);
    }
}
