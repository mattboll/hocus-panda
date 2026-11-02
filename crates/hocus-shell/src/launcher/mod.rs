//! The launcher (5.1, variant C): `Super+Space` opens it full screen on
//! the screen under the pointer, a field near the top and the
//! applications in a grid of eight columns under it; typing filters the
//! grid in place, the best match first and selected; `Enter`, or a click,
//! starts the selected one and closes it. `Escape`, `Super+Space` again,
//! or a click outside the field and the tiles close it.
//!
//! The field empty, the grid shows "FRÉQUENTES" (the eight most launched)
//! then "TOUTES LES APPLICATIONS" from A to Z. Hover selects a tile, the
//! arrows move the selection in two dimensions, the wheel scrolls by rows.
//!
//! Its surface is a layer surface of the shell's over the whole screen
//! ([`LAUNCHER_NAMESPACE`]), in the overlay layer, holding the keyboard.
//! The compositor darkens and blurs what is behind it and moves it in and
//! out; the shell draws it once for that motion, and again only for what
//! changes (a key, the pointer on another tile, an icon read). As for the
//! popovers, nothing goes through iced's messages, which would draw every
//! surface: the effects are layer-shell actions and lines to the
//! compositor, carried out by [`crate::popover::effects`].
//!
//! The applications ([`apps`]) and their icons ([`icons`]) are read on
//! threads of their own.

pub mod apps;
pub mod draw;
pub mod entries;
pub mod icons;
pub mod search;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{LAUNCHER_CLOSE, LAUNCHER_NAMESPACE, Launch, ToCompositor};
use iced::{keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use crate::{clock::time::Lang, popover::Effect};

/// The grid's columns, and the frequent applications shown at most.
pub const COLUMNS: usize = 8;
const FREQUENT: usize = 8;
/// The characters the field keeps at most.
const QUERY_MOST: usize = 200;

/// The launcher's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn placeholder(self) -> &'static str {
        self.pick("Chercher une application", "Search for an application")
    }

    pub fn escape(self) -> &'static str {
        self.pick("Échap", "Esc")
    }

    pub fn frequent(self) -> &'static str {
        self.pick("FRÉQUENTES", "FREQUENT")
    }

    pub fn all(self) -> &'static str {
        self.pick("TOUTES LES APPLICATIONS", "ALL APPLICATIONS")
    }

    pub fn none_for(self, query: &str) -> String {
        if self.fr {
            format!("Aucune application pour «\u{a0}{query}\u{a0}».")
        } else {
            format!("No application for \u{201c}{query}\u{201d}.")
        }
    }
}

/// A row of the grid: a heading, or up to [`COLUMNS`] tiles (their places
/// in [`Grid::flat`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Head(&'static str),
    Tiles(Vec<usize>),
}

/// What the grid shows: its rows, and each tile's application (its index
/// in the catalog), in the selection's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub flat: Vec<usize>,
}

impl Grid {
    /// The grid for `query` over `catalog`, launched `counts` times each.
    pub fn of(
        catalog: &apps::Catalog,
        counts: &HashMap<String, u32>,
        query: &str,
        words: Words,
    ) -> Self {
        let count = |i: usize| {
            catalog
                .apps
                .get(i)
                .and_then(|a| counts.get(&a.entry.id))
                .copied()
                .unwrap_or(0)
        };
        let mut grid = Self::default();
        if query.trim().is_empty() {
            let mut frequent: Vec<usize> =
                (0..catalog.apps.len()).filter(|i| count(*i) > 0).collect();
            // The most launched first; the catalog is A to Z for ties.
            frequent.sort_by_key(|i| std::cmp::Reverse(count(*i)));
            frequent.truncate(FREQUENT);
            if !frequent.is_empty() {
                grid.rows.push(Row::Head(words.frequent()));
                grid.chunk(&frequent);
            }
            if !catalog.apps.is_empty() {
                grid.rows.push(Row::Head(words.all()));
                let all: Vec<usize> = (0..catalog.apps.len()).collect();
                grid.chunk(&all);
            }
        } else {
            let apps: Vec<(&search::Folded, u32)> = catalog
                .apps
                .iter()
                .enumerate()
                .map(|(i, a)| (&a.folded, count(i)))
                .collect();
            let found = search::rank(&apps, query);
            grid.chunk(&found);
        }
        grid
    }

