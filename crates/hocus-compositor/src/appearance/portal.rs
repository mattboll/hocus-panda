//! The desktop portal's settings, as the desktop serves them
//! (`org.freedesktop.impl.portal.Settings`, version 2): xdg-desktop-portal
//! reads `org.freedesktop.appearance` from it for the applications
//! (`hocuspanda.portal`, `hocuspanda-portals.conf`), and hears its
//! `SettingChanged` when `[appearance]` changes in the file. Served on the
//! screen cast portal's connection (one name for the desktop's portal
//! backend, [`crate::share`]'s thread): what it answers is kept here, the
//! loop handing it each new value; nothing waits on the loop.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use hocus_link::theme::Choice;
use zbus::{Connection, OwnedVariant, Variant, object_server::SignalEmitter};

pub const NAMESPACE: &str = "org.freedesktop.appearance";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// What `org.freedesktop.appearance` says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Values {
    /// 1 dark, 2 light.
    pub color_scheme: u32,
    /// The accent's base, red, green and blue from 0 to 1.
    pub accent_color: (f64, f64, f64),
    /// 1 with high contrast, else 0.
    pub contrast: u32,
}

impl Values {
    pub fn of(choice: Choice, high_contrast: bool) -> Self {
        let [r, g, b] = choice.accent().base;
        let unit = |v: u8| (f64::from(v) / 255.0 * 1000.0).round() / 1000.0;
        Self {
            color_scheme: choice.scheme.portal_value(),
            accent_color: (unit(r), unit(g), unit(b)),
            contrast: u32::from(high_contrast),
        }
    }

    /// Its keys and values, in the order the spec lists them.
    fn entries(self) -> [(&'static str, Variant<'static>); 3] {
        [
            ("color-scheme", Variant::from(self.color_scheme)),
            ("accent-color", Variant::from(self.accent_color)),
            ("contrast", Variant::from(self.contrast)),
        ]
    }
}

impl Default for Values {
    fn default() -> Self {
        Self::of(Choice::default(), false)
    }
}

pub type Shared = Arc<Mutex<Values>>;

/// The interface's object.
pub struct Settings {
    pub values: Shared,
}

/// Whether `namespace` is asked for by `patterns` (none: all; a trailing
/// `*` matches any rest, as the portal's spec says).
fn wanted(patterns: &[String], namespace: &str) -> bool {
    patterns.is_empty()
        || patterns.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => namespace.starts_with(prefix),
            None => p == namespace,
        })
}

fn owned(value: Variant<'_>) -> Option<OwnedVariant> {
    OwnedVariant::try_from(value).ok()
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
impl Settings {
    async fn read_all(
        &self,
        namespaces: Vec<String>,
    ) -> HashMap<String, HashMap<String, OwnedVariant>> {
        let values = *self.values.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all = HashMap::new();
        if wanted(&namespaces, NAMESPACE) {
            let keys = values
                .entries()
                .into_iter()
                .filter_map(|(key, value)| Some((key.to_owned(), owned(value)?)))
                .collect();
            all.insert(NAMESPACE.to_owned(), keys);
        }
        all
    }

    async fn read(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedVariant> {
        let values = *self.values.lock().unwrap_or_else(PoisonError::into_inner);
        if namespace == NAMESPACE
            && let Some((_, value)) = values.entries().into_iter().find(|(k, _)| *k == key)
        {
            return owned(value).ok_or_else(|| zbus::fdo::Error::Failed("no value".to_owned()));
        }
        Err(zbus::fdo::Error::UnknownProperty(format!(
            "{namespace} {key}: not served here"
        )))
    }

    #[zbus(signal)]
    async fn setting_changed(
        emitter: &SignalEmitter<'_>,
        namespace: &str,
        key: &str,
        value: Variant<'_>,
    ) -> zbus::Result<()>;

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

/// The settings are now `new`: kept, and each key that changed signalled.
pub async fn changed(connection: &Connection, values: &Shared, new: Values) {
    let old = std::mem::replace(
        &mut *values.lock().unwrap_or_else(PoisonError::into_inner),
        new,
    );
    let Ok(emitter) = SignalEmitter::new(connection, PATH) else {
        return;
    };
    for ((key, before), (_, now)) in old.entries().into_iter().zip(new.entries()) {
        if before == now {
            continue;
        }
        tracing::info!(key, value = ?now, "the portal's appearance changed");
        if let Err(err) = Settings::setting_changed(&emitter, NAMESPACE, key, now).await {
            tracing::warn!(%err, key, "the portal's SettingChanged not sent");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::theme::{Accent, Scheme};

    #[test]
    fn the_values_follow_the_choice() {
        let light = Values::of(
            Choice {
                scheme: Scheme::Light,
                accent: Accent::Blue,
                ..Choice::default()
            },
            true,
        );
        assert_eq!(light.color_scheme, 2);
        assert_eq!(light.contrast, 1);
        assert_eq!(light.accent_color, (0.357, 0.553, 0.937));
        assert_eq!(Values::default().color_scheme, 1);
    }

    #[test]
    fn namespaces_match_by_name_or_prefix() {
        assert!(wanted(&[], NAMESPACE));
        assert!(wanted(&["org.freedesktop.*".to_owned()], NAMESPACE));
        assert!(wanted(&[NAMESPACE.to_owned()], NAMESPACE));
        assert!(!wanted(&["org.gnome.*".to_owned()], NAMESPACE));
    }
}
