//! The system bus, on a thread of its own with its runtime, never on the
//! loop: logind's sleep inhibitor and its `PrepareForSleep`, and UPower's
//! `OnBattery` (7.2).
//!
//! - **logind**: a `delay` inhibitor for `sleep` (`Inhibit`), taken as the
//!   thread starts and again each time the loop asks (after a sleep): its
//!   file descriptor goes to the loop, which closes it once the lock shows.
//!   `PrepareForSleep(true)` and `(false)` go to the loop as they come;
//!   the signal is listened to before the inhibitor is asked for, so that
//!   none is missed.
//! - **UPower**: `OnBattery`, read as the thread starts and again when its
//!   properties change or UPower comes back.
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
}

/// What the loop asks of the thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToSystem {
    /// A new inhibitor, the last one closed for a sleep.
    Inhibit,
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
    // The first inhibitor once the signal is listened to: `sleeps` takes
    // it as it starts; each one after, as the loop asks.
    while let Some(ToSystem::Inhibit) = asks.recv().await {
        inhibit(&connection, &to_loop).await;
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

/// UPower's `OnBattery`, to the loop as it changes.
async fn power(connection: Connection, to_loop: Sender<FromSystem>) {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(UPOWER_PATH)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .build();
    let changes = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None).await,
        Err(err) => Err(err),
    };
    // UPower coming back (it is started on demand, or restarted).
    let owners = match zbus::fdo::DBusProxy::new(&connection).await {
        Ok(proxy) => {
            proxy
                .receive_name_owner_changed_with_args(&[(0, UPOWER)])
                .await
        }
        Err(err) => Err(err),
    };
    let (changes, owners) = match (changes, owners) {
        (Ok(changes), Ok(owners)) => (changes, owners),
        (Err(err), _) | (_, Err(err)) => {
            tracing::warn!(%err, "cannot follow UPower: on AC as far as idle goes");
            return;
        }
    };
    let mut said = None;
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
    tokio::spawn(async move {
        let mut owners = std::pin::pin!(owners);
        while next(&mut owners).await.is_some() {
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

async fn read_power(
    connection: &Connection,
    to_loop: &Sender<FromSystem>,
    said: &mut Option<bool>,
) {
    let reply = connection
        .call_method(
            Some(UPOWER),
            UPOWER_PATH,
            Some(PROPERTIES),
            "Get",
            &(UPOWER, "OnBattery"),
        )
        .await;
    let battery = reply
        .ok()
        .and_then(|r| r.body().deserialize::<OwnedVariant>().ok())
        .and_then(|v| v.downcast_ref::<bool>().ok());
    // UPower away: on AC, as far as idle goes.
    let battery = battery.unwrap_or(false);
    if *said != Some(battery) {
        *said = Some(battery);
        let _ = to_loop.send(FromSystem::OnBattery(battery));
    }
}
