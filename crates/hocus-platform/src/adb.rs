//! Wireless adb (#153), the phone's other half: pairing a phone for
//! debugging by a QR code, without a cable, and the devices adb reaches,
//! for mirroring their screen with scrcpy.
//!
//! The flow is Android Studio's: the computer shows a QR code
//! `WIFI:T:ADB;S:<name>;P:<password>;;`; the phone scans it (Developer
//! options, Wireless debugging, Pair device with QR code) and advertises a
//! pairing service under that name over mDNS (`_adb-tls-pairing._tcp`).
//! adb's own mDNS discovery (`adb mdns services`) finds it, `adb pair`
//! pairs with the password (SPAKE2, then TLS: the password never travels),
//! and the phone then advertises its debugging port
//! (`_adb-tls-connect._tcp`), which `adb connect` joins. Asking adb rather
//! than browsing mDNS here keeps one view of the network, adb's, and needs
//! no daemon of our own.
//!
//! adb and scrcpy are found on `PATH` at run time; without adb nothing
//! runs. Everything here runs on threads of its own; the compositor gets
//! [`AdbEvent`]s.

use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// The pairing service a phone advertises once it scanned the code.
const PAIRING: &str = "_adb-tls-pairing";
/// The debugging port it advertises once paired.
const CONNECT: &str = "_adb-tls-connect";
/// How long the phone has to scan the code.
const SCAN_WAIT: Duration = Duration::from_secs(180);
/// How long it has, once paired, to advertise its port.
const CONNECT_WAIT: Duration = Duration::from_secs(30);
/// How long one adb command may take.
const COMMAND_WAIT: Duration = Duration::from_secs(20);
/// Between two looks at what adb discovered.
const POLL: Duration = Duration::from_millis(500);

/// The programs, by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub adb: PathBuf,
    /// For mirroring a device's screen.
    pub scrcpy: Option<PathBuf>,
}

/// A program on `path` (a `PATH` value), by its full path.
pub fn on_path(path: Option<&OsStr>, program: &str) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

/// adb and scrcpy on `path`; `None` without adb.
pub fn find(path: Option<&OsStr>) -> Option<Tools> {
    Some(Tools {
        adb: on_path(path, "adb")?,
        scrcpy: on_path(path, "scrcpy"),
    })
}

/// How adb is run: the session's environment for its programs (`None`:
/// the compositor's own), and its runtime directory.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub vars: Option<Vec<(OsString, OsString)>>,
    pub runtime_dir: Option<PathBuf>,
}

impl Env {
    /// `PATH` as the session's programs see it.
    pub fn path(&self) -> Option<OsString> {
        match &self.vars {
            Some(vars) => vars
                .iter()
                .find(|(k, _)| k == "PATH")
                .map(|(_, v)| v.clone()),
            None => std::env::var_os("PATH"),
        }
    }
}

/// A device adb reaches, ready for debugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What adb names it by (`-s`): an address, or a USB serial.
    pub serial: String,
    /// Its model, as the phone says it ("Pixel 8"); else the serial.
    pub name: String,
    /// Reached over the network rather than a cable.
    pub wireless: bool,
}

/// What the phone scans: the service name it advertises back, and the
/// password both sides prove they know.
#[derive(Clone, PartialEq, Eq)]
pub struct Code {
    pub name: String,
    pub password: String,
}

impl std::fmt::Debug for Code {
    // The password stays out of logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Code")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Code {
    /// A fresh name and password, from the system's random source.
    pub fn new() -> std::io::Result<Self> {
        let mut bytes = [0u8; 20];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        // Letters and digits only: nothing to escape in the QR text.
        const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
        let pick = |b: &[u8]| -> String {
            b.iter()
                .map(|x| char::from(ALPHABET[usize::from(*x) % ALPHABET.len()]))
                .collect()
        };
        Ok(Self {
            name: format!("hocuspanda-{}", pick(&bytes[..8])),
            password: pick(&bytes[8..]),
        })
    }

    /// The QR code's text, as Android's Wireless debugging reads it.
    pub fn qr(&self) -> String {
        format!("WIFI:T:ADB;S:{};P:{};;", self.name, self.password)
    }

    /// The name and password in a QR code's text.
    pub fn parse(qr: &str) -> Option<Self> {
        let body = qr.strip_prefix("WIFI:")?;
        let field = |key: &str| {
            body.split(';')
                .find_map(|f| f.strip_prefix(key))
                .map(str::to_owned)
        };
        (field("T:")? == "ADB").then_some(())?;
        Some(Self {
            name: field("S:")?,
            password: field("P:")?,
        })
    }
}

