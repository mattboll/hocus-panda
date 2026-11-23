//! The notifications' history, the bell's (5.4, #923): every notification,
//! newest first, where it is in the bell ([`Sight`]: new, noticed, seen),
//! an agent's pin, and whether its sender still holds it; kept 7 days in
//! `$XDG_STATE_HOME/hocuspanda/notifications`.
//!
//! Pure: the stack says what came and went, the history panel reads it,
//! grouped by application ([`grouped`]). What is written and read back
//! ([`encode`], [`decode`]) is one JSON object a line, the newest first; a
//! notification's actions are not kept, its sender's hold ending with the
//! shell.

use hocus_link::{NoteBefore, NotePin, NoteState};
use jiff::{Timestamp, Zoned, civil::Date};

use super::note::{IconFrom, Note, Timeout, Urgency};

/// How long the history keeps a notification.
pub const KEEP_MS: i64 = 7 * 24 * 3600 * 1000;
/// The most it keeps: past it, the oldest seen go first, then the oldest
/// noticed, then the oldest new.
pub const MOST: usize = 200;

/// Where a notification is in the bell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    /// Its bubble shows, or waits its turn: not counted yet.
    Showing,
    /// The badge counts it, its line has the blue dot: its bubble went
    /// without the pointer resting on it or a click, it was of low
    /// urgency, held while the user was not to be disturbed, or pinned.
    New,
    /// Seen in passing (the pointer rested on its bubble, the bubble was
    /// clicked or closed, the history listed it): its line stays open.
    Noticed,
    /// Its application was opened, its line clicked, or an agent tidied
    /// it: folded under "N vues".
    Seen,
}

impl Sight {
    /// As agents read it: a bubble on screen is new.
    pub const fn state(self) -> NoteState {
        match self {
            Self::Showing | Self::New => NoteState::New,
            Self::Noticed => NoteState::Noticed,
            Self::Seen => NoteState::Seen,
        }
    }

    pub const fn of(state: NoteState) -> Self {
        match state {
            NoteState::New => Self::New,
            NoteState::Noticed => Self::Noticed,
            NoteState::Seen => Self::Seen,
        }
    }

    /// Whether it is folded.
    pub fn folded(self) -> bool {
        self == Self::Seen
    }
}

/// A notification in the history.
#[derive(Debug, Clone)]
pub struct Kept {
    /// Its number ([`Note::number`]): the bell's, the journal's and the
    /// agents'.
    pub key: u64,
    /// The server's id while its sender holds it: its actions show, and its
    /// default is invoked by a click.
    pub live: Option<u32>,
    pub note: Note,
    pub seen: Sight,
    /// An agent's pin: at the top, new.
    pub pin: Option<NotePin>,
    /// The desktop's own (a capture, a meeting's reminder): it never folds.
    pub own: bool,
    /// How many times the user changed it (a click, its application
    /// opened): an agent's « Annuler » leaves one changed since as it is.
    pub touched: u64,
}

impl Kept {
    /// Its application, as the levels know it.
    pub fn app_id(&self) -> String {
        self.note.app_id()
    }

    /// What it was, for « Annuler ».
    pub fn before(&self) -> NoteBefore {
        NoteBefore {
            id: self.key,
            state: self.seen.state(),
            pin: self.pin.clone(),
            touched: self.touched,
        }
    }
}

/// The history.
#[derive(Debug, Default)]
pub struct History {
    /// Newest first.
    items: Vec<Kept>,
    /// The panel shows it: the badge says 0, and what it lists is noticed
    /// as it closes.
    open: bool,
    /// Changes with what it lists, for the panel to lay it out again.
    version: u64,
    /// The notifications still held by their senders that went for their
    /// age or past the most kept: to close (1).
    dropped: Vec<u32>,
}

