//! The system bus, on a thread of its own: the battery from UPower's
//! display device, whether the machine is on AC (`OnBattery`), the
//! battery's health (its device's `Capacity`), the keyboard's backlight
//! (`KbdBacklight`), and the power profiles from power-profiles-daemon;
//! each read again when its signals say it changed, or its owner did (no
//! polling). And what the keys and the popovers ask: the brightness
//! through logind's `SetBrightness` on the session, the keyboard's
//! backlight through UPower, the profile through power-profiles-daemon.
//!
//! What is asked while a call runs waits for it: then only the last of
//! each is asked (the keys' steps taken together), so that a slider moved
//! fast never queues calls.
//!
//! The bus is the one `DBUS_SYSTEM_BUS_ADDRESS` names, else the system's.

use std::{collections::HashMap, time::Duration};

use hocus_link::LevelKey;
use iced::futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    fdo::DBusProxy,
};

use super::{
    Presence, RETRY_FIRST, backlight,
    model::{Battery, Charge, Degraded, Keyboard, Profile, Profiles, brightness_raw},
    next_retry, publish,
};

const UPOWER: &str = "org.freedesktop.UPower";
const ROOT: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const KEYBOARD: &str = "org.freedesktop.UPower.KbdBacklight";
const KEYBOARD_PATH: &str = "/org/freedesktop/UPower/KbdBacklight";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const LOGIN: &str = "org.freedesktop.login1";
const SESSION: &str = "/org/freedesktop/login1/session/auto";
/// UPower's `Type` of a laptop's battery.
const BATTERY: u32 = 2;

type Properties = HashMap<String, OwnedVariant>;

/// What the keys and the popovers ask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ask {
    /// A brightness key, or a notch of the wheel.
    Key(LevelKey),
    /// The brightness's slider, in percent.
    Brightness(f32),
    /// The keyboard's backlight's level.
    Keyboard(u32),
    Profile(Profile),
}

pub fn start(asks: UnboundedReceiver<Ask>) {
    let started = std::thread::Builder::new()
        .name("levels: system bus".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(asks)),
                Err(err) => tracing::warn!(%err, "no runtime for the system bus"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the system bus");
    }
}

async fn run(mut asks: UnboundedReceiver<Ask>) {
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
                    levels.profiles = Profiles::default();
                    levels.degraded = None;
                    levels.keyboard = None;
                });
                // What was asked meanwhile has nowhere to go.
                while asks.try_recv().is_ok() {}
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    let reader = Reader::new(connection.clone());
    tokio::spawn(reader.run());
    while let Some(first) = asks.recv().await {
        let mut batch = vec![first];
        while let Ok(next) = asks.try_recv() {
            batch.push(next);
        }
        carry_out(&connection, &batch).await;
    }
}

// --- Reading ---------------------------------------------------------------------

/// What tells the reader to read again.
#[derive(Debug)]
enum Input {
    Signal(Message),
    /// UPower's owner came (`true`) or went.
    UPower(bool),
    Profiles(bool),
}

/// What is read, as last said.
struct Reader {
    connection: Connection,
    /// UPower's display device's properties; `None` while UPower is away.
    display: Option<Properties>,
    on_battery: Option<bool>,
    /// The laptop's battery's device, and its health.
    battery: Option<String>,
    health: Option<f32>,
    keyboard: Option<Keyboard>,
    profiles: Option<Properties>,
    upower: Presence,
    power_profiles: Presence,
    keyboard_seen: Presence,
}

