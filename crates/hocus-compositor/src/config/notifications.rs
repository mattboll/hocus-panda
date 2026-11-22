//! Notifications per application, `[notifications.apps]` (#899,
//! docs/design/notifications.md "Per application"): each application's
//! level, by its desktop entry or its name; and
//! `[notifications.seen-on-focus]` (#923): whether opening it folds its
//! notifications. The compositor only passes them on to the shell, which
//! serves the notifications and applies them; followed live.

use std::collections::BTreeMap;

use serde::Deserialize;

/// `[notifications]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Notifications {
    /// Each application's level; those not named show everything.
    pub apps: BTreeMap<String, hocus_link::NotifyLevel>,
    /// Whether opening each folds its notifications; those not named do.
    pub seen_on_focus: BTreeMap<String, bool>,
}

impl Notifications {
    /// What the shell is told of it.
    pub fn levels(&self) -> hocus_link::NotifyLevels {
        hocus_link::NotifyLevels {
            apps: self.apps.clone(),
            seen_on_focus: self.seen_on_focus.clone(),
        }
    }
}
