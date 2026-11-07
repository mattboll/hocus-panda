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

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::{
    Color, Font, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};
use iced_layershell::reexport::KeyboardInteractivity;
use jiff::Zoned;

use super::{
    Effect, Event as Seen, Pressed, event,
    kit::{self, Clip, Input, Pointer, Wheel, text, text_in},
};
use crate::theme::Ink;
use crate::{
    clipboard::{self, Item, Kind},
    clock::time::Lang,
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    notifications::history::{Ago, ago},
    popover::kit::MANROPE_SEMIBOLD,
};

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
const HEAD_SPACING: f32 = 0.08 * HEAD_TEXT;
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
const RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
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

    pub fn title(self) -> &'static str {
        self.pick("Presse-papiers", "Clipboard")
    }

    fn clear(self) -> &'static str {
        self.pick("Tout effacer", "Clear all")
    }

    fn placeholder(self) -> &'static str {
        self.pick("Chercher dans ce que tu as copié", "Search what you copied")
    }

    fn head(self, head: Head) -> &'static str {
        match head {
            Head::Pinned => self.pick("ÉPINGLÉS", "PINNED"),
            Head::Recent => self.pick("RÉCENTS", "RECENT"),
            Head::Off => self.pick("Historique coupé · Réglages", "History off · Settings"),
        }
    }

    fn keys(self) -> &'static str {
        self.pick(
            "Entrée colle · Maj+Entrée copie · Suppr retire",
            "Enter pastes · Shift+Enter copies · Delete removes",
        )
    }

    fn nothing(self) -> &'static str {
        self.pick("Rien de copié pour l’instant.", "Nothing copied yet.")
    }

    fn not_found(self, query: &str) -> String {
        if self.fr {
            format!("Rien pour «\u{a0}{query}\u{a0}»")
        } else {
            format!("Nothing for \u{201c}{query}\u{201d}")
        }
    }

    /// Where a capture comes from (5.8b).
    fn capture(self) -> &'static str {
        self.pick("Capture d'écran", "Screenshot")
    }

    fn image(self, width: u32, height: u32) -> String {
        format!("Image · {width}\u{a0}×\u{a0}{height}")
    }

    fn ago(self, ago: Ago) -> String {
        match (ago, self.fr) {
            (Ago::Now, true) => "maintenant".to_owned(),
            (Ago::Now, false) => "now".to_owned(),
            (Ago::Minutes(n), true) => format!("il y a {n}\u{a0}min"),
            (Ago::Minutes(n), false) => format!("{n}\u{a0}min ago"),
            (Ago::Hours(n), true) => format!("il y a {n}\u{a0}h"),
            (Ago::Hours(n), false) => format!("{n}\u{a0}h ago"),
            (Ago::Yesterday, true) => "hier".to_owned(),
            (Ago::Yesterday, false) => "yesterday".to_owned(),
            (Ago::Days(n), true) => format!("il y a {n}\u{a0}jours"),
            (Ago::Days(n), false) => format!("{n}\u{a0}days ago"),
        }
    }

    /// The pin's name, for the log.
    fn pin(self, pinned: bool) -> &'static str {
        if pinned {
            self.pick("Désépingler", "Unpin")
        } else {
            self.pick("Épingler", "Pin")
        }
    }
}

// --- Laying it out ----------------------------------------------------------------

/// What can be acted on in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Clear,
    Field,
    Row(u64),
    Pin(u64),
    Remove(u64),
}

impl Target {
    /// The item it belongs to, if it is an item's.
    fn key(self) -> Option<u64> {
        match self {
            Self::Row(k) | Self::Pin(k) | Self::Remove(k) => Some(k),
            Self::Clear | Self::Field => None,
        }
    }
}

