//! The emoji picker (5.7, variant A, docs/design/emoji.md): `Super+.`
//! opens it under the focused application's caret (else centred on its
//! window, else on the screen), its search holding the keyboard. The
//! recent emoji first, then the groups; the tabs jump to a group; typing
//! searches the names and keywords in the shell's language and in
//! English. `Enter` or a click inserts the selected emoji into the
//! application and closes it; `Shift+Enter` or `Shift+click` inserts it
//! and keeps it open. The skin tone, chosen with the round button, applies
//! to every emoji that has tones.
//!
//! Its surface is a layer surface of the shell's ([`EMOJI_NAMESPACE`]), in
//! the overlay layer, as large as the panel, placed by its margins from
//! the screen's top left corner. The compositor draws its glass and its
//! shadow and moves it in and out, as a popover's. Its surface is the
//! widget kit's ([`view`]): what its widgets ask is handled at once, and
//! it alone draws again ([`crate::popover::effects`] carries out the
//! rest). The compositor inserts the emoji (`emoji insert TEXT`).
//!
//! The emoji's data ([`data`]) are read once; the recent ones and the tone
//! are kept on disk ([`store`]).

pub mod data;
pub mod store;
pub mod view;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use hocus_link::{EMOJI_NAMESPACE, EmojiAt, POPOVER_CLOSE, ToCompositor};
use iced::{Element, keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use self::data::Group;
use crate::{
    clock::time::Lang,
    field::{self, Entry, Took},
    popover::{Effect, Pressed},
    surfaces::{Asked, Context, Lent, Surface},
};

/// The grid's columns.
pub const COLUMNS: usize = 8;
/// The characters the search keeps at most.
const QUERY_MOST: usize = 100;
/// The margin kept to the screen's edges, and between the caret and the
/// picker (logical pixels).
const EDGE: f32 = 8.0;
const UNDER: f32 = 4.0 * view::ZOOM;

/// The picker's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn fr(self) -> bool {
        self.lang == Lang::French
    }

    pub fn placeholder(self) -> &'static str {
        if self.fr() {
            "Chercher un emoji"
        } else {
            "Search emoji"
        }
    }

    pub fn recent(self) -> &'static str {
        if self.fr() { "Récents" } else { "Recent" }
    }

    pub fn heading(self, heading: Heading) -> &'static str {
        match heading {
            Heading::Recent => self.recent(),
            Heading::Group(group) => group.name(self.lang),
        }
    }

    pub fn inserts(self) -> &'static str {
        if self.fr() {
            "Entrée insère"
        } else {
            "Enter inserts"
        }
    }

    pub fn none(self, query: &str) -> String {
        if self.fr() {
            format!("Aucun emoji pour «\u{a0}{query}\u{a0}»")
        } else {
            format!("No emoji for \u{201c}{query}\u{201d}")
        }
    }

    /// The skin tone's button.
    pub fn tones(self) -> &'static str {
        if self.fr() {
            "Teinte de peau"
        } else {
            "Skin tone"
        }
    }

    /// Tone `i`'s name: yellow, then light to dark.
    pub fn tone(self, i: usize) -> &'static str {
        const FR: [&str; 6] = [
            "Jaune",
            "Claire",
            "Moyennement claire",
            "Moyenne",
            "Moyennement foncée",
            "Foncée",
        ];
        const EN: [&str; 6] = [
            "Yellow",
            "Light",
            "Medium-light",
            "Medium",
            "Medium-dark",
            "Dark",
        ];
        let names = if self.fr() { FR } else { EN };
        names.get(i).copied().unwrap_or_default()
    }

    /// The launcher's emoji row's heading (5.7).
    pub fn launcher_row(self) -> &'static str {
        if self.fr() {
            "Emoji · Entrée insère le premier, les flèches choisissent"
        } else {
            "Emoji · Enter inserts the first, the arrows choose"
        }
    }
}

/// A heading of the grid, and a tab: the recent emoji, or a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    Recent,
    Group(Group),
}

impl Heading {
    /// The tabs, in order.
    pub fn tabs() -> Vec<Self> {
        std::iter::once(Self::Recent)
            .chain(Group::ALL.iter().map(|g| Self::Group(*g)))
            .collect()
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Recent => "🕘",
            Self::Group(group) => group.icon(),
        }
    }
}

