//! A stand-in for KDE Connect's daemon on the private bus, with one phone
//! paired and within reach: the parts of its D-Bus interface the session
//! uses (the phones, their battery, notifications, ring, clipboard, share,
//! sftp), saying what it was asked. No phone, no network.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use zbus::{Connection, interface, object_server::SignalEmitter};

const SERVICE: &str = "org.kde.kdeconnect";
const ROOT: &str = "/modules/kdeconnect";
/// The phone's id, as KDE Connect makes them.
pub const DEVICE: &str = "a1b2c3d4e5f6";

/// What it was asked, in order: `ring`, `reply ID TEXT`, `clipboard
/// TEXT`, `share URL`, `browse`.
pub type Log = Arc<Mutex<Vec<String>>>;

#[derive(Debug, Clone)]
struct Shown {
    name: String,
    battery: i32,
    charging: bool,
    /// Where its files are "mounted".
    mount: PathBuf,
}

type Phone = Arc<Mutex<Shown>>;

fn shown(phone: &Phone) -> Shown {
    phone
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

fn note(log: &Log, what: String) {
    if let Ok(mut log) = log.lock() {
        log.push(what);
    }
}

fn device_path() -> String {
    format!("{ROOT}/devices/{DEVICE}")
}

struct Daemon;

#[interface(name = "org.kde.kdeconnect.daemon")]
impl Daemon {
    #[zbus(name = "devices")]
    fn devices(&self, _only_reachable: bool, _only_paired: bool) -> Vec<String> {
        vec![DEVICE.to_owned()]
    }

    #[zbus(signal, name = "deviceListChanged")]
    async fn device_list_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

struct Device {
    phone: Phone,
}

#[interface(name = "org.kde.kdeconnect.device")]
impl Device {
    #[zbus(property(emits_changed_signal = "false"), name = "name")]
    fn name(&self) -> String {
        shown(&self.phone).name
    }

    #[zbus(property(emits_changed_signal = "false"), name = "type")]
    fn kind(&self) -> &str {
        "smartphone"
    }

    #[zbus(property(emits_changed_signal = "false"), name = "isReachable")]
    fn is_reachable(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "false"), name = "isPaired")]
    fn is_paired(&self) -> bool {
        true
    }
}

struct Battery {
    phone: Phone,
}

#[interface(name = "org.kde.kdeconnect.device.battery")]
impl Battery {
    #[zbus(property(emits_changed_signal = "false"), name = "charge")]
    fn charge(&self) -> i32 {
        shown(&self.phone).battery
    }

    #[zbus(property(emits_changed_signal = "false"), name = "isCharging")]
    fn is_charging(&self) -> bool {
        shown(&self.phone).charging
    }

    #[zbus(signal, name = "refreshed")]
    async fn refreshed(
        emitter: &SignalEmitter<'_>,
        is_charging: bool,
        charge: i32,
    ) -> zbus::Result<()>;
}

struct Notifications {
    active: Arc<Mutex<Vec<String>>>,
    log: Log,
}

#[interface(name = "org.kde.kdeconnect.device.notifications")]
impl Notifications {
    #[zbus(name = "activeNotifications")]
    fn active_notifications(&self) -> Vec<String> {
        self.active.lock().map(|a| a.clone()).unwrap_or_default()
    }

    #[zbus(name = "sendReply")]
    fn send_reply(&self, reply_id: String, message: String) {
        note(&self.log, format!("reply {reply_id} {message}"));
    }

    #[zbus(signal, name = "notificationPosted")]
    async fn notification_posted(emitter: &SignalEmitter<'_>, public_id: &str) -> zbus::Result<()>;

    #[zbus(signal, name = "notificationRemoved")]
    async fn notification_removed(emitter: &SignalEmitter<'_>, public_id: &str)
    -> zbus::Result<()>;
}

struct Notification {
    app: String,
    title: String,
    text: String,
    reply_id: String,
}

#[interface(name = "org.kde.kdeconnect.device.notifications.notification")]
impl Notification {
    #[zbus(property(emits_changed_signal = "false"), name = "appName")]
    fn app_name(&self) -> String {
        self.app.clone()
    }

    #[zbus(property(emits_changed_signal = "false"), name = "title")]
    fn title(&self) -> String {
        self.title.clone()
    }

    #[zbus(property(emits_changed_signal = "false"), name = "ticker")]
    fn ticker(&self) -> String {
        format!("{}: {}", self.title, self.text)
    }

    #[zbus(property(emits_changed_signal = "false"), name = "text")]
    fn text(&self) -> String {
        self.text.clone()
    }

    #[zbus(property(emits_changed_signal = "false"), name = "replyId")]
    fn reply_id(&self) -> String {
        self.reply_id.clone()
    }

    #[zbus(property(emits_changed_signal = "false"), name = "silent")]
    fn silent(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "false"), name = "dismissable")]
    fn dismissable(&self) -> bool {
        true
    }
}

struct FindMyPhone {
    log: Log,
}

