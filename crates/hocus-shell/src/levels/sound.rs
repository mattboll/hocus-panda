//! The sound: the default output's volume and mute, from PipeWire.
//!
//! `pw-dump --monitor` (PipeWire's own tool) streams the graph's changes as
//! JSON: the `default` metadata names the default sink, and that node's
//! `Props` hold its volumes and its mute. No polling: it writes when
//! something changes, and the shell follows a new default sink as it
//! follows a new volume. `wpctl` (WirePlumber's) sets them, on the node
//! found, as the user would. Both run as programs, on threads of their own;
//! linking to libpipewire would need its headers to build, for the same
//! result.
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

pub fn start(keys: Receiver<LevelKey>) {
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

/// What the shell keeps of PipeWire's graph.
#[derive(Debug, Default)]
pub struct Graph {
    /// The default sink's name, from the `default` metadata.
    sink: Option<String>,
    /// The `default` metadata's id, to see it go.
    metadata: Option<u32>,
    /// Audio sinks by id: their name, and their volume and mute.
    nodes: HashMap<u32, Node>,
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    sound: Option<Sound>,
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
            if self.metadata == Some(id) {
                self.metadata = None;
                self.sink = None;
            }
            return;
        }
        match object.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Metadata") => self.metadata(id, &object),
            Some("PipeWire:Interface:Node") => self.node(id, &object),
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
        if let Some(name) = info.pointer("/props/node.name").and_then(Value::as_str) {
            name.clone_into(&mut node.name);
        }
        // An update without the params keeps those known.
        if let Some(props) = info.pointer("/params/Props").and_then(Value::as_array) {
            node.sound = props.iter().find_map(sound_of);
        }
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

/// What the keys pressed so far ask for, taken together: volume steps
/// (up less down), and whether to toggle mute.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub steps: i32,
    pub volume: bool,
    pub toggle: bool,
}

impl Asked {
    pub fn add(&mut self, key: LevelKey) {
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

    /// `wpctl`'s commands for node `id`, muted or not now.
    pub fn commands(&self, id: u32, muted: bool) -> Vec<Vec<String>> {
        let id = id.to_string();
        let mut commands = Vec::new();
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

/// Sets the volume and the mute as the keys ask, those pressed while the
/// last command ran taken together.
fn set(keys: &Receiver<LevelKey>, default: &Default) {
    while let Ok(key) = keys.recv() {
        let mut asked = Asked::default();
        asked.add(key);
        loop {
            match keys.try_recv() {
                Ok(key) => asked.add(key),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let Some((id, muted)) = *default.lock().unwrap_or_else(PoisonError::into_inner) else {
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
    fn keys_taken_together_make_few_commands() {
        let asked = |keys: &[LevelKey]| {
            let mut asked = Asked::default();
            for key in keys {
                asked.add(*key);
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