/// Why pairing did not go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The phone never advertised the code's service: not scanned.
    NotSeen,
    /// `adb pair` refused, with what it said.
    Pair(String),
    /// Paired, but its debugging port was not joined.
    Connect(String),
}

/// Where pairing is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// The code is shown; the phone has not scanned it yet.
    Waiting,
    /// The phone scanned it: pairing, then joining its port.
    Pairing,
    Paired(Device),
    Failed(Failure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbEvent {
    /// adb was looked for: the tools when found, and the devices it
    /// reaches.
    Found {
        tools: Option<Tools>,
        devices: Vec<Device>,
    },
    /// Pairing `id` went on.
    Pairing { id: u64, stage: Stage },
}

type Sink = Arc<dyn Fn(AdbEvent) + Send + Sync>;

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread for adb");
    }
}

/// Looks for adb and scrcpy, and asks adb for its devices, on a thread of
/// its own. Without adb, nothing runs and nothing is said but at debug
/// level.
pub fn look(env: Env, sink: impl Fn(AdbEvent) + Send + Sync + 'static) {
    spawn("adb", move || {
        let tools = find(env.path().as_deref());
        let devices = match &tools {
            Some(tools) => devices(&tools.adb, &env),
            None => {
                tracing::debug!("no adb: no wireless debugging");
                Vec::new()
            }
        };
        sink(AdbEvent::Found { tools, devices });
    });
}

/// Pairs the phone that scans `code`, then joins it, on a thread of its
/// own; `cancel` stops it between two steps.
pub fn pair(
    id: u64,
    adb: PathBuf,
    env: Env,
    code: Code,
    cancel: Arc<AtomicBool>,
    sink: impl Fn(AdbEvent) + Send + Sync + 'static,
) {
    let sink: Sink = Arc::new(sink);
    spawn("adb pairing", move || {
        let say = |stage| sink(AdbEvent::Pairing { id, stage });
        let stage = match pairing(&adb, &env, &code, &cancel, &say) {
            Some(stage) => stage,
            None => return,
        };
        if let Stage::Paired(_) = &stage {
            let devices = devices(&adb, &env);
            sink(AdbEvent::Found {
                tools: find(env.path().as_deref()),
                devices,
            });
        }
        if !cancel.load(Ordering::Relaxed) {
            say(stage);
        }
    });
}

/// The pairing's end, or `None` when cancelled.
fn pairing(
    adb: &Path,
    env: &Env,
    code: &Code,
    cancel: &AtomicBool,
    say: &dyn Fn(Stage),
) -> Option<Stage> {
    say(Stage::Waiting);
    let deadline = Instant::now() + SCAN_WAIT;
    let address = loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let found = services(adb, env)
            .into_iter()
            .find(|s| s.kind.starts_with(PAIRING) && s.name == code.name);
        if let Some(found) = found {
            break found.address;
        }
        if Instant::now() > deadline {
            return Some(Stage::Failed(Failure::NotSeen));
        }
        std::thread::sleep(POLL);
    };
    say(Stage::Pairing);
    tracing::info!(%address, "pairing a phone for debugging");
    let said = match run(adb, env, &["pair", &address, &code.password]) {
        Ok(said) => said,
        Err(err) => return Some(Stage::Failed(Failure::Pair(err))),
    };
    if !said.contains("Successfully paired") {
        let why = last_line(&said);
        let why = why.strip_prefix("Failed: ").unwrap_or(&why).to_owned();
        return Some(Stage::Failed(Failure::Pair(why)));
    }
    let guid = paired_guid(&said);
    let host = host_of(&address).to_owned();
    // Its debugging port: adb may join it by itself (its mDNS
    // auto-connect), else it is joined here.
    let deadline = Instant::now() + CONNECT_WAIT;
    let mut tried = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let reached = devices(adb, env);
        if let Some(device) = reached
            .iter()
            .find(|d| is_it(&d.serial, guid.as_deref(), &host))
        {
            return Some(Stage::Paired(device.clone()));
        }
        let port = services(adb, env).into_iter().find(|s| {
            s.kind.starts_with(CONNECT)
                && (guid.as_deref() == Some(s.name.as_str()) || host_of(&s.address) == host)
        });
        if let Some(port) = port {
            match run(adb, env, &["connect", &port.address]) {
                Ok(said) if said.contains("connected to") && !said.contains("failed") => {
                    let device = devices(adb, env)
                        .into_iter()
                        .find(|d| {
                            d.serial == port.address || is_it(&d.serial, guid.as_deref(), &host)
                        })
                        .unwrap_or(Device {
                            serial: port.address.clone(),
                            name: port.address.clone(),
                            wireless: true,
                        });
                    return Some(Stage::Paired(device));
                }
                Ok(said) => tried = last_line(&said),
                Err(err) => tried = err,
            }
        }
        if Instant::now() > deadline {
            return Some(Stage::Failed(Failure::Connect(tried)));
        }
        std::thread::sleep(POLL);
    }
}

