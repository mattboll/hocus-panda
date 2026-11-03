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
//! shadow and moves it in and out, as a popover's. As for the launcher,
//! nothing goes through iced's messages: keys and the pointer change its
//! state and draw its surface alone ([`crate::popover::effects`] carries
//! out the rest). The compositor inserts the emoji (`emoji insert TEXT`).
//!
//! The emoji's data ([`data`]) are read once; the recent ones and the tone
//! are kept on disk ([`store`]).

pub mod data;
pub mod draw;
pub mod store;

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{EMOJI_NAMESPACE, EmojiAt, POPOVER_CLOSE, ToCompositor};
use iced::{keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use self::data::Group;
use crate::{clock::time::Lang, popover::Effect};

/// The grid's columns.
pub const COLUMNS: usize = 8;
/// The characters the search keeps at most.
const QUERY_MOST: usize = 100;
/// The margin kept to the screen's edges, and between the caret and the
/// picker (logical pixels).
const EDGE: f32 = 8.0;
const UNDER: f32 = 4.0 * draw::ZOOM;

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
    pub scale: f32,
    /// Where it is asked to open, and the screen's size.
    pub at: EmojiAt,
    pub screen: (f32, f32),
    /// Its margins from the screen's top left corner, as last asked.
    pub place: (i32, i32),
    pub query: String,
    pub grid: Grid,
    /// The place selected, in [`Grid::flat`], and the first row shown.
    pub selected: usize,
    pub offset: usize,
    pub tones_open: bool,
    /// What the pointer is over and pressed, and where it is.
    pub hover: Option<draw::Target>,
    pub pressed: Option<draw::Target>,
    pub pointer: Option<iced::Point>,
    /// It holds the keyboard; not while an emoji inserted with the picker
    /// kept open goes in.
    pub holding: bool,
}

impl Open {
    fn regrid(&mut self) {
        let table = data::table();
        let recent = recent_of(table);
        self.grid = Grid::of(table, &self.query, &recent);
        self.selected = 0;
        self.offset = 0;
    }

    /// The emoji of place `at`, as inserted: with the tone chosen.
    pub fn emoji(&self, at: usize) -> Option<(&'static data::Emoji, String)> {
        let emoji = data::table().emoji.get(*self.grid.flat.get(at)?)?;
        Some((emoji, emoji.with_tone(store::kept().tone)))
    }

