//! The system's own state for quick settings and the status row, from the
//! services that keep it: UPower (battery), power-profiles-daemon,
//! NetworkManager, BlueZ, PipeWire (through `wpctl`) and the backlight
//! (sysfs, set through logind). Each source runs on a thread of its own;
//! a missing service leaves its part as it was. Nothing here ever runs on
//! the compositor's loop. Off the session (tests on a private bus), the
//! sound and the backlight are kept in memory.

use std::{future::poll_fn, path::PathBuf, pin::Pin, process::Command, sync::mpsc, time::Duration};

use futures_core::Stream;
use zbus::{Connection, OwnedObjectPath, Proxy, Variant, fdo::PropertiesProxy};

use crate::quick::{Battery, Change, Charge, Profile, State};

/// How often what has no signal (sound, backlight) is read again.
const POLL: Duration = Duration::from_secs(2);

/// Which system bus: the machine's, or another (tests).
#[derive(Debug, Clone, Default)]
pub struct Buses {
    pub system: Option<String>,
    pub devices: Devices,
}

/// Where the sound and the backlight are, which no bus serves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Devices {
    /// The machine's: PipeWire through `wpctl`, the backlight in sysfs.
    #[default]
    Machine,
    /// Kept in memory: a test beside a live session reads and changes
    /// neither the developer's sound nor their screen.
    Memory,
}

/// A part of the state, as a source last read it.
#[derive(Debug, Clone, PartialEq)]
enum Update {
    Battery(Option<Battery>),
    Profile(Profile, [bool; 3]),
    Network {
        wifi: bool,
        name: Option<String>,
    },
    Bluetooth {
        powered: bool,
    },
    Sound {
        volume: u8,
        muted: bool,
        device: String,
        input: String,
    },
    Brightness(Vec<(String, u8)>),
}

enum Message {
    Update(Update),
    Change(Change),
}

/// The service: changes go in, the new state comes out through `sink`.
#[derive(Debug, Clone)]
pub struct System {
    messages: mpsc::Sender<Message>,
}

impl System {
    /// Starts the sources; `screens` are the outputs' names, the first
    /// internal one gets the backlight.
    pub fn start(
        buses: Buses,
        screens: &[String],
        sink: impl Fn(State) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (messages, received) = mpsc::channel::<Message>();
        let mut state = crate::quick::stub_state(&[]);
        state.wifi = false;
        state.volume = 0;
        let devices = buses.devices;
        if devices == Devices::Memory {
            state.output_device = "Built-in speakers".into();
            state.input_device = "Built-in microphone".into();
        }
        let backlit = screens
            .iter()
            .find(|s| ["eDP", "LVDS", "DSI"].iter().any(|p| s.starts_with(p)))
            .cloned();
        let actions = buses.clone();
        let backlight_for = backlit.clone();
        std::thread::Builder::new()
            .name("system".into())
            .spawn(move || {
                sink(state.clone());
                for message in received {
                    match message {
                        Message::Update(update) => apply(&mut state, update),
                        // Shown once the system took it: a refused change
                        // (polkit, a missing service) leaves what is true.
                        Message::Change(change) => {
                            if act(&actions, &change, backlight_for.as_deref(), devices) {
                                state.apply_change(change);
                            }
                        }
                    }
                    sink(state.clone());
                }
            })?;
        type Watch = fn(Buses, mpsc::Sender<Message>) -> zbus::Result<()>;
        let watches: [(&str, Watch); 4] = [
            ("battery", |b, t| zbus::block_on(battery(b, t))),
            ("power profile", |b, t| zbus::block_on(profile(b, t))),
            ("network", |b, t| zbus::block_on(network(b, t))),
            ("bluetooth", |b, t| zbus::block_on(bluetooth(b, t))),
        ];
        for (what, watch) in watches {
            let tx = messages.clone();
            let buses = buses.clone();
            std::thread::Builder::new()
                .name(format!("system {what}"))
                .spawn(move || {
                    if let Err(err) = watch(buses, tx) {
                        tracing::debug!(what, %err, "a system service is not there");
                    }
                })?;
        }
        if devices == Devices::Machine {
            let tx = messages.clone();
            std::thread::Builder::new()
                .name("system poll".into())
                .spawn(move || poll(backlit, tx))?;
        }
        Ok(Self { messages })
    }

