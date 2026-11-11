//! The clipboard's history (5.6, variant A: from the bar), opened by the
//! footer's clipboard (a 150 ms hover or a click) or `Super+V`: 360 px
//! wide, its bottom on the bar's, 12 px inside. In this order:
//!
//! - "Presse-papiers" / "Clipboard" and "Tout effacer" / "Clear all", a
//!   ghost button, which keeps the pinned items;
//! - the search, a 36 px field in its green border: a substring of the
//!   items' text or application, accents and case ignored. It holds the
//!   keyboard when the popover does: opened by `Super+V` or a click, or
//!   once clicked in after a hover;
//! - the list, 430 px, its rows 44 px: "ÉPINGLÉS" / "PINNED" (when there
//!   are some), then "RÉCENTS" / "RECENT", newest first, each its kind's
//!   30 px tile (`>_`, `↗`, `Aa`, `/`, a colour's swatch, an image's
//!   thumbnail), its content on one line, cut (Martian Mono 11.5 px for
//!   code, paths and colours, else Manrope 600 13 px), and "App · il y a 4
//!   min" under it. The selected row, the first one at first, has the
//!   green border and tint, and its pin and ✕ (the pin amber when it is
//!   pinned); the pointer selects the row it is over. The wheel scrolls it.
//! - "Entrée colle · Maj+Entrée copie · Suppr retire".
//!
//! `Enter` or a click pastes the selected item into the window that had
//! the keyboard (`Ctrl+V`, `Ctrl+Shift+V` in a terminal: the
//! compositor's), `Shift+Enter` only copies it, both closing the history;
//! `Delete` or ✕ removes it, the pin pins it. `Esc` closes. Nothing goes
//! through iced's messages: the history's changes wake its surface.

use hocus_words::shell::popover_clipboard as txt;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_a11y::Role;
use iced::{
    Color, ContentFit, Element, Font, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    alignment::{Horizontal, Vertical},
    keyboard, mouse,
    widget::{
        Id, Space, Stack, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, image, mouse_area, pin, row, scrollable, space,
    },
    window,
};
use iced_layershell::reexport::KeyboardInteractivity;
use jiff::Zoned;

use super::{Effect, Event as Seen, Pressed, event, kit};
use crate::theme::Ink;
use crate::{
    clipboard::{self, Item, Kind},
    clock::time::Lang,
    field::{
        self, Entry, Took,
        entry::Pasted,
        view::{Look, Style},
    },
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    notifications::history::{Ago, ago},
    popover::kit::MANROPE_SEMIBOLD,
    ui::{
        self, Around, Later, control,
        field::Field,
        notched::Notched,
        parts::{drawn, gap, spaced_by},
        style::{self, boxed, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ---------------------------------------------------

pub const WIDTH: u32 = 360;
const W: f32 = WIDTH as f32;
const BORDER: f32 = 1.0;
const RADIUS: f32 = 14.0;
/// 12 px inside, 8 px between its parts.
const PAD: f32 = 12.0;
const LEFT: f32 = BORDER + PAD;
const RIGHT: f32 = W - BORDER - PAD;
const GAP: f32 = 8.0;
/// The header: its title 15 px bold; "Clear all" a 26 px ghost button,
/// 9 px in, radius 8, 12 px 600.
const HEADER_TOP: f32 = BORDER + PAD;
const HEADER: f32 = 26.0;
const TITLE: f32 = 15.0;
const GHOST_TEXT: f32 = 12.0;
const GHOST_PAD: f32 = 9.0;
const GHOST_RADIUS: f32 = 8.0;
/// The search: 36 px, radius 10, 10 px in; its magnifier 15 px, 8 px
/// before what is typed (13.5 px).
const FIELD_TOP: f32 = HEADER_TOP + HEADER + GAP;
const FIELD: f32 = 36.0;
const FIELD_RADIUS: f32 = 10.0;
const FIELD_PAD: f32 = 10.0;
const MAGNIFIER: f32 = 15.0;
const FIELD_GAP: f32 = 8.0;
const QUERY: f32 = 13.5;
/// What is typed, at most.
const QUERY_MOST: usize = 200;
/// The list: 430 px; its labels 22 px, 6 px in, 10.5 px bold spaced
/// 0.08 em; its rows 44 px, radius 10, everything 2 px apart.
const LIST_TOP: f32 = FIELD_TOP + FIELD + GAP;
const LIST: f32 = 430.0;
const HEAD: f32 = 22.0;
const HEAD_PAD: f32 = 6.0;
const HEAD_TEXT: f32 = 10.5;
const HEAD_SPACING: f32 = 0.08;
const ROW: f32 = 44.0;
const ROW_GAP: f32 = 2.0;
const ROW_RADIUS: f32 = 10.0;
/// A row's tile: 30 px, 9 px in (its border and 8 px), radius 8; inside,
/// a swatch or a thumbnail 3 px in, radius 6, or its glyph 11 px bold.
const TILE: f32 = 30.0;
const TILE_IN: f32 = 9.0;
const TILE_RADIUS: f32 = 8.0;
const TILE_PAD: f32 = 3.0;
const INNER_RADIUS: f32 = 6.0;
const GLYPH: f32 = 11.0;
/// Its text 10 px after the tile: its content 13 px (code 11.5 px) over
/// "App · il y a 4 min" (10.5 px), 1 px apart.
const TEXT_GAP: f32 = 10.0;
const CONTENT: f32 = 13.0;
const MONO_CONTENT: f32 = 11.5;
const META: f32 = 10.5;
const LINES_GAP: f32 = 1.0;
/// Its pin and ✕: 24 px, 2 px apart, 5 px from its right (its border
/// and 4 px), radius 6; the pin's glyph 14 px, the ✕ 11 px.
const TOOL: f32 = 24.0;
const TOOL_GAP: f32 = 2.0;
const TOOLS_RIGHT: f32 = 5.0;
const TOOL_RADIUS: f32 = 6.0;
const PIN: f32 = 14.0;
const CROSS: f32 = 11.0;
/// What the content has: from after the tile to the tools.
const TEXT_X: f32 = TILE_IN + TILE + TEXT_GAP;
const TEXT_ROOM: f32 = RIGHT - LEFT - TEXT_X - (TOOLS_RIGHT + 2.0 * TOOL + TOOL_GAP);
/// Nothing to list: 30 px down, 13 px.
const EMPTY_TOP: f32 = 30.0;
const EMPTY: f32 = 13.0;
/// The keys, 10.5 px, under the list.
const FOOTER_TOP: f32 = LIST_TOP + LIST + GAP;
const FOOTER: f32 = 10.5;
/// A notch of the wheel scrolls a row.
const NOTCH: f32 = ROW + ROW_GAP;

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// The mockup's tint, opaque without glass.
const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.96));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.98));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const GHOST_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const FIELD_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const FIELD_RIM: Ink = Ink::Accent(0.45);
const QUIET: Ink = Ink::Dark(rgb(0x7C8698));
const SELECTED_RIM: Ink = Ink::Accent(0.55);
const SELECTED_FILL: Ink = Ink::Accent(0.13);
const TILE_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.07));
const AMBER: Ink = Ink::Dark(rgb(0xF5C46E));
const KEYS: Ink = Ink::Dark(rgb(0x6A7384));

