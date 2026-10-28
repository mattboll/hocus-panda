//! The configuration file, `$XDG_CONFIG_HOME/hocuspanda/config.toml`. Only
//! what this slice uses is read so far: the keyboard. It is read once, as
//! the compositor starts; sections other slices will read are let through.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keyboard: Keyboard,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    /// XKB layouts, each with its variant in brackets, the first active at
    /// start: `["fr", "fr(bepo)", "us"]`. Empty: XKB's defaults (and the
    /// `XKB_DEFAULT_*` variables).
    pub layouts: Vec<String>,
    /// XKB options, e.g. `"grp:alt_shift_toggle,caps:escape"`.
    pub options: String,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: i32,
    /// Repeats per second.
    pub repeat_rate: i32,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layouts: Vec::new(),
            options: String::new(),
            repeat_delay: 200,
            repeat_rate: 25,
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
             [bar]\nform = \"full\"\n",
        )?;
        assert_eq!(config.keyboard.layouts, ["fr", "fr(bepo)"]);
        assert_eq!(config.keyboard.options, "caps:escape");
        assert_eq!(config.keyboard.repeat_delay, 200);
        assert!(toml::from_str::<Config>("[keyboard]\nlayout = \"fr\"\n").is_err());
        Ok(())
    }
}