    pub fn change(&self, change: Change) {
        let _ = self.messages.send(Message::Change(change));
    }
}

fn apply(state: &mut State, update: Update) {
    match update {
        Update::Battery(b) => state.battery = b,
        Update::Profile(p, available) => {
            state.profile = p;
            state.profiles = available;
        }
        Update::Network { wifi, name } => {
            state.wifi = wifi;
            state.network = name;
        }
        Update::Bluetooth { powered } => state.bluetooth = powered,
        Update::Sound {
            volume,
            muted,
            device,
            input,
        } => {
            state.volume = volume;
            state.muted = muted;
            state.output_device = device;
            state.input_device = input;
        }
        Update::Brightness(b) => state.brightness = b,
    }
}

async fn connect(buses: &Buses) -> zbus::Result<Connection> {
    match &buses.system {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())
                .build()
                .await
        }
        None => Connection::system().await,
    }
}

/// Reads with `read`, then again at each change of `path`'s properties.
async fn follow<'a, F, Fut>(
    conn: &'a Connection,
    destination: &'static str,
    path: &'a str,
    tx: &mpsc::Sender<Message>,
    read: F,
) -> zbus::Result<()>
where
    F: Fn() -> Fut,
    Fut: Future<Output = zbus::Result<Update>>,
{
    let properties = PropertiesProxy::builder(conn)
        .destination(destination)
        .path(path)
        .build()
        .await?;
    let mut changes = properties.receive_properties_changed().await?;
    loop {
        let update = read().await?;
        if tx.send(Message::Update(update)).is_err() {
            return Ok(());
        }
        let next = poll_fn(|cx| Pin::new(&mut changes).poll_next(cx)).await;
        if next.is_none() {
            return Ok(());
        }
    }
}

async fn battery(buses: Buses, tx: mpsc::Sender<Message>) -> zbus::Result<()> {
    const DEST: &str = "org.freedesktop.UPower";
    const PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
    let conn = connect(&buses).await?;
    let device = Proxy::new(&conn, DEST, PATH, "org.freedesktop.UPower.Device").await?;
    follow(&conn, DEST, PATH, &tx, || async {
        let present: bool = device.get_property("IsPresent").await.unwrap_or(false);
        if !present {
            return Ok(Update::Battery(None));
        }
        let percent: f64 = device.get_property("Percentage").await?;
        let state: u32 = device.get_property("State").await.unwrap_or(0);
        let to_empty: i64 = device.get_property("TimeToEmpty").await.unwrap_or(0);
        let charge = Charge::from_upower(state);
        let minutes_left =
            (charge == Charge::Discharging && to_empty > 0).then_some((to_empty / 60) as u32);
        Ok(Update::Battery(Some(Battery {
            percent: percent.round().clamp(0.0, 100.0) as u8,
            minutes_left,
            charge,
        })))
    })
    .await
}

/// power-profiles-daemon, under its new name or its old one.
async fn profile(buses: Buses, tx: mpsc::Sender<Message>) -> zbus::Result<()> {
    let conn = connect(&buses).await?;
    for (dest, path) in [
        (
            "org.freedesktop.UPower.PowerProfiles",
            "/org/freedesktop/UPower/PowerProfiles",
        ),
        ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
    ] {
        let Ok(proxy) = Proxy::new(&conn, dest, path, dest).await else {
            continue;
        };
        if proxy.get_property::<String>("ActiveProfile").await.is_err() {
            continue;
        }
        return follow(&conn, dest, path, &tx, || async {
            let active: String = proxy.get_property("ActiveProfile").await?;
            // A machine without the hardware for it has no performance.
            let listed: Vec<std::collections::HashMap<String, zbus::OwnedVariant>> =
                proxy.get_property("Profiles").await.unwrap_or_default();
            let has = |name: &str| {
                listed.is_empty()
                    || listed.iter().any(|p| {
                        p.get("Profile")
                            .and_then(|v| String::try_from(v.clone()).ok())
                            .is_some_and(|n| n == name)
                    })
            };
            Ok(Update::Profile(
                match active.as_str() {
                    "power-saver" => Profile::Saver,
                    "performance" => Profile::Performance,
                    _ => Profile::Balanced,
                },
                [has("power-saver"), has("balanced"), has("performance")],
            ))
        })
        .await;
    }
    Ok(())
}