const MONO: Font = Font::with_name("Martian Mono");

/// Its height: the header, the field, the list and the keys.
pub fn height() -> u32 {
    (FOOTER_TOP + FOOTER * LINE + PAD + BORDER).round() as u32
}

// --- Words ------------------------------------------------------------------------

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

    pub fn title(self) -> &'static str {
        self.pick(txt::CLIPBOARD)
    }

    fn clear(self) -> &'static str {
        self.pick(txt::CLEAR_ALL)
    }

    fn placeholder(self) -> &'static str {
        self.pick(txt::SEARCH_WHAT_YOU_COPIED)
    }

    fn head(self, head: Head) -> &'static str {
        match head {
            Head::Pinned => self.pick(txt::PINNED),
            Head::Recent => self.pick(txt::RECENT),
            Head::Off => self.pick(txt::HISTORY_OFF_SETTINGS),
        }
    }

    fn keys(self) -> &'static str {
        self.pick(txt::ENTER_PASTES_SHIFT_ENTER_COPIES_DELETE)
    }

    fn nothing(self) -> &'static str {
        self.pick(txt::NOTHING_COPIED_YET)
    }

    fn not_found(self, query: &str) -> String {
        txt::nothing_for(self.lang, query)
    }

    /// Where a capture comes from (5.8b).
    fn capture(self) -> &'static str {
        self.pick(txt::SCREENSHOT)
    }

    fn image(self, width: u32, height: u32) -> String {
        format!("Image · {width}\u{a0}×\u{a0}{height}")
    }

    fn ago(self, ago: Ago) -> String {
        match ago {
            Ago::Now => txt::NOW.get(self.lang).to_owned(),
            Ago::Minutes(n) => txt::min_ago(self.lang, n),
            Ago::Hours(n) => txt::h_ago(self.lang, n),
            Ago::Yesterday => txt::YESTERDAY.get(self.lang).to_owned(),
            Ago::Days(n) => txt::days_ago(self.lang, n),
        }
    }

    /// The ✕'s name.
    fn remove(self) -> &'static str {
        self.pick(txt::REMOVE)
    }

    /// The pin's name, for the log.
    fn pin(self, pinned: bool) -> &'static str {
        if pinned {
            self.pick(txt::UNPIN)
        } else {
            self.pick(txt::PIN)
        }
    }
}

// --- Laying it out ----------------------------------------------------------------

/// What can be acted on in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Clear,
    Field,
    Row(u64),
    Pin(u64),
    Remove(u64),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("clipboard:{self:?}")
    }
}

/// The list's scrollable.
const LIST_ID: &str = "clipboard:list";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    Pinned,
    Recent,
    /// The history is off (`[clipboard] history = false`, 6.17): said
    /// first, under the search, the pinned items under it.
    Off,
}

