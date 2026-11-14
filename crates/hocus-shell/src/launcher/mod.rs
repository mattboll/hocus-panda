//! The launcher (5.1, variant C): `Super+Space` opens it full screen on
//! the screen under the pointer, a field near the top and the
//! applications in a grid of eight columns under it (fewer on a screen too
//! narrow for them, #518); typing filters the
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
//! changes (a key, the pointer on another tile, an icon read). Its surface
//! is the widget kit's ([`view`]): what its widgets ask is handled at once
//! and it alone draws again; the effects are layer-shell actions and lines
//! to the compositor, carried out by [`crate::popover::effects`].
//!
//! The applications ([`apps`]) and their icons ([`icons`]) are read on
//! threads of their own.
//!
//! The field also answers (5.2, variant A): a calculation or a conversion
//! shows in a card under the field, and an address in a card of its own
//! ([`answers`]); then no application is listed. The web search is
//! always the last row once something is typed. `Enter` on a result
//! copies it and a toast says so ([`toast`]); on an address or the web
//! row, it opens the browser; `Ctrl+Enter` searches the web whatever is
//! selected.
//!
//! A query starting with `:` (5.7, `:chat`) shows a row of emoji first, up
//! to eight, found as the emoji picker finds them ([`crate::emoji`]):
//! `Enter` inserts the selected one, the first at first, into the window
//! that had the keyboard before the launcher opened.
//!
//! Projects (docs/design/projects.md §2.2, board 2): `projet` or `project`
//! then a space lists only "PROJETS" (those whose name holds the rest, the
//! open ones first, then the closed, newest closed first) and "DÉPÔTS GIT
//! RÉCENTS" (three at most, found on a thread, none a project's folder),
//! no application nor web row; nothing found says so. A project is found
//! by its name without the word too (§13, I), its row before the
//! applications. `Enter` on an open project goes to its desktop, on a
//! closed one reopens it, on a repository makes it a project; `→` on a
//! closed one shows its card. Nine desktops taken, the compositor says
//! so: the launcher opens again on the open projects, `Enter` asking to
//! close one (§13, H).

pub mod answers;
pub mod apps;
pub mod entries;
pub mod icons;
pub mod search;
pub mod toast;
pub mod view;

use hocus_words::shell::launcher as txt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{LAUNCHER_CLOSE, LAUNCHER_NAMESPACE, Launch, ToCompositor};
use iced::{keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use crate::{
    clock::time::Lang,
    field::{self, Entry, Took},
    popover::Effect,
};

/// The grid's columns at most, and the frequent applications shown at
/// most.
pub const COLUMNS: usize = 8;
const FREQUENT: usize = 8;
/// The characters the field keeps at most.
const QUERY_MOST: usize = 200;
/// The emoji a `:` query shows at most (5.7).
pub const EMOJI_MOST: usize = 8;

/// The launcher's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    pub fn lang(self) -> Lang {
        if self.lang == Lang::French {
            Lang::French
        } else {
            Lang::English
        }
    }

    pub fn placeholder(self) -> &'static str {
        self.pick(txt::SEARCH_FOR_AN_APPLICATION_CALCULATE)
    }

    pub fn escape(self) -> &'static str {
        self.pick(txt::ESC)
    }

    pub fn frequent(self) -> &'static str {
        self.pick(txt::FREQUENT)
    }

    pub fn all(self) -> &'static str {
        self.pick(txt::ALL_APPLICATIONS)
    }

    /// What `Enter` does on a card, shown at its right while it is
    /// selected.
    pub fn hint(self, card: Card) -> &'static str {
        match card {
            Card::Answer => self.pick(txt::ENTER_COPIES),
            Card::Address => self.pick(txt::ENTER_OPENS),
            Card::Web => self.pick(txt::ENTER_SEARCHES),
        }
    }

    pub fn open(self, typed: &str) -> String {
        txt::open(self.lang, typed)
    }

    pub fn in_browser(self, url: &str) -> String {
        txt::in_the_browser(self.lang, url)
    }

    pub fn search_web(self, query: &str) -> String {
        txt::search_the_web_for(self.lang, query)
    }

    pub fn web_engine(self) -> &'static str {
        self.pick(txt::DUCKDUCKGO_CTRL_ENTER_FROM_ANYWHERE)
    }

    /// The toast once a result is copied.
    pub fn copied(self, value: &str) -> String {
        txt::copied(self.lang, value)
    }

    pub fn projects_heading(self) -> &'static str {
        self.pick(hocus_words::shell::project::PROJECTS_HEADING)
    }

    pub fn repos_heading(self) -> &'static str {
        self.pick(hocus_words::shell::project::REPOS_HEADING)
    }

    pub fn nothing_for(self, term: &str) -> String {
        hocus_words::shell::project::nothing_for(self.lang, term)
    }

    /// Nine desktops taken: the notice's two sentences.
    pub fn no_room(self, name: &str) -> String {
        format!(
            "{} {}",
            self.pick(hocus_words::shell::project::NO_ROOM),
            hocus_words::shell::project::no_room_for(self.lang, name)
        )
    }
}

