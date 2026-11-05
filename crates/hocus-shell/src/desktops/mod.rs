//! The desktops in the bar (bar.md §4 group 8, §5; 3.9, 3.9b): a tile per
//! desktop of the bar's screen with a sketch of its windows and their
//! applications' icons in the full form, a dot per desktop in the mini
//! form; a click goes to that desktop, the wheel to the next or the
//! previous one, and resting on one shows the list of its windows.
//!
//! The compositor owns the desktops: it says, for each bar's screen, how
//! many there are, which one shows, and each one's windows as rectangles
//! in shares of the screen with their application's id, as the shell
//! connects and when they change (at most once a frame); and their titles
//! on a line of their own. The link keeps them here without a message and
//! wakes the desktops' surface, nothing else (a change of their number
//! resizes it, [`draw`]); the titles wake only an open list
//! ([`crate::popover::desktop`]), never the tiles.

pub mod draw;
pub mod icons;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{DESKTOPS_NAMESPACE, Desktops, Look, Recent, Titles};

use crate::popover::{self, Item};

static DESKTOPS: Mutex<Option<HashMap<String, Desktops>>> = Mutex::new(None);
static TITLES: Mutex<Option<HashMap<String, Titles>>> = Mutex::new(None);
static RECENT: Mutex<Option<HashMap<String, Recent>>> = Mutex::new(None);
/// The desktops' names, by position, the same on every screen (6.14).
static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Desktop `index`'s name (from 0), if it has one: else the words'
/// "Bureau N".
pub fn name(index: usize) -> Option<String> {
    NAMES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(index)
        .filter(|n| !n.is_empty())
        .cloned()
}

/// The compositor says the desktops' names (`[desktops] names`): kept;
/// the tiles say them again, and an open list's title shows it.
pub fn set_names(names: Vec<String>) {
    let mut kept = NAMES.lock().unwrap_or_else(PoisonError::into_inner);
    if *kept == names {
        return;
    }
    *kept = names;
    drop(kept);
    popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
    list_changed(None);
}

/// Bar `bar`'s screen's desktops as last said.
pub fn snapshot(bar: &str) -> Option<Desktops> {
    DESKTOPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(bar)
        .cloned()
}

/// Bar `bar`'s screen's windows' titles as last said.
pub fn titles(bar: &str) -> Option<Titles> {
    TITLES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(bar)
        .cloned()
}

/// Bar `bar`'s screen's windows, the most recently focused first, as last
/// said (#604).
pub fn recent(bar: &str) -> Option<Recent> {
    RECENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(bar)
        .cloned()
}

/// The title of a window of the application that goes by one of `ids`
/// (lower case): on any screen, the focused one first. Its application's
/// id matches whole or by its last part (`org.mozilla.firefox`,
/// `firefox`).
pub fn title_of_app(ids: &[String]) -> Option<String> {
    let desktops = DESKTOPS.lock().unwrap_or_else(PoisonError::into_inner);
    let titles = TITLES.lock().unwrap_or_else(PoisonError::into_inner);
    let (desktops, titles) = (desktops.as_ref()?, titles.as_ref()?);
    let matches = |app: &str| {
        let app = app.to_lowercase();
        let last = app.rsplit('.').next().unwrap_or(&app).to_owned();
        !app.is_empty() && ids.iter().any(|id| *id == app || *id == last)
    };
    let mut windows: Vec<(&String, &hocus_link::DeskWindow)> = desktops
        .iter()
        .flat_map(|(output, d)| d.windows.iter().map(move |w| (output, w)))
        .filter(|(_, w)| matches(&w.app))
        .collect();
    windows.sort_by_key(|(_, w)| w.look != Look::Focused);
    windows.into_iter().find_map(|(output, w)| {
        let (_, title) = titles
            .get(output)?
            .titles
            .iter()
            .find(|(id, _)| *id == w.id)?;
        Some(title.clone()).filter(|t| !t.is_empty())
    })
}

/// The compositor says `desktops`: kept, their icons looked for, the
/// desktops' surfaces woken (resized when their number changes the rows),
/// and the list drawn again if it shows on that bar.
pub fn set(desktops: Desktops) {
    let output = desktops.output.clone();
    let count = desktops.count;
    icons::want(desktops.windows.iter().map(|w| w.app.as_str()));
    let mut before = None;
    listed(&output, || {
        before = DESKTOPS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(output.clone(), desktops)
            .map(|d| d.count);
    });
    if before != Some(count) {
        draw::count(&output, usize::from(count));
    }
    popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
    // The windows over the tiles, if what they list changed (#604).
    crate::windows::changed();
}

/// The compositor says the titles of a screen's windows: kept; only the
/// list draws for them, if it shows that screen's.
pub fn set_titles(titles: Titles) {
    let output = titles.output.clone();
    listed(&output, || {
        TITLES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(output.clone(), titles);
    });
    crate::windows::changed();
}

