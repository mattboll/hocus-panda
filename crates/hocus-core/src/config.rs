//! The configuration: `~/.config/hocuspanda/config.toml`. Defaults live
//! here; the file holds only what the user changed. A file that does not
//! read is reported with its line and key, and the previous configuration
//! stays.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub windows: Windows,
    pub desktops: Desktops,
    pub keyboard: Keyboard,
    pub mouse: Mouse,
    pub touchpad: Touchpad,
    pub decorations: Decorations,
    pub appearance: Appearance,
    pub wallpaper: Wallpaper,
    pub launcher: Launcher,
    pub dropdown: Dropdown,
    pub settings: Settings,
    pub idle: Idle,
    pub session: SessionConfig,
    pub screenshots: Screenshots,
    pub recording: Recording,
    pub night_light: NightLight,
    pub updates: Updates,
    pub phone: PhoneConfig,
    /// Calendar accounts and subscriptions (#75): `[[calendars]]`.
    pub calendars: Vec<CalendarAccount>,
    pub dock: Dock,
    pub pandas: Pandas,
    pub agents: Agents,
    pub language: Language,
    pub clock: Clock,
    pub hot_corners: HotCorners,
    pub gestures: Gestures,
    pub clipboard: ClipboardConfig,
    /// Key combination → action, in file order: `"Super+Return" = "spawn foot"`.
    pub shortcuts: toml::Table,
    /// Window rules, in file order: later rules override earlier ones.
    pub rules: Vec<Rule>,
    /// Each screen's mode, scale, rotation, place: `[[outputs]]`.
    pub outputs: Vec<crate::outputs::OutputConfig>,
    /// How each set of screens is placed: `[[layouts]]`.
    pub layouts: Vec<crate::outputs::Layout>,
}

/// Where and how windows matching `app_id` and `title` (globs with `*` and
/// `?`) open.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub output: Option<String>,
    /// From 1.
    pub desktop: Option<u32>,
    /// Width and height, logical pixels.
    pub size: Option<[i32; 2]>,
    pub maximized: Option<bool>,
    /// 0.1 to 1.
    pub opacity: Option<f64>,
    /// Shown on every desktop of its output.
    pub pinned: Option<bool>,
    /// Above the other windows.
    pub on_top: Option<bool>,
    /// What is behind it blurred, for a see-through window, where the
    /// compositor can blur (#37).
    pub blur: Option<bool>,
}

impl Rule {
    pub fn matches(&self, app_id: &str, title: &str) -> bool {
        self.app_id.as_deref().is_none_or(|p| glob(p, app_id))
            && self.title.as_deref().is_none_or(|p| glob(p, title))
    }

    /// `self` with what `later` sets over it.
    fn merged(self, later: &Rule) -> Rule {
        Rule {
            app_id: self.app_id,
            title: self.title,
            output: later.output.clone().or(self.output),
            desktop: later.desktop.or(self.desktop),
            size: later.size.or(self.size),
            maximized: later.maximized.or(self.maximized),
            opacity: later.opacity.or(self.opacity),
            pinned: later.pinned.or(self.pinned),
            on_top: later.on_top.or(self.on_top),
            blur: later.blur.or(self.blur),
        }
    }
}

/// `*` any run of characters, `?` one character; case-sensitive.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Windows {
    pub gaps: Gaps,
    /// A window dragged to a screen's edge or corner snaps there, a zone
    /// showing where first; off, it moves freely (its edges still stick).
    pub snap: bool,
    /// A dragged window bends like jelly (#35); off, it moves stiff. With
    /// reduced motion it never bends.
    pub wobbly: bool,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            gaps: Gaps::default(),
            snap: true,
            wobbly: true,
        }
    }
}

/// Space between windows (`inner`) and around them at the edges of the
/// work area (`outer`), logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gaps {
    pub inner: u32,
    pub outer: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Desktops {
    /// Desktops of an output seen for the first time.
    pub per_output: u32,
    /// A desktop's own theme and wallpaper (#49), to know at a glance
    /// where you are: `[[desktops.looks]]`.
    pub looks: Vec<DesktopLook>,
}

impl Default for Desktops {
    fn default() -> Self {
        Self {
            per_output: 4,
            looks: Vec::new(),
        }
    }
}

/// How one desktop looks; what is not set comes from `[appearance]` and
/// `[wallpaper]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DesktopLook {
    /// From 1.
    pub desktop: u32,
    /// Only on this output; every output's desktop of that number when
    /// not set.
    pub output: Option<String>,
    pub theme: Option<String>,
    pub accent: Option<String>,
    pub kind: Option<String>,
    pub image: Option<String>,
    pub folder: Option<String>,
    pub mode: Option<String>,
    pub colour: Option<String>,
}

