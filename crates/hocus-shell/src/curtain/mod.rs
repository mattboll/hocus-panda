//! The curtain's pill (7.2, variant A, docs/design/idle.md): while the
//! compositor dims the screens before the lock (`curtain …`), a pill on
//! the screen it names says how long is left, and how to stay; a panda
//! falls asleep in it. It goes at `curtain-end`.
//!
//! The compositor draws the dim, the pill's shadow and its motion in and
//! out; this draws the pill itself, its countdown at most ten times a
//! second, nothing once it is gone.

pub mod draw;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{CURTAIN_NAMESPACE, CurtainShown};
use iced::window;

use crate::{clock::time::Lang, popover::Effect};

/// The pill as the shell shows it.
#[derive(Debug, Clone)]
pub struct Pill {
    pub shown: CurtainShown,
    /// When the compositor said it: the countdown runs from there.
    pub told: Instant,
    pub surface: window::Id,
}

impl Pill {
    /// How long it has been up, at `now`.
    pub fn elapsed(&self, now: Instant) -> Duration {
        Duration::from_millis(u64::from(self.shown.elapsed))
            + now.saturating_duration_since(self.told)
    }

    pub fn length(&self) -> Duration {
        Duration::from_millis(u64::from(self.shown.length))
    }

    /// Its share gone, from 0 to 1.
    pub fn progress(&self, now: Instant) -> f32 {
        let length = self.length().as_secs_f32();
        if length <= 0.0 {
            return 1.0;
        }
        (self.elapsed(now).as_secs_f32() / length).clamp(0.0, 1.0)
    }

    /// The whole seconds left, at least 1.
    pub fn seconds_left(&self, now: Instant) -> u32 {
        let left = self.length().saturating_sub(self.elapsed(now));
        let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
        u32::try_from(seconds.max(1)).unwrap_or(u32::MAX)
    }
}

static PILL: Mutex<Option<Pill>> = Mutex::new(None);

/// `with` on the pill, if one shows.
pub fn with<T>(with: impl FnOnce(&mut Option<Pill>) -> T) -> T {
    with(&mut PILL.lock().unwrap_or_else(PoisonError::into_inner))
}

pub fn is_surface(id: window::Id) -> bool {
    with(|p| p.as_ref().is_some_and(|p| p.surface == id))
}

/// The compositor's `curtain …`: the pill opens on the screen it names (a
/// new one if that changed), its countdown from what it says.
pub fn set(shown: CurtainShown) -> Vec<Effect> {
    let now = Instant::now();
    with(|slot| {
        let mut effects = Vec::new();
        let same = slot
            .as_ref()
            .is_some_and(|p| p.shown.output == shown.output && p.shown.top == shown.top);
        if !same {
            if let Some(old) = slot.take() {
                effects.push(Effect::Remove(old.surface));
            }
            let surface = window::Id::unique();
            tracing::debug!(output = shown.output, "the curtain's pill");
            effects.push(Effect::CurtainSurface {
                id: surface,
                output: shown.output.clone(),
                top: shown.top,
            });
            *slot = Some(Pill {
                shown,
                told: now,
                surface,
            });
        } else if let Some(pill) = slot.as_mut() {
            pill.shown = shown;
            pill.told = now;
        }
        effects.push(Effect::Wake(CURTAIN_NAMESPACE));
        effects
    })
}

/// The compositor's `curtain-end`: the pill goes.
pub fn end() -> Vec<Effect> {
    with(Option::take)
        .map(|pill| vec![Effect::Remove(pill.surface)])
        .unwrap_or_default()
}

/// The pill's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    /// "Verrouillage dans 7 s".
    pub fn locking_in(self, seconds: u32) -> String {
        if self.fr {
            format!("Verrouillage dans {seconds} s")
        } else {
            format!("Locking in {seconds} s")
        }
    }

    pub fn stay(self) -> &'static str {
        if self.fr {
            "Bouge la souris pour rester"
        } else {
            "Move the mouse to stay"
        }
    }

    /// Its name for a screen reader, said once as it shows (11,
    /// accessibility).
    #[allow(dead_code)]
    pub fn name(self, seconds: u32) -> String {
        if self.fr {
            format!("Verrouillage dans {seconds} secondes. Bouge la souris pour rester.")
        } else {
            format!("Locking in {seconds} seconds. Move the mouse to stay.")
        }
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(elapsed: u32) -> CurtainShown {
        CurtainShown {
            elapsed,
            length: 10_000,
            top: 1010,
            motion: true,
            output: "eDP-1".into(),
        }
    }

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.locking_in(7), "Verrouillage dans 7 s");
        assert_eq!(en.locking_in(1), "Locking in 1 s");
        assert_eq!(fr.stay(), "Bouge la souris pour rester");
        assert_eq!(en.stay(), "Move the mouse to stay");
        assert_eq!(en.name(7), "Locking in 7 seconds. Move the mouse to stay.");
    }

    #[test]
    fn the_countdown_from_what_the_compositor_said() {
        let told = Instant::now();
        let pill = Pill {
            shown: shown(2_500),
            told,
            surface: window::Id::unique(),
        };
        assert_eq!(pill.seconds_left(told), 8);
        assert_eq!(pill.seconds_left(told + Duration::from_millis(500)), 7);
        assert_eq!(pill.seconds_left(told + Duration::from_secs(30)), 1);
        assert!((pill.progress(told) - 0.25).abs() < 1e-6);
    }
}
