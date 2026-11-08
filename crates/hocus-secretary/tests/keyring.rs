//! Her memory's key in the keyring (§7.1): a fake Secret Service on a
//! private `dbus-daemon` of the test's own, never the session's. A new
//! memory's key goes to the keyring, no file beside it; the next start
//! reads it back and opens her memory; a keyring that holds nothing for
//! a memory already made opens nothing.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};

use hocus_secretary::{
    clock::Clock,
    key::{self, Place},
    store::Store,
};
use zbus::{ObjectPath, OwnedObjectPath, OwnedVariant as OwnedValue};

/// A `dbus-daemon` of the test's own; stopped (its own process) on drop.
struct PrivateBus {
    daemon: Child,
    address: String,
}

impl PrivateBus {
    fn start(dir: &Path) -> anyhow::Result<Option<Self>> {
        let socket = dir.join("bus");
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:path={}</listen>\
                 <auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )?;
        let Ok(mut daemon) = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        };
        let out = daemon
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no output from dbus-daemon"))?;
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let read = BufReader::new(out).read_line(&mut line);
            let _ = said.send(read.map(|_| line));
        });
        match heard.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(line)) if !line.trim().is_empty() => Ok(Some(Self {
                daemon,
                address: format!("unix:path={}", socket.display()),
            })),
            other => {
                let _ = daemon.kill();
                let _ = daemon.wait();
                anyhow::bail!("dbus-daemon did not listen: {other:?}")
            }
        }
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// What the fake keyring holds: one item, its attributes and its secret.
type Held = Arc<Mutex<Option<(HashMap<String, String>, Vec<u8>)>>>;

struct Service(Held);
struct Collection(Held);
struct Item(Held);

const ITEM: &str = "/org/freedesktop/secrets/collection/login/1";
const COLLECTION: &str = "/org/freedesktop/secrets/collection/login";

fn path(text: &str) -> OwnedObjectPath {
    ObjectPath::try_from(text)
        .map(Into::into)
        .unwrap_or_default()
}

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(&self, _algorithm: &str, _input: OwnedValue) -> (OwnedValue, OwnedObjectPath) {
        (
            OwnedValue::from(zbus::Str::from("")),
            path("/org/freedesktop/secrets/session/1"),
        )
    }

    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let found = held
            .as_ref()
            .is_some_and(|(kept, _)| attributes.iter().all(|(k, v)| kept.get(k) == Some(v)));
        (
            if found { vec![path(ITEM)] } else { Vec::new() },
            Vec::new(),
        )
    }

    fn read_alias(&self, _name: &str) -> OwnedObjectPath {
        path(COLLECTION)
    }
}

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    fn create_item(
        &self,
        properties: HashMap<String, OwnedValue>,
        secret: (OwnedObjectPath, Vec<u8>, Vec<u8>, String),
        _replace: bool,
    ) -> (OwnedObjectPath, OwnedObjectPath) {
        let attributes: HashMap<String, String> = properties
            .get("org.freedesktop.Secret.Item.Attributes")
            .and_then(|v| HashMap::<String, String>::try_from(v.clone()).ok())
            .unwrap_or_default();
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((attributes, secret.2));
        (path(ITEM), path("/"))
    }
}

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl Item {
    fn get_secret(
        &self,
        session: OwnedObjectPath,
    ) -> zbus::fdo::Result<(OwnedObjectPath, Vec<u8>, Vec<u8>, String)> {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (_, secret) = held
            .as_ref()
            .ok_or_else(|| zbus::fdo::Error::Failed("no item".to_owned()))?;
        Ok((
            session,
            Vec::new(),
            secret.clone(),
            "application/octet-stream".to_owned(),
        ))
    }
}

/// The fake keyring on `bus`, while the runtime lives.
fn keyring(bus: &str, held: &Held) -> anyhow::Result<(tokio::runtime::Runtime, zbus::Connection)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let connection = runtime.block_on(async {
        let connection = zbus::connection::Builder::address(bus)
            .serve_at("/org/freedesktop/secrets", Service(held.clone()))
            .serve_at(COLLECTION, Collection(held.clone()))
            .serve_at(ITEM, Item(held.clone()))
            .name("org.freedesktop.secrets")
            .build()
            .await?;
        anyhow::Ok(connection)
    })?;
    Ok((runtime, connection))
}

#[test]
fn her_memory_s_key_is_kept_in_the_keyring_of_the_bus_given() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(bus) = PrivateBus::start(dir.path())? else {
        return Ok(());
    };
    let held = Held::default();
    let _keyring = keyring(&bus.address, &held)?;
    let data = dir.path().join("data");
    std::fs::create_dir(&data)?;

    let (made, place) = key::key(&data, Some(&bus.address), false).map_err(anyhow::Error::msg)?;
    assert_eq!(place, Place::Keyring);
    assert!(
        !data.join(key::FILE).exists(),
        "no key file beside her memory"
    );
    let kept = held.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (attributes, secret) = kept.expect("an item");
    assert_eq!(
        attributes.get("hocuspanda").map(String::as_str),
        Some("secretary")
    );
    assert_eq!(secret, made);

    let file = data.join("memory.db");
    drop(Store::open(&file, &made, Clock::Wall)?);
    let (again, place) = key::key(&data, Some(&bus.address), true).map_err(anyhow::Error::msg)?;
    assert_eq!((again, place), (made, Place::Keyring));
    Store::open(&file, &again, Clock::Wall)?;

    // The keyring emptied: her memory stays closed, no new key over it.
    *held.lock().unwrap_or_else(PoisonError::into_inner) = None;
    assert!(key::key(&data, Some(&bus.address), true).is_err());
    assert!(!data.join(key::FILE).exists());
    Ok(())
}
