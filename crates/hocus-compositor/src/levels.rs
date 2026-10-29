//! The keyboard's level keys (volume, mute, brightness; bar.md §4 group
//! 10): the compositor takes them as shortcuts and hands them to the shell,
//! which keeps the sound and the backlight. Their only feedback is the
//! bar's gauge.
//!
//! The up and down keys step on while held, at the keyboard's repeat delay
//! and rate, as Alt+Tab's Tab does: devices send no repeats of their own.
//! While the shell is away (restarting), presses are kept, a few and for a
//! few seconds, and handed over when it is back: a key pressed during a
//! restart still works, and one pressed long ago does not surprise.

use std::{collections::VecDeque, time::Duration};

use hocus_link::{LevelKey, ToShell};
use smithay::{
    input::keyboard::Keycode,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
};

use crate::State;

/// Presses kept while the shell is away: no more, and none older.
const KEPT: usize = 16;
const KEPT_FOR: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Levels {
    /// The key held down, stepping on: its keycode and its timer.
    held: Option<(Keycode, RegistrationToken)>,
    /// Presses while the shell was away, with when (`State::now`).
    kept: VecDeque<(LevelKey, Duration)>,
}

impl Levels {
    /// The presses still fresh at `now`, oldest first, and none kept.
    fn take_fresh(&mut self, now: Duration) -> Vec<LevelKey> {
        self.kept
            .drain(..)
            .filter(|(_, at)| now.saturating_sub(*at) <= KEPT_FOR)
            .map(|(key, _)| key)
            .collect()
    }

    fn keep(&mut self, key: LevelKey, now: Duration) {
        if self.kept.len() == KEPT {
            self.kept.pop_front();
        }
        self.kept.push_back((key, now));
    }
}

impl State {
    /// A level key, pressed or repeating: to the shell, or kept for it.
    pub fn level_key(&mut self, key: LevelKey) {
        if self.shell_linked() {
            self.tell_shell(ToShell::Level(key));
        } else {
            let now = self.now();
            self.levels.keep(key, now);
        }
    }

    /// The shell is back: the presses it missed, if still fresh.
    pub fn hand_kept_levels(&mut self) {
        let now = self.now();
        for key in self.levels.take_fresh(now) {
            self.tell_shell(ToShell::Level(key));
        }
    }

    /// Level key `key`, pressed on `code`, steps on while held.
    pub fn repeat_level_key(&mut self, code: Keycode, key: LevelKey) {
        self.stop_level_repeat();
        let keyboard = &self.config.keyboard;
        let Ok(rate) = u32::try_from(keyboard.repeat_rate) else {
            return;
        };
        if rate == 0 || !key.repeats() {
            return;
        }
        let delay = Duration::from_millis(u64::try_from(keyboard.repeat_delay).unwrap_or(0));
        let every = Duration::from_secs(1) / rate;
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(delay), move |_, _, app| {
                    app.state.level_key(key);
                    TimeoutAction::ToDuration(every)
                });
        match inserted {
            Ok(token) => self.levels.held = Some((code, token)),
            Err(err) => tracing::warn!(%err.error, "could not repeat the level key"),
        }
    }

    /// Key `code` was let go: if it is the level key held, it stops.
    pub fn level_key_released(&mut self, code: Keycode) {
        if self.levels.held.is_some_and(|(held, _)| held == code) {
            self.stop_level_repeat();
        }
    }

    fn stop_level_repeat(&mut self) {
        if let Some((_, token)) = self.levels.held.take() {
            self.loop_handle.remove(token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_are_kept_a_few_and_for_a_few_seconds() {
        let mut levels = Levels::default();
        let s = Duration::from_secs;
        levels.keep(LevelKey::Mute, s(1));
        levels.keep(LevelKey::VolumeUp, s(4));
        // Mute is 6 s old: dropped; volume-up is 3 s old: handed over.
        assert_eq!(levels.take_fresh(s(7)), vec![LevelKey::VolumeUp]);
        assert_eq!(levels.take_fresh(s(7)), vec![]);
        for _ in 0..40 {
            levels.keep(LevelKey::VolumeDown, s(10));
        }
        assert_eq!(levels.take_fresh(s(10)).len(), KEPT);
    }
}
