//! The clipboard's history (docs/design/bar.md, 5.6): what the user
//! copied, newest first, the pinned items apart.
//!
//! The compositor hands the shell each copy (`copied`: its text or its PNG
//! image, and the application of the window focused as it was copied),
//! never a secret. The history keeps text and images; the same copy again
//! is the same item, moved to the top, as is an item used again. Pinned
//! items stay for good; the others, the last [`KEPT`], 30 days at
//! most. "Clear all" keeps the pinned ones.
//!
//! It lives here, in memory, for the popover ([`crate::popover::clipboard`])
//! to draw, and on disk for the next session ([`store`]): sealed with a key
//! kept in the session's keyring, else in a file of the user's alone; read
//! and written on a thread of its own, never on the one that draws.
//!
//! Its rules are the compositor's (`[clipboard] history` and `keep`,
//! 6.17): they show at once (off, only the pinned items; a shorter keep,
//! only what is younger), and what they leave out is erased [`GRACE`]
//! later, if they still say so: the Settings' "Annuler" within that time
//! brings it all back. Kept until the session ends, what is not pinned is
//! not on disk. How many items it holds goes to the compositor, for the
//! Settings.

pub mod keyring;
pub mod kind;
mod store;

use std::sync::{Arc, Mutex, PoisonError};

use hocus_link::{Clip, ClipKind, ClipRules, ToCompositor};

pub use kind::{Kind, Picture};

use crate::popover;

/// The items kept besides the pinned ones (how long: `[clipboard] keep`,
/// 30 days by default).
pub const KEPT: usize = 200;
const DAY_MS: i64 = 24 * 3600 * 1000;
/// How long what the rules leave out waits before it is erased: the
/// Settings' toast's, during which its "Annuler" brings it back.
pub const GRACE: std::time::Duration = std::time::Duration::from_secs(6);

/// An item of the history.
#[derive(Debug, Clone)]
pub struct Item {
    /// Its own number, which also names its files.
    pub key: u64,
    pub kind: Kind,
    /// Its text; an image's is empty.
    pub text: Arc<str>,
    /// The application of the window focused as it was copied, by its id
    /// (empty: none).
    pub app: String,
    /// When it was copied, or used last, as the user reads it:
    /// milliseconds since the epoch.
    pub at: i64,
    pub pinned: bool,
    /// An image's size and thumbnail.
    pub picture: Option<Picture>,
    /// What it holds, digested: the same copy again is the same item.
    pub digest: u64,
}

impl Item {
    /// Whether it holds the same as `other`.
    fn same(&self, other: &Self) -> bool {
        self.digest == other.digest
            && (self.kind == Kind::Image) == (other.kind == Kind::Image)
            && self.text == other.text
    }
}

/// FNV-1a: a digest that stays the same from one build to the next.
pub fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a change of the history asks of the disk.
#[derive(Debug, Default)]
pub struct Change {
    /// A new item, written whole.
    pub new: Option<Item>,
    /// An item kept already, whose details changed (its time, its pin).
    pub changed: Option<Item>,
    /// The items that went.
    pub gone: Vec<u64>,
}

/// The history: its items, newest first.
#[derive(Debug, Default)]
pub struct History {
    items: Vec<Item>,
    /// Every change counts: what is drawn from it is made again.
    version: u64,
    /// The rules shown (the compositor's last), and those its items were
    /// pruned by (the same, [`GRACE`] after a change).
    rules: ClipRules,
    applied: ClipRules,
}

impl History {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn rules(&self) -> ClipRules {
        self.rules
    }

    /// What it shows at `now`, by its rules: off, the pinned items only;
    /// else those its keep has not let go.
    pub fn shown(&self, now: i64) -> Vec<Item> {
        let oldest = self.rules.keep.days().map(|d| now - d * DAY_MS);
        self.items
            .iter()
            .filter(|i| {
                i.pinned || (self.rules.history && oldest.is_none_or(|oldest| i.at >= oldest))
            })
            .cloned()
            .collect()
    }

    /// How many items it shows, and how many of them are pinned.
    pub fn count(&self, now: i64) -> (u32, u32) {
        let shown = self.shown(now);
        let pinned = shown.iter().filter(|i| i.pinned).count();
        (
            u32::try_from(shown.len()).unwrap_or(u32::MAX),
            u32::try_from(pinned).unwrap_or(u32::MAX),
        )
    }