async fn network(buses: Buses, tx: mpsc::Sender<Message>) -> zbus::Result<()> {
    const DEST: &str = "org.freedesktop.NetworkManager";
    const PATH: &str = "/org/freedesktop/NetworkManager";
    let conn = connect(&buses).await?;
    let nm = Proxy::new(&conn, DEST, PATH, DEST).await?;
    follow(&conn, DEST, PATH, &tx, || async {
        let wifi: bool = nm.get_property("WirelessEnabled").await?;
        let primary: OwnedObjectPath = nm.get_property("PrimaryConnection").await?;
        let name = if primary.as_str() == "/" {
            None
        } else {
            let active = Proxy::new(
                &conn,
                DEST,
                primary.as_str().to_owned(),
                "org.freedesktop.NetworkManager.Connection.Active",
            )
            .await?;
            active.get_property::<String>("Id").await.ok()
        };
        Ok(Update::Network { wifi, name })
    })
    .await
}

async fn bluetooth(buses: Buses, tx: mpsc::Sender<Message>) -> zbus::Result<()> {
    const DEST: &str = "org.bluez";
    const PATH: &str = "/org/bluez/hci0";
    let conn = connect(&buses).await?;
    let adapter = Proxy::new(&conn, DEST, PATH, "org.bluez.Adapter1").await?;
    follow(&conn, DEST, PATH, &tx, || async {
        let powered: bool = adapter.get_property("Powered").await?;
        Ok(Update::Bluetooth { powered })
    })
    .await
}

/// What has no signal: the sound through `wpctl`, the backlight in sysfs.
fn poll(backlit: Option<String>, tx: mpsc::Sender<Message>) {
    let mut last = Vec::new();
    loop {
        let mut updates = Vec::new();
        if let Some(sound) = sound() {
            updates.push(sound);
        }
        if let (Some(screen), Some((_, percent))) = (&backlit, backlight()) {
            updates.push(Update::Brightness(vec![(screen.clone(), percent)]));
        }
        if updates != last {
            for update in &updates {
                if tx.send(Message::Update(update.clone())).is_err() {
                    return;
                }
            }
            last = updates;
        }
        std::thread::sleep(POLL);
    }
}