    fn chunk(&mut self, apps: &[usize]) {
        for row in apps.chunks(COLUMNS) {
            let start = self.flat.len();
            self.flat.extend_from_slice(row);
            self.rows
                .push(Row::Tiles((start..self.flat.len()).collect()));
        }
    }

    /// The row holding tile `at`, and its column.
    pub fn place(&self, at: usize) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().find_map(|(r, row)| match row {
            Row::Tiles(tiles) => tiles.iter().position(|t| *t == at).map(|c| (r, c)),
            Row::Head(_) => None,
        })
    }

    /// The tile the selection goes to from `at` by an arrow: across, by one
    /// in the selection's order; up or down, the same column of the next
    /// row of tiles (its last when it is shorter).
    pub fn step(&self, at: usize, arrow: Arrow) -> usize {
        let n = self.flat.len();
        if n == 0 {
            return 0;
        }
        match arrow {
            Arrow::Left => at.saturating_sub(1),
            Arrow::Right => (at + 1).min(n - 1),
            Arrow::Up | Arrow::Down => {
                let Some((row, column)) = self.place(at) else {
                    return at;
                };
                let rows: Box<dyn Iterator<Item = usize>> = if arrow == Arrow::Up {
                    Box::new((0..row).rev())
                } else {
                    Box::new(row + 1..self.rows.len())
                };
                rows.into_iter()
                    .find_map(|r| match &self.rows[r] {
                        Row::Tiles(tiles) => tiles
                            .get(column.min(tiles.len().saturating_sub(1)))
                            .copied(),
                        Row::Head(_) => None,
                    })
                    .unwrap_or(at)
            }
        }
    }
}

/// An arrow key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// The launcher open.
#[derive(Debug)]
pub struct Open {
    pub surface: window::Id,
    pub scale: f32,
    pub query: String,
    pub grid: Grid,
    pub catalog: Arc<apps::Catalog>,
    counts: HashMap<String, u32>,
    /// The tile selected, in [`Grid::flat`].
    pub selected: usize,
    /// The first row shown.
    pub offset: usize,
    /// The tile pressed, and the one under the pointer since when.
    pub pressed: Option<usize>,
    pub hover: Option<(usize, Instant)>,
    /// Where the pointer is on the surface, once it moved.
    pub pointer: Option<iced::Point>,
    /// The surface's size, once drawn: what the grid's rows fit in.
    pub size: iced::Size,
    /// When a cut name rested on shows whole.
    pub tooltip: Option<Instant>,
}

impl Open {
    fn regrid(&mut self, words: Words) {
        self.grid = Grid::of(&self.catalog, &self.counts, &self.query, words);
        self.selected = 0;
        self.offset = 0;
        self.hover = None;
        self.tooltip = None;
    }

    /// The application of tile `at`.
    pub fn app(&self, at: usize) -> Option<&apps::App> {
        self.catalog.apps.get(*self.grid.flat.get(at)?)
    }
}

#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Closed,
    Open(Box<Open>),
}

/// The launcher, and its surfaces fading out until they go.
#[derive(Debug, Default)]
pub struct Launcher {
    pub phase: Phase,
    closing: Vec<(window::Id, Instant)>,
}

static LAUNCHER: Mutex<Option<Launcher>> = Mutex::new(None);

/// `with` on the launcher.
pub fn with<T>(with: impl FnOnce(&mut Launcher) -> T) -> T {
    let mut launcher = LAUNCHER.lock().unwrap_or_else(PoisonError::into_inner);
    with(launcher.get_or_insert_with(Launcher::default))
}

fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Reads the applications.
pub fn start() {
    apps::start();
}

/// Its surface draws again.
pub fn wake() {
    crate::popover::send(vec![Effect::Wake(LAUNCHER_NAMESPACE)]);
}

/// The applications changed: an open launcher shows them.
pub fn changed() {
    let open = with(|l| match &mut l.phase {
        Phase::Open(open) => {
            open.catalog = apps::catalog();
            open.regrid(words());
            Some((icon_names(open), open.scale))
        }
        Phase::Closed => None,
    });
    if let Some((names, scale)) = open {
        icons::want(names.iter().map(String::as_str), draw::ICON as u32, scale);
        wake();
    }
}

/// The icons of the grid's applications, in its order.
fn icon_names(open: &Open) -> Vec<String> {
    open.grid
        .flat
        .iter()
        .filter_map(|i| open.catalog.apps.get(*i)?.entry.icon.clone())
        .collect()
}

/// `Super+Space` on `output` (its scale in 120ths): it opens there, or
/// closes.
pub fn toggle(output: String, scale: u32) {
    let now = Instant::now();
    let (effects, wanted) = with(|l| {
        let mut effects = Vec::new();
        if matches!(l.phase, Phase::Open(_)) {
            l.close(now, &mut effects);
            return (effects, None);
        }
        let surface = window::Id::unique();
        let catalog = apps::catalog();
        let counts = apps::counts();
        let scale = (scale as f32 / 120.0).max(0.5);
        let mut open = Open {
            surface,
            scale,
            query: String::new(),
            grid: Grid::default(),
            catalog,
            counts,
            selected: 0,
            offset: 0,
            pressed: None,
            hover: None,
            pointer: None,
            size: iced::Size::ZERO,
            tooltip: None,
        };
        open.regrid(words());
        let wanted = (icon_names(&open), scale);
        tracing::debug!(output, apps = open.catalog.apps.len(), "the launcher opens");
        l.phase = Phase::Open(Box::new(open));
        effects.push(Effect::Launcher {
            id: surface,
            output,
        });
        (effects, Some(wanted))
    });
    if let Some((names, scale)) = wanted {
        // One popover at a time, and not under the launcher.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        icons::want(names.iter().map(String::as_str), draw::ICON as u32, scale);
    }
    crate::popover::send(effects);
}

/// A press on none of the shell's surfaces (another screen's): it closes.
pub fn outside() {
    close();
}

/// It closes.
pub fn close() {
    let effects = with(|l| {
        let mut effects = Vec::new();
        l.close(Instant::now(), &mut effects);
        effects
    });
    if !effects.is_empty() {
        crate::popover::send(effects);
    }
}

impl Launcher {
    /// It closes at `now`: the keyboard goes back at once, and the
    /// compositor fades it out before it goes.
    fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        let Phase::Open(open) = std::mem::take(&mut self.phase) else {
            return;
        };
        tracing::debug!("the launcher closes");
        self.closing.push((open.surface, now + LAUNCHER_CLOSE));
        effects.push(Effect::Keyboard(open.surface, KeyboardInteractivity::None));
        effects.push(Effect::Tell(ToCompositor::Closing(
            LAUNCHER_NAMESPACE.to_owned(),
        )));
    }

    /// Whether surface `id` is the launcher's, open.
    pub fn open_on(&self, id: window::Id) -> Option<&Open> {
        match &self.phase {
            Phase::Open(open) if open.surface == id => Some(open),
            _ => None,
        }
    }

    pub fn open_mut(&mut self, id: window::Id) -> Option<&mut Open> {
        match &mut self.phase {
            Phase::Open(open) if open.surface == id => Some(open),
            _ => None,
        }
    }
}

/// Whether surface `id` is the launcher's, open or fading out.
pub fn is_surface(id: window::Id) -> bool {
    with(|l| l.open_on(id).is_some() || l.closing.iter().any(|(c, _)| *c == id))
}