    /// The rules now: shown at once.
    pub fn set_rules(&mut self, rules: ClipRules) -> bool {
        if rules == self.rules {
            return false;
        }
        self.rules = rules;
        self.version += 1;
        true
    }

    /// The rules applied at `now`, [`GRACE`] after they changed: what they
    /// leave out goes.
    pub fn apply_rules(&mut self, now: i64) -> Vec<u64> {
        self.applied = self.rules;
        let mut gone = if self.rules.history {
            Vec::new()
        } else {
            self.clear()
        };
        gone.extend(self.prune(now));
        gone
    }

    pub fn get(&self, key: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }

    fn touched(&mut self) {
        self.version += 1;
        self.items
            .sort_by(|a, b| b.at.cmp(&a.at).then(b.key.cmp(&a.key)));
    }

    /// `item` copied at `now`: first. The same copy kept already moves up
    /// instead, from its new application, keeping its pin.
    pub fn add(&mut self, mut item: Item, now: i64) -> Change {
        item.at = now;
        let mut change = Change::default();
        match self.items.iter_mut().find(|kept| kept.same(&item)) {
            Some(kept) => {
                kept.at = now;
                kept.app = item.app;
                change.changed = Some(kept.clone());
            }
            None => {
                change.new = Some(item.clone());
                self.items.push(item);
            }
        }
        self.touched();
        change.gone = self.prune(now);
        change
    }

    /// Item `old` replaced by `item`, copied at `now` (a capture
    /// annotated, 5.9): `old` goes, `item` comes first with its pin.
    pub fn replace(&mut self, old: u64, mut item: Item, now: i64) -> Change {
        item.pinned = self.get(old).is_some_and(|i| i.pinned);
        let gone = self.remove(old);
        let mut change = self.add(item, now);
        if gone {
            change.gone.insert(0, old);
        }
        change
    }

    /// The items read back from the disk, with those copied meanwhile:
    /// what does not stay.
    pub fn loaded(&mut self, items: Vec<Item>, now: i64) -> Vec<u64> {
        let mut gone = Vec::new();
        for item in items {
            if self.items.iter().any(|kept| kept.same(&item)) {
                gone.push(item.key);
            } else {
                self.items.push(item);
            }
        }
        self.touched();
        gone.extend(self.prune(now));
        gone
    }

    /// Item `key` used at `now` (pasted or copied): first.
    pub fn used(&mut self, key: u64, now: i64) -> Option<Item> {
        let item = self.items.iter_mut().find(|i| i.key == key)?;
        item.at = now;
        let item = item.clone();
        self.touched();
        Some(item)
    }

    /// Every text item whose trimmed text has `digest`, pinned or not,
    /// gone: their keys.
    pub fn forget_secret(&mut self, digest: u64) -> Vec<u64> {
        let keys: Vec<u64> = self
            .items
            .iter()
            .filter(|i| {
                i.kind != Kind::Image && hocus_secretary_link::clip_digest(&i.text) == digest
            })
            .map(|i| i.key)
            .collect();
        keys.into_iter().filter(|k| self.remove(*k)).collect()
    }

    pub fn remove(&mut self, key: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.key != key);
        let gone = self.items.len() != before;
        if gone {
            self.touched();
        }
        gone
    }

    /// Item `key` pinned, or not any more.
    pub fn pin(&mut self, key: u64) -> Option<Item> {
        let item = self.items.iter_mut().find(|i| i.key == key)?;
        item.pinned = !item.pinned;
        let item = item.clone();
        self.touched();
        Some(item)
    }

    /// All but the pinned ones go.
    pub fn clear(&mut self) -> Vec<u64> {
        let gone: Vec<u64> = self
            .items
            .iter()
            .filter(|i| !i.pinned)
            .map(|i| i.key)
            .collect();
        self.items.retain(|i| i.pinned);
        self.touched();
        gone
    }

    /// What is past the limits at `now` goes: the unpinned items older
    /// than the rules applied keep them (30 days by default), and those
    /// past the [`KEPT`] newest.
    pub fn prune(&mut self, now: i64) -> Vec<u64> {
        let oldest = self
            .applied
            .keep
            .days()
            .map_or(i64::MIN, |d| now - d * DAY_MS);
        let mut unpinned = 0;
        let mut gone = Vec::new();
        self.items.retain(|item| {
            if item.pinned {
                return true;
            }
            unpinned += 1;
            let keep = unpinned <= KEPT && item.at >= oldest;
            if !keep {
                gone.push(item.key);
            }
            keep
        });
        if !gone.is_empty() {
            self.version += 1;
        }
        gone
    }
}