/// What a row's tile shows.
#[derive(Debug, Clone)]
enum Tile {
    Glyph(&'static str),
    Swatch(Color),
    Thumbnail(iced::advanced::image::Handle),
}

/// An item, laid out: its top in the list's content.
#[derive(Debug, Clone)]
struct Row {
    key: u64,
    top: f32,
    tile: Tile,
    kind: Kind,
    /// Its content on one line, cut; under it, its application and when.
    line: String,
    meta: String,
    pinned: bool,
}

/// The history, laid out for a query.
#[derive(Debug, Clone)]
struct Laid {
    clear: Rectangle,
    heads: Vec<(Head, f32)>,
    rows: Vec<Row>,
    content: f32,
    /// What it says without rows.
    empty: Option<String>,
}

/// A text's width at a size in a font.
type Measure<'a> = &'a mut dyn FnMut(&str, f32, Font) -> f32;

/// `text` as the search compares it: in lower case, without its accents.
fn fold(text: &str) -> String {
    crate::launcher::search::fold(text)
}

/// Where an item comes from: a capture (5.8b), else its application.
fn source(app: &str, words: Words, names: &mut HashMap<String, String>) -> String {
    if app == hocus_link::CAPTURE_APP {
        return words.capture().to_owned();
    }
    app_name(app, names)
}

/// An application's name, as its desktop entry gives it (by its id, in
/// any case, or its last part), else its id.
fn app_name(app: &str, names: &mut HashMap<String, String>) -> String {
    if app.is_empty() {
        return String::new();
    }
    if let Some(name) = names.get(app) {
        return name.clone();
    }
    let catalog = crate::launcher::apps::catalog();
    let lower = app.to_lowercase();
    let last = lower.rsplit('.').next().unwrap_or_default().to_owned();
    let found = catalog.apps.iter().find(|a| {
        let id = a.entry.id.to_lowercase();
        id == lower || id.rsplit('.').next() == Some(last.as_str())
    });
    let name = found.map_or_else(|| app.to_owned(), |a| a.entry.name.clone());
    // Kept once the applications are read.
    if !catalog.apps.is_empty() {
        names.insert(app.to_owned(), name.clone());
    }
    name
}

/// `text` on one line: its runs of white space made one space.
fn one_line(text: &str) -> String {
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        // Enough for the row, which shows far less.
        if line.len() > 400 {
            break;
        }
    }
    line
}

/// The rows' application names and cut lines, kept between layouts.
#[derive(Debug, Default)]
struct Cache {
    names: HashMap<String, String>,
    lines: HashMap<u64, String>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// `items` laid out for `query`, at `now`, in `words`; `off`: the history
/// is off.
fn lay_out(
    items: &[Item],
    off: bool,
    query: &str,
    words: Words,
    now: &Zoned,
    cache: &mut Cache,
    measure: Measure<'_>,
) -> Laid {
    let width = measure(words.clear(), GHOST_TEXT, MANROPE_SEMIBOLD) + 2.0 * GHOST_PAD;
    let clear = Rectangle::new(
        Point::new(RIGHT - width, HEADER_TOP),
        Size::new(width, HEADER),
    );
    let wanted = fold(query.trim());
    let shown: Vec<(&Item, String)> = items
        .iter()
        .map(|item| (item, source(&item.app, words, &mut cache.names)))
        .filter(|(item, app)| {
            wanted.is_empty() || fold(&format!("{} {app}", item.text)).contains(&wanted)
        })
        .collect();
    let mut heads = Vec::new();
    let mut rows = Vec::new();
    let mut y = 0.0;
    if off {
        heads.push((Head::Off, y));
        y += HEAD + ROW_GAP;
    }
    for (head, pinned) in [(Head::Pinned, true), (Head::Recent, false)] {
        let mut first = true;
        for (item, app) in shown.iter().filter(|(i, _)| i.pinned == pinned) {
            if first {
                heads.push((head, y));
                y += HEAD + ROW_GAP;
                first = false;
            }
            let line = match cache.lines.get(&item.key) {
                Some(line) => line.clone(),
                None => {
                    let full = match &item.picture {
                        Some(p) => words.image(p.width, p.height),
                        None => one_line(&item.text),
                    };
                    let (size, font) = if item.kind.mono() {
                        (MONO_CONTENT, MONO)
                    } else {
                        (CONTENT, MANROPE_SEMIBOLD)
                    };
                    let cut = kit::elide(&full, TEXT_ROOM, |t| measure(t, size, font));
                    cache.lines.insert(item.key, cut.clone());
                    cut
                }
            };
            let when = words.ago(ago(item.at, now));
            let meta = if app.is_empty() {
                when
            } else {
                format!("{app} · {when}")
            };
            let meta = kit::elide(&meta, TEXT_ROOM, |t| measure(t, META, MANROPE));
            let tile = match (item.kind, &item.picture) {
                (_, Some(p)) => Tile::Thumbnail(p.handle.clone()),
                (Kind::Colour([r, g, b, a]), None) => {
                    Tile::Swatch(Color::from_rgba8(r, g, b, f32::from(a) / 255.0))
                }
                (kind, None) => Tile::Glyph(kind.glyph().unwrap_or("Aa")),
            };
            rows.push(Row {
                key: item.key,
                top: y,
                tile,
                kind: item.kind,
                line,
                meta,
                pinned: item.pinned,
            });
            y += ROW + ROW_GAP;
        }
    }
    let content = (y - ROW_GAP).max(0.0);
    let empty = (rows.is_empty() && !(off && wanted.is_empty())).then(|| {
        if wanted.is_empty() {
            words.nothing().to_owned()
        } else {
            words.not_found(query.trim())
        }
    });
    Laid {
        clear,
        heads,
        rows,
        content,
        empty,
    }
}

impl Laid {
    fn most_scroll(&self) -> f32 {
        (self.content - LIST).max(0.0)
    }