/// A wide row of the launcher's: a calculation's or a conversion's result,
/// an address, or the web search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Card {
    Answer,
    Address,
    Web,
}

/// What can be selected: an application (its index in the catalog), a
/// card, or an emoji (its index in the emoji's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    App(usize),
    Card(Card),
    Emoji(usize),
    /// A project, its place in [`Grid::projects`].
    Project(usize),
    /// A git repository, its place in [`Grid::repos`].
    Repo(usize),
}

/// An address typed, and what opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub typed: String,
    pub url: String,
}

/// A row of the grid: a heading, up to [`Grid::columns`] tiles, or a card
/// (their places in [`Grid::flat`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Head(&'static str),
    Tiles(Vec<usize>),
    Card(Card, usize),
    /// The emoji a `:` query finds, in a card of their own.
    Emoji(Vec<usize>),
    /// A project's row, or a repository's (their places in [`Grid::flat`]).
    Project(usize),
    Repo(usize),
    /// What the projects' rows say instead or before: nothing found, or
    /// no free desktop (amber).
    Notice {
        text: String,
        amber: bool,
    },
}

/// What the grid shows: its rows, and what each of their places holds, in
/// the selection's order; the answer or the address its cards show; how
/// many columns its rows of tiles have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub flat: Vec<Item>,
    pub answer: Option<answers::Answer>,
    pub address: Option<Address>,
    pub columns: usize,
    /// The projects and repositories listed, which their items name.
    pub projects: Vec<hocus_link::ProjectRow>,
    pub repos: Vec<crate::project::repos::Repo>,
    /// Nine desktops taken: `Enter` on a project asks to close it.
    pub no_room: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            flat: Vec::new(),
            answer: None,
            address: None,
            columns: COLUMNS,
            projects: Vec::new(),
            repos: Vec::new(),
            no_room: false,
        }
    }
}

/// What the projects' rows are made of: the projects the compositor said,
/// the repositories found, and the project waiting for a free desktop.
#[derive(Debug, Clone, Copy, Default)]
pub struct Places<'a> {
    pub projects: &'a [hocus_link::ProjectRow],
    pub repos: &'a [crate::project::repos::Repo],
    pub no_room: Option<&'a str>,
}

/// The repositories shown at most (§2.2).
const REPOS_MOST: usize = 3;

/// A text as names are compared: case and accents aside.
fn folded(text: &str) -> String {
    hocus_link::projects::name_key(text)
}

/// What follows `projet ` or `project ` at the field's start, if it does.
pub fn project_term(query: &str) -> Option<&str> {
    let trimmed = query.trim_start();
    let (word, rest) = trimmed.split_once(' ')?;
    matches!(folded(word).as_str(), "projet" | "project").then(|| rest.trim())
}

/// What the field is read with besides the applications: how numbers are
/// written.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub words: Words,
    pub numbers: answers::Numbers,
}