/// A band of the background, across the popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    Above,
    List,
    Below,
}

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

    fn view() -> Rectangle {
        Rectangle::new(Point::new(LEFT, LIST_TOP), Size::new(RIGHT - LEFT, LIST))
    }

    fn field() -> Rectangle {
        Rectangle::new(Point::new(LEFT, FIELD_TOP), Size::new(RIGHT - LEFT, FIELD))
    }

    /// Row `row` on the popover, scrolled by `scroll`.
    fn rect(row: &Row, scroll: f32) -> Rectangle {
        Rectangle::new(
            Point::new(LEFT, LIST_TOP + row.top - scroll),
            Size::new(RIGHT - LEFT, ROW),
        )
    }

    /// A row's pin and ✕, on the popover.
    fn tools(rect: Rectangle) -> (Rectangle, Rectangle) {
        let size = Size::new(TOOL, TOOL);
        let y = rect.center_y() - TOOL / 2.0;
        let remove = Point::new(rect.x + rect.width - TOOLS_RIGHT - TOOL, y);
        let pin = Point::new(remove.x - TOOL_GAP - TOOL, y);
        (Rectangle::new(pin, size), Rectangle::new(remove, size))
    }

    /// What is under `at`, scrolled by `scroll`: a row's tools over it, in
    /// the list only within its viewport.
    fn target_at(&self, at: Point, scroll: f32) -> Option<Target> {
        if self.clear.contains(at) {
            return Some(Target::Clear);
        }
        if Self::field().contains(at) {
            return Some(Target::Field);
        }
        if !Self::view().contains(at) {
            return None;
        }
        let row = self
            .rows
            .iter()
            .find(|r| Self::rect(r, scroll).contains(at))?;
        let (pin, remove) = Self::tools(Self::rect(row, scroll));
        Some(if pin.contains(at) {
            Target::Pin(row.key)
        } else if remove.contains(at) {
            Target::Remove(row.key)
        } else {
            Target::Row(row.key)
        })
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

/// What its surface keeps between draws: a surface each time it opens,
/// so that it opens on an empty search.
#[derive(Debug, Default)]
pub struct State {
    laid: Option<Laid>,
    /// What it was laid out from: the history's version, the query, the
    /// minute.
    from: Option<(u64, String, i64)>,
    query: String,
    /// The query as it shows, and the caret after it.
    field: (String, f32),
    pointer: Pointer<Target>,
    selected: Option<u64>,
    scroll: f32,
    wheel: Wheel,
    logged: String,
}

impl State {
    fn keys(&self) -> Vec<u64> {
        self.laid
            .as_ref()
            .map_or_else(Vec::new, |l| l.rows.iter().map(|r| r.key).collect())
    }
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                self.refresh(state);
                None
            }
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // One frame: the list's band of the background, its rows, then the
        // bands above and below it, over what of the rows scrolls past its
        // edges (a frame of its own, clipped to the list, has iced's GPU
        // renderer clip the other frame's shapes to it too). Text and
        // images, drawn over every shape, show only whole in the list.
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        self.background(&mut frame, size, Band::List);
        let Some(laid) = &state.laid else {
            self.background(&mut frame, size, Band::Above);
            self.background(&mut frame, size, Band::Below);
            self.rim(&mut frame, size);
            return vec![frame.into_geometry()];
        };
        let view = Laid::view();
        let clip = Clip::new(true, Some(view));
        if let Some(empty) = &laid.empty {
            frame.fill_text(text(
                empty.clone(),
                Point::new(view.center_x(), LIST_TOP + EMPTY_TOP + EMPTY * LINE / 2.0),
                EMPTY,
                FAINT.get(),
                Alignment::Center,
            ));
        }
        for (head, top) in &laid.heads {
            let middle = LIST_TOP + top - state.scroll + HEAD / 2.0;
            if middle < view.y - HEAD || middle > view.y + view.height + HEAD {
                continue;
            }
            self.head(&mut frame, clip, *head, middle);
        }
        for row in &laid.rows {
            let rect = Laid::rect(row, state.scroll);
            if rect.y > view.y + view.height || rect.y + rect.height < view.y {
                continue;
            }
            self.row(&mut frame, clip, row, rect, state);
        }
        self.background(&mut frame, size, Band::Above);
        self.background(&mut frame, size, Band::Below);
        self.rim(&mut frame, size);
        self.header(&mut frame, laid, state.pointer.hover);
        let keyboard = super::with(|c| c.holds_keyboard(self.id));
        self.field(&mut frame, state, keyboard);
        if laid.content > LIST + 0.5 {
            let list = kit::list(LIST_TOP, LIST, laid.content);
            list.scrollbar_at(&mut frame, state.scroll, W - BORDER - kit::SCROLLBAR - 2.0);
        }
        frame.fill_text(text(
            self.words.keys().to_owned(),
            Point::new(LEFT, FOOTER_TOP + FOOTER * LINE / 2.0),
            FOOTER,
            KEYS.get(),
            Alignment::Left,
        ));
        self.tell_readers(state, laid);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.pointer.hover {
            Some(Target::Field) => mouse::Interaction::Text,
            _ => state.pointer.interaction(false),
        }
    }
}