/// Whether the device adb names `serial` is the one just paired.
fn is_it(serial: &str, guid: Option<&str>, host: &str) -> bool {
    guid.is_some_and(|g| serial.starts_with(g)) || (serial.contains(':') && host_of(serial) == host)
}

/// The host of `host:port` (`[v6]:port` too).
fn host_of(address: &str) -> &str {
    address
        .rsplit_once(':')
        .map_or(address, |(host, _)| host)
        .trim_start_matches('[')
        .trim_end_matches(']')
}

fn last_line(said: &str) -> String {
    said.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// `adb pair`'s `[guid=adb-…]`: the name the phone's debugging port is
/// advertised under.
fn paired_guid(said: &str) -> Option<String> {
    let rest = &said[said.find("[guid=")? + 6..];
    Some(rest[..rest.find(']')?].to_owned())
}

/// A service adb discovered over mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub kind: String,
    pub address: String,
}

fn services(adb: &Path, env: &Env) -> Vec<Service> {
    match run(adb, env, &["mdns", "services"]) {
        Ok(said) => parse_services(&said),
        Err(err) => {
            tracing::debug!(%err, "adb listed no mDNS services");
            Vec::new()
        }
    }
}

/// `adb mdns services`: a heading, then `NAME\tTYPE\tADDRESS` lines.
pub fn parse_services(said: &str) -> Vec<Service> {
    said.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = if line.contains('\t') {
                line.split('\t').map(str::trim).collect()
            } else {
                line.split_whitespace().collect()
            };
            let [name, kind, .., address] = fields.as_slice() else {
                return None;
            };
            kind.starts_with("_adb").then(|| Service {
                name: (*name).to_owned(),
                kind: (*kind).to_owned(),
                address: (*address).to_owned(),
            })
        })
        .collect()
}

/// The devices adb reaches, ready (not offline nor unauthorized).
pub fn devices(adb: &Path, env: &Env) -> Vec<Device> {
    match run(adb, env, &["devices", "-l"]) {
        Ok(said) => parse_devices(&said),
        Err(err) => {
            tracing::debug!(%err, "adb listed no devices");
            Vec::new()
        }
    }
}

/// `adb devices -l`: `SERIAL STATE key:value…` under a heading.
pub fn parse_devices(said: &str) -> Vec<Device> {
    said.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            if fields.next()? != "device" {
                return None;
            }
            let model = fields
                .find_map(|f| f.strip_prefix("model:"))
                .map(|m| m.replace('_', " "));
            Some(Device {
                serial: serial.to_owned(),
                name: model.unwrap_or_else(|| serial.to_owned()),
                wireless: serial.contains(':') || serial.contains(CONNECT),
            })
        })
        .collect()
}