    /// For the log (and the tests): the sections and their rows, each its
    /// kind, its line (its first 32 characters) and its details.
    fn said(&self, words: Words, selected: Option<u64>) -> String {
        if let Some(empty) = &self.empty {
            return empty.clone();
        }
        let mut said = Vec::new();
        let mut rows = self.rows.iter().peekable();
        for (i, (head, _)) in self.heads.iter().enumerate() {
            said.push(format!("{}:", words.head(*head)));
            let next = self.heads.get(i + 1).map(|(_, top)| *top);
            while let Some(row) = rows.next_if(|r| next.is_none_or(|top| r.top < top)) {
                let line: String = row.line.chars().take(32).collect();
                let pinned = if row.pinned { " (pinned)" } else { "" };
                let selected = if selected == Some(row.key) {
                    " (selected)"
                } else {
                    ""
                };
                said.push(format!(
                    "[{}] «{line}» {}{pinned}{selected}",
                    row.kind.word(),
                    row.meta
                ));
            }
        }
        said.join(" ")
    }
}

// --- The panel --------------------------------------------------------------------

/// The history on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What it keeps while it is open: it opens on an empty search.
#[derive(Debug)]
struct Ui {
    /// What it was laid out from: the history's version, the query, the
    /// minute.
    from: Option<(u64, String, i64)>,
    laid: Option<Laid>,
    /// The search field (#771).
    query: Entry,
    selected: Option<u64>,
    /// How far the list is scrolled, as it said or was told.
    scroll: f32,
    logged: String,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            from: None,
            laid: None,
            query: Entry::new(field::Kind::Line, QUERY_MOST),
            selected: None,
            scroll: 0.0,
            logged: String::new(),
        }
    }
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);
static LATER: Later = Later::new();

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The search, to the kit's field.
fn query(with: &mut dyn FnMut(&mut Entry)) {
    ui(|ui| with(&mut ui.query));
}

/// What a paste read for the search field.
static PASTED: Pasted = Pasted::new();

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button pressed, or `Enter` on it.
    Act(Target),
    /// The pointer came over an item's row: it is selected.
    Select(u64),
    /// What the search took.
    Took(Took),
    /// The list scrolled to there.
    Scrolled(f32),
    /// A press anywhere on the popover.
    Pressed,
    /// A key of its own: `Enter`, `Delete`, the arrows, `Tab`.
    Key(Pressed),
    /// The history changed under the view, or a paste came.
    Stale,
}

/// The history on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (id, words, glass) = (panel.id, panel.words, panel.glass);
    let minute = || crate::notifications::wall().timestamp().as_second() / 60;
    Around::new(
        id,
        build(id, words),
        move |m| handle(id, words, m),
        Msg::Stale,
    )
    .rebuild(move || build(id, words))
    .stale_when(move || ui(|ui| ui.from.as_ref().is_some_and(|f| f.2 != minute())))
    .keys(keys)
    .later(&LATER)
    .press(Msg::Pressed)
    .on(iced::widget::canvas(Tint(glass))
        .width(Length::Fill)
        .height(Length::Fill))
}

/// Its keys before the field's: `Enter` pastes, `Shift+Enter` copies,
/// `Delete` removes, the arrows select, `Tab` stays.
fn keys(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    matches!(
        pressed.key,
        Key::Named(Named::Enter | Named::Delete | Named::ArrowDown | Named::ArrowUp | Named::Tab)
    )
    .then(|| Msg::Key(pressed.clone()))
}

/// Its content, as the history is now.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let keyboard = super::with(|c| c.holds_keyboard(id));
    ui(|ui| {
        refresh(ui, words);
        let laid = ui.laid.clone();
        match laid {
            Some(laid) => content(&laid, words, ui, keyboard),
            None => Space::new().into(),
        }
    })
}