/// A row of the grid: a heading, or up to [`COLUMNS`] emoji (their places
/// in [`Grid::flat`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Head(Heading),
    Cells(std::ops::Range<usize>),
}

/// What the grid shows: its rows, the emoji of their places (indices in
/// the table), and the row each heading is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub flat: Vec<usize>,
    pub starts: Vec<(Heading, usize)>,
}

impl Grid {
    /// The grid for `query`, the recent emoji `recent` first when it is
    /// empty.
    pub fn of(table: &data::Table, query: &str, recent: &[usize]) -> Self {
        let mut grid = Self::default();
        if query.trim().is_empty() {
            if !recent.is_empty() {
                grid.head(Heading::Recent);
                grid.cells(recent);
            }
            for (group, range) in table.groups() {
                grid.head(Heading::Group(group));
                grid.cells(&range.collect::<Vec<_>>());
            }
        } else {
            grid.cells(&table.search(query));
        }
        grid
    }

    fn head(&mut self, heading: Heading) {
        self.starts.push((heading, self.rows.len()));
        self.rows.push(Row::Head(heading));
    }

    fn cells(&mut self, emoji: &[usize]) {
        for chunk in emoji.chunks(COLUMNS) {
            let start = self.flat.len();
            self.flat.extend_from_slice(chunk);
            self.rows.push(Row::Cells(start..self.flat.len()));
        }
    }

    /// The row holding place `at`, and its column.
    pub fn place(&self, at: usize) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().find_map(|(r, row)| match row {
            Row::Cells(range) if range.contains(&at) => Some((r, at - range.start)),
            _ => None,
        })
    }

    /// Where an arrow goes from `at`: across, by one; up or down, the same
    /// column of the next row of emoji (its last when it is shorter).
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
                        Row::Cells(range) => {
                            Some((range.start + column).min(range.end.saturating_sub(1)))
                        }
                        Row::Head(_) => None,
                    })
                    .unwrap_or(at)
            }
        }
    }

    /// The tab lit with `offset` the first row shown: the heading last
    /// passed.
    pub fn lit(&self, offset: usize) -> Option<Heading> {
        self.starts
            .iter()
            .rev()
            .find(|(_, row)| *row <= offset)
            .or_else(|| self.starts.first())
            .map(|(h, _)| *h)
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

/// The picker open.
#[derive(Debug)]
pub struct Open {
    pub surface: window::Id,
    /// Where it is asked to open, and the screen's size.
    pub at: EmojiAt,
    pub screen: (f32, f32),
    /// Its margins from the screen's top left corner, as last asked.
    pub place: (i32, i32),
    /// The search field (#771).
    pub query: Entry,
    pub grid: Grid,
    /// The place selected, in [`Grid::flat`], and how far the grid is
    /// scrolled, as it said or was told.
    pub selected: usize,
    pub scroll: f32,
    pub tones_open: bool,
    /// `Shift` is held: a click inserts and keeps it open.
    pub shift: bool,
    /// What it last said it shows, in the log.
    pub logged: String,
    /// It holds the keyboard; not while an emoji inserted with the picker
    /// kept open goes in.
    pub holding: bool,
}

impl Open {
    fn regrid(&mut self) {
        let table = data::table();
        let recent = recent_of(table);
        self.grid = Grid::of(table, self.query.text(), &recent);
        self.selected = 0;
        view::to_row(self, 0);
    }

    /// The emoji of place `at`, as inserted: with the tone chosen.
    pub fn emoji(&self, at: usize) -> Option<(&'static data::Emoji, String)> {
        let emoji = data::table().emoji.get(*self.grid.flat.get(at)?)?;
        Some((emoji, emoji.with_tone(store::kept().tone)))
    }

    /// Its size, logical pixels.
    pub fn size(&self) -> (u32, u32) {
        view::size(self.tones_open)
    }
}

/// The recent emoji, as places in the table.
fn recent_of(table: &data::Table) -> Vec<usize> {
    store::kept()
        .recent
        .iter()
        .filter_map(|e| table.find(e))
        .collect()
}

/// Where a picker `size` large opens on a screen `screen` large, at `at`:
/// its margins from the screen's top left corner. Under the caret, its
/// left on the caret's, or above it without room under it; centred on the
/// window or the screen; on the screen, 8 px from its edges, whatever.
pub fn place(at: EmojiAt, size: (u32, u32), screen: (f32, f32)) -> (i32, i32) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let (sw, sh) = screen;
    let (x, y) = match at {
        EmojiAt::Caret(c) => {
            let below = c.y as f32 + c.height as f32 + UNDER;
            let y = if below + h <= sh - EDGE {
                below
            } else {
                c.y as f32 - UNDER - h
            };
            (c.x as f32, y)
        }
        EmojiAt::Window(r) => (
            r.x as f32 + (r.width as f32 - w) / 2.0,
            r.y as f32 + (r.height as f32 - h) / 2.0,
        ),
        EmojiAt::Screen => ((sw - w) / 2.0, (sh - h) / 2.0),
    };
    let x = x.min(sw - EDGE - w).max(EDGE);
    let y = y.min(sh - EDGE - h).max(EDGE);
    (x.round() as i32, y.round() as i32)
}

