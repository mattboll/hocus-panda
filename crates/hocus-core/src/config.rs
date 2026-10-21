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
        if self.accent().is_none() {
            let a = format!("{:?}", self.decorations.accent);
            return out_of_range("decorations.accent", a, "a colour like \"#7bc47f\"");
        }
        Ok(())
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
    fn the_whole_configuration_reads_back() -> Result<(), ConfigError> {
        let config = Config::parse("[keyboard]\nlayouts = [\"fr\", \"us\"]\n")?;
        assert_eq!(Config::parse(&config.to_toml())?, config);
        Ok(())
    }
}