impl Desktops {
    /// How desktop `index` (from 0) of `output` looks, if it has a look:
    /// one for that output wins over one for every output.
    pub fn look(&self, output: &str, index: usize) -> Option<&DesktopLook> {
        let number = index as u32 + 1;
        let mine = |l: &&DesktopLook| l.desktop == number;
        self.looks
            .iter()
            .filter(mine)
            .find(|l| l.output.as_deref() == Some(output))
            .or_else(|| self.looks.iter().filter(mine).find(|l| l.output.is_none()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    /// XKB layouts, the first active at start, e.g. `["fr", "us"]`.
    pub layouts: Vec<String>,
    /// XKB options, e.g. `"caps:escape"`.
    pub options: String,
    pub repeat_delay: u32,
    pub repeat_rate: u32,
    pub numlock: bool,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layouts: Vec::new(),
            options: String::new(),
            repeat_delay: 200,
            repeat_rate: 25,
            numlock: true,
        }
    }
}

/// Mice, trackballs and pointing sticks, set through libinput on a
/// session of its own (in a window of another session, that one's own
/// settings apply).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mouse {
    /// The content follows the wheel, as a finger on a phone pushes it.
    pub natural_scroll: bool,
    /// The buttons swapped, for the left hand.
    pub left_handed: bool,
    /// The pointer's speed, -1 (slowest) to 1 (fastest); 0 is libinput's.
    pub speed: f64,
}

/// Touchpads, set through libinput as mice are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Touchpad {
    /// The content follows the fingers.
    pub natural_scroll: bool,
    /// A tap clicks: one finger the left button, two the right, three the
    /// middle.
    pub tap_to_click: bool,
    /// Off while typing, so that a palm does not move the pointer.
    pub disable_while_typing: bool,
    /// -1 (slowest) to 1 (fastest); 0 is libinput's.
    pub speed: f64,
}

impl Default for Touchpad {
    fn default() -> Self {
        Self {
            natural_scroll: false,
            tap_to_click: false,
            disable_while_typing: true,
            speed: 0.0,
        }
    }
}

/// What one pointing device is set to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointing {
    pub natural_scroll: bool,
    pub left_handed: bool,
    pub speed: f64,
    /// For a touchpad only.
    pub tap_to_click: Option<bool>,
    pub disable_while_typing: Option<bool>,
}

impl Config {
    /// What a pointing device is set to: a touchpad's settings, or a
    /// mouse's. A touchpad's buttons follow the mouse's hand.
    pub fn pointing(&self, touchpad: bool) -> Pointing {
        let m = &self.mouse;
        if touchpad {
            let t = &self.touchpad;
            Pointing {
                natural_scroll: t.natural_scroll,
                left_handed: m.left_handed,
                speed: t.speed,
                tap_to_click: Some(t.tap_to_click),
                disable_while_typing: Some(t.disable_while_typing),
            }
        } else {
            Pointing {
                natural_scroll: m.natural_scroll,
                left_handed: m.left_handed,
                speed: m.speed,
                tap_to_click: None,
                disable_while_typing: None,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Decorations {
    pub radius: f64,
    pub shadow: bool,
    pub ring_width: f64,
    /// The focus ring's colour: `theme` (the accent in effect) or
    /// `#rrggbb`.
    pub accent: String,
    /// Title bars above windows whose frame the compositor draws.
    pub title_bars: bool,
    /// Which side of a title bar has its buttons: `right` or `left`.
    pub buttons: String,
}

impl Default for Decorations {
    fn default() -> Self {
        Self {
            radius: 12.0,
            shadow: true,
            ring_width: 2.0,
            accent: "theme".into(),
            title_bars: false,
            buttons: "right".into(),
        }
    }
}

/// The look of the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// `dark`, `light`, `matrix`, `hud` or `paper`.
    pub theme: String,
    /// `theme` (the theme's own), `bamboo`, `amber`, `blue`, `pink`, or
    /// `wallpaper` (taken from it, made readable).
    pub accent: String,
    /// Text size, percent of the theme's: 80 to 150.
    pub text_size: u32,
    pub high_contrast: bool,
    /// Animations jump to their end.
    pub reduced_motion: bool,
    /// Panels see-through over a blur of what is behind them, where the
    /// compositor can blur (#37); off, they are solid.
    pub transparency: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            accent: "theme".into(),
            text_size: 100,
            high_contrast: false,
            reduced_motion: false,
            transparency: true,
        }
    }
}

pub const THEMES: [&str; 5] = ["dark", "light", "matrix", "hud", "paper"];
pub const ACCENTS: [&str; 6] = ["theme", "bamboo", "amber", "blue", "pink", "wallpaper"];
pub const WALLPAPER_KINDS: [&str; 6] = ["glow", "image", "folder", "colour", "rain", "stars"];
pub const WALLPAPER_MODES: [&str; 4] = ["fill", "fit", "stretch", "center"];

/// What a screen shows behind its windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Wallpaper {
    /// `glow` (the theme's soft light, placed by the time of day), `image`,
    /// `folder` (its images through the day, in name order), `colour`, or
    /// one that moves: `rain` (the Matrix rain), `stars`.
    pub kind: String,
    pub image: Option<String>,
    pub folder: Option<String>,
    /// How an image covers the screen: `fill`, `fit`, `stretch`, `center`.
    pub mode: String,
    /// Around a fitted or centred image, or the whole screen; the theme's
    /// background if not set.
    pub colour: Option<String>,
    /// Per screen, by output name; what is not set comes from above.
    pub screens: Vec<ScreenWallpaper>,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self {
            kind: "glow".into(),
            image: None,
            folder: None,
            mode: "fill".into(),
            colour: None,
            screens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenWallpaper {
    pub output: String,
    pub kind: Option<String>,
    pub image: Option<String>,
    pub folder: Option<String>,
    pub mode: Option<String>,
    pub colour: Option<String>,
}

/// A screen's wallpaper with everything resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperSpec {
    pub kind: String,
    pub image: Option<String>,
    pub folder: Option<String>,
    pub mode: String,
    pub colour: Option<String>,
}

impl Wallpaper {
    /// What `output` shows on a desktop that `look`s its own way: the
    /// desktop's settings over the output's, over the general ones.
    pub fn for_desktop(&self, output: &str, look: Option<&DesktopLook>) -> WallpaperSpec {
        let spec = self.for_output(output);
        let Some(look) = look else {
            return spec;
        };
        // A desktop with its own kind starts from that kind's own settings.
        let or = |mine: &Option<String>, below: Option<String>| mine.clone().or(below);
        WallpaperSpec {
            kind: look.kind.clone().unwrap_or(spec.kind),
            image: or(&look.image, spec.image),
            folder: or(&look.folder, spec.folder),
            mode: look.mode.clone().unwrap_or(spec.mode),
            colour: or(&look.colour, spec.colour),
        }
    }