impl History {
    pub fn items(&self) -> &[Kept] {
        &self.items
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// What the bell's badge counts: what is new, none while the history
    /// shows.
    pub fn unseen(&self) -> usize {
        if self.open {
            return 0;
        }
        self.items.iter().filter(|k| k.seen == Sight::New).count()
    }

    /// How many are pinned.
    #[cfg(test)]
    pub fn pins(&self) -> usize {
        self.items.iter().filter(|k| k.pin.is_some()).count()
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    /// What is past the most kept goes: the oldest seen first, then the
    /// oldest noticed, then the oldest new (a pinned one last).
    fn trim(&mut self) {
        let rank = |k: &Kept| match (k.pin.is_some(), k.seen) {
            (true, _) => 3,
            (false, Sight::Seen) => 0,
            (false, Sight::Noticed) => 1,
            (false, Sight::New | Sight::Showing) => 2,
        };
        while self.items.len() > MOST {
            // The oldest of the lowest rank: the items are newest first.
            let Some(at) = (0..self.items.len())
                .rev()
                .min_by_key(|i| self.items.get(*i).map_or(u8::MAX, rank))
            else {
                return;
            };
            let gone = self.items.remove(at);
            self.dropped.extend(gone.live);
        }
    }

    /// The notifications still held by their senders that went since last
    /// asked (for their age, or past the most kept).
    pub fn take_dropped(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.dropped)
    }

    /// `note` comes, live, `seen` so: the newest, under its number. One of
    /// the same id held by its sender is taken out first (replaced), its
    /// pin kept. `own`: the desktop's own.
    pub fn add(&mut self, note: Note, seen: Sight, own: bool) -> u64 {
        let replaced = self
            .items
            .iter()
            .position(|k| k.live == Some(note.id))
            .map(|at| self.items.remove(at));
        let key = note.number;
        let (pin, touched) = replaced.map_or((None, 0), |k| (k.pin, k.touched));
        // A pinned one stays new.
        let seen = if pin.is_some() && seen != Sight::Showing {
            Sight::New
        } else {
            seen
        };
        self.items.insert(
            0,
            Kept {
                key,
                live: Some(note.id),
                note,
                seen,
                pin,
                own,
                touched,
            },
        );
        self.trim();
        self.changed();
        key
    }

    /// The live notification `id` now says what `note` does, in its place.
    pub fn update(&mut self, note: Note) {
        if let Some(kept) = self.items.iter_mut().find(|k| k.live == Some(note.id)) {
            let key = kept.key;
            kept.note = note;
            kept.note.number = key;
            self.version += 1;
        }
    }

    /// The live notification `id`'s bubble went, the user's eye on it
    /// (`to`, noticed) or not (new): what it shows changes only from its
    /// bubble on screen, or from new to noticed. Seen stays seen.
    pub fn bubble_left(&mut self, id: u32, to: Sight) {
        if let Some(kept) = self.items.iter_mut().find(|k| {
            k.live == Some(id)
                && (k.seen == Sight::Showing
                    || k.seen == Sight::New && to == Sight::Noticed && k.pin.is_none())
        }) {
            kept.seen = to;
            self.version += 1;
        }
    }

    /// Notification `id`'s sender holds it no more (it was closed): its
    /// actions go.
    pub fn closed(&mut self, id: u32) {
        if let Some(kept) = self.items.iter_mut().find(|k| k.live == Some(id)) {
            kept.live = None;
            self.version += 1;
        }
    }

    /// The live notification `id` taken out (its application took it
    /// back): whether it was there.
    pub fn remove_live(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|k| k.live != Some(id));
        let gone = self.items.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    /// The one of `key` taken out.
    pub fn remove(&mut self, key: u64) -> Option<Kept> {
        let at = self.items.iter().position(|k| k.key == key)?;
        self.changed();
        Some(self.items.remove(at))
    }

    pub fn get(&self, key: u64) -> Option<&Kept> {
        self.items.iter().find(|k| k.key == key)
    }

    /// The panel opens, or closes: as it closes, what it listed new is
    /// noticed, but a pinned one. Whether the badge or the dots change.
    pub fn set_open(&mut self, open: bool) -> bool {
        if self.open == open {
            return false;
        }
        self.open = open;
        if !open {
            for kept in &mut self.items {
                if kept.seen == Sight::New && kept.pin.is_none() {
                    kept.seen = Sight::Noticed;
                }
            }
        }
        self.changed();
        true
    }

    /// The user saw these (`keys`): seen, folded, the pinned ones too when
    /// `unpin` (a click on its line), else left pinned (its application
    /// opened). The desktop's own never fold. How many changed.
    pub fn user_saw(&mut self, keys: &[u64], unpin: bool) -> usize {
        let mut changed = 0;
        for kept in self.items.iter_mut().filter(|k| keys.contains(&k.key)) {
            if kept.own || (kept.pin.is_some() && !unpin) {
                continue;
            }
            // Seen by the user, changed or not: an agent's « Annuler »
            // leaves it.
            kept.touched += 1;
            if kept.seen != Sight::Seen || kept.pin.is_some() {
                kept.seen = Sight::Seen;
                kept.pin = None;
                changed += 1;
            }
        }
        if changed > 0 {
            self.changed();
        }
        changed
    }

    /// An agent pins `key`: new, at the top. Refused when the bell no
    /// longer holds it, or [`hocus_link::PINS`] are pinned (saying which).
    pub fn pin(&mut self, key: u64, pin: NotePin) -> Result<NoteBefore, String> {
        let pinned: Vec<u64> = self
            .items
            .iter()
            .filter(|k| k.pin.is_some() && k.key != key)
            .map(|k| k.key)
            .collect();
        let Some(kept) = self.items.iter_mut().find(|k| k.key == key) else {
            return Err(format!(
                "no notification {key} in the bell: list_notifications says which there are"
            ));
        };
        if kept.pin.is_none() && pinned.len() >= hocus_link::PINS {
            let which: Vec<String> = pinned.iter().map(u64::to_string).collect();
            return Err(format!(
                "refused: {} notifications are pinned already ({}); unpin one first",
                hocus_link::PINS,
                which.join(", ")
            ));
        }
        let before = kept.before();
        kept.pin = Some(pin);
        kept.seen = Sight::New;
        self.changed();
        Ok(before)
    }

    /// An agent unpins `key`: back with its application's, its state kept.
    pub fn unpin(&mut self, key: u64) -> Result<NoteBefore, String> {
        let Some(kept) = self.items.iter_mut().find(|k| k.key == key) else {
            return Err(format!("no notification {key} in the bell"));
        };
        if kept.pin.is_none() {
            return Err(format!("notification {key} is not pinned"));
        }
        let before = kept.before();
        kept.pin = None;
        self.changed();
        Ok(before)
    }

    /// An agent tidies these (`keys`): seen, folded, as opening their
    /// application does; a pinned one is unpinned. The desktop's own stay.
    /// What they were before, for those that changed.
    pub fn tidy(&mut self, keys: &[u64]) -> Vec<NoteBefore> {
        let mut befores = Vec::new();
        for kept in self.items.iter_mut().filter(|k| keys.contains(&k.key)) {
            if kept.own || (kept.seen == Sight::Seen && kept.pin.is_none()) {
                continue;
            }
            befores.push(kept.before());
            kept.seen = Sight::Seen;
            kept.pin = None;
        }
        if !befores.is_empty() {
            self.changed();
        }
        befores
    }

    /// « Annuler »: each as it was, the last change first, if the bell
    /// still holds it and the user did not change it since. How many came
    /// back.
    pub fn undo(&mut self, befores: &[NoteBefore]) -> usize {
        let mut back = 0;
        for before in befores.iter().rev() {
            let Some(kept) = self.items.iter_mut().find(|k| k.key == before.id) else {
                continue;
            };
            if kept.touched != before.touched {
                continue;
            }
            kept.seen = Sight::of(before.state);
            kept.pin.clone_from(&before.pin);
            back += 1;
        }
        if back > 0 {
            self.changed();
        }
        back
    }

    /// What is older than 7 days at `now` (wall clock, milliseconds) goes:
    /// whether any did.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.items.len();
        let (kept, old): (Vec<Kept>, Vec<Kept>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|k| now - k.note.at < KEEP_MS);
        self.items = kept;
        self.dropped.extend(old.iter().filter_map(|k| k.live));
        let gone = self.items.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    /// What came before the numbers kept were known moves `by` after them.
    pub fn renumber(&mut self, by: u64) {
        if by == 0 {
            return;
        }
        for kept in &mut self.items {
            kept.key += by;
            kept.note.number += by;
        }
        self.changed();
    }

    /// What was kept before the shell started again, read back: after what
    /// came since, by time, newest first.
    pub fn restore(&mut self, kept: Vec<Restored>) {
        for r in kept {
            self.items.push(Kept {
                key: r.note.number,
                live: None,
                note: r.note,
                seen: r.seen,
                pin: r.pin,
                own: r.own,
                touched: 0,
            });
        }
        self.items.sort_by_key(|k| std::cmp::Reverse(k.note.at));
        self.trim();
        self.changed();
    }
}

/// A notification read back from the history's file.
#[derive(Debug, Clone)]
pub struct Restored {
    pub note: Note,
    pub seen: Sight,
    pub pin: Option<NotePin>,
    pub own: bool,
}

// --- By application (#923) ---------------------------------------------------------

/// An application's notifications in the list, newest first.
#[derive(Debug, Clone)]
pub struct Group<'a> {
    /// What it goes by, lowercase.
    pub app: String,
    /// New and noticed: its card.
    pub open: Vec<&'a Kept>,
    /// Seen: under its fold.
    pub seen: Vec<&'a Kept>,
}