impl Reader {
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            display: None,
            on_battery: None,
            battery: None,
            health: None,
            keyboard: None,
            profiles: None,
            upower: Presence::Unknown,
            power_profiles: Presence::Unknown,
            keyboard_seen: Presence::Unknown,
        }
    }

    async fn run(mut self) {
        let (inputs, mut incoming) = unbounded_channel();
        if let Err(err) = listen(&self.connection, inputs).await {
            tracing::warn!(%err, "cannot follow UPower and power-profiles-daemon");
        }
        // Read when it starts, then at each change while there is an owner:
        // not when it went, which would start it again through the bus.
        self.read_upower().await;
        self.read_profiles().await;
        self.publish();
        while let Some(input) = incoming.recv().await {
            self.input(input).await;
            // A burst is shown once.
            while let Ok(input) = incoming.try_recv() {
                self.input(input).await;
            }
            self.publish();
        }
    }

    async fn input(&mut self, input: Input) {
        match input {
            Input::UPower(true) => self.read_upower().await,
            Input::UPower(false) => {
                self.upower.now(false, "UPower", "it stopped");
                self.display = None;
                self.on_battery = None;
                self.battery = None;
                self.health = None;
                self.keyboard = None;
            }
            Input::Profiles(true) => self.read_profiles().await,
            Input::Profiles(false) => {
                self.power_profiles
                    .now(false, "power-profiles-daemon", "it stopped");
                self.profiles = None;
            }
            Input::Signal(message) => self.signal(&message).await,
        }
    }

    async fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(path), Some(member)) = (header.path(), header.member()) else {
            return;
        };
        match (path.as_str(), member.as_str()) {
            (DISPLAY_DEVICE, "PropertiesChanged") => {
                self.display = self.get_all(UPOWER, DISPLAY_DEVICE, DEVICE).await.ok();
            }
            (ROOT, "PropertiesChanged" | "DeviceAdded" | "DeviceRemoved") => {
                self.read_root().await;
            }
            (PROFILES_PATH, "PropertiesChanged") => self.read_profiles().await,
            (KEYBOARD_PATH, "BrightnessChanged" | "BrightnessChangedWithSource") => {
                let level = message.body().deserialize::<(i32,)>().map(|(l,)| l);
                match (self.keyboard.as_mut(), level) {
                    (Some(keyboard), Ok(level)) => {
                        keyboard.level = u32::try_from(level).unwrap_or(0).min(keyboard.max);
                    }
                    _ => self.read_keyboard().await,
                }
            }
            (path, "PropertiesChanged") if self.battery.as_deref() == Some(path) => {
                // Its health changes seldom: read again only when it does.
                let changed = message
                    .body()
                    .deserialize::<(String, Properties, Vec<String>)>()
                    .is_ok_and(|(_, changed, gone)| {
                        changed.contains_key("Capacity") || gone.iter().any(|g| g == "Capacity")
                    });
                if changed {
                    self.read_root().await;
                }
            }
            _ => {}
        }
    }

    async fn get_all(&self, name: &str, path: &str, interface: &str) -> zbus::Result<Properties> {
        let reply = self
            .connection
            .call_method(Some(name), path, Some(PROPERTIES), "GetAll", &(interface,))
            .await?;
        reply.body().deserialize()
    }

    /// UPower's `method` of `interface` at `path`, without arguments.
    async fn call(&self, path: &str, interface: &str, method: &str) -> zbus::Result<Message> {
        self.connection
            .call_method(Some(UPOWER), path, Some(interface), method, &())
            .await
    }

    /// The display device, whether on AC, the battery's health and the
    /// keyboard's backlight.
    async fn read_upower(&mut self) {
        match self.get_all(UPOWER, DISPLAY_DEVICE, DEVICE).await {
            Ok(display) => {
                self.upower.now(true, "UPower", "");
                self.display = Some(display);
            }
            Err(err) => {
                self.upower.now(false, "UPower", &err);
                self.display = None;
            }
        }
        self.read_root().await;
        self.read_keyboard().await;
    }

    /// `OnBattery`, and the laptop's battery's `Capacity`: the first of
    /// UPower's devices that is a battery powering the machine.
    async fn read_root(&mut self) {
        self.on_battery = self
            .get_all(UPOWER, ROOT, UPOWER)
            .await
            .ok()
            .and_then(|root| get::<bool>(&root, "OnBattery"));
        let devices: Vec<OwnedObjectPath> = match self.call(ROOT, UPOWER, "EnumerateDevices").await
        {
            Ok(reply) => reply.body().deserialize().unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let mut paths: Vec<String> = devices.iter().map(|p| p.as_str().to_owned()).collect();
        paths.sort();
        self.battery = None;
        self.health = None;
        for path in paths {
            let Ok(device) = self.get_all(UPOWER, &path, DEVICE).await else {
                continue;
            };
            let battery = get::<u32>(&device, "Type") == Some(BATTERY)
                && get::<bool>(&device, "PowerSupply").unwrap_or(false);
            if battery {
                self.health = get::<f64>(&device, "Capacity").map(|c| c as f32);
                self.battery = Some(path);
                break;
            }
        }
    }

    async fn read_keyboard(&mut self) {
        let level = |reply: zbus::Result<Message>| -> zbus::Result<i32> {
            reply?.body().deserialize::<i32>()
        };
        let max = level(self.call(KEYBOARD_PATH, KEYBOARD, "GetMaxBrightness").await);
        let now = level(self.call(KEYBOARD_PATH, KEYBOARD, "GetBrightness").await);
        let what = "the keyboard's backlight";
        self.keyboard = match (max, now) {
            (Ok(max), Ok(now)) if max > 0 => {
                self.keyboard_seen.now(true, what, "");
                let max = u32::try_from(max).unwrap_or(1);
                Some(Keyboard {
                    level: u32::try_from(now).unwrap_or(0).min(max),
                    max,
                })
            }
            (Err(err), _) | (_, Err(err)) => {
                self.keyboard_seen.now(false, what, &err);
                None
            }
            _ => {
                self.keyboard_seen.now(false, what, "no levels");
                None
            }
        };
    }

    async fn read_profiles(&mut self) {
        match self.get_all(PROFILES, PROFILES_PATH, PROFILES).await {
            Ok(all) => {
                self.power_profiles.now(true, "power-profiles-daemon", "");
                self.profiles = Some(all);
            }
            Err(err) => {
                self.power_profiles
                    .now(false, "power-profiles-daemon", &err);
                self.profiles = None;
            }
        }
    }

    fn publish(&self) {
        let battery = self.display.as_ref().and_then(battery).map(|battery| {
            let battery = Battery {
                health: self.health,
                ..battery
            };
            on_ac(battery, self.on_battery, self.display.as_ref())
        });
        let profiles = self.profiles.as_ref();
        let keyboard = self.keyboard;
        publish(|levels| {
            levels.battery = battery;
            levels.profile = profiles.and_then(profile);
            levels.profiles = profiles.map(available).unwrap_or_default();
            levels.degraded = profiles
                .and_then(|p| get::<&str>(p, "PerformanceDegraded"))
                .and_then(Degraded::from_reason);
            levels.keyboard = keyboard;
        });
    }
}