fn content<'a>(laid: &Laid, words: Words, ui: &Ui, keyboard: bool) -> El<'a> {
    let clear = button(
        container(said(words.clear(), GHOST_TEXT, MANROPE_SEMIBOLD, DIM.get()))
            .center(Length::Fill),
    )
    .padding(0)
    .width(laid.clear.width)
    .height(HEADER)
    .style(|_, status| {
        if style::lit(status) {
            style::hovered(GHOST_HOVER.get(), GHOST_RADIUS)
        } else {
            style::clear()
        }
    })
    .on_press(Msg::Act(Target::Clear));
    let header = row![
        said(words.title(), TITLE, MANROPE_BOLD, TEXT.get()),
        space::horizontal(),
        control(clear, Target::Clear.id(), Role::Button, words.clear()),
    ]
    .height(HEADER)
    .align_y(Vertical::Center);
    let magnifier = drawn(MAGNIFIER, MAGNIFIER, |f, _| magnifier(f, Point::ORIGIN));
    let typed = Field {
        with: Box::new(query),
        style: Style::one(QUERY, MANROPE),
        placeholder: words.placeholder().into(),
        look: Look::of(TEXT.get(), QUIET.get()),
        focused: keyboard,
        left: BORDER + FIELD_PAD + MAGNIFIER + FIELD_GAP,
        right: BORDER + FIELD_PAD,
        on_focus: Msg::Stale,
        on_took: Msg::Took,
    }
    .view(FIELD);
    let search = container(Stack::with_children([
        pin(magnifier)
            .x(BORDER + FIELD_PAD)
            .y((FIELD - MAGNIFIER) / 2.0)
            .into(),
        typed,
    ]))
    .height(FIELD)
    .style(|_| container::Style {
        background: Some(FIELD_FILL.get().into()),
        border: iced::Border {
            color: FIELD_RIM.get(),
            width: 1.0,
            radius: FIELD_RADIUS.into(),
        },
        ..container::Style::default()
    });
    let search = control(
        search,
        Target::Field.id(),
        Role::TextField,
        words.placeholder(),
    )
    .node(|n| n.value(ui.query.text().to_owned()))
    .focus(keyboard, Msg::Stale, FIELD_RADIUS)
    .typing();
    // The list: its labels and rows, scrolling past 430 px.
    let mut list = column![].width(RIGHT - LEFT);
    let mut end = 0.0;
    let mut rows = laid.rows.iter().peekable();
    for (i, (head, top)) in laid.heads.iter().enumerate() {
        if *top > end {
            list = list.push(Space::new().height(top - end));
        }
        list = list.push(head_of(words, *head));
        end = top + HEAD;
        let next = laid.heads.get(i + 1).map(|(_, top)| *top);
        while let Some(shown) = rows.next_if(|r| next.is_none_or(|top| r.top < top)) {
            list = list.push(Space::new().height(shown.top - end));
            list = list.push(row_of(words, shown, ui.selected == Some(shown.key)));
            end = shown.top + ROW;
        }
    }
    // In a container: a scrollable lays its content out as wide as its
    // viewport at least.
    let scrolled = scrollable(container(list))
        .id(Id::new(LIST_ID))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(kit::SCROLLBAR)
                .scroller_width(kit::SCROLLBAR)
                .margin(0)
                .spacing(0),
        ))
        .style(|_, _| style::scrollbar(kit::SCROLLBAR))
        .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
        .width(W - BORDER - 2.0 - LEFT)
        .height(LIST);
    let mut middle: Vec<El<'a>> = vec![Notched::new(scrolled, NOTCH).into()];
    if let Some(empty) = &laid.empty {
        middle.push(
            pin(ui::line(
                container(said(empty.clone(), EMPTY, MANROPE, FAINT.get()))
                    .width(RIGHT - LEFT)
                    .height(EMPTY * LINE)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center),
                empty.clone(),
            ))
            .y(EMPTY_TOP)
            .into(),
        );
    }
    let all = column![
        container(header).width(RIGHT - LEFT),
        Space::new().height(GAP),
        container(search).width(RIGHT - LEFT),
        Space::new().height(GAP),
        Stack::with_children(middle).height(LIST),
        Space::new().height(GAP),
        ui::line(
            said(words.keys(), FOOTER, MANROPE, KEYS.get()),
            words.keys()
        ),
    ];
    container(all)
        .padding(Padding {
            top: HEADER_TOP,
            right: 0.0,
            bottom: 0.0,
            left: LEFT,
        })
        .into()
}

/// A section's label, spaced; the history off, its line as it is.
fn head_of<'a>(words: Words, head: Head) -> El<'a> {
    let word = words.head(head);
    let shown: El<'a> = if head == Head::Off {
        said(word, META, MANROPE, DIM.get()).into()
    } else {
        spaced_by(word, HEAD_TEXT, MANROPE_BOLD, QUIET.get(), HEAD_SPACING)
    };
    let mut chars = word.chars();
    let read = chars.next().map_or_else(String::new, |c| {
        c.to_string() + &chars.as_str().to_lowercase()
    });
    ui::line(
        container(shown)
            .padding([0.0, HEAD_PAD])
            .height(HEAD)
            .align_y(Vertical::Center),
        read,
    )
    .into()
}

/// An item's row: its tile, its content over its application and when;
/// selected, its border and tint, its pin and ✕.
fn row_of<'a>(words: Words, shown: &Row, selected: bool) -> El<'a> {
    let tile: El<'a> = match &shown.tile {
        Tile::Glyph(glyph) => container(said(*glyph, GLYPH, MANROPE_BOLD, DIM.get()))
            .center(TILE)
            .into(),
        Tile::Swatch(colour) => {
            let colour = *colour;
            container(
                container(
                    Space::new()
                        .width(TILE - 2.0 * TILE_PAD)
                        .height(TILE - 2.0 * TILE_PAD),
                )
                .style(move |_| boxed(colour, INNER_RADIUS)),
            )
            .center(TILE)
            .into()
        }
        Tile::Thumbnail(handle) => container(
            image(handle.clone())
                .width(TILE - 2.0 * TILE_PAD)
                .height(TILE - 2.0 * TILE_PAD)
                .content_fit(ContentFit::Fill),
        )
        .center(TILE)
        .into(),
    };
    let tile = container(tile).style(|_| boxed(TILE_FILL.get(), TILE_RADIUS));
    let (size, font) = if shown.kind.mono() {
        (MONO_CONTENT, MONO)
    } else {
        (CONTENT, MANROPE_SEMIBOLD)
    };
    let texts = column![
        container(said(shown.line.clone(), size, font, TEXT.get()))
            .height(CONTENT * LINE)
            .align_y(Vertical::Center),
        Space::new().height(LINES_GAP),
        said(shown.meta.clone(), META, MANROPE, QUIET.get()),
    ];
    let key = shown.key;
    let face = row![gap(TILE_IN), tile, gap(TEXT_GAP), texts]
        .height(ROW)
        .align_y(Vertical::Center);
    let pressed = button(face)
        .padding(0)
        .width(Length::Fill)
        .height(ROW)
        .style(move |_, _| {
            if selected {
                iced::widget::button::Style {
                    background: Some(SELECTED_FILL.get().into()),
                    border: iced::Border {
                        color: SELECTED_RIM.get(),
                        width: 1.0,
                        radius: ROW_RADIUS.into(),
                    },
                    ..iced::widget::button::Style::default()
                }
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(Target::Row(key)));
    let item = control(
        pressed,
        Target::Row(key).id(),
        Role::ListItem,
        shown.line.clone(),
    )
    .node(|n| n.describe(shown.meta.clone()).selected(selected));
    // Its pin and ✕, drawn while it is selected (the pointer over them
    // selects it first).
    let pinned = shown.pinned;
    let tool = |target: Target, face: El<'a>, role: Role, name: &str| -> El<'a> {
        let pressed = button(face)
            .padding(0)
            .width(TOOL)
            .height(TOOL)
            .style(move |_, status| {
                if selected && style::lit(status) {
                    style::hovered(GHOST_HOVER.get(), TOOL_RADIUS)
                } else {
                    style::clear()
                }
            })
            .on_press(Msg::Act(target));
        control(pressed, target.id(), role, name)
            .node(|n| {
                if role == Role::ToggleButton {
                    n.checked(pinned)
                } else {
                    n
                }
            })
            .into()
    };
    let pin_face = drawn(TOOL, TOOL, move |f, s| {
        if selected {
            pin_glyph(f, Point::new(s.width / 2.0, s.height / 2.0), pinned);
        }
    });
    let cross: El<'a> = if selected {
        container(said("✕", CROSS, MANROPE, FAINT.get()))
            .center(TOOL)
            .into()
    } else {
        Space::new().width(TOOL).height(TOOL).into()
    };
    let tools = row![
        tool(
            Target::Pin(key),
            pin_face,
            Role::ToggleButton,
            words.pin(false)
        ),
        gap(TOOL_GAP),
        tool(Target::Remove(key), cross, Role::Button, words.remove()),
    ];
    let whole = Stack::with_children([
        item.into(),
        pin(tools)
            .x(RIGHT - LEFT - TOOLS_RIGHT - 2.0 * TOOL - TOOL_GAP)
            .y((ROW - TOOL) / 2.0)
            .into(),
    ])
    .width(Length::Fill)
    .height(ROW);
    mouse_area(whole).on_enter(Msg::Select(key)).into()
}

