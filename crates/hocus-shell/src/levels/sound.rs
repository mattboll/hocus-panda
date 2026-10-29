//! The sound: the outputs (sinks), the inputs (sources), the streams
//! applications play, and the default output's volume and mute, from
//! PipeWire.
//!
//! `pw-dump --monitor` (PipeWire's own tool) streams the graph's changes as
//! JSON: the `default` metadata names the default sink and source, each
//! sink's or source's node its description and its `Props` (volumes,
//! mute), its device what it is (a headset, a screen), each stream's node
//! its application, its state and its own volume. No polling: it writes
//! when something changes, and the shell follows a new default sink as it
//! follows a new volume or a stream starting to play. `wpctl`
//! (WirePlumber's) sets them as the user would: a volume, a mute, the
//! default sink or source, each keeping its own volume. Both run as
//! programs, on threads of their own; linking to libpipewire would need its
//! headers to build, for the same result.
//!
//! Without PipeWire (or its tools), the sound's row is hidden and the
//! shell tries again later each time, up to a minute: PipeWire announces
//! nothing when it starts.

use std::{
    collections::HashMap,
    io::BufReader,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use hocus_link::LevelKey;
use serde_json::Value;

use super::{
    Presence, RETRY_FIRST,
    icons::Icon,
    model::{STEP, Sound},
    next_retry, publish,
};

/// A `pw-dump` that ran this long before ending did not fail to start:
/// the next try comes at once.
const STEADY: Duration = Duration::from_secs(60);

/// The default sink as last read: its node's id, and whether it is muted.
type Default = Arc<Mutex<Option<(u32, bool)>>>;

pub fn start(keys: Receiver<Ask>) {
    let default: Default = Arc::default();
    let reader = default.clone();
    let started = std::thread::Builder::new()
        .name("levels: sound".to_owned())
        .spawn(move || follow(&reader))
        .and_then(|_| {
            std::thread::Builder::new()
                .name("levels: sound keys".to_owned())
                .spawn(move || set(&keys, &default))
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow the sound");
    }
}

/// Follows the graph, starting `pw-dump` again whenever it ends.
fn follow(default: &Default) {
    let mut presence = Presence::Unknown;
    let mut wait = RETRY_FIRST;
    loop {
        let started = Instant::now();
        let ended = dump(default, &mut presence);
        *default.lock().unwrap_or_else(PoisonError::into_inner) = None;
        publish(|levels| levels.sound = None);
        publish_mixer(Mixer::default());
        presence.now(false, "PipeWire", ended);
        if started.elapsed() >= STEADY {
            wait = RETRY_FIRST;
        }
        std::thread::sleep(wait);
        wait = next_retry(wait);
    }
}

/// Runs `pw-dump --monitor` until it ends, showing the default sink as it
/// changes; why it ended.
fn dump(default: &Default, presence: &mut Presence) -> String {
    let child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return format!("pw-dump: {err}"),
    };
    let Some(out) = child.stdout.take() else {
        return "pw-dump: no output".to_owned();
    };
    *DUMP.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    let mut graph = Graph::default();
    let mut icons = HashMap::new();
    let updates =
        serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Vec<Value>>();
    for update in updates {
        let update = match update {
            Ok(update) => update,
            Err(err) => {
                tracing::warn!(%err, "pw-dump wrote what does not read");
                break;
            }
        };
        for object in update {
            graph.apply(object);
        }
        let sound = graph.default_sink();
        if sound.is_some() {
            presence.now(true, "PipeWire", "");
        }
        *default.lock().unwrap_or_else(PoisonError::into_inner) =
            sound.map(|(id, sound)| (id, sound.muted));
        publish(|levels| levels.sound = sound.map(|(_, sound)| sound));
        publish_mixer(graph.mixer(&mut icons));
    }
    match end_dump() {
        Some(Ok(status)) => format!("pw-dump ended: {status}"),
        Some(Err(err)) => format!("pw-dump: {err}"),
        None => "the shell ends".to_owned(),
    }
}

/// The `pw-dump` running: ended with the shell, which it would otherwise
/// outlive until PipeWire's next change.
static DUMP: Mutex<Option<Child>> = Mutex::new(None);

fn end_dump() -> Option<std::io::Result<std::process::ExitStatus>> {
    let mut child = DUMP.lock().unwrap_or_else(PoisonError::into_inner).take()?;
    let _ = child.kill();
    Some(child.wait())
}

/// The shell ends: its `pw-dump` too.
pub fn stop() {
    end_dump();
}

/// The outputs, the inputs and the applications as last read, for the
/// sound's popover.
static MIXER: Mutex<Mixer> = Mutex::new(Mixer {
    sinks: Vec::new(),
    sources: Vec::new(),
    apps: Vec::new(),
});

/// The outputs, the inputs and the applications as last read.
pub fn mixer() -> Mixer {
    MIXER.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The mixer now `mixer`: the popover shows the change.
fn publish_mixer(mixer: Mixer) {
    let changed = {
        let mut kept = MIXER.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *kept != mixer;
        *kept = mixer;
        changed
    };
    if changed {
        crate::popover::changed();
    }
}

/// What the sound's popover lists, each by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mixer {
    pub sinks: Vec<Sink>,
    /// The microphones (sources), as the sinks.
    pub sources: Vec<Sink>,
    /// The applications with a stream, in the order their first stream
    /// came.
    pub apps: Vec<App>,
}

impl Mixer {
    /// The microphone the popover's slider sets: the default source, else
    /// the first.
    pub fn microphone(&self) -> Option<&Sink> {
        self.sources
            .iter()
            .find(|s| s.default)
            .or(self.sources.first())
    }
}

/// An output or an input, as the popover lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub description: String,
    pub kind: Kind,
    /// Its own volume and mute: each keeps its own.
    pub sound: Option<Sound>,
    pub default: bool,
}