/// The list as it shows: the pins, the newest pin first; the applications
/// with something new or noticed, the one with the newest such first; then
/// those with seen ones only, the newest first.
#[derive(Debug, Clone, Default)]
pub struct Grouped<'a> {
    pub pins: Vec<&'a Kept>,
    pub open: Vec<Group<'a>>,
    pub seen_only: Vec<Group<'a>>,
}

/// `items` (newest first) grouped by application.
pub fn grouped(items: &[Kept]) -> Grouped<'_> {
    let mut pins: Vec<&Kept> = items.iter().filter(|k| k.pin.is_some()).collect();
    pins.sort_by_key(|k| std::cmp::Reverse(k.pin.as_ref().map_or(0, |p| p.at)));
    let mut groups: Vec<Group<'_>> = Vec::new();
    for kept in items.iter().filter(|k| k.pin.is_none()) {
        let app = kept.app_id().to_lowercase();
        let at = match groups.iter().position(|g| g.app == app) {
            Some(at) => at,
            None => {
                groups.push(Group {
                    app,
                    open: Vec::new(),
                    seen: Vec::new(),
                });
                groups.len() - 1
            }
        };
        if let Some(group) = groups.get_mut(at) {
            if kept.seen.folded() {
                group.seen.push(kept);
            } else {
                group.open.push(kept);
            }
        }
    }
    let newest = |list: &[&Kept]| list.iter().map(|k| k.note.at).max().unwrap_or(i64::MIN);
    let (mut open, mut seen_only): (Vec<Group<'_>>, Vec<Group<'_>>) =
        groups.into_iter().partition(|g| !g.open.is_empty());
    open.sort_by_key(|g| std::cmp::Reverse(newest(&g.open)));
    seen_only.sort_by_key(|g| std::cmp::Reverse(newest(&g.seen)));
    Grouped {
        pins,
        open,
        seen_only,
    }
}