/// What it shows, read afresh: laid out again when the history, the
/// query or the minute changed.
fn refresh(ui: &mut Ui, words: Words) {
    let now = crate::notifications::wall();
    let (version, items, off) = clipboard::with(|h| {
        (
            h.version(),
            h.shown(now.timestamp().as_millisecond()),
            !h.rules().history,
        )
    });
    let minute = now.timestamp().as_second() / 60;
    let from = (version, ui.query.text().to_owned(), minute);
    if ui.from.as_ref() == Some(&from) && ui.laid.is_some() {
        log(ui, words);
        return;
    }
    let laid = {
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        let cache = cache.get_or_insert_with(Cache::default);
        // Only what the history holds stays cut.
        cache
            .lines
            .retain(|key, _| items.iter().any(|i| i.key == *key));
        lay_out(
            &items,
            off,
            ui.query.text(),
            words,
            &now,
            cache,
            &mut kit::measure,
        )
    };
    let keys: Vec<u64> = laid.rows.iter().map(|r| r.key).collect();
    if ui.selected.is_none_or(|s| !keys.contains(&s)) {
        ui.selected = keys.first().copied();
    }
    ui.laid = Some(laid);
    ui.from = Some(from);
    log(ui, words);
}

/// What it shows, in the log, when that changed.
fn log(ui: &mut Ui, words: Words) {
    let Some(laid) = &ui.laid else {
        return;
    };
    let said = laid.said(words, ui.selected);
    if said == ui.logged {
        return;
    }
    tracing::debug!(
        title = words.title(),
        typed = ui.query.chars(),
        history = %said,
        "the clipboard's history shows it"
    );
    ui.logged = said;
}

// --- What a message does -------------------------------------------------------

fn handle(id: window::Id, words: Words, message: Msg) {
    match message {
        Msg::Stale => {
            if let Some(pasted) = PASTED.take(id)
                && ui(|ui| ui.query.paste(&pasted))
            {
                typed();
            }
        }
        // Clicked in, a hovered history takes the keyboard.
        Msg::Pressed => event(Seen::Hold),
        Msg::Select(key) => ui(|ui| ui.selected = Some(key)),
        Msg::Scrolled(y) => ui(|ui| ui.scroll = y),
        Msg::Took(took) => match took {
            Took::Edited => typed(),
            Took::Paste => PASTED.ask(id, crate::paste::Clean::Spaced, || {
                super::changed(super::Item::Clipboard);
            }),
            Took::No | Took::Same | Took::Moved => {}
        },
        Msg::Act(target) => act(id, words, target),
        Msg::Key(pressed) => key(id, &pressed),
    }
}

fn act(id: window::Id, words: Words, target: Target) {
    match target {
        Target::Clear => clipboard::clear(),
        Target::Field => {}
        Target::Row(key) => choose(id, key, true),
        Target::Pin(key) => {
            let pinned = clipboard::with(|h| h.get(key).is_some_and(|i| i.pinned));
            tracing::debug!(name = words.pin(pinned), "the clipboard's history's pin");
            clipboard::pin(key);
        }
        Target::Remove(key) => remove(key),
    }
}

/// Item `key` to the clipboard, and pasted (`paste`): the history closes,
/// the keyboard going back to the window that had it.
fn choose(id: window::Id, key: u64, paste: bool) {
    super::send(vec![Effect::Keyboard(id, KeyboardInteractivity::None)]);
    clipboard::choose(key, paste);
    event(Seen::Chosen);
}

/// The rows' keys, from the top.
fn shown_keys(ui: &Ui) -> Vec<u64> {
    ui.laid
        .as_ref()
        .map_or_else(Vec::new, |l| l.rows.iter().map(|r| r.key).collect())
}

