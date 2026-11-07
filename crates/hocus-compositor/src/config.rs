//! The configuration file, `$XDG_CONFIG_HOME/hocuspanda/config.toml`. Only
//! what this slice uses is read so far: the keyboard, the screens' scales,
//! the shortcuts and the programs they start, the mice and touchpads. It is
//! read as the compositor starts, then again each time it changes on disk
//! ([`crate::reload`]): the parts that follow it live take the new values;
//! sections other slices will read are let through. `packaging/config.toml`
//! lists every setting with its default.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer};

use crate::shortcuts::Shortcuts;

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keyboard: Keyboard,
    /// Settings per screen, by connector name (`eDP-1`, `DP-2`).
    pub outputs: BTreeMap<String, OutputSettings>,
    /// Each set of screens' own arrangement over `[outputs.NAME]`
    /// (`[[screen-sets]]`, windows.md §12): the Settings write it.
    #[serde(rename = "screen-sets")]
    pub screen_sets: Vec<ScreenSet>,
    pub shortcuts: Shortcuts,
    pub programs: Programs,
    /// The last session's applications reopened at log in (windows.md
    /// §14).
    pub session: SessionSettings,
    pub appearance: Appearance,
    /// The desktop's background and the lock's (6.3).
    pub wallpaper: Wallpaper,
    /// The desktop's language and formats (6.2).
    #[serde(deserialize_with = "locale")]
    pub locale: hocus_link::locale::Locale,
    pub desktops: Desktops,
    /// How windows move, snap and switch (6.14).
    pub windows: Windows,
    /// Drawing on the screen (6.14, docs/design/ink.md).
    pub ink: Ink,
    /// The overview's ways in (docs/design/windows.md §12b).
    pub overview: Overview,
    /// The magnifier (docs/design/accessibility.md, #424).
    pub magnifier: Magnifier,
    /// Warmer colours in the evening (#436, docs/design/windows.md §14).
    #[serde(rename = "night-light")]
    pub night_light: NightLight,
    /// The screen reader (docs/design/screen-reader.md, #734).
    pub accessibility: Accessibility,
    pub panel: Panel,
    /// Where screenshots go, their format and the shutter (6.17).
    pub capture: Capture,
    pub recording: Recording,
    /// What the clipboard's history keeps (6.17).
    pub clipboard: Clipboard,
    /// How the bar calls the user for the agents: the halo, its reminder,
    /// the panda peeking out, the pandas in the rows (6.16).
    pub agents: Agents,
    pub idle: Idle,
    pub power: Power,
    /// A day's CPU and memory, recorded (#655, docs/performance.md).
    pub perf: Perf,
    /// Every mouse, trackball and trackpoint (6.12).
    pub pointer: Pointer,
    /// Every touchpad (6.12).
    pub touchpad: Touchpad,
    /// The system's keyboard (`/etc/default/keyboard`), used when
    /// `[keyboard] layouts` is empty. Not part of the file: the program
    /// reads it once at start.
    #[serde(skip)]
    pub system_keyboard: Option<SystemKeyboard>,
    /// What the environment says about cursors. Not part of the file: the
    /// program reads it once at start; empty, no theme is looked for.
    #[serde(skip)]
    pub cursor_env: crate::cursor::CursorEnv,
    /// Where the desktop keeps what it remembers between sessions (the
    /// bar's form on each screen): `$XDG_STATE_HOME/hocuspanda`. Not part
    /// of the file; `None` remembers nothing (tests).
    #[serde(skip)]
    pub state_dir: Option<PathBuf>,
    /// Where the agents' tools are (their settings, their programs) and
    /// the `hocus` program their hooks run. Not part of the file: the
    /// program reads them from its environment; `None` looks for no tool
    /// (tests, unless they give a home of their own).
    #[serde(skip)]
    pub agent_places: Option<hocus_agents::tools::Places>,
    /// A flat background instead of the default gradient, for tests that
    /// look through translucent pixels. Not part of the file.
    #[serde(skip)]
    pub plain_background: bool,
    /// The directories of the applications' desktop entries (the XDG data
    /// directories' `applications`), where a window's tells whether it is
    /// a terminal (the clipboard's paste, 5.6). Not part of the file: the
    /// program reads them from its environment; empty, none is (tests,
    /// unless they give their own).
    #[serde(skip)]
    pub applications: Vec<PathBuf>,
    /// The pictures' folder, where captures are saved in a folder of
    /// their own (5.8): `$XDG_PICTURES_DIR`, else xdg-user-dirs', else
    /// `~/Pictures`. Not part of the file; `None` saves none, they are
    /// only copied (tests, unless they give one).
    #[serde(skip)]
    pub pictures: Option<PathBuf>,
    /// The user reads French (the messages' locale: `LC_ALL`,
    /// `LC_MESSAGES`, `LANG`): the captures' folder and files are named in
    /// it. Not part of the file.
    #[serde(skip)]
    pub french: bool,
    /// The PipeWire socket screen sharing streams to (5.11), when not
    /// PipeWire's default (`PIPEWIRE_REMOTE`, else `pipewire-0` in the
    /// runtime directory): tests give one of their own. Not part of the
    /// file.
    #[serde(skip)]
    pub pipewire_remote: Option<PathBuf>,
    /// The videos' folder, where recordings are saved in a folder of their
    /// own (5.10): `$XDG_VIDEOS_DIR`, else xdg-user-dirs', else `~/Videos`.
    /// Not part of the file; `None` records nothing (tests, unless they
    /// give one).
    #[serde(skip)]
    pub videos: Option<PathBuf>,
    /// The program that encodes recordings; empty: `ffmpeg`, on the
    /// `PATH`. Not part of the file.
    #[serde(skip)]
    pub ffmpeg: PathBuf,
    /// The home `~/` stands for in the file's paths (a wallpaper's): `HOME`.
    /// Not part of the file; `None`: a `~/` path is not found (tests,
    /// unless they give one).
    #[serde(skip)]
    pub home: Option<PathBuf>,
    /// The file this was read from, which the shell reads its own sections
    /// of (`[day]`, docs/design/day.md). Not part of the file; `None`: the
    /// shell reads none (tests, unless they give one).
    #[serde(skip)]
    pub file: Option<PathBuf>,
    /// Who checks the lock's password (7.1): PAM. Not part of the file;
    /// a test's own only in the crate's tests (`test-auth`).
    #[serde(skip)]
    pub lock_auth: crate::lock::Authenticator,
    /// Where the programs that play the shutter's sound (6.17), `wpctl`
    /// and `pw-play`, are found. Not part of the file: the session's on
    /// the `PATH`; none in tests (and headless), unless they give their
    /// own, so that no test is heard.
    #[serde(skip)]
    pub sound_programs: SoundPrograms,
}

/// Where the shutter's programs are found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SoundPrograms {
    /// Nowhere: no shutter is heard.
    #[default]
    None,
    /// On the `PATH`.
    Path,
    /// In this folder (tests' fakes).
    In(PathBuf),
}

impl Config {
    /// A folder the file names (`[capture] folder`, `[recording]
    /// folder`): absolute, or `~/` and the rest in the home; none when it
    /// is empty (the default folder) or cannot be read so.
    pub fn chosen_folder(&self, said: &str) -> Option<PathBuf> {
        let said = said.trim();
        if said.is_empty() {
            return None;
        }
        let path = match said.strip_prefix("~/") {
            Some(rest) => self.home.as_ref()?.join(rest),
            None if said == "~" => self.home.clone()?,
            None => PathBuf::from(said),
        };
        path.is_absolute().then_some(path)
    }
}

/// The screen capture, `[capture]` (5.8, 6.17): where its images go, in
/// which format, and whether the shutter is heard.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capture {
    /// Empty: the pictures' folder's "Captures d'écran" ("Screenshots").
    /// Absolute, or from the home (`~/`). A folder that cannot be written
    /// (gone, read-only) is passed over for the default one meanwhile.
    pub folder: String,
    /// The file's: the clipboard always gets PNG.
    pub format: CaptureFormat,
    /// A discreet click as the picture is taken, never when the sound is
    /// muted.
    pub sound: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: CaptureFormat::Png,
            sound: true,
        }
    }
}

/// A capture's file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureFormat {
    #[default]
    Png,
    /// At quality 90: lighter, for photos.
    Jpeg,
}

impl CaptureFormat {
    /// Its files' extension.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// The day's performance, `[perf]` (#655, docs/performance.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Perf {
    /// The compositor's and the shell's CPU and memory sampled every
    /// 10 s on a thread of their own, kept 14 days in the state folder.
    pub record: bool,
    /// How often, when not every 10 s (tests). Not part of the file.
    #[serde(skip)]
    pub interval: Option<std::time::Duration>,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            record: true,
            interval: None,
        }
    }
}

/// The clipboard's history, `[clipboard]` (5.6, 6.17).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Clipboard {
    /// Off: nothing new is read or kept, the history shows its pinned
    /// items only, and the others are erased.
    pub history: bool,
    /// How long what is not pinned is kept, 200 items at most: `"session"`
    /// (never on disk), `"1d"`, `"7d"` or `"30d"`.
    #[serde(deserialize_with = "clip_keep")]
    pub keep: hocus_link::ClipKeep,
    /// What a password manager marks secret, and what is copied from one
    /// (KeePassXC, Bitwarden, 1Password, and [`Clipboard::ignore`]'s), is
    /// never kept.
    pub skip_secrets: bool,
    /// More applications, by their ids, whose copies are never kept.
    pub ignore: Vec<String>,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            history: true,
            keep: hocus_link::ClipKeep::Month,
            skip_secrets: true,
            ignore: Vec::new(),
        }
    }
}

impl Clipboard {
    /// What the shell's history is told of it.
    pub const fn rules(&self) -> hocus_link::ClipRules {
        hocus_link::ClipRules {
            history: self.history,
            keep: self.keep,
        }
    }
}