impl Panel {
    /// What it shows, for screen readers (crate::a11y::rows): the search,
    /// then each section's items, the selected one selected; the arrows
    /// go through them.
    fn tell_readers(&self, state: &State, laid: &Laid) {
        use crate::a11y::rows::{self, Rows};
        use hocus_a11y::{Node, Role};
        let words = self.words;
        let mut nodes = vec![
            Node::new(
                rows::key(Target::Field),
                Role::TextField,
                words.placeholder(),
            )
            .value(state.query.clone())
            .bounds(rows::at(Laid::field())),
            Node::new(rows::key(Target::Clear), Role::Button, words.clear())
                .bounds(rows::at(laid.clear)),
        ];
        if let Some(empty) = &laid.empty {
            nodes.push(rows::line(empty.clone()));
        }
        let mut rows_left = laid.rows.iter().peekable();
        for (i, (head, _)) in laid.heads.iter().enumerate() {
            let label = words.head(*head);
            let mut chars = label.chars();
            let label = chars.next().map_or_else(String::new, |c| {
                c.to_string() + &chars.as_str().to_lowercase()
            });
            nodes.push(rows::line(label));
            let next = laid.heads.get(i + 1).map(|(_, top)| *top);
            while let Some(row) = rows_left.next_if(|r| next.is_none_or(|top| r.top < top)) {
                let mut node = rows::act(
                    Target::Row(row.key),
                    Role::ListItem,
                    row.line.clone(),
                    Laid::rect(row, state.scroll),
                )
                .describe(row.meta.clone())
                .selected(state.selected == Some(row.key));
                if row.pinned {
                    node = node.child(
                        Node::new(
                            rows::key(Target::Pin(row.key)),
                            Role::ToggleButton,
                            words.pin(false),
                        )
                        .checked(true),
                    );
                }
                nodes.push(node);
            }
        }
        rows::publish(
            self.id,
            Rows {
                subtitle: None,
                nodes,
                order: laid
                    .rows
                    .iter()
                    .map(|r| rows::key(Target::Row(r.key)))
                    .collect(),
                focus: state.selected.map(|k| rows::key(Target::Row(k))),
                arrows: true,
            },
        );
    }

