//! The sound: the outputs (sinks), the default one's volume and mute, from
//! PipeWire.
//!
//! `pw-dump --monitor` (PipeWire's own tool) streams the graph's changes as
//! JSON: the `default` metadata names the default sink, each sink's node
//! its description and its `Props` (volumes, mute), its device what it is
//! (a headset, a screen). No polling: it writes when something changes,
//! and the shell follows a new default sink as it follows a new volume.
//! `wpctl` (WirePlumber's) sets them as the user would: a volume, a mute,
//! the default sink, each sink keeping its own volume. Both run as
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
        publish_sinks(Vec::new());
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
        publish_sinks(graph.sinks());
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

/// The sinks as last read, for the sound's popover.
static SINKS: Mutex<Vec<Sink>> = Mutex::new(Vec::new());

/// The sinks as last read, by id.
pub fn sinks() -> Vec<Sink> {
    SINKS.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The sinks now `sinks`: the popover shows the change.
fn publish_sinks(sinks: Vec<Sink>) {
    let changed = {
        let mut kept = SINKS.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *kept != sinks;
        *kept = sinks;
        changed
    };
    if changed {
        crate::popover::changed();
    }
}

/// An output, as the popover lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub description: String,
    pub kind: Kind,
    /// Its own volume and mute: each keeps its own.
    pub sound: Option<Sound>,
    pub default: bool,
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
    /// The default sink's name, from the `default` metadata.
    sink: Option<String>,
    /// The `default` metadata's id, to see it go.
    metadata: Option<u32>,
    /// Audio sinks by id: their name, and their volume and mute.
    nodes: HashMap<u32, Node>,
    /// Devices by id: what they are.
    devices: HashMap<u32, Device>,
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    description: Option<String>,
    device: Option<u32>,
    /// What its own props say it is, if they do.
    what: Device,
    sound: Option<Sound>,
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
        self.sink = entries
            .iter()
            .filter(|e| e.get("subject").and_then(Value::as_u64) == Some(0))
            .find(|e| e.get("key").and_then(Value::as_str) == Some("default.audio.sink"))
            .and_then(|e| e.get("value"))
            .and_then(sink_name);
    }

    fn node(&mut self, id: u32, object: &Value) {
        let Some(info) = object.get("info") else {
            return;
        };
        let class = info.pointer("/props/media.class").and_then(Value::as_str);
        if class != Some("Audio/Sink") {
            return;
        }
        let node = self.nodes.entry(id).or_default();
        if let Some(props) = info.get("props") {
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
        }
        // An update without the params keeps those known.
        if let Some(props) = info.pointer("/params/Props").and_then(Value::as_array) {
            node.sound = props.iter().find_map(sound_of);
        }
    }

    /// The sinks, by id: what they are called, what they are, their own
    /// sound, and which is the default.
    pub fn sinks(&self) -> Vec<Sink> {
        let default = self.default_sink().map(|(id, _)| id);
        let mut sinks: Vec<Sink> = self
            .nodes
            .iter()
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
                    default: default == Some(*id),
                }
            })
            .collect();
        sinks.sort_by_key(|s| s.id);
        sinks
    }

    /// The default sink's node id and its sound.
    pub fn default_sink(&self) -> Option<(u32, Sound)> {
        let name = self.sink.as_deref()?;
        self.nodes
            .iter()
            .filter(|(_, node)| node.name == name)
            .find_map(|(id, node)| node.sound.map(|sound| (*id, sound)))
    }
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
/// (the popover's slider, in percent), another default sink (by node id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ask {
    Key(LevelKey),
    Volume(f32),
    Default(u32),
}

/// What was asked so far, taken together: a volume set and the steps after
/// it (up less down), whether the volume was touched (which unmutes),
/// whether to toggle mute, and another default sink.
#[derive(Debug, Default, PartialEq)]
pub struct Asked {
    pub steps: i32,
    pub volume: bool,
    pub toggle: bool,
    pub to: Option<f32>,
    pub default: Option<u32>,
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
        let current = *default.lock().unwrap_or_else(PoisonError::into_inner);
        let target = match (asked.default, current) {
            (Some(id), _) => sinks()
                .iter()
                .find(|s| s.id == id)
                .map(|s| (id, s.sound.is_some_and(|s| s.muted))),
            (None, current) => current,
        };
        let Some((id, muted)) = target else {
            tracing::debug!("no default sink: the key does nothing");
            continue;
        };
        for args in asked.commands(id, muted) {
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
}
