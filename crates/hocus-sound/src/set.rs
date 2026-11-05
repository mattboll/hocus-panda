//! Setting the sound as the user would, with PipeWire's and WirePlumber's
//! own tools, on a thread of its own: `wpctl` for a volume, a mute, the
//! default sink or source and a device's profile; `pw-cli` for the
//! channels' volumes (the balance); `pw-metadata` for the output an
//! application plays on. What was asked while the last commands ran is
//! taken together: a slider dragged sends many volumes, and only the last
//! goes to PipeWire.

use std::{
    collections::{BTreeMap, HashMap},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        mpsc::{Receiver, TryRecvError},
    },
};

use crate::{Graph, STEP};

/// A volume key, for the default sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    /// Mute toggles.
    Mute,
}

/// What is asked of the sound: a key's step or mute, a volume set (the
/// popover's slider, in percent), another default sink (by node id); a
/// volume or a mute for a node of its own (a sink, a microphone, an
/// application's stream), another default source; a node's channels'
/// linear volumes (its balance), a device's profile, the output a stream
/// plays on (`None`: the default).
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Step(Step),
    Volume(f32),
    Default(u32),
    Node(u32, Set),
    DefaultSource(u32),
    Channels(u32, Vec<f32>),
    Profile { device: u32, index: u32 },
    Target { stream: u32, to: Option<u32> },
}

/// What is set on a node of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Set {
    /// In percent.
    Volume(f32),
    Mute(bool),
}

/// What the setter knows of the graph: the default sink and whether it is
/// muted, and each node's mute and channels' volumes. The follower
/// teaches it ([`Known::learn`]) after each change.
#[derive(Debug, Clone, Default)]
pub struct Known(Arc<Mutex<Knowing>>);

#[derive(Debug, Default)]
struct Knowing {
    default: Option<u32>,
    muted: HashMap<u32, bool>,
    channels: HashMap<u32, Vec<f32>>,
}

impl Known {
    pub fn learn(&self, graph: &Graph) {
        let mut known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        known.default = graph.default_sink().map(|(id, _)| id);
        known.muted.clear();
        known.channels.clear();
        for sink in graph.sinks().into_iter().chain(graph.sources()) {
            if let Some(sound) = sink.sound {
                known.muted.insert(sink.id, sound.muted);
            }
            known.channels.insert(sink.id, sink.channels);
        }
        for app in graph.apps() {
            for id in app.streams {
                if let Some(muted) = graph.muted(id) {
                    known.muted.insert(id, muted);
                }
            }
        }
    }

    /// PipeWire is gone: nothing is known.
    pub fn forget(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Knowing::default();
    }

    fn default_sink(&self) -> Option<(u32, bool)> {
        let known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let id = known.default?;
        Some((id, known.muted.get(&id).copied().unwrap_or(false)))
    }

    fn muted(&self, id: u32) -> bool {
        let known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        known.muted.get(&id).copied().unwrap_or(false)
    }

    /// Node `id`'s channels, when they are not all at one volume (its
    /// balance is not centred): a volume then keeps their ratio.
    fn unequal(&self, id: u32) -> Option<Vec<f32>> {
        let known = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let channels = known.channels.get(&id)?;
        let first = *channels.first()?;
        channels
            .iter()
            .any(|c| (c - first).abs() > 1e-4)
            .then(|| channels.clone())
    }
}

/// `channels` scaled so that the loudest is at `percent` (as `wpctl`
/// shows a volume: the cube root of the linear one).
pub fn scaled(channels: &[f32], percent: f32) -> Vec<f32> {
    let loudest = channels.iter().copied().fold(0.0_f32, f32::max);
    let to = (percent.clamp(0.0, 100.0) / 100.0).powi(3);
    if loudest <= 0.0 {
        return vec![to; channels.len()];
    }
    channels.iter().map(|c| c / loudest * to).collect()
}