    /// The background's `band`: its tint (opaque without glass), the
    /// same gradient across the three.
    fn background(&self, f: &mut Frame, size: Size, band: Band) {
        let (top, bottom) = if self.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let (y, height, radius) = match band {
            Band::Above => (0.0, LIST_TOP, [RADIUS, RADIUS, 0.0, 0.0]),
            Band::List => (LIST_TOP, LIST, [0.0; 4]),
            Band::Below => (
                LIST_TOP + LIST,
                size.height - LIST_TOP - LIST,
                [0.0, 0.0, RADIUS, RADIUS],
            ),
        };
        let [top_left, top_right, bottom_right, bottom_left] = radius;
        let shape = Path::rounded_rectangle(
            Point::new(0.0, y),
            Size::new(size.width, height),
            iced::border::Radius {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            },
        );
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top.get())
            .add_stop(1.0, bottom.get());
        f.fill(&shape, canvas::Fill::from(tint));
    }

    fn rim(&self, f: &mut Frame, size: Size) {
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (RADIUS - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM.get()).with_width(BORDER),
        );
    }

    fn header(&self, f: &mut Frame, laid: &Laid, hover: Option<Target>) {
        let middle = HEADER_TOP + HEADER / 2.0;
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                self.words.title().to_owned(),
                Point::new(LEFT, middle),
                TITLE,
                TEXT.get(),
                Alignment::Left,
            ),
        ));
        if hover == Some(Target::Clear) {
            crate::theme::hover(
                f,
                &kit::rounded(laid.clear, GHOST_RADIUS),
                GHOST_HOVER.get(),
            );
        }
        f.fill_text(text_in(
            MANROPE_SEMIBOLD,
            text(
                self.words.clear().to_owned(),
                laid.clear.center(),
                GHOST_TEXT,
                DIM.get(),
                Alignment::Center,
            ),
        ));
    }

    fn field(&self, f: &mut Frame, state: &State, keyboard: bool) {
        let field = Laid::field();
        f.fill(&kit::rounded(field, FIELD_RADIUS), FIELD_FILL.get());
        f.stroke(
            &kit::rounded(inset(field, 0.5), FIELD_RADIUS - 0.5),
            Stroke::default()
                .with_color(FIELD_RIM.get())
                .with_width(1.0),
        );
        magnifier(
            f,
            Point::new(
                field.x + BORDER + FIELD_PAD,
                field.center_y() - MAGNIFIER / 2.0,
            ),
        );
        let x = field.x + BORDER + FIELD_PAD + MAGNIFIER + FIELD_GAP;
        let (shown, caret) = &state.field;
        let (content, colour) = if state.query.is_empty() {
            (self.words.placeholder().to_owned(), QUIET)
        } else {
            (shown.clone(), TEXT)
        };
        f.fill_text(text(
            content,
            Point::new(x, field.center_y()),
            QUERY,
            colour.get(),
            Alignment::Left,
        ));
        // The caret after what is typed, while the field has the keyboard.
        if keyboard {
            let caret_x = if state.query.is_empty() {
                x - 2.0
            } else {
                x + caret + 1.0
            };
            f.fill_rectangle(
                Point::new(caret_x, field.center_y() - QUERY * 0.6),
                Size::new(1.0, QUERY * 1.2),
                TEXT.get(),
            );
        }
    }

    /// A section's label, spaced, its line centred on `middle`; the
    /// history off, its line as it is.
    fn head(&self, f: &mut Frame, clip: Clip, head: Head, middle: f32) {
        let mut x = LEFT + HEAD_PAD;
        if head == Head::Off {
            clip.fill(
                f,
                text_in(
                    MANROPE,
                    text(
                        self.words.head(head).to_owned(),
                        Point::new(x, middle),
                        META,
                        DIM.get(),
                        Alignment::Left,
                    ),
                ),
            );
            return;
        }
        for ch in self.words.head(head).chars() {
            let letter = ch.to_string();
            let width = kit::measure(&letter, HEAD_TEXT, MANROPE_BOLD);
            clip.fill(
                f,
                text_in(
                    MANROPE_BOLD,
                    text(
                        letter,
                        Point::new(x, middle),
                        HEAD_TEXT,
                        QUIET.get(),
                        Alignment::Left,
                    ),
                ),
            );
            x += width + HEAD_SPACING;
        }
    }

    fn row(&self, f: &mut Frame, clip: Clip, row: &Row, rect: Rectangle, state: &State) {
        let selected = state.selected == Some(row.key);
        if selected {
            f.fill(&kit::rounded(rect, ROW_RADIUS), SELECTED_FILL.get());
            f.stroke(
                &kit::rounded(inset(rect, 0.5), ROW_RADIUS - 0.5),
                Stroke::default()
                    .with_color(SELECTED_RIM.get())
                    .with_width(1.0),
            );
        }
        // Its kind's tile.
        let tile = Rectangle::new(
            Point::new(rect.x + TILE_IN, rect.center_y() - TILE / 2.0),
            Size::new(TILE, TILE),
        );
        f.fill(&kit::rounded(tile, TILE_RADIUS), TILE_FILL.get());
        let inner = inset(tile, TILE_PAD);
        match &row.tile {
            Tile::Glyph(glyph) => clip.fill(
                f,
                text_in(
                    MANROPE_BOLD,
                    text(
                        (*glyph).to_owned(),
                        tile.center(),
                        GLYPH,
                        DIM.get(),
                        Alignment::Center,
                    ),
                ),
            ),
            Tile::Swatch(colour) => f.fill(&kit::rounded(inner, INNER_RADIUS), *colour),
            // Images go over every shape: only whole in the list.
            Tile::Thumbnail(handle) => {
                let view = Laid::view();
                if inner.y >= view.y && inner.y + inner.height <= view.y + view.height {
                    f.draw_image(inner, canvas::Image::new(handle.clone()));
                }
            }
        }
        // Its content over its application and when.
        let (size, font) = if row.kind.mono() {
            (MONO_CONTENT, MONO)
        } else {
            (CONTENT, MANROPE_SEMIBOLD)
        };
        let column = CONTENT * LINE + LINES_GAP + META * LINE;
        let top = rect.center_y() - column / 2.0;
        let x = rect.x + TEXT_X;
        clip.fill(
            f,
            text_in(
                font,
                text(
                    row.line.clone(),
                    Point::new(x, top + CONTENT * LINE / 2.0),
                    size,
                    TEXT.get(),
                    Alignment::Left,
                ),
            ),
        );
        clip.fill(
            f,
            text(
                row.meta.clone(),
                Point::new(x, top + CONTENT * LINE + LINES_GAP + META * LINE / 2.0),
                META,
                QUIET.get(),
                Alignment::Left,
            ),
        );
        if !selected {
            return;
        }
        // Its pin and ✕.
        let (pin, remove) = Laid::tools(rect);
        let hover = state.pointer.hover;
        for (tool, lit) in [
            (pin, hover == Some(Target::Pin(row.key))),
            (remove, hover == Some(Target::Remove(row.key))),
        ] {
            if lit {
                crate::theme::hover(f, &kit::rounded(tool, TOOL_RADIUS), GHOST_HOVER.get());
            }
        }
        pin_glyph(f, pin.center(), row.pinned);
        clip.fill(
            f,
            text(
                "✕".to_owned(),
                remove.center(),
                CROSS,
                FAINT.get(),
                Alignment::Center,
            ),
        );
    }

    /// What it shows, read afresh while it is open: laid out again when the
    /// history, the query or the minute changed.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let now = crate::notifications::wall();
        let (version, items, off) = clipboard::with(|h| {
            (
                h.version(),
                h.shown(now.timestamp().as_millisecond()),
                !h.rules().history,
            )
        });
        let minute = now.timestamp().as_second() / 60;
        let from = (version, state.query.clone(), minute);
        if state.from.as_ref() == Some(&from) && state.laid.is_some() {
            return;
        }
        let typed = state.from.as_ref().map(|f| &f.1) != Some(&state.query);
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
                &state.query,
                self.words,
                &now,
                cache,
                &mut kit::measure,
            )
        };
        if typed {
            let room = Laid::field().width - 2.0 * (BORDER + FIELD_PAD) - MAGNIFIER - FIELD_GAP;
            let width = |t: &str| kit::measure(t, QUERY, MANROPE);
            let shown = tail(&state.query, room, width);
            let caret = if state.query.is_empty() {
                0.0
            } else {
                width(&shown)
            };
            state.field = (shown, caret);
        }
        let keys: Vec<u64> = laid.rows.iter().map(|r| r.key).collect();
        if state.selected.is_none_or(|s| !keys.contains(&s)) {
            state.selected = keys.first().copied();
        }
        state.scroll = state.scroll.clamp(0.0, laid.most_scroll());
        state.laid = Some(laid);
        state.from = Some(from);
        self.log(state);
    }

    /// What it shows, in the log, when that changed.
    fn log(&self, state: &mut State) {
        let Some(laid) = &state.laid else {
            return;
        };
        let said = laid.said(self.words, state.selected);
        if said == state.logged {
            return;
        }
        let targets: Vec<String> = laid
            .rows
            .iter()
            .flat_map(|row| {
                let rect = Laid::rect(row, state.scroll);
                let (pin, remove) = Laid::tools(rect);
                [
                    (format!("Row({})", row.key), rect),
                    (format!("Pin({})", row.key), pin),
                    (format!("Remove({})", row.key), remove),
                ]
            })
            .chain([
                ("Clear".to_owned(), laid.clear),
                ("Field".to_owned(), Laid::field()),
            ])
            .map(|(name, r)| format!("{name}@{:.0},{:.0}", r.center().x, r.center().y))
            .collect();
        tracing::debug!(
            title = self.words.title(),
            typed = state.query.chars().count(),
            history = %said,
            items = %targets.join(" "),
            "the clipboard's history shows it"
        );
        state.logged = said;
    }

    fn act(&self, state: &mut State, target: Target) {
        match target {
            Target::Clear => clipboard::clear(),
            Target::Field => {}
            Target::Row(key) => self.choose(key, true),
            Target::Pin(key) => {
                let pinned = clipboard::with(|h| h.get(key).is_some_and(|i| i.pinned));
                tracing::debug!(
                    name = self.words.pin(pinned),
                    "the clipboard's history's pin"
                );
                clipboard::pin(key);
            }
            Target::Remove(key) => self.remove(state, key),
        }
    }

    /// Item `key` to the clipboard, and pasted (`paste`): the history
    /// closes, the keyboard going back to the window that had it.
    fn choose(&self, key: u64, paste: bool) {
        super::send(vec![Effect::Keyboard(self.id, KeyboardInteractivity::None)]);
        clipboard::choose(key, paste);
        event(Seen::Chosen);
    }

    /// Item `key` goes: the next one (else the one before) is selected.
    fn remove(&self, state: &mut State, key: u64) {
        let keys = state.keys();
        if state.selected == Some(key)
            && let Some(at) = keys.iter().position(|k| *k == key)
        {
            state.selected = keys
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|before| keys.get(before)))
                .copied();
        }
        clipboard::remove(key);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .laid
                .as_ref()
                .and_then(|l| l.target_at(at, state.scroll))
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                let changed = state.pointer.over(target);
                // Hovering a row selects it.
                if let Some(key) = target.and_then(Target::key)
                    && state.selected != Some(key)
                {
                    state.selected = Some(key);
                    self.log(state);
                    return redraw;
                }
                changed.then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                // Clicked in, a hovered history takes the keyboard.
                event(Seen::Hold);
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.act(state, target);
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } => {
                let laid = state.laid.as_ref()?;
                if !at.is_some_and(|at| Laid::view().contains(at)) {
                    return None;
                }
                let by = state.wheel.down(delta, NOTCH, kit::popover_scale());
                state.scroll = (state.scroll + by).clamp(0.0, laid.most_scroll());
                state.pointer.hover = at.and_then(|at| laid.target_at(at, state.scroll));
                if let Some(key) = state.pointer.hover.and_then(Target::key) {
                    state.selected = Some(key);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let keys = state.keys();
        let modifiers = pressed.modifiers;
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Enter) => {
                if let Some(key) = state.selected.filter(|s| keys.contains(s)) {
                    self.choose(key, !modifiers.shift());
                }
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Delete) => {
                if let Some(key) = state.selected.filter(|s| keys.contains(s)) {
                    self.remove(state, key);
                }
            }
            Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
                let at = state
                    .selected
                    .and_then(|s| keys.iter().position(|k| *k == s));
                let next = match (at, *arrow == Named::ArrowDown) {
                    (Some(at), true) => (at + 1).min(keys.len().saturating_sub(1)),
                    (Some(at), false) => at.saturating_sub(1),
                    (None, _) => 0,
                };
                state.selected = keys.get(next).copied();
                self.reveal(state);
                self.log(state);
            }
            Key::Named(Named::Backspace) => {
                if modifiers.control() {
                    let kept = state.query.trim_end().rfind(' ').map_or(0, |at| at + 1);
                    state.query.truncate(kept);
                } else {
                    state.query.pop();
                }
                self.typed(state);
            }
            // Nothing else to go to: the field keeps the keyboard.
            Key::Named(Named::Tab) => return Some(canvas::Action::capture()),
            _ => match pressed.text.as_deref() {
                Some(typed)
                    if !modifiers.control()
                        && !modifiers.alt()
                        && !modifiers.logo()
                        && !typed.is_empty()
                        && !typed.chars().any(char::is_control) =>
                {
                    if state.query.chars().count() + typed.chars().count() <= QUERY_MOST {
                        state.query.push_str(typed);
                        self.typed(state);
                    }
                }
                _ => return None,
            },
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The query changed: the list from its top, its first row selected.
    fn typed(&self, state: &mut State) {
        state.scroll = 0.0;
        state.selected = None;
        self.refresh(state);
    }

    /// The list scrolled so that the selected row shows, with its
    /// section's label when it is the first.
    fn reveal(&self, state: &mut State) {
        let (Some(laid), Some(key)) = (&state.laid, state.selected) else {
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
        state.scroll = kit::reveal(
            (top, row.top + ROW - top),
            LIST,
            state.scroll,
            laid.most_scroll(),
        );
    }
}

/// `query`'s end that fits `room`: what was typed last shows.
fn tail(query: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(query) <= room {
        return query.to_owned();
    }
    let chars: Vec<char> = query.chars().collect();
    for skip in 1..chars.len() {
        let cut: String = std::iter::once('…')
            .chain(chars[skip..].iter().copied())
            .collect();
        if width(&cut) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

fn inset(rect: Rectangle, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + by, rect.y + by),
        Size::new(rect.width - 2.0 * by, rect.height - 2.0 * by),
    )
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

/// The history opens: what is past its limits goes.
pub fn opening() {
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
        let rect = Laid::rect(&laid.rows[0], 0.0);
        assert_eq!((rect.x, rect.width, rect.height), (13.0, 334.0, 44.0));
        // Its tools 5 px from its right, the ✕ last.
        let (pin, remove) = Laid::tools(rect);
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
    fn a_long_query_shows_its_end() {
        let width = |t: &str| t.chars().count() as f32 * 7.0;
        assert_eq!(tail("cargo", 100.0, width), "cargo");
        assert_eq!(tail("cargo nextest run", 70.0, width), "…xtest run");
        assert_eq!(one_line("  git log\n\t--oneline  \n"), "git log --oneline");
    }
}
