//! PipeWire's graph as `pw-dump` writes it: the sinks and sources, the
//! devices they belong to and their profiles, the streams applications
//! play and record, the links between them, and the `default` metadata
//! (the default sink and source, the ones the user chose, and each
//! stream's chosen target).

use std::collections::HashMap;

use serde_json::Value;

use crate::Sound;

/// What an output (or an input) is, for its icon.
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

/// An output or an input (a sink, a source).
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    /// `object.serial`: what `target.object` names (ids are reused).
    pub serial: Option<u32>,
    /// `node.name`.
    pub name: String,
    pub description: String,
    pub kind: Kind,
    /// Its own volume and mute: each keeps its own.
    pub sound: Option<Sound>,
    /// Its channels' linear volumes and their positions (`FL`, `FR`).
    pub channels: Vec<f32>,
    pub positions: Vec<String>,
    pub default: bool,
    /// The device it belongs to, if any (its profiles).
    pub device: Option<u32>,
    /// `device.bus` (`bluetooth`, `pci`, `usb`), its own or its device's.
    pub bus: Option<String>,
    /// `device.form-factor`, its own or its device's (`internal`).
    pub form_factor: Option<String>,
}

impl Sink {
    /// Whether it plays (or records) one channel only.
    pub fn mono(&self) -> bool {
        self.channels.len() == 1
            || (!self.positions.is_empty() && self.positions.iter().all(|p| p == "MONO"))
    }
}

/// A device's profile (`EnumProfile`): HiFi, A2DP, HFP…
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub index: u32,
    pub name: String,
    pub description: String,
    /// Whether it can be chosen now (`available` is not `no`).
    pub available: bool,
}

/// A device (an ALSA card, a Bluetooth headset): its profiles and the one
/// it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: u32,
    pub name: String,
    pub description: String,
    /// `device.api`: `alsa`, `bluez5`.
    pub api: Option<String>,
    pub profiles: Vec<Profile>,
    /// The current profile's index.
    pub profile: Option<u32>,
}

impl Card {
    /// The profiles a user may choose: available, never "Off" (it would
    /// take the device away).
    pub fn choices(&self) -> impl Iterator<Item = &Profile> {
        self.profiles
            .iter()
            .filter(|p| p.available && p.name != "off")
    }
}

/// An application playing sound, its streams taken together; `I` is its
/// icon, as whoever shows it reads one.
#[derive(Debug, Clone, PartialEq)]
pub struct App<I> {
    /// What its streams share: its sandbox's app id, else its name, else
    /// its program.
    pub key: String,
    pub name: String,
    pub icon: Option<I>,
    /// The names its icon may go by, the likeliest first.
    pub icon_names: Vec<String>,
    /// Its streams' node ids, the oldest first.
    pub streams: Vec<u32>,
    /// Its oldest stream's volume; muted when all its streams are.
    pub sound: Option<Sound>,
    /// Whether one of its streams plays now: its node runs, not corked.
    pub playing: bool,
    /// The sink its oldest stream is linked into, if any.
    pub sink: Option<u32>,
    /// The output chosen for it (`target.object` in the `default`
    /// metadata, a serial or a name); `None`: it follows the default.
    pub target: Option<String>,
}

/// An application recording: its capture streams linked from a source,
/// taken together.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorder {
    pub key: String,
    pub name: String,
    pub icon_names: Vec<String>,
    pub streams: Vec<u32>,
    pub sound: Option<Sound>,
    /// The sources its streams record.
    pub sources: Vec<u32>,
    /// One of the Settings' own level meters ([`crate::meter::NODE`]).
    pub ours: bool,
}

/// What the sound's popover and the Settings list.
#[derive(Debug, Clone, PartialEq)]
pub struct Mixer<I> {
    pub sinks: Vec<Sink>,
    /// The microphones (sources), as the sinks.
    pub sources: Vec<Sink>,
    /// The applications with an output stream, in the order their first
    /// stream came.
    pub apps: Vec<App<I>>,
    /// The applications recording a source.
    pub recorders: Vec<Recorder>,
    pub cards: Vec<Card>,
    /// The sink and source the user chose (`default.configured.*`), which
    /// stay chosen while they are away.
    pub configured_sink: Option<String>,
    pub configured_source: Option<String>,
}