/// When [`tick`] has something to do.
pub fn deadline() -> Option<Instant> {
    with(|l| {
        let tooltip = match &l.phase {
            Phase::Open(open) => open.tooltip,
            Phase::Closed => None,
        };
        l.closing.iter().map(|(_, at)| *at).chain(tooltip).min()
    })
}

/// What is due at `now`: the surfaces faded out go.
pub fn tick(now: Instant) -> Vec<Effect> {
    with(|l| {
        let mut effects = Vec::new();
        // A cut name rested on long enough: it shows whole.
        if let Phase::Open(open) = &mut l.phase
            && open.tooltip.is_some_and(|at| at <= now)
        {
            open.tooltip = None;
            effects.push(Effect::Wake(LAUNCHER_NAMESPACE));
        }
        l.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        effects
    })
}

/// Tile `at` of the open launcher on `id` starts, and the launcher closes.
pub fn launch(id: window::Id, at: usize) {
    let effects = with(|l| {
        let mut effects = Vec::new();
        let Some(open) = l.open_on(id) else {
            return effects;
        };
        let Some(app) = open.app(at) else {
            return effects;
        };
        let entry = &app.entry;
        let launch = Launch {
            id: entry.id.clone(),
            terminal: entry.terminal,
            dir: entry.dir.clone().unwrap_or_default(),
            argv: entry.argv.clone(),
        };
        if launch.fits() {
            tracing::debug!(app = entry.id, "the launcher launches");
            apps::launched(&entry.id);
            effects.push(Effect::Tell(ToCompositor::Launch(launch)));
        } else {
            tracing::warn!(app = entry.id, "a command that cannot be sent: not started");
        }
        l.close(Instant::now(), &mut effects);
        effects
    });
    crate::popover::send(effects);
}

/// A key on the open launcher on `id`: what it does. Whether it was its.
pub fn key_on(
    id: window::Id,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> bool {
    use keyboard::{Key, key::Named};
    enum Then {
        Nothing,
        Close,
        Launch(usize),
        Draw,
    }
    let words = words();
    let then = with(|l| {
        let open = l.open_mut(id)?;
        let arrow = |arrow: Arrow, open: &mut Open| {
            open.selected = open.grid.step(open.selected, arrow);
            open.offset = draw::keep_in_view(&open.grid, open.selected, open.offset, open.size);
            Then::Draw
        };
        Some(match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) => {
                if open.grid.flat.is_empty() {
                    Then::Nothing
                } else {
                    Then::Launch(open.selected)
                }
            }
            Key::Named(Named::ArrowLeft) => arrow(Arrow::Left, open),
            Key::Named(Named::ArrowRight) => arrow(Arrow::Right, open),
            Key::Named(Named::ArrowUp) => arrow(Arrow::Up, open),
            Key::Named(Named::ArrowDown) => arrow(Arrow::Down, open),
            Key::Named(Named::Backspace) => {
                if open.query.is_empty() {
                    Then::Nothing
                } else {
                    if modifiers.control() {
                        // The last word, and the spaces after it.
                        let kept = open.query.trim_end().rfind(' ').map_or(0, |at| at + 1);
                        open.query.truncate(kept);
                    } else {
                        open.query.pop();
                    }
                    open.regrid(words);
                    Then::Draw
                }
            }
            _ => match text {
                Some(typed)
                    if !modifiers.control()
                        && !modifiers.alt()
                        && !modifiers.logo()
                        && !typed.is_empty()
                        && !typed.chars().any(char::is_control) =>
                {
                    if open.query.chars().count() + typed.chars().count() > QUERY_MOST {
                        return Some(Then::Nothing);
                    }
                    open.query.push_str(typed);
                    open.regrid(words);
                    Then::Draw
                }
                _ => Then::Nothing,
            },
        })
    });
    match then {
        None => false,
        Some(Then::Nothing) => true,
        Some(Then::Close) => {
            close();
            true
        }
        Some(Then::Launch(at)) => {
            launch(id, at);
            true
        }
        Some(Then::Draw) => true,
    }
}

