//! When the session is idle and what the power settings do: `[idle]`, `[power]`.

use super::*;

/// When the session is idle, `[idle]` (docs/design/idle.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Idle {
    /// How long without input before the session is idle, on AC (the
    /// curtain then comes before the lock, 7.2); `None`, `"off"` in the
    /// file: never. On battery, `[power.battery] idle` says it.
    #[serde(deserialize_with = "idle_delay")]
    pub delay: Option<std::time::Duration>,
}

/// Five minutes on AC (7.2).
pub const IDLE_ON_AC: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// Three minutes on battery (7.2).
pub const IDLE_ON_BATTERY: std::time::Duration = std::time::Duration::from_secs(3 * 60);

impl Default for Idle {
    fn default() -> Self {
        Self {
            delay: Some(IDLE_ON_AC),
        }
    }
}

/// `[power]` (the Settings' Énergie page, 6.10, docs/design/settings.md):
/// the delays of each power source, locking, the lid, the power button and
/// the automatic power saver. Every value is read leniently: one that does
/// not read is logged and its default applies, the rest of the file kept;
/// a key not known refuses the file, as elsewhere. 6.1's keys still read
/// ([`Power::old`]): `[power.battery] screen-off` is the battery's `idle`
/// when that is absent, and `[power.*] lock-after` says `lock-on-idle` when
/// that is absent; neither is written again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "RawPower")]
pub struct Power {
    pub ac: PowerAc,
    pub battery: PowerBattery,
    /// The session locks as the dim ends; off, the screens go off after
    /// it, unlocked.
    pub lock_on_idle: bool,
    /// The session locks before the machine sleeps.
    pub lock_before_sleep: bool,
    pub lid: Lid,
    /// Docked (on AC, another screen on), the lid only turns the laptop's
    /// panel off.
    pub lid_ignored_when_docked: bool,
    pub power_button: PowerButton,
    pub auto_saver: bool,
    /// The battery level, in percent, at or under which the automatic
    /// power saver comes on: from 5 to 50.
    pub auto_saver_below: u8,
    /// 6.1's keys the file has.
    pub old: OldKeys,
}

/// `[power.ac]`. On AC the dim's delay is `[idle] delay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerAc {
    /// The screens off this long after the lock (or after the dim, with
    /// locking off).
    pub screen_off_after: Delay,
    /// The machine asleep after this long without activity.
    pub suspend_after: Delay,
}

/// `[power.battery]`: the same on battery, and the dim's delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerBattery {
    /// How long without input before the session is idle (the dim).
    pub idle: Delay,
    pub screen_off_after: Delay,
    pub suspend_after: Delay,
}

/// 6.1's keys found in the file, and what they were read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OldKeys {
    /// `[power.battery] screen-off`, read as `idle` unless that is given.
    pub screen_off: bool,
    /// `[power.*] lock-after`, read as `lock-on-idle` unless that is given.
    pub lock_after: bool,
    /// The new keys were there too: the old ones were ignored.
    pub ignored: bool,
}

/// What closing the lid does (`[power] lid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lid {
    Suspend,
    Lock,
    ScreenOff,
    Nothing,
}

/// What a short press of the power button does (`[power] power-button`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerButton {
    /// The session menu opens (7.4).
    Ask,
    Suspend,
    PowerOff,
    Nothing,
}

pub(super) const fn after(seconds: u64) -> Delay {
    Delay::After(std::time::Duration::from_secs(seconds))
}

/// 7.2's and 6.1's defaults: the dim at 5 min on AC (`[idle] delay`), 3 on
/// battery; the screens off 30 s after the lock on AC, 10 s on battery;
/// asleep never on AC, after 15 min on battery.
impl Default for Power {
    fn default() -> Self {
        Self {
            ac: PowerAc {
                screen_off_after: after(30),
                suspend_after: Delay::Never,
            },
            battery: PowerBattery {
                idle: Delay::After(IDLE_ON_BATTERY),
                screen_off_after: after(10),
                suspend_after: after(15 * 60),
            },
            lock_on_idle: true,
            lock_before_sleep: true,
            lid: Lid::Suspend,
            lid_ignored_when_docked: true,
            power_button: PowerButton::Ask,
            auto_saver: true,
            auto_saver_below: 20,
            old: OldKeys::default(),
        }
    }
}