impl<I> Mixer<I> {
    pub const fn new() -> Self {
        Self {
            sinks: Vec::new(),
            sources: Vec::new(),
            apps: Vec::new(),
            recorders: Vec::new(),
            cards: Vec::new(),
            configured_sink: None,
            configured_source: None,
        }
    }

    /// The microphone the popover's slider sets: the default source, else
    /// the first.
    pub fn microphone(&self) -> Option<&Sink> {
        self.sources
            .iter()
            .find(|s| s.default)
            .or(self.sources.first())
    }

    /// The output in use: the default sink, else the first.
    pub fn output(&self) -> Option<&Sink> {
        self.sinks.iter().find(|s| s.default).or(self.sinks.first())
    }

    pub fn card(&self, id: u32) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == id)
    }

    /// The sink or source of node `id`.
    pub fn node(&self, id: u32) -> Option<&Sink> {
        self.sinks.iter().chain(&self.sources).find(|s| s.id == id)
    }
}

impl<I> Default for Mixer<I> {
    fn default() -> Self {
        Self::new()
    }
}

/// An application using the microphone or the camera (5.12).
#[derive(Debug, Clone, PartialEq)]
pub struct Using {
    /// Its application's key (its streams taken together).
    pub key: String,
    /// The name people know it by.
    pub app: String,
    pub icon_names: Vec<String>,
    /// The names its windows' application ids may go by, lower case.
    pub ids: Vec<String>,
    /// Its stream's `media.name`, if it says one.
    pub media: Option<String>,
}

/// What the graph says is in use.
#[derive(Debug, Default)]
pub struct Used {
    pub microphone: Vec<Using>,
    pub muted: bool,
    pub camera: Vec<Using>,
    pub cameras: usize,
}

/// What is kept of PipeWire's graph.
#[derive(Debug, Default)]
pub struct Graph {
    /// The default sink's and source's names, from the `default` metadata.
    sink: Option<String>,
    source: Option<String>,
    /// The ones chosen (`default.configured.audio.*`).
    configured_sink: Option<String>,
    configured_source: Option<String>,
    /// Each stream's chosen target (`target.object`, or the older
    /// `target.node`), by the stream's id.
    targets: HashMap<u32, String>,
    /// The `default` metadata's id, to see it go.
    metadata: Option<u32>,
    /// A key of it went, which `pw-dump` does not name: it is read again
    /// whole ([`Graph::reset_metadata`]).
    lost: bool,
    /// Audio sinks, sources and streams by id.
    nodes: HashMap<u32, Node>,
    /// Devices by id.
    devices: HashMap<u32, Device>,
    /// Links by id: the node out of which they go, and the one into which.
    links: HashMap<u32, (u32, u32)>,
}

/// What a node is, from its `media.class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Class {
    #[default]
    Sink,
    Source,
    /// An application's output stream (a "sink input").
    Stream,
    /// An application's input stream: a microphone's when it is linked to
    /// a source (5.12).
    Capture,
    /// A camera, and an application's video input stream: the camera's
    /// when linked to one.
    Camera,
    VideoCapture,
}