/// A key that reached another of the shell's surfaces while the launcher
/// is open (layershellev hands keys to the surface last created or
/// clicked): its, and its surface draws again. Whether it was taken.
pub fn key(key: &keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> bool {
    let Some(id) = with(|l| match &l.phase {
        Phase::Open(open) => Some(open.surface),
        Phase::Closed => None,
    }) else {
        return false;
    };
    let taken = key_on(id, key, modifiers, text);
    if taken {
        wake();
    }
    taken
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(names: &[&str]) -> apps::Catalog {
        let entries = names
            .iter()
            .map(|name| entries::Entry {
                id: name.to_lowercase(),
                name: (*name).to_owned(),
                generic: None,
                keywords: Vec::new(),
                icon: None,
                argv: vec![name.to_lowercase()],
                terminal: false,
                dir: None,
            })
            .map(|entry| apps::App {
                folded: search::Folded::of(&entry.name, None, &[]),
                entry,
            })
            .collect();
        apps::Catalog { apps: entries }
    }

    #[test]
    fn the_empty_field_shows_the_frequent_then_all_from_a_to_z() {
        let names: Vec<String> = (0..12).map(|i| format!("App {i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let catalog = catalog(&names);
        let words = Words { fr: true };
        // Nothing launched yet: no frequent row.
        let grid = Grid::of(&catalog, &HashMap::new(), "", words);
        assert_eq!(grid.rows[0], Row::Head("TOUTES LES APPLICATIONS"));
        assert_eq!(grid.rows.len(), 3);
        let counts: HashMap<String, u32> = [("app 05", 3), ("app 01", 7), ("app 09", 3)]
            .into_iter()
            .map(|(id, n)| (id.to_owned(), n))
            .collect();
        let grid = Grid::of(&catalog, &counts, "", words);
        assert_eq!(grid.rows[0], Row::Head("FRÉQUENTES"));
        // The most launched first, ties from A to Z.
        assert_eq!(&grid.flat[..3], &[1, 5, 9]);
        assert_eq!(grid.rows[2], Row::Head("TOUTES LES APPLICATIONS"));
        assert_eq!(grid.rows.len(), 5);
        assert_eq!(grid.flat.len(), 3 + 12);
    }

    #[test]
    fn the_arrows_go_across_and_down_the_columns() {
        let names: Vec<String> = (0..20).map(|i| format!("App {i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let catalog = catalog(&names);
        let grid = Grid::of(&catalog, &HashMap::new(), "", Words { fr: false });
        // Rows of 8, 8 and 4 under the heading.
        assert_eq!(grid.step(0, Arrow::Left), 0);
        assert_eq!(grid.step(7, Arrow::Right), 8);
        assert_eq!(grid.step(3, Arrow::Down), 11);
        assert_eq!(grid.step(14, Arrow::Down), 19);
        assert_eq!(grid.step(19, Arrow::Down), 19);
        assert_eq!(grid.step(11, Arrow::Up), 3);
        assert_eq!(grid.step(3, Arrow::Up), 3);
        assert_eq!(grid.step(19, Arrow::Right), 19);
    }

    #[test]
    fn typing_filters_the_grid_best_first() {
        let catalog = catalog(&["Firefox", "Fichiers", "kitty"]);
        let grid = Grid::of(&catalog, &HashMap::new(), "fire", Words { fr: true });
        assert_eq!(grid.rows, [Row::Tiles(vec![0])]);
        assert_eq!(grid.flat, [0]);
        let none = Grid::of(&catalog, &HashMap::new(), "zzz", Words { fr: true });
        assert!(none.rows.is_empty());
        assert_eq!(
            Words { fr: true }.none_for("zzz"),
            "Aucune application pour «\u{a0}zzz\u{a0}»."
        );
    }
}