fn clip_keep<'de, D: Deserializer<'de>>(deserializer: D) -> Result<hocus_link::ClipKeep, D::Error> {
    hocus_link::ClipKeep::of(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

/// How the bar calls the user for the agents, `[agents]` (6.16): the
/// halo (bar.md §6), its reminder, the panda peeking out (§7 C) and the
/// pandas in the agents' rows (4.1). The meetings' halo is not theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agents {
    pub halo: HaloRule,
    /// One more wave, once, for an agent still waiting after this long;
    /// its panda peeks out again with it.
    pub remind: Remind,
    pub peek: PeekRule,
    /// Off: a dot of the state's colour in each agent's row instead of
    /// its panda.
    pub pandas: bool,
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            halo: HaloRule::All,
            remind: Remind::Five,
            peek: PeekRule::WaitingAndDone,
            pandas: true,
        }
    }
}

/// What of the agents lights the halo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HaloRule {
    /// An agent waiting, stuck, or finished and not seen.
    #[default]
    All,
    /// An agent waiting only.
    Waiting,
    /// No agent: the meetings' halo only.
    Off,
}

/// When an agent still waiting is called again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum Remind {
    #[serde(rename = "2m")]
    Two,
    #[default]
    #[serde(rename = "5m")]
    Five,
    #[serde(rename = "10m")]
    Ten,
    #[serde(rename = "never")]
    Never,
}

impl Remind {
    /// After how long of waiting; `None`: never.
    pub const fn after(self) -> Option<std::time::Duration> {
        match self {
            Self::Two => Some(std::time::Duration::from_secs(2 * 60)),
            Self::Five => Some(std::time::Duration::from_secs(5 * 60)),
            Self::Ten => Some(std::time::Duration::from_secs(10 * 60)),
            Self::Never => None,
        }
    }
}

/// For what an agent's panda peeks out of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeekRule {
    /// An agent starts waiting, or finishes.
    #[default]
    WaitingAndDone,
    /// An agent starts waiting.
    Waiting,
    Off,
}

impl PeekRule {
    /// Whether a panda peeks out for `kind`.
    pub const fn peeks(self, kind: hocus_link::PeekKind) -> bool {
        match self {
            Self::WaitingAndDone => true,
            Self::Waiting => matches!(kind, hocus_link::PeekKind::Waiting),
            Self::Off => false,
        }
    }
}

/// Screen recording, `[recording]` (5.10, 6.17).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recording {
    /// `"mp4"` (H.264 and AAC) or `"webm"` (VP9 and Opus).
    #[serde(deserialize_with = "record_format")]
    pub format: hocus_link::RecordFormat,
    /// Empty: the videos' folder's "Enregistrements d'écran" ("Screen
    /// recordings"). As `[capture] folder`.
    pub folder: String,
}

fn record_format<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::RecordFormat, D::Error> {
    hocus_link::RecordFormat::of(&String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

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

const fn after(seconds: u64) -> Delay {
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
struct RawPower {
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
struct RawSource {
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
struct RawBattery {
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

fn power_delay<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Delay>, D::Error> {
    let value = toml::Value::deserialize(deserializer)?;
    let read = value.as_str().and_then(Delay::parse);
    if read.is_none() {
        tracing::warn!(%value, "a [power] delay that does not read: its default applies");
    }
    Ok(read)
}

/// A `[power]` value, or `None` (its default) when it does not read.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
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
const IDLE_MOST: u64 = 24 * 60 * 60;

fn idle_delay<'de, D: Deserializer<'de>>(
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

/// `[pointer]`: every mouse, trackball and trackpoint (6.12, the
/// Settings' "Souris et pavé tactile"), given to each as it is plugged in
/// and again when the file changes. A key not known refuses the file, as
/// elsewhere; a value that does not read is logged and its default
/// applies, the rest of the file kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// From -1 (slow) to 1 (fast); libinput's.
    pub speed: f64,
    pub acceleration: Acceleration,
    pub natural_scroll: bool,
    /// The right button is the main one.
    pub left_handed: bool,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            speed: 0.0,
            acceleration: Acceleration::Adaptive,
            natural_scroll: false,
            left_handed: false,
        }
    }
}

/// `[touchpad]`: every touchpad (a device that taps), as `[pointer]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touchpad {
    pub speed: f64,
    /// On, where libinput's own default is off (the maintainer's choice).
    pub tap_to_click: bool,
    pub tap_and_drag: bool,
    /// `None`: the touchpad's own (libinput's default for it).
    pub right_click: Option<RightClick>,
    pub disable_while_typing: bool,
    pub natural_scroll: bool,
    pub scroll: ScrollWay,
    /// Off while an external mouse is plugged in.
    pub off_with_mouse: bool,
    /// Three fingers sideways change desktop (windows.md §12).
    pub swipe_desktops: bool,
    /// Four fingers up open the launcher, down close it (6.13); down with
    /// no launcher open show the overview (8.5 A).
    pub swipe_launcher: bool,
}

impl Default for Touchpad {
    fn default() -> Self {
        Self {
            speed: 0.0,
            tap_to_click: true,
            tap_and_drag: true,
            right_click: None,
            disable_while_typing: true,
            natural_scroll: false,
            scroll: ScrollWay::TwoFingers,
            off_with_mouse: false,
            swipe_desktops: true,
            swipe_launcher: true,
        }
    }
}

pub use hocus_link::input::{Acceleration, RightClick, ScrollWay};

/// A speed from -1 to 1, written `0.5` or `1`.
fn speed_of(value: &toml::Value) -> Option<f64> {
    let speed = value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))?;
    (-1.0..=1.0).contains(&speed).then_some(speed)
}

/// `[pointer]` or `[touchpad]` read key by key: `read` takes a key and its
/// value, and says `None` when the key is not known, `Some(false)` when
/// its value does not read (its default then applies).
fn device_table<'de, D: Deserializer<'de>>(
    deserializer: D,
    table: &str,
    mut read: impl FnMut(&str, &toml::Value) -> Option<bool>,
) -> Result<(), D::Error> {
    let keys = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    for (key, value) in &keys {
        match read(key, value) {
            None => {
                return Err(serde::de::Error::custom(format!(
                    "unknown key `{key}` in [{table}]"
                )));
            }
            Some(false) => tracing::warn!(
                key,
                %value,
                "[{table}]: a value that does not read; its default applies"
            ),
            Some(true) => {}
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for Pointer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        device_table(deserializer, "pointer", |key, value| {
            Some(match key {
                "speed" => speed_of(value).map(|v| out.speed = v).is_some(),
                "acceleration" => value
                    .as_str()
                    .and_then(Acceleration::of)
                    .map(|v| out.acceleration = v)
                    .is_some(),
                "natural-scroll" => value.as_bool().map(|v| out.natural_scroll = v).is_some(),
                "left-handed" => value.as_bool().map(|v| out.left_handed = v).is_some(),
                _ => return None,
            })
        })?;
        Ok(out)
    }
}

impl<'de> Deserialize<'de> for Touchpad {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut out = Self::default();
        device_table(deserializer, "touchpad", |key, value| {
            let flag = |value: &toml::Value, to: &mut bool| value.as_bool().map(|v| *to = v);
            Some(match key {
                "speed" => speed_of(value).map(|v| out.speed = v).is_some(),
                "tap-to-click" => flag(value, &mut out.tap_to_click).is_some(),
                "tap-and-drag" => flag(value, &mut out.tap_and_drag).is_some(),
                "right-click" => value
                    .as_str()
                    .and_then(RightClick::of)
                    .map(|v| out.right_click = Some(v))
                    .is_some(),
                "disable-while-typing" => flag(value, &mut out.disable_while_typing).is_some(),
                "natural-scroll" => flag(value, &mut out.natural_scroll).is_some(),
                "scroll" => value
                    .as_str()
                    .and_then(ScrollWay::of)
                    .map(|v| out.scroll = v)
                    .is_some(),
                "off-with-mouse" => flag(value, &mut out.off_with_mouse).is_some(),
                "swipe-desktops" => flag(value, &mut out.swipe_desktops).is_some(),
                "swipe-launcher" => flag(value, &mut out.swipe_launcher).is_some(),
                _ => return None,
            })
        })?;
        Ok(out)
    }
}

/// The bar, `[panel]` (docs/design/bar.md §2, §3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Panel {
    /// The screens with a bar.
    pub screens: PanelScreens,
    /// The side of every screen the bar stands on (6.4); its edge zone,
    /// the windows' room and what opens from it go with it.
    pub side: PanelSide,
    /// The primary screen, by connector name. Empty: the built-in panel
    /// (eDP, LVDS, DSI) when it is connected, else the first screen.
    pub output: String,
    /// The bar's form on every screen; `[outputs.NAME] panel-form` sets one
    /// screen's.
    pub form: FormChoice,
    pub clock: Clock,
    /// The windows of the screen's current desktop over its desktops'
    /// tiles (#604); `false` hides them in both forms.
    pub windows: bool,
    /// The next meeting's card and mark (6.4); `false` hides them in both
    /// forms, the day still read.
    pub meeting: bool,
    /// Today's events after the meeting (6.4).
    pub today: bool,
    /// The to-do list and its mini item (6.4).
    pub tasks: bool,
    /// The agents' group and its mini item (6.4).
    pub agents: bool,
    /// The order of Today, the to-do list and the agents under the
    /// meeting's card and the sign of what is in use (6.4): an unknown
    /// name is skipped, a missing one comes after those named.
    #[serde(deserialize_with = "bar_order")]
    pub order: [hocus_link::BarGroup; 3],
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            screens: PanelScreens::default(),
            side: PanelSide::default(),
            output: String::new(),
            form: FormChoice::default(),
            clock: Clock::default(),
            windows: true,
            meeting: true,
            today: true,
            tasks: true,
            agents: true,
            order: hocus_link::BarGroup::DEFAULT,
        }
    }
}