/// UPower's and power-profiles-daemon's signals, and their owners'
/// changes, into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    // Both live under UPower's path.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace(ROOT)
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
    let to = inputs.clone();
    tokio::spawn(async move {
        while let Some(Ok(message)) = signals.next().await {
            if to.send(Input::Signal(message)).is_err() {
                return;
            }
        }
    });
    let bus = DBusProxy::new(connection).await?;
    for name in [UPOWER, PROFILES] {
        let mut owners = bus
            .receive_name_owner_changed_with_args(&[(0, name)])
            .await?;
        let to = inputs.clone();
        tokio::spawn(async move {
            while let Some(changed) = owners.next().await {
                let owned = changed
                    .args()
                    .map_or(true, |args| args.new_owner().is_some());
                let input = if name == UPOWER {
                    Input::UPower(owned)
                } else {
                    Input::Profiles(owned)
                };
                if to.send(input).is_err() {
                    return;
                }
            }
        });
    }
    Ok(())
}

fn get<'a, T>(properties: &'a Properties, name: &str) -> Option<T>
where
    T: TryFrom<&'a Variant<'a>>,
    <T as TryFrom<&'a Variant<'a>>>::Error: Into<zbus::Error>,
{
    properties.get(name)?.downcast_ref::<T>().ok()
}

/// UPower's display device as a battery; `None` without one.
fn battery(properties: &Properties) -> Option<Battery> {
    if !get::<bool>(properties, "IsPresent")? {
        return None;
    }
    let charge = Charge::from_upower(get::<u32>(properties, "State").unwrap_or(0));
    Some(Battery {
        percent: get::<f64>(properties, "Percentage").unwrap_or(0.0) as f32,
        charge,
        left: left(properties, charge),
        health: None,
    })
}

