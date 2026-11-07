//! Screens known by who they are, and the sets they make
//! (docs/design/windows.md §12, "Sets of screens").
//!
//! - **A screen's identity** is its EDID's make, model and serial number:
//!   a different monitor on the same cable is another screen, the same
//!   one on another cable the same. Without a serial, the connector tells
//!   it apart (two screens of one model without serials swap when their
//!   cables do); without an EDID (a VM's, the headless outputs), the
//!   connector is all it is. Two screens of the same identity plugged at
//!   once are told apart by their connectors. The windows of a screen
//!   gone wait for that identity ([`crate::screens`]).
//! - **A set** is the identities of the screens plugged and on (neither
//!   turned off by the file nor under a closed lid). Each set keeps its
//!   own arrangement in the file, `[[screen-sets]]`: each screen's place,
//!   mode, scale and VRR, and which is primary, over `[outputs.NAME]`
//!   and `[panel] output`, which stay the defaults any set starts from.
//!   The Settings write it (6.9); the compositor only reads it.
//! - **The sets seen** (when each was last in use, its screens' sizes)
//!   are kept in `$XDG_STATE_HOME/hocuspanda/screen-sets`, written on a
//!   thread, for the Settings' list; "Oublier cet ensemble" forgets one
//!   there as the Settings take its table out of the file.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use smithay::output::Output;

use crate::{
    State,
    config::{OutputSettings, ScreenSet},
};

/// The state file the sets seen are kept in.
const STATE_FILE: &str = "screen-sets";

/// Who a screen is, from its EDID: what the backend could read of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub make: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
}

impl Identity {
    pub fn new(make: &str, model: &str, serial: &str) -> Self {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        Self {
            make: some(make),
            model: some(model),
            serial: some(serial),
        }
    }

    /// What it is known by on connector `connector`, before two of the
    /// same are told apart: `make model serial`; without a serial,
    /// `make model (connector)`; without an EDID, the connector.
    pub fn key(&self, connector: &str) -> String {
        let named: Vec<&str> = [&self.make, &self.model]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        match &self.serial {
            Some(serial) => {
                let mut parts = named;
                parts.push(serial);
                parts.join(" ")
            }
            None if !named.is_empty() => format!("{} ({connector})", named.join(" ")),
            None => connector.to_owned(),
        }
    }
}

/// The sets' file key: their screens, sorted, one per line.
pub fn set_name(screens: &[String]) -> String {
    let mut screens = screens.to_vec();
    screens.sort();
    screens.join("\n")
}

/// A screen of a set seen, as the Settings draw its row's icon and name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SeenScreen {
    pub key: String,
    #[serde(default)]
    pub built_in: bool,
    /// Millimetres, 0 unknown.
    #[serde(default)]
    pub mm: (i32, i32),
    /// Logical pixels.
    #[serde(default)]
    pub size: (i32, i32),
}

/// A set seen, and when it was last in use (seconds since 1970, what the
/// user reads).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub screens: Vec<SeenScreen>,
    pub seen: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Remembered {
    #[serde(default)]
    set: Vec<Seen>,
}

/// The screens' identities and the sets, on the loop.
#[derive(Debug, Default)]
pub struct Sets {
    /// Each connector's screen's key, as it was last plugged: kept once
    /// it goes, for its windows to wait for.
    keys: BTreeMap<String, String>,
    /// The sets seen, by [`set_name`].
    seen: BTreeMap<String, Seen>,
    /// The set in use, as last noted.
    current: Option<String>,
}

impl Sets {
    /// With the sets seen kept in `state_dir`, read once as the compositor
    /// starts, before its loop runs.
    pub fn load(state_dir: Option<&Path>) -> Self {
        let mut sets = Self::default();
        let Some(file) = state_dir.map(|d| d.join(STATE_FILE)) else {
            return sets;
        };
        match std::fs::read_to_string(&file) {
            Ok(text) => match toml::from_str::<Remembered>(&text) {
                Ok(remembered) => {
                    sets.seen = remembered
                        .set
                        .into_iter()
                        .map(|s| {
                            let keys: Vec<String> =
                                s.screens.iter().map(|k| k.key.clone()).collect();
                            (set_name(&keys), s)
                        })
                        .collect();
                }
                Err(err) => tracing::warn!(file = %file.display(), %err, "screen sets not read"),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(file = %file.display(), %err, "screen sets not read"),
        }
        sets
    }

    /// The sets seen, the latest first.
    pub fn seen(&self) -> Vec<(&String, &Seen)> {
        let mut seen: Vec<(&String, &Seen)> = self.seen.iter().collect();
        seen.sort_by(|a, b| b.1.seen.cmp(&a.1.seen).then(a.0.cmp(b.0)));
        seen
    }
}

impl State {
    /// Output `output` joined: the key it is known by, its identity's,
    /// told apart from a screen of the same identity already there by
    /// its connector.
    pub(crate) fn screen_joined(&mut self, output: &Output) {
        let name = output.name();
        let identity = output
            .user_data()
            .get::<Identity>()
            .cloned()
            .unwrap_or_default();
        let mut key = identity.key(&name);
        let taken = self
            .displays
            .outputs
            .iter()
            .filter(|o| o.name() != name)
            .any(|o| self.screen_sets.keys.get(&o.name()) == Some(&key));
        if taken && !key.ends_with(&format!("({name})")) && key != name {
            key = format!("{key} ({name})");
        }
        tracing::info!(output = name, screen = key, "a screen, known as");
        self.screen_sets.keys.insert(name, key);
    }

