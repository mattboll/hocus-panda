//! The levels' services, faked privately, never the machine's: a D-Bus
//! daemon of its own serving UPower, power-profiles-daemon and logind
//! objects; a PipeWire of its own, in a short runtime directory of its own,
//! with null sinks (two by default, or those asked for), null sources if
//! asked for, applications' streams (`pw-cat` playing silence, linked to
//! the default sink, as WirePlumber would), and WirePlumber's part in
//! choosing the default sink and source (one chosen with `wpctl
//! set-default` becomes the default); and a backlight directory. The shell
//! is started through a script pointing it at them.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use anyhow::Context;
use zbus::{Connection, Variant, connection::Builder};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
pub const DEVICE: &str = "org.freedesktop.UPower.Device";
pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
pub const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// What the fake UPower and power-profiles-daemon say.
#[derive(Debug, Clone, PartialEq)]
pub struct Power {
    pub present: bool,
    pub percentage: f64,
    /// UPower's: 1 charging, 2 discharging, 4 full.
    pub state: u32,
    pub to_empty: i64,
    pub to_full: i64,
    pub profile: String,
}

impl Default for Power {
    fn default() -> Self {
        Self {
            present: true,
            percentage: 82.0,
            state: 2,
            to_empty: 220 * 60,
            to_full: 0,
            profile: "balanced".to_owned(),
        }
    }
}

type Shared = Arc<Mutex<Power>>;

fn power(shared: &Shared) -> Power {
    shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

struct Device(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl Device {
    #[zbus(property)]
    fn is_present(&self) -> bool {
        power(&self.0).present
    }
    #[zbus(property)]
    fn percentage(&self) -> f64 {
        power(&self.0).percentage
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        power(&self.0).state
    }
    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        power(&self.0).to_empty
    }
    #[zbus(property)]
    fn time_to_full(&self) -> i64 {
        power(&self.0).to_full
    }
}

struct Profiles(Shared);

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        power(&self.0).profile
    }
}

/// logind's session: `SetBrightness` writes the fake backlight.
struct Session {
    backlights: PathBuf,
    calls: Arc<Mutex<Vec<(String, String, u32)>>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    fn set_brightness(&self, subsystem: &str, name: &str, value: u32) -> zbus::fdo::Result<()> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((subsystem.to_owned(), name.to_owned(), value));
        std::fs::write(
            self.backlights.join(name).join("brightness"),
            format!("{value}\n"),
        )
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

/// The fakes, stopped when dropped.
pub struct Fakes {
    pub dir: tempfile::TempDir,
    runtime: tokio::runtime::Runtime,
    bus: Option<Child>,
    pub address: String,
    pub power: Shared,
    upower: Option<Connection>,
    profiles: Option<Connection>,
    login: Option<Connection>,
    pub brightness_calls: Arc<Mutex<Vec<(String, String, u32)>>>,
    pipewire: Option<Child>,
    /// The default sink's follower, WirePlumber's part.
    policy: Option<Child>,
    /// PipeWire's runtime directory: short, its socket's path has a limit.
    pw_runtime: tempfile::TempDir,
    /// The null sinks, the first the default; the sources, the same.
    sinks: Vec<FakeSink>,
    sources: Vec<FakeSink>,
    /// The streams playing, by node name.
    streams: Vec<(String, Child)>,
}

/// A null sink: its node's name, its description, and what it says it is
/// (`device.form-factor`), if anything.
#[derive(Debug, Clone)]
pub struct FakeSink {
    pub name: &'static str,
    pub description: &'static str,
    pub form_factor: Option<&'static str>,
}

impl FakeSink {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            form_factor: None,
        }
    }
}

/// The two sinks of the levels' tests.
pub const TWO_SINKS: [FakeSink; 2] = [
    FakeSink::new("hocus-test-sink", "Test"),
    FakeSink::new("hocus-test-other", "Other"),
];