/// A command: its program and its arguments.
pub type Line = (&'static str, Vec<String>);

/// What was asked so far, taken together: a volume set and the steps after
/// it (up less down), whether the volume was touched (which unmutes),
/// whether to toggle mute, and another default sink; the last volume and
/// mute asked of each other node, and another default source; the last
/// channels, profile and target asked of each.
#[derive(Debug, Default, PartialEq)]
pub struct Asked {
    pub steps: i32,
    pub volume: bool,
    pub toggle: bool,
    pub to: Option<f32>,
    pub default: Option<u32>,
    pub nodes: BTreeMap<u32, (Option<f32>, Option<bool>)>,
    pub source: Option<u32>,
    pub channels: BTreeMap<u32, Vec<f32>>,
    pub profiles: BTreeMap<u32, u32>,
    pub targets: BTreeMap<u32, Option<u32>>,
}

impl Asked {
    pub fn add(&mut self, ask: Ask) {
        let step = match ask {
            Ask::Step(step) => step,
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
                    Set::Volume(to) => {
                        node.0 = Some(to.clamp(0.0, 100.0));
                        // Its channels are set by the volume now.
                        self.channels.remove(&id);
                    }
                    Set::Mute(muted) => node.1 = Some(muted),
                }
                return;
            }
            Ask::DefaultSource(id) => {
                self.source = Some(id);
                return;
            }
            Ask::Channels(id, channels) => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.0 = None;
                }
                self.channels.insert(id, channels);
                return;
            }
            Ask::Profile { device, index } => {
                self.profiles.insert(device, index);
                return;
            }
            Ask::Target { stream, to } => {
                self.targets.insert(stream, to);
                return;
            }
        };
        match step {
            Step::Up => {
                self.steps += 1;
                self.volume = true;
            }
            Step::Down => {
                self.steps -= 1;
                self.volume = true;
            }
            Step::Mute => self.toggle = !self.toggle,
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

    /// The commands for the other nodes: another default source, the
    /// devices' profiles, the nodes' volumes, then their mutes, their
    /// channels, the streams' targets. `unequal` says a node's channels
    /// when they are not at one volume.
    pub fn node_commands(&self, unequal: impl Fn(u32) -> Option<Vec<f32>>) -> Vec<Line> {
        let mut commands = Vec::new();
        if let Some(id) = self.source {
            commands.push(wpctl(&["set-default", &id.to_string()]));
        }
        for (device, index) in &self.profiles {
            commands.push(wpctl(&[
                "set-profile",
                &device.to_string(),
                &index.to_string(),
            ]));
        }
        for (id, (volume, muted)) in &self.nodes {
            if let Some(to) = volume {
                commands.push(volume_of(*id, *to, unequal(*id)));
            }
            if let Some(muted) = muted {
                let muted = if *muted { "1" } else { "0" };
                commands.push(wpctl(&["set-mute", &id.to_string(), muted]));
            }
        }
        for (id, channels) in &self.channels {
            commands.push(channels_of(*id, channels));
        }
        for (stream, to) in &self.targets {
            let stream = stream.to_string();
            commands.push(match to {
                Some(serial) => (
                    "pw-metadata",
                    [
                        "-n",
                        "default",
                        &stream,
                        "target.object",
                        &serial.to_string(),
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                ),
                None => (
                    "pw-metadata",
                    ["-n", "default", "-d", &stream, "target.object"]
                        .map(str::to_owned)
                        .to_vec(),
                ),
            });
        }
        // The metadata read once more after the targets written: before
        // PipeWire 1.0.9 (Ubuntu 24.04 has 1.0.5), `pw-dump --monitor`
        // writes a metadata change out only at the graph's next change, and
        // the writing `pw-metadata` connected (its change) before writing.
        // A client connecting after the write is such a change; without
        // it, the target chosen would not show until something else moved.
        if !self.targets.is_empty() {
            commands.push(("pw-metadata", ["-n", "default"].map(str::to_owned).to_vec()));
        }
        commands
    }

    /// The commands for node `id` (the default sink, or the one asked to
    /// be), muted or not now, its channels when they are unequal (a
    /// volume then keeps their ratio).
    pub fn commands(&self, id: u32, muted: bool, unequal: Option<Vec<f32>>) -> Vec<Line> {
        let node = id.to_string();
        let mut commands = Vec::new();
        if self.default.is_some() {
            commands.push(wpctl(&["set-default", &node]));
        }
        if let Some(to) = self.to {
            commands.push(volume_of(id, to, unequal.clone()));
        }
        if self.steps != 0 {
            let percent = STEP as i32 * self.steps.abs();
            match &unequal {
                // Unequal channels keep their ratio, the loudest stepped.
                Some(channels) => {
                    let loudest = channels.iter().copied().fold(0.0_f32, f32::max);
                    let now = loudest.max(0.0).cbrt() * 100.0;
                    let to = now + (STEP * self.steps as f32);
                    commands.push(channels_of(id, &scaled(channels, to)));
                }
                None => {
                    let sign = if self.steps > 0 { '+' } else { '-' };
                    commands.push(wpctl(&[
                        "set-volume",
                        "--limit",
                        "1.0",
                        &node,
                        &format!("{percent}%{sign}"),
                    ]));
                }
            }
        }
        // A volume key unmutes; mute toggles (after it, if both).
        let unmute = self.volume && muted;
        match (unmute, self.toggle) {
            (true, false) => commands.push(wpctl(&["set-mute", &node, "0"])),
            (false, true) => commands.push(wpctl(&["set-mute", &node, "toggle"])),
            _ => {}
        }
        commands
    }
}

fn wpctl(args: &[&str]) -> Line {
    ("wpctl", args.iter().map(|a| (*a).to_owned()).collect())
}

/// Node `id` at `percent`: by `wpctl`, or its channels scaled when they
/// are unequal (`wpctl` would set them all alike, losing the balance).
fn volume_of(id: u32, percent: f32, unequal: Option<Vec<f32>>) -> Line {
    match unequal {
        Some(channels) => channels_of(id, &scaled(&channels, percent)),
        None => wpctl(&[
            "set-volume",
            &id.to_string(),
            &format!("{:.3}", percent / 100.0),
        ]),
    }
}

fn channels_of(id: u32, channels: &[f32]) -> Line {
    let list: Vec<String> = channels.iter().map(|c| format!("{c:.6}")).collect();
    (
        "pw-cli",
        vec![
            "set-param".to_owned(),
            id.to_string(),
            "Props".to_owned(),
            format!("{{ channelVolumes: [ {} ] }}", list.join(", ")),
        ],
    )
}

/// `first` and what was asked while the last commands ran, taken
/// together; and another default sink asked after something for the one
/// before, which waits for the next batch. `None`: the asks' sender is
/// gone.
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

/// Sets what is asked until the asks' sender goes, what was asked while
/// the last commands ran taken together.
pub fn run(asks: &Receiver<Ask>, known: &Known) {
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
            let target = match asked.default {
                Some(id) => Some((id, known.muted(id))),
                None => known.default_sink(),
            };
            match target {
                Some((id, muted)) => commands = asked.commands(id, muted, known.unequal(id)),
                None => tracing::debug!("no default sink: the key does nothing"),
            }
        }
        commands.extend(asked.node_commands(|id| known.unequal(id)));
        for (program, args) in commands {
            match Command::new(program)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(program, ?args, %status, "the sound is not set"),
                Err(err) => tracing::warn!(program, ?args, %err, "cannot set the sound"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(program: &'static str, c: &[&str]) -> Line {
        (program, c.iter().map(|s| (*s).to_owned()).collect())
    }

    fn w(c: &[&str]) -> Line {
        line("wpctl", c)
    }

    #[test]
    fn a_slider_dragged_and_an_output_chosen_make_few_commands() {
        let mut asked = Asked::default();
        for v in [10.0, 20.0, 35.5, 42.0] {
            asked.add(Ask::Volume(v));
        }
        // Only the last volume; unmuted, as the slider is moved.
        assert_eq!(
            asked.commands(30, true, None),
            vec![
                w(&["set-volume", "30", "0.420"]),
                w(&["set-mute", "30", "0"])
            ]
        );
        // Keys after it step from it.
        asked.add(Ask::Step(Step::Up));
        assert_eq!(
            asked.commands(30, false, None),
            vec![
                w(&["set-volume", "30", "0.420"]),
                w(&["set-volume", "--limit", "1.0", "30", "5%+"])
            ]
        );
        // Another default sink, its own volume kept.
        let mut asked = Asked::default();
        asked.add(Ask::Default(31));
        assert_eq!(
            asked.commands(31, false, None),
            vec![w(&["set-default", "31"])]
        );
        // Out of range: kept within 0 and 100 %.
        let mut asked = Asked::default();
        asked.add(Ask::Volume(130.0));
        assert_eq!(
            asked.commands(7, false, None),
            vec![w(&["set-volume", "7", "1.000"])]
        );
    }

    #[test]
    fn a_mute_for_one_output_is_not_taken_by_the_next() {
        let mut keys = Asked::default();
        keys.add(Ask::Step(Step::Up));
        assert!(keys.for_the_default());
        let (send, asks) = std::sync::mpsc::channel();
        for ask in [Ask::Default(31), Ask::Volume(40.0)] {
            send.send(ask).expect("sent");
        }
        let (first, next) = batch(Ask::Step(Step::Mute), &asks).expect("a batch");
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
        let asked = |keys: &[Step]| {
            let mut asked = Asked::default();
            for key in keys {
                asked.add(Ask::Step(*key));
            }
            asked
        };
        let (up, down, mute) = (Step::Up, Step::Down, Step::Mute);
        assert_eq!(
            asked(&[up]).commands(30, false, None),
            vec![w(&["set-volume", "--limit", "1.0", "30", "5%+"])]
        );
        assert_eq!(
            asked(&[up, up, down, down, down]).commands(30, false, None),
            vec![w(&["set-volume", "--limit", "1.0", "30", "5%-"])]
        );
        // A volume key unmutes.
        assert_eq!(
            asked(&[up]).commands(7, true, None),
            vec![
                w(&["set-volume", "--limit", "1.0", "7", "5%+"]),
                w(&["set-mute", "7", "0"])
            ]
        );
        assert_eq!(
            asked(&[up, down]).commands(7, true, None),
            vec![w(&["set-mute", "7", "0"])]
        );
        assert_eq!(
            asked(&[mute]).commands(7, false, None),
            vec![w(&["set-mute", "7", "toggle"])]
        );
        assert_eq!(
            asked(&[mute, mute]).commands(7, false, None),
            Vec::<Line>::new()
        );
    }

    #[test]
    fn microphones_and_applications_asks_make_few_commands() {
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
            asked.node_commands(|_| None),
            vec![
                w(&["set-default", "41"]),
                w(&["set-mute", "60", "0"]),
                w(&["set-volume", "61", "0.550"]),
                w(&["set-mute", "61", "0"]),
                w(&["set-mute", "62", "0"]),
            ]
        );
        // Asked of the default sink too: both.
        asked.add(Ask::Step(Step::Up));
        assert!(asked.for_a_sink());
        // Another default source does not wait for the next batch.
        let (send, asks) = std::sync::mpsc::channel();
        send.send(Ask::DefaultSource(40)).expect("sent");
        let (both, next) = batch(Ask::Step(Step::Mute), &asks).expect("a batch");
        assert_eq!((both.toggle, both.source, next), (true, Some(40), None));
    }

    #[test]
    fn a_balance_a_profile_and_a_target_make_their_commands() {
        let mut asked = Asked::default();
        asked.add(Ask::Channels(30, vec![1.0, 0.5]));
        asked.add(Ask::Channels(30, vec![0.5, 1.0]));
        asked.add(Ask::Profile {
            device: 64,
            index: 2,
        });
        asked.add(Ask::Target {
            stream: 90,
            to: Some(130),
        });
        asked.add(Ask::Target {
            stream: 91,
            to: None,
        });
        assert_eq!(
            asked.node_commands(|_| None),
            vec![
                w(&["set-profile", "64", "2"]),
                line(
                    "pw-cli",
                    &[
                        "set-param",
                        "30",
                        "Props",
                        "{ channelVolumes: [ 0.500000, 1.000000 ] }"
                    ]
                ),
                line(
                    "pw-metadata",
                    &["-n", "default", "90", "target.object", "130"]
                ),
                line(
                    "pw-metadata",
                    &["-n", "default", "-d", "91", "target.object"]
                ),
                // Read once more, which an older pw-dump needs to write
                // the targets out.
                line("pw-metadata", &["-n", "default"]),
            ]
        );
        // A volume on a node whose balance is not centred keeps it: the
        // loudest channel at the volume.
        let mut asked = Asked::default();
        asked.add(Ask::Node(30, Set::Volume(50.0)));
        let unequal = |_| Some(vec![0.8, 0.4]);
        assert_eq!(
            asked.node_commands(unequal),
            vec![line(
                "pw-cli",
                &[
                    "set-param",
                    "30",
                    "Props",
                    "{ channelVolumes: [ 0.125000, 0.062500 ] }"
                ]
            )]
        );
        // So does a key, the loudest stepped by 5 %.
        let mut asked = Asked::default();
        asked.add(Ask::Step(Step::Down));
        let commands = asked.commands(30, false, Some(vec![0.125, 0.0625]));
        assert_eq!(commands[0].0, "pw-cli");
        assert!(
            commands[0].1[3].contains("0.091125, 0.045562"),
            "{commands:?}"
        );
    }

    #[test]
    fn scaled_channels_keep_their_ratio() {
        assert_eq!(scaled(&[1.0, 0.5], 50.0), vec![0.125, 0.0625]);
        assert_eq!(scaled(&[0.0, 0.0], 100.0), vec![1.0, 1.0]);
        assert_eq!(scaled(&[0.2, 0.4], 150.0), vec![0.5, 1.0]);
    }
}
