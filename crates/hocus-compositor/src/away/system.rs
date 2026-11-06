//! The system bus, on a thread of its own with its runtime, never on the
//! loop: logind's sleep inhibitor and its `PrepareForSleep`, its lid and
//! power button, UPower's `OnBattery` (7.2) and battery level, and
//! power-profiles-daemon's profile (6.10).
//!
//! - **logind**: a `delay` inhibitor for `sleep` (`Inhibit`), taken as the
//!   thread starts and again each time the loop asks (after a sleep): its
//!   file descriptor goes to the loop, which closes it once the lock shows.
//!   `PrepareForSleep(true)` and `(false)` go to the loop as they come;
//!   the signal is listened to before the inhibitor is asked for, so that
//!   none is missed. A `block` inhibitor of `handle-lid-switch` and
//!   `handle-power-key`, taken once and held for the session: logind
//!   leaves the lid and the button to the compositor (`[power] lid`,
//!   `power-button`); refused, logind keeps its own handling.
//! - **UPower**: `OnBattery`, and its display device's level when a
//!   battery is there; **power-profiles-daemon**: `ActiveProfile`, set as
//!   the loop asks (the automatic power saver). Read as the thread starts
//!   and again when their properties change or either comes back; only
//!   what changed goes to the loop.
//!
//! The bus is the system's on a virtual terminal, or the one given
//! (`--system-bus`): nested or headless without one, none is read, so that
//! a test never reaches the developer's machine.

use std::os::fd::OwnedFd;

use smithay::reexports::calloop::channel::Sender;
use tokio::sync::mpsc::UnboundedReceiver;
use zbus::{Connection, MatchRule, MessageStream, OwnedVariant, connection::Builder};

const LOGIN: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Which system bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The machine's.
    System,
    /// One given by its address.
    Given(String),
}

/// What the thread hands the loop.
#[derive(Debug)]
pub enum FromSystem {
    /// logind's delay inhibitor, held until it is closed.
    Inhibitor(OwnedFd),
    /// The machine is about to sleep (`true`), or woke up.
    PrepareForSleep(bool),
    /// UPower: on battery, or on AC.
    OnBattery(bool),
    /// logind's `block` inhibitor of the lid and the power button, held
    /// for the session: the compositor handles them.
    Handles(OwnedFd),
    /// UPower's battery level, in percent; `None`: no battery.
    Level(Option<f64>),
    /// power-profiles-daemon's profile; `None`: no power-profiles-daemon.
    Profile(Option<String>),
}

/// What the loop asks of the thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToSystem {
    /// A new inhibitor, the last one closed for a sleep.
    Inhibit,
    /// power-profiles-daemon's profile set so.
    Profile(String),
}

pub fn start(bus: Bus, to_loop: Sender<FromSystem>, asks: UnboundedReceiver<ToSystem>) {
    let started = std::thread::Builder::new()
        .name("system bus".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, to_loop, asks)),
                Err(err) => tracing::warn!(%err, "no runtime for the system bus"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the system bus: no lock before sleep");
    }
}

/// The next item of `stream`, without a futures crate.
async fn next<S: zbus::export::futures_core::Stream>(
    stream: &mut std::pin::Pin<&mut S>,
) -> Option<S::Item> {
    std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
}

async fn run(bus: Bus, to_loop: Sender<FromSystem>, mut asks: UnboundedReceiver<ToSystem>) {
    let builder = match &bus {
        Bus::System => Builder::system(),
        Bus::Given(address) => Builder::address(address.as_str()),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the system bus: no lock before sleep, AC assumed");
            return;
        }
    };
    tokio::spawn(sleeps(connection.clone(), to_loop.clone()));
    tokio::spawn(power(connection.clone(), to_loop.clone()));
    handles(&connection, &to_loop).await;
    // The first inhibitor once the signal is listened to: `sleeps` takes
    // it as it starts; each one after, as the loop asks.
    while let Some(ask) = asks.recv().await {
        match ask {
            ToSystem::Inhibit => inhibit(&connection, &to_loop).await,
            ToSystem::Profile(profile) => set_profile(&connection, &profile).await,
        }
    }
}

/// logind's `block` inhibitor of the lid and the power button, to the
/// loop; refused, logind handles them itself.
async fn handles(connection: &Connection, to_loop: &Sender<FromSystem>) {
    let reply = connection
        .call_method(
            Some(LOGIN),
            LOGIN_PATH,
            Some(MANAGER),
            "Inhibit",
            &(
                "handle-lid-switch:handle-power-key",
                "Hocuspanda",
                "The desktop does what its settings say of the lid and the power button",
                "block",
            ),
        )
        .await;
    match reply.and_then(|r| r.body().deserialize::<zbus::OwnedFd>()) {
        Ok(fd) => {
            let _ = to_loop.send(FromSystem::Handles(OwnedFd::from(fd)));
        }
        Err(err) => {
            tracing::warn!(%err, "logind keeps the lid and the power button: the system decides");
        }
    }
}

/// power-profiles-daemon's profile set to `profile`.
async fn set_profile(connection: &Connection, profile: &str) {
    let set = connection
        .call_method(
            Some(PROFILES),
            PROFILES_PATH,
            Some(PROPERTIES),
            "Set",
            &(PROFILES, "ActiveProfile", zbus::Variant::from(profile)),
        )
        .await;
    match set {
        Ok(_) => tracing::info!(profile, "the power profile set"),
        Err(err) => tracing::warn!(%err, profile, "power-profiles-daemon did not take the profile"),
    }
}