impl Class {
    fn of(media_class: &str) -> Option<Self> {
        match media_class {
            "Audio/Sink" => Some(Self::Sink),
            "Audio/Source" | "Audio/Source/Virtual" => Some(Self::Source),
            "Stream/Output/Audio" => Some(Self::Stream),
            "Stream/Input/Audio" => Some(Self::Capture),
            "Video/Source" => Some(Self::Camera),
            "Stream/Input/Video" => Some(Self::VideoCapture),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct Node {
    class: Class,
    name: String,
    serial: Option<u32>,
    description: Option<String>,
    device: Option<u32>,
    /// What its own props say it is, if they do.
    what: What,
    sound: Option<Sound>,
    channels: Vec<f32>,
    positions: Vec<String>,
    /// A stream's application, and whether it plays: its node runs
    /// (`info.state`), and PulseAudio's clients did not cork it.
    app: Who,
    running: bool,
    corked: bool,
    /// A stream's `media.name`, and its process's id.
    media: Option<String>,
    pid: Option<u32>,
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
struct What {
    form_factor: Option<String>,
    bus: Option<String>,
}

impl What {
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

/// A device: what it is, and its profiles.
#[derive(Debug, Default, Clone)]
struct Device {
    what: What,
    name: String,
    description: Option<String>,
    api: Option<String>,
    profiles: Vec<Profile>,
    profile: Option<u32>,
}

fn number(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .and_then(|n| u32::try_from(n).ok())
}

impl Graph {
    /// One object of `pw-dump`'s output: added, changed, or gone (with no
    /// type, or no info).
    pub fn apply(&mut self, object: Value) {
        let Some(id) = number(object.get("id")) else {
            return;
        };
        let gone = object.get("info").is_some_and(Value::is_null)
            || object.get("metadata").is_some_and(Value::is_null)
            || object.get("type").is_none();
        if gone {
            self.nodes.remove(&id);
            self.devices.remove(&id);
            self.links.remove(&id);
            self.targets.remove(&id);
            if self.metadata == Some(id) {
                self.metadata = None;
                self.sink = None;
                self.source = None;
                self.configured_sink = None;
                self.configured_source = None;
                self.targets.clear();
            }
            return;
        }
        match object.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Metadata") => self.metadata(id, &object),
            Some("PipeWire:Interface:Node") => self.node(id, &object),
            Some("PipeWire:Interface:Link") => {
                // An update without them keeps those known.
                let node = |key: &str| number(object.pointer(&format!("/info/{key}")));
                if let (Some(out), Some(into)) = (node("output-node-id"), node("input-node-id")) {
                    self.links.insert(id, (out, into));
                }
            }
            Some("PipeWire:Interface:Device") => self.device(id, &object),
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
        // A key taken away comes as an update listing nothing.
        if entries.is_empty() {
            self.lost = true;
            return;
        }
        self.entries(entries);
    }

    /// Whether a key of the `default` metadata went since last asked:
    /// its entries are then to be read again whole.
    pub fn take_lost(&mut self) -> bool {
        std::mem::take(&mut self.lost)
    }

    /// The `default` metadata's entries as they all are now (subject,
    /// key, value as text), read again whole.
    pub fn reset_metadata(&mut self, entries: &[(u32, String, String)]) {
        self.sink = None;
        self.source = None;
        self.configured_sink = None;
        self.configured_source = None;
        self.targets.clear();
        let entries: Vec<Value> = entries
            .iter()
            .map(|(subject, key, value)| {
                serde_json::json!({ "subject": subject, "key": key, "value": value })
            })
            .collect();
        self.entries(&entries);
    }

    fn entries(&mut self, entries: &[Value]) {
        // An update lists the keys that changed only (WirePlumber writes
        // the sink's and the source's apart): a key not listed keeps its
        // value; one listed without a value is gone.
        for (key, kept) in [
            ("default.audio.sink", &mut self.sink),
            ("default.audio.source", &mut self.source),
            ("default.configured.audio.sink", &mut self.configured_sink),
            (
                "default.configured.audio.source",
                &mut self.configured_source,
            ),
        ] {
            let entry = entries
                .iter()
                .filter(|e| e.get("subject").and_then(Value::as_u64) == Some(0))
                .find(|e| e.get("key").and_then(Value::as_str) == Some(key));
            if let Some(entry) = entry {
                *kept = entry.get("value").and_then(sink_name);
            }
        }
        // The streams' targets, by their own subject.
        for entry in entries {
            let Some(subject) = number(entry.get("subject")).filter(|s| *s != 0) else {
                continue;
            };
            let key = entry.get("key").and_then(Value::as_str);
            if !matches!(key, Some("target.object" | "target.node")) {
                continue;
            }
            let value = match entry.get("value") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            }
            .filter(|v| !v.is_empty() && v != "-1");
            match value {
                Some(value) => {
                    self.targets.insert(subject, value);
                }
                None => {
                    self.targets.remove(&subject);
                }
            }
        }
    }

    fn device(&mut self, id: u32, object: &Value) {
        let device = self.devices.entry(id).or_default();
        // An update without props (or params) keeps those known.
        if let Some(props) = object.pointer("/info/props") {
            device.what = What::read(Some(props));
            let text = |key: &str| props.get(key).and_then(Value::as_str).map(str::to_owned);
            device.name = text("device.name").unwrap_or_default();
            device.description = text("device.description").or_else(|| text("device.nick"));
            device.api = text("device.api");
        }
        if let Some(profiles) = object
            .pointer("/info/params/EnumProfile")
            .and_then(Value::as_array)
        {
            device.profiles = profiles.iter().filter_map(profile_of).collect();
        }
        if let Some(profile) = object
            .pointer("/info/params/Profile")
            .and_then(Value::as_array)
            .and_then(|p| p.first())
        {
            device.profile = number(profile.get("index"));
        }
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
            node.serial = number(props.get("object.serial"));
            node.description = ["node.description", "node.nick"]
                .iter()
                .find_map(|key| props.get(*key).and_then(Value::as_str))
                .map(str::to_owned);
            node.device = number(props.get("device.id"));
            node.what = What::read(Some(props));
            node.app = Who::read(props);
            node.corked = props
                .get("pulse.corked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            node.media = props
                .get("media.name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_owned);
            node.pid = number(props.get("application.process.id"));
        }
        if let Some(state) = info.get("state").and_then(Value::as_str) {
            node.running = state == "running";
        }
        // An update without the params keeps those known.
        if let Some(props) = info.pointer("/params/Props").and_then(Value::as_array) {
            node.sound = props.iter().find_map(sound_of);
            if let Some((channels, positions)) = props.iter().find_map(channels_of) {
                node.channels = channels;
                node.positions = positions;
            }
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
                    .or(device.and_then(|d| d.what.form_factor.as_deref()));
                let bus = node
                    .what
                    .bus
                    .as_deref()
                    .or(device.and_then(|d| d.what.bus.as_deref()));
                Sink {
                    id: *id,
                    serial: node.serial,
                    name: node.name.clone(),
                    description: node
                        .description
                        .clone()
                        .unwrap_or_else(|| node.name.clone()),
                    kind: Kind::of(form_factor, bus, &node.name),
                    sound: node.sound,
                    channels: node.channels.clone(),
                    positions: node.positions.clone(),
                    default: default == Some(node.name.as_str()),
                    device: node.device,
                    bus: bus.map(str::to_owned),
                    form_factor: form_factor.map(str::to_owned),
                }
            })
            .collect();
        sinks.sort_by_key(|s| s.id);
        sinks
    }

    /// The devices with their profiles, by id.
    pub fn cards(&self) -> Vec<Card> {
        let mut cards: Vec<Card> = self
            .devices
            .iter()
            .map(|(id, d)| Card {
                id: *id,
                name: d.name.clone(),
                description: d.description.clone().unwrap_or_else(|| d.name.clone()),
                api: d.api.clone(),
                profiles: d.profiles.clone(),
                profile: d.profile,
            })
            .collect();
        cards.sort_by_key(|c| c.id);
        cards
    }

    /// The nodes of class `class` linked into node `id`, or out of it
    /// (`into`: the links into `id`).
    fn linked_with(&self, id: u32, into: bool, class: Class) -> Vec<u32> {
        let mut found: Vec<u32> = self
            .links
            .values()
            .filter_map(|(out, to)| match into {
                true if *to == id => Some(*out),
                false if *out == id => Some(*to),
                _ => None,
            })
            .filter(|n| self.nodes.get(n).is_some_and(|n| n.class == class))
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The streams of `class`, the oldest first (by serial, as ids are
    /// reused).
    fn streams(&self, class: Class) -> Vec<(u32, &Node)> {
        let mut streams: Vec<(u32, &Node)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.class == class)
            .map(|(id, node)| (*id, node))
            .collect();
        streams.sort_by_key(|(id, node)| (node.serial.unwrap_or(u32::MAX), *id));
        streams
    }

    /// The applications with an output stream; their icons not looked for.
    pub fn apps(&self) -> Vec<App<()>> {
        let mut apps: Vec<App<()>> = Vec::new();
        for (id, node) in self.streams(Class::Stream) {
            let key = node.app.key(&node.name);
            let playing = node.running && !node.corked;
            match apps.iter_mut().find(|app| app.key == key) {
                Some(app) => {
                    app.streams.push(id);
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
                    apps.push(App {
                        key,
                        name,
                        icon: None,
                        icon_names: node.app.icon_names(),
                        streams: vec![id],
                        sound: node.sound,
                        playing,
                        sink: self.linked_with(id, false, Class::Sink).first().copied(),
                        target: self.targets.get(&id).cloned(),
                    });
                }
            }
        }
        apps
    }

    /// The applications recording a source, by application.
    pub fn recorders(&self) -> Vec<Recorder> {
        let mut recorders: Vec<Recorder> = Vec::new();
        for (id, node) in self.streams(Class::Capture) {
            let sources = self.linked_with(id, true, Class::Source);
            if sources.is_empty() {
                continue;
            }
            let key = node.app.key(&node.name);
            match recorders.iter_mut().find(|r| r.key == key) {
                Some(recorder) => {
                    recorder.streams.push(id);
                    for source in sources {
                        if !recorder.sources.contains(&source) {
                            recorder.sources.push(source);
                        }
                    }
                }
                None => recorders.push(Recorder {
                    key,
                    name: node
                        .app
                        .name
                        .clone()
                        .or_else(|| node.app.binary.clone())
                        .or_else(|| node.pid.and_then(program_of))
                        .unwrap_or_else(|| node.name.clone()),
                    icon_names: node.app.icon_names(),
                    streams: vec![id],
                    sound: node.sound,
                    sources,
                    ours: node.name == crate::meter::NODE,
                }),
            }
        }
        recorders
    }

    /// Everything listed, the applications' icons read by `icon` (given
    /// their key and the names their icon may go by).
    pub fn mixer<I>(&self, mut icon: impl FnMut(&str, &[String]) -> Option<I>) -> Mixer<I> {
        let apps = self
            .apps()
            .into_iter()
            .map(|app| App {
                icon: icon(&app.key, &app.icon_names),
                key: app.key,
                name: app.name,
                icon_names: app.icon_names,
                streams: app.streams,
                sound: app.sound,
                playing: app.playing,
                sink: app.sink,
                target: app.target,
            })
            .collect();
        Mixer {
            sinks: self.sinks(),
            sources: self.sources(),
            apps,
            recorders: self.recorders(),
            cards: self.cards(),
            configured_sink: self.configured_sink.clone(),
            configured_source: self.configured_source.clone(),
        }
    }

    /// The streams of class `class` linked from a node of class `from`
    /// (a source, a camera), by application.
    fn linked(&self, class: Class, from: Class) -> Vec<Using> {
        let fed = |id: u32| {
            self.links.values().any(|(out, into)| {
                *into == id && self.nodes.get(out).is_some_and(|n| n.class == from)
            })
        };
        let mut streams: Vec<(&u32, &Node)> = self
            .nodes
            .iter()
            .filter(|(id, node)| node.class == class && fed(**id))
            .collect();
        streams.sort_by_key(|(id, _)| **id);
        let mut used: Vec<Using> = Vec::new();
        for (_, node) in streams {
            let key = node.app.key(&node.name);
            if let Some(media) = used.iter_mut().find(|m| m.key == key) {
                if media.media.is_none() {
                    media.media.clone_from(&node.media);
                }
                continue;
            }
            let app = node
                .app
                .name
                .clone()
                .or_else(|| node.app.binary.clone())
                .or_else(|| node.pid.and_then(program_of))
                .unwrap_or_else(|| node.name.clone());
            let ids = [
                &node.app.sandboxed,
                &node.app.id,
                &node.app.binary,
                &node.app.name,
            ]
            .into_iter()
            .flatten()
            .map(|id| id.to_lowercase())
            .collect();
            used.push(Using {
                key,
                app,
                icon_names: node.app.icon_names(),
                ids,
                media: node.media.clone(),
            });
        }
        used
    }

    /// What is in use (5.12): the applications whose streams take a
    /// source's sound, whether the default source is muted, those whose
    /// streams take a camera's pictures, and how many cameras there are.
    pub fn in_use(&self) -> Used {
        let sources = self.sources();
        let muted = sources
            .iter()
            .find(|s| s.default)
            .or(sources.first())
            .and_then(|s| s.sound)
            .is_some_and(|s| s.muted);
        Used {
            microphone: self.linked(Class::Capture, Class::Source),
            muted,
            camera: self.linked(Class::VideoCapture, Class::Camera),
            cameras: self
                .nodes
                .values()
                .filter(|n| n.class == Class::Camera)
                .count(),
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

    /// Whether node `id` (a sink, a source, a stream) is muted, if known.
    pub fn muted(&self, id: u32) -> Option<bool> {
        self.nodes.get(&id)?.sound.map(|s| s.muted)
    }

    /// The default sink's and source's names.
    pub fn defaults(&self) -> (Option<&str>, Option<&str>) {
        (self.sink.as_deref(), self.source.as_deref())
    }
}

/// Process `pid`'s program, as `/proc` names it.
fn program_of(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
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
/// loudest channel's linear volume, the others below it by the balance)
/// and mute.
fn sound_of(props: &Value) -> Option<Sound> {
    let channels: Vec<f64> = props
        .get("channelVolumes")?
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    let loudest = channels.iter().copied().reduce(f64::max)?;
    Some(Sound {
        volume: (loudest.max(0.0).cbrt() * 100.0) as f32,
        muted: props.get("mute").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// A `Props` object's channels' linear volumes and their positions.
fn channels_of(props: &Value) -> Option<(Vec<f32>, Vec<String>)> {
    let channels: Vec<f32> = props
        .get("channelVolumes")?
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .map(|v| v as f32)
        .collect();
    if channels.is_empty() {
        return None;
    }
    let positions = props
        .get("channelMap")
        .and_then(Value::as_array)
        .map(|map| {
            map.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some((channels, positions))
}

/// One of `EnumProfile`'s objects.
fn profile_of(profile: &Value) -> Option<Profile> {
    let index = number(profile.get("index"))?;
    let text = |key: &str| {
        profile
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let available = !matches!(profile.get("available").and_then(Value::as_str), Some("no"));
    Some(Profile {
        index,
        name: text("name"),
        description: text("description"),
        available,
    })
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

    fn classed(id: u32, class: &str, props: Value) -> Value {
        let mut props = props;
        props["media.class"] = json!(class);
        json!({ "id": id, "type": "PipeWire:Interface:Node", "info": { "props": props } })
    }

    fn link(id: u32, out: u32, into: u32) -> Value {
        json!({
            "id": id, "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": out, "input-node-id": into, "state": "active" }
        })
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
    fn a_stream_linked_to_a_source_uses_the_microphone_one_to_a_camera_the_camera() {
        let mut graph = Graph::default();
        let mut mic = sink(30, "mic", &[0.5, 0.5], false);
        mic["info"]["props"]["media.class"] = json!("Audio/Source");
        graph.apply(mic);
        graph.apply(sink(31, "speakers", &[0.5, 0.5], false));
        graph.apply(classed(
            40,
            "Stream/Input/Audio",
            json!({
                "node.name": "Firefox", "application.name": "Firefox",
                "application.process.binary": "firefox", "media.name": "meet.google.com"
            }),
        ));
        graph.apply(classed(
            41,
            "Stream/Input/Audio",
            json!({
                "node.name": "peak", "application.name": "Volume Meter"
            }),
        ));
        // Nothing linked: nothing used.
        let used = graph.in_use();
        assert!(used.microphone.is_empty() && used.camera.is_empty());
        assert_eq!(used.cameras, 0);
        // Linked to the source: Firefox uses it; the meter, linked to a
        // sink's monitor, does not.
        graph.apply(link(50, 30, 40));
        graph.apply(link(51, 31, 41));
        let used = graph.in_use();
        assert_eq!(used.microphone.len(), 1);
        let firefox = &used.microphone[0];
        assert_eq!(firefox.app, "Firefox");
        assert_eq!(firefox.media.as_deref(), Some("meet.google.com"));
        assert!(firefox.ids.contains(&"firefox".to_owned()));
        assert!(!used.muted);
        // It records the source.
        let recorders = graph.recorders();
        assert_eq!(recorders.len(), 1);
        assert_eq!(
            (recorders[0].name.as_str(), recorders[0].sources.as_slice()),
            ("Firefox", &[30][..])
        );
        // An update of the link without its nodes keeps them.
        graph.apply(
            json!({ "id": 50, "type": "PipeWire:Interface:Link", "info": { "state": "paused" } }),
        );
        assert_eq!(graph.in_use().microphone.len(), 1);
        // The default source muted.
        graph.apply(json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [{ "subject": 0, "key": "default.audio.source",
                           "type": "Spa:String:JSON", "value": { "name": "mic" } }]
        }));
        let mut muted = sink(30, "mic", &[0.5, 0.5], true);
        muted["info"]["props"]["media.class"] = json!("Audio/Source");
        graph.apply(muted);
        assert!(graph.in_use().muted);
        // The link goes: no longer used.
        graph.apply(json!({ "id": 50, "info": null }));
        assert!(graph.in_use().microphone.is_empty());
        assert!(graph.recorders().is_empty());
        // A camera, and a stream linked to it.
        graph.apply(classed(
            60,
            "Video/Source",
            json!({ "node.name": "v4l2_input.usb" }),
        ));
        graph.apply(classed(
            61,
            "Stream/Input/Video",
            json!({
                "node.name": "zoom", "application.name": "Zoom",
                "pipewire.access.portal.app_id": "us.zoom.Zoom"
            }),
        ));
        let used = graph.in_use();
        assert_eq!(used.cameras, 1);
        assert!(used.camera.is_empty());
        graph.apply(link(62, 60, 61));
        let used = graph.in_use();
        assert_eq!(used.camera.len(), 1);
        assert_eq!(used.camera[0].key, "us.zoom.Zoom");
        assert!(used.camera[0].ids.contains(&"us.zoom.zoom".to_owned()));
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
        // Its balance all on the left: the loudest channel says.
        graph.apply(sink(31, "headset", &[0.512, 0.0], false));
        let (_, sound) = graph.default_sink().expect("a default sink");
        assert!((sound.volume - 80.0).abs() < 0.01, "{}", sound.volume);
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
        assert_eq!(sinks[1].bus.as_deref(), Some("bluetooth"));
        assert_eq!(sinks[0].form_factor.as_deref(), Some("internal"));
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
        let mixer = graph.mixer(|_, _| None::<()>);
        assert_eq!(mixer.microphone().map(|s| s.id), Some(41));
        // An update of the sink's alone keeps the source's.
        let mut sink = default.clone();
        sink["metadata"][0]["key"] = json!("default.audio.sink");
        sink["metadata"][0]["value"] = json!({ "name": "speakers" });
        graph.apply(sink);
        assert_eq!(graph.sink.as_deref(), Some("speakers"));
        assert_eq!(
            graph.mixer(|_, _| None::<()>).microphone().map(|s| s.id),
            Some(41)
        );
        // Without a default one (its key gone), the first.
        default["metadata"][0]["value"] = Value::Null;
        graph.apply(default);
        assert_eq!(graph.sink.as_deref(), Some("speakers"));
        assert_eq!(
            graph.mixer(|_, _| None::<()>).microphone().map(|s| s.id),
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
            .map(|a| {
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
        let firefox = graph.apps().into_iter().next();
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
        let firefox = graph.apps().into_iter().next();
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
    fn a_devices_profiles_and_the_one_it_is_in_never_off() {
        let mut graph = Graph::default();
        graph.apply(json!({
            "id": 64, "type": "PipeWire:Interface:Device",
            "info": {
                "props": { "device.name": "bluez_card.AA_BB", "device.description": "Casque",
                           "device.api": "bluez5", "device.bus": "bluetooth" },
                "params": {
                    "EnumProfile": [
                        { "index": 0, "name": "off", "description": "Off", "available": "yes" },
                        { "index": 1, "name": "a2dp-sink", "description": "High Fidelity Playback (A2DP Sink)", "available": "yes" },
                        { "index": 2, "name": "headset-head-unit", "description": "Headset Head Unit (HSP/HFP)", "available": "yes" },
                        { "index": 3, "name": "a2dp-sink-sbc", "description": "SBC", "available": "no" }
                    ],
                    "Profile": [ { "index": 1, "name": "a2dp-sink" } ]
                }
            }
        }));
        let cards = graph.cards();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(
            (card.description.as_str(), card.api.as_deref(), card.profile),
            ("Casque", Some("bluez5"), Some(1))
        );
        let choices: Vec<u32> = card.choices().map(|p| p.index).collect();
        assert_eq!(choices, [1, 2]);
        // Another profile chosen: an update of its params alone.
        graph.apply(json!({
            "id": 64, "type": "PipeWire:Interface:Device",
            "info": { "params": { "Profile": [ { "index": 2 } ] } }
        }));
        let card = &graph.cards()[0];
        assert_eq!(
            (card.profile, card.description.as_str()),
            (Some(2), "Casque")
        );
    }

    #[test]
    fn a_streams_target_its_sink_and_the_chosen_defaults() {
        let mut graph = Graph::default();
        let mut speakers = sink(30, "speakers", &[0.5, 0.25], false);
        speakers["info"]["props"]["object.serial"] = json!(130);
        speakers["info"]["params"]["Props"][0]["channelMap"] = json!(["FL", "FR"]);
        graph.apply(speakers);
        graph.apply(sink(31, "headset", &[0.5], false));
        graph.apply(node(
            60,
            json!({ "node.name": "music", "media.class": "Stream/Output/Audio",
                    "application.name": "Musique", "object.serial": "160" }),
            "running",
            &[1.0; 2],
            false,
        ));
        graph.apply(link(70, 60, 30));
        graph.apply(json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [
                { "subject": 0, "key": "default.configured.audio.sink", "type": "Spa:String:JSON",
                  "value": { "name": "headset" } },
                { "subject": 60, "key": "target.object", "type": "Spa:Id", "value": "130" }
            ]
        }));
        let mixer = graph.mixer(|_, _| None::<()>);
        let music = &mixer.apps[0];
        assert_eq!(
            (music.sink, music.target.as_deref()),
            (Some(30), Some("130"))
        );
        assert_eq!(mixer.configured_sink.as_deref(), Some("headset"));
        let speakers = &mixer.sinks[0];
        assert_eq!(speakers.serial, Some(130));
        assert_eq!(speakers.channels, [0.5, 0.25]);
        assert_eq!(speakers.positions, ["FL", "FR"]);
        assert!(!speakers.mono() && mixer.sinks[1].mono());
        // Its target taken away: it follows the default again.
        graph.apply(json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [ { "subject": 60, "key": "target.object", "value": null } ]
        }));
        let mixer = graph.mixer(|_, _| None::<()>);
        assert_eq!(mixer.apps[0].target, None);
        assert_eq!(mixer.configured_sink.as_deref(), Some("headset"));
        // As pw-dump says a key taken away: nothing listed; read whole
        // again, as pw-metadata lists it.
        graph.apply(json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [ { "subject": 60, "key": "target.object", "value": "130" } ]
        }));
        assert_eq!(graph.apps()[0].target.as_deref(), Some("130"));
        graph.apply(json!({
            "id": 29, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" }, "metadata": []
        }));
        assert!(graph.take_lost() && !graph.take_lost());
        graph.reset_metadata(&[(
            0,
            "default.audio.sink".into(),
            "{\"name\":\"speakers\"}".into(),
        )]);
        let mixer = graph.mixer(|_, _| None::<()>);
        assert_eq!(mixer.apps[0].target, None);
        assert_eq!(mixer.configured_sink, None);
        assert!(mixer.sinks[0].default);
    }
}
