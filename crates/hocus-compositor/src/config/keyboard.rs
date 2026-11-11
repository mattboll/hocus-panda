//! The keyboard: `[keyboard]`, the system's, the session's programs.

use super::*;

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