static HISTORY: Mutex<History> = Mutex::new(History {
    items: Vec::new(),
    version: 0,
    rules: ClipRules {
        history: true,
        keep: hocus_link::ClipKeep::Month,
    },
    applied: ClipRules {
        history: true,
        keep: hocus_link::ClipKeep::Month,
    },
});

/// The count last told the compositor.
static TOLD: Mutex<Option<(u32, u32)>> = Mutex::new(None);

/// `with` on the history.
pub fn with<T>(with: impl FnOnce(&mut History) -> T) -> T {
    with(&mut HISTORY.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The history changed: its popover draws it again, if it is open, and
/// the compositor hears how many items it holds.
fn changed() {
    popover::changed(popover::Item::Clipboard);
    report();
}

/// How many items it holds, to the compositor (for the Settings, 6.17),
/// when that changed since it was told; again with `again`.
fn report_count(again: bool) {
    let count = with(|h| h.count(crate::notifications::wall_ms()));
    let mut told = TOLD.lock().unwrap_or_else(PoisonError::into_inner);
    if !again && *told == Some(count) {
        return;
    }
    let (items, pinned) = count;
    if popover::tell(ToCompositor::ClipboardCount { items, pinned }) {
        *told = Some(count);
    }
}

fn report() {
    report_count(false);
}

/// Linked to the compositor: it hears how many items there are.
pub fn linked() {
    report_count(true);
}

/// The compositor's rules (`[clipboard] history` and `keep`, 6.17): shown
/// at once, applied [`GRACE`] later on the history's thread.
pub fn rules(rules: ClipRules) {
    let changed_now = with(|h| h.set_rules(rules));
    tracing::info!(
        history = rules.history,
        keep = %rules.keep.word(),
        changed = changed_now,
        "the clipboard's history's rules"
    );
    store::ask(store::Ask::Rules(rules));
    if changed_now {
        changed();
    } else {
        report();
    }
}

/// Reads the history back and keeps it from now on, on its own thread:
/// sealed in `$XDG_STATE_HOME/hocuspanda/clipboard`, with the key the
/// session's keyring keeps (on the bus the compositor names), else the
/// key file beside it.
pub fn start() {
    let dir = crate::launcher::apps::state_file("clipboard");
    let bus = std::env::var(hocus_link::NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty());
    store::start(dir, bus);
}

/// A client copied `clip`, from the application `app` (the compositor's
/// `copied`).
pub fn copied(app: String, clip: Clip) {
    store::ask(store::Ask::Copied {
        app,
        clip,
        at: crate::notifications::wall_ms(),
    });
}

/// Capture `capture` of the compositor's session (its `capture-copied`,
/// 5.8b), its PNG: an image copied from the capture
/// ([`hocus_link::CAPTURE_APP`], "Capture d'écran" / "Screenshot"), first.
/// The same capture again, its annotated image (5.9), replaces the item it
/// made: the old one and its files go, the new one is first, pinned if the
/// old one was.
pub fn captured(capture: u64, clip: Clip) {
    store::ask(store::Ask::Captured {
        capture,
        clip,
        at: crate::notifications::wall_ms(),
    });
}

/// Item `key` chosen: the clipboard takes it, and the window that had the
/// keyboard gets it (`paste`); it moves to the top.
pub fn choose(key: u64, paste: bool) {
    let Some(item) = with(|h| h.used(key, crate::notifications::wall_ms())) else {
        return;
    };
    tracing::debug!(
        key,
        paste,
        kind = item.kind.word(),
        "the clipboard's history gives an item"
    );
    if item.kind == Kind::Image {
        store::ask(store::Ask::Choose { key, paste });
    } else {
        popover::tell(ToCompositor::Clip {
            paste,
            clip: Clip {
                kind: ClipKind::Text,
                data: item.text.as_bytes().to_vec(),
            },
        });
    }
    store::ask(store::Ask::Changed(item));
    changed();
}

/// Item `key` goes.
pub fn remove(key: u64) {
    if with(|h| h.remove(key)) {
        tracing::debug!(key, "an item of the clipboard's history goes");
        store::ask(store::Ask::Remove(vec![key]));
        changed();
    }
}

/// Item `key` pinned, or not any more.
pub fn pin(key: u64) {
    if let Some(item) = with(|h| h.pin(key)) {
        tracing::debug!(
            key,
            pinned = item.pinned,
            "an item of the clipboard's history"
        );
        store::ask(store::Ask::Changed(item));
        changed();
    }
}

/// A key the secretary kept (#752): any copy of it leaves the history,
/// pinned or not, by its digest ([`hocus_secretary_link::clip_digest`]);
/// she never says the key itself.
pub fn forget_secret(digest: u64) {
    let gone = with(|h| h.forget_secret(digest));
    if !gone.is_empty() {
        tracing::info!(
            gone = gone.len(),
            "a key's copy leaves the clipboard's history"
        );
        store::ask(store::Ask::Remove(gone));
        changed();
    }
}

/// "Clear all": the pinned ones stay.
pub fn clear() {
    let gone = with(History::clear);
    tracing::debug!(gone = gone.len(), "the clipboard's history is cleared");
    if !gone.is_empty() {
        store::ask(store::Ask::Remove(gone));
    }
    changed();
}

/// The history as it opens: what is past its limits goes.
pub fn opening() {
    let gone = with(|h| h.prune(crate::notifications::wall_ms()));
    if !gone.is_empty() {
        store::ask(store::Ask::Remove(gone));
    }
}

/// A text item, for the tests.
#[cfg(test)]
pub fn text_item(key: u64, text: &str, app: &str, at: i64) -> Item {
    Item {
        key,
        kind: Kind::of(text),
        text: text.into(),
        app: app.to_owned(),
        at,
        pinned: false,
        picture: None,
        digest: digest(text.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_000_000_000;

    fn keys(h: &History) -> Vec<u64> {
        h.items().iter().map(|i| i.key).collect()
    }

    #[test]
    fn a_copy_comes_first_and_the_same_again_moves_up() {
        let mut h = History::default();
        h.add(text_item(1, "git status", "kitty", 0), NOW - 60_000);
        h.add(text_item(2, "https://docs.rs", "firefox", 0), NOW - 30_000);
        assert_eq!(keys(&h), [2, 1]);
        // Copied again, from another application: the same item, first.
        let change = h.add(text_item(3, "git status", "foot", 0), NOW);
        assert!(change.new.is_none());
        assert_eq!(change.changed.as_ref().map(|i| i.key), Some(1));
        assert_eq!(keys(&h), [1, 2]);
        assert_eq!(h.items()[0].app, "foot");
        // Used again: first.
        h.used(2, NOW + 1000);
        assert_eq!(keys(&h), [2, 1]);
    }

    /// A key the secretary kept (#752): its copies go, pinned or not, a
    /// trailing newline included; nothing else does.
    #[test]
    fn a_key_kept_by_the_secretary_leaves_the_history() {
        let key = "sk-ant-api03-test-made-up";
        let mut h = History::default();
        h.add(text_item(1, "git status", "kitty", 0), NOW - 3);
        h.add(text_item(2, key, "firefox", 0), NOW - 2);
        h.add(text_item(3, &format!("{key}\n"), "firefox", 0), NOW - 1);
        h.pin(3);
        let gone = h.forget_secret(hocus_secretary_link::clip_digest(key));
        assert_eq!(gone.len(), 2);
        assert_eq!(keys(&h), [1]);
    }

    #[test]
    fn the_pinned_stay_through_clear_all_and_the_limits() {
        let mut h = History::default();
        h.add(
            text_item(1, "ssh alex@192.168.1.24", "kitty", 0),
            NOW - 40 * DAY_MS,
        );
        h.pin(1);
        for n in 0..KEPT as u64 + 5 {
            h.add(
                text_item(100 + n, &format!("copy {n}"), "", 0),
                NOW + n as i64,
            );
        }
        // The oldest five unpinned went, the pin stayed though 40 days old.
        assert_eq!(h.items().len(), KEPT + 1);
        assert!(h.get(1).is_some_and(|i| i.pinned));
        assert!(h.get(100).is_none() && h.get(105).is_some());
        // Older than 30 days, unpinned: gone.
        let mut h = History::default();
        h.add(text_item(7, "old", "", 0), NOW - 31 * DAY_MS);
        assert!(h.prune(NOW).contains(&7));
        // Clear all keeps the pinned ones.
        let mut h = History::default();
        h.add(text_item(1, "a", "", 0), NOW);
        h.add(text_item(2, "b", "", 0), NOW + 1);
        h.pin(1);
        assert_eq!(h.clear(), [2]);
        assert_eq!(keys(&h), [1]);
    }

    #[test]
    fn a_capture_replaced_goes_and_its_new_image_comes_first_with_its_pin() {
        let mut h = History::default();
        h.add(
            text_item(1, "the capture", "hocuspanda.capture", 0),
            NOW - 2000,
        );
        h.pin(1);
        h.add(text_item(2, "a copy", "kitty", 0), NOW - 1000);
        let change = h.replace(1, text_item(3, "annotated", "hocuspanda.capture", 0), NOW);
        assert_eq!(change.new.as_ref().map(|i| i.key), Some(3));
        assert_eq!(change.gone, [1]);
        assert_eq!(keys(&h), [3, 2]);
        assert!(h.get(3).is_some_and(|i| i.pinned));
        // Gone meanwhile: the new one is only added.
        let change = h.replace(9, text_item(4, "again", "", 0), NOW + 1);
        assert!(change.gone.is_empty());
        assert!(h.get(4).is_some_and(|i| !i.pinned));
    }

    #[test]
    fn the_rules_show_at_once_and_erase_once_applied() {
        use hocus_link::ClipKeep;
        let mut h = History::default();
        h.add(text_item(1, "pinned", "", 0), NOW - 3 * DAY_MS);
        h.pin(1);
        h.add(text_item(2, "three days", "", 0), NOW - 3 * DAY_MS);
        h.add(text_item(3, "now", "", 0), NOW);
        // A day: what is older hides at once, and goes once applied.
        h.set_rules(ClipRules {
            history: true,
            keep: ClipKeep::Day,
        });
        let shown: Vec<u64> = h.shown(NOW).iter().map(|i| i.key).collect();
        assert_eq!(shown, [3, 1]);
        assert_eq!(h.count(NOW), (2, 1));
        assert_eq!(h.items().len(), 3, "nothing erased yet");
        // Back to 30 days before it applied: all of it shows again.
        h.set_rules(ClipRules::default());
        assert_eq!(h.count(NOW), (3, 1));
        // Off: the pinned items only; applied, the others go.
        h.set_rules(ClipRules {
            history: false,
            keep: ClipKeep::Month,
        });
        assert_eq!(h.count(NOW), (1, 1));
        let mut gone = h.apply_rules(NOW);
        gone.sort_unstable();
        assert_eq!(gone, [2, 3]);
        assert_eq!(keys(&h), [1]);
        // The session: nothing is too old.
        let mut h = History::default();
        h.set_rules(ClipRules {
            history: true,
            keep: ClipKeep::Session,
        });
        assert!(h.apply_rules(NOW).is_empty());
        h.add(text_item(4, "old", "", 0), NOW - 90 * DAY_MS);
        assert_eq!(h.count(NOW), (1, 0));
    }

    #[test]
    fn what_is_read_back_joins_what_was_copied_meanwhile() {
        let mut h = History::default();
        h.add(text_item(9, "fresh", "", 0), NOW);
        let mut old = text_item(1, "kept", "", 0);
        old.at = NOW - 1000;
        let twin = text_item(2, "fresh", "", NOW - 5000);
        let gone = h.loaded(vec![old, twin], NOW);
        assert_eq!(gone, [2]);
        assert_eq!(keys(&h), [9, 1]);
        assert_eq!(digest(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