// --- On disk ----------------------------------------------------------------------

/// What is written of a notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Its number.
    pub id: u64,
    pub app: String,
    pub entry: Option<String>,
    pub icon_from: IconFrom,
    pub summary: String,
    pub body: String,
    pub pid: u32,
    pub agent: bool,
    pub conversation: Option<hocus_link::AgentConversation>,
    pub at: i64,
    pub state: NoteState,
    pub pin: Option<NotePin>,
    pub own: bool,
}

impl Record {
    pub fn of(kept: &Kept) -> Self {
        let note = &kept.note;
        Self {
            id: kept.key,
            app: note.app.clone(),
            entry: note.entry.clone(),
            icon_from: note.icon_from.clone(),
            summary: note.summary.clone(),
            body: note.body.clone(),
            pid: note.pid,
            agent: note.agent,
            conversation: note.conversation.clone(),
            at: note.at,
            // A bubble on screen as the shell ends was not seen.
            state: kept.seen.state(),
            pin: kept.pin.clone(),
            own: kept.own,
        }
    }

    /// The notification it was, its sender holding it no more.
    pub fn restored(self) -> Restored {
        let note = Note {
            id: 0,
            number: self.id,
            app: self.app,
            entry: self.entry,
            icon: None,
            summary: self.summary,
            body: self.body,
            actions: Vec::new(),
            default: false,
            urgency: Urgency::Normal,
            timeout: Timeout::Never,
            progress: None,
            resident: false,
            pid: self.pid,
            agent: self.agent,
            conversation: self.conversation,
            icon_from: self.icon_from,
            at: self.at,
            sound: None,
            transient: false,
            ancestry: Vec::new(),
        };
        Restored {
            note,
            seen: Sight::of(self.state),
            pin: self.pin,
            own: self.own,
        }
    }
}