/// `PrepareForSleep`, to the loop; the first inhibitor once listening.
async fn sleeps(connection: Connection, to_loop: Sender<FromSystem>) {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(LOGIN_PATH)
        .interface(MANAGER)
        .member("PrepareForSleep")
        .build();
    let stream = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None).await,
        Err(err) => Err(err),
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            tracing::warn!(%err, "cannot follow logind's sleeps: no lock before sleep");
            return;
        }
    };
    inhibit(&connection, &to_loop).await;
    let mut stream = std::pin::pin!(stream);
    while let Some(message) = next(&mut stream).await {
        let Ok(message) = message else {
            continue;
        };
        let Ok((sleeping,)) = message.body().deserialize::<(bool,)>() else {
            continue;
        };
        tracing::info!(sleeping, "logind: PrepareForSleep");
        if to_loop.send(FromSystem::PrepareForSleep(sleeping)).is_err() {
            return;
        }
    }
}

/// logind's delay inhibitor for sleep, to the loop.
async fn inhibit(connection: &Connection, to_loop: &Sender<FromSystem>) {
    let reply = connection
        .call_method(
            Some(LOGIN),
            LOGIN_PATH,
            Some(MANAGER),
            "Inhibit",
            &(
                "sleep",
                "Hocuspanda",
                "The session locks before the machine sleeps",
                "delay",
            ),
        )
        .await;
    let fd = reply.and_then(|r| r.body().deserialize::<zbus::OwnedFd>());
    match fd {
        Ok(fd) => {
            let _ = to_loop.send(FromSystem::Inhibitor(OwnedFd::from(fd)));
        }
        Err(err) => tracing::warn!(%err, "logind gave no sleep inhibitor: no lock before sleep"),
    }
}

/// UPower's `OnBattery` and level, power-profiles-daemon's profile, to
/// the loop as they change.
async fn power(connection: Connection, to_loop: Sender<FromSystem>) {
    // UPower's root, its display device and power-profiles-daemon's
    // object are all under UPower's path.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace(UPOWER_PATH)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .build();
    let changes = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None).await,
        Err(err) => Err(err),
    };
    // UPower or power-profiles-daemon coming back (started on demand, or
    // restarted).
    let owners = |name: &'static str| {
        let connection = connection.clone();
        async move {
            let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
            proxy
                .receive_name_owner_changed_with_args(&[(0, name)])
                .await
        }
    };
    let (changes, owners, profiles) = match (changes, owners(UPOWER).await, owners(PROFILES).await)
    {
        (Ok(changes), Ok(owners), Ok(profiles)) => (changes, owners, profiles),
        (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
            tracing::warn!(%err, "cannot follow UPower: on AC as far as idle goes");
            return;
        }
    };
    let mut said = Said::default();
    read_power(&connection, &to_loop, &mut said).await;
    // Both streams into one: whichever says something, read again.
    let (wake, mut woken) = tokio::sync::mpsc::unbounded_channel::<()>();
    let on_change = wake.clone();
    tokio::spawn(async move {
        let mut changes = std::pin::pin!(changes);
        while next(&mut changes).await.is_some() {
            if on_change.send(()).is_err() {
                return;
            }
        }
    });
    let on_owner = wake.clone();
    tokio::spawn(async move {
        let mut owners = std::pin::pin!(owners);
        while next(&mut owners).await.is_some() {
            if on_owner.send(()).is_err() {
                return;
            }
        }
    });
    tokio::spawn(async move {
        let mut profiles = std::pin::pin!(profiles);
        while next(&mut profiles).await.is_some() {
            if wake.send(()).is_err() {
                return;
            }
        }
    });
    while woken.recv().await.is_some() {
        while woken.try_recv().is_ok() {}
        read_power(&connection, &to_loop, &mut said).await;
    }
}

/// What the loop was told last: only changes go.
#[derive(Debug, Default)]
struct Said {
    battery: Option<bool>,
    level: Option<Option<f64>>,
    profile: Option<Option<String>>,
}

/// One property, if its service answers.
async fn property(
    connection: &Connection,
    (name, path, interface): (&str, &str, &str),
    property: &str,
) -> Option<OwnedVariant> {
    let reply = connection
        .call_method(
            Some(name),
            path,
            Some(PROPERTIES),
            "Get",
            &(interface, property),
        )
        .await
        .ok()?;
    reply.body().deserialize::<OwnedVariant>().ok()
}

async fn read_power(connection: &Connection, to_loop: &Sender<FromSystem>, said: &mut Said) {
    let root = (UPOWER, UPOWER_PATH, UPOWER);
    let battery = property(connection, root, "OnBattery")
        .await
        .and_then(|v| v.downcast_ref::<bool>().ok());
    // UPower away: on AC, as far as idle goes.
    let battery = battery.unwrap_or(false);
    if said.battery != Some(battery) {
        said.battery = Some(battery);
        let _ = to_loop.send(FromSystem::OnBattery(battery));
    }
    let device = (UPOWER, DISPLAY_DEVICE, DEVICE);
    let present = property(connection, device, "IsPresent")
        .await
        .and_then(|v| v.downcast_ref::<bool>().ok())
        .unwrap_or(false);
    let level = if present {
        property(connection, device, "Percentage")
            .await
            .and_then(|v| v.downcast_ref::<f64>().ok())
    } else {
        None
    };
    if said.level != Some(level) {
        said.level = Some(level);
        let _ = to_loop.send(FromSystem::Level(level));
    }
    let profile = property(
        connection,
        (PROFILES, PROFILES_PATH, PROFILES),
        "ActiveProfile",
    )
    .await
    .and_then(|v| String::try_from(v).ok());
    if said.profile.as_ref() != Some(&profile) {
        said.profile = Some(profile.clone());
        let _ = to_loop.send(FromSystem::Profile(profile));
    }
}