    /// What `output` shows: its own settings over the general ones.
    pub fn for_output(&self, output: &str) -> WallpaperSpec {
        let own = self.screens.iter().find(|s| s.output == output);
        let pick = |own: Option<&Option<String>>, general: &Option<String>| {
            own.and_then(|o| o.clone()).or_else(|| general.clone())
        };
        WallpaperSpec {
            kind: own
                .and_then(|s| s.kind.clone())
                .unwrap_or_else(|| self.kind.clone()),
            image: pick(own.map(|s| &s.image), &self.image),
            folder: pick(own.map(|s| &s.folder), &self.folder),
            mode: own
                .and_then(|s| s.mode.clone())
                .unwrap_or_else(|| self.mode.clone()),
            colour: pick(own.map(|s| &s.colour), &self.colour),
        }
    }
}

#[cfg(test)]
mod desktop_looks {
    use super::*;

    #[test]
    fn a_desktop_s_look_for_its_output_wins_over_one_for_every_output() {
        let config: Config = toml::from_str(
            r##"
            [[desktops.looks]]
            desktop = 2
            theme = "paper"

            [[desktops.looks]]
            desktop = 2
            output = "DP-1"
            theme = "matrix"
            kind = "colour"
            colour = "#102030"
            "##,
        )
        .expect("it reads");
        let d = &config.desktops;
        assert_eq!(
            d.look("eDP-1", 1).and_then(|l| l.theme.as_deref()),
            Some("paper")
        );
        assert_eq!(
            d.look("DP-1", 1).and_then(|l| l.theme.as_deref()),
            Some("matrix")
        );
        assert!(d.look("DP-1", 0).is_none());
        let paper = config.wallpaper.for_desktop("DP-1", d.look("DP-1", 1));
        assert_eq!(
            (paper.kind.as_str(), paper.colour.as_deref()),
            ("colour", Some("#102030"))
        );
        assert_eq!(config.wallpaper.for_desktop("DP-1", None).kind, "glow");
    }
}

/// Putting the last session back at start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    /// `ask` (a card at start), `always` or `never`.
    pub restore: String,
    /// Starts the XDG autostart entries.
    pub autostart: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            restore: "ask".into(),
            autostart: true,
        }
    }
}

/// The clipboard history (#71).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClipboardConfig {
    /// Keeps what is copied (never what password managers mark secret).
    pub history: bool,
    /// How many copies are kept, pinned ones besides.
    pub entries: u32,
    /// Kept across sessions, encrypted.
    pub keep: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            history: true,
            entries: 100,
            keep: true,
        }
    }
}

/// Where screenshots go (#69).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Screenshots {
    /// The folder they are saved in; empty: `Screenshots` in the pictures
    /// folder.
    pub folder: String,
    /// Saved to the folder.
    pub save: bool,
    /// Put on the clipboard.
    pub copy: bool,
    /// The program that reads text in a picture, as tesseract does:
    /// `PROGRAM IMAGE -` prints it.
    pub ocr: String,
    /// The annotation window, given the capture's file.
    pub annotate: String,
}

impl Default for Screenshots {
    fn default() -> Self {
        Self {
            folder: String::new(),
            save: true,
            copy: true,
            ocr: "tesseract".into(),
            annotate: "hocus-annotate".into(),
        }
    }
}

/// The pandas (#47), which show what happens: off, none is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pandas {
    pub enabled: bool,
}

impl Default for Pandas {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Coding agents (#41): the column's section, from an agent manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agents {
    pub enabled: bool,
    /// herdr's API socket; empty: its default one
    /// (`$XDG_CONFIG_HOME/herdr/herdr.sock`), which only the session
    /// looks for.
    pub herdr_socket: String,
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            enabled: true,
            herdr_socket: String::new(),
        }
    }
}

/// The discreet dock (#38): hidden while working, shown on an empty
/// desktop, at the bottom edge, or with its shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dock {
    pub enabled: bool,
    /// Applications kept in it, by desktop entry (`firefox.desktop`);
    /// open ones come after.
    pub pinned: Vec<String>,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            enabled: true,
            pinned: [
                "org.codeberg.dnkl.foot.desktop",
                "firefox.desktop",
                "firefox-esr.desktop",
                "org.gnome.Nautilus.desktop",
                "thunderbird.desktop",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

/// The interface's language (#51).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceLanguage {
    /// The session's locale (`LANG`).
    #[default]
    Auto,
    En,
    Fr,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Language {
    pub interface: InterfaceLanguage,
}

/// How the shell writes the time (#67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ClockFormat {
    /// 14:37.
    #[default]
    #[serde(rename = "24h")]
    TwentyFour,
    /// 2:37 PM.
    #[serde(rename = "12h")]
    Twelve,
}

