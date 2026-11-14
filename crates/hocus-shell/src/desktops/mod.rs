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
//!
//! With several screens (8.4), a bar on the primary screen alone shows a
//! row of tiles per screen, its own first: the compositor says which
//! screens a bar shows (`desk-screens`), with their sizes, and each one's
//! desktops on their own lines, kept by screen. A tile is then a screen's
//! (by its row, from 0) and a desktop's.

pub mod draw;
pub mod icons;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{DESKTOPS_NAMESPACE, DeskScreen, Desktops, Look, Recent, Titles};

use crate::popover::{self, Item};

static DESKTOPS: Mutex<Option<HashMap<String, Desktops>>> = Mutex::new(None);
static TITLES: Mutex<Option<HashMap<String, Titles>>> = Mutex::new(None);
static RECENT: Mutex<Option<HashMap<String, Recent>>> = Mutex::new(None);
/// The screens each bar shows a row of tiles for, by the bar's screen.
static SCREENS: Mutex<Option<HashMap<String, Vec<DeskScreen>>>> = Mutex::new(None);
/// The tile a window being moved is over (8.5 B): the bar's screen, the
/// tile's screen and desktop.
static DROP: Mutex<Option<(String, String, u8)>> = Mutex::new(None);

/// The compositor says a window being moved is over bar `bar`'s tile
/// `over` (its screen and desktop), or over none: that tile lights.
pub fn set_drop(bar: String, over: Option<(String, u8)>) {
    let now = over.map(|(output, desktop)| (bar.clone(), output, desktop));
    let mut kept = DROP.lock().unwrap_or_else(PoisonError::into_inner);
    let changed = match (&*kept, &now) {
        (None, None) => false,
        (Some((was, ..)), None) => *was == bar,
        _ => *kept != now,
    };
    if !changed {
        return;
    }
    *kept = now;
    drop(kept);
    tracing::debug!(bar, over = ?drop_on(&bar), "a window over the tiles");
    popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
}

/// The tile of bar `bar` a window being moved is over: its screen and
/// desktop.
pub fn drop_on(bar: &str) -> Option<(String, u8)> {
    DROP.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(on, ..)| on == bar)
        .map(|(_, output, desktop)| (output.clone(), *desktop))
}

/// The screens bar `bar` shows the desktops of, in its rows' order: its
/// own first. Until the compositor says, its own alone, of no known size.
pub fn screens(bar: &str) -> Vec<DeskScreen> {
    SCREENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|s| s.get(bar).cloned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            vec![DeskScreen {
                output: bar.to_owned(),
                width: 0,
                height: 0,
                built_in: false,
                inches: 0,
                number: 0,
            }]
        })
}

/// The screen of bar `bar`'s row `row` (from 0: its own), if there is one.
pub fn screen_of(bar: &str, row: u8) -> Option<DeskScreen> {
    screens(bar).into_iter().nth(usize::from(row))
}

/// The screen of bar `bar`'s row `row`, by name: the bar's own when there
/// is no such row.
pub fn output_of(bar: &str, row: u8) -> String {
    screen_of(bar, row).map_or_else(|| bar.to_owned(), |s| s.output)
}

/// The compositor says which screens bar `bar` shows: kept, its surface
/// resized to their rows and drawn again.
pub fn set_screens(bar: String, said: Vec<DeskScreen>) {
    SCREENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(bar.clone(), said);
    draw::rows_changed(&bar);
    popover::send(vec![popover::Effect::Wake(DESKTOPS_NAMESPACE)]);
    list_changed(Some(&bar));
}

/// The bars that show screen `output`'s desktops.
fn bars_showing(output: &str) -> Vec<String> {
    let screens = SCREENS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut bars: Vec<String> = screens
        .as_ref()
        .map(|s| {
            s.iter()
                .filter(|(_, rows)| rows.iter().any(|r| r.output == output))
                .map(|(bar, _)| bar.clone())
                .collect()
        })
        .unwrap_or_default();
    if !bars.iter().any(|b| b == output) {
        bars.push(output.to_owned());
    }
    bars
}
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