impl Panel {
    /// What the bar's column shows, as the shell is told it.
    pub fn groups(&self) -> hocus_link::BarGroups {
        hocus_link::BarGroups {
            order: self.order,
            meeting: self.meeting,
            today: self.today,
            tasks: self.tasks,
            agents: self.agents,
        }
    }
}

fn bar_order<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[hocus_link::BarGroup; 3], D::Error> {
    Ok(hocus_link::BarGroup::order_of(&Vec::<String>::deserialize(
        deserializer,
    )?))
}

/// The bar's clock, `[panel.clock]` (bar.md §4 group 1, §5): sent to the
/// shell as it connects.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clock {
    /// The family of the hours and minutes: the bundled Martian Mono, or
    /// any installed family by name (the shell falls back to Martian Mono
    /// when it is not installed).
    #[serde(deserialize_with = "font_family")]
    pub font: String,
    /// `#RRGGBB` or `#RGB`.
    #[serde(deserialize_with = "color")]
    pub color: [u8; 3],
    /// On a minute change.
    #[serde(deserialize_with = "transition")]
    pub transition: hocus_link::Transition,
    /// The seconds beside the minutes, in the full form.
    pub seconds: bool,
}

impl Default for Clock {
    fn default() -> Self {
        let look = hocus_link::ClockLook::default();
        Self {
            font: look.font,
            color: look.color,
            transition: look.transition,
            seconds: look.seconds,
        }
    }
}

impl Clock {
    pub fn look(&self) -> hocus_link::ClockLook {
        hocus_link::ClockLook {
            font: self.font.clone(),
            color: self.color,
            transition: self.transition,
            seconds: self.seconds,
        }
    }
}

fn font_family<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let family = String::deserialize(deserializer)?;
    // One line to the shell: no line breaks; spaces are kept.
    let family = family.split_whitespace().collect::<Vec<_>>().join(" ");
    if family.is_empty() {
        Ok(hocus_link::ClockLook::DEFAULT_FONT.to_owned())
    } else {
        Ok(family)
    }
}

fn color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 3], D::Error> {
    hocus_link::parse_color(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

fn transition<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::Transition, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

/// Which screens have a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelScreens {
    /// The primary screen only.
    #[default]
    Primary,
    All,
}

/// The side of the screens the bar stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelSide {
    #[default]
    Right,
    Left,
}

impl PanelSide {
    /// As the shell is told it.
    pub const fn link(self) -> hocus_link::Side {
        match self {
            Self::Right => hocus_link::Side::Right,
            Self::Left => hocus_link::Side::Left,
        }
    }
}

/// The bar's form on a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormChoice {
    /// Full on a screen wide enough (bar.md §3), or as last chosen with a
    /// click or `Super+Shift+B`.
    #[default]
    Auto,
    Full,
    Mini,
}

impl FormChoice {
    /// As the file writes it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Mini => "mini",
        }
    }
}

/// How the desktop looks and moves, `[appearance]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Appearance {
    /// Windows fade instead of moving, and do not wobble.
    pub reduced_motion: bool,
    /// The theme reinforced (#423): the shell, the Settings, the lock and
    /// the authentication dialog draw its high-contrast tokens, with no
    /// glass (it implies `reduce-transparency`); the focused window's rim
    /// is 2 px wide instead of 1; applications are told (`contrast`).
    pub high_contrast: bool,
    /// The Xcursor theme; else `XCURSOR_THEME`'s, else `default`.
    pub cursor_theme: Option<String>,
    /// The cursor's size in logical pixels; else `XCURSOR_SIZE`'s, else 24.
    #[serde(deserialize_with = "cursor_size")]
    pub cursor_size: Option<u32>,
    /// The bar is opaque instead of glass over what is behind it.
    pub reduce_transparency: bool,
    /// `"dark"` or `"light"`: the shell, the Settings, the lock and the
    /// authentication dialog draw in it, and applications are told
    /// (#402).
    #[serde(deserialize_with = "scheme")]
    pub theme: hocus_link::theme::Scheme,
    /// `"green"`, `"blue"`, `"violet"`, `"rose"`, `"teal"` or
    /// `"wallpaper"`: what is chosen, on or focused.
    #[serde(deserialize_with = "accent")]
    pub accent: hocus_link::theme::Accent,
    /// The Settings' text, from 80 to 150 %: written as `1.2`, held in
    /// hundredths.
    #[serde(deserialize_with = "text_scale")]
    pub text_scale: TextScale,
}

/// The desktop's background, `[wallpaper]` (6.3, docs/design/settings.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Wallpaper {
    /// `"night"`, `"mist"`, `"dawn"`, `"forest"`, `"rain"`, `"stars"`,
    /// `"plain"`, or a picture's path: every screen's, but those that have their own.
    #[serde(deserialize_with = "picture")]
    pub picture: hocus_link::wallpaper::Picture,
    /// Each screen its own: `[outputs.NAME] wallpaper`, else `picture`.
    pub per_screen: bool,
    /// How a picture of one's own fills its screen.
    #[serde(deserialize_with = "fit")]
    pub fit: hocus_link::wallpaper::Fit,
    /// `"plain"`'s colour, and around a picture not covering: `#RRGGBB`.
    #[serde(deserialize_with = "colour")]
    pub color: [u8; 3],
    /// The lock screen: `"dimmed"`, `"blurred"` or `"picture"`.
    #[serde(deserialize_with = "lock_look")]
    pub lock: hocus_link::wallpaper::LockLook,
    /// The lock's own picture, with `lock = "picture"`.
    #[serde(deserialize_with = "picture")]
    pub lock_picture: hocus_link::wallpaper::Picture,
    /// A wallpaper that moves ("rain", "stars") moves; else its still
    /// picture (#433).
    pub animate: bool,
    /// On battery a moving wallpaper goes at half its frames, stays
    /// still, or goes as on mains power.
    pub on_battery: OnBattery,
}

/// `[wallpaper] on-battery` (#433).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnBattery {
    /// Half its frames, so half its pace.
    #[default]
    Half,
    /// Its still picture.
    Still,
    /// As on mains power.
    Full,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self {
            picture: hocus_link::wallpaper::Picture::default(),
            per_screen: false,
            fit: hocus_link::wallpaper::Fit::default(),
            color: hocus_link::wallpaper::DEFAULT_COLOR,
            lock: hocus_link::wallpaper::LockLook::default(),
            lock_picture: hocus_link::wallpaper::Picture::default(),
            animate: true,
            on_battery: OnBattery::Half,
        }
    }
}

fn picture<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::wallpaper::Picture, D::Error> {
    hocus_link::wallpaper::Picture::of(&String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

fn some_picture<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<hocus_link::wallpaper::Picture>, D::Error> {
    picture(deserializer).map(Some)
}

fn fit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<hocus_link::wallpaper::Fit, D::Error> {
    let word = String::deserialize(deserializer)?;
    hocus_link::wallpaper::Fit::of(&word).ok_or_else(|| {
        serde::de::Error::custom(format!("no fit {word:?}: cover, contain or center"))
    })
}

fn colour<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 3], D::Error> {
    let text = String::deserialize(deserializer)?;
    hocus_link::wallpaper::colour_of(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("no colour {text:?}: #RRGGBB")))
}

fn lock_look<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::wallpaper::LockLook, D::Error> {
    let word = String::deserialize(deserializer)?;
    hocus_link::wallpaper::LockLook::of(&word).ok_or_else(|| {
        serde::de::Error::custom(format!("no lock {word:?}: dimmed, blurred or picture"))
    })
}

/// `[appearance] text-scale`, in hundredths: 100 by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextScale(pub u32);

impl Default for TextScale {
    fn default() -> Self {
        Self(100)
    }
}

fn text_scale<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TextScale, D::Error> {
    let scale = f64::deserialize(deserializer)?;
    let hundredths = (scale * 100.0).round();
    if (80.0..=150.0).contains(&hundredths) {
        Ok(TextScale(hundredths as u32))
    } else {
        Err(serde::de::Error::custom(format!(
            "text scale {scale}: from 0.8 to 1.5"
        )))
    }
}

/// `[locale]`: its three keys, each a word.
fn locale<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::locale::Locale, D::Error> {
    use hocus_link::locale::{DateFormat, Language, Locale, TimeFormat};
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Words {
        language: Option<String>,
        date: Option<String>,
        time: Option<String>,
    }
    let words = Words::deserialize(deserializer)?;
    let bad = |key: &str, word: &str, choices: &str| {
        serde::de::Error::custom(format!("[locale] {key} = {word:?}: {choices}"))
    };
    Ok(Locale {
        language: match words.language.as_deref() {
            None => Language::System,
            Some(word) => {
                Language::of(word).ok_or_else(|| bad("language", word, "system, fr or en"))?
            }
        },
        date: match words.date.as_deref() {
            None => DateFormat::Long,
            Some(word) => DateFormat::of(word)
                .ok_or_else(|| bad("date", word, "long, short, numeric or iso"))?,
        },
        time: match words.time.as_deref() {
            None => None,
            Some(word) => {
                Some(TimeFormat::of(word).ok_or_else(|| bad("time", word, "24h or 12h"))?)
            }
        },
    })
}

impl Appearance {
    /// The theme the shell draws in, `wallpaper` the wallpaper's accent
    /// (only kept while the accent is the wallpaper's: another accent does
    /// not change with the wallpaper).
    pub fn theme_choice(
        &self,
        wallpaper: hocus_link::theme::AccentColours,
    ) -> hocus_link::theme::Choice {
        hocus_link::theme::Choice {
            scheme: self.theme,
            accent: self.accent,
            wallpaper: if self.accent == hocus_link::theme::Accent::Wallpaper {
                wallpaper
            } else {
                hocus_link::theme::AccentColours::default()
            },
            contrast: self.high_contrast,
        }
    }
}