impl ClockFormat {
    /// The time of day as the clock shows it.
    pub fn time(self, hour: u8, minute: u8) -> String {
        match self {
            Self::TwentyFour => format!("{hour:02}:{minute:02}"),
            Self::Twelve => {
                let half = if hour < 12 { "AM" } else { "PM" };
                let h = match hour % 12 {
                    0 => 12,
                    h => h,
                };
                format!("{h}:{minute:02} {half}")
            }
        }
    }
}

/// The clock: its format. Its time zone is the system's (set through
/// timedated, from the settings' Language and region page).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clock {
    pub format: ClockFormat,
}

impl Clock {
    /// Whether `name` can be a time zone's: `Area/City` letters, digits,
    /// `_`, `-` and `+`, never a way out of the database's folder.
    pub fn zone_name_is_valid(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 64
            && name.split('/').all(|part| {
                !part.is_empty()
                    && !part.starts_with('.')
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
            })
    }
}

/// What a hot corner, Super alone or a touchpad gesture does (#86).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    /// Every screen's windows in a grid.
    Overview,
    Launcher,
    /// The desktop, its windows out of the way until asked again.
    ShowDesktop,
    #[default]
    None,
}

impl Trigger {
    pub const ALL: [Trigger; 4] = [
        Trigger::Overview,
        Trigger::Launcher,
        Trigger::ShowDesktop,
        Trigger::None,
    ];

    /// As the file writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Launcher => "launcher",
            Self::ShowDesktop => "show-desktop",
            Self::None => "none",
        }
    }
}

/// A screen's four corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Corners {
    pub top_left: Trigger,
    pub top_right: Trigger,
    pub bottom_left: Trigger,
    pub bottom_right: Trigger,
}

/// One screen's corners set apart from the others'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CornersOverride {
    pub top_left: Option<Trigger>,
    pub top_right: Option<Trigger>,
    pub bottom_left: Option<Trigger>,
    pub bottom_right: Option<Trigger>,
}

/// Hot corners (#86): the pointer resting in a screen's corner, one that
/// is not against another screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotCorners {
    /// How long the pointer rests there first, so passing by does nothing.
    pub delay_ms: u32,
    pub top_left: Trigger,
    pub top_right: Trigger,
    pub bottom_left: Trigger,
    pub bottom_right: Trigger,
    /// By output name: `[hot_corners.outputs."HDMI-A-1"]`.
    pub outputs: std::collections::BTreeMap<String, CornersOverride>,
}

impl Default for HotCorners {
    fn default() -> Self {
        Self {
            delay_ms: 150,
            top_left: Trigger::None,
            top_right: Trigger::None,
            bottom_left: Trigger::None,
            bottom_right: Trigger::None,
            outputs: Default::default(),
        }
    }
}

impl HotCorners {
    /// The corners of the output named so.
    pub fn of(&self, output: &str) -> Corners {
        let o = self.outputs.get(output).copied().unwrap_or_default();
        Corners {
            top_left: o.top_left.unwrap_or(self.top_left),
            top_right: o.top_right.unwrap_or(self.top_right),
            bottom_left: o.bottom_left.unwrap_or(self.bottom_left),
            bottom_right: o.bottom_right.unwrap_or(self.bottom_right),
        }
    }
}

/// Super alone and touchpad gestures (#86).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gestures {
    /// Super pressed and let go with nothing else.
    pub super_alone: Trigger,
    /// Three fingers sideways switch desktops (#36).
    pub three_sideways: bool,
    pub three_up: Trigger,
    pub three_down: Trigger,
    pub four_up: Trigger,
    pub four_down: Trigger,
}

impl Default for Gestures {
    fn default() -> Self {
        Self {
            super_alone: Trigger::Overview,
            three_sideways: true,
            three_up: Trigger::Overview,
            three_down: Trigger::None,
            four_up: Trigger::Launcher,
            four_down: Trigger::None,
        }
    }
}

/// A calendar account or subscription; its password is in the keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarAccount {
    pub name: String,
    /// `caldav` (an account on a server) or `subscription` (an ICS link).
    #[serde(default = "caldav")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub user: String,
}

fn caldav() -> String {
    "caldav".into()
}

impl CalendarAccount {
    /// What names it, for its secret and its cache.
    pub fn id(&self) -> String {
        format!("{}@{}", self.user, self.url)
    }

    /// Passwords never travel in the clear: https, or plain http to this
    /// machine only.
    pub fn url_is_safe(&self) -> bool {
        let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"];
        let secure = ["https://", "webcal://"];
        secure.iter().any(|w| self.url.starts_with(w))
            || local.iter().any(|w| {
                self.url
                    .strip_prefix(w)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
            })
    }
}

/// System updates (#84).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Updates {
    pub enabled: bool,
    /// Hours between looks, 1 to 168.
    pub every_hours: u32,
    /// A command of the user's own that prints `NAME VERSION` (or `NAME OLD
    /// -> NEW`) per update; empty: the system's package manager.
    pub check: String,
    /// What installs them all; empty: the package manager through polkit.
    pub install: String,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            enabled: true,
            every_hours: 6,
            check: String::new(),
            install: String::new(),
        }
    }
}