/// Runs adb with `args` and what it printed, both streams; a command
/// over its time is stopped.
fn run(adb: &Path, env: &Env, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(adb);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(vars) = &env.vars {
        cmd.env_clear().envs(vars.iter().map(|(k, v)| (k, v)));
    }
    if let Some(dir) = &env.runtime_dir {
        cmd.env("XDG_RUNTIME_DIR", dir);
    }
    // Older adb servers discover over mDNS only with their own
    // implementation asked for, as Android Studio does.
    let asked = match &env.vars {
        Some(vars) => vars.iter().any(|(k, _)| k == "ADB_MDNS_OPENSCREEN"),
        None => std::env::var_os("ADB_MDNS_OPENSCREEN").is_some(),
    };
    if !asked {
        cmd.env("ADB_MDNS_OPENSCREEN", "1");
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::Builder::new()
            .name("adb output".into())
            .spawn(move || {
                let mut out = Vec::new();
                if let Some(mut stream) = stream {
                    let _ = stream.read_to_end(&mut out);
                }
                out
            })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + COMMAND_WAIT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("adb {} took too long", args.first().unwrap_or(&"")));
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    let mut said = String::new();
    for reader in [out, err].into_iter().flatten() {
        if let Ok(bytes) = reader.join() {
            said.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    if status.success() || args.first() == Some(&"pair") {
        // `adb pair` says why it failed; the caller reads it.
        Ok(said)
    } else {
        Err(last_line(&said))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_reads_as_android_scans_it() {
        let code = Code::new().expect("random bytes");
        let qr = code.qr();
        assert!(qr.starts_with("WIFI:T:ADB;S:hocuspanda-"), "{qr}");
        assert!(qr.ends_with(";;"));
        assert_eq!(Code::parse(&qr), Some(code.clone()));
        assert!(!format!("{code:?}").contains(&code.password));
        assert_ne!(Code::new().expect("random bytes"), code);
        assert_eq!(Code::parse("WIFI:T:WPA;S:home;P:secret;;"), None);
    }

    #[test]
    fn services_are_read_as_adb_lists_them() {
        let said = "List of discovered mdns services\n\
            hocuspanda-ab12cd34\t_adb-tls-pairing._tcp.\t192.168.1.20:37099\n\
            adb-1A2B3C-xYz\t_adb-tls-connect._tcp\t192.168.1.20:41231\n";
        let found = parse_services(said);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "hocuspanda-ab12cd34");
        assert!(found[0].kind.starts_with(PAIRING));
        assert_eq!(found[0].address, "192.168.1.20:37099");
        assert!(found[1].kind.starts_with(CONNECT));
        assert!(parse_services("* daemon not running; starting now at tcp:5037\n").is_empty());
    }

    #[test]
    fn devices_are_read_with_their_model() {
        let said = "List of devices attached\n\
            192.168.1.20:41231     device product:shiba model:Pixel_8 device:shiba transport_id:2\n\
            0A1B2C3D               unauthorized usb:1-2 transport_id:3\n\
            R58M12345              device usb:1-1 product:a52 model:SM_A525F transport_id:1\n";
        let devices = parse_devices(said);
        assert_eq!(
            devices,
            vec![
                Device {
                    serial: "192.168.1.20:41231".into(),
                    name: "Pixel 8".into(),
                    wireless: true,
                },
                Device {
                    serial: "R58M12345".into(),
                    name: "SM A525F".into(),
                    wireless: false,
                },
            ]
        );
    }

    #[test]
    fn a_cancelled_pairing_stops_asking_adb() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("a directory");
        // An adb that sees no phone, saying each time it was asked.
        let adb = dir.path().join("adb");
        std::fs::write(
            &adb,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$XDG_RUNTIME_DIR/asked\"\n\
             echo 'List of discovered mdns services'\n",
        )
        .expect("written");
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o755))
            .expect("made runnable");
        let env = Env {
            vars: Some(vec![("PATH".into(), "/usr/bin:/bin".into())]),
            runtime_dir: Some(dir.path().to_owned()),
        };
        let said = Arc::new(std::sync::Mutex::new(Vec::new()));
        let heard = said.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let code = Code::new().expect("random bytes");
        pair(1, adb, env, code, cancel.clone(), move |e| {
            if let Ok(mut said) = heard.lock() {
                said.push(e);
            }
        });
        let asked = || {
            std::fs::read_to_string(dir.path().join("asked"))
                .map(|t| t.lines().count())
                .unwrap_or(0)
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while asked() == 0 {
            assert!(Instant::now() < deadline, "adb was never asked");
            std::thread::sleep(Duration::from_millis(20));
        }
        cancel.store(true, Ordering::Relaxed);
        // One look may be under way; then none.
        std::thread::sleep(POLL * 3);
        let then = asked();
        std::thread::sleep(POLL * 3);
        assert_eq!(asked(), then, "adb still asked once cancelled");
        let said = said.lock().map(|s| s.clone()).unwrap_or_default();
        assert_eq!(
            said,
            vec![AdbEvent::Pairing {
                id: 1,
                stage: Stage::Waiting
            }],
            "nothing said once cancelled"
        );
    }

    #[test]
    fn the_paired_phone_is_known_by_its_guid_or_host() {
        let said = "Successfully paired to 192.168.1.20:37099 [guid=adb-1A2B3C-xYz]\n";
        assert_eq!(paired_guid(said).as_deref(), Some("adb-1A2B3C-xYz"));
        assert_eq!(host_of("192.168.1.20:37099"), "192.168.1.20");
        assert_eq!(host_of("[fe80::1]:5555"), "fe80::1");
        assert!(is_it("192.168.1.20:41231", None, "192.168.1.20"));
        assert!(is_it(
            "adb-1A2B3C-xYz._adb-tls-connect._tcp",
            Some("adb-1A2B3C-xYz"),
            "192.168.1.20"
        ));
        assert!(!is_it("R58M12345", Some("adb-1A2B3C-xYz"), "192.168.1.20"));
    }
}
