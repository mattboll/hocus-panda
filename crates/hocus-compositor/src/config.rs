//! The configuration file, `$XDG_CONFIG_HOME/hocuspanda/config.toml`. Only
//! what this slice uses is read so far: the keyboard, the screens' scales,
//! the shortcuts and the programs they start. It is read once, as the compositor starts; sections
//! other slices will read are let through. `packaging/config.toml` lists
//! every setting with its default.

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
    pub shortcuts: Shortcuts,
    pub programs: Programs,
    pub appearance: Appearance,
    pub desktops: Desktops,
    /// The system's keyboard (`/etc/default/keyboard`), used when
    /// `[keyboard] layouts` is empty. Not part of the file: the program
    /// reads it once at start.
    #[serde(skip)]
    pub system_keyboard: Option<SystemKeyboard>,
}

/// How the desktop looks and moves, `[appearance]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Appearance {
    /// Windows fade instead of moving, and do not wobble.
    pub reduced_motion: bool,
    /// The focused window's rim is 2 px wide instead of 1.
    pub high_contrast: bool,
}

/// Virtual desktops, `[desktops]` (docs/design/windows.md §12).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Desktops {
    /// Desktops on each screen, from 1 to 9.
    #[serde(deserialize_with = "desktop_count")]
    pub count: usize,
}

impl Default for Desktops {
    fn default() -> Self {
        Self {
            count: crate::desktops::DEFAULT_COUNT,
        }
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

/// One screen's settings, `[outputs.eDP-1]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputSettings {
    /// From 0.5 to 4. None: the scale of its density.
    #[serde(deserialize_with = "scale")]
    pub scale: Option<f64>,
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
    /// Its density, `output_spec::default_scale`.
    Density,
}

impl Config {
    /// The scale of the screen on connector `name`: the configured one, or
    /// the one of its density.
    pub fn output_scale(
        &self,
        name: &str,
        pixels: (i32, i32),
        millimetres: (i32, i32),
    ) -> (f64, ScaleFrom) {
        match self.outputs.get(name).and_then(|o| o.scale) {
            Some(scale) => (scale, ScaleFrom::Config),
            None => (
                crate::output_spec::default_scale(pixels, millimetres),
                ScaleFrom::Density,
            ),
        }
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

    #[test]
    fn shortcuts_and_the_terminal_read() -> anyhow::Result<()> {
        let config: Config = toml::from_str(
            "[shortcuts]\nterminal = \"Ctrl+Alt+T\"\nlog-out = \"\"\n\
             [programs]\nterminal = \"foot -e htop\"\n",
        )?;
        assert_eq!(config.shortcuts.terminal, Some("Ctrl+Alt+T".parse()?));
        assert_eq!(config.shortcuts.log_out, None);
        assert_eq!(config.programs.terminal, "foot -e htop");
        // A shortcut that does not read refuses the file, as any mistake.
        assert!(toml::from_str::<Config>("[shortcuts]\nterminal = \"Hyper+T\"\n").is_err());
        assert!(toml::from_str::<Config>("[shortcuts]\nlaunch = \"Super+D\"\n").is_err());
        Ok(())
    }

    #[test]
    fn desktops_are_from_1_to_9() -> anyhow::Result<()> {
        assert_eq!(Config::default().desktops.count, 4);
        let six: Config = toml::from_str("[desktops]\ncount = 6\n")?;
        assert_eq!(six.desktops.count, 6);
        for bad in ["count = 0", "count = 10", "count = -1", "number = 3"] {
            let text = format!("[desktops]\n{bad}\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn a_screens_scale_is_the_configured_one_or_its_densitys() -> anyhow::Result<()> {
        let config: Config = toml::from_str("[outputs.eDP-1]\nscale = 1\n")?;
        // 2560×1600 on 340×210 mm: 1.75 by its density.
        let panel = ((2560, 1600), (340, 210));
        assert_eq!(
            config.output_scale("eDP-1", panel.0, panel.1),
            (1.0, ScaleFrom::Config)
        );
        assert_eq!(
            config.output_scale("DP-2", panel.0, panel.1),
            (1.75, ScaleFrom::Density)
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
}