#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Closed,
    Open(Box<Open>),
}

/// The picker, and its surfaces fading out until they go: the shell's own
/// (#773).
#[derive(Debug, Default)]
pub struct Picker {
    pub phase: Phase,
    closing: Vec<(window::Id, Instant)>,
    /// The time a tick was asked for.
    asked: Option<Instant>,
}

/// Whether the picker holds the keyboard open, for the threads that do
/// not own it (a press outside, a key that reached another surface): a
/// flag it keeps, nothing they read of its state.
static HOLDING: AtomicBool = AtomicBool::new(false);
/// Whether it is open at all.
static OPEN: AtomicBool = AtomicBool::new(false);

/// What a screen reader clicks on the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clicked {
    Cell(usize),
    Tab(Heading),
    Tone(usize),
    Tones,
}

/// What other threads and surfaces ask of the picker, as messages for the
/// loop that owns it ([`crate::surfaces::ask`]).
#[derive(Debug, Clone)]
pub enum Come {
    /// `Super+.`: it opens on that screen at that place, or closes.
    Toggle {
        output: String,
        screen: (i32, i32),
        at: EmojiAt,
    },
    /// A press outside, another surface opening, the lock: it closes.
    Close,
    /// The emoji inserted with the picker kept open is in.
    Inserted,
    /// A key that reached another of the shell's surfaces.
    Key(Pressed),
    /// The clipboard's text for the search of the picker on that surface.
    Pasted(window::Id, String),
    /// A screen reader's click.
    Clicked(Clicked),
    /// What was due is due ([`Picker::due`]).
    Tick,
}