/// An application playing sound, its streams taken together.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    /// What its streams share: its sandbox's app id, else its name, else
    /// its program.
    pub key: String,
    pub name: String,
    pub icon: Option<Icon>,
    /// Its streams' node ids, the oldest first.
    pub streams: Vec<u32>,
    /// Its oldest stream's volume; muted when all its streams are.
    pub sound: Option<Sound>,
    /// Whether one of its streams plays now: its node runs, not corked.
    pub playing: bool,
}

/// What an output is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Speaker,
    Headphones,
    Screen,
}

impl Kind {
    /// From its device's form factor and bus (`device.form-factor`,
    /// `device.bus`), and its node's name; a speaker when nothing says.
    fn of(form_factor: Option<&str>, bus: Option<&str>, name: &str) -> Self {
        match form_factor {
            Some("headset" | "headphone" | "hands-free" | "handset") => return Self::Headphones,
            Some("tv" | "monitor" | "hdmi") => return Self::Screen,
            Some(_) => return Self::Speaker,
            None => {}
        }
        let name = name.to_ascii_lowercase();
        if name.contains("hdmi") || name.contains("displayport") {
            Self::Screen
        } else if bus == Some("bluetooth") {
            Self::Headphones
        } else {
            Self::Speaker
        }
    }
}

/// What the shell keeps of PipeWire's graph.
#[derive(Debug, Default)]
pub struct Graph {
    /// The default sink's and source's names, from the `default` metadata.
    sink: Option<String>,
    source: Option<String>,
    /// The `default` metadata's id, to see it go.
    metadata: Option<u32>,
    /// Audio sinks, sources and output streams by id.
    nodes: HashMap<u32, Node>,
    /// Devices by id: what they are.
    devices: HashMap<u32, Device>,
}

/// What a node is, from its `media.class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Class {
    #[default]
    Sink,
    Source,
    /// An application's output stream (a "sink input").
    Stream,
}