    /// The key the screen on connector `name` is (or was last) known by.
    pub(crate) fn screen_key(&self, name: &str) -> String {
        self.screen_sets
            .keys
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    /// The connector of the screen plugged now as `key`.
    pub(crate) fn connector_of(&self, key: &str) -> Option<String> {
        self.displays
            .outputs
            .iter()
            .map(Output::name)
            .find(|n| self.screen_key(n) == key)
    }

    /// The screens of the set in use: plugged, neither turned off by the
    /// file nor under a closed lid; their keys, sorted.
    pub(crate) fn current_set(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .displays
            .outputs
            .iter()
            .filter(|o| self.counts_in_set(o))
            .map(|o| self.screen_key(&o.name()))
            .collect();
        keys.sort();
        keys
    }

    fn counts_in_set(&self, output: &Output) -> bool {
        let name = output.name();
        self.config.outputs.get(&name).is_none_or(|s| s.enabled) && !self.lid_takes(output)
    }

    /// The file's arrangement of the set in use, if it has one.
    fn set_in_file(&self) -> Option<&ScreenSet> {
        self.config.screen_set(&self.current_set())
    }

    /// Screen `name`'s settings now: `[outputs.NAME]`, the set in use's
    /// arrangement over it.
    pub(crate) fn screen_settings(&self, name: &str) -> Option<OutputSettings> {
        let base = self.config.outputs.get(name).cloned();
        let key = self.screen_key(name);
        let over = self.set_in_file().and_then(|s| s.outputs.get(&key));
        match (base, over) {
            (base, Some(over)) => Some(over.over(base.unwrap_or_default())),
            (base, None) => base,
        }
    }

    /// The settings of a screen about to light on connector `name` as
    /// `identity`: those of the set it will make with the others.
    pub(crate) fn joining_settings(
        &self,
        name: &str,
        identity: &Identity,
    ) -> Option<OutputSettings> {
        let key = identity.key(name);
        let mut set = self.current_set();
        set.push(key.clone());
        let over = self
            .config
            .screen_set(&set)
            .and_then(|s| s.outputs.get(&key));
        let base = self.config.outputs.get(name).cloned();
        match (base, over) {
            (base, Some(over)) => Some(over.over(base.unwrap_or_default())),
            (base, None) => base,
        }
    }

    /// The primary screen the file asks for: the set in use's, else
    /// `[panel] output`.
    pub(crate) fn primary_wanted(&self) -> String {
        self.set_in_file()
            .and_then(|s| s.primary.as_deref())
            .and_then(|key| self.connector_of(key))
            .unwrap_or_else(|| self.config.panel.output.clone())
    }

    /// After the screens changed: the set in use noted, and the one left
    /// with the time it was left.
    pub(crate) fn note_screen_set(&mut self) {
        let keys = self.current_set();
        if keys.is_empty() {
            return;
        }
        let name = set_name(&keys);
        let now = crate::displays::wall_now();
        let screens: Vec<SeenScreen> = keys
            .iter()
            .map(|key| {
                let output = self
                    .displays
                    .outputs
                    .iter()
                    .find(|o| self.screen_key(&o.name()) == *key);
                let facts = output.and_then(|o| o.user_data().get::<crate::displays::Facts>());
                SeenScreen {
                    key: key.clone(),
                    built_in: output.is_some_and(crate::away::is_internal),
                    mm: facts.map_or((0, 0), |f| f.millimetres),
                    size: output
                        .and_then(crate::displays::logical_size)
                        .unwrap_or_default(),
                }
            })
            .collect();
        let changed = self.screen_sets.current.as_ref() != Some(&name);
        let was = self.screen_sets.seen.get(&name).cloned();
        if !changed && was.as_ref().is_some_and(|w| w.screens == screens) {
            return;
        }
        if changed
            && let Some(left) = self.screen_sets.current.take()
            && let Some(seen) = self.screen_sets.seen.get_mut(&left)
        {
            seen.seen = now;
        }
        if changed {
            tracing::info!(set = keys.join(" + "), "the set of screens");
        }
        self.screen_sets
            .seen
            .insert(name.clone(), Seen { screens, seen: now });
        self.screen_sets.current = Some(name);
        self.remember_screen_sets();
    }

    /// The Settings forget set `name` (6.9, "Oublier cet ensemble"): they
    /// take its table out of the file; it is no longer listed. The set in
    /// use is never forgotten.
    pub(crate) fn forget_screen_set(&mut self, name: &str) -> Result<(), String> {
        if self.screen_sets.current.as_deref() == Some(name) {
            return Err("the set in use".to_owned());
        }
        if self.screen_sets.seen.remove(name).is_some() {
            tracing::info!(
                set = name.replace('\n', " + "),
                "a set of screens forgotten"
            );
            self.remember_screen_sets();
        }
        self.displays.dirty = true;
        Ok(())
    }

    /// The sets as the Settings list them (6.9): those seen and those in
    /// the file, the one in use first; for each its screens, when it was
    /// last in use, whether the file arranges it and how many windows
    /// wait for one of its screens.
    pub(crate) fn sets_json(&self) -> serde_json::Value {
        let current = self.screen_sets.current.clone();
        let mut rows: Vec<(String, Vec<SeenScreen>, Option<i64>)> = self
            .screen_sets
            .seen()
            .into_iter()
            .map(|(name, s)| (name.clone(), s.screens.clone(), Some(s.seen)))
            .collect();
        for set in &self.config.screen_sets {
            let name = set_name(&set.screens);
            if !rows.iter().any(|r| r.0 == name) {
                let screens = set
                    .screens
                    .iter()
                    .map(|key| SeenScreen {
                        key: key.clone(),
                        built_in: false,
                        mm: (0, 0),
                        size: (0, 0),
                    })
                    .collect();
                rows.push((name, screens, None));
            }
        }
        rows.sort_by_key(|r| current.as_ref() != Some(&r.0));
        let waiting = self.windows_waiting();
        let list: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(name, screens, seen)| {
                let keys: Vec<String> = screens.iter().map(|s| s.key.clone()).collect();
                let waits = waiting.iter().filter(|k| keys.contains(k)).count();
                serde_json::json!({
                    "name": name,
                    "screens": screens.iter().map(|s| serde_json::json!({
                        "key": s.key,
                        "built_in": s.built_in,
                        "mm": [s.mm.0, s.mm.1],
                        "size": [s.size.0, s.size.1],
                    })).collect::<Vec<_>>(),
                    "current": current.as_ref() == Some(&name),
                    "seen": seen,
                    "in_file": self.config.screen_set(&keys).is_some(),
                    "waiting": waits,
                })
            })
            .collect();
        serde_json::Value::Array(list)
    }