/// The history as written: a JSON object a line.
pub fn encode(records: &[Record]) -> String {
    let mut out = String::new();
    for r in records {
        let mut line = serde_json::json!({
            "id": r.id,
            "app": r.app,
            "entry": r.entry,
            "app_icon": r.icon_from.app_icon,
            "image_path": r.icon_from.image_path,
            "summary": r.summary,
            "body": r.body,
            "pid": r.pid,
            "agent": r.agent,
            "conversation": r.conversation,
            "at": r.at,
            "state": r.state,
        });
        if let Some(pin) = &r.pin {
            line["pin"] = serde_json::json!(pin);
        }
        if r.own {
            line["own"] = serde_json::json!(true);
        }
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}

/// What `text` keeps that is younger than 7 days at `now` (milliseconds),
/// and how many it dropped for their age; lines that do not read are left
/// out. A line written before #923 has no number (0: given one as it is
/// read back) and says `unseen`: new, else noticed.
pub fn decode(text: &str, now: i64) -> (Vec<Record>, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let text = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_owned);
        let Some(at) = value.get("at").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if now - at >= KEEP_MS {
            dropped += 1;
            continue;
        }
        let state = value
            .get("state")
            .and_then(|s| serde_json::from_value::<NoteState>(s.clone()).ok())
            .unwrap_or_else(|| {
                let unseen = value
                    .get("unseen")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if unseen {
                    NoteState::New
                } else {
                    NoteState::Noticed
                }
            });
        kept.push(Record {
            id: value
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            app: text("app").unwrap_or_default(),
            entry: text("entry"),
            icon_from: IconFrom {
                app_icon: text("app_icon").unwrap_or_default(),
                image_path: text("image_path"),
            },
            summary: text("summary").unwrap_or_default(),
            body: text("body").unwrap_or_default(),
            pid: value
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or(0),
            agent: value
                .get("agent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            conversation: value
                .get("conversation")
                .and_then(|c| serde_json::from_value(c.clone()).ok()),
            at,
            state,
            pin: value
                .get("pin")
                .and_then(|p| serde_json::from_value(p.clone()).ok()),
            own: value
                .get("own")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        });
        if kept.len() == MOST {
            break;
        }
    }
    (kept, dropped)
}

// --- How long ago -------------------------------------------------------------------

/// The list's sections of old (5.4), by which the age is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    /// Under an hour.
    Recent,
    /// Today, an hour ago or more.
    Today,
    Yesterday,
    Older,
}

/// The section of a notification that came at `at` (milliseconds), at
/// `now` on the wall clock: by the hour, then by the days of the calendar.
fn section(at: i64, now: &Zoned) -> Section {
    let ago = now.timestamp().as_millisecond() - at;
    if ago < 3600 * 1000 {
        return Section::Recent;
    }
    let day = day_of(at, now);
    let today = now.date();
    if day == Some(today) {
        Section::Today
    } else if day.is_some() && day == today.yesterday().ok() {
        Section::Yesterday
    } else {
        Section::Older
    }
}

/// The calendar day of `at` (milliseconds) in `now`'s time zone.
fn day_of(at: i64, now: &Zoned) -> Option<Date> {
    let at = Timestamp::from_millisecond(at).ok()?;
    Some(at.to_zoned(now.time_zone().clone()).date())
}