/// Whether the programs the fakes need are installed.
pub fn available() -> bool {
    ["dbus-daemon", "pipewire", "pw-dump", "wpctl", "pw-metadata"]
        .iter()
        .all(|p| {
            Command::new("sh")
                .args(["-c", &format!("command -v {p}")])
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
}

impl Fakes {
    /// Everything there: a battery at 82 % with 3 h 40 left, balanced, a
    /// backlight at 70 %, and the default sink at 62 %.
    pub fn start() -> anyhow::Result<Self> {
        Self::start_with(&TWO_SINKS)
    }

    /// The same with these sinks, the first the default at 62 %.
    pub fn start_with(sinks: &[FakeSink]) -> anyhow::Result<Self> {
        Self::start_with_sources(sinks, &[])
    }

    /// The same with these sources too (microphones), the first the
    /// default.
    pub fn start_with_sources(sinks: &[FakeSink], sources: &[FakeSink]) -> anyhow::Result<Self> {
        let mut fakes = Self::empty()?;
        fakes.sinks = sinks.to_vec();
        fakes.sources = sources.to_vec();
        fakes.backlight(700, 1000)?;
        fakes.start_bus()?;
        fakes.start_upower()?;
        fakes.start_profiles()?;
        fakes.start_pipewire()?;
        let first = sinks.first().map_or("hocus-test-sink", |s| s.name);
        fakes.wpctl(&["set-volume", &fakes.sink(first)?.to_string(), "0.62"])?;
        Ok(fakes)
    }

    /// Nothing there yet.
    pub fn empty() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("backlight"))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        Ok(Self {
            address: format!("unix:path={}", dir.path().join("bus").display()),
            dir,
            runtime,
            bus: None,
            power: Shared::default(),
            upower: None,
            profiles: None,
            login: None,
            brightness_calls: Arc::default(),
            pipewire: None,
            policy: None,
            pw_runtime: tempfile::Builder::new().prefix("hp-").tempdir_in("/tmp")?,
            sinks: TWO_SINKS.to_vec(),
            sources: Vec::new(),
            streams: Vec::new(),
        })
    }

    pub fn backlights(&self) -> PathBuf {
        self.dir.path().join("backlight")
    }

    /// A raw backlight, `intel_backlight`, at `value` of `max`.
    /// It appears whole, as a device does: made aside, then moved in.
    pub fn backlight(&self, value: u32, max: u32) -> anyhow::Result<()> {
        let path = self.backlights().join("intel_backlight");
        if path.exists() {
            std::fs::write(path.join("brightness"), format!("{value}\n"))?;
            return Ok(());
        }
        let aside = self.dir.path().join("intel_backlight");
        std::fs::create_dir_all(&aside)?;
        std::fs::write(aside.join("type"), "raw\n")?;
        std::fs::write(aside.join("max_brightness"), format!("{max}\n"))?;
        std::fs::write(aside.join("brightness"), format!("{value}\n"))?;
        std::fs::rename(&aside, &path)?;
        Ok(())
    }

    pub fn remove_backlight(&self) -> anyhow::Result<()> {
        let path = self.backlights().join("intel_backlight");
        let aside = self.dir.path().join("removed_backlight");
        std::fs::rename(&path, &aside)?;
        std::fs::remove_dir_all(&aside)?;
        Ok(())
    }

    pub fn brightness(&self) -> Option<u32> {
        let path = self.backlights().join("intel_backlight/brightness");
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    /// A D-Bus daemon of its own, as the system bus.
    pub fn start_bus(&mut self) -> anyhow::Result<()> {
        let config = self.dir.path().join("bus.conf");
        let socket = self.dir.path().join("bus");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:path={}</listen>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )?;
        let bus = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("dbus-daemon")?;
        self.bus = Some(bus);
        wait_for(|| socket.exists(), "the bus's socket")?;
        let session = Session {
            backlights: self.backlights(),
            calls: self.brightness_calls.clone(),
        };
        self.login = Some(self.serve(
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            session,
        )?);
        Ok(())
    }

    fn serve<I: zbus::object_server::Interface>(
        &self,
        name: &str,
        path: &str,
        iface: I,
    ) -> anyhow::Result<Connection> {
        let address = self.address.clone();
        let (name, path) = (name.to_owned(), path.to_owned());
        self.runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(name)
                .serve_at(path, iface)
                .build()
                .await?;
            Ok(connection)
        })
    }

    pub fn start_upower(&mut self) -> anyhow::Result<()> {
        self.upower = Some(self.serve(UPOWER, DEVICE_PATH, Device(self.power.clone()))?);
        Ok(())
    }

    pub fn stop_upower(&mut self) {
        if let Some(connection) = self.upower.take() {
            self.runtime.block_on(async move { drop(connection) });
        }
    }

    pub fn start_profiles(&mut self) -> anyhow::Result<()> {
        self.profiles = Some(self.serve(PROFILES, PROFILES_PATH, Profiles(self.power.clone()))?);
        Ok(())
    }

    /// Changes the battery or the profile, and says so as the services
    /// do.
    pub fn set_power(&self, change: impl FnOnce(&mut Power)) -> anyhow::Result<()> {
        change(&mut self.power.lock().unwrap_or_else(PoisonError::into_inner));
        for (connection, path, iface) in [
            (&self.upower, DEVICE_PATH, DEVICE),
            (&self.profiles, PROFILES_PATH, PROFILES),
        ] {
            let Some(connection) = connection.clone() else {
                continue;
            };
            self.runtime.block_on(async move {
                connection
                    .emit_signal(
                        None::<&str>,
                        path,
                        "org.freedesktop.DBus.Properties",
                        "PropertiesChanged",
                        &(
                            iface,
                            HashMap::<&str, Variant<'_>>::new(),
                            Vec::<&str>::new(),
                        ),
                    )
                    .await
            })?;
        }
        Ok(())
    }

    /// A PipeWire of its own, with its null sinks, the first the default.
    pub fn start_pipewire(&mut self) -> anyhow::Result<()> {
        let conf = self.dir.path().join("pw-config/pipewire/pipewire.conf.d");
        std::fs::create_dir_all(&conf)?;
        let first = self.sinks.first().map_or("hocus-test-sink", |s| s.name);
        let source = self
            .sources
            .first()
            .map(|s| {
                format!(
                    "{{ key = default.audio.source type = \"Spa:String:JSON\" value = {{ name = {} }} }}",
                    s.name
                )
            })
            .unwrap_or_default();
        let mut objects = format!(
            "context.objects = [\n\
             {{ factory = metadata args = {{ metadata.name = default metadata.values = [\n\
               {{ key = default.audio.sink type = \"Spa:String:JSON\" value = {{ name = {first} }} }} {source} ] }} }}\n"
        );
        let classes = (self.sinks.iter().map(|s| (s, "Audio/Sink")))
            .chain(self.sources.iter().map(|s| (s, "Audio/Source")));
        for (sink, class) in classes {
            let form = sink
                .form_factor
                .map(|f| format!(" device.form-factor = {f}"))
                .unwrap_or_default();
            // Its ports made at once, as WirePlumber would: streams link
            // to them.
            objects.push_str(&format!(
                "{{ factory = adapter args = {{ factory.name = support.null-audio-sink node.name = {}\n\
                   node.description = \"{}\"{form} media.class = {class} audio.position = [ FL FR ] \
                   object.linger = true adapter.auto-port-config = {{ mode = dsp position = preserve }} }} }}\n",
                sink.name, sink.description
            ));
        }
        objects.push_str("]\n");
        std::fs::write(conf.join("hocus-test.conf"), objects)?;
        let home = self.dir.path().join("pw-home");
        std::fs::create_dir_all(&home)?;
        let pipewire = Command::new("pipewire")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", self.pw_runtime.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("pw-config"))
            // No session bus, nor the system's.
            .env("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("pipewire")?;
        self.pipewire = Some(pipewire);
        let socket = self.pw_socket();
        wait_for(|| socket.exists(), "PipeWire's socket")?;
        wait_for(|| self.sink(first).is_ok(), "the test sink")?;
        self.start_policy()?;
        Ok(())
    }

    /// WirePlumber's part: `wpctl set-default` writes the sink or source
    /// chosen (`default.configured.audio.sink`, `….source`), and
    /// WirePlumber makes it the default (`default.audio.sink`, `….source`).
    /// Here a `pw-metadata` follows the first, and the second is set when
    /// it changes.
    fn start_policy(&mut self) -> anyhow::Result<()> {
        use std::io::BufRead;
        let mut monitor = self
            .pw("pw-metadata")
            .args(["-m", "-n", "default"])
            .stdout(Stdio::piped())
            .spawn()
            .context("pw-metadata")?;
        let out = monitor.stdout.take().context("pw-metadata's output")?;
        let (home, runtime, socket) = (
            self.dir.path().join("pw-home"),
            self.pw_runtime.path().to_owned(),
            self.pw_socket(),
        );
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines() {
                let Ok(line) = line else {
                    return;
                };
                let Some(key) = ["sink", "source"]
                    .into_iter()
                    .find(|k| line.contains(&format!("key:'default.configured.audio.{k}'")))
                else {
                    continue;
                };
                let Some(value) = line
                    .split("value:'")
                    .nth(1)
                    .and_then(|v| v.split('\'').next())
                else {
                    continue;
                };
                let pw = |program: &str| {
                    let mut command = Command::new(program);
                    command
                        .env_clear()
                        .env("PATH", "/usr/bin:/bin")
                        .env("HOME", &home)
                        .env("XDG_RUNTIME_DIR", &runtime)
                        .env("PIPEWIRE_REMOTE", &socket)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    command
                };
                let _ = pw("pw-metadata")
                    .args(["-n", "default", "0", &format!("default.audio.{key}"), value])
                    .arg("Spa:String:JSON")
                    .status();
                // PipeWire 1.0's pw-dump writes a metadata change out with
                // the graph's next change only: a client connecting is one.
                let _ = pw("wpctl").arg("status").status();
            }
        });
        self.policy = Some(monitor);
        Ok(())
    }

    /// An application's stream playing silence: node `node` of
    /// application `app` (and its icon's name), linked to the default
    /// sink; once it runs, its node's id.
    pub fn play(&mut self, node: &str, app: &str, icon: Option<&str>) -> anyhow::Result<u32> {
        let icon = icon
            .map(|i| format!(" application.icon-name = \"{i}\""))
            .unwrap_or_default();
        let props = format!(
            "{{ node.name = {node} application.name = \"{app}\"{icon} \
             adapter.auto-port-config = {{ mode = dsp }} }}"
        );
        let child = self
            .pw("pw-cat")
            .args(["--playback", "--raw", "-P", &props, "-"])
            .stdin(std::fs::File::open("/dev/zero")?)
            .stdout(Stdio::null())
            .spawn()
            .context("pw-cat")?;
        self.streams.push((node.to_owned(), child));
        wait_for(
            || self.ports(node, "output").len() == 2,
            "the stream's ports",
        )?;
        self.link(node, true)?;
        let id = self.sink(node)?;
        wait_for(
            || self.state(id).as_deref() == Some("running"),
            "the stream running",
        )?;
        Ok(id)
    }

    /// Stream `node` paused, unlinked from its sink (it goes idle), or
    /// playing again.
    pub fn pause(&self, node: &str, paused: bool) -> anyhow::Result<()> {
        self.link(node, !paused)?;
        let id = self.sink(node)?;
        wait_for(
            || (self.state(id).as_deref() == Some("running")) != paused,
            "the stream's new state",
        )
    }

    /// Stream `node` ends.
    pub fn stop(&mut self, node: &str) {
        if let Some(at) = self.streams.iter().position(|(n, _)| n == node) {
            let (_, mut child) = self.streams.remove(at);
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Links stream `node`'s ports to the first sink's (or unlinks them).
    fn link(&self, node: &str, linked: bool) -> anyhow::Result<()> {
        let sink = self.sinks.first().map_or("hocus-test-sink", |s| s.name);
        for (out, sink_port) in self
            .ports(node, "output")
            .iter()
            .zip(self.ports(sink, "playback").iter())
        {
            let mut command = self.pw("pw-link");
            if !linked {
                command.arg("-d");
            }
            let status = command
                .args([out, sink_port])
                .stdout(Stdio::null())
                .status()?;
            anyhow::ensure!(status.success(), "pw-link {out} {sink_port}: {status}");
        }
        Ok(())
    }

    /// Node `node`'s ports whose names start with `prefix`, in order.
    fn ports(&self, node: &str, prefix: &str) -> Vec<String> {
        let Ok(out) = self.pw("pw-link").args(["-o", "-i"]).output() else {
            return Vec::new();
        };
        let mut ports: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| {
                l.strip_prefix(node)
                    .and_then(|l| l.strip_prefix(':'))
                    .is_some_and(|port| port.starts_with(prefix))
            })
            .map(str::to_owned)
            .collect();
        ports.sort();
        ports.dedup();
        ports
    }

    /// Node `id`'s state (`running`, `idle`…).
    pub fn state(&self, id: u32) -> Option<String> {
        let out = self.pw("pw-dump").arg(id.to_string()).output().ok()?;
        let objects: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
        objects
            .iter()
            .find(|o| o.get("id").and_then(serde_json::Value::as_u64) == Some(u64::from(id)))?
            .pointer("/info/state")?
            .as_str()
            .map(str::to_owned)
    }

    pub fn stop_pipewire(&mut self) {
        let streams = self.streams.drain(..).map(|(_, child)| child);
        for mut child in streams.chain(
            [self.policy.take(), self.pipewire.take()]
                .into_iter()
                .flatten(),
        ) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// The default sink's node name, as PipeWire's metadata says.
    pub fn default_sink_name(&self) -> anyhow::Result<String> {
        let out = self
            .pw("pw-metadata")
            .args(["-n", "default", "0", "default.audio.sink"])
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let value = text
            .lines()
            .find(|l| l.contains("key:'default.audio.sink'"))
            .and_then(|l| l.split("value:'").nth(1))
            .and_then(|v| v.split('\'').next())
            .context("no default sink")?;
        // `{"name":"…"}`, or SPA's `{ name = … }`.
        let name = value
            .trim_matches(|c| c == '{' || c == '}' || c == ' ')
            .trim_start_matches("\"name\"")
            .trim_start_matches("name")
            .trim_start_matches([':', '=', ' '])
            .trim_matches('"');
        Ok(name.to_owned())
    }

    pub fn pw_socket(&self) -> PathBuf {
        self.pw_runtime.path().join("pipewire-0")
    }

    fn pw(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.path().join("pw-home"))
            .env("XDG_RUNTIME_DIR", self.pw_runtime.path())
            .env("PIPEWIRE_REMOTE", self.pw_socket())
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    pub fn wpctl(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = self.pw("wpctl").args(args).output()?;
        anyhow::ensure!(out.status.success(), "wpctl {args:?}: {}", out.status);
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// A sink's node id, by name.
    pub fn sink(&self, name: &str) -> anyhow::Result<u32> {
        let out = self.pw("pw-dump").output()?;
        // Older pw-dumps may write the graph in several arrays.
        let mut objects = Vec::new();
        for array in
            serde_json::Deserializer::from_slice(&out.stdout).into_iter::<Vec<serde_json::Value>>()
        {
            objects.extend(array?);
        }
        objects
            .iter()
            .find(|o| o.pointer("/info/props/node.name").and_then(|n| n.as_str()) == Some(name))
            .and_then(|o| o.get("id")?.as_u64())
            .and_then(|id| u32::try_from(id).ok())
            .context("no such sink")
    }

    /// A sink's volume as `wpctl` shows it, and whether it is muted.
    pub fn volume(&self, name: &str) -> anyhow::Result<(f32, bool)> {
        let line = self.wpctl(&["get-volume", &self.sink(name)?.to_string()])?;
        let mut words = line.split_whitespace().skip(1);
        let volume: f32 = words.next().context("no volume")?.parse()?;
        Ok((volume, words.next() == Some("[MUTED]")))
    }

    /// Makes `name` the default sink, as WirePlumber would.
    pub fn default_sink(&self, name: &str) -> anyhow::Result<()> {
        let value = format!("{{\"name\":\"{name}\"}}");
        let status = self
            .pw("pw-metadata")
            .args([
                "-n",
                "default",
                "0",
                "default.audio.sink",
                &value,
                "Spa:String:JSON",
            ])
            .stdout(Stdio::null())
            .status()?;
        anyhow::ensure!(status.success(), "pw-metadata: {status}");
        // PipeWire 1.0's pw-dump (CI's) writes a metadata change out with
        // the graph's next change only: a client connecting is one.
        self.wpctl(&["status"])?;
        Ok(())
    }

    /// A script starting `shell` against these fakes, in `lang`.
    pub fn shell(&self, shell: &Path, lang: &str) -> anyhow::Result<PathBuf> {
        let script = self.dir.path().join("shell");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{}' PIPEWIRE_REMOTE='{}' \
                 HOCUS_BACKLIGHT_DIR='{}' LC_ALL='{lang}' LANG='{lang}' \
                 RUST_LOG=${{HOCUS_SHELL_LOG:-hocuspanda_shell=debug}}\n\
                 exec '{}' \"$@\"\n",
                self.address,
                self.pw_socket().display(),
                self.backlights().display(),
                shell.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        Ok(script)
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        self.stop_pipewire();
        let connections = [self.upower.take(), self.profiles.take(), self.login.take()];
        self.runtime.block_on(async move { drop(connections) });
        if let Some(mut bus) = self.bus.take() {
            let _ = bus.kill();
            let _ = bus.wait();
        }
    }
}

fn wait_for(mut done: impl FnMut() -> bool, what: &str) -> anyhow::Result<()> {
    let start = Instant::now();
    while !done() {
        anyhow::ensure!(start.elapsed() < Duration::from_secs(10), "no {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