impl Class {
    fn of(media_class: &str) -> Option<Self> {
        match media_class {
            "Audio/Sink" => Some(Self::Sink),
            "Audio/Source" | "Audio/Source/Virtual" => Some(Self::Source),
            "Stream/Output/Audio" => Some(Self::Stream),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct Node {
    class: Class,
    name: String,
    description: Option<String>,
    device: Option<u32>,
    /// What its own props say it is, if they do.
    what: Device,
    sound: Option<Sound>,
    /// A stream's application, and whether it plays: its node runs
    /// (`info.state`), and PulseAudio's clients did not cork it.
    app: Who,
    running: bool,
    corked: bool,
}

/// Who plays a stream, from its node's props.
#[derive(Debug, Default, Clone)]
struct Who {
    name: Option<String>,
    binary: Option<String>,
    icon: Option<String>,
    /// `application.id`, and a sandbox's (Flatpak's) app id: desktop
    /// files' names.
    id: Option<String>,
    sandboxed: Option<String>,
}

impl Who {
    fn read(props: &Value) -> Self {
        let text = |key: &str| {
            props
                .get(key)
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        Self {
            name: text("application.name"),
            binary: text("application.process.binary"),
            icon: text("application.icon-name"),
            id: text("application.id"),
            sandboxed: text("pipewire.access.portal.app_id"),
        }
    }

    /// What groups the streams of one application.
    fn key(&self, node: &str) -> String {
        self.sandboxed
            .clone()
            .or_else(|| self.name.clone())
            .or_else(|| self.binary.clone())
            .unwrap_or_else(|| node.to_owned())
    }

    /// The names its icon may go by, the likeliest first: its icon's name,
    /// then its desktop file's (its ids, its program).
    fn icon_names(&self) -> Vec<String> {
        [&self.icon, &self.sandboxed, &self.id, &self.binary]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

/// `device.form-factor` and `device.bus`.
#[derive(Debug, Default, Clone)]
struct Device {
    form_factor: Option<String>,
    bus: Option<String>,
}

impl Device {
    fn read(props: Option<&Value>) -> Self {
        let text = |key: &str| {
            props
                .and_then(|p| p.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        Self {
            form_factor: text("device.form-factor"),
            bus: text("device.bus"),
        }
    }
}

impl Graph {
    /// One object of `pw-dump`'s output: added, changed, or gone (with no
    /// type, or no info).
    pub fn apply(&mut self, object: Value) {
        let Some(id) = object
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok())
        else {
            return;
        };
        let gone = object.get("info").is_some_and(Value::is_null)
            || object.get("metadata").is_some_and(Value::is_null)
            || object.get("type").is_none();
        if gone {
            self.nodes.remove(&id);
            self.devices.remove(&id);
            if self.metadata == Some(id) {
                self.metadata = None;
                self.sink = None;
                self.source = None;
            }
            return;
        }
        match object.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Metadata") => self.metadata(id, &object),
            Some("PipeWire:Interface:Node") => self.node(id, &object),
            Some("PipeWire:Interface:Device") => {
                // An update without props keeps those known.
                if let Some(props) = object.pointer("/info/props") {
                    self.devices.insert(id, Device::read(Some(props)));
                }
            }
            _ => {}
        }
    }

    fn metadata(&mut self, id: u32, object: &Value) {
        let name = object
            .pointer("/props/metadata.name")
            .and_then(Value::as_str);
        if name != Some("default") {
            return;
        }
        self.metadata = Some(id);
        let Some(entries) = object.get("metadata").and_then(Value::as_array) else {
            return;
        };
        let value = |key: &str| {
            entries
                .iter()
                .filter(|e| e.get("subject").and_then(Value::as_u64) == Some(0))
                .find(|e| e.get("key").and_then(Value::as_str) == Some(key))
                .and_then(|e| e.get("value"))
                .and_then(sink_name)
        };
        self.sink = value("default.audio.sink");
        self.source = value("default.audio.source");
    }

    fn node(&mut self, id: u32, object: &Value) {
        let Some(info) = object.get("info") else {
            return;
        };
        let props = info.get("props");
        let class = match props
            .and_then(|p| p.get("media.class"))
            .and_then(Value::as_str)
        {
            Some(media_class) => Class::of(media_class),
            // An update without props: the class known.
            None => self.nodes.get(&id).map(|n| n.class),
        };
        let Some(class) = class else {
            return;
        };
        if class == Class::Source && props.is_some_and(is_monitor) {
            return;
        }
        let node = self.nodes.entry(id).or_default();
        node.class = class;
        if let Some(props) = props {
            if let Some(name) = props.get("node.name").and_then(Value::as_str) {
                name.clone_into(&mut node.name);
            }
            node.description = ["node.description", "node.nick"]
                .iter()
                .find_map(|key| props.get(*key).and_then(Value::as_str))
                .map(str::to_owned);
            node.device = props
                .get("device.id")
                .and_then(Value::as_u64)
                .and_then(|d| u32::try_from(d).ok());
            node.what = Device::read(Some(props));
            node.app = Who::read(props);
            node.corked = props
                .get("pulse.corked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        if let Some(state) = info.get("state").and_then(Value::as_str) {
            node.running = state == "running";
        }
        // An update without the params keeps those known.
        if let Some(props) = info.pointer("/params/Props").and_then(Value::as_array) {
            node.sound = props.iter().find_map(sound_of);
        }
    }

    /// The sinks, by id: what they are called, what they are, their own
    /// sound, and which is the default.
    pub fn sinks(&self) -> Vec<Sink> {
        self.devices_of(Class::Sink, self.sink.as_deref())
    }

    /// The sources, as the sinks.
    pub fn sources(&self) -> Vec<Sink> {
        self.devices_of(Class::Source, self.source.as_deref())
    }

    fn devices_of(&self, class: Class, default: Option<&str>) -> Vec<Sink> {
        let mut sinks: Vec<Sink> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.class == class)
            .map(|(id, node)| {
                let device = node.device.and_then(|d| self.devices.get(&d));
                let form_factor = node
                    .what
                    .form_factor
                    .as_deref()
                    .or(device.and_then(|d| d.form_factor.as_deref()));
                let bus = node
                    .what
                    .bus
                    .as_deref()
                    .or(device.and_then(|d| d.bus.as_deref()));
                Sink {
                    id: *id,
                    description: node
                        .description
                        .clone()
                        .unwrap_or_else(|| node.name.clone()),
                    kind: Kind::of(form_factor, bus, &node.name),
                    sound: node.sound,
                    default: default == Some(node.name.as_str()),
                }
            })
            .collect();
        sinks.sort_by_key(|s| s.id);
        sinks
    }

    /// The applications with a stream, each with the names its icon may go
    /// by; their icons not looked for.
    fn apps(&self) -> Vec<(App, Vec<String>)> {
        let mut streams: Vec<(&u32, &Node)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.class == Class::Stream)
            .collect();
        streams.sort_by_key(|(id, _)| **id);
        let mut apps: Vec<(App, Vec<String>)> = Vec::new();
        for (id, node) in streams {
            let key = node.app.key(&node.name);
            let playing = node.running && !node.corked;
            match apps.iter_mut().find(|(app, _)| app.key == key) {
                Some((app, _)) => {
                    app.streams.push(*id);
                    app.playing |= playing;
                    match (&mut app.sound, node.sound) {
                        (Some(sound), Some(this)) => sound.muted &= this.muted,
                        (sound @ None, this) => *sound = this,
                        (Some(_), None) => {}
                    }
                }
                None => {
                    let name = node
                        .app
                        .name
                        .clone()
                        .or_else(|| node.app.binary.clone())
                        .unwrap_or_else(|| node.name.clone());
                    let app = App {
                        key,
                        name,
                        icon: None,
                        streams: vec![*id],
                        sound: node.sound,
                        playing,
                    };
                    apps.push((app, node.app.icon_names()));
                }
            }
        }
        apps
    }

    /// The sinks, the sources and the applications, their icons looked for
    /// once each (`icons`, by application).
    fn mixer(&self, icons: &mut HashMap<String, Option<Icon>>) -> Mixer {
        let apps = self
            .apps()
            .into_iter()
            .map(|(mut app, names)| {
                app.icon = icons
                    .entry(app.key.clone())
                    .or_insert_with(|| {
                        let icon = super::icons::find(&names);
                        tracing::debug!(
                            app = app.key,
                            ?names,
                            found = icon.is_some(),
                            "an application's icon"
                        );
                        icon
                    })
                    .clone();
                app
            })
            .collect();
        Mixer {
            sinks: self.sinks(),
            sources: self.sources(),
            apps,
        }
    }

    /// The default sink's node id and its sound.
    pub fn default_sink(&self) -> Option<(u32, Sound)> {
        let name = self.sink.as_deref()?;
        self.nodes
            .iter()
            .filter(|(_, node)| node.class == Class::Sink && node.name == name)
            .find_map(|(id, node)| node.sound.map(|sound| (*id, sound)))
    }
}

/// A sink's monitor made a source: not a microphone.
fn is_monitor(props: &Value) -> bool {
    props.get("device.class").and_then(Value::as_str) == Some("monitor")
        || props
            .get("node.name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.ends_with(".monitor"))
}

/// `{"name": "…"}`, as WirePlumber writes it, or the same as a string.
fn sink_name(value: &Value) -> Option<String> {
    let parsed;
    let value = match value {
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text).ok()?;
            &parsed
        }
        other => other,
    };
    value.get("name")?.as_str().map(str::to_owned)
}

/// A `Props` object's volume (as `wpctl` shows it: the cube root of the
/// channels' mean linear volume) and mute.
fn sound_of(props: &Value) -> Option<Sound> {
    let channels: Vec<f64> = props
        .get("channelVolumes")?
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    if channels.is_empty() {
        return None;
    }
    let mean = channels.iter().sum::<f64>() / channels.len() as f64;
    Some(Sound {
        volume: (mean.max(0.0).cbrt() * 100.0) as f32,
        muted: props.get("mute").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// What the shell asks of the sound: a key's step or mute, a volume set
/// (the popover's slider, in percent), another default sink (by node id);
/// a volume or a mute for a node of its own (a microphone, an
/// application's stream), another default source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ask {
    Key(LevelKey),
    Volume(f32),
    Default(u32),
    Node(u32, Set),
    DefaultSource(u32),
}

/// What is set on a node of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Set {
    /// In percent.
    Volume(f32),
    Mute(bool),
}

/// What was asked so far, taken together: a volume set and the steps after
/// it (up less down), whether the volume was touched (which unmutes),
/// whether to toggle mute, and another default sink; the last volume and
/// mute asked of each other node, and another default source.
#[derive(Debug, Default, PartialEq)]
pub struct Asked {
    pub steps: i32,
    pub volume: bool,
    pub toggle: bool,
    pub to: Option<f32>,
    pub default: Option<u32>,
    pub nodes: std::collections::BTreeMap<u32, (Option<f32>, Option<bool>)>,
    pub source: Option<u32>,
}

impl Asked {
    pub fn add(&mut self, ask: Ask) {
        let key = match ask {
            Ask::Key(key) => key,
            Ask::Volume(to) => {
                self.to = Some(to.clamp(0.0, 100.0));
                self.steps = 0;
                self.volume = true;
                return;
            }
            Ask::Default(id) => {
                self.default = Some(id);
                return;
            }
            Ask::Node(id, set) => {
                let node = self.nodes.entry(id).or_default();
                match set {
                    Set::Volume(to) => node.0 = Some(to.clamp(0.0, 100.0)),
                    Set::Mute(muted) => node.1 = Some(muted),
                }
                return;
            }
            Ask::DefaultSource(id) => {
                self.source = Some(id);
                return;
            }
        };
        match key {
            LevelKey::VolumeUp => {
                self.steps += 1;
                self.volume = true;
            }
            LevelKey::VolumeDown => {
                self.steps -= 1;
                self.volume = true;
            }
            LevelKey::Mute => self.toggle = !self.toggle,
            LevelKey::BrightnessUp | LevelKey::BrightnessDown => {}
        }
    }

    /// Whether something was asked of the default sink as it is, which
    /// another default sink asked after must not take.
    fn for_the_default(&self) -> bool {
        self.default.is_none() && (self.volume || self.toggle)
    }

    /// Whether something was asked of the default sink, or of another to
    /// be.
    fn for_a_sink(&self) -> bool {
        self.default.is_some() || self.volume || self.toggle
    }

    /// `wpctl`'s commands for the other nodes: their volumes, then their
    /// mutes; another default source.
    pub fn node_commands(&self) -> Vec<Vec<String>> {
        let mut commands = Vec::new();
        if let Some(id) = self.source {
            commands.push(vec!["set-default".into(), id.to_string()]);
        }
        for (id, (volume, muted)) in &self.nodes {
            if let Some(to) = volume {
                commands.push(vec![
                    "set-volume".into(),
                    id.to_string(),
                    format!("{:.3}", to / 100.0),
                ]);
            }
            if let Some(muted) = muted {
                let muted = if *muted { "1" } else { "0" };
                commands.push(vec!["set-mute".into(), id.to_string(), muted.into()]);
            }
        }
        commands
    }

    /// `wpctl`'s commands for node `id` (the default sink, or the one
    /// asked to be), muted or not now.
    pub fn commands(&self, id: u32, muted: bool) -> Vec<Vec<String>> {
        let id = id.to_string();
        let mut commands = Vec::new();
        if self.default.is_some() {
            commands.push(vec!["set-default".into(), id.clone()]);
        }
        if let Some(to) = self.to {
            commands.push(vec![
                "set-volume".into(),
                id.clone(),
                format!("{:.3}", to / 100.0),
            ]);
        }
        if self.steps != 0 {
            let percent = STEP as i32 * self.steps.abs();
            let sign = if self.steps > 0 { '+' } else { '-' };
            commands.push(vec![
                "set-volume".into(),
                "--limit".into(),
                "1.0".into(),
                id.clone(),
                format!("{percent}%{sign}"),
            ]);
        }
        // A volume key unmutes; mute toggles (after it, if both).
        let unmute = self.volume && muted;
        match (unmute, self.toggle) {
            (true, false) => commands.push(vec!["set-mute".into(), id, "0".into()]),
            (false, true) => commands.push(vec!["set-mute".into(), id, "toggle".into()]),
            _ => {}
        }
        commands
    }
}

/// `first` and what was asked while the last commands ran, taken
/// together; and another default sink asked after something for the one
/// before, which waits for the next batch. `None`: the shell ends.
fn batch(first: Ask, asks: &Receiver<Ask>) -> Option<(Asked, Option<Ask>)> {
    let mut asked = Asked::default();
    asked.add(first);
    loop {
        match asks.try_recv() {
            Ok(ask @ Ask::Default(_)) if asked.for_the_default() => {
                return Some((asked, Some(ask)));
            }
            Ok(ask) => asked.add(ask),
            Err(TryRecvError::Empty) => return Some((asked, None)),
            Err(TryRecvError::Disconnected) => return None,
        }
    }
}

/// Sets the volume, the mute and the default sink as asked, what was
/// asked while the last commands ran taken together: a slider dragged
/// sends many volumes, and only the last goes to PipeWire.
fn set(asks: &Receiver<Ask>, default: &Default) {
    // An ask taken off the channel for the next batch: another default
    // sink, after asks for the one before.
    let mut next = None;
    loop {
        let ask = match next.take() {
            Some(ask) => ask,
            None => match asks.recv() {
                Ok(ask) => ask,
                Err(_) => return,
            },
        };
        let Some((asked, after)) = batch(ask, asks) else {
            return;
        };
        next = after;
        let mut commands = Vec::new();
        if asked.for_a_sink() {
            let current = *default.lock().unwrap_or_else(PoisonError::into_inner);
            let target = match (asked.default, current) {
                (Some(id), _) => mixer()
                    .sinks
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| (id, s.sound.is_some_and(|s| s.muted))),
                (None, current) => current,
            };
            match target {
                Some((id, muted)) => commands = asked.commands(id, muted),
                None => tracing::debug!("no default sink: the key does nothing"),
            }
        }
        commands.extend(asked.node_commands());
        for args in commands {
            match Command::new("wpctl")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(?args, %status, "wpctl did not set the sound"),
                Err(err) => tracing::warn!(?args, %err, "cannot run wpctl"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sink(id: u32, name: &str, volumes: &[f64], mute: bool) -> Value {
        json!({
            "id": id,
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": { "node.name": name, "media.class": "Audio/Sink" },
                "params": { "Props": [
                    { "volume": 1.0, "mute": mute, "channelVolumes": volumes },
                    { "params": [] }
                ] }
            }
        })
    }

    fn default_is(id: u32, name: &str) -> Value {
        json!({
            "id": id,
            "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [
                { "subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON",
                  "value": { "name": name } }
            ]
        })
    }

    #[test]
    fn the_default_sink_its_volume_and_its_changes() {
        let mut graph = Graph::default();
        assert_eq!(graph.default_sink(), None);
        // 0.62 as wpctl shows it is 0.238328 linear.
        graph.apply(sink(30, "speakers", &[0.238_328, 0.238_328], false));
        graph.apply(sink(31, "headset", &[0.125, 0.125], true));
        graph.apply(json!({ "id": 40, "type": "PipeWire:Interface:Client", "info": {} }));
        assert_eq!(graph.default_sink(), None);
        graph.apply(default_is(29, "speakers"));
        let (id, sound) = graph.default_sink().expect("a default sink");
        assert_eq!(id, 30);
        assert!((sound.volume - 62.0).abs() < 0.01, "{}", sound.volume);
        assert!(!sound.muted);
        // The default changes, as a JSON string this time.
        let mut other = default_is(29, "");
        other["metadata"][0]["value"] = json!("{\"name\":\"headset\"}");
        graph.apply(other);
        let (id, sound) = graph.default_sink().expect("a default sink");
        assert_eq!((id, sound.muted), (31, true));
        assert!((sound.volume - 50.0).abs() < 0.01);
        // An update without params keeps the volume; one with changes it.
        graph.apply(json!({
            "id": 31, "type": "PipeWire:Interface:Node",
            "info": { "props": { "node.name": "headset", "media.class": "Audio/Sink" } }
        }));
        assert_eq!(graph.default_sink().map(|(id, _)| id), Some(31));
        graph.apply(sink(31, "headset", &[1.0, 0.0], false));
        let (_, sound) = graph.default_sink().expect("a default sink");
        assert!((sound.volume - 79.37).abs() < 0.01, "{}", sound.volume);
        // The sink goes: no sound.
        graph.apply(json!({ "id": 31, "info": null }));
        assert_eq!(graph.default_sink(), None);
        // The metadata goes: no default.
        graph.apply(json!({ "id": 29, "info": null }));
        assert_eq!(graph.sink, None);
    }

    #[test]
    fn the_sinks_their_names_what_they_are_and_their_own_volumes() {
        let mut graph = Graph::default();
        let mut speakers = sink(
            30,
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
            &[0.238_328; 2],
            false,
        );
        speakers["info"]["props"]["node.description"] = json!("Haut-parleurs du portable");
        speakers["info"]["props"]["device.id"] = json!(20);
        let mut headset = sink(31, "bluez_output.AA_BB.1", &[0.064; 2], true);
        headset["info"]["props"]["node.description"] = json!("Casque Bluetooth");
        headset["info"]["props"]["device.id"] = json!(21);
        let mut screen = sink(
            32,
            "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
            &[0.512; 2],
            false,
        );
        screen["info"]["props"]["node.nick"] = json!("Écran externe (HDMI)");
        // A sink of no device, named only.
        let plain = sink(33, "hocus-test-other", &[1.0; 2], false);
        for object in [speakers, headset, screen, plain] {
            graph.apply(object);
        }
        graph.apply(json!({
            "id": 20, "type": "PipeWire:Interface:Device",
            "info": { "props": { "device.form-factor": "internal", "device.bus": "pci" } }
        }));
        graph.apply(json!({
            "id": 21, "type": "PipeWire:Interface:Device",
            "info": { "props": { "device.bus": "bluetooth" } }
        }));
        graph.apply(default_is(29, "bluez_output.AA_BB.1"));
        let sinks = graph.sinks();
        let shown: Vec<_> = sinks
            .iter()
            .map(|s| (s.id, s.description.as_str(), s.kind, s.default))
            .collect();
        assert_eq!(
            shown,
            vec![
                (30, "Haut-parleurs du portable", Kind::Speaker, false),
                (31, "Casque Bluetooth", Kind::Headphones, true),
                (32, "Écran externe (HDMI)", Kind::Screen, false),
                (33, "hocus-test-other", Kind::Speaker, false),
            ]
        );
        // Each its own volume: 62, 40 (muted), 80, 100.
        let volumes: Vec<_> = sinks
            .iter()
            .filter_map(|s| s.sound)
            .map(|s| (s.volume.round() as u32, s.muted))
            .collect();
        assert_eq!(
            volumes,
            [(62, false), (40, true), (80, false), (100, false)]
        );
        // A form factor on the node itself says first.
        let mut tv = sink(34, "alsa_output.usb-stereo", &[1.0; 2], false);
        tv["info"]["props"]["device.form-factor"] = json!("headset");
        graph.apply(tv);
        assert_eq!(graph.sinks().last().map(|s| s.kind), Some(Kind::Headphones));
        // A sink going leaves the list.
        graph.apply(json!({ "id": 32, "info": null }));
        assert_eq!(graph.sinks().len(), 4);
    }

    #[test]
    fn a_slider_dragged_and_an_output_chosen_make_few_commands() {
        let line = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let mut asked = Asked::default();
        for v in [10.0, 20.0, 35.5, 42.0] {
            asked.add(Ask::Volume(v));
        }
        // Only the last volume; unmuted, as the slider is moved.
        assert_eq!(
            asked.commands(30, true),
            vec![
                line(&["set-volume", "30", "0.420"]),
                line(&["set-mute", "30", "0"])
            ]
        );
        // Keys after it step from it.
        asked.add(Ask::Key(LevelKey::VolumeUp));
        assert_eq!(
            asked.commands(30, false),
            vec![
                line(&["set-volume", "30", "0.420"]),
                line(&["set-volume", "--limit", "1.0", "30", "5%+"])
            ]
        );
        // Another default sink, its own volume kept.
        let mut asked = Asked::default();
        asked.add(Ask::Default(31));
        assert_eq!(
            asked.commands(31, false),
            vec![line(&["set-default", "31"])]
        );
        // Out of range: kept within 0 and 100 %.
        let mut asked = Asked::default();
        asked.add(Ask::Volume(130.0));
        assert_eq!(
            asked.commands(7, false),
            vec![line(&["set-volume", "7", "1.000"])]
        );
    }

    #[test]
    fn a_mute_for_one_output_is_not_taken_by_the_next() {
        let mut keys = Asked::default();
        keys.add(Ask::Key(LevelKey::VolumeUp));
        assert!(keys.for_the_default());
        let (send, asks) = std::sync::mpsc::channel();
        for ask in [Ask::Default(31), Ask::Volume(40.0)] {
            send.send(ask).expect("sent");
        }
        let (first, next) = batch(Ask::Key(LevelKey::Mute), &asks).expect("a batch");
        assert!(first.toggle && first.default.is_none());
        assert_eq!(next, Some(Ask::Default(31)));
        let (second, next) = batch(Ask::Default(31), &asks).expect("a batch");
        assert_eq!(
            (second.default, second.to, next),
            (Some(31), Some(40.0), None)
        );
        drop(send);
        assert!(batch(Ask::Default(31), &asks).is_none());
    }

    #[test]
    fn keys_taken_together_make_few_commands() {
        let asked = |keys: &[LevelKey]| {
            let mut asked = Asked::default();
            for key in keys {
                asked.add(Ask::Key(*key));
            }
            asked
        };
        let up = LevelKey::VolumeUp;
        let down = LevelKey::VolumeDown;
        let mute = LevelKey::Mute;
        let line = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            asked(&[up]).commands(30, false),
            vec![line(&["set-volume", "--limit", "1.0", "30", "5%+"])]
        );
        assert_eq!(
            asked(&[up, up, down, down, down]).commands(30, false),
            vec![line(&["set-volume", "--limit", "1.0", "30", "5%-"])]
        );
        // A volume key unmutes.
        assert_eq!(
            asked(&[up]).commands(7, true),
            vec![
                line(&["set-volume", "--limit", "1.0", "7", "5%+"]),
                line(&["set-mute", "7", "0"])
            ]
        );
        assert_eq!(
            asked(&[up, down]).commands(7, true),
            vec![line(&["set-mute", "7", "0"])]
        );
        assert_eq!(
            asked(&[mute]).commands(7, false),
            vec![line(&["set-mute", "7", "toggle"])]
        );
        assert_eq!(
            asked(&[mute, mute]).commands(7, false),
            Vec::<Vec<String>>::new()
        );
    }

    fn node(id: u32, props: Value, state: &str, volumes: &[f64], mute: bool) -> Value {
        json!({
            "id": id,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": state,
                "props": props,
                "params": { "Props": [ { "mute": mute, "channelVolumes": volumes } ] }
            }
        })
    }

    #[test]
    fn the_microphones_and_the_default_one_not_the_monitors() {
        let mut graph = Graph::default();
        let source = |name: &str, description: &str| {
            json!({ "node.name": name, "node.description": description,
                    "media.class": "Audio/Source" })
        };
        graph.apply(node(
            40,
            source("mic.internal", "Micro du portable"),
            "suspended",
            &[0.343; 2],
            false,
        ));
        graph.apply(node(
            41,
            source("mic.headset", "Micro du casque"),
            "idle",
            &[0.614_125; 2],
            true,
        ));
        let mut null = source("null-source", "Null");
        null["media.class"] = json!("Audio/Source/Virtual");
        graph.apply(node(42, null, "idle", &[1.0; 2], false));
        // A sink's monitor made a source is not a microphone.
        let mut monitor = source("speakers.monitor", "Monitor of speakers");
        monitor["device.class"] = json!("monitor");
        graph.apply(node(43, monitor, "idle", &[1.0; 2], false));
        let mut default = json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [
                { "subject": 0, "key": "default.audio.source", "type": "Spa:String:JSON",
                  "value": { "name": "mic.headset" } }
            ]
        });
        graph.apply(default.clone());
        let sources = graph.sources();
        let shown: Vec<_> = sources
            .iter()
            .map(|s| (s.id, s.description.as_str(), s.default))
            .collect();
        assert_eq!(
            shown,
            [
                (40, "Micro du portable", false),
                (41, "Micro du casque", true),
                (42, "Null", false)
            ]
        );
        let volumes: Vec<_> = sources
            .iter()
            .filter_map(|s| s.sound)
            .map(|s| (s.volume.round() as u32, s.muted))
            .collect();
        assert_eq!(volumes, [(70, false), (85, true), (100, false)]);
        // No sink among them, no source among the sinks.
        assert!(graph.sinks().is_empty());
        let mixer = graph.mixer(&mut HashMap::new());
        assert_eq!(mixer.microphone().map(|s| s.id), Some(41));
        // Without a default one, the first.
        default["metadata"] = json!([]);
        graph.apply(default);
        assert_eq!(
            graph.mixer(&mut HashMap::new()).microphone().map(|s| s.id),
            Some(40)
        );
    }

    #[test]
    fn the_streams_by_application_and_which_play() {
        let mut graph = Graph::default();
        let stream = |app: &str, binary: &str, corked: bool| {
            json!({ "node.name": app, "media.class": "Stream/Output/Audio",
                    "application.name": app, "application.process.binary": binary,
                    "pulse.corked": corked })
        };
        // Firefox: two tabs, one paused (idle and corked).
        graph.apply(node(
            60,
            stream("Firefox", "firefox", false),
            "running",
            &[1.0; 2],
            false,
        ));
        graph.apply(node(
            61,
            stream("Musique", "music", false),
            "running",
            &[0.262_144; 2],
            true,
        ));
        graph.apply(node(
            62,
            stream("Firefox", "firefox", true),
            "idle",
            &[0.125; 2],
            false,
        ));
        // A stream with no application's name: its program's.
        let mut bare = stream("", "pw-play", false);
        bare["node.name"] = json!("pw-play");
        bare.as_object_mut().map(|o| o.remove("application.name"));
        graph.apply(node(63, bare, "idle", &[1.0; 2], false));
        // A sink is not a stream.
        graph.apply(sink(30, "speakers", &[1.0; 2], false));
        let apps: Vec<_> = graph
            .apps()
            .into_iter()
            .map(|(a, _)| {
                let sound = a.sound.map(|s| (s.volume.round() as u32, s.muted));
                (a.key, a.name, a.streams, sound, a.playing)
            })
            .collect();
        assert_eq!(
            apps,
            [
                (
                    "Firefox".into(),
                    "Firefox".into(),
                    vec![60, 62],
                    Some((100, false)),
                    true
                ),
                (
                    "Musique".into(),
                    "Musique".into(),
                    vec![61],
                    Some((64, true)),
                    true
                ),
                (
                    "pw-play".into(),
                    "pw-play".into(),
                    vec![63],
                    Some((100, false)),
                    false
                ),
            ]
        );
        // The first tab paused too: Firefox plays no more. An update
        // without props keeps the stream's class and application.
        graph.apply(json!({ "id": 60, "type": "PipeWire:Interface:Node",
                            "info": { "state": "idle" } }));
        let firefox = graph.apps().into_iter().next().map(|(a, _)| a);
        assert_eq!(
            firefox.map(|a| (a.name, a.playing)),
            Some(("Firefox".into(), false))
        );
        // Muted when all its streams are.
        graph.apply(node(
            62,
            stream("Firefox", "firefox", true),
            "idle",
            &[0.125; 2],
            true,
        ));
        graph.apply(node(
            60,
            stream("Firefox", "firefox", false),
            "running",
            &[1.0; 2],
            true,
        ));
        let firefox = graph.apps().into_iter().next().map(|(a, _)| a);
        assert_eq!(firefox.and_then(|a| a.sound).map(|s| s.muted), Some(true));
        // A stream going leaves its application; the last, the list.
        graph.apply(json!({ "id": 61, "info": null }));
        assert_eq!(graph.apps().len(), 2);
    }

    #[test]
    fn an_application_is_known_by_its_sandbox_then_its_name_then_its_program() {
        let who = |props: Value| Who::read(&props);
        let flatpak = who(json!({ "application.name": "Firefox",
                                  "pipewire.access.portal.app_id": "org.mozilla.firefox",
                                  "application.process.binary": "firefox" }));
        assert_eq!(flatpak.key("n"), "org.mozilla.firefox");
        assert_eq!(flatpak.icon_names(), ["org.mozilla.firefox", "firefox"]);
        let chrome = who(json!({ "application.name": "Google Chrome",
                                 "application.icon-name": "google-chrome",
                                 "application.process.binary": "chrome" }));
        assert_eq!(chrome.key("n"), "Google Chrome");
        assert_eq!(chrome.icon_names(), ["google-chrome", "chrome"]);
        assert_eq!(
            who(json!({ "application.process.binary": "mpv" })).key("n"),
            "mpv"
        );
        assert_eq!(
            who(json!({})).key("alsa_playback.speaker-test"),
            "alsa_playback.speaker-test"
        );
    }

    #[test]
    fn microphones_and_applications_asks_make_few_commands() {
        let line = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let mut asked = Asked::default();
        // A slider dragged: only its last volume; a mute after it.
        for v in [10.0, 30.0, 55.0] {
            asked.add(Ask::Node(61, Set::Volume(v)));
        }
        asked.add(Ask::Node(61, Set::Mute(false)));
        // An application of two streams muted, then unmuted.
        for id in [60, 62] {
            asked.add(Ask::Node(id, Set::Mute(true)));
        }
        for id in [60, 62] {
            asked.add(Ask::Node(id, Set::Mute(false)));
        }
        asked.add(Ask::DefaultSource(41));
        assert!(!asked.for_a_sink());
        assert_eq!(
            asked.node_commands(),
            vec![
                line(&["set-default", "41"]),
                line(&["set-mute", "60", "0"]),
                line(&["set-volume", "61", "0.550"]),
                line(&["set-mute", "61", "0"]),
                line(&["set-mute", "62", "0"]),
            ]
        );
        // Asked of the default sink too: both.
        asked.add(Ask::Key(LevelKey::VolumeUp));
        assert!(asked.for_a_sink());
        // Another default source does not wait for the next batch.
        let (send, asks) = std::sync::mpsc::channel();
        send.send(Ask::DefaultSource(40)).expect("sent");
        let (both, next) = batch(Ask::Key(LevelKey::Mute), &asks).expect("a batch");
        assert_eq!((both.toggle, both.source, next), (true, Some(40), None));
    }
}