/// The compositor says a screen's windows' order (#604): kept; only the
/// windows over the tiles draw for it, if their lines changed.
pub fn set_recent(recent: Recent) {
    RECENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(recent.output.clone(), recent);
    crate::windows::changed();
}

/// `change` on bar `bar`'s desktops: the list draws again if it shows one
/// of them and its rows changed.
fn listed(bar: &str, change: impl FnOnce()) {
    let open = popover::with(|c| match c.open_on() {
        Some((on, item @ (Item::Desktop(_) | Item::WindowsMore))) if on == bar => Some(item),
        _ => None,
    });
    let open = open.map(|item| match item {
        Item::Desktop(index) => usize::from(index),
        // "+ N" shows the current desktop's list (#604).
        _ => current(bar),
    });
    let before = open.map(|index| rows(bar, index));
    change();
    if let Some(index) = open
        && before.as_ref() != Some(&rows(bar, index))
    {
        list_changed(Some(bar));
    }
}

/// What a desktop's list shows changed (on bar `bar`, or on any): if it
/// shows, it draws again, to its new size.
pub fn list_changed(bar: Option<&str>) {
    let effects = popover::with(|c| match c.open_on() {
        Some((on, Item::Desktop(_) | Item::WindowsMore)) if bar.is_none_or(|b| b == on) => {
            let mut effects = c.refit();
            effects.push(popover::Effect::Wake(hocus_link::POPOVER_NAMESPACE));
            effects
        }
        _ => Vec::new(),
    });
    if !effects.is_empty() {
        popover::send(effects);
    }
}

/// The desktop bar `bar`'s screen shows, from 0.
pub fn current(bar: &str) -> usize {
    snapshot(bar).map_or(0, |d| usize::from(d.current))
}

/// A window of a desktop's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: u64,
    pub app: String,
    pub title: String,
    /// Minimised: greyed, after the others.
    pub minimized: bool,
}

/// Desktop `index`'s windows on bar `bar`'s screen, front to back, then
/// its minimised ones, with their titles (their application's id until
/// the compositor says one).
pub fn rows(bar: &str, index: usize) -> Vec<Row> {
    let Some(desktops) = snapshot(bar) else {
        return Vec::new();
    };
    let titles = TITLES.lock().unwrap_or_else(PoisonError::into_inner);
    let titles = titles.as_ref().and_then(|t| t.get(bar));
    rows_of(&desktops, titles, index)
}

fn rows_of(desktops: &Desktops, titles: Option<&Titles>, index: usize) -> Vec<Row> {
    let mut rows: Vec<Row> = desktops
        .windows
        .iter()
        .rev()
        .filter(|w| usize::from(w.desktop) == index)
        .map(|w| {
            let title = titles
                .and_then(|t| t.titles.iter().find(|(id, _)| *id == w.id))
                .map(|(_, title)| title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| w.app.clone());
            Row {
                id: w.id,
                app: w.app.clone(),
                title,
                minimized: w.look == Look::Minimized,
            }
        })
        .collect();
    // Stable: each part keeps its order, front to back.
    rows.sort_by_key(|r| r.minimized);
    rows
}

#[cfg(test)]
mod tests {
    use hocus_link::DeskWindow;

    use super::*;

    #[test]
    fn a_desktops_list_is_its_windows_front_to_back_with_their_titles() {
        let window = |desktop, id, app: &str| DeskWindow {
            desktop,
            rect: [0, 0, 500, 500],
            look: Look::Plain,
            id,
            app: app.into(),
        };
        let desktops = Desktops {
            output: "eDP-1".into(),
            count: 4,
            current: 0,
            windows: vec![
                window(0, 1, "foot"),
                window(1, 2, "firefox"),
                window(0, 3, "org.gnome.Nautilus"),
            ],
        };
        let titles = Titles {
            output: "eDP-1".into(),
            titles: vec![(1, "~/hocus — fish".into()), (3, String::new())],
        };
        let rows = rows_of(&desktops, Some(&titles), 0);
        // The one on top first; without a title, its application's id.
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["org.gnome.Nautilus", "~/hocus — fish"]
        );
        assert_eq!(rows[1].id, 1);
        assert!(rows_of(&desktops, None, 2).is_empty());
        // Minimised ones after the others, front to back too.
        let mut desktops = desktops;
        let minimized = |id, app| DeskWindow {
            look: Look::Minimized,
            ..window(0, id, app)
        };
        desktops.windows.insert(0, minimized(4, "a"));
        desktops.windows.push(minimized(5, "b"));
        let rows = rows_of(&desktops, None, 0);
        assert_eq!(
            rows.iter().map(|r| (r.id, r.minimized)).collect::<Vec<_>>(),
            [(3, false), (1, false), (5, true), (4, true)]
        );
    }
}