impl Grid {
    /// The grid for `query` over `catalog`, launched `counts` times each,
    /// in rows of `columns` tiles.
    pub fn of(
        catalog: &apps::Catalog,
        counts: &HashMap<String, u32>,
        query: &str,
        reading: Reading,
        columns: usize,
        places: Places<'_>,
    ) -> Self {
        let words = reading.words;
        let count = |i: usize| {
            catalog
                .apps
                .get(i)
                .and_then(|a| counts.get(&a.entry.id))
                .copied()
                .unwrap_or(0)
        };
        let mut grid = Self {
            columns: columns.clamp(1, COLUMNS),
            ..Self::default()
        };
        if let Some(name) = places.no_room {
            grid.no_room_rows(words, name, places.projects);
            return grid;
        }
        if let Some(term) = project_term(query) {
            grid.project_rows(words, term, places);
            return grid;
        }
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
            let typed = query.trim();
            if let Some(name) = typed.strip_prefix(':').filter(|n| !n.trim().is_empty()) {
                let found: Vec<usize> = crate::emoji::data::table()
                    .search(name)
                    .into_iter()
                    .take(EMOJI_MOST)
                    .collect();
                if !found.is_empty() {
                    let start = grid.flat.len();
                    grid.flat.extend(found.iter().map(|e| Item::Emoji(*e)));
                    grid.rows
                        .push(Row::Emoji((start..grid.flat.len()).collect()));
                }
            }
            grid.answer = answers::answer(typed, reading.numbers, words);
            if grid.answer.is_some() {
                grid.card(Card::Answer);
            } else if let Some(url) = answers::address(typed) {
                grid.address = Some(Address {
                    typed: typed.to_owned(),
                    url,
                });
                grid.card(Card::Address);
            } else {
                // A project found by its name, before the applications
                // (§13, I).
                grid.projects_named(words, typed, places.projects, false);
                let apps: Vec<(&search::Folded, u32)> = catalog
                    .apps
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (&a.folded, count(i)))
                    .collect();
                let found = search::rank(&apps, query);
                grid.chunk(&found);
            }
            grid.card(Card::Web);
        }
        grid
    }

    /// The projects whose name holds `term` (all of them for none), the
    /// open ones first, then the closed by when they closed, newest first,
    /// under their heading; only the open ones when `open_only`.
    fn projects_named(
        &mut self,
        words: Words,
        term: &str,
        projects: &[hocus_link::ProjectRow],
        open_only: bool,
    ) {
        use hocus_link::ProjectState;
        let term = folded(term);
        let mut found: Vec<&hocus_link::ProjectRow> = projects
            .iter()
            .filter(|p| folded(&p.name).contains(&term))
            .filter(|p| !open_only || matches!(p.state, ProjectState::Open { .. }))
            .collect();
        found.sort_by_key(|p| match p.state {
            ProjectState::Open { desktop, .. } => (0, i64::from(desktop)),
            ProjectState::Closed { at } => (1, -at),
        });
        if found.is_empty() {
            return;
        }
        self.rows.push(Row::Head(words.projects_heading()));
        for project in found {
            let at = self.flat.len();
            self.flat.push(Item::Project(self.projects.len()));
            self.projects.push(project.clone());
            self.rows.push(Row::Project(at));
        }
    }

    /// `projet …`'s rows: the projects, then three recent repositories
    /// that are no project's; nothing found, it says so.
    fn project_rows(&mut self, words: Words, term: &str, places: Places<'_>) {
        self.projects_named(words, term, places.projects, false);
        let wanted = folded(term);
        let taken: Vec<&str> = places.projects.iter().map(|p| p.folder.as_str()).collect();
        let repos: Vec<&crate::project::repos::Repo> = places
            .repos
            .iter()
            .filter(|r| folded(&r.name()).contains(&wanted))
            .filter(|r| !taken.contains(&r.path.to_string_lossy().as_ref()))
            .take(REPOS_MOST)
            .collect();
        if !repos.is_empty() {
            self.rows.push(Row::Head(words.repos_heading()));
            for repo in repos {
                let at = self.flat.len();
                self.flat.push(Item::Repo(self.repos.len()));
                self.repos.push(repo.clone());
                self.rows.push(Row::Repo(at));
            }
        }
        if self.flat.is_empty() && !term.is_empty() {
            self.rows.push(Row::Notice {
                text: words.nothing_for(term),
                amber: false,
            });
        }
    }

    /// Nine desktops taken for project `name`: it says so, and lists the
    /// open projects to close one.
    fn no_room_rows(&mut self, words: Words, name: &str, projects: &[hocus_link::ProjectRow]) {
        self.no_room = true;
        self.rows.push(Row::Notice {
            text: words.no_room(name),
            amber: true,
        });
        self.projects_named(words, "", projects, true);
    }

    fn chunk(&mut self, apps: &[usize]) {
        for row in apps.chunks(self.columns.max(1)) {
            let start = self.flat.len();
            self.flat.extend(row.iter().map(|i| Item::App(*i)));
            self.rows
                .push(Row::Tiles((start..self.flat.len()).collect()));
        }
    }

    fn card(&mut self, card: Card) {
        self.rows.push(Row::Card(card, self.flat.len()));
        self.flat.push(Item::Card(card));
    }

    /// The row holding place `at`, and its column (a card's is 0).
    pub fn place(&self, at: usize) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().find_map(|(r, row)| match row {
            Row::Tiles(tiles) | Row::Emoji(tiles) => {
                tiles.iter().position(|t| *t == at).map(|c| (r, c))
            }
            Row::Card(_, place) | Row::Project(place) | Row::Repo(place) => {
                (*place == at).then_some((r, 0))
            }
            Row::Head(_) | Row::Notice { .. } => None,
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
                        Row::Tiles(tiles) | Row::Emoji(tiles) => tiles
                            .get(column.min(tiles.len().saturating_sub(1)))
                            .copied(),
                        Row::Card(_, place) | Row::Project(place) | Row::Repo(place) => {
                            Some(*place)
                        }
                        Row::Head(_) | Row::Notice { .. } => None,
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
    /// Its screen, where a toast shows once it closes.
    pub output: String,
    pub scale: f32,
    /// The search field (#771).
    pub query: Entry,
    pub grid: Grid,
    pub catalog: Arc<apps::Catalog>,
    counts: HashMap<String, u32>,
    /// The tile selected, in [`Grid::flat`].
    pub selected: usize,
    /// The first row shown.
    pub offset: usize,
    /// The tile under the pointer since when.
    pub hover: Option<(usize, Instant)>,
    /// The surface's size, once laid out: what the grid's rows fit in;
    /// and the size its view was built for.
    pub size: iced::Size,
    pub built: iced::Size,
    /// When a cut name rested on shows whole.
    pub tooltip: Option<Instant>,
    /// The wheel's notches.
    pub wheel: crate::popover::kit::Wheel,
    /// What it last said it shows, in the log.
    pub logged: String,
    /// The project waiting for a free desktop (§13, H), until the field
    /// changes.
    pub no_room: Option<String>,
}

impl Open {
    fn regrid(&mut self, words: Words) {
        self.grid = self.grid_of(words, self.grid.columns);
        self.selected = 0;
        self.offset = 0;
        self.hover = None;
        self.tooltip = None;
    }

    fn grid_of(&self, words: Words, columns: usize) -> Grid {
        let reading = Reading {
            words,
            numbers: answers::Numbers::new(Lang::of_numbers()),
        };
        let projects = crate::project::rows();
        if project_term(self.query.text()).is_some() {
            repos_wanted(&projects);
        }
        let repos = REPOS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .clone();
        Grid::of(
            &self.catalog,
            &self.counts,
            self.query.text(),
            reading,
            columns,
            Places {
                projects: &projects,
                repos: &repos,
                no_room: self.no_room.as_deref(),
            },
        )
    }

    /// Its grid in rows of `columns` tiles, as its screen has room for:
    /// the same places in the same order, the selection kept in view.
    pub fn fit(&mut self, columns: usize) {
        if self.grid.columns == columns {
            return;
        }
        tracing::debug!(columns, "the launcher's grid fits its screen");
        self.grid = self.grid_of(words(), columns);
        self.offset = view::keep_in_view(&self.grid, self.selected, 0, self.size);
        self.hover = None;
        self.tooltip = None;
    }

    /// The application of tile `at`.
    pub fn app(&self, at: usize) -> Option<&apps::App> {
        match self.grid.flat.get(at)? {
            Item::App(i) => self.catalog.apps.get(*i),
            Item::Card(_) | Item::Emoji(_) | Item::Project(_) | Item::Repo(_) => None,
        }
    }

    /// The project of place `at`.
    pub fn project(&self, at: usize) -> Option<&hocus_link::ProjectRow> {
        match self.grid.flat.get(at)? {
            Item::Project(i) => self.grid.projects.get(*i),
            _ => None,
        }
    }

    /// The repository of place `at`.
    pub fn repo(&self, at: usize) -> Option<&crate::project::repos::Repo> {
        match self.grid.flat.get(at)? {
            Item::Repo(i) => self.grid.repos.get(*i),
            _ => None,
        }
    }
}

/// The recent repositories (§2.3), as last found, and whether a look is
/// under way.
static REPOS: Mutex<(bool, Vec<crate::project::repos::Repo>)> = Mutex::new((false, Vec::new()));

/// The repositories are looked for again on a thread (the look itself is
/// kept a minute), unless a look is under way; found, an open launcher
/// shows them.
fn repos_wanted(projects: &[hocus_link::ProjectRow]) {
    {
        let mut repos = REPOS.lock().unwrap_or_else(PoisonError::into_inner);
        if repos.0 {
            return;
        }
        repos.0 = true;
    }
    let folders: Vec<std::path::PathBuf> = projects
        .iter()
        .map(|p| std::path::PathBuf::from(&p.folder))
        .collect();
    let started = std::thread::Builder::new()
        .name("launcher repositories".to_owned())
        .spawn(move || {
            let found = crate::project::repos::Places::from_env()
                .map(|places| crate::project::repos::find(&places, &[], &folders))
                .unwrap_or_default();
            let changed = {
                let mut repos = REPOS.lock().unwrap_or_else(PoisonError::into_inner);
                let changed = repos.1 != found;
                repos.1 = found;
                repos.0 = false;
                changed
            };
            if changed {
                tracing::debug!("the launcher's repositories");
                self::changed();
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to look for the repositories");
        REPOS.lock().unwrap_or_else(PoisonError::into_inner).0 = false;
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
    crate::ui::touch();
    crate::popover::send(vec![Effect::Wake(LAUNCHER_NAMESPACE)]);
}

/// The applications, the projects or the repositories changed: an open
/// launcher shows them.
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
        icons::want(names.iter().map(String::as_str), view::ICON as u32, scale);
        wake();
    }
}

/// The icons of the grid's applications, in its order.
fn icon_names(open: &Open) -> Vec<String> {
    (0..open.grid.flat.len())
        .filter_map(|at| open.app(at)?.entry.icon.clone())
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
        let given = scale;
        let scale = (scale as f32 / 120.0).max(0.5);
        let mut open = Open {
            surface,
            output: output.clone(),
            scale,
            query: Entry::new(field::Kind::Line, QUERY_MOST),
            grid: Grid::default(),
            catalog,
            counts,
            selected: 0,
            offset: 0,
            hover: None,
            size: iced::Size::ZERO,
            built: iced::Size::ZERO,
            tooltip: None,
            wheel: crate::popover::kit::Wheel::default(),
            logged: String::new(),
            no_room: None,
        };
        *SCALE.lock().unwrap_or_else(PoisonError::into_inner) = Some(given);
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
        crate::actions::close();
        icons::want(names.iter().map(String::as_str), view::ICON as u32, scale);
    }
    crate::popover::send(effects);
}

/// The scale the launcher last opened at, in 120ths of the screen's: the
/// one it opens again at when nine desktops are taken.
static SCALE: Mutex<Option<u32>> = Mutex::new(None);

/// Project `name` could not open on `output`, nine desktops taken (§13,
/// H): the launcher opens there again, saying so, the open projects
/// listed to close one.
pub fn no_room(output: String, name: &str) {
    tracing::info!(name, "no free desktop for a project");
    let open = with(|l| matches!(l.phase, Phase::Open(_)));
    if !open {
        let scale = SCALE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or(120);
        toggle(output, scale);
    }
    with(|l| {
        if let Phase::Open(open) = &mut l.phase {
            open.no_room = Some(name.to_owned());
            open.query.set("");
            open.regrid(words());
        }
    });
    wake();
}

/// The actions palette's "Chercher « … » dans le lanceur" (#736): it opens
/// on `output`, `query` typed in its field.
pub fn open_with(output: String, scale: u32, query: &str) {
    if !with(|l| matches!(l.phase, Phase::Open(_))) {
        toggle(output, scale);
    }
    let wanted = with(|l| match &mut l.phase {
        Phase::Open(open) => {
            let query: String = query.chars().take(QUERY_MOST).collect();
            open.query.set(&query);
            open.regrid(words());
            tracing::debug!(query, "the launcher opens searching");
            Some((icon_names(open), open.scale))
        }
        Phase::Closed => None,
    });
    if let Some((names, scale)) = wanted {
        icons::want(names.iter().map(String::as_str), view::ICON as u32, scale);
        wake();
    }
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
    let launcher = with(|l| {
        let tooltip = match &l.phase {
            Phase::Open(open) => open.tooltip,
            Phase::Closed => None,
        };
        l.closing.iter().map(|(_, at)| *at).chain(tooltip).min()
    });
    launcher.into_iter().chain(toast::deadline()).min()
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
            crate::ui::touch();
            effects.push(Effect::Wake(LAUNCHER_NAMESPACE));
        }
        l.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        effects.extend(toast::tick(now));
        effects
    })
}

