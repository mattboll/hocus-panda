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

pub mod answers;
pub mod apps;
pub mod entries;
pub mod icons;
pub mod search;
pub mod toast;
pub mod view;

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

    pub fn lang(self) -> Lang {
        if self.fr { Lang::French } else { Lang::English }
    }

    pub fn placeholder(self) -> &'static str {
        self.pick(
            "Chercher une application, calculer…",
            "Search for an application, calculate…",
        )
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

    /// What `Enter` does on a card, shown at its right while it is
    /// selected.
    pub fn hint(self, card: Card) -> &'static str {
        match card {
            Card::Answer => self.pick("Entrée copie", "Enter copies"),
            Card::Address => self.pick("Entrée ouvre", "Enter opens"),
            Card::Web => self.pick("Entrée cherche", "Enter searches"),
        }
    }

    pub fn open(self, typed: &str) -> String {
        if self.fr {
            format!("Ouvrir {typed}")
        } else {
            format!("Open {typed}")
        }
    }

    pub fn in_browser(self, url: &str) -> String {
        if self.fr {
            format!("{url} · dans le navigateur")
        } else {
            format!("{url} · in the browser")
        }
    }

    pub fn search_web(self, query: &str) -> String {
        if self.fr {
            format!("Chercher «\u{a0}{query}\u{a0}» sur le web")
        } else {
            format!("Search the web for \u{201c}{query}\u{201d}")
        }
    }

    pub fn web_engine(self) -> &'static str {
        self.pick(
            "DuckDuckGo · Ctrl+Entrée depuis n\u{2019}importe où",
            "DuckDuckGo · Ctrl+Enter from anywhere",
        )
    }

    /// The toast once a result is copied.
    pub fn copied(self, value: &str) -> String {
        if self.fr {
            format!("Copié\u{a0}: {value}")
        } else {
            format!("Copied: {value}")
        }
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
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            flat: Vec::new(),
            answer: None,
            address: None,
            columns: COLUMNS,
        }
    }
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
            Row::Card(_, place) => (*place == at).then_some((r, 0)),
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
                        Row::Tiles(tiles) | Row::Emoji(tiles) => tiles
                            .get(column.min(tiles.len().saturating_sub(1)))
                            .copied(),
                        Row::Card(_, place) => Some(*place),
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
        Grid::of(
            &self.catalog,
            &self.counts,
            self.query.text(),
            reading,
            columns,
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
            Item::Card(_) | Item::Emoji(_) => None,
        }
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
        crate::actions::close();
        icons::want(names.iter().map(String::as_str), view::ICON as u32, scale);
    }
    crate::popover::send(effects);
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
            // Across the grid; with Shift or Ctrl, in the field.
            Key::Named(Named::ArrowLeft) if !held => arrow(Arrow::Left, open),
            Key::Named(Named::ArrowRight) if !held => arrow(Arrow::Right, open),
            Key::Named(Named::ArrowUp) => arrow(Arrow::Up, open),
            Key::Named(Named::ArrowDown) => arrow(Arrow::Down, open),
            _ => match open.query.key(key, modifiers, text) {
                Took::No | Took::Same => Then::Nothing,
                Took::Moved => Then::Draw,
                Took::Edited => {
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
            words: Words { fr },
            numbers: answers::Numbers::new(if fr { Lang::French } else { Lang::English }),
        }
    }

    fn apps(found: &[usize]) -> Vec<Item> {
        found.iter().map(|i| Item::App(*i)).collect()
    }

    #[test]
    fn a_colon_shows_a_row_of_emoji_first_the_web_last() {
        let catalog = catalog(&["Chat", "Firefox"]);
        let grid = Grid::of(&catalog, &HashMap::new(), ":chat", reading(true), COLUMNS);
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
        let alone = Grid::of(&catalog, &HashMap::new(), ":", reading(true), COLUMNS);
        assert!(!alone.rows.iter().any(|r| matches!(r, Row::Emoji(_))));
        let none = Grid::of(&catalog, &HashMap::new(), ":zzzz", reading(true), COLUMNS);
        assert!(!none.rows.iter().any(|r| matches!(r, Row::Emoji(_))));
    }

    #[test]
    fn the_empty_field_shows_the_frequent_then_all_from_a_to_z() {
        let names: Vec<String> = (0..12).map(|i| format!("App {i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let catalog = catalog(&names);
        // Nothing launched yet: no frequent row.
        let grid = Grid::of(&catalog, &HashMap::new(), "", reading(true), COLUMNS);
        assert_eq!(grid.rows[0], Row::Head("TOUTES LES APPLICATIONS"));
        assert_eq!(grid.rows.len(), 3);
        let counts: HashMap<String, u32> = [("app 05", 3), ("app 01", 7), ("app 09", 3)]
            .into_iter()
            .map(|(id, n)| (id.to_owned(), n))
            .collect();
        let grid = Grid::of(&catalog, &counts, "", reading(true), COLUMNS);
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
        let grid = Grid::of(&catalog, &HashMap::new(), "", reading(false), COLUMNS);
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
        let narrow = Grid::of(&catalog, &HashMap::new(), "", reading(false), 5);
        assert_eq!(narrow.rows.len(), 1 + 4);
        assert_eq!(narrow.flat, grid.flat);
        assert_eq!(narrow.step(3, Arrow::Down), 8);
        assert_eq!(narrow.step(4, Arrow::Right), 5);
    }

    #[test]
    fn typing_filters_the_grid_best_first_and_the_web_comes_last() {
        let catalog = catalog(&["Firefox", "Fichiers", "kitty"]);
        let grid = Grid::of(&catalog, &HashMap::new(), "fire", reading(true), COLUMNS);
        assert_eq!(grid.rows, [Row::Tiles(vec![0]), Row::Card(Card::Web, 1)]);
        assert_eq!(grid.flat, [Item::App(0), Item::Card(Card::Web)]);
        // Down from the tile: the web row; and back up.
        assert_eq!(grid.step(0, Arrow::Down), 1);
        assert_eq!(grid.step(1, Arrow::Up), 0);
        let none = Grid::of(&catalog, &HashMap::new(), "zzz", reading(true), COLUMNS);
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
        );
        assert_eq!(grid.flat, [Item::Card(Card::Answer), Item::Card(Card::Web)]);
        assert_eq!(grid.answer.as_ref().map(|a| a.main.as_str()), Some("84"));
        let grid = Grid::of(
            &catalog,
            &HashMap::new(),
            "github.com",
            reading(true),
            COLUMNS,
        );
        assert_eq!(
            grid.flat,
            [Item::Card(Card::Address), Item::Card(Card::Web)]
        );
        assert_eq!(
            grid.address.as_ref().map(|a| a.url.as_str()),
            Some("https://github.com")
        );
        let words = Words { fr: true };
        assert_eq!(
            words.search_web("crêpes"),
            "Chercher «\u{a0}crêpes\u{a0}» sur le web"
        );
    }
}