/// `[power]` as the file has it, each value `None` when absent or when it
/// does not read.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawPower {
    ac: RawSource,
    battery: RawBattery,
    #[serde(deserialize_with = "lenient")]
    lock_on_idle: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    lock_before_sleep: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    lid: Option<Lid>,
    #[serde(deserialize_with = "lenient")]
    lid_ignored_when_docked: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    power_button: Option<PowerButton>,
    #[serde(deserialize_with = "lenient")]
    auto_saver: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    auto_saver_below: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawSource {
    #[serde(deserialize_with = "power_delay")]
    screen_off_after: Option<Delay>,
    #[serde(deserialize_with = "power_delay")]
    suspend_after: Option<Delay>,
    /// 6.1's.
    #[serde(deserialize_with = "power_delay")]
    lock_after: Option<Delay>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RawBattery {
    #[serde(deserialize_with = "power_delay")]
    idle: Option<Delay>,
    #[serde(deserialize_with = "power_delay")]
    screen_off_after: Option<Delay>,
    #[serde(deserialize_with = "power_delay")]
    suspend_after: Option<Delay>,
    /// 6.1's name for `idle`.
    #[serde(deserialize_with = "power_delay")]
    screen_off: Option<Delay>,
    /// 6.1's.
    #[serde(deserialize_with = "power_delay")]
    lock_after: Option<Delay>,
}

impl From<RawPower> for Power {
    fn from(raw: RawPower) -> Self {
        let default = Self::default();
        // 6.1's `lock-after`: `"never"` on every source it names turns
        // locking off; anything else keeps it on (its delays after the
        // screens went off are gone: the session locks as the dim ends).
        let lock_after = [raw.ac.lock_after, raw.battery.lock_after];
        let old_lock = lock_after
            .iter()
            .any(Option::is_some)
            .then(|| !lock_after.iter().flatten().all(|d| *d == Delay::Never));
        let old = OldKeys {
            screen_off: raw.battery.screen_off.is_some(),
            lock_after: old_lock.is_some(),
            ignored: (raw.battery.screen_off.is_some() && raw.battery.idle.is_some())
                || (old_lock.is_some() && raw.lock_on_idle.is_some()),
        };
        Self {
            ac: PowerAc {
                screen_off_after: raw
                    .ac
                    .screen_off_after
                    .unwrap_or(default.ac.screen_off_after),
                suspend_after: raw.ac.suspend_after.unwrap_or(default.ac.suspend_after),
            },
            battery: PowerBattery {
                idle: raw
                    .battery
                    .idle
                    .or(raw.battery.screen_off)
                    .unwrap_or(default.battery.idle),
                screen_off_after: raw
                    .battery
                    .screen_off_after
                    .unwrap_or(default.battery.screen_off_after),
                suspend_after: raw
                    .battery
                    .suspend_after
                    .unwrap_or(default.battery.suspend_after),
            },
            lock_on_idle: raw
                .lock_on_idle
                .or(old_lock)
                .unwrap_or(default.lock_on_idle),
            lock_before_sleep: raw.lock_before_sleep.unwrap_or(default.lock_before_sleep),
            lid: raw.lid.unwrap_or(default.lid),
            lid_ignored_when_docked: raw
                .lid_ignored_when_docked
                .unwrap_or(default.lid_ignored_when_docked),
            power_button: raw.power_button.unwrap_or(default.power_button),
            auto_saver: raw.auto_saver.unwrap_or(default.auto_saver),
            auto_saver_below: raw
                .auto_saver_below
                .filter(|p| (5..=50).contains(p))
                .unwrap_or(default.auto_saver_below),
            old,
        }
    }
}

/// One of `[power]`'s delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delay {
    Never,
    After(std::time::Duration),
}

