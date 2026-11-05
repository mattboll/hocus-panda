//! The sound: the outputs (sinks), the inputs (sources), the streams
//! applications play, and the default output's volume and mute, from
//! PipeWire, through the crate the Settings share (`hocus-sound`).
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
    sync::{Mutex, PoisonError, mpsc::Receiver},
    time::{Duration, Instant},
};

pub use hocus_sound::{Ask, Kind, Set, Sink, Step};
use hocus_sound::{Known, Using};

use super::{Presence, RETRY_FIRST, icons::Icon, next_retry, publish};

/// What the sound's popover lists, each by id, with the applications'
/// icons.
pub type Mixer = hocus_sound::Mixer<Icon>;
/// An application playing sound, its streams taken together.
pub type App = hocus_sound::App<Icon>;

/// A `pw-dump` that ran this long before ending did not fail to start:
/// the next try comes at once.
const STEADY: Duration = Duration::from_secs(60);

pub fn start(keys: Receiver<Ask>) {
    let known = Known::default();
    let reader = known.clone();
    let started = std::thread::Builder::new()
        .name("levels: sound".to_owned())
        .spawn(move || follow(&reader))
        .and_then(|_| {
            std::thread::Builder::new()
                .name("levels: sound keys".to_owned())
                .spawn(move || hocus_sound::set::run(&keys, &known))
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow the sound");
    }
}

/// Follows the graph, starting `pw-dump` again whenever it ends.
fn follow(known: &Known) {
    let mut presence = Presence::Unknown;
    let mut wait = RETRY_FIRST;
    loop {
        let started = Instant::now();
        let ended = dump(known, &mut presence);
        known.forget();
        publish(|levels| levels.sound = None);
        publish_mixer(Mixer::new());
        crate::in_use::cameras::nodes(0);
        crate::in_use::set_media(Vec::new(), false, Vec::new());
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
fn dump(known: &Known, presence: &mut Presence) -> String {
    let mut icons: HashMap<String, Option<Icon>> = HashMap::new();
    hocus_sound::dump::dump(|graph| {
        known.learn(graph);
        let sound = graph.default_sink();
        if sound.is_some() {
            presence.now(true, "PipeWire", "");
        }
        publish(|levels| levels.sound = sound.map(|(_, sound)| sound));
        publish_mixer(graph.mixer(|key, names| icon(&mut icons, key, names)));
        let used = graph.in_use();
        crate::in_use::cameras::nodes(used.cameras);
        let media = |using: Vec<Using>, icons: &mut HashMap<String, Option<Icon>>| {
            using
                .into_iter()
                .map(|u| crate::in_use::Media {
                    icon: icon(icons, &u.key, &u.icon_names),
                    key: u.key,
                    app: u.app,
                    ids: u.ids,
                    media: u.media,
                })
                .collect()
        };
        let microphone = media(used.microphone, &mut icons);
        let camera = media(used.camera, &mut icons);
        crate::in_use::set_media(microphone, used.muted, camera);
    })
}

/// An application's icon, looked for once (`icons`, by application).
fn icon(icons: &mut HashMap<String, Option<Icon>>, key: &str, names: &[String]) -> Option<Icon> {
    icons
        .entry(key.to_owned())
        .or_insert_with(|| {
            let icon = super::icons::find(names);
            tracing::debug!(
                app = key,
                ?names,
                found = icon.is_some(),
                "an application's icon"
            );
            icon
        })
        .clone()
}

/// The shell ends: its `pw-dump` too.
pub fn stop() {
    hocus_sound::dump::stop();
}

/// The outputs, the inputs and the applications as last read, for the
/// sound's popover.
static MIXER: Mutex<Mixer> = Mutex::new(Mixer::new());

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
        crate::popover::changed(crate::popover::Item::Sound);
    }
}
