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
        let hex = self.decorations.accent.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
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