/// What screen readers are told of the picker ([`a11y`]).
#[derive(Debug, Clone)]
pub struct Seen {
    surface: window::Id,
    holding: bool,
    focus: String,
    size: (u32, u32),
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Reads what is kept, and the table.
pub fn start() {
    store::start();
}

/// A press on none of the shell's surfaces: it closes.
pub fn outside() {
    close();
}

/// It closes, if it is open: asked of the loop, from any thread.
pub fn close() {
    if OPEN.load(Ordering::Relaxed) {
        crate::surfaces::ask(Asked::Emoji(Come::Close));
    }
}

/// A key that reached another of the shell's surfaces while the picker
/// holds the keyboard (layershellev hands keys to the surface last created
/// or clicked): its, handed to the loop. Whether it was taken.
pub fn key(key: &keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> bool {
    if !HOLDING.load(Ordering::Relaxed) {
        return false;
    }
    crate::surfaces::ask(Asked::Emoji(Come::Key(Pressed {
        key: key.clone(),
        modifiers,
        text: text.map(str::to_owned),
    })));
    true
}

/// `text` chosen (`keep`: the picker stays open): the recent ones take it,
/// and the compositor inserts it.
pub fn chosen(emoji: &data::Emoji, text: String, keep: bool, effects: &mut Vec<Effect>) {
    tracing::debug!(keep, "an emoji chosen");
    store::change(|k| k.used(emoji.text));
    effects.push(Effect::Tell(ToCompositor::Emoji { keep, text }));
}

/// Its surface drawn again, its view built again.
fn woken() -> Vec<Effect> {
    crate::ui::touch();
    vec![Effect::Wake(EMOJI_NAMESPACE)]
}

impl Picker {
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

    fn surface(&self) -> Option<window::Id> {
        match &self.phase {
            Phase::Open(open) => Some(open.surface),
            Phase::Closed => None,
        }
    }

    /// Whether surface `id` is the picker's, open or fading out.
    pub fn is_surface(&self, id: window::Id) -> bool {
        self.open_on(id).is_some() || self.closing.iter().any(|(c, _)| *c == id)
    }

    /// `come` taken in at `now`: what the shell then does.
    pub fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        let effects = match come {
            Come::Toggle { output, screen, at } => self.toggle(output, screen, at, now),
            Come::Close => {
                let mut effects = Vec::new();
                self.close(now, &mut effects);
                effects
            }
            Come::Inserted => self.inserted(),
            Come::Key(pressed) => match self.surface() {
                Some(id) => {
                    let mut effects = self
                        .key_on(
                            id,
                            &pressed.key,
                            pressed.modifiers,
                            pressed.text.as_deref(),
                            now,
                        )
                        .unwrap_or_default();
                    effects.extend(woken());
                    effects
                }
                None => Vec::new(),
            },
            Come::Pasted(id, pasted) => {
                let pasted = |entry: &mut Entry| match entry.paste(&pasted) {
                    true => Took::Edited,
                    false => Took::Same,
                };
                if self.edited(id, pasted) {
                    woken()
                } else {
                    Vec::new()
                }
            }
            Come::Clicked(clicked) => match self.surface() {
                Some(id) => {
                    let mut effects = match clicked {
                        Clicked::Cell(at) => self.act(id, at, false, now),
                        Clicked::Tab(heading) => {
                            self.tab(id, heading);
                            Vec::new()
                        }
                        Clicked::Tone(tone) => self.tones(id, Some(tone)),
                        Clicked::Tones => self.tones(id, None),
                    };
                    effects.extend(woken());
                    effects
                }
                None => Vec::new(),
            },
            Come::Tick => {
                self.asked = None;
                self.tick(now)
            }
        };
        self.said();
        effects
    }

    /// What the threads that do not own it are told of it: whether it is
    /// open and holds the keyboard, and what screen readers read.
    pub fn said(&self) {
        let (open, holding) = match &self.phase {
            Phase::Open(open) => (true, open.holding),
            Phase::Closed => (false, false),
        };
        OPEN.store(open, Ordering::Relaxed);
        HOLDING.store(holding, Ordering::Relaxed);
        crate::a11y::shown(crate::a11y::Shown::Emoji(self.seen()));
    }

    fn seen(&self) -> Option<Seen> {
        let Phase::Open(open) = &self.phase else {
            return None;
        };
        Some(Seen {
            surface: open.surface,
            holding: open.holding,
            focus: if open.grid.flat.is_empty() {
                "emoji:Field".to_owned()
            } else {
                format!("emoji:Cell({})", open.selected)
            },
            size: open.size(),
        })
    }

    /// When a tick is due, if one was not asked for by then already.
    pub fn due(&mut self) -> Option<Instant> {
        let at = self.closing.iter().map(|(_, at)| *at).min()?;
        if self.asked.is_some_and(|asked| asked <= at) {
            return None;
        }
        self.asked = Some(at);
        Some(at)
    }

    /// It closes at `now`: the keyboard goes back at once, the compositor
    /// fades it out, and it goes after that.
    fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        let Phase::Open(open) = std::mem::take(&mut self.phase) else {
            return;
        };
        tracing::debug!("the emoji picker closes");
        self.closing.push((open.surface, now + POPOVER_CLOSE));
        effects.push(Effect::Keyboard(open.surface, KeyboardInteractivity::None));
        effects.push(Effect::Tell(ToCompositor::Closing(
            EMOJI_NAMESPACE.to_owned(),
        )));
    }

    /// `Super+.` (the compositor's line): it opens on `output` at `at`, or
    /// closes when it is open.
    fn toggle(
        &mut self,
        output: String,
        screen: (i32, i32),
        at: EmojiAt,
        now: Instant,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        if matches!(self.phase, Phase::Open(_)) {
            self.close(now, &mut effects);
            return effects;
        }
        let surface = window::Id::unique();
        let screen = (screen.0 as f32, screen.1 as f32);
        let mut open = Open {
            surface,
            at,
            screen,
            place: (0, 0),
            query: Entry::new(field::Kind::Line, QUERY_MOST),
            grid: Grid::default(),
            selected: 0,
            scroll: 0.0,
            tones_open: false,
            shift: false,
            logged: String::new(),
            holding: true,
        };
        open.regrid();
        let size = open.size();
        open.place = place(at, size, screen);
        tracing::debug!(output, ?at, place = ?open.place, "the emoji picker opens");
        effects.push(Effect::Emoji {
            id: surface,
            output,
            size,
            margin: (open.place.1, 0, 0, open.place.0),
        });
        self.phase = Phase::Open(Box::new(open));
        // One at a time: the bar's popover, the launcher and the actions
        // palette close.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        crate::launcher::close();
        crate::actions::close();
        effects
    }