/// The time left for `charge`: until empty, or until full.
fn left(properties: &Properties, charge: Charge) -> Option<Duration> {
    let seconds = match charge {
        Charge::Discharging => get::<i64>(properties, "TimeToEmpty"),
        Charge::Charging => get::<i64>(properties, "TimeToFull"),
        _ => None,
    };
    seconds
        .and_then(|s| u64::try_from(s).ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
}

/// `battery` as `OnBattery` says, when it says.
fn on_ac(battery: Battery, on_battery: Option<bool>, display: Option<&Properties>) -> Battery {
    let Some(on_battery) = on_battery else {
        return battery;
    };
    let charge = battery.charge.on_ac(!on_battery);
    if charge == battery.charge {
        return battery;
    }
    Battery {
        charge,
        left: display.and_then(|d| left(d, charge)),
        ..battery
    }
}

fn profile(properties: &Properties) -> Option<Profile> {
    Profile::from_name(get::<&str>(properties, "ActiveProfile")?)
}

/// The profiles power-profiles-daemon has: `Profiles`, each a dictionary
/// whose `Profile` names it.
fn available(properties: &Properties) -> Profiles {
    let Some(list) = properties.get("Profiles") else {
        return Profiles::default();
    };
    let Ok(list) = list.try_clone() else {
        return Profiles::default();
    };
    let Ok(list) = Vec::<HashMap<String, OwnedVariant>>::try_from(list) else {
        return Profiles::default();
    };
    Profiles::from_names(list.iter().filter_map(|p| get::<&str>(p, "Profile")))
}

// --- Asking ----------------------------------------------------------------------

/// What `batch` asks, in one call each: the brightness its keys and its
/// slider end at, the last keyboard level, the last profile.
async fn carry_out(connection: &Connection, batch: &[Ask]) {
    let lights = batch
        .iter()
        .any(|a| matches!(a, Ask::Key(_) | Ask::Brightness(_)));
    if lights {
        set_brightness(connection, batch).await;
    }
    let keyboard = batch.iter().rev().find_map(|a| match a {
        Ask::Keyboard(level) => Some(*level),
        _ => None,
    });
    if let Some(level) = keyboard {
        let level = i32::try_from(level).unwrap_or(0);
        let set = connection
            .call_method(
                Some(UPOWER),
                KEYBOARD_PATH,
                Some(KEYBOARD),
                "SetBrightness",
                &(level,),
            )
            .await;
        if let Err(err) = set {
            tracing::warn!(%err, level, "UPower did not set the keyboard's backlight");
        }
    }
    let profile = batch.iter().rev().find_map(|a| match a {
        Ask::Profile(profile) => Some(*profile),
        _ => None,
    });
    if let Some(profile) = profile {
        let name = match profile {
            Profile::PowerSaver => "power-saver",
            Profile::Balanced => "balanced",
            Profile::Performance => "performance",
        };
        let set = connection
            .call_method(
                Some(PROFILES),
                PROFILES_PATH,
                Some(PROPERTIES),
                "Set",
                &(PROFILES, "ActiveProfile", Variant::from(name)),
            )
            .await;
        if let Err(err) = set {
            tracing::warn!(%err, name, "power-profiles-daemon did not set the profile");
        }
    }
}

/// The backlight where the keys' steps and the slider end, from what it
/// is: one call to logind, then shown at once (udev tells it again), unless
/// the backlight said another brightness meanwhile: changed from elsewhere
/// after logind set it, it is newer than what was asked (#478).
async fn set_brightness(connection: &Connection, batch: &[Ask]) {
    let Some(device) = backlight::device(&backlight::dir()) else {
        return;
    };
    let Some((raw, max)) = backlight::read(&device) else {
        return;
    };
    let value = batch.iter().fold(raw, |value, ask| match ask {
        Ask::Key(key) => super::model::brightness_step(value, max, *key == LevelKey::BrightnessUp),
        Ask::Brightness(percent) => brightness_raw(*percent, max),
        _ => value,
    });
    if value == raw {
        return;
    }
    let before = super::snapshot().brightness;
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
        Ok(_) => publish(|levels| {
            if levels.brightness == before {
                levels.brightness = Some(value as f32 * 100.0 / max as f32);
            }
        }),
        Err(err) => tracing::warn!(%err, device = device.name, "logind did not set the brightness"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(pairs: Vec<(&str, Variant<'static>)>) -> Properties {
        pairs
            .into_iter()
            .filter_map(|(k, v)| Some((k.to_owned(), OwnedVariant::try_from(v).ok()?)))
            .collect()
    }

    #[test]
    fn on_ac_or_on_battery_as_upower_says() {
        let display = properties(vec![
            ("IsPresent", Variant::from(true)),
            ("Percentage", Variant::from(80.0_f64)),
            // A charge limit the firmware calls discharging.
            ("State", Variant::from(2_u32)),
            ("TimeToEmpty", Variant::from(3600_i64)),
            ("TimeToFull", Variant::from(0_i64)),
        ]);
        let b = battery(&display).expect("a battery");
        assert_eq!(b.charge, Charge::Discharging);
        // `OnBattery` false: on AC.
        let held = on_ac(b, Some(false), Some(&display));
        assert_eq!(held.charge, Charge::PluggedIn);
        assert_eq!(held.left, None);
        assert!(held.bolt());
        assert_eq!(on_ac(b, Some(true), Some(&display)), b);
        // Not said: as the state says.
        assert_eq!(on_ac(b, None, Some(&display)), b);
    }

    #[test]
    fn the_profiles_there() {
        let entry = |name: &str| {
            HashMap::from([
                ("Profile".to_owned(), Variant::from(name.to_owned())),
                ("Driver".to_owned(), Variant::from("placeholder".to_owned())),
            ])
        };
        let all = properties(vec![
            ("ActiveProfile", Variant::from("balanced")),
            (
                "Profiles",
                Variant::from(vec![entry("power-saver"), entry("balanced")]),
            ),
        ]);
        let p = available(&all);
        assert!(p.power_saver && p.balanced && !p.performance);
        assert_eq!(profile(&all), Some(Profile::Balanced));
        assert!(!available(&Properties::new()).any());
    }
}
