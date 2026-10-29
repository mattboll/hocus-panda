//! The system bus, on a thread of its own: the battery from UPower's
//! display device, the power profile from power-profiles-daemon, each read
//! again when its properties or its owner change (no polling); and the
//! brightness keys, through logind's `SetBrightness` on the session.
//!
//! The bus is the one `DBUS_SYSTEM_BUS_ADDRESS` names, else the system's.

use std::{collections::HashMap, time::Duration};

use hocus_link::LevelKey;
use iced::futures::{StreamExt, stream};
use tokio::sync::mpsc::UnboundedReceiver;
use zbus::{
    Connection,
    fdo::{DBusProxy, PropertiesProxy},
    zvariant::OwnedValue,
};

use super::{
    Presence, RETRY_FIRST, backlight,
    model::{Battery, Charge, Profile},
    next_retry, publish,
};

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const LOGIN: &str = "org.freedesktop.login1";
const SESSION: &str = "/org/freedesktop/login1/session/auto";

type Properties = HashMap<String, OwnedValue>;

pub fn start(keys: UnboundedReceiver<LevelKey>) {
    let started = std::thread::Builder::new()
        .name("levels: system bus".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(keys)),
                Err(err) => tracing::warn!(%err, "no runtime for the system bus"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the system bus");
    }
}

async fn run(mut keys: UnboundedReceiver<LevelKey>) {
    let mut wait = RETRY_FIRST;
    let mut presence = Presence::Unknown;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => {
                presence.now(true, "the system bus", "");
                break connection;
            }
            Err(err) => {
                presence.now(false, "the system bus", &err);
                publish(|levels| {
                    levels.battery = None;
                    levels.profile = None;
                });
                // Keys pressed meanwhile have nowhere to go.
                while keys.try_recv().is_ok() {}
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    let battery = follow(
        connection.clone(),
        "UPower",
        UPOWER,
        DISPLAY_DEVICE,
        DEVICE,
        |properties| publish(|levels| levels.battery = properties.and_then(battery)),
    );
    tokio::spawn(battery);
    let profile = follow(
        connection.clone(),
        "power-profiles-daemon",
        PROFILES,
        PROFILES_PATH,
        PROFILES,
        |properties| publish(|levels| levels.profile = properties.and_then(profile)),
    );
    tokio::spawn(profile);
    while let Some(key) = keys.recv().await {
        // Keys held repeat faster than logind answers: one step each, from
        // what the last one set.
        set_brightness(&connection, key == LevelKey::BrightnessUp).await;
    }
}

/// The service `name`'s properties of `interface` at `path`, handed to
/// `show` each time they change, or its owner does; `None` while it is
/// not there.
async fn follow(
    connection: Connection,
    what: &'static str,
    name: &'static str,
    path: &'static str,
    interface: &'static str,
    show: impl Fn(Option<Properties>),
) {
    let mut presence = Presence::Unknown;
    let watched = async {
        let bus = DBusProxy::new(&connection).await?;
        let owners = bus
            .receive_name_owner_changed_with_args(&[(0, name)])
            .await?
            .map(|_| ());
        let properties = PropertiesProxy::builder(&connection)
            .destination(name)?
            .path(path)?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        let changes = properties.receive_properties_changed().await?.map(|_| ());
        Ok::<_, zbus::Error>((properties, stream::select(owners, changes)))
    };
    let (properties, mut changes) = match watched.await {
        Ok(watched) => watched,
        Err(err) => {
            presence.now(false, what, &err);
            show(None);
            return;
        }
    };
    loop {
        match properties
            .get_all(zbus::names::InterfaceName::from_static_str_unchecked(
                interface,
            ))
            .await
        {
            Ok(all) => {
                presence.now(true, what, "");
                show(Some(all));
            }
            Err(err) => {
                presence.now(false, what, &err);
                show(None);
            }
        }
        if changes.next().await.is_none() {
            return;
        }
    }
}

fn get<'a, T>(properties: &'a Properties, name: &str) -> Option<T>
where
    T: TryFrom<&'a zbus::zvariant::Value<'a>>,
    <T as TryFrom<&'a zbus::zvariant::Value<'a>>>::Error: Into<zbus::zvariant::Error>,
{
    properties.get(name)?.downcast_ref::<T>().ok()
}

/// UPower's display device as a battery; `None` without one.
fn battery(properties: Properties) -> Option<Battery> {
    if !get::<bool>(&properties, "IsPresent")? {
        return None;
    }
    let charge = Charge::from_upower(get::<u32>(&properties, "State").unwrap_or(0));
    let seconds = match charge {
        Charge::Discharging => get::<i64>(&properties, "TimeToEmpty"),
        Charge::Charging => get::<i64>(&properties, "TimeToFull"),
        _ => None,
    };
    Some(Battery {
        percent: get::<f64>(&properties, "Percentage").unwrap_or(0.0) as f32,
        charge,
        left: seconds
            .and_then(|s| u64::try_from(s).ok())
            .filter(|s| *s > 0)
            .map(Duration::from_secs),
    })
}

fn profile(properties: Properties) -> Option<Profile> {
    Profile::from_name(get::<&str>(&properties, "ActiveProfile")?)
}

/// One step of the backlight, through logind, then shown at once (udev
/// tells it again).
async fn set_brightness(connection: &Connection, up: bool) {
    let Some(device) = backlight::device(&backlight::dir()) else {
        return;
    };
    let Some((raw, max)) = backlight::read(&device) else {
        return;
    };
    let value = super::model::brightness_step(raw, max, up);
    if value == raw {
        return;
    }
    let set = connection
        .call_method(
            Some(LOGIN),
            SESSION,
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &("backlight", device.name.as_str(), value),
        )
        .await;
    match set {
        Ok(_) => publish(|levels| levels.brightness = Some(value as f32 * 100.0 / max as f32)),
        Err(err) => tracing::warn!(%err, device = device.name, "logind did not set the brightness"),
    }
}