    /// The emoji inserted with the picker kept open is in: it takes the
    /// keyboard back.
    fn inserted(&mut self) -> Vec<Effect> {
        match &mut self.phase {
            Phase::Open(open) if !open.holding => {
                open.holding = true;
                vec![
                    Effect::Keyboard(open.surface, KeyboardInteractivity::Exclusive),
                    Effect::Wake(EMOJI_NAMESPACE),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// What is due at `now`: the surfaces faded out go.
    fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        effects
    }

    /// Place `at` of the picker on `id` is chosen: inserted, and the
    /// picker closes unless `keep`.
    pub fn act(&mut self, id: window::Id, at: usize, keep: bool, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let Some(open) = self.open_mut(id) else {
            return effects;
        };
        if !open.holding {
            // The one before is still on its way.
            return effects;
        }
        let Some((emoji, text)) = open.emoji(at) else {
            return effects;
        };
        if keep {
            // The window takes the keyboard to get it; the picker takes it
            // back once it is in (`emoji-inserted`).
            open.holding = false;
            effects.push(Effect::Keyboard(open.surface, KeyboardInteractivity::None));
        } else {
            self.close(now, &mut effects);
        }
        chosen(emoji, text, keep, &mut effects);
        effects
    }

    /// The tones' row opens or closes, or tone `tone` is chosen (it
    /// closes).
    pub fn tones(&mut self, id: window::Id, tone: Option<usize>) -> Vec<Effect> {
        let Some(open) = self.open_mut(id) else {
            return Vec::new();
        };
        match tone {
            Some(tone) => {
                store::change(|k| k.tone = tone);
                open.tones_open = false;
            }
            None => open.tones_open = !open.tones_open,
        }
        let size = open.size();
        let place = place(open.at, size, open.screen);
        let margin = (place != open.place).then_some((place.1, 0, 0, place.0));
        open.place = place;
        vec![Effect::Resize { id, size, margin }]
    }

    /// A tab is chosen: the grid goes to its heading, the search cleared.
    pub fn tab(&mut self, id: window::Id, heading: Heading) {
        let Some(open) = self.open_mut(id) else {
            return;
        };
        if !open.query.is_empty() {
            open.query.clear();
            open.regrid();
        }
        let row = open
            .grid
            .starts
            .iter()
            .find(|(h, _)| *h == heading)
            .map(|(_, r)| *r);
        if let Some(row) = row {
            view::to_row(open, row);
            if let Some(Row::Cells(range)) = open.grid.rows.get(row + 1) {
                open.selected = range.start;
            }
        } else {
            view::to_row(open, 0);
        }
    }

    /// A key on the open picker on `id`: what it does, if it was its.
    pub fn key_on(
        &mut self,
        id: window::Id,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        now: Instant,
    ) -> Option<Vec<Effect>> {
        use keyboard::{Key, key::Named};
        enum Then {
            Nothing,
            Close,
            Act(usize, bool),
            Paste,
        }
        let open = self.open_mut(id)?;
        let held = modifiers.shift() || modifiers.control();
        let arrow = |arrow: Arrow, open: &mut Open| {
            open.selected = open.grid.step(open.selected, arrow);
            view::reveal(open);
            Then::Nothing
        };
        let then = match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) => {
                if open.grid.flat.is_empty() {
                    Then::Nothing
                } else {
                    Then::Act(open.selected, modifiers.shift())
                }
            }
            // Across the grid; with Shift or Ctrl, in the field.
            Key::Named(Named::ArrowLeft) if !held => arrow(Arrow::Left, open),
            Key::Named(Named::ArrowRight) if !held => arrow(Arrow::Right, open),
            Key::Named(Named::ArrowUp) => arrow(Arrow::Up, open),
            Key::Named(Named::ArrowDown) => arrow(Arrow::Down, open),
            Key::Named(Named::Tab) => Then::Nothing,
            _ => match open.query.key(key, modifiers, text) {
                Took::No | Took::Same | Took::Moved => Then::Nothing,
                Took::Edited => {
                    open.regrid();
                    Then::Nothing
                }
                Took::Paste => Then::Paste,
            },
        };
        Some(match then {
            Then::Nothing => Vec::new(),
            Then::Close => {
                let mut effects = Vec::new();
                self.close(now, &mut effects);
                effects
            }
            Then::Act(at, keep) => self.act(id, at, keep, now),
            Then::Paste => {
                paste(id);
                Vec::new()
            }
        })
    }

    /// The field of the picker on `id` edited: its grid follows. Whether
    /// it draws again.
    fn edited(&mut self, id: window::Id, edit: impl FnOnce(&mut Entry) -> Took) -> bool {
        self.open_mut(id).is_some_and(|open| {
            let took = edit(&mut open.query);
            if took == Took::Edited {
                open.regrid();
            }
            took.draws()
        })
    }

    /// What the search of the picker on `id` took of an input method or of
    /// the pointer: its grid follows what was typed; a paste is asked for.
    pub fn took(&mut self, id: window::Id, took: Took) {
        match took {
            Took::Edited => {
                if let Some(open) = self.open_mut(id) {
                    open.regrid();
                }
            }
            Took::Paste => paste(id),
            Took::No | Took::Same | Took::Moved => {}
        }
    }
}

impl Surface for Lent<Picker> {
    type Message = Come;

    fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        self.get_mut().update(come, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        self.read(|p| p.is_surface(id))?
            .then(|| view::view(self, id, words(), context.look.glass))
    }
}

/// The clipboard's text, on one line, into the field of the picker on
/// `id` at its caret, read off the loop and handed to it.
fn paste(id: window::Id) {
    crate::paste::request(crate::paste::Clean::Spaced, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        crate::surfaces::ask(Asked::Emoji(Come::Pasted(id, pasted.to_owned())));
    });
}

/// The open picker for screen readers: a dialog named "Emoji" holding
/// what its controls declared (the search, the tone's button, the tabs,
/// the emoji near the grid's view), the selected emoji holding the focus.
pub fn a11y(seen: &Seen) -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    let rows = crate::a11y::rows::of(seen.surface)?;
    let dialog = Node::new("dialog", Role::Dialog, "Emoji").children(rows.nodes);
    let mut window = Window::new("emoji", "Emoji");
    window.nodes = vec![dialog];
    window.active = seen.holding;
    window.focus = Some(seen.focus.clone());
    window.size = Some((seen.size.0 as f32, seen.size.1 as f32));
    Some(window)
}

/// A screen reader clicks control `key` of the open picker: an emoji
/// (`emoji:Cell(N)`), a tab, the tone's button or a tone. Asked of the
/// loop.
pub fn a11y_click(key: &str) {
    let arg = |prefix: &str| -> Option<usize> {
        key.strip_prefix(prefix)?.strip_suffix(')')?.parse().ok()
    };
    let clicked = if let Some(at) = arg("emoji:Cell(") {
        Clicked::Cell(at)
    } else if let Some(i) = arg("emoji:Tab(") {
        match Heading::tabs().get(i) {
            Some(heading) => Clicked::Tab(*heading),
            None => return,
        }
    } else if let Some(tone) = arg("emoji:Tone(") {
        Clicked::Tone(tone)
    } else if key == "emoji:Tones" {
        Clicked::Tones
    } else {
        return;
    };
    crate::surfaces::ask(Asked::Emoji(Come::Clicked(clicked)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::Rect;

    #[test]
    fn the_recent_come_first_then_each_group_under_its_heading() {
        let table = data::table();
        let recent: Vec<usize> = ["🐼", "👍"].iter().filter_map(|e| table.find(e)).collect();
        let grid = Grid::of(table, "", &recent);
        assert_eq!(grid.rows[0], Row::Head(Heading::Recent));
        assert_eq!(grid.rows[1], Row::Cells(0..2));
        assert_eq!(grid.rows[2], Row::Head(Heading::Group(Group::Smileys)));
        assert_eq!(table.emoji[grid.flat[0]].text, "🐼");
        assert_eq!(grid.starts.len(), 9);
        // No recent ones: the smileys first.
        let none = Grid::of(table, "", &[]);
        assert_eq!(none.rows[0], Row::Head(Heading::Group(Group::Smileys)));
        assert_eq!(none.lit(0), Some(Heading::Group(Group::Smileys)));
        // A search: a flat grid.
        let found = Grid::of(table, "chat", &recent);
        assert!(found.starts.is_empty());
        assert!(found.rows.iter().all(|r| matches!(r, Row::Cells(_))));
    }

    #[test]
    fn the_arrows_go_across_and_down_the_columns_past_the_headings() {
        let table = data::table();
        let recent: Vec<usize> = (0..3).collect();
        let grid = Grid::of(table, "", &recent);
        // Rows: RECENT, [0..3], SMILEYS, [3..11], …
        assert_eq!(grid.step(1, Arrow::Down), 4);
        assert_eq!(grid.step(2, Arrow::Down), 5);
        assert_eq!(grid.step(10, Arrow::Up), 2);
        assert_eq!(grid.step(0, Arrow::Left), 0);
        assert_eq!(grid.step(2, Arrow::Right), 3);
        assert_eq!(grid.lit(0), Some(Heading::Recent));
        assert_eq!(grid.lit(3), Some(Heading::Group(Group::Smileys)));
    }

    #[test]
    fn it_opens_under_the_caret_above_it_without_room_and_stays_on_the_screen() {
        let size = (504, 606);
        let screen = (1920.0, 1080.0);
        let caret = |x, y| {
            EmojiAt::Caret(Rect {
                x,
                y,
                width: 1,
                height: 20,
            })
        };
        assert_eq!(place(caret(300, 200), size, screen), (300, 226));
        // No room under it: above it.
        assert_eq!(place(caret(300, 700), size, screen), (300, 700 - 6 - 606));
        // Near the right edge: pushed in.
        assert_eq!(place(caret(1800, 100), size, screen), (1920 - 8 - 504, 126));
        let window = EmojiAt::Window(Rect {
            x: 100,
            y: 100,
            width: 800,
            height: 700,
        });
        assert_eq!(place(window, size, screen), (248, 147));
        assert_eq!(place(EmojiAt::Screen, size, screen), (708, 237));
    }

    /// The picker alone (#773): `Super+.` opens it, a key from elsewhere
    /// searches as its view says, `Shift+Enter` inserts and keeps it open
    /// until the emoji is in, `Escape` closes it and its tick takes the
    /// surface away.
    #[test]
    fn the_picker_alone_searches_inserts_keeps_and_goes() {
        use keyboard::{Key, Modifiers, key::Named};
        let mut picker = Picker::default();
        let now = Instant::now();
        let effects = picker.update(
            Come::Toggle {
                output: "eDP-1".into(),
                screen: (1920, 1080),
                at: EmojiAt::Screen,
            },
            now,
        );
        let Some(Effect::Emoji { id, .. }) = effects.first() else {
            panic!("its surface opened: {effects:?}");
        };
        let id = *id;
        let words = Words::new(Lang::English);
        let shows = |p: &Picker| p.open_on(id).map(|o| view::shows(o, words));
        let all = shows(&picker).expect("open");
        let typed = Pressed {
            key: Key::Character("c".into()),
            modifiers: Modifiers::empty(),
            text: Some("c".into()),
        };
        picker.update(Come::Key(typed), now);
        assert_ne!(
            shows(&picker).expect("open"),
            all,
            "the grid follows the search"
        );
        let enter = Key::Named(Named::Enter);
        let effects = picker
            .key_on(id, &enter, Modifiers::SHIFT, None, now)
            .expect("its key");
        assert!(effects.contains(&Effect::Keyboard(id, KeyboardInteractivity::None)));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Tell(ToCompositor::Emoji { keep: true, .. })))
        );
        assert_eq!(picker.open_on(id).map(|o| o.holding), Some(false));
        let effects = picker.update(Come::Inserted, now);
        assert!(effects.contains(&Effect::Keyboard(id, KeyboardInteractivity::Exclusive)));
        let escape = Key::Named(Named::Escape);
        picker.key_on(id, &escape, Modifiers::empty(), None, now);
        assert!(picker.open_on(id).is_none() && picker.is_surface(id));
        let due = picker.due().expect("a tick due");
        assert_eq!(picker.update(Come::Tick, due), vec![Effect::Remove(id)]);
        assert!(!picker.is_surface(id));
    }
}