/// Place `at` of the open launcher on `id` is chosen: its application
/// starts, its result is copied, its address or the web search opens; and
/// the launcher closes.
pub fn act(id: window::Id, at: usize) {
    let effects = with(|l| {
        let mut effects = Vec::new();
        let Some(open) = l.open_on(id) else {
            return effects;
        };
        let words = words();
        match open.grid.flat.get(at) {
            None => return effects,
            Some(Item::App(_)) => {
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
            }
            Some(Item::Card(Card::Answer)) => {
                let Some(answer) = &open.grid.answer else {
                    return effects;
                };
                tracing::debug!(copied = answer.copy, "the launcher copies its result");
                effects.push(Effect::Copy(answer.copy.clone()));
                let output = open.output.clone();
                let said = words.copied(&answer.copy);
                l.close(Instant::now(), &mut effects);
                effects.extend(toast::show(output, said, Instant::now()));
                return effects;
            }
            Some(Item::Card(Card::Address)) => {
                if let Some(address) = &open.grid.address {
                    effects.extend(browse(&address.url));
                }
            }
            Some(Item::Card(Card::Web)) => effects.extend(browse(&answers::web(open.query.text()))),
            Some(Item::Project(_)) => {
                let Some(project) = open.project(at) else {
                    return effects;
                };
                let name = project.name.clone();
                let tell = if open.grid.no_room {
                    tracing::debug!(name, "the launcher asks to close a project");
                    ToCompositor::ProjectCloseAsk(name)
                } else {
                    match project.state {
                        hocus_link::ProjectState::Open { .. } => {
                            tracing::debug!(name, "the launcher goes to a project");
                            ToCompositor::ProjectGo(name)
                        }
                        hocus_link::ProjectState::Closed { .. } => {
                            tracing::debug!(name, "the launcher reopens a project");
                            ToCompositor::ProjectOpen {
                                output: open.output.clone(),
                                name,
                            }
                        }
                    }
                };
                effects.push(Effect::Tell(tell));
            }
            Some(Item::Repo(_)) => {
                let Some(repo) = open.repo(at) else {
                    return effects;
                };
                tracing::debug!("the launcher makes a project of a repository");
                effects.push(Effect::Tell(ToCompositor::ProjectNew {
                    output: open.output.clone(),
                    folder: repo.path.display().to_string(),
                }));
            }
            Some(Item::Emoji(e)) => {
                let Some(emoji) = crate::emoji::data::table().emoji.get(*e) else {
                    return effects;
                };
                let text = emoji.with_tone(crate::emoji::store::kept().tone);
                // The keyboard back to the window first, then the emoji.
                l.close(Instant::now(), &mut effects);
                crate::emoji::chosen(emoji, text, false, &mut effects);
                return effects;
            }
        }
        l.close(Instant::now(), &mut effects);
        effects
    });
    crate::popover::send(effects);
}