#[interface(name = "org.kde.kdeconnect.device.findmyphone")]
impl FindMyPhone {
    #[zbus(name = "ring")]
    fn ring(&self) {
        note(&self.log, "ring".into());
    }
}

struct Clipboard {
    log: Log,
}

#[interface(name = "org.kde.kdeconnect.device.clipboard")]
impl Clipboard {
    #[zbus(name = "sendClipboard")]
    fn send_clipboard(&self, content: String) {
        note(&self.log, format!("clipboard {content}"));
    }
}

struct Share {
    log: Log,
}

#[interface(name = "org.kde.kdeconnect.device.share")]
impl Share {
    #[zbus(name = "shareUrl")]
    fn share_url(&self, url: String) {
        note(&self.log, format!("share {url}"));
    }
}

struct Sftp {
    phone: Phone,
    log: Log,
}

#[interface(name = "org.kde.kdeconnect.device.sftp")]
impl Sftp {
    #[zbus(name = "mountAndWait")]
    fn mount_and_wait(&self) -> bool {
        note(&self.log, "browse".into());
        true
    }

    #[zbus(name = "mountPoint")]
    fn mount_point(&self) -> String {
        shown(&self.phone).mount.to_string_lossy().into_owned()
    }
}

/// KDE Connect with a phone, on the bus until dropped.
pub struct FakePhone {
    connection: Connection,
    phone: Phone,
    active: Arc<Mutex<Vec<String>>>,
    posted: usize,
    pub log: Log,
}

impl std::fmt::Debug for FakePhone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakePhone")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl FakePhone {
    /// Takes KDE Connect's name on the bus at `address`, with a phone
    /// named `name` whose files are at `mount`.
    pub fn start(
        address: &str,
        name: &str,
        battery: i32,
        charging: bool,
        mount: PathBuf,
    ) -> anyhow::Result<Self> {
        let phone = Arc::new(Mutex::new(Shown {
            name: name.to_owned(),
            battery,
            charging,
            mount,
        }));
        let log = Log::default();
        let active = Arc::new(Mutex::new(Vec::new()));
        let device = device_path();
        let connection = zbus::block_on(
            zbus::connection::Builder::address(address)
                .serve_at(ROOT, Daemon)
                .serve_at(
                    device.as_str(),
                    Device {
                        phone: phone.clone(),
                    },
                )
                .serve_at(
                    format!("{device}/battery"),
                    Battery {
                        phone: phone.clone(),
                    },
                )
                .serve_at(
                    format!("{device}/notifications"),
                    Notifications {
                        active: active.clone(),
                        log: log.clone(),
                    },
                )
                .serve_at(
                    format!("{device}/findmyphone"),
                    FindMyPhone { log: log.clone() },
                )
                .serve_at(
                    format!("{device}/clipboard"),
                    Clipboard { log: log.clone() },
                )
                .serve_at(format!("{device}/share"), Share { log: log.clone() })
                .serve_at(
                    format!("{device}/sftp"),
                    Sftp {
                        phone: phone.clone(),
                        log: log.clone(),
                    },
                )
                .name(SERVICE)
                .build(),
        )
        .context("could not take KDE Connect's name on the bus")?;
        Ok(Self {
            connection,
            phone,
            active,
            posted: 0,
            log,
        })
    }

    /// Its battery changes; it says so as KDE Connect does.
    pub fn set_battery(&self, battery: i32, charging: bool) -> anyhow::Result<()> {
        if let Ok(mut s) = self.phone.lock() {
            s.battery = battery;
            s.charging = charging;
        }
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            format!("{}/battery", device_path()).as_str(),
            "org.kde.kdeconnect.device.battery",
            "refreshed",
            &(charging, battery),
        ))?;
        Ok(())
    }

    /// The phone posts a notification; with `reply`, its application takes
    /// replies. Its public id.
    pub fn post(
        &mut self,
        app: &str,
        title: &str,
        text: &str,
        reply: bool,
    ) -> anyhow::Result<String> {
        self.posted += 1;
        let id = format!("0_{}", self.posted);
        let path = format!("{}/notifications/{id}", device_path());
        let notification = Notification {
            app: app.to_owned(),
            title: title.to_owned(),
            text: text.to_owned(),
            reply_id: if reply {
                format!("r{}", self.posted)
            } else {
                String::new()
            },
        };
        zbus::block_on(
            self.connection
                .object_server()
                .at(path.as_str(), notification),
        )?;
        if let Ok(mut a) = self.active.lock() {
            a.push(id.clone());
        }
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            format!("{}/notifications", device_path()).as_str(),
            "org.kde.kdeconnect.device.notifications",
            "notificationPosted",
            &(id.as_str(),),
        ))?;
        Ok(id)
    }

    /// The notification `id` goes on the phone.
    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        if let Ok(mut a) = self.active.lock() {
            a.retain(|i| i != id);
        }
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            format!("{}/notifications", device_path()).as_str(),
            "org.kde.kdeconnect.device.notifications",
            "notificationRemoved",
            &(id,),
        ))?;
        Ok(())
    }

    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}