fn scheme<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::theme::Scheme, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(|err: hocus_link::BadLine| serde::de::Error::custom(err.0))
}

fn accent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::theme::Accent, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(|err: hocus_link::BadLine| serde::de::Error::custom(err.0))
}

fn cursor_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    let size = u32::deserialize(deserializer)?;
    let sizes = crate::cursor::theme::SIZES;
    if sizes.contains(&size) {
        Ok(Some(size))
    } else {
        Err(serde::de::Error::custom(format!(
            "cursor size {size}: from {} to {}",
            sizes.start(),
            sizes.end()
        )))
    }
}

/// Virtual desktops, `[desktops]` (docs/design/windows.md §12).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Desktops {
    /// Desktops on each screen, from 1 to 9.
    #[serde(deserialize_with = "desktop_count")]
    pub count: usize,
    /// Their names, by position (6.14): one missing or empty is "Bureau
    /// N" (the shell's words).
    pub names: Vec<String>,
}

impl Default for Desktops {
    fn default() -> Self {
        Self {
            count: crate::desktops::DEFAULT_COUNT,
            names: Vec::new(),
        }
    }
}

impl Desktops {
    /// Desktop `index`'s name (from 0), if it has one.
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names
            .get(index)
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
    }

    /// The names the desktops there are have, the empty ones last left
    /// out: what the shell is told.
    pub fn shown_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .names
            .iter()
            .take(self.count)
            .map(|n| n.trim().to_owned())
            .collect();
        while names.last().is_some_and(String::is_empty) {
            names.pop();
        }
        names
    }
}

fn desktop_count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let count = usize::deserialize(deserializer)?;
    if (1..=crate::desktops::MAX_COUNT).contains(&count) {
        Ok(count)
    } else {
        Err(serde::de::Error::custom(format!(
            "{count} desktops: from 1 to 9"
        )))
    }
}

/// How windows move, snap and switch, `[windows]` (6.14;
/// docs/design/windows.md §6, §8, §9, §11).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Windows {
    /// The key held to move a window by a drag anywhere in it (`Alt`).
    pub move_modifier: DragKey,
    /// The key held to resize one by a right-button drag (`Super`).
    pub resize_modifier: DragKey,
    /// A moved window's edges stick to others' and the work area's.
    pub magnet: bool,
    /// Dragged against an edge of the work area, it snaps there.
    pub snap_by_drag: bool,
    /// A moved window wobbles (never with reduced motion).
    pub wobble: bool,
    /// How applications' windows open and close (#432).
    pub animation: Animation,
    /// Which windows Alt+Tab lists.
    pub alt_tab: AltTab,
    /// Rules per application, `[[windows.rules]]`, in the file's order:
    /// the first of an application's applies.
    pub rules: Vec<Rule>,
}

impl Windows {
    /// The rule of the application that goes by `app_id`, if any.
    pub fn rule(&self, app_id: &str) -> Option<&Rule> {
        if app_id.is_empty() {
            return None;
        }
        self.rules
            .iter()
            .find(|r| r.app_id.eq_ignore_ascii_case(app_id))
    }
}

/// How an application's windows open (6.14): on a desktop, at a size,
/// above the others. A dialog follows its parent, not the rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Rule {
    pub app_id: String,
    /// The desktop it opens on (from 1), on the screen it opens on; none:
    /// the one the screen shows.
    #[serde(default, deserialize_with = "rule_desktop")]
    pub desktop: Option<usize>,
    #[serde(default)]
    pub size: OpenSize,
    /// Kept above the others, even without the focus; a fullscreen
    /// window in front covers it.
    #[serde(default)]
    pub above: bool,
}

fn rule_desktop<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<usize>, D::Error> {
    let desktop = usize::deserialize(deserializer)?;
    if (1..=crate::desktops::MAX_COUNT).contains(&desktop) {
        Ok(Some(desktop))
    } else {
        Err(serde::de::Error::custom(format!(
            "desktop {desktop}: from 1 to 9"
        )))
    }
}

/// The size a window opens at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenSize {
    /// Its application's last size (§5).
    #[default]
    Last,
    Maximized,
    Left,
    Right,
    Fullscreen,
}

impl OpenSize {
    /// The mode it opens in, none for its last size.
    pub fn mode(self) -> Option<crate::model::Mode> {
        use crate::model::{Mode, Zone};
        match self {
            Self::Last => None,
            Self::Maximized => Some(Mode::Snapped(Zone::Maximized)),
            Self::Left => Some(Mode::Snapped(Zone::Left)),
            Self::Right => Some(Mode::Snapped(Zone::Right)),
            Self::Fullscreen => Some(Mode::Fullscreen),
        }
    }
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            move_modifier: DragKey::Alt,
            resize_modifier: DragKey::Super,
            magnet: true,
            snap_by_drag: true,
            wobble: true,
            animation: Animation::Standard,
            alt_tab: AltTab::All,
            rules: Vec::new(),
        }
    }
}

/// How applications' windows open and close, `[windows] animation`
/// (docs/design/windows.md §5): each theme with a look of its own brings
/// its own, and any can be chosen in any theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Animation {
    /// Growing from 94 % as they fade in; shrinking a little as they go.
    #[default]
    Standard,
    /// Columns of glyphs falling through the window (#432).
    Rain,
    /// Givre's own, designed later: the standard motion until then.
    Frost,
}

/// The key held for a drag anywhere in a window: nested, `Super` is
/// `Alt+Super`, as every shortcut there (windows.md §13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DragKey {
    Alt,
    Super,
    /// None: title bars, the grip and the frame only.
    None,
}

/// Which windows Alt+Tab lists (windows.md §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AltTab {
    /// This desktop's first, then the others' after a line, dimmed.
    All,
    /// This desktop's only.
    Desktop,
}

/// Drawing on the screen, `[ink]` (docs/design/ink.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ink {
    /// Off, `Super`+drag and the laser do nothing, and pass to the window.
    pub enabled: bool,
}

impl Default for Ink {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// The overview, `[overview]` (docs/design/windows.md §12b).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Overview {
    /// The pointer held in a screen's top-left corner opens it (#727).
    pub hot_corner: bool,
}

impl Default for Overview {
    fn default() -> Self {
        Self { hot_corner: true }
    }
}

/// `[accessibility]` (docs/design/screen-reader.md, #734).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Accessibility {
    /// The screen reader (Orca) starts with the session; `Super+Alt+S`
    /// starts or stops it whatever this says.
    pub screen_reader: bool,
}

/// The magnifier, `[magnifier]` (docs/design/accessibility.md, #424).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Magnifier {
    /// On: the screen under the pointer magnified, at `zoom`.
    pub enabled: bool,
    /// From 1 to 16: what `enabled` turns it on at.
    #[serde(deserialize_with = "zoom")]
    pub zoom: f64,
    /// How the view follows the pointer.
    pub follow_pointer: FollowPointer,
    /// The view jumps to what takes the keyboard, and to the text caret.
    pub follow_keyboard: bool,
    /// Smoothed (linear filtering); off, hard pixels.
    pub smooth: bool,
}

impl Default for Magnifier {
    fn default() -> Self {
        Self {
            enabled: false,
            zoom: 2.0,
            follow_pointer: FollowPointer::Edge,
            follow_keyboard: true,
            smooth: true,
        }
    }
}

/// The night light, `[night-light]` (#436): the screens' colours warmer
/// from `from` to `to`, wall-clock hours, fading in and out 30 min.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct NightLight {
    pub enabled: bool,
    /// The colour temperature at night, in kelvin, from 1500 to 6500
    /// (6500: the screen's own white).
    #[serde(deserialize_with = "kelvin")]
    pub temperature: u32,
    /// When it starts warming and when it goes back, in minutes since
    /// midnight (`"21:00"`, `"7:30"`). The same hour: all day.
    #[serde(deserialize_with = "clock_time")]
    pub from: u16,
    #[serde(deserialize_with = "clock_time")]
    pub to: u16,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            enabled: false,
            temperature: 4000,
            from: 21 * 60,
            to: 7 * 60,
        }
    }
}

fn kelvin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let kelvin = u32::deserialize(deserializer)?;
    if (1500..=6500).contains(&kelvin) {
        Ok(kelvin)
    } else {
        Err(serde::de::Error::custom(format!(
            "night light temperature {kelvin}: from 1500 to 6500"
        )))
    }
}

/// `"21:00"`, `"7:30"`: minutes since midnight.
fn clock_time<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_clock_time(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("{text:?}: a time of day, as \"21:00\"")))
}

pub(crate) fn parse_clock_time(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && text.trim().len() <= 5).then_some(h * 60 + m)
}

/// How the magnifier's view follows the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FollowPointer {
    /// It moves only when the pointer pushes its edge, by as much.
    #[default]
    Edge,
    /// The pointer stays in its middle.
    #[serde(alias = "center")]
    Centre,
    /// It sits where the pointer sits on the screen.
    Proportional,
}

fn zoom<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let zoom = f64::deserialize(deserializer)?;
    if (1.0..=16.0).contains(&zoom) {
        Ok(zoom)
    } else {
        Err(serde::de::Error::custom(format!(
            "magnifier zoom {zoom}: from 1 to 16"
        )))
    }
}