/// Desktop `index` (from 0) of screen `output`, if it is a project
/// (docs/design/projects.md): its name and folder.
pub fn project(output: &str, index: usize) -> Option<hocus_link::DeskProject> {
    snapshot(output)?.project(index).cloned()
}

/// A project's folder as it is shown: from `~` under the home.
pub fn folder_shown(folder: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    hocus_link::projects::tilde(folder, &home)
}

/// Desktop `index`'s name on screen `output`: its project's while it is
/// one (docs/design/projects.md §1), else its own (6.14).
pub fn name_on(output: &str, index: usize) -> Option<String> {
    project(output, index)
        .map(|p| p.name)
        .or_else(|| name(index))
}

/// Desktop `index` of screen `output` as screen readers title it: "Bureau
/// 2, projet hocus-site" for a project, else its name or "Bureau N".
pub fn said_title(words: crate::levels::model::Words, output: &str, index: usize) -> String {
    match project(output, index) {
        Some(project) => words.project_title(index + 1, &project.name),
        None => words.desktop_title(index + 1, name(index).as_deref()),
    }
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
    // Its rows, and whether the full form keeps room for a project's line
    // over them (docs/design/projects.md §3).
    let shape = (desktops.count, desktops.projects.is_empty());
    icons::want(desktops.windows.iter().map(|w| w.app.as_str()));
    let mut before = None;
    listed(&output, || {
        before = DESKTOPS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(output.clone(), desktops)
            .map(|d| (d.count, d.projects.is_empty()));
    });
    if before != Some(shape) {
        for bar in bars_showing(&output) {
            draw::rows_changed(&bar);
        }
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

/// The screen and the desktop of the list that shows, and its bar.
fn list_open() -> Option<(String, String, usize)> {
    popover::with(|c| match c.open_on() {
        Some((on, Item::Desktop { row, index })) => {
            Some((on.to_owned(), output_of(on, row), usize::from(index)))
        }
        // "+ N" shows the current desktop's list (#604).
        Some((on, Item::WindowsMore)) => Some((on.to_owned(), on.to_owned(), current(on))),
        _ => None,
    })
}

/// `change` on screen `output`'s desktops: the list draws again if it
/// shows one of them and its rows changed.
fn listed(output: &str, change: impl FnOnce()) {
    let open = list_open().filter(|(_, screen, _)| screen == output);
    let before = open.as_ref().map(|(_, screen, index)| rows(screen, *index));
    change();
    if let Some((bar, screen, index)) = open
        && before.as_ref() != Some(&rows(&screen, index))
    {
        list_changed(Some(&bar));
    }
}

/// What a desktop's list shows changed (on bar `bar`, or on any): if it
/// shows, it draws again, to its new size.
pub fn list_changed(bar: Option<&str>) {
    let effects = popover::with(|c| match c.open_on() {
        Some((on, Item::Desktop { .. } | Item::WindowsMore)) if bar.is_none_or(|b| b == on) => {
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

/// Desktop `index`'s windows on screen `output`, front to back, then its
/// minimised ones, with their titles (their application's id until the
/// compositor says one).
pub fn rows(output: &str, index: usize) -> Vec<Row> {
    let Some(desktops) = snapshot(output) else {
        return Vec::new();
    };
    let titles = TITLES.lock().unwrap_or_else(PoisonError::into_inner);
    let titles = titles.as_ref().and_then(|t| t.get(output));
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
    fn a_bars_rows_are_its_screens_its_own_first() {
        // Until the compositor says, its own screen alone.
        let own = screens("TEST-0");
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].output, "TEST-0");
        let screen = |output: &str| DeskScreen {
            output: output.into(),
            width: 1920,
            height: 1080,
            built_in: false,
            inches: 0,
            number: 1,
        };
        SCREENS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert("TEST-1".into(), vec![screen("TEST-1"), screen("TEST-2")]);
        assert_eq!(output_of("TEST-1", 0), "TEST-1");
        assert_eq!(output_of("TEST-1", 1), "TEST-2");
        // A row no longer there: the bar's own.
        assert_eq!(output_of("TEST-1", 4), "TEST-1");
        assert_eq!(bars_showing("TEST-2"), ["TEST-1", "TEST-2"]);
    }

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
            projects: Vec::new(),
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