    /// The sets seen, written on a thread: a file is no work for the loop.
    fn remember_screen_sets(&self) {
        let Some(dir) = self.config.state_dir.clone() else {
            return;
        };
        let remembered = Remembered {
            set: self.screen_sets.seen.values().cloned().collect(),
        };
        let text = match toml::to_string(&remembered) {
            Ok(text) => format!(
                "# The sets of screens seen and when each was last in use, for the\n\
                 # Settings' list. Written by Hocuspanda; their arrangements are\n\
                 # [[screen-sets]] in config.toml.\n{text}"
            ),
            Err(err) => {
                tracing::warn!(%err, "could not write the screen sets");
                return;
            }
        };
        crate::programs::on_a_thread("remember the screen sets".to_owned(), move || {
            write_state(&dir, &text);
        });
    }
}

/// Writes `text` into the state file in `dir`, through a temporary file so
/// that it is never read half written.
fn write_state(dir: &PathBuf, text: &str) {
    let file = dir.join(STATE_FILE);
    let written = std::fs::create_dir_all(dir).and_then(|()| {
        let partial = dir.join(format!("{STATE_FILE}.new"));
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &file)
    });
    if let Err(err) = written {
        tracing::warn!(file = %file.display(), %err, "could not remember the screen sets");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_is_known_by_its_edid_else_its_connector() {
        let full = Identity::new("DEL", "U3423WE", "7XY1234");
        assert_eq!(full.key("DP-2"), "DEL U3423WE 7XY1234");
        assert_eq!(full.key("HDMI-A-1"), "DEL U3423WE 7XY1234");
        let no_serial = Identity::new("DEL", "U2723QE", "");
        assert_eq!(no_serial.key("DP-2"), "DEL U2723QE (DP-2)");
        assert_eq!(Identity::default().key("HEADLESS-2"), "HEADLESS-2");
        assert_eq!(
            set_name(&["b".to_owned(), "a".to_owned()]),
            set_name(&["a".to_owned(), "b".to_owned()])
        );
    }
}