/// One screen's settings, `[outputs.eDP-1]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct OutputSettings {
    /// From 0.5 to 4. None: 1 or 2 by its density
    /// (`output_spec::default_scale`).
    #[serde(deserialize_with = "scale")]
    pub scale: Option<f64>,
    /// The bar's form on this screen, over `[panel] form` (`auto` too:
    /// the Settings write each screen's, 6.4). None: `[panel] form`.
    pub panel_form: Option<FormChoice>,
    /// `mode = "2560x1600@60"`: its size in pixels and its refresh rate
    /// (Hz, whole or not). None: the mode it prefers.
    #[serde(deserialize_with = "mode")]
    pub mode: Option<ModeWanted>,
    /// `position = [x, y]`: its top-left corner, logical pixels. Used when
    /// every screen on has one and they neither overlap nor leave one
    /// apart; otherwise the screens go left to right.
    #[serde(deserialize_with = "position")]
    pub position: Option<(i32, i32)>,
    /// `enabled = false` turns it off, plugged; its windows go to another.
    pub enabled: bool,
    /// Variable refresh rate (VRR, adaptive sync), when it offers it.
    pub vrr: bool,
    /// Its own wallpaper, with `[wallpaper] per-screen` (6.3); kept when
    /// per-screen is off, for when it is on again.
    #[serde(deserialize_with = "some_picture")]
    pub wallpaper: Option<hocus_link::wallpaper::Picture>,
}

impl OutputSettings {
    /// The same, without its wallpaper: what the screens' layout takes.
    pub fn screen_part(&self) -> Self {
        Self {
            wallpaper: None,
            ..self.clone()
        }
    }
}

/// A set of screens and its own arrangement, `[[screen-sets]]`: its
/// screens by identity (windows.md §12), which is primary, and each
/// screen's place, mode, scale and VRR over its `[outputs.NAME]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenSet {
    pub screens: Vec<String>,
    /// The primary screen, by identity, over `[panel] output`.
    pub primary: Option<String>,
    /// By identity.
    pub outputs: BTreeMap<String, SetScreen>,
}

impl ScreenSet {
    /// Whether it is the set of `keys`, in any order.
    pub fn is(&self, keys: &[String]) -> bool {
        let mut mine = self.screens.clone();
        let mut theirs = keys.to_vec();
        mine.sort();
        mine.dedup();
        theirs.sort();
        theirs.dedup();
        !mine.is_empty() && mine == theirs
    }
}

/// A screen's arrangement in a set: what it says wins over
/// `[outputs.NAME]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetScreen {
    #[serde(deserialize_with = "scale")]
    pub scale: Option<f64>,
    #[serde(deserialize_with = "mode")]
    pub mode: Option<ModeWanted>,
    #[serde(deserialize_with = "position")]
    pub position: Option<(i32, i32)>,
    pub vrr: Option<bool>,
}

impl SetScreen {
    /// `base` with what this says over it.
    pub fn over(&self, base: OutputSettings) -> OutputSettings {
        OutputSettings {
            scale: self.scale.or(base.scale),
            mode: self.mode.or(base.mode),
            position: self.position.or(base.position),
            vrr: self.vrr.unwrap_or(base.vrr),
            ..base
        }
    }
}

impl Config {
    /// The file's arrangement of the set of `keys`.
    pub fn screen_set(&self, keys: &[String]) -> Option<&ScreenSet> {
        self.screen_sets.iter().find(|s| s.is(keys))
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            scale: None,
            panel_form: None,
            mode: None,
            position: None,
            enabled: true,
            vrr: false,
            wallpaper: None,
        }
    }
}

/// A mode asked for in the file: `2560x1600@60`, `1920x1080@59.94`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeWanted {
    pub width: i32,
    pub height: i32,
    /// In Hz.
    pub refresh: f64,
}

impl ModeWanted {
    /// `2560x1600@60`; the refresh rate may be left out (`2560x1600`: its
    /// highest).
    pub fn parse(text: &str) -> Option<Self> {
        let (size, refresh) = match text.trim().split_once('@') {
            Some((size, refresh)) => (size, Some(refresh)),
            None => (text.trim(), None),
        };
        let (w, h) = size.split_once('x')?;
        let width: i32 = w.parse().ok()?;
        let height: i32 = h.parse().ok()?;
        let refresh = match refresh {
            Some(r) => r
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite() && *r > 0.0)?,
            None => 0.0,
        };
        (width > 0 && height > 0).then_some(Self {
            width,
            height,
            refresh,
        })
    }

    /// Of `modes` (width, height, refresh in mHz), the one asked: that
    /// size, the refresh rate nearest the one asked within half a hertz,
    /// or its highest when none was asked.
    pub fn pick(&self, modes: &[(i32, i32, i32)]) -> Option<(i32, i32, i32)> {
        let sized = modes
            .iter()
            .copied()
            .filter(|(w, h, _)| (*w, *h) == (self.width, self.height));
        if self.refresh == 0.0 {
            return sized.max_by_key(|m| m.2);
        }
        let asked = self.refresh * 1000.0;
        sized
            .map(|m| (m, (f64::from(m.2) - asked).abs()))
            .filter(|(_, off)| *off <= 500.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m)
    }
}

impl std::fmt::Display for ModeWanted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}@{}", self.width, self.height, self.refresh)
    }
}

fn mode<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<ModeWanted>, D::Error> {
    let text = String::deserialize(deserializer)?;
    ModeWanted::parse(&text).map(Some).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "mode {text:?}: WIDTHxHEIGHT@HZ, as \"2560x1600@60\""
        ))
    })
}

fn position<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<(i32, i32)>, D::Error> {
    let [x, y] = <[i32; 2]>::deserialize(deserializer)?;
    Ok(Some((x, y)))
}

fn scale<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let scale = f64::deserialize(deserializer)?;
    if (0.5..=4.0).contains(&scale) {
        Ok(Some(scale))
    } else {
        Err(serde::de::Error::custom(format!(
            "scale {scale}: from 0.5 to 4"
        )))
    }
}

/// Where a screen's scale came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFrom {
    /// `[outputs.NAME] scale`.
    Config,
    /// 1 or 2 by its density, `output_spec::default_scale`; the density in
    /// whole dpi, rounded down, when its size is believed.
    Automatic { dpi: Option<u32> },
}

impl ScaleFrom {
    /// In words, after the scale in the journal, for a user looking for the
    /// name to put in the configuration.
    pub fn describe(self, name: &str) -> String {
        match self {
            Self::Config => format!("(from [outputs.{name}] in config.toml)"),
            Self::Automatic { dpi } => {
                let density = dpi.map_or_else(|| "size unknown".to_owned(), |d| format!("{d} dpi"));
                format!(
                    "(automatic: {density}) — [outputs.{name}] scale = ... in config.toml sets it"
                )
            }
        }
    }
}

impl Config {
    /// The scale of the screen on connector `name`: the configured one, or
    /// 1 or 2 by its density.
    pub fn output_scale(
        &self,
        name: &str,
        pixels: (i32, i32),
        millimetres: (i32, i32),
    ) -> (f64, ScaleFrom) {
        output_scale(self.outputs.get(name), pixels, millimetres)
    }
}

/// The scale of a screen with `settings`: theirs, or
/// 1 or 2 by its density.
pub fn output_scale(
    settings: Option<&OutputSettings>,
    pixels: (i32, i32),
    millimetres: (i32, i32),
) -> (f64, ScaleFrom) {
    match settings.and_then(|o| o.scale) {
        Some(scale) => (scale, ScaleFrom::Config),
        None => (
            crate::output_spec::default_scale(pixels, millimetres),
            ScaleFrom::Automatic {
                dpi: crate::output_spec::density(pixels, millimetres)
                    // Down: a screen shown at 191 dpi or less never takes 2 by itself.
                    .map(|dpi| dpi.floor() as u32),
            },
        ),
    }
}

/// What XKB is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XkbNames {
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
    /// Where the layouts came from: `config.toml`, the system, or XKB's
    /// defaults (all empty).
    pub from: &'static str,
}

impl Config {
    /// The keyboard: `[keyboard]`'s layouts when it names some, else the
    /// system's; `[keyboard] options` win over the system's when set.
    pub fn xkb_names(&self) -> XkbNames {
        let keyboard = &self.keyboard;
        if !keyboard.layouts.is_empty() {
            let (layout, variant) = xkb_lists(&keyboard.layouts);
            return XkbNames {
                model: String::new(),
                layout,
                variant,
                options: keyboard.options.clone(),
                from: "config.toml",
            };
        }
        let system = self.system_keyboard.clone().unwrap_or_default();
        XkbNames {
            from: if system.layout.is_empty() {
                "XKB's defaults"
            } else {
                SystemKeyboard::PATH
            },
            model: system.model,
            layout: system.layout,
            variant: system.variant,
            options: if keyboard.options.is_empty() {
                system.options
            } else {
                keyboard.options.clone()
            },
        }
    }
}

/// The system's keyboard, as Debian's `/etc/default/keyboard` gives it:
/// XKB names, the layouts, variants and options comma-separated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemKeyboard {
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
}

impl SystemKeyboard {
    pub const PATH: &str = "/etc/default/keyboard";

    /// `/etc/default/keyboard`, read once as the program starts; `None`
    /// when it is missing, does not read, or names no layout (XKB's
    /// defaults then).
    pub fn read(path: &Path) -> Option<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let keyboard = Self::parse(&text);
                if keyboard.is_none() {
                    tracing::info!(file = %path.display(), "no XKBLAYOUT; XKB's defaults");
                }
                keyboard
            }
            Err(err) => {
                tracing::info!(file = %path.display(), %err, "system keyboard not read; XKB's defaults");
                None
            }
        }
    }

    /// `KEY="value"` lines (or `KEY=value`, or `'value'`); comments and
    /// other keys are passed over. `None` without a layout.
    pub fn parse(text: &str) -> Option<Self> {
        let mut keyboard = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value)
                .to_owned();
            match key.trim() {
                "XKBMODEL" => keyboard.model = value,
                "XKBLAYOUT" => keyboard.layout = value,
                "XKBVARIANT" => keyboard.variant = value,
                "XKBOPTIONS" => keyboard.options = value,
                _ => {}
            }
        }
        (!keyboard.layout.is_empty()).then_some(keyboard)
    }
}

/// The session, `[session]` (windows.md §14, 6.15).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionSettings {
    /// At log in, the applications open at the last log out start again,
    /// each window back in its place.
    pub restore: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self { restore: true }
    }
}