    /// Its size, logical pixels.
    pub fn size(&self) -> (u32, u32) {
        draw::size(self.tones_open)
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

/// The picker, and its surfaces fading out until they go.
#[derive(Debug, Default)]
pub struct Picker {
    pub phase: Phase,
    closing: Vec<(window::Id, Instant)>,
}

static PICKER: Mutex<Option<Picker>> = Mutex::new(None);

/// `with` on the picker.
pub fn with<T>(with: impl FnOnce(&mut Picker) -> T) -> T {
    let mut picker = PICKER.lock().unwrap_or_else(PoisonError::into_inner);
    with(picker.get_or_insert_with(Picker::default))
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Reads what is kept, and the table.
pub fn start() {
    store::start();
}

/// Its surface draws again.
pub fn wake() {
    crate::popover::send(vec![Effect::Wake(EMOJI_NAMESPACE)]);
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
}

/// Whether surface `id` is the picker's, open or fading out.
pub fn is_surface(id: window::Id) -> bool {
    with(|p| p.open_on(id).is_some() || p.closing.iter().any(|(c, _)| *c == id))
}

/// `Super+.` (the compositor's line): it opens on `output` at `at`, or
/// closes when it is open.
pub fn toggle(output: String, scale: u32, screen: (i32, i32), at: EmojiAt) {
    let now = Instant::now();
    let opened = with(|p| {
        let mut effects = Vec::new();
        if matches!(p.phase, Phase::Open(_)) {
            p.close(now, &mut effects);
            return (effects, false);
        }
        let surface = window::Id::unique();
        let screen = (screen.0 as f32, screen.1 as f32);
        let mut open = Open {
            surface,
            scale: (scale as f32 / 120.0).max(0.5),
            at,
            screen,
            place: (0, 0),
            query: String::new(),
            grid: Grid::default(),
            selected: 0,
            offset: 0,
            tones_open: false,
            hover: None,
            pressed: None,
            pointer: None,
            holding: true,
        };
        open.regrid();
        let size = open.size();
        open.place = place(at, size, screen);
        tracing::debug!(output, ?at, place = ?open.place, "the emoji picker opens");
        effects.push(Effect::Emoji {
            id: surface,
            output: output.clone(),
            size,
            margin: (open.place.1, 0, 0, open.place.0),
        });
        p.phase = Phase::Open(Box::new(open));
        (effects, true)
    });
    let (effects, opened) = opened;
    if opened {
        // One at a time: the bar's popover and the launcher close.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        crate::launcher::close();
    }
    crate::popover::send(effects);
}

/// A press on none of the shell's surfaces: it closes.
pub fn outside() {
    close();
}

pub fn close() {
    let effects = with(|p| {
        let mut effects = Vec::new();
        p.close(Instant::now(), &mut effects);
        effects
    });
    if !effects.is_empty() {
        crate::popover::send(effects);
    }
}

/// The emoji inserted with the picker kept open is in: it takes the
/// keyboard back.
pub fn inserted() {
    let effects = with(|p| match &mut p.phase {
        Phase::Open(open) if !open.holding => {
            open.holding = true;
            vec![
                Effect::Keyboard(open.surface, KeyboardInteractivity::Exclusive),
                Effect::Wake(EMOJI_NAMESPACE),
            ]
        }
        _ => Vec::new(),
    });
    crate::popover::send(effects);
}

/// When [`tick`] has something to do.
pub fn deadline() -> Option<Instant> {
    with(|p| p.closing.iter().map(|(_, at)| *at).min())
}

/// What is due at `now`: the surfaces faded out go.
pub fn tick(now: Instant) -> Vec<Effect> {
    with(|p| {
        let mut effects = Vec::new();
        p.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        effects
    })
}

/// `text` chosen (`keep`: the picker stays open): the recent ones take it,
/// and the compositor inserts it.
pub fn chosen(emoji: &data::Emoji, text: String, keep: bool, effects: &mut Vec<Effect>) {
    tracing::debug!(keep, "an emoji chosen");
    store::change(|k| k.used(emoji.text));
    effects.push(Effect::Tell(ToCompositor::Emoji { keep, text }));
}

/// Place `at` of the picker on `id` is chosen: inserted, and the picker
/// closes unless `keep`.
pub fn act(id: window::Id, at: usize, keep: bool) {
    let effects = with(|p| {
        let mut effects = Vec::new();
        let Some(open) = p.open_mut(id) else {
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
            p.close(Instant::now(), &mut effects);
        }
        chosen(emoji, text, keep, &mut effects);
        effects
    });
    crate::popover::send(effects);
}

/// The tones' row opens or closes, or tone `tone` is chosen (it closes).
pub fn tones(id: window::Id, tone: Option<usize>) {
    let effects = with(|p| {
        let open = p.open_mut(id)?;
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
        Some(vec![Effect::Resize { id, size, margin }])
    });
    if let Some(effects) = effects {
        crate::popover::send(effects);
    }
}

/// A tab is chosen: the grid goes to its heading, the search cleared.
pub fn tab(id: window::Id, heading: Heading) {
    with(|p| {
        let Some(open) = p.open_mut(id) else {
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
            open.offset = row.min(draw::most_offset(&open.grid, open.tones_open));
            if let Some(Row::Cells(range)) = open.grid.rows.get(row + 1) {
                open.selected = range.start;
            }
        } else {
            open.offset = 0;
        }
    });
}

/// A key on the open picker on `id`. Whether it was its.
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
        Act(usize, bool),
        Draw,
    }
    let then = with(|p| {
        let open = p.open_mut(id)?;
        let arrow = |arrow: Arrow, open: &mut Open| {
            open.selected = open.grid.step(open.selected, arrow);
            open.offset =
                draw::keep_in_view(&open.grid, open.selected, open.offset, open.tones_open);
            Then::Draw
        };
        Some(match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) => {
                if open.grid.flat.is_empty() {
                    Then::Nothing
                } else {
                    Then::Act(open.selected, modifiers.shift())
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
                        let kept = open.query.trim_end().rfind(' ').map_or(0, |at| at + 1);
                        open.query.truncate(kept);
                    } else {
                        open.query.pop();
                    }
                    open.regrid();
                    Then::Draw
                }
            }
            Key::Named(Named::Tab) => Then::Nothing,
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
                    open.regrid();
                    Then::Draw
                }
                _ => Then::Nothing,
            },
        })
    });
    match then {
        None => false,
        Some(Then::Nothing | Then::Draw) => true,
        Some(Then::Close) => {
            close();
            true
        }
        Some(Then::Act(at, keep)) => {
            act(id, at, keep);
            true
        }
    }
}

/// A key that reached another of the shell's surfaces while the picker is
/// open (layershellev hands keys to the surface last created or clicked):
/// its, and its surface draws again. Whether it was taken.
pub fn key(key: &keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> bool {
    let Some(id) = with(|p| match &p.phase {
        Phase::Open(open) if open.holding => Some(open.surface),
        _ => None,
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
}
