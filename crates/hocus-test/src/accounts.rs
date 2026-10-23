//! Stand-ins for AccountsService and fprintd on a private system bus, as
//! far as the settings' Account page uses them: any user name is "Alex
//! Panda", without a picture, and a fingerprint reader holds one finger.
//! What they are asked to change is changed (with a PropertiesChanged
//! signal) and written down. Never the machine's.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use zbus::{Connection, OwnedObjectPath, interface, object_server::SignalEmitter};

const ACCOUNTS: &str = "/org/freedesktop/Accounts";
const USER: &str = "/org/freedesktop/Accounts/User1000";
const MANAGER: &str = "/net/reactivated/Fprint/Manager";
const DEVICE: &str = "/net/reactivated/Fprint/Device/0";

/// What they were asked, in order: `SetRealName Alex P.`, `SetIconFile
/// /path`.
pub type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: String) {
    if let Ok(mut log) = log.lock() {
        log.push(line);
    }
}

fn object(path: &str) -> zbus::fdo::Result<OwnedObjectPath> {
    OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

struct Accounts;

#[interface(name = "org.freedesktop.Accounts")]
impl Accounts {
    fn find_user_by_name(&self, _name: String) -> zbus::fdo::Result<OwnedObjectPath> {
        object(USER)
    }
}

struct User {
    real_name: String,
    icon_file: String,
    log: Log,
}

#[interface(name = "org.freedesktop.Accounts.User")]
impl User {
    #[zbus(property)]
    fn real_name(&self) -> String {
        self.real_name.clone()
    }

    #[zbus(property)]
    fn icon_file(&self) -> String {
        self.icon_file.clone()
    }

    async fn set_real_name(
        &mut self,
        name: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        note(&self.log, format!("SetRealName {name}"));
        self.real_name = name;
        self.real_name_changed(&emitter).await?;
        Ok(())
    }

    async fn set_icon_file(
        &mut self,
        file: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        note(&self.log, format!("SetIconFile {file}"));
        // The service keeps a copy of its own; the file itself does here.
        self.icon_file = file;
        self.icon_file_changed(&emitter).await?;
        Ok(())
    }
}

struct Manager;

#[interface(name = "net.reactivated.Fprint.Manager")]
impl Manager {
    fn get_default_device(&self) -> zbus::fdo::Result<OwnedObjectPath> {
        object(DEVICE)
    }
}

struct Device;

#[interface(name = "net.reactivated.Fprint.Device")]
impl Device {
    #[zbus(property, name = "name")]
    fn name(&self) -> String {
        "Panda Touch".into()
    }

    fn list_enrolled_fingers(&self, _user: String) -> Vec<String> {
        vec!["right-index-finger".into()]
    }
}

pub struct FakeAccounts {
    _accounts: Connection,
    _fprint: Connection,
    pub log: Log,
}

impl std::fmt::Debug for FakeAccounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeAccounts")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl FakeAccounts {
    /// Takes AccountsService's and fprintd's names on the bus at
    /// `address`.
    pub fn start(address: &str) -> anyhow::Result<Self> {
        let log = Log::default();
        let bus = || zbus::connection::Builder::address(address);
        let accounts = zbus::block_on(
            bus()
                .serve_at(ACCOUNTS, Accounts)
                .serve_at(
                    USER,
                    User {
                        real_name: "Alex Panda".into(),
                        icon_file: String::new(),
                        log: log.clone(),
                    },
                )
                .name("org.freedesktop.Accounts")
                .build(),
        )
        .context("could not take AccountsService's name on the bus")?;
        let fprint = zbus::block_on(
            bus()
                .serve_at(MANAGER, Manager)
                .serve_at(DEVICE, Device)
                .name("net.reactivated.Fprint")
                .build(),
        )
        .context("could not take fprintd's name on the bus")?;
        Ok(Self {
            _accounts: accounts,
            _fprint: fprint,
            log,
        })
    }

    /// What they were asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}