/// The phone (#85), through KDE Connect when it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhoneConfig {
    pub enabled: bool,
    /// Its notifications as toasts, with a reply where it takes one.
    pub notifications: bool,
}

impl Default for PhoneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            notifications: true,
        }
    }
}

/// Night light (#78): warmer colours in the evening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NightLight {
    pub enabled: bool,
    /// `fixed` (from `from` to `to`) or `sunset` (from sunset to sunrise
    /// where `latitude` and `longitude` are, else as `fixed`).
    pub schedule: String,
    /// `HH:MM`, the user's time.
    pub from: String,
    pub to: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Kelvin at night, 1500 to 6500.
    pub temperature: u32,
    /// Outputs it leaves alone, by name.
    pub except: Vec<String>,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: "fixed".into(),
            from: "21:00".into(),
            to: "07:00".into(),
            latitude: None,
            longitude: None,
            temperature: 3500,
            except: Vec::new(),
        }
    }
}

/// `HH:MM` as minutes after midnight.
pub fn parse_clock(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Screen recordings (#70).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recording {
    /// The folder they are saved in; empty: `Screencasts` in the videos
    /// folder.
    pub folder: String,
    /// Frames a second, 10 to 60.
    pub fps: u32,
    /// Seconds counted down before it starts, 0 to 10.
    pub countdown: u32,
    /// Clicks drawn as rings in the video, never on the screen.
    pub show_clicks: bool,
    /// The graphics card encodes when it can (VA-API).
    pub hardware: bool,
    /// The encoder, as ffmpeg takes its arguments.
    pub encoder: String,
    /// The options offered when picking what to record (#131), as they
    /// start: the microphone, the system's sound, a webcam bubble, the keys
    /// pressed drawn in the video.
    pub microphone: bool,
    pub system_sound: bool,
    pub camera: bool,
    pub show_keys: bool,
    /// How the encoder reads sound: `pulse` (PipeWire too), `oss`
    /// (FreeBSD), `alsa`.
    pub sound: String,
    pub microphone_device: String,
    /// The system's sound: the monitor of what plays.
    pub system_device: String,
    /// How the encoder reads the webcam: `v4l2` (Linux, FreeBSD's webcamd).
    pub camera_input: String,
    pub camera_device: String,
    /// The recording toast's Trim (#132), given the file.
    pub editor: String,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            folder: String::new(),
            fps: 30,
            countdown: 3,
            show_clicks: true,
            hardware: true,
            encoder: "ffmpeg".into(),
            microphone: false,
            system_sound: false,
            camera: false,
            show_keys: false,
            sound: if cfg!(target_os = "freebsd") {
                "oss".into()
            } else {
                "pulse".into()
            },
            microphone_device: if cfg!(target_os = "freebsd") {
                "/dev/dsp".into()
            } else {
                "default".into()
            },
            system_device: "@DEFAULT_MONITOR@".into(),
            camera_input: "v4l2".into(),
            camera_device: "/dev/video0".into(),
            editor: "hocus-trim".into(),
        }
    }
}

/// What happens when nobody uses the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Idle {
    /// Seconds without input before the curtain dims the screens; 0 for
    /// never.
    pub curtain_after: u32,
    /// Seconds the curtain shows before the session locks.
    pub lock_after: u32,
    /// Whether the curtain ends by locking; otherwise it only dims.
    pub lock: bool,
}

impl Default for Idle {
    fn default() -> Self {
        Self {
            curtain_after: 300,
            lock_after: 30,
            lock: true,
        }
    }
}

/// The settings app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// What starts it; `--page <page>` is added for a page.
    pub command: String,
    /// How its window is recognised.
    pub app_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            command: "hocus-settings".into(),
            app_id: "hocus-settings".into(),
        }
    }
}

/// The drop-down terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dropdown {
    /// What starts it; its window must have `app_id`.
    pub command: String,
    pub app_id: String,
    /// Its share of the screen's height, 0.2 to 0.9.
    pub height: f64,
}

impl Default for Dropdown {
    fn default() -> Self {
        Self {
            command: "foot --app-id=hocus-dropdown".into(),
            app_id: "hocus-dropdown".into(),
            height: 0.45,
        }
    }
}

/// What the launcher offers besides applications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Launcher {
    pub calculator: bool,
    pub units: bool,
    pub urls: bool,
    pub web_search: bool,
    /// The search's address, `{}` where the words go.
    pub search_engine: String,
    pub large_type: bool,
    pub qr: bool,
}

impl Default for Launcher {
    fn default() -> Self {
        Self {
            calculator: true,
            units: true,
            urls: true,
            web_search: true,
            search_engine: "https://duckduckgo.com/?q={}".into(),
            large_type: true,
            qr: true,
        }
    }
}