/// Item `key` goes: the next one (else the one before) is selected.
fn remove(key: u64) {
    ui(|ui| {
        let keys = shown_keys(ui);
        if ui.selected == Some(key)
            && let Some(at) = keys.iter().position(|k| *k == key)
        {
            ui.selected = keys
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|before| keys.get(before)))
                .copied();
        }
    });
    clipboard::remove(key);
}

fn key(id: window::Id, pressed: &Pressed) {
    use keyboard::{Key, key::Named};
    let (keys, selected) = ui(|ui| (shown_keys(ui), ui.selected));
    let selected = selected.filter(|s| keys.contains(s));
    match &pressed.key {
        Key::Named(Named::Enter) => {
            if let Some(key) = selected {
                choose(id, key, !pressed.modifiers.shift());
            }
        }
        Key::Named(Named::Delete) => {
            if let Some(key) = selected {
                remove(key);
            }
        }
        Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
            let at = selected.and_then(|s| keys.iter().position(|k| *k == s));
            let next = match (at, *arrow == Named::ArrowDown) {
                (Some(at), true) => (at + 1).min(keys.len().saturating_sub(1)),
                (Some(at), false) => at.saturating_sub(1),
                (None, _) => 0,
            };
            ui(|ui| {
                ui.selected = keys.get(next).copied();
                reveal(ui);
            });
        }
        // Nothing else to go to: the field keeps the keyboard.
        _ => {}
    }
}

/// The query changed: the list from its top, its first row selected.
fn typed() {
    ui(|ui| {
        ui.selected = None;
        ui.from = None;
        scroll_to(ui, 0.0);
    });
}

/// The list scrolled to `y`, once its tree is built.
fn scroll_to(ui: &mut Ui, y: f32) {
    if (ui.scroll - y).abs() < 0.5 {
        return;
    }
    ui.scroll = y;
    LATER.push(iced::advanced::widget::operation::scrollable::scroll_to(
        Id::new(LIST_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    ));
}

/// The list scrolled so that the selected row shows, with its section's
/// label when it is the first.
fn reveal(ui: &mut Ui) {
    let (Some(laid), Some(key)) = (&ui.laid, ui.selected) else {
        return;
    };
    let Some(row) = laid.rows.iter().find(|r| r.key == key) else {
        return;
    };
    let first = laid
        .heads
        .iter()
        .any(|(_, top)| (top + HEAD + ROW_GAP - row.top).abs() < 0.5);
    let top = if first {
        row.top - HEAD - ROW_GAP
    } else {
        row.top
    };
    let to = kit::reveal(
        (top, row.top + ROW - top),
        LIST,
        ui.scroll,
        laid.most_scroll(),
    );
    scroll_to(ui, to);
}

/// Its background: its tint (opaque without glass) and its rim, as the
/// mockup has them.
#[derive(Debug, Clone, Copy)]
struct Tint(bool);

impl<M> canvas::Program<M> for Tint {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut f = Frame::new(renderer, size);
        let (top, bottom) = if self.0 {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into());
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top.get())
            .add_stop(1.0, bottom.get());
        f.fill(&shape, canvas::Fill::from(tint));
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (RADIUS - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM.get()).with_width(BORDER),
        );
        vec![f.into_geometry()]
    }
}

/// The magnifier, 15 px from `at`: the mockup's 24 px path.
fn magnifier(f: &mut Frame, at: Point) {
    let k = MAGNIFIER / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(DIM.get()).with_width(2.2 * k)
    };
    f.stroke(&Path::circle(p(10.5, 10.5), 6.5 * k), stroke);
    f.stroke(&Path::line(p(15.5, 15.5), p(20.0, 20.0)), stroke);
}

/// The pin, 14 px around `centre`: the mockup's 24 px path, amber and
/// filled when pinned.
fn pin_glyph(f: &mut Frame, centre: Point, pinned: bool) {
    let k = PIN / 24.0;
    let at = Point::new(centre.x - PIN / 2.0, centre.y - PIN / 2.0);
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let head = Path::new(|b| {
        b.move_to(p(9.0, 3.0));
        b.line_to(p(15.0, 3.0));
        b.line_to(p(14.0, 9.0));
        b.line_to(p(18.0, 13.0));
        b.line_to(p(6.0, 13.0));
        b.line_to(p(10.0, 9.0));
        b.close();
    });
    let colour = if pinned { AMBER } else { FAINT };
    if pinned {
        f.fill(&head, colour.get());
    }
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default()
            .with_color(colour.get())
            .with_width(1.8 * k)
    };
    f.stroke(&head, stroke);
    f.stroke(&Path::line(p(12.0, 13.0), p(12.0, 21.0)), stroke);
}

