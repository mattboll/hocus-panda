//! The levels near the bar's bottom (bar.md §4 group 10, §5): the sound,
//! the brightness and the battery as gauges, the battery's time left and
//! the power profile; and the keyboard's level keys, which the compositor
//! hands over.
//!
//! The shell owns the services: the compositor stays small, and only the
//! shell shows them. Each is read on a thread of its own, never on the
//! loop that draws, and pushes its changes:
//!
//! - the battery and the power profile from UPower and power-profiles-
//!   daemon on the system bus ([`power`]), which also sets the brightness
//!   through logind (`SetBrightness` on the session: no root, no helper);
//! - the backlight from sysfs, its changes from udev ([`backlight`]);
//! - the sound from PipeWire: `pw-dump --monitor` streams the graph's
//!   changes (the default sink, its volume and mute; the outputs, the
//!   microphones and the applications' streams, for the popover), and
//!   `wpctl` sets them ([`sound`]); the applications' icons are found as
//!   the freedesktop specifications say ([`icons`]).
//!
//! A service missing hides its row or its part, says so once in the log,
//! and is picked up when it comes: from the bus's name changes, from udev,
//! or, for PipeWire, which has nothing to announce it, by trying again a
//! little later each time, up to a minute.
//!
//! What they read goes into one [`Snapshot`], which the levels' surface
//! ([`draw`]) reads when it draws. That surface is its own, as the clock's
//! is, so that a change draws only its region; and a change reaches it
//! without a message to iced, which would draw every surface: the shell
//! asks the compositor to wake that surface (`hocus_link::ToCompositor::
//! Wake`). Alone, without a compositor's link, a change is a message.

pub mod backlight;
pub mod draw;
pub mod icons;
pub mod model;
pub mod power;
pub mod sound;

use std::{
    sync::{Mutex, OnceLock, PoisonError, mpsc},
    time::Duration,
};

use hocus_link::LevelKey;

pub use self::model::Snapshot;

/// Says that the levels changed; `false` once whoever listened is gone.
type Waker = Box<dyn FnMut() -> bool + Send>;

struct Service {
    levels: Mutex<Snapshot>,
    waker: Mutex<Option<Waker>>,
    sound: mpsc::Sender<sound::Ask>,
    brightness: tokio::sync::mpsc::UnboundedSender<LevelKey>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

/// Starts the services' threads, once.
pub fn start() {
    let (sound, sound_keys) = mpsc::channel();
    let (brightness, brightness_keys) = tokio::sync::mpsc::unbounded_channel();
    let service = Service {
        levels: Mutex::new(Snapshot::default()),
        waker: Mutex::new(None),
        sound,
        brightness,
    };
    if SERVICE.set(service).is_err() {
        return;
    }
    sound::start(sound_keys);
    backlight::start();
    power::start(brightness_keys);
}

/// The levels as last read.
pub fn snapshot() -> Snapshot {
    SERVICE.get().map_or_else(Snapshot::default, |s| {
        *s.levels.lock().unwrap_or_else(PoisonError::into_inner)
    })
}

/// Changes the levels with `change`; if they changed, wakes the surface.
pub fn publish(change: impl FnOnce(&mut Snapshot)) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let changed = {
        let mut levels = service
            .levels
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let before = *levels;
        change(&mut levels);
        *levels != before
    };
    if changed {
        tracing::debug!(levels = ?snapshot(), "the levels changed");
        let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
        if waker.as_mut().is_some_and(|wake| !wake()) {
            *waker = None;
        }
    }
}

/// From now on, a change calls `wake` (until it returns `false`); it is
/// called once now, for what changed before.
pub fn on_change(wake: impl FnMut() -> bool + Send + 'static) {
    if let Some(service) = SERVICE.get() {
        let mut wake: Waker = Box::new(wake);
        let alive = wake();
        *service.waker.lock().unwrap_or_else(PoisonError::into_inner) = alive.then_some(wake);
    }
}

/// A level key the compositor handed over.
pub fn key(key: LevelKey) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let sent = match key {
        LevelKey::VolumeUp | LevelKey::VolumeDown | LevelKey::Mute => {
            service.sound.send(sound::Ask::Key(key)).is_ok()
        }
        LevelKey::BrightnessUp | LevelKey::BrightnessDown => service.brightness.send(key).is_ok(),
    };
    if !sent {
        tracing::warn!(
            key = key.name(),
            "the level's service has stopped: key dropped"
        );
    }
}

/// Asks the sound for a volume or another default sink (the popover).
pub fn ask(ask: sound::Ask) {
    let sent = SERVICE
        .get()
        .is_some_and(|service| service.sound.send(ask).is_ok());
    if !sent {
        tracing::warn!(?ask, "the sound's service has stopped: dropped");
    }
}

/// A service that is away is tried again after this, doubled each time it
/// is still away, up to [`RETRY_LAST`].
pub(crate) const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LAST: Duration = Duration::from_secs(60);

/// The next wait before trying a missing service again.
pub(crate) fn next_retry(wait: Duration) -> Duration {
    (wait * 2).min(RETRY_LAST)
}

/// Whether a service is there, to say once when it goes and comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Unknown,
    There,
    Away,
}

impl Presence {
    /// Logs what changed: `what` is there, or away (`why`).
    pub(crate) fn now(&mut self, there: bool, what: &str, why: impl std::fmt::Display) {
        let next = if there { Self::There } else { Self::Away };
        match (*self, next) {
            (Self::Away, Self::There) => tracing::info!("{what}: back"),
            (Self::Unknown | Self::There, Self::Away) => {
                tracing::info!("{what}: not there ({why}); hidden until it comes");
            }
            _ => {}
        }
        *self = next;
    }
}