impl Delay {
    /// `"5m"`, `"90s"`, `"1h"`, `"never"`, as the Settings write them.
    pub fn parse(text: &str) -> Option<Self> {
        if text == "never" {
            return Some(Self::Never);
        }
        let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
        let n: u64 = number.parse().ok()?;
        let by = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3600,
            _ => return None,
        };
        n.checked_mul(by)
            .filter(|s| *s <= IDLE_MOST)
            .map(|s| Self::After(std::time::Duration::from_secs(s)))
    }

    /// The delay; `None`: never.
    pub fn time(self) -> Option<std::time::Duration> {
        match self {
            Self::Never => None,
            Self::After(d) => Some(d),
        }
    }
}

pub(super) fn power_delay<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Delay>, D::Error> {
    let value = toml::Value::deserialize(deserializer)?;
    let read = value.as_str().and_then(Delay::parse);
    if read.is_none() {
        tracing::warn!(%value, "a [power] delay that does not read: its default applies");
    }
    Ok(read)
}

/// A `[power]` value, or `None` (its default) when it does not read.
pub(super) fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = toml::Value::deserialize(deserializer)?;
    match T::deserialize(value.clone()) {
        Ok(read) => Ok(Some(read)),
        Err(err) => {
            tracing::warn!(%value, %err, "a [power] value that does not read: its default applies");
            Ok(None)
        }
    }
}

impl Power {
    /// 6.1's keys the file still has, and what they are read as: for the
    /// log, as the file is read.
    pub fn old_keys(&self) -> Vec<&'static str> {
        let mut old = Vec::new();
        if self.old.screen_off {
            old.push("[power.battery] screen-off, read as idle unless idle is set");
        }
        if self.old.lock_after {
            old.push("[power.*] lock-after, read as lock-on-idle unless lock-on-idle is set");
        }
        old
    }
}

impl Config {
    /// How long without input before the session is idle (the dim), on
    /// battery or on AC; `None`: never.
    pub fn idle_delay(&self, battery: bool) -> Option<std::time::Duration> {
        if !battery {
            return self.idle.delay;
        }
        self.power.battery.idle.time().filter(|d| !d.is_zero())
    }

    /// How long after the lock (or after the dim, with locking off) the
    /// screens go off; `None`: never.
    pub fn screen_off_after(&self, battery: bool) -> Option<std::time::Duration> {
        if battery {
            self.power.battery.screen_off_after.time()
        } else {
            self.power.ac.screen_off_after.time()
        }
    }

    /// How long without activity before the machine sleeps; `None`:
    /// never.
    pub fn suspend_after(&self, battery: bool) -> Option<std::time::Duration> {
        let delay = if battery {
            self.power.battery.suspend_after
        } else {
            self.power.ac.suspend_after
        };
        delay.time().filter(|d| !d.is_zero())
    }
}

/// The longest delay taken: a day, far beyond any use, so that a deadline
/// never overflows the clock.
pub(super) const IDLE_MOST: u64 = 24 * 60 * 60;

pub(super) fn idle_delay<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<std::time::Duration>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Delay {
        Seconds(u64),
        Word(String),
    }
    match Delay::deserialize(deserializer)? {
        Delay::Word(word) if word == "off" => Ok(None),
        Delay::Seconds(seconds) if (1..=IDLE_MOST).contains(&seconds) => {
            Ok(Some(std::time::Duration::from_secs(seconds)))
        }
        Delay::Seconds(seconds) => Err(serde::de::Error::custom(format!(
            "an idle delay of {seconds} s: from 1 to {IDLE_MOST} seconds, or \"off\""
        ))),
        Delay::Word(word) => Err(serde::de::Error::custom(format!(
            "an idle delay of {word:?}: a number of seconds, or \"off\""
        ))),
    }
}