/// `Ctrl+Enter` on the open launcher on `id`: the web search for what is
/// typed, whatever is selected.
fn search_web(id: window::Id) {
    let at = with(|l| {
        let open = l.open_on(id)?;
        open.grid
            .flat
            .iter()
            .position(|item| *item == Item::Card(Card::Web))
    });
    if let Some(at) = at {
        act(id, at);
    }
}

/// The browser opens `url`: `xdg-open`, started by the compositor as the
/// applications are, off its loop. The program is looked for in the
/// shell's `PATH` and sent whole.
pub(crate) fn browse(url: &str) -> Option<Effect> {
    let Some(opener) = program("xdg-open") else {
        tracing::warn!("no xdg-open in PATH: the address does not open");
        return None;
    };
    let launch = Launch {
        id: "xdg-open".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![opener, url.to_owned()],
    };
    if !launch.fits() {
        tracing::warn!("an address that cannot be sent: not opened");
        return None;
    }
    tracing::debug!(url, "the launcher opens an address");
    Some(Effect::Tell(ToCompositor::Launch(launch)))
}

/// Where `name` is in `PATH`, if it is there and runs.
pub(crate) fn program(name: &str) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|file| {
            std::fs::metadata(file)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .map(|file| file.display().to_string())
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
        Act(usize),
        Web,
        Draw,
        Paste,
        Card(usize),
    }
    let words = words();
    let then = with(|l| {
        let open = l.open_mut(id)?;
        let arrow = |arrow: Arrow, open: &mut Open| {
            open.selected = open.grid.step(open.selected, arrow);
            open.offset = view::keep_in_view(&open.grid, open.selected, open.offset, open.size);
            Then::Draw
        };
        let held = modifiers.shift() || modifiers.control();
        Some(match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) => {
                if modifiers.control() && !open.query.text().trim().is_empty() {
                    Then::Web
                } else if open.grid.flat.is_empty() {
                    Then::Nothing
                } else {
                    Then::Act(open.selected)
                }
            }
            // `→` on a closed project: its card.
            Key::Named(Named::ArrowRight)
                if !held
                    && open.project(open.selected).is_some_and(|p| {
                        matches!(p.state, hocus_link::ProjectState::Closed { .. })
                    }) =>
            {
                Then::Card(open.selected)
            }
            // Across the grid; with Shift or Ctrl, in the field.
            Key::Named(Named::ArrowLeft) if !held => arrow(Arrow::Left, open),
            Key::Named(Named::ArrowRight) if !held => arrow(Arrow::Right, open),
            Key::Named(Named::ArrowUp) => arrow(Arrow::Up, open),
            Key::Named(Named::ArrowDown) => arrow(Arrow::Down, open),
            _ => match open.query.key(key, modifiers, text) {
                Took::No | Took::Same => Then::Nothing,
                Took::Moved => Then::Draw,
                Took::Edited => {
                    open.no_room = None;
                    open.regrid(words);
                    Then::Draw
                }
                Took::Paste => Then::Paste,
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
        Some(Then::Act(at)) => {
            act(id, at);
            true
        }
        Some(Then::Web) => {
            search_web(id);
            true
        }
        Some(Then::Draw) => true,
        Some(Then::Paste) => {
            paste(id);
            true
        }
        Some(Then::Card(at)) => {
            closed_card(id, at);
            true
        }
    }
}

/// `→` on closed project `at`: its card, centred on the launcher's
/// screen, the launcher closing.
fn closed_card(id: window::Id, at: usize) {
    let card = with(|l| {
        let open = l.open_on(id)?;
        let project = open.project(at)?;
        Some(hocus_link::ProjectCard {
            output: open.output.clone(),
            at: None,
            screen: (open.size.width as i32, open.size.height as i32),
            name: project.name.clone(),
            folder: project.folder.clone(),
            state: project.state,
            windows: project.windows.clone(),
        })
    });
    if let Some(card) = card {
        tracing::debug!(
            name = card.name,
            "the launcher shows a closed project's card"
        );
        crate::surfaces::ask(crate::surfaces::Asked::Project(crate::project::Come::Card(
            card,
        )));
    }
}

/// The clipboard's text, on one line, into the field of the launcher on
/// `id` at its caret, read off the loop.
fn paste(id: window::Id) {
    crate::paste::request(crate::paste::Clean::Spaced, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        let words = words();
        let took = with(|l| {
            l.open_mut(id).is_some_and(|open| {
                let took = open.query.paste(pasted);
                if took {
                    open.regrid(words);
                }
                took
            })
        });
        if took {
            wake();
        }
    });
}