fn wpctl(args: &[&str]) -> Option<String> {
    let out = Command::new("wpctl").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `Volume: 0.40 [MUTED]`, and the sink's and the source's descriptions.
fn sound() -> Option<Update> {
    let line = wpctl(&["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
    let (volume, muted) = parse_volume(&line)?;
    let described = |node: &str| {
        wpctl(&["inspect", node])
            .and_then(|text| {
                text.lines()
                    .find_map(|l| {
                        l.trim()
                            .trim_start_matches("* ")
                            .strip_prefix("node.description = ")
                    })
                    .map(|d| d.trim_matches('"').to_owned())
            })
            .unwrap_or_default()
    };
    Some(Update::Sound {
        volume,
        muted,
        device: described("@DEFAULT_AUDIO_SINK@"),
        input: described("@DEFAULT_AUDIO_SOURCE@"),
    })
}

fn parse_volume(line: &str) -> Option<(u8, bool)> {
    let value: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some((
        (value * 100.0).round().clamp(0.0, 150.0) as u8,
        line.contains("[MUTED]"),
    ))
}

/// The first backlight: its name and brightness in percent.
fn backlight() -> Option<(String, u8)> {
    let dir = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()?
        .path();
    let read = |file: &str| -> Option<u32> {
        std::fs::read_to_string(dir.join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    let (now, max) = (read("brightness")?, read("max_brightness")?);
    let name = dir.file_name()?.to_string_lossy().into_owned();
    Some((name, (u64::from(now) * 100 / u64::from(max.max(1))) as u8))
}

fn backlight_dir() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()
        .map(|e| e.path())
}

/// Carries a change out on the system; whether it was taken (failures are
/// logged).
fn act(buses: &Buses, change: &Change, _backlit: Option<&str>, devices: Devices) -> bool {
    let result = match change {
        // Kept as asked, touching nothing.
        Change::Volume(_) | Change::Muted(_) | Change::Brightness(..)
            if devices == Devices::Memory =>
        {
            Ok(())
        }
        Change::Volume(v) => run_wpctl(&[
            "set-volume",
            "@DEFAULT_AUDIO_SINK@",
            &format!("{:.2}", f64::from(*v) / 100.0),
        ]),
        Change::Muted(m) => run_wpctl(&[
            "set-mute",
            "@DEFAULT_AUDIO_SINK@",
            if *m { "1" } else { "0" },
        ]),
        other => zbus::block_on(dbus_act(buses, other)).map_err(|e| e.to_string()),
    };
    match result {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(?change, %err, "could not change the setting");
            false
        }
    }
}

fn run_wpctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("wpctl")
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wpctl {}", status))
    }
}

async fn dbus_act(buses: &Buses, change: &Change) -> zbus::Result<()> {
    let conn = connect(buses).await?;
    let set = |dest: &'static str,
               path: &'static str,
               iface: &'static str,
               name: &'static str,
               value: Variant<'static>| {
        let conn = conn.clone();
        async move {
            let proxy = Proxy::new(&conn, dest, path, iface).await?;
            proxy
                .set_property(name, value)
                .await
                .map_err(zbus::Error::from)
        }
    };
    match change {
        Change::Wifi(on) => {
            set(
                "org.freedesktop.NetworkManager",
                "/org/freedesktop/NetworkManager",
                "org.freedesktop.NetworkManager",
                "WirelessEnabled",
                (*on).into(),
            )
            .await
        }
        Change::Bluetooth(on) => {
            set(
                "org.bluez",
                "/org/bluez/hci0",
                "org.bluez.Adapter1",
                "Powered",
                (*on).into(),
            )
            .await
        }
        Change::Airplane(on) => {
            let wifi = set(
                "org.freedesktop.NetworkManager",
                "/org/freedesktop/NetworkManager",
                "org.freedesktop.NetworkManager",
                "WirelessEnabled",
                (!*on).into(),
            )
            .await;
            if *on {
                let _ = set(
                    "org.bluez",
                    "/org/bluez/hci0",
                    "org.bluez.Adapter1",
                    "Powered",
                    false.into(),
                )
                .await;
            }
            wifi
        }
        Change::Profile(p) => {
            let name = match p {
                Profile::Saver => "power-saver",
                Profile::Balanced => "balanced",
                Profile::Performance => "performance",
            };
            let new = set(
                "org.freedesktop.UPower.PowerProfiles",
                "/org/freedesktop/UPower/PowerProfiles",
                "org.freedesktop.UPower.PowerProfiles",
                "ActiveProfile",
                name.into(),
            )
            .await;
            match new {
                Ok(()) => Ok(()),
                Err(_) => {
                    set(
                        "net.hadess.PowerProfiles",
                        "/net/hadess/PowerProfiles",
                        "net.hadess.PowerProfiles",
                        "ActiveProfile",
                        name.into(),
                    )
                    .await
                }
            }
        }
        // Through logind, which may write the backlight for us.
        Change::Brightness(_, percent) => {
            let Some(dir) = backlight_dir() else {
                return Ok(());
            };
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let max: u32 = std::fs::read_to_string(dir.join("max_brightness"))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(100);
            let value = u32::from(*percent).min(100) * max / 100;
            let session = Proxy::new(
                &conn,
                "org.freedesktop.login1",
                "/org/freedesktop/login1/session/auto",
                "org.freedesktop.login1.Session",
            )
            .await?;
            session
                .call::<_, _, ()>("SetBrightness", &("backlight", name.as_str(), value))
                .await
        }
        Change::Volume(_) | Change::Muted(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_volume_lines_are_read() {
        assert_eq!(parse_volume("Volume: 0.40"), Some((40, false)));
        assert_eq!(parse_volume("Volume: 0.73 [MUTED]\n"), Some((73, true)));
        assert_eq!(parse_volume("nothing"), None);
    }
}