/// The history opens, on an empty search: what is past its limits goes.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
    clipboard::opening();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::text_item;

    fn measure(t: &str, size: f32, _font: Font) -> f32 {
        t.chars().count() as f32 * size * 0.5
    }

    fn now() -> Zoned {
        "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time")
    }

    fn items(now: &Zoned) -> Vec<Item> {
        let ms = now.timestamp().as_millisecond();
        let mut pinned = text_item(1, "ssh alex@192.168.1.24", "kitty", ms - 9000 * 60_000);
        pinned.pinned = true;
        vec![
            text_item(2, "cargo nextest run -p hocus-test", "kitty", ms - 60_000),
            text_item(
                3,
                "https://github.com/mattboll/hocus-panda/pull/513",
                "Firefox",
                ms - 4 * 60_000,
            ),
            text_item(4, "Réunion à la Pépinière", "Thunderbird", ms - 12 * 60_000),
            pinned,
        ]
    }

    fn laid(query: &str, words: Words) -> Laid {
        let now = now();
        lay_out(
            &items(&now),
            false,
            query,
            words,
            &now,
            &mut Cache::default(),
            &mut measure,
        )
    }

    #[test]
    fn it_lists_the_pinned_then_the_recent_each_row_as_the_mockup_lays_it() {
        let words = Words::new(Lang::French);
        let laid = laid("", words);
        let said = laid.said(words, None);
        assert!(
            said.starts_with(
                "ÉPINGLÉS: [code] «ssh alex@192.168.1.24» kitty · il y a 6\u{a0}jours (pinned) \
                 RÉCENTS: [code] «cargo nextest run -p hocus-test» kitty · il y a 1\u{a0}min \
                 [link] «https://github.com/mattboll/hocu"
            ),
            "{said}"
        );
        assert!(
            said.ends_with("[text] «Réunion à la Pépinière» Thunderbird · il y a 12\u{a0}min"),
            "{said}"
        );
        // A label, then rows of 44 px, 2 px apart.
        assert_eq!(laid.heads, [(Head::Pinned, 0.0), (Head::Recent, 70.0)]);
        let tops: Vec<f32> = laid.rows.iter().map(|r| r.top).collect();
        assert_eq!(tops, [24.0, 94.0, 140.0, 186.0]);
        // Laid out on the popover, as the kit declares them.
        let shown = Ui {
            laid: Some(laid.clone()),
            ..Ui::default()
        };
        let placed: HashMap<String, Rectangle> =
            ui::walk::declared(content(&laid, words, &shown, false), W)
                .into_iter()
                .map(|(r, d)| (d.node.key, r))
                .collect();
        let rect = placed["clipboard:Row(1)"];
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (13.0, 115.0, 334.0, 44.0)
        );
        // Its tools 5 px from its right, the ✕ last.
        let (pin, remove) = (placed["clipboard:Pin(1)"], placed["clipboard:Remove(1)"]);
        assert_eq!(remove.x + remove.width, 347.0 - 5.0);
        assert_eq!(pin.x + pin.width + 2.0, remove.x);
        // The search 8 px under the header, the list 8 px under it.
        assert_eq!(FIELD_TOP, 47.0);
        assert_eq!(LIST_TOP, 91.0);
        assert_eq!(height(), 556);
    }

    #[test]
    fn the_search_finds_a_substring_of_the_text_or_the_application_without_accents() {
        let words = Words::new(Lang::French);
        let found =
            |query: &str| -> Vec<u64> { laid(query, words).rows.iter().map(|r| r.key).collect() };
        assert_eq!(found("pepin"), [4]);
        assert_eq!(found("KITTY"), [1, 2]);
        assert_eq!(found("hocus"), [2, 3]);
        assert_eq!(
            laid("zzz ", words).empty.as_deref(),
            Some("Rien pour «\u{a0}zzz\u{a0}»")
        );
        let empty = lay_out(
            &[],
            false,
            "",
            Words::new(Lang::English),
            &now(),
            &mut Cache::default(),
            &mut measure,
        );
        assert_eq!(empty.empty.as_deref(), Some("Nothing copied yet."));
        assert!(empty.heads.is_empty());
    }

    #[test]
    fn a_capture_comes_from_the_screenshot_and_is_found_by_it() {
        let now = now();
        let ms = now.timestamp().as_millisecond();
        let mut items = items(&now);
        items.insert(0, text_item(9, "", hocus_link::CAPTURE_APP, ms));
        for (lang, said, query) in [
            (Lang::French, "Capture d'écran · maintenant", "ecran"),
            (Lang::English, "Screenshot · now", "screens"),
        ] {
            let words = Words::new(lang);
            let laid = lay_out(
                &items,
                false,
                "",
                words,
                &now,
                &mut Cache::default(),
                &mut measure,
            );
            assert_eq!(laid.rows[1].key, 9);
            assert_eq!(laid.rows[1].meta, said);
            let found = lay_out(
                &items,
                false,
                query,
                words,
                &now,
                &mut Cache::default(),
                &mut measure,
            );
            let keys: Vec<u64> = found.rows.iter().map(|r| r.key).collect();
            assert_eq!(keys, [9]);
        }
    }

    #[test]
    fn the_history_off_says_so_over_its_pinned_items() {
        let now = now();
        let words = Words::new(Lang::French);
        let pinned: Vec<Item> = items(&now).into_iter().filter(|i| i.pinned).collect();
        let laid = lay_out(
            &pinned,
            true,
            "",
            words,
            &now,
            &mut Cache::default(),
            &mut measure,
        );
        let said = laid.said(words, None);
        assert!(
            said.starts_with("Historique coupé · Réglages: ÉPINGLÉS: [code] «ssh"),
            "{said}"
        );
        // Nothing pinned: only its line.
        let laid = lay_out(
            &[],
            true,
            "",
            words,
            &now,
            &mut Cache::default(),
            &mut measure,
        );
        assert!(laid.empty.is_none());
        assert_eq!(laid.heads, [(Head::Off, 0.0)]);
    }

    #[test]
    fn an_item_shows_on_one_line() {
        assert_eq!(one_line("  git log\n\t--oneline  \n"), "git log --oneline");
    }
}