/// The programs the desktop starts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Programs {
    /// The terminal's command, run through `sh -c`. Empty: the first of
    /// [`crate::programs::TERMINALS`] found in `PATH`.
    pub terminal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    /// XKB layouts, each with its variant in brackets, the first active at
    /// start: `["fr", "fr(bepo)", "us"]`. Empty: the system's
    /// (`/etc/default/keyboard`), or else XKB's defaults (and the
    /// `XKB_DEFAULT_*` variables).
    pub layouts: Vec<String>,
    /// XKB options, e.g. `"grp:alt_shift_toggle,caps:escape"`.
    pub options: String,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: i32,
    /// Repeats per second.
    pub repeat_rate: i32,
    /// Num Lock on as the session opens, on every keyboard.
    pub numlock: bool,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layouts: Vec::new(),
            options: String::new(),
            repeat_delay: 200,
            repeat_rate: 25,
            numlock: false,
        }
    }
}

impl Config {
    /// The file's configuration; the defaults when there is none, or when
    /// it does not read (said in the log, with where).
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                tracing::warn!(file = %path.display(), %err, "configuration not read; defaults");
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!(file = %path.display(), "configuration refused, defaults kept: {err}");
                Self::default()
            }
        }
    }

    /// The file read again after it changed, off the loop: `None` when it
    /// does not read, the desktop keeping what it had (said in the log);
    /// no file is the defaults.
    pub fn read_again(path: &Path) -> Option<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Self::default()),
            Err(err) => {
                tracing::warn!(file = %path.display(), %err, "configuration not read again; kept");
                return None;
            }
        };
        match toml::from_str(&text) {
            Ok(config) => Some(config),
            Err(err) => {
                tracing::warn!(file = %path.display(), "configuration refused, the last one kept: {err}");
                None
            }
        }
    }

    /// `$XDG_STATE_HOME/hocuspanda`, or under `~/.local/state`.
    pub fn default_state_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
        Some(base.join("hocuspanda"))
    }

    /// The form asked for the screen `name`: its own, else `[panel]`'s.
    pub fn form_choice(&self, name: &str) -> FormChoice {
        self.outputs
            .get(name)
            .and_then(|o| o.panel_form)
            .unwrap_or(self.panel.form)
    }

    /// `$XDG_CONFIG_HOME/hocuspanda/config.toml`, or under `~/.config`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;
        Some(base.join("hocuspanda/config.toml"))
    }
}

/// A layout's XKB layout and variant: `fr(bepo)` is `("fr", "bepo")`, `fr`
/// is `("fr", "")`. `None` for anything else: XKB names are letters,
/// digits, `_` and `-`.
pub fn split_layout(code: &str) -> Option<(&str, &str)> {
    let name = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match code.split_once('(') {
        Some((layout, rest)) => {
            let variant = rest.strip_suffix(')')?;
            (name(layout) && name(variant)).then_some((layout, variant))
        }
        None => name(code).then_some((code, "")),
    }
}