/// What the field of the launcher on `id` took of an input method or of
/// the pointer: its grid follows what was typed; a paste is asked for.
pub fn took(id: window::Id, took: Took, words: Words) {
    match took {
        Took::Edited => with(|l| {
            if let Some(open) = l.open_mut(id) {
                open.no_room = None;
                open.regrid(words);
            }
        }),
        Took::Paste => paste(id),
        Took::No | Took::Same | Took::Moved => {}
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

/// The open launcher for screen readers: a dialog holding what its
/// controls declared (its field, the tiles, cards and emoji shown), the
/// selected one holding the focus.
pub fn a11y() -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    with(|l| {
        let Phase::Open(open) = &l.phase else {
            return None;
        };
        let rows = crate::a11y::rows::of(open.surface)?;
        let title = words().placeholder();
        let dialog = Node::new("dialog", Role::Dialog, title).children(rows.nodes);
        let mut window = Window::new("launcher", title);
        window.nodes = vec![dialog];
        window.active = true;
        window.focus = Some(if open.grid.flat.is_empty() {
            "launcher:Field".to_owned()
        } else {
            format!("launcher:Item({})", open.selected)
        });
        window.size = Some((open.size.width, open.size.height));
        Some(window)
    })
}

/// A screen reader clicks control `key` (`launcher:Item(N)`) of the open
/// launcher.
pub fn a11y_click(key: &str) {
    let Some(at) = key
        .strip_prefix("launcher:Item(")
        .and_then(|k| k.strip_suffix(')'))
        .and_then(|n| n.parse().ok())
    else {
        return;
    };
    if let Some(id) = with(|l| match &l.phase {
        Phase::Open(open) => Some(open.surface),
        Phase::Closed => None,
    }) {
        act(id, at);
    }
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
                ..entries::Entry::default()
            })
            .map(|entry| apps::App {
                folded: search::Folded::of(&entry.name, None, &[]),
                entry,
            })
            .collect();
        apps::Catalog { apps: entries }
    }

    fn reading(fr: bool) -> Reading {
        Reading {
            words: Words {
                lang: if fr {
                    hocus_words::Lang::French
                } else {
                    hocus_words::Lang::English
                },
            },
            numbers: answers::Numbers::new(if fr { Lang::French } else { Lang::English }),
        }
    }

    fn apps(found: &[usize]) -> Vec<Item> {
        found.iter().map(|i| Item::App(*i)).collect()
    }

    #[test]
    fn a_colon_shows_a_row_of_emoji_first_the_web_last() {
        let catalog = catalog(&["Chat", "Firefox"]);
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            ":chat",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        let Some(Row::Emoji(places)) = grid.rows.first() else {
            panic!("no emoji row: {:?}", grid.rows);
        };
        assert!(!places.is_empty() && places.len() <= EMOJI_MOST);
        let table = crate::emoji::data::table();
        let first = match grid.flat[0] {
            Item::Emoji(e) => table.emoji[e].text,
            other => panic!("{other:?}"),
        };
        assert!(["🐱", "🐈", "😺"].contains(&first), "{first}");
        assert_eq!(
            grid.rows.last(),
            Some(&Row::Card(Card::Web, grid.flat.len() - 1))
        );
        // Across the row, then down to the web.
        assert_eq!(grid.step(0, Arrow::Right), 1);
        assert_eq!(grid.step(0, Arrow::Down), grid.flat.len() - 1);
        // A colon alone, or nothing found: no row.
        let alone = Grid::of(
            &catalog,
            &HashMap::new(),
            ":",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert!(!alone.rows.iter().any(|r| matches!(r, Row::Emoji(_))));
        let none = Grid::of(
            &catalog,
            &HashMap::new(),
            ":zzzz",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert!(!none.rows.iter().any(|r| matches!(r, Row::Emoji(_))));
    }

    #[test]
    fn the_empty_field_shows_the_frequent_then_all_from_a_to_z() {
        let names: Vec<String> = (0..12).map(|i| format!("App {i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let catalog = catalog(&names);
        // Nothing launched yet: no frequent row.
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(grid.rows[0], Row::Head("TOUTES LES APPLICATIONS"));
        assert_eq!(grid.rows.len(), 3);
        let counts: HashMap<String, u32> = [("app 05", 3), ("app 01", 7), ("app 09", 3)]
            .into_iter()
            .map(|(id, n)| (id.to_owned(), n))
            .collect();
        let grid = Grid::of(
            &catalog,
            &counts,
            "",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(grid.rows[0], Row::Head("FRÉQUENTES"));
        // The most launched first, ties from A to Z.
        assert_eq!(&grid.flat[..3], &apps(&[1, 5, 9])[..]);
        assert_eq!(grid.rows[2], Row::Head("TOUTES LES APPLICATIONS"));
        assert_eq!(grid.rows.len(), 5);
        assert_eq!(grid.flat.len(), 3 + 12);
    }

    #[test]
    fn the_arrows_go_across_and_down_the_columns() {
        let names: Vec<String> = (0..20).map(|i| format!("App {i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let catalog = catalog(&names);
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "",
            reading(false),
            COLUMNS,
            Places::default(),
        );
        // Rows of 8, 8 and 4 under the heading.
        assert_eq!(grid.step(0, Arrow::Left), 0);
        assert_eq!(grid.step(7, Arrow::Right), 8);
        assert_eq!(grid.step(3, Arrow::Down), 11);
        assert_eq!(grid.step(14, Arrow::Down), 19);
        assert_eq!(grid.step(19, Arrow::Down), 19);
        assert_eq!(grid.step(11, Arrow::Up), 3);
        assert_eq!(grid.step(3, Arrow::Up), 3);
        assert_eq!(grid.step(19, Arrow::Right), 19);
        // On a screen with room for five columns: rows of 5, 5, 5 and 5,
        // the same places in the same order.
        let narrow = Grid::of(
            &catalog,
            &HashMap::new(),
            "",
            reading(false),
            5,
            Places::default(),
        );
        assert_eq!(narrow.rows.len(), 1 + 4);
        assert_eq!(narrow.flat, grid.flat);
        assert_eq!(narrow.step(3, Arrow::Down), 8);
        assert_eq!(narrow.step(4, Arrow::Right), 5);
    }

    #[test]
    fn typing_filters_the_grid_best_first_and_the_web_comes_last() {
        let catalog = catalog(&["Firefox", "Fichiers", "kitty"]);
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "fire",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(grid.rows, [Row::Tiles(vec![0]), Row::Card(Card::Web, 1)]);
        assert_eq!(grid.flat, [Item::App(0), Item::Card(Card::Web)]);
        // Down from the tile: the web row; and back up.
        assert_eq!(grid.step(0, Arrow::Down), 1);
        assert_eq!(grid.step(1, Arrow::Up), 0);
        let none = Grid::of(
            &catalog,
            &HashMap::new(),
            "zzz",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(none.rows, [Row::Card(Card::Web, 0)]);
    }

    #[test]
    fn an_answer_or_an_address_comes_first_and_lists_no_application() {
        let catalog = catalog(&["Calculatrice", "GitHub Desktop"]);
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "12 × (3 + 4)",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(grid.flat, [Item::Card(Card::Answer), Item::Card(Card::Web)]);
        assert_eq!(grid.answer.as_ref().map(|a| a.main.as_str()), Some("84"));
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "github.com",
            reading(true),
            COLUMNS,
            Places::default(),
        );
        assert_eq!(
            grid.flat,
            [Item::Card(Card::Address), Item::Card(Card::Web)]
        );
        assert_eq!(
            grid.address.as_ref().map(|a| a.url.as_str()),
            Some("https://github.com")
        );
        let words = Words {
            lang: hocus_words::Lang::French,
        };
        assert_eq!(
            words.search_web("crêpes"),
            "Chercher «\u{a0}crêpes\u{a0}» sur le web"
        );
    }

    #[test]
    fn projet_lists_the_projects_open_first_then_three_repositories() {
        use hocus_link::{ProjectRow, ProjectState};
        let row = |name: &str, state| ProjectRow {
            name: name.into(),
            folder: format!("/home/alex/github/{name}"),
            output: String::new(),
            state,
            windows: Vec::new(),
        };
        let projects = [
            row("hocus-old", ProjectState::Closed { at: 100 }),
            row("hocus-site", ProjectState::Closed { at: 200 }),
            row("blog", ProjectState::Closed { at: 300 }),
            row(
                "hocus-panda",
                ProjectState::Open {
                    desktop: 0,
                    since: 0,
                },
            ),
        ];
        let repo = |name: &str| crate::project::repos::Repo {
            path: format!("/home/alex/github/{name}").into(),
            when: None,
            terminals: false,
        };
        let repos = [
            repo("hocus-site"),
            repo("hocus-cli"),
            repo("hocus-icons"),
            repo("hocus-tools"),
            repo("hocus-more"),
        ];
        let places = Places {
            projects: &projects,
            repos: &repos,
            no_room: None,
        };
        let catalog = catalog(&["Hocus App"]);
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "Projet hoc",
            reading(true),
            COLUMNS,
            places,
        );
        let names: Vec<String> = grid.projects.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["hocus-panda", "hocus-site", "hocus-old"]);
        // Three repositories, none a project's folder; no application, no
        // web row.
        let repos: Vec<String> = grid
            .repos
            .iter()
            .map(crate::project::repos::Repo::name)
            .collect();
        assert_eq!(repos, ["hocus-cli", "hocus-icons", "hocus-tools"]);
        assert!(
            grid.flat
                .iter()
                .all(|i| matches!(i, Item::Project(_) | Item::Repo(_)))
        );
        // Nothing found says so.
        let none = Grid::of(
            &catalog,
            &HashMap::new(),
            "projet zut",
            reading(true),
            COLUMNS,
            places,
        );
        assert!(matches!(&none.rows[..], [Row::Notice { amber: false, .. }]));
        // By its name alone, before the applications (§13, I).
        let named = Grid::of(
            &catalog,
            &HashMap::new(),
            "hocus-p",
            reading(true),
            COLUMNS,
            places,
        );
        assert_eq!(named.flat.first(), Some(&Item::Project(0)));
        assert_eq!(named.projects[0].name, "hocus-panda");
        // Nine desktops taken: the open ones, to close one.
        let full = Places {
            no_room: Some("blog"),
            ..places
        };
        let full = Grid::of(&catalog, &HashMap::new(), "", reading(true), COLUMNS, full);
        assert!(full.no_room);
        assert!(matches!(full.rows[0], Row::Notice { amber: true, .. }));
        assert_eq!(full.projects.len(), 1);
        assert_eq!(project_term("projet "), Some(""));
        assert_eq!(project_term("PROJECT hoc"), Some("hoc"));
        assert_eq!(project_term("projets x"), None);
    }
}
