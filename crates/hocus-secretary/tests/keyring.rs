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

/// A provider's API key (§11.3): kept by `hocus-secretary key PROVIDER`
/// in the keyring of the bus given, under its own attributes, and read
/// back by her providers; none for a provider without one.
#[test]
fn a_provider_s_key_is_kept_and_read_in_the_keyring_of_the_bus_given() -> anyhow::Result<()> {
    use hocus_secretary::providers::{Keyring, Keys};
    let dir = tempfile::tempdir()?;
    let Some(bus) = PrivateBus::start(dir.path())? else {
        return Ok(());
    };
    let held = Held::default();
    let _keyring = keyring(&bus.address, &held)?;
    let made_up = "sk-ant-test-made-up-key";
    assert!(key::keep_provider_key(&bus.address, "anthropic", made_up));
    let kept = held.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (attributes, secret) = kept.expect("an item");
    assert_eq!(
        attributes.get("provider").map(String::as_str),
        Some("anthropic")
    );
    assert_eq!(
        attributes.get("hocuspanda").map(String::as_str),
        Some("secretary-provider")
    );
    assert_eq!(secret, made_up.as_bytes());

    let keys = Keyring::new(Some(bus.address.clone()));
    let read = keys.key("anthropic").map_err(anyhow::Error::msg)?;
    assert_eq!(read.as_ref().map(|k| k.expose()), Some(made_up));
    assert!(!format!("{read:?}").contains(made_up), "never printed");
    assert_eq!(keys.key("other").map_err(anyhow::Error::msg)?, None);
    assert_eq!(
        Keyring::new(None)
            .key("anthropic")
            .map_err(anyhow::Error::msg)?,
        None
    );
    Ok(())
}

/// `hocus-secretary key anthropic` typed in a terminal of the session
/// (#750): the compositor tells its programs her bus
/// (`HOCUS_SECRETARY_BUS`, as it tells her), so the key goes where she
/// reads it; without one, it says how instead of guessing a keyring.
#[test]
fn the_key_command_keeps_the_key_in_the_keyring_of_her_bus() -> anyhow::Result<()> {
    use std::io::Write as _;

    use hocus_secretary::providers::{Keyring, Keys};
    let dir = tempfile::tempdir()?;
    let Some(bus) = PrivateBus::start(dir.path())? else {
        return Ok(());
    };
    let held = Held::default();
    let _keyring = keyring(&bus.address, &held)?;
    let made_up = "sk-ant-test-made-up-from-a-terminal";
    let run = |bus: Option<&str>| -> anyhow::Result<std::process::Output> {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_hocus-secretary"));
        command
            .args(["key", "anthropic"])
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove(hocus_secretary_link::BUS_ENV)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(bus) = bus {
            command.env(hocus_secretary_link::BUS_ENV, bus);
        }
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdin"))?
            .write_all(format!("{made_up}\n").as_bytes())?;
        Ok(child.wait_with_output()?)
    };
    let refused = run(None)?;
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("terminal of the Hocuspanda session"),
        "{refused:?}"
    );
    assert!(
        held.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    );
    let kept = run(Some(&bus.address))?;
    assert!(kept.status.success(), "{kept:?}");
    assert!(
        !String::from_utf8_lossy(&kept.stdout).contains(made_up),
        "never echoed"
    );
    let read = Keyring::new(Some(bus.address.clone()))
        .key("anthropic")
        .map_err(anyhow::Error::msg)?;
    assert_eq!(read.as_ref().map(|k| k.expose()), Some(made_up));
    Ok(())
}

/// On a terminal, `hocus-secretary key anthropic` asks for the key
/// (« Colle ta clé Anthropic puis Entrée : ») with the echo off: what is
/// pasted never shows on the screen, and the terminal echoes again after.
#[test]
fn on_a_terminal_the_key_command_asks_with_the_echo_off() -> anyhow::Result<()> {
    use std::{
        ffi::OsStr,
        io::{Read as _, Write as _},
        os::unix::{ffi::OsStrExt as _, fs::OpenOptionsExt as _},
    };

    use hocus_secretary::providers::{Keyring, Keys};
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let dir = tempfile::tempdir()?;
    let Some(bus) = PrivateBus::start(dir.path())? else {
        return Ok(());
    };
    let held = Held::default();
    let _keyring = keyring(&bus.address, &held)?;
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let name = ptsname(&master, Vec::new())?;
    let terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
        .open(OsStr::from_bytes(name.as_bytes()))?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_hocus-secretary"))
        .args(["key", "anthropic"])
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env(hocus_secretary_link::BUS_ENV, &bus.address)
        .env("LANG", "fr_FR.UTF-8")
        .env("RUST_LOG", "off")
        .stdin(Stdio::from(terminal.try_clone()?))
        .stdout(Stdio::from(terminal.try_clone()?))
        .stderr(Stdio::from(terminal.try_clone()?))
        .spawn()?;
    let mut master = std::fs::File::from(master);
    // What the terminal shows, read on a thread until the program ends.
    let (shown_to, shown) = mpsc::channel::<Vec<u8>>();
    let mut reader = master.try_clone()?;
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 || shown_to.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut screen = String::new();
    let prompt = "Colle ta clé Anthropic puis Entrée : ";
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !screen.contains(prompt) {
        let wait = deadline.saturating_duration_since(std::time::Instant::now());
        let piece = shown.recv_timeout(wait)?;
        screen.push_str(&String::from_utf8_lossy(&piece));
    }
    let made_up = "sk-ant-test-pasted-on-a-terminal";
    master.write_all(format!("{made_up}\n").as_bytes())?;
    let status = child.wait()?;
    let modes = rustix::termios::tcgetattr(&terminal)?;
    drop(terminal);
    while let Ok(piece) = shown.recv_timeout(Duration::from_millis(500)) {
        screen.push_str(&String::from_utf8_lossy(&piece));
    }
    assert!(status.success(), "{screen}");
    assert!(!screen.contains(made_up), "never on the screen: {screen}");
    assert!(
        modes
            .local_modes
            .contains(rustix::termios::LocalModes::ECHO),
        "the echo is back"
    );
    let read = Keyring::new(Some(bus.address.clone()))
        .key("anthropic")
        .map_err(anyhow::Error::msg)?;
    assert_eq!(read.as_ref().map(|k| k.expose()), Some(made_up));
    Ok(())
}

/// A key put on the command line (after the provider, or in its place)
/// is refused and never printed back: the message says to paste it once
/// asked, and that the shell's history may keep it.
#[test]
fn a_key_on_the_command_line_is_refused_and_never_printed_back() -> anyhow::Result<()> {
    let made_up = "sk-ant-test-typed-on-the-line-by-mistake";
    for args in [vec!["key", "anthropic", made_up], vec!["key", made_up]] {
        let out = Command::new(env!("CARGO_BIN_EXE_hocus-secretary"))
            .args(&args)
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove(hocus_secretary_link::BUS_ENV)
            .stdin(Stdio::null())
            .output()?;
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.status.success(), "{said}");
        assert!(!said.contains(made_up), "never printed back: {said}");
        assert!(
            said.contains("ne se met pas sur la ligne de commande"),
            "{said}"
        );
        assert!(said.contains("never goes on the command line"), "{said}");
        assert!(said.contains("history"), "{said}");
    }
    Ok(())
}