/// `#rrggbb` as RGB.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Why a configuration file was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// What the file sets for the output `name`; the first entry for it
    /// wins.
    pub fn output(&self, name: &str) -> Option<&crate::outputs::OutputConfig> {
        self.outputs.iter().find(|o| o.name == name)
    }

    /// The layout for exactly these connected screens; the first wins.
    pub fn layout_for(&self, connected: &[&str]) -> Option<&crate::outputs::Layout> {
        self.layouts.iter().find(|l| l.is_for(connected))
    }

    /// Reads a configuration file's text: what it holds over the defaults.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|err| ConfigError(err.to_string()))?;
        config.validate(text)?;
        Ok(config)
    }

    /// Values TOML accepts but the desktop does not.
    fn validate(&self, text: &str) -> Result<(), ConfigError> {
        let out_of_range = |key: &str, value: String, range: &str| {
            let line = text
                .lines()
                .position(|l| {
                    l.trim_start()
                        .starts_with(key.rsplit('.').next().unwrap_or(key))
                })
                .map(|i| format!("line {}: ", i + 1))
                .unwrap_or_default();
            Err(ConfigError(format!(
                "{line}{key} = {value}: expected {range}"
            )))
        };
        let Gaps { inner, outer } = self.windows.gaps;
        if inner > 64 {
            return out_of_range("windows.gaps.inner", inner.to_string(), "0 to 64");
        }
        if outer > 64 {
            return out_of_range("windows.gaps.outer", outer.to_string(), "0 to 64");
        }
        for (key, speed) in [
            ("mouse.speed", self.mouse.speed),
            ("touchpad.speed", self.touchpad.speed),
        ] {
            if !(-1.0..=1.0).contains(&speed) {
                return out_of_range(key, speed.to_string(), "-1 to 1");
            }
        }
        if !(1..=8).contains(&self.desktops.per_output) {
            let n = self.desktops.per_output.to_string();
            return out_of_range("desktops.per_output", n, "1 to 8");
        }
        if !(0.0..=32.0).contains(&self.decorations.radius) {
            let r = self.decorations.radius.to_string();
            return out_of_range("decorations.radius", r, "0 to 32");
        }
        for (i, rule) in self.rules.iter().enumerate() {
            if let Some(o) = rule.opacity
                && !(0.1..=1.0).contains(&o)
            {
                return out_of_range(&format!("rules[{i}].opacity"), o.to_string(), "0.1 to 1");
            }
            if let Some(d) = rule.desktop
                && !(1..=8).contains(&d)
            {
                return out_of_range(&format!("rules[{i}].desktop"), d.to_string(), "1 to 8");
            }
        }
        for (i, output) in self.outputs.iter().enumerate() {
            if let Err(err) = output.check() {
                let line = text
                    .match_indices("[[outputs]]")
                    .nth(i)
                    .map(|(at, _)| format!("line {}: ", text[..at].lines().count() + 1))
                    .unwrap_or_default();
                return Err(ConfigError(format!("{line}outputs[{i}].{err}")));
            }
        }
        for (i, layout) in self.layouts.iter().enumerate() {
            if let Err(err) = layout.check() {
                return Err(ConfigError(format!("layouts[{i}].{err}")));
            }
        }
        if !["right", "left"].contains(&self.decorations.buttons.as_str()) {
            let b = format!("{:?}", self.decorations.buttons);
            return out_of_range("decorations.buttons", b, "right, left");
        }
        if !["ask", "always", "never"].contains(&self.session.restore.as_str()) {
            let r = format!("{:?}", self.session.restore);
            return out_of_range("session.restore", r, "ask, always, never");
        }
        if !(1..=168).contains(&self.updates.every_hours) {
            let h = self.updates.every_hours.to_string();
            return out_of_range("updates.every_hours", h, "1 to 168");
        }
        for (i, c) in self.calendars.iter().enumerate() {
            if !["caldav", "subscription"].contains(&c.kind.as_str()) {
                let k = format!("{:?}", c.kind);
                return out_of_range(&format!("calendars[{i}].kind"), k, "caldav, subscription");
            }
            if !c.url_is_safe() {
                let u = format!("{:?}", c.url);
                return out_of_range(&format!("calendars[{i}].url"), u, "an https:// address");
            }
        }
        let n = &self.night_light;
        if !(1500..=6500).contains(&n.temperature) {
            let t = n.temperature.to_string();
            return out_of_range("night_light.temperature", t, "1500 to 6500");
        }
        if !["fixed", "sunset"].contains(&n.schedule.as_str()) {
            let s = format!("{:?}", n.schedule);
            return out_of_range("night_light.schedule", s, "fixed, sunset");
        }
        for (key, value) in [("night_light.from", &n.from), ("night_light.to", &n.to)] {
            if parse_clock(value).is_none() {
                return out_of_range(key, format!("{value:?}"), "HH:MM");
            }
        }
        if !(10..=60).contains(&self.recording.fps) {
            let f = self.recording.fps.to_string();
            return out_of_range("recording.fps", f, "10 to 60");
        }
        if self.recording.countdown > 10 {
            let c = self.recording.countdown.to_string();
            return out_of_range("recording.countdown", c, "0 to 10");
        }
        if !(80..=150).contains(&self.appearance.text_size) {
            let s = self.appearance.text_size.to_string();
            return out_of_range("appearance.text_size", s, "80 to 150");
        }
        if !THEMES.contains(&self.appearance.theme.as_str()) {
            let t = format!("{:?}", self.appearance.theme);
            return out_of_range("appearance.theme", t, &THEMES.join(", "));
        }
        if !ACCENTS.contains(&self.appearance.accent.as_str()) {
            let a = format!("{:?}", self.appearance.accent);
            return out_of_range("appearance.accent", a, &ACCENTS.join(", "));
        }
        let w = &self.wallpaper;
        let kinds = std::iter::once(Some(&w.kind)).chain(w.screens.iter().map(|s| s.kind.as_ref()));
        for kind in kinds.flatten() {
            if !WALLPAPER_KINDS.contains(&kind.as_str()) {
                return out_of_range(
                    "wallpaper.kind",
                    format!("{kind:?}"),
                    &WALLPAPER_KINDS.join(", "),
                );
            }
        }
        let modes = std::iter::once(Some(&w.mode)).chain(w.screens.iter().map(|s| s.mode.as_ref()));
        for mode in modes.flatten() {
            if !WALLPAPER_MODES.contains(&mode.as_str()) {
                return out_of_range(
                    "wallpaper.mode",
                    format!("{mode:?}"),
                    &WALLPAPER_MODES.join(", "),
                );
            }
        }
        let colours =
            std::iter::once(w.colour.as_ref()).chain(w.screens.iter().map(|s| s.colour.as_ref()));
        for colour in colours.flatten() {
            if parse_hex(colour).is_none() {
                return out_of_range(
                    "wallpaper.colour",
                    format!("{colour:?}"),
                    "a colour like \"#0e0f12\"",
                );
            }
        }
        if !(0.2..=0.9).contains(&self.dropdown.height) {
            let h = self.dropdown.height.to_string();
            return out_of_range("dropdown.height", h, "0.2 to 0.9");
        }
        let engine = &self.launcher.search_engine;
        if !(engine.starts_with("https://") || engine.starts_with("http://"))
            || !engine.contains("{}")
        {
            let e = format!("{engine:?}");
            return out_of_range(
                "launcher.search_engine",
                e,
                "an http(s) address with {} where the words go",
            );
        }
        if self.decorations.accent != "theme" && self.accent().is_none() {
            let a = format!("{:?}", self.decorations.accent);
            return out_of_range(
                "decorations.accent",
                a,
                "\"theme\" or a colour like \"#7bc47f\"",
            );
        }
        Ok(())
    }

    /// What the rules say about a window, every matching rule merged in
    /// file order.
    pub fn rule_for(&self, app_id: &str, title: &str) -> Rule {
        self.rules
            .iter()
            .filter(|r| r.matches(app_id, title))
            .fold(Rule::default(), Rule::merged)
    }

    /// The shortcuts in effect: the defaults with the file's over them.
    pub fn shortcuts(&self) -> crate::Shortcuts {
        let mut not_text = Vec::new();
        let pairs: Vec<(&str, &str)> = self
            .shortcuts
            .iter()
            .filter_map(|(combo, action)| match action.as_str() {
                Some(action) => Some((combo.as_str(), action)),
                None => {
                    not_text.push(format!("shortcuts.{combo:?}: expected an action as text"));
                    None
                }
            })
            .collect();
        let mut shortcuts = crate::Shortcuts::resolve(pairs);
        shortcuts.warnings.extend(not_text);
        shortcuts
    }

    /// The focus ring's own colour as RGB; `None` when it follows the
    /// theme.
    pub fn accent(&self) -> Option<[u8; 3]> {
        parse_hex(&self.decorations.accent)
    }

    /// The whole configuration, defaults included, as TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_its_format() -> Result<(), ConfigError> {
        let config = Config::parse("[clock]\nformat = \"12h\"\n")?;
        assert_eq!(config.clock.format, ClockFormat::Twelve);
        assert_eq!(config.clock.format.time(14, 37), "2:37 PM");
        assert_eq!(config.clock.format.time(0, 5), "12:05 AM");
        assert_eq!(config.clock.format.time(12, 0), "12:00 PM");
        assert_eq!(ClockFormat::TwentyFour.time(9, 3), "09:03");
        assert_eq!(Config::parse("")?.clock, Clock::default());
        for bad in [
            "../../etc/passwd",
            "/Europe/Paris",
            "Europe/Paris ",
            "Europe//Paris",
        ] {
            assert!(!Clock::zone_name_is_valid(bad), "{bad}");
        }
        assert!(Clock::zone_name_is_valid("America/Argentina/Buenos_Aires"));
        // The zone is the system's now, not the file's.
        assert!(Config::parse("[clock]\ntime_zone = \"Asia/Tokyo\"\n").is_err());
        assert!(Config::parse("[clock]\nformat = \"13h\"\n").is_err());
        Ok(())
    }

    #[test]
    fn a_screen_wallpaper_falls_back_on_the_general_one() -> Result<(), ConfigError> {
        let c = Config::parse(
            "[wallpaper]\nkind = \"image\"\nimage = \"/a.png\"\n[[wallpaper.screens]]\noutput = \"DP-1\"\nimage = \"/b.png\"\nmode = \"fit\"\n",
        )?;
        let dp = c.wallpaper.for_output("DP-1");
        assert_eq!(
            (dp.kind.as_str(), dp.image.as_deref(), dp.mode.as_str()),
            ("image", Some("/b.png"), "fit")
        );
        let other = c.wallpaper.for_output("eDP-1");
        assert_eq!(
            (other.image.as_deref(), other.mode.as_str()),
            (Some("/a.png"), "fill")
        );
        let bad = Config::parse("[wallpaper]\nmode = \"tile\"\n");
        assert!(bad.is_err_and(|e| e.0.contains("wallpaper.mode")));
        Ok(())
    }

    #[test]
    fn an_empty_file_is_the_defaults() -> Result<(), ConfigError> {
        assert_eq!(Config::parse("")?, Config::default());
        Ok(())
    }

    #[test]
    fn a_file_holds_only_what_changes() -> Result<(), ConfigError> {
        let config = Config::parse("[windows.gaps]\nouter = 16\n")?;
        assert_eq!(
            config.windows.gaps,
            Gaps {
                inner: 0,
                outer: 16
            }
        );
        assert_eq!(config.desktops, Desktops::default());
        Ok(())
    }

    #[test]
    fn a_wrong_type_names_the_line_and_the_key() {
        let error = Config::parse("[windows.gaps]\nouter = \"big\"\n")
            .err()
            .map(|e| e.0)
            .unwrap_or_default();
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("outer"), "{error}");
    }

    #[test]
    fn unknown_keys_and_out_of_range_values_are_refused() {
        assert!(Config::parse("[windows]\ngap = 4\n").is_err());
        let error = Config::parse("[windows.gaps]\nouter = 100\n")
            .err()
            .map(|e| e.0)
            .unwrap_or_default();
        assert_eq!(error, "line 2: windows.gaps.outer = 100: expected 0 to 64");
        assert!(Config::parse("[decorations]\naccent = \"green\"\n").is_err());
    }

    #[test]
    fn a_touchpad_and_a_mouse_are_set_apart() -> Result<(), ConfigError> {
        let config = Config::parse(
            "[mouse]\nleft_handed = true\nspeed = -0.5\n\n\
             [touchpad]\nnatural_scroll = true\ntap_to_click = true\n",
        )?;
        let pad = config.pointing(true);
        assert!(pad.natural_scroll && pad.left_handed);
        assert_eq!(pad.tap_to_click, Some(true));
        assert_eq!(pad.disable_while_typing, Some(true));
        assert_eq!(pad.speed, 0.0);
        let mouse = config.pointing(false);
        assert!(!mouse.natural_scroll && mouse.left_handed);
        assert_eq!((mouse.speed, mouse.tap_to_click), (-0.5, None));
        let err = Config::parse("[touchpad]\nspeed = 2\n").expect_err("too fast");
        assert!(err.0.contains("touchpad.speed = 2"), "{err}");
        Ok(())
    }

    #[test]
    fn snapping_and_wobbling_are_on_until_turned_off() -> Result<(), ConfigError> {
        let windows = Config::parse("")?.windows;
        assert!(windows.snap && windows.wobbly);
        let windows = Config::parse("[windows]\nsnap = false\nwobbly = false\n")?.windows;
        assert!(!windows.snap && !windows.wobbly);
        Ok(())
    }

    #[test]
    fn desktops_are_named_per_screen() -> Result<(), ConfigError> {
        let c = Config::parse(
            "[[outputs]]\nname = \"DP-1\"\ndesktop_names = [\"Mail\", \"\", \"Code\"]\n",
        )?;
        let names = c.output("DP-1").map(|o| o.desktop_names.clone());
        assert_eq!(
            names,
            Some(vec!["Mail".into(), String::new(), "Code".into()])
        );
        let nine = format!(
            "[[outputs]]\nname = \"DP-1\"\ndesktop_names = [{}]\n",
            ["\"a\""; 9].join(", ")
        );
        assert!(Config::parse(&nine).is_err());
        Ok(())
    }

    #[test]
    fn globs_match_like_shell_patterns() {
        assert!(glob("org.mozilla.*", "org.mozilla.firefox"));
        assert!(glob("*chat*", "my-chat-app"));
        assert!(glob("fo?t", "foot"));
        assert!(!glob("foot", "footclient"));
        assert!(glob("*", ""));
    }

    #[test]
    fn later_rules_override_earlier_ones() -> Result<(), ConfigError> {
        let config = Config::parse(
            "[[rules]]\napp_id = \"chat*\"\noutput = \"laptop\"\ndesktop = 1\n\n\
             [[rules]]\napp_id = \"chat-work\"\ndesktop = 2\npinned = true\nblur = true\n",
        )?;
        let rule = config.rule_for("chat-work", "");
        assert_eq!(rule.output.as_deref(), Some("laptop"));
        assert_eq!(rule.desktop, Some(2));
        assert_eq!(rule.pinned, Some(true));
        assert_eq!(rule.blur, Some(true));
        assert_eq!(config.rule_for("chat-home", "").blur, None);
        assert_eq!(config.rule_for("editor", ""), Rule::default());
        Ok(())
    }

    #[test]
    fn the_whole_configuration_reads_back() -> Result<(), ConfigError> {
        let config = Config::parse("[keyboard]\nlayouts = [\"fr\", \"us\"]\n")?;
        assert_eq!(Config::parse(&config.to_toml())?, config);
        Ok(())
    }

    #[test]
    fn screens_are_set_by_name_and_wrong_ones_said_with_their_line() {
        let c = Config::parse(
            "[[outputs]]\nname = \"eDP-1\"\nscale = 1.5\ntransform = \"90\"\n\n\
             [[outputs]]\nname = \"DP-1\"\nmode = \"2560x1440@144\"\nposition = [2560, 0]\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.output("eDP-1").and_then(|o| o.scale), Some(1.5));
        assert_eq!(c.output("DP-1").and_then(|o| o.position), Some([2560, 0]));
        let err =
            Config::parse("[[outputs]]\nname = \"a\"\n\n[[outputs]]\nname = \"b\"\nscale = 9\n")
                .expect_err("scale 9 is refused");
        assert!(err.0.starts_with("line 4: outputs[1].scale"), "{err}");
    }
}