/// The layouts and the variants XKB is given, each list comma-separated in
/// the same order: `["fr", "fr(bepo)"]` is `("fr,fr", ",bepo")`. A code
/// that does not read goes as it is, for XKB to refuse.
pub fn xkb_lists(codes: &[String]) -> (String, String) {
    let (layouts, variants): (Vec<&str>, Vec<&str>) = codes
        .iter()
        .map(|c| split_layout(c).unwrap_or((c.as_str(), "")))
        .unzip();
    (layouts.join(","), variants.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_and_its_variant() {
        assert_eq!(split_layout("fr"), Some(("fr", "")));
        assert_eq!(split_layout("fr(bepo)"), Some(("fr", "bepo")));
        assert_eq!(split_layout("us(altgr-intl)"), Some(("us", "altgr-intl")));
        for bad in ["", "fr(", "fr()", "(bepo)", "fr(bepo", "fr bepo", "fr,us"] {
            assert_eq!(split_layout(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn xkb_gets_two_lists_in_the_same_order() {
        let codes = [
            "fr".to_owned(),
            "fr(bepo)".to_owned(),
            "us(intl)".to_owned(),
        ];
        assert_eq!(xkb_lists(&codes), ("fr,fr,us".into(), ",bepo,intl".into()));
        assert_eq!(xkb_lists(&[]), (String::new(), String::new()));
    }

    #[test]
    fn the_keyboard_section_reads_and_other_sections_wait_their_slice() -> anyhow::Result<()> {
        let config: Config = toml::from_str(
            "[keyboard]\nlayouts = [\"fr\", \"fr(bepo)\"]\noptions = \"caps:escape\"\n\
             [secretary]\nbriefing = true\n",
        )?;
        assert_eq!(config.keyboard.layouts, ["fr", "fr(bepo)"]);
        assert_eq!(config.keyboard.options, "caps:escape");
        assert_eq!(config.keyboard.repeat_delay, 200);
        assert!(toml::from_str::<Config>("[keyboard]\nlayout = \"fr\"\n").is_err());
        Ok(())
    }

    #[test]
    fn shortcuts_and_the_terminal_read() -> anyhow::Result<()> {
        let config: Config = toml::from_str(
            "[shortcuts]\nterminal = \"Ctrl+Alt+T\"\nlog-out = \"\"\n\
             [programs]\nterminal = \"foot -e htop\"\n",
        )?;
        assert_eq!(config.shortcuts.terminal, ["Ctrl+Alt+T".parse()?]);
        assert_eq!(config.shortcuts.log_out, []);
        // Several for one action.
        let two: Config = toml::from_str("[shortcuts]\nclose = [\"Super+Q\", \"Ctrl+Q\"]\n")?;
        assert_eq!(two.shortcuts.close, ["Super+Q".parse()?, "Ctrl+Q".parse()?]);
        assert_eq!(config.programs.terminal, "foot -e htop");
        // A shortcut that does not read refuses the file, as any mistake.
        assert!(toml::from_str::<Config>("[shortcuts]\nterminal = \"Hyper+T\"\n").is_err());
        assert!(toml::from_str::<Config>("[shortcuts]\nlaunch = \"Super+D\"\n").is_err());
        Ok(())
    }

    #[test]
    fn the_power_delays_read_as_the_settings_write_them() -> anyhow::Result<()> {
        use std::time::Duration;
        let none = Config::default();
        assert_eq!(none.idle_delay(false), Some(Duration::from_secs(300)));
        assert_eq!(none.idle_delay(true), Some(Duration::from_secs(180)));
        assert_eq!(none.screen_off_after(false), Some(Duration::from_secs(30)));
        assert_eq!(none.screen_off_after(true), Some(Duration::from_secs(10)));
        assert_eq!(none.suspend_after(false), None);
        assert_eq!(none.suspend_after(true), Some(Duration::from_secs(900)));
        assert!(none.power.lock_on_idle && none.power.lock_before_sleep);
        assert_eq!(none.power.lid, Lid::Suspend);
        assert_eq!(none.power.power_button, PowerButton::Ask);
        assert!(none.power.lid_ignored_when_docked && none.power.auto_saver);
        assert_eq!(none.power.auto_saver_below, 20);
        let set: Config = toml::from_str(
            "[idle]\ndelay = 600\n[power]\nlid = \"screen-off\"\n\
             power-button = \"power-off\"\nlock-on-idle = false\n\
             lock-before-sleep = false\nauto-saver = false\nauto-saver-below = 35\n\
             lid-ignored-when-docked = false\n\
             [power.battery]\nidle = \"2m\"\nscreen-off-after = \"0s\"\n\
             suspend-after = \"5m\"\n[power.ac]\nscreen-off-after = \"5m\"\n\
             suspend-after = \"1h\"\n",
        )?;
        assert_eq!(set.idle_delay(false), Some(Duration::from_secs(600)));
        assert_eq!(set.idle_delay(true), Some(Duration::from_secs(120)));
        assert_eq!(set.screen_off_after(true), Some(Duration::ZERO));
        assert_eq!(set.screen_off_after(false), Some(Duration::from_secs(300)));
        assert_eq!(set.suspend_after(true), Some(Duration::from_secs(300)));
        assert_eq!(set.suspend_after(false), Some(Duration::from_secs(3600)));
        assert!(!set.power.lock_on_idle && !set.power.lock_before_sleep);
        assert_eq!(set.power.lid, Lid::ScreenOff);
        assert_eq!(set.power.power_button, PowerButton::PowerOff);
        assert!(!set.power.auto_saver && !set.power.lid_ignored_when_docked);
        assert_eq!(set.power.auto_saver_below, 35);
        let never: Config = toml::from_str("[power.battery]\nidle = \"never\"\n")?;
        assert_eq!(never.idle_delay(true), None);
        // A value that does not read: its default, the rest of the file kept.
        let odd: Config = toml::from_str(
            "[power]\nlid = \"explode\"\nauto-saver-below = 90\nlock-on-idle = \"yes\"\n\
             [power.battery]\nidle = \"3 heures\"\n[desktops]\ncount = 6\n",
        )?;
        assert_eq!(odd.idle_delay(true), Some(Duration::from_secs(180)));
        assert_eq!(odd.power.lid, Lid::Suspend);
        assert_eq!(odd.power.auto_saver_below, 20);
        assert!(odd.power.lock_on_idle);
        assert_eq!(odd.desktops.count, 6);
        // A key not known refuses the file, as elsewhere.
        assert!(toml::from_str::<Config>("[power]\nlid-closed = \"lock\"\n").is_err());
        assert!(toml::from_str::<Config>("[power.ac]\nidle = \"5m\"\n").is_err());
        Ok(())
    }

    /// A file 6.1's Settings wrote: its battery's `screen-off` is the dim's
    /// delay, its `lock-after` says whether the session locks.
    #[test]
    fn a_file_written_by_6_1_keeps_its_battery_screen_off_as_the_dim_delay() -> anyhow::Result<()> {
        use std::time::Duration;
        let old: Config = toml::from_str(
            "[power]\nlid = \"suspend\"\n[power.battery]\nscreen-off = \"2m\"\n\
             lock-after = \"5m\"\nsuspend-after = \"15m\"\n[power.ac]\nlock-after = \"0s\"\n",
        )?;
        assert_eq!(old.idle_delay(true), Some(Duration::from_secs(120)));
        assert!(old.power.lock_on_idle);
        assert_eq!(old.power.old_keys().len(), 2);
        let never: Config = toml::from_str(
            "[power.battery]\nlock-after = \"never\"\n[power.ac]\nlock-after = \"never\"\n",
        )?;
        assert!(!never.power.lock_on_idle);
        // Locking stays on unless every source said never.
        let one: Config = toml::from_str(
            "[power.ac]\nlock-after = \"never\"\n[power.battery]\nlock-after = \"0s\"\n",
        )?;
        assert!(one.power.lock_on_idle);
        // 6.10's keys win over 6.1's.
        let both: Config = toml::from_str(
            "[power]\nlock-on-idle = true\n[power.battery]\nidle = \"1m\"\nscreen-off = \"30m\"\n\
             lock-after = \"never\"\n[power.ac]\nlock-after = \"never\"\n",
        )?;
        assert_eq!(both.idle_delay(true), Some(Duration::from_secs(60)));
        assert!(both.power.lock_on_idle);
        assert!(both.power.old.ignored);
        Ok(())
    }

    #[test]
    fn the_mice_and_touchpads_read_as_the_settings_write_them() -> anyhow::Result<()> {
        let none = Config::default();
        assert!(none.touchpad.tap_to_click, "tap is on by default");
        assert_eq!(none.touchpad.right_click, None);
        assert_eq!(none.pointer.acceleration, Acceleration::Adaptive);
        let set: Config = toml::from_str(
            "[pointer]\nspeed = -0.5\nacceleration = \"flat\"\nleft-handed = true\n\
             [touchpad]\nspeed = 1\ntap-to-click = false\nright-click = \"corner\"\n\
             scroll = \"edge\"\noff-with-mouse = true\nswipe-desktops = false\n",
        )?;
        assert_eq!(set.pointer.speed, -0.5);
        assert_eq!(set.pointer.acceleration, Acceleration::Flat);
        assert!(set.pointer.left_handed);
        assert_eq!(set.touchpad.speed, 1.0);
        assert!(!set.touchpad.tap_to_click);
        assert_eq!(set.touchpad.right_click, Some(RightClick::Corner));
        assert_eq!(set.touchpad.scroll, ScrollWay::Edge);
        assert!(set.touchpad.off_with_mouse && !set.touchpad.swipe_desktops);
        // A value that does not read: its default, the rest kept.
        let odd: Config = toml::from_str(
            "[touchpad]\nspeed = 3\nscroll = \"sideways\"\nnatural-scroll = true\n",
        )?;
        assert_eq!(odd.touchpad.speed, 0.0);
        assert_eq!(odd.touchpad.scroll, ScrollWay::TwoFingers);
        assert!(odd.touchpad.natural_scroll);
        // A key not known refuses the file.
        assert!(toml::from_str::<Config>("[touchpad]\ntap = true\n").is_err());
        Ok(())
    }

    #[test]
    fn desktops_are_from_1_to_9() -> anyhow::Result<()> {
        assert_eq!(Config::default().desktops.count, 4);
        assert_eq!(Config::default().idle.delay, Some(IDLE_ON_AC));
        let idle: Config = toml::from_str("[idle]\ndelay = 300\n")?;
        assert_eq!(idle.idle.delay, Some(std::time::Duration::from_secs(300)));
        let off: Config = toml::from_str("[idle]\ndelay = \"off\"\n")?;
        assert_eq!(off.idle.delay, None);
        for bad in ["0", "\"5m\"", "-3", "86401"] {
            assert!(
                toml::from_str::<Config>(&format!("[idle]\ndelay = {bad}\n")).is_err(),
                "{bad}"
            );
        }
        let six: Config = toml::from_str("[desktops]\ncount = 6\n")?;
        assert_eq!(six.desktops.count, 6);
        for bad in ["count = 0", "count = 10", "count = -1", "number = 3"] {
            let text = format!("[desktops]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_windows_and_the_ink_read_as_the_settings_write_them() -> anyhow::Result<()> {
        let none = Config::default();
        assert_eq!(none.windows.move_modifier, DragKey::Alt);
        assert_eq!(none.windows.resize_modifier, DragKey::Super);
        assert!(none.windows.magnet && none.windows.snap_by_drag && none.windows.wobble);
        assert_eq!(none.windows.alt_tab, AltTab::All);
        assert!(none.ink.enabled);
        let set: Config = toml::from_str(
            "[windows]\nmove-modifier = \"super\"\nresize-modifier = \"none\"\n\
             magnet = false\nsnap-by-drag = false\nwobble = false\nalt-tab = \"desktop\"\n\
             [ink]\nenabled = false\n",
        )?;
        assert_eq!(set.windows.move_modifier, DragKey::Super);
        assert_eq!(set.windows.resize_modifier, DragKey::None);
        assert!(!set.windows.magnet && !set.windows.snap_by_drag && !set.windows.wobble);
        assert_eq!(set.windows.alt_tab, AltTab::Desktop);
        assert!(!set.ink.enabled);
        // The overview's corner (#727): on unless said.
        assert!(none.overview.hot_corner);
        let off: Config = toml::from_str("[overview]\nhot-corner = false\n")?;
        assert!(!off.overview.hot_corner);
        assert!(toml::from_str::<Config>("[overview]\ncorner = true\n").is_err());
        for bad in [
            "move-modifier = \"ctrl\"",
            "alt-tab = \"some\"",
            "focus = \"hover\"",
        ] {
            let text = format!("[windows]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_rules_read_in_the_files_order() -> anyhow::Result<()> {
        let set: Config = toml::from_str(
            "[[windows.rules]]\napp-id = \"org.example.Mail\"\ndesktop = 2\n\
             size = \"maximized\"\nabove = true\n\n\
             [[windows.rules]]\napp-id = \"foot\"\n\n\
             [[windows.rules]]\napp-id = \"org.example.mail\"\nsize = \"left\"\n",
        )?;
        let rule = set.windows.rule("org.example.mail").cloned();
        assert_eq!(
            rule,
            Some(Rule {
                app_id: "org.example.Mail".into(),
                desktop: Some(2),
                size: OpenSize::Maximized,
                above: true,
            }),
            "the first, whatever the case"
        );
        let foot = set
            .windows
            .rule("foot")
            .map(|r| (r.desktop, r.size, r.above));
        assert_eq!(foot, Some((None, OpenSize::Last, false)));
        assert!(set.windows.rule("kitty").is_none());
        for bad in [
            "desktop = 0",
            "desktop = 10",
            "size = \"half\"",
            "opacity = 0.5",
        ] {
            let text = format!("[[windows.rules]]\napp-id = \"foot\"\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        assert!(toml::from_str::<Config>("[[windows.rules]]\nabove = true\n").is_err());
        Ok(())
    }

    #[test]
    fn the_desktops_names_are_by_position() -> anyhow::Result<()> {
        let named: Config = toml::from_str(
            "[desktops]\ncount = 3\nnames = [\"Travail\", \" \", \"Courrier\", \"Musique\"]\n",
        )?;
        let desktops = &named.desktops;
        assert_eq!(desktops.name(0), Some("Travail"));
        assert_eq!(desktops.name(1), None, "blank: \"Bureau 2\"");
        assert_eq!(desktops.name(5), None);
        // Past the count, not told; the blank ones last left out.
        assert_eq!(desktops.shown_names(), ["Travail", "", "Courrier"]);
        let first: Config = toml::from_str("[desktops]\nnames = [\"Travail\", \"\", \"\"]\n")?;
        assert_eq!(first.desktops.shown_names(), ["Travail"]);
        assert!(Config::default().desktops.shown_names().is_empty());
        Ok(())
    }

    #[test]
    fn a_screens_scale_is_the_configured_one_or_its_densitys() -> anyhow::Result<()> {
        let config: Config = toml::from_str("[outputs.eDP-1]\nscale = 1\n")?;
        // 2560×1600 on 340×210 mm: 191.9 dpi, 1 by itself.
        let panel = ((2560, 1600), (340, 210));
        assert_eq!(
            config.output_scale("eDP-1", panel.0, panel.1),
            (1.0, ScaleFrom::Config)
        );
        assert_eq!(
            config.output_scale("DP-2", panel.0, panel.1),
            (1.0, ScaleFrom::Automatic { dpi: Some(191) })
        );
        assert_eq!(
            config.output_scale("DP-2", (3840, 2160), (340, 190)),
            (2.0, ScaleFrom::Automatic { dpi: Some(287) })
        );
        assert_eq!(
            config.output_scale("HDMI-A-1", (1920, 1080), (0, 0)),
            (1.0, ScaleFrom::Automatic { dpi: None })
        );
        assert_eq!(
            ScaleFrom::Automatic { dpi: Some(191) }.describe("eDP-1"),
            "(automatic: 191 dpi) — [outputs.eDP-1] scale = ... in config.toml sets it"
        );
        assert_eq!(
            ScaleFrom::Automatic { dpi: None }.describe("HDMI-A-1"),
            "(automatic: size unknown) — [outputs.HDMI-A-1] scale = ... in config.toml sets it"
        );
        assert_eq!(
            ScaleFrom::Config.describe("eDP-1"),
            "(from [outputs.eDP-1] in config.toml)"
        );
        let fractional: Config = toml::from_str("[outputs.\"DP-2\"]\nscale = 1.25\n")?;
        assert_eq!(fractional.outputs["DP-2"].scale, Some(1.25));
        for bad in ["scale = 0", "scale = 5", "scale = \"1\"", "scael = 1"] {
            let text = format!("[outputs.eDP-1]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_cursors_theme_and_size_are_read_and_bounded() -> anyhow::Result<()> {
        let config: Config =
            toml::from_str("[appearance]\ncursor-theme = \"Adwaita\"\ncursor-size = 32\n")?;
        assert_eq!(config.appearance.cursor_theme.as_deref(), Some("Adwaita"));
        assert_eq!(config.appearance.cursor_size, Some(32));
        for bad in [
            "cursor-size = 4",
            "cursor-size = 300",
            "cursor-size = \"32\"",
        ] {
            let text = format!("[appearance]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_theme_and_accent_are_read_by_their_words() -> anyhow::Result<()> {
        use hocus_link::theme::{Accent, Scheme};
        let config: Config =
            toml::from_str("[appearance]\ntheme = \"light\"\naccent = \"teal\"\n")?;
        assert_eq!(config.appearance.theme, Scheme::Light);
        assert_eq!(config.appearance.accent, Accent::Teal);
        let config: Config = toml::from_str("")?;
        assert_eq!(
            config.appearance.theme_choice(Default::default()),
            Default::default()
        );
        for bad in ["theme = \"neon\"", "accent = \"amber\"", "theme = 1"] {
            let text = format!("[appearance]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_locale_and_the_text_scale_are_read() -> anyhow::Result<()> {
        use hocus_link::locale::{DateFormat, Language, TimeFormat};
        let config: Config = toml::from_str(
            "[locale]\nlanguage = \"en\"\ndate = \"iso\"\ntime = \"12h\"\n\
             [appearance]\ntext-scale = 1.2\n",
        )?;
        assert_eq!(config.locale.language, Language::English);
        assert_eq!(config.locale.date, DateFormat::Iso);
        assert_eq!(config.locale.time, Some(TimeFormat::H12));
        assert_eq!(config.appearance.text_scale, TextScale(120));
        let config: Config = toml::from_str("")?;
        assert_eq!(config.locale, Default::default());
        for bad in [
            "[locale]\nlanguage = \"de\"",
            "[locale]\ntime = \"13h\"",
            "[locale]\ncalendar = \"iso\"",
            "[appearance]\ntext-scale = 2.0",
        ] {
            assert!(toml::from_str::<Config>(bad).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn the_system_keyboard_reads_as_debian_writes_it() {
        let debian = "# KEYBOARD CONFIGURATION FILE\n\n# Consult the keyboard(5) manual page.\n\n\
                      XKBMODEL=\"pc105\"\nXKBLAYOUT=\"fr,us\"\nXKBVARIANT=\"bepo,\"\n\
                      XKBOPTIONS=\"grp:alt_shift_toggle,caps:escape\"\n\nBACKSPACE=\"guess\"\n";
        assert_eq!(
            SystemKeyboard::parse(debian),
            Some(SystemKeyboard {
                model: "pc105".into(),
                layout: "fr,us".into(),
                variant: "bepo,".into(),
                options: "grp:alt_shift_toggle,caps:escape".into(),
            })
        );
        // Unquoted or single-quoted values; no layout is no keyboard.
        let plain = SystemKeyboard::parse("XKBLAYOUT=de\nXKBVARIANT='nodeadkeys'\n");
        assert_eq!(plain.as_ref().map(|k| k.layout.as_str()), Some("de"));
        assert_eq!(plain.map(|k| k.variant), Some("nodeadkeys".into()));
        assert_eq!(
            SystemKeyboard::parse("XKBMODEL=\"pc105\"\nXKBLAYOUT=\"\"\n"),
            None
        );
        assert_eq!(
            SystemKeyboard::read(Path::new("/nonexistent/keyboard")),
            None
        );
    }

    #[test]
    fn the_configs_layouts_win_over_the_systems() {
        let system = SystemKeyboard::parse(
            "XKBLAYOUT=\"fr\"\nXKBVARIANT=\"bepo\"\nXKBOPTIONS=\"caps:escape\"\n",
        );
        let mut config = Config {
            system_keyboard: system,
            ..Config::default()
        };
        let names = config.xkb_names();
        assert_eq!(
            (names.layout.as_str(), names.variant.as_str()),
            ("fr", "bepo")
        );
        assert_eq!(
            (names.options.as_str(), names.from),
            ("caps:escape", "/etc/default/keyboard")
        );
        config.keyboard.options = "compose:ralt".into();
        assert_eq!(config.xkb_names().options, "compose:ralt");
        config.keyboard.layouts = vec!["us(intl)".into()];
        let names = config.xkb_names();
        assert_eq!(
            (names.layout.as_str(), names.variant.as_str()),
            ("us", "intl")
        );
        assert_eq!(names.from, "config.toml");
        config.keyboard.layouts.clear();
        config.system_keyboard = None;
        assert_eq!(config.xkb_names().layout, "");
    }

    #[test]
    fn the_bars_groups_show_by_default_and_take_an_order() -> anyhow::Result<()> {
        use hocus_link::BarGroup::{Agents, Tasks, Today};
        assert_eq!(
            Config::default().panel.groups(),
            hocus_link::BarGroups::default()
        );
        let config: Config = toml::from_str(
            "[panel]\nmeeting = false\nagents = false\norder = [\"tasks\", \"today\"]\n",
        )?;
        let groups = config.panel.groups();
        assert_eq!(groups.order, [Tasks, Today, Agents]);
        assert!(!groups.meeting && groups.today && groups.tasks && !groups.agents);
        // An unknown name is skipped, not refused.
        let config: Config = toml::from_str("[panel]\norder = [\"agents\", \"clock\"]\n")?;
        assert_eq!(config.panel.order, [Agents, Today, Tasks]);
        assert!(toml::from_str::<Config>("[panel]\norder = \"tasks\"\n").is_err());
        Ok(())
    }

    #[test]
    fn the_bar_stands_on_the_right_unless_told_left() -> anyhow::Result<()> {
        assert_eq!(Config::default().panel.side, PanelSide::Right);
        let config: Config = toml::from_str("[panel]\nside = \"left\"\n")?;
        assert_eq!(config.panel.side.link(), hocus_link::Side::Left);
        assert!(toml::from_str::<Config>("[panel]\nside = \"top\"\n").is_err());
        Ok(())
    }

    #[test]
    fn the_clock_takes_a_font_a_colour_a_transition_and_its_seconds() -> anyhow::Result<()> {
        let clock = Config::default().panel.clock;
        assert_eq!(
            (clock.font.as_str(), clock.color, clock.seconds),
            ("Martian Mono", [0xEE, 0xF1, 0xF6], true)
        );
        assert_eq!(clock.transition, hocus_link::Transition::Odometer);
        let config: Config = toml::from_str(
            "[panel.clock]\nfont = \"  JetBrains   Mono \"\ncolor = \"#3dffa2\"\n\
             transition = \"flip\"\nseconds = false\n",
        )?;
        let clock = config.panel.clock;
        assert_eq!(clock.font, "JetBrains Mono");
        assert_eq!(clock.color, [0x3D, 0xFF, 0xA2]);
        assert_eq!(clock.transition, hocus_link::Transition::Flip);
        assert!(!clock.seconds);
        // What the shell is told reads back the same.
        let line = hocus_link::ToShell::Clock(clock.look()).to_string();
        assert_eq!(
            line.parse::<hocus_link::ToShell>(),
            Ok(hocus_link::ToShell::Clock(clock.look()))
        );
        let empty: Config = toml::from_str("[panel.clock]\nfont = \"\"\n")?;
        assert_eq!(empty.panel.clock.font, "Martian Mono");
        for bad in [
            "color = \"white\"",
            "transition = \"spin\"",
            "seconds = \"yes\"",
            "size = 40",
        ] {
            let text = format!("[panel.clock]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    /// The commented file the package ships lists every setting with its
    /// default: uncommented, it reads as no file at all.
    #[test]
    fn the_shipped_example_is_the_defaults() -> anyhow::Result<()> {
        let example = include_str!("../../../packaging/config.toml");
        let uncommented: String = example
            .lines()
            .filter_map(|l| l.strip_prefix("# ").filter(|l| !l.starts_with(' ')))
            .filter(|l| l.starts_with('[') || l.contains(" = "))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(uncommented.contains("[shortcuts]"), "{uncommented}");
        assert_eq!(toml::from_str::<Config>(&uncommented)?, Config::default());
        Ok(())
    }

    #[test]
    fn agents_reads_its_four_keys_and_refuses_what_it_does_not_know() -> anyhow::Result<()> {
        use super::{Agents, HaloRule, PeekRule, Remind};
        assert_eq!(toml::from_str::<Config>("")?.agents, Agents::default());
        let config: Config = toml::from_str(
            "[agents]\nhalo = \"waiting\"\nremind = \"10m\"\npeek = \"off\"\npandas = false\n",
        )?;
        assert_eq!(
            config.agents,
            Agents {
                halo: HaloRule::Waiting,
                remind: Remind::Ten,
                peek: PeekRule::Off,
                pandas: false,
            }
        );
        let config: Config = toml::from_str("[agents]\nremind = \"never\"\npeek = \"waiting\"\n")?;
        assert_eq!(config.agents.remind.after(), None);
        assert_eq!(config.agents.peek, PeekRule::Waiting);
        assert_eq!(config.agents.halo, HaloRule::All);
        assert_eq!(
            Remind::Two.after(),
            Some(std::time::Duration::from_secs(120))
        );
        for bad in [
            "[agents]\nhalo = \"some\"\n",
            "[agents]\nremind = \"3m\"\n",
            "[agents]\npeek = \"done\"\n",
            "[agents]\nwave = true\n",
        ] {
            assert!(toml::from_str::<Config>(bad).is_err(), "{bad:?}");
        }
        Ok(())
    }
}