/// How long ago `at` was at `now`: whole minutes under an hour, whole hours
/// today, else whole days (yesterday is a day).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ago {
    Now,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Days(i64),
}

pub fn ago(at: i64, now: &Zoned) -> Ago {
    let ms = (now.timestamp().as_millisecond() - at).max(0);
    let minutes = ms / 60_000;
    match section(at, now) {
        Section::Recent if minutes < 1 => Ago::Now,
        Section::Recent => Ago::Minutes(minutes),
        Section::Today => Ago::Hours(minutes / 60),
        Section::Yesterday => Ago::Yesterday,
        Section::Older => {
            let days = match (day_of(at, now), Some(now.date())) {
                (Some(then), Some(today)) => (today - then).get_days().into(),
                _ => ms / (24 * 3600 * 1000),
            };
            Ago::Days(days.max(2))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::notifications::note::LIFE;

    pub(crate) fn note(id: u32, at: i64) -> Note {
        Note {
            id,
            number: u64::from(id),
            app: "Slack".to_owned(),
            entry: Some("slack".to_owned()),
            icon: None,
            summary: format!("Message {id}"),
            body: "Bonjour\ttout le monde\n« ça va ? »".to_owned(),
            actions: vec![("reply".to_owned(), "Répondre".to_owned())],
            default: true,
            urgency: Urgency::Normal,
            timeout: Timeout::After(LIFE),
            progress: None,
            resident: false,
            pid: 42,
            agent: false,
            conversation: None,
            icon_from: IconFrom {
                app_icon: "slack".to_owned(),
                image_path: None,
            },
            at,
            sound: None,
            transient: false,
            ancestry: Vec::new(),
        }
    }

    fn of(id: u32, app: &str, at: i64) -> Note {
        let mut n = note(id, at);
        n.app = app.to_owned();
        n.entry = None;
        n
    }

    const DAY: i64 = 24 * 3600 * 1000;

    fn pin(agent: &str, at: i64) -> NotePin {
        NotePin {
            agent: agent.to_owned(),
            reason: Some("À payer avant vendredi".to_owned()),
            at,
        }
    }

    #[test]
    fn the_newest_comes_first_and_a_replacement_keeps_its_number_and_pin() {
        let mut h = History::default();
        h.add(note(1, 1000), Sight::Showing, false);
        h.add(note(2, 2000), Sight::New, false);
        assert!(h.pin(1, pin("secretaire", 2500)).is_ok());
        let mut again = note(1, 3000);
        again.number = 1;
        h.add(again, Sight::Noticed, false);
        let ids: Vec<Option<u32>> = h.items().iter().map(|k| k.live).collect();
        assert_eq!(ids, [Some(1), Some(2)]);
        assert_eq!(h.items()[0].key, 1);
        // Its pin kept, and pinned it stays new.
        assert!(h.items()[0].pin.is_some());
        assert_eq!(h.items()[0].seen, Sight::New);
        assert_eq!(h.unseen(), 2);
        // Open, the badge says nothing; closed, the new are noticed, but
        // the pinned one.
        h.set_open(true);
        assert_eq!(h.unseen(), 0);
        h.set_open(false);
        assert_eq!(h.unseen(), 1);
        assert_eq!(h.items()[1].seen, Sight::Noticed);
        h.closed(2);
        assert_eq!(h.items()[1].live, None);
        assert!(h.remove_live(1));
        assert_eq!(h.items().len(), 1);
    }

    #[test]
    fn a_bubble_going_notices_or_counts_but_never_unfolds() {
        let mut h = History::default();
        h.add(note(1, 1000), Sight::Showing, false);
        h.add(note(2, 1000), Sight::Showing, false);
        // Its application opened while its bubble shows: seen at once.
        assert_eq!(h.user_saw(&[1], false), 1);
        h.bubble_left(1, Sight::New);
        assert_eq!(h.get(1).map(|k| k.seen), Some(Sight::Seen));
        // Gone after its time, unseen: new; then clicked: noticed.
        h.bubble_left(2, Sight::New);
        assert_eq!(h.unseen(), 1);
        h.bubble_left(2, Sight::Noticed);
        assert_eq!(h.get(2).map(|k| k.seen), Some(Sight::Noticed));
    }

    #[test]
    fn the_states_pins_tidying_and_undo() {
        let mut h = History::default();
        for id in 1..=5 {
            h.add(of(id, "Chat", 1000 + i64::from(id)), Sight::New, false);
        }
        let mut capture = of(6, "Capture d’écran", 2000);
        capture.number = 6;
        h.add(capture, Sight::Noticed, true);
        // Three pins at most, the fourth refused naming them.
        for key in 1..=3 {
            assert!(h.pin(key, pin("secretaire", key as i64)).is_ok());
        }
        let refused = h.pin(4, pin("secretaire", 9));
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.contains("1, 2, 3") || e.contains("3, 2, 1")),
            "{refused:?}"
        );
        assert!(h.pin(99, pin("secretaire", 9)).is_err());
        // Opening the application leaves the pinned ones pinned; the
        // desktop's own never folds.
        assert_eq!(h.user_saw(&[1, 4, 6], false), 1);
        assert_eq!(h.get(4).map(|k| k.seen), Some(Sight::Seen));
        assert_eq!(h.get(6).map(|k| k.seen), Some(Sight::Noticed));
        assert_eq!(h.pins(), 3);
        // Tidied: seen, unpinned; the desktop's own stays.
        let befores = h.tidy(&[2, 5, 6]);
        assert_eq!(befores.len(), 2);
        assert_eq!(h.pins(), 2);
        assert_eq!(h.get(5).map(|k| k.seen), Some(Sight::Seen));
        // The user clicks one of them since: it stays as they left it.
        h.user_saw(&[5], true);
        assert_eq!(h.undo(&befores), 1);
        assert!(
            h.get(2)
                .is_some_and(|k| k.pin.is_some() && k.seen == Sight::New)
        );
        assert_eq!(h.get(5).map(|k| k.seen), Some(Sight::Seen));
        // Unpinned: its state kept.
        assert!(h.unpin(3).is_ok());
        assert!(h.unpin(3).is_err());
        assert_eq!(h.get(3).map(|k| k.seen), Some(Sight::New));
    }

    #[test]
    fn past_two_hundred_the_oldest_seen_go_first_then_noticed_then_new() {
        let mut h = History::default();
        let mut number = 0;
        let mut add = |h: &mut History, seen: Sight| {
            number += 1;
            let mut n = note(number, i64::from(number));
            n.number = u64::from(number);
            h.add(n, seen, false);
        };
        add(&mut h, Sight::New);
        add(&mut h, Sight::Noticed);
        add(&mut h, Sight::Seen);
        for _ in 3..MOST {
            add(&mut h, Sight::New);
        }
        assert_eq!(h.items().len(), MOST);
        add(&mut h, Sight::New);
        assert!(h.get(3).is_none(), "the seen one goes first");
        add(&mut h, Sight::New);
        assert!(h.get(2).is_none(), "then the noticed one");
        add(&mut h, Sight::New);
        assert!(h.get(1).is_none(), "then the oldest new");
        assert_eq!(h.items().len(), MOST);
        assert_eq!(h.take_dropped(), [3, 2, 1]);
    }

    #[test]
    fn grouped_by_application_the_pins_first_then_open_then_seen_only() {
        let mut h = History::default();
        h.add(of(1, "Courriel", 100), Sight::Seen, false);
        h.add(of(2, "Mises à jour", 150), Sight::Seen, false);
        h.add(of(3, "Chat", 200), Sight::Seen, false);
        h.add(of(4, "Agenda", 300), Sight::Noticed, false);
        h.add(of(5, "Chat", 400), Sight::New, false);
        h.add(of(6, "Chat", 500), Sight::New, false);
        h.add(of(7, "Courriel", 450), Sight::New, false);
        assert!(h.pin(7, pin("secretaire", 600)).is_ok());
        let g = grouped(h.items());
        assert_eq!(g.pins.iter().map(|k| k.key).collect::<Vec<_>>(), [7]);
        let open: Vec<(&str, usize, usize)> = g
            .open
            .iter()
            .map(|g| (g.app.as_str(), g.open.len(), g.seen.len()))
            .collect();
        assert_eq!(open, [("chat", 2, 1), ("agenda", 1, 0)]);
        // The newest first within.
        assert_eq!(g.open[0].open[0].key, 6);
        let seen: Vec<&str> = g.seen_only.iter().map(|g| g.app.as_str()).collect();
        assert_eq!(seen, ["mises à jour", "courriel"]);
    }

    #[test]
    fn it_is_written_and_read_back_without_what_is_older_than_seven_days() {
        let now = 100 * DAY;
        let mut h = History::default();
        h.add(note(1, now - 8 * DAY), Sight::Noticed, false);
        // An agent's conversation (#823) kept: a click after a restart
        // opens it.
        let conversation = hocus_link::AgentConversation {
            tool: "claude-code".to_owned(),
            session: "3f6c2a1e".to_owned(),
            folder: "/home/alex/site".into(),
            transcript: "/home/alex/.claude/projects/-home-alex-site/3f6c2a1e.jsonl".into(),
            pid: 40,
        };
        let mut two = note(2, now - DAY);
        two.conversation = Some(conversation.clone());
        h.add(two, Sight::Seen, false);
        h.add(note(3, now - 1000), Sight::Showing, false);
        assert!(h.pin(3, pin("secretaire", now)).is_ok());
        let records: Vec<Record> = h.items().iter().map(Record::of).collect();
        let text = encode(&records);
        assert_eq!(text.lines().count(), 3);
        let (read, dropped) = decode(&format!("{text}not json\n"), now);
        assert_eq!(dropped, 1);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], records[0]);
        assert_eq!((read[0].id, read[0].state), (3, NoteState::New));
        assert!(read[0].pin.is_some());
        assert_eq!(read[1].state, NoteState::Seen);
        assert_eq!(read[1].body, "Bonjour\ttout le monde\n« ça va ? »");
        assert_eq!(read[1].conversation, Some(conversation));
        let mut again = History::default();
        let mut fresh = note(4, now);
        fresh.number = 4;
        again.add(fresh, Sight::Showing, false);
        again.restore(read.into_iter().map(Record::restored).collect());
        let ats: Vec<i64> = again.items().iter().map(|k| k.note.at).collect();
        assert_eq!(ats, [now, now - 1000, now - DAY]);
        assert_eq!(again.items()[1].live, None);
        assert!(again.items()[1].note.actions.is_empty());
        // Seven days on, the older goes.
        assert!(again.prune(now + 7 * DAY - DAY + 1));
        assert_eq!(again.items().len(), 2);
        // A file written before #923: no number, `unseen` new.
        let old = "{\"app\":\"Slack\",\"at\":5,\"unseen\":true}\n\
                   {\"app\":\"Slack\",\"at\":4,\"unseen\":false}\n";
        let (read, _) = decode(old, 10);
        let states: Vec<(u64, NoteState)> = read.iter().map(|r| (r.id, r.state)).collect();
        assert_eq!(states, [(0, NoteState::New), (0, NoteState::Noticed)]);
    }

    #[test]
    fn its_age_is_said_by_the_hour_then_by_the_days_of_the_calendar() {
        let now: Zoned = "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time");
        let at = |s: &str| -> i64 {
            s.parse::<Zoned>()
                .expect("a time")
                .timestamp()
                .as_millisecond()
        };
        let ms = now.timestamp().as_millisecond();
        assert_eq!(ago(ms - 9 * 60_000, &now), Ago::Minutes(9));
        assert_eq!(ago(ms - 20_000, &now), Ago::Now);
        let morning = at("2026-11-02T08:10:00+01:00[Europe/Paris]");
        assert_eq!(ago(morning, &now), Ago::Hours(6));
        let late = at("2026-11-01T23:50:00+01:00[Europe/Paris]");
        assert_eq!(ago(late, &now), Ago::Yesterday);
        let before = at("2026-10-30T12:00:00+01:00[Europe/Paris]");
        assert_eq!(ago(before, &now), Ago::Days(3));
    }
}
