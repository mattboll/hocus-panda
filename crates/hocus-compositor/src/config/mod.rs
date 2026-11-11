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

mod accessibility;
mod appearance;
mod devices;
mod keyboard;
mod outputs;
mod panel;
mod power;
mod windows;

pub use accessibility::*;
pub use appearance::*;
pub use devices::*;
pub use keyboard::*;
pub use outputs::*;
pub use panel::*;
pub use power::*;
pub use windows::*;

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
    /// What only someone developing the desktop needs (#783).
    pub developer: Developer,
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
    pub lang: hocus_words::Lang,
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
        // Givre (#740), with its ice blue.
        let config: Config = toml::from_str("[appearance]\ntheme = \"frost\"\naccent = \"ice\"\n")?;
        assert_eq!(config.appearance.theme, Scheme::Frost);
        assert_eq!(config.appearance.accent, Accent::Ice);
        let choice = config.appearance.theme_choice(Default::default());
        // What the shell is told reads back the same.
        let line = hocus_link::ToShell::Theme(choice).line();
        assert_eq!(
            hocus_link::ToShell::read(&line),
            Ok(hocus_link::ToShell::Theme(choice))
        );
        let config: Config = toml::from_str("")?;
        assert_eq!(
            config.appearance.theme_choice(Default::default()),
            Default::default()
        );
        for bad in [
            "theme = \"neon\"",
            "theme = \"givre\"",
            "accent = \"amber\"",
            "theme = 1",
        ] {
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
        assert_eq!(
            config.locale.language,
            Language::Chosen(hocus_words::Lang::English)
        );
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
        let line = hocus_link::ToShell::Clock(clock.look()).line();
        assert_eq!(
            hocus_link::ToShell::read(&line),
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
    /// default (`hocus_config::defaults`, which the Settings read theirs
    /// from): uncommented, it reads as no file at all.
    #[test]
    fn the_shipped_example_is_the_defaults() -> anyhow::Result<()> {
        use hocus_config::defaults;
        let uncommented = defaults::uncomment(defaults::SHIPPED);
        assert!(uncommented.contains("[shortcuts]"), "{uncommented}");
        assert_eq!(toml::from_str::<Config>(&uncommented)?, Config::default());
        Ok(())
    }

    /// The secretary was removed (2026-11-10): a file that still has her
    /// section, as Settings and her first run wrote it, reads as one
    /// without it.
    #[test]
    fn a_file_that_still_has_the_secretary_s_section_reads() -> anyhow::Result<()> {
        let config: Config = toml::from_str(
            "[secretary]\nmode = \"assistant\"\nname = \"Mina\"\n\
             [secretary.models]\nanswer = \"anthropic/claude\"\n\
             [secretary.providers.local]\nkind = \"ollama\"\n\
             [secretary.triage]\nabove = 40\n\
             [secretary.speak]\nstays = 12\n",
        )?;
        assert_eq!(config, Config::default());
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
