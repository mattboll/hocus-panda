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
    pub decorations: Decorations,
    pub appearance: Appearance,
    pub wallpaper: Wallpaper,
    pub launcher: Launcher,
    /// Key combination → action, in file order: `"Super+Return" = "spawn foot"`.
    pub shortcuts: toml::Table,
    /// Window rules, in file order: later rules override earlier ones.
    pub rules: Vec<Rule>,
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

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Windows {
    pub gaps: Gaps,
}

/// Space between windows (`inner`) and around them at the edges of the
/// work area (`outer`), logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gaps {
    pub inner: u32,
    pub outer: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Desktops {
    /// Desktops of an output seen for the first time.
    pub per_output: u32,
}

impl Default for Desktops {
    fn default() -> Self {
        Self { per_output: 4 }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Decorations {
    pub radius: f64,
    pub shadow: bool,
    pub ring_width: f64,
    /// `#rrggbb`.
    pub accent: String,
}

impl Default for Decorations {
    fn default() -> Self {
        Self {
            radius: 12.0,
            shadow: true,
            ring_width: 2.0,
            accent: "#7bc47f".into(),
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
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            accent: "theme".into(),
        }
    }
}

pub const THEMES: [&str; 5] = ["dark", "light", "matrix", "hud", "paper"];
pub const ACCENTS: [&str; 6] = ["theme", "bamboo", "amber", "blue", "pink", "wallpaper"];
pub const WALLPAPER_KINDS: [&str; 4] = ["glow", "image", "folder", "colour"];
pub const WALLPAPER_MODES: [&str; 4] = ["fill", "fit", "stretch", "center"];

/// What a screen shows behind its windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Wallpaper {
    /// `glow` (the theme's soft light, placed by the time of day), `image`,
    /// `folder` (its images through the day, in name order), `colour`.
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
        if self.accent().is_none() {
            let a = format!("{:?}", self.decorations.accent);
            return out_of_range("decorations.accent", a, "a colour like \"#7bc47f\"");
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

    /// The accent as RGB.
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
             [[rules]]\napp_id = \"chat-work\"\ndesktop = 2\npinned = true\n",
        )?;
        let rule = config.rule_for("chat-work", "");
        assert_eq!(rule.output.as_deref(), Some("laptop"));
        assert_eq!(rule.desktop, Some(2));
        assert_eq!(rule.pinned, Some(true));
        assert_eq!(config.rule_for("editor", ""), Rule::default());
        Ok(())
    }

    #[test]
    fn the_whole_configuration_reads_back() -> Result<(), ConfigError> {
        let config = Config::parse("[keyboard]\nlayouts = [\"fr\", \"us\"]\n")?;
        assert_eq!(Config::parse(&config.to_toml())?, config);
        Ok(())
    }
}
