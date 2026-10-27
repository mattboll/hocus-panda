//! The launcher (Super+Space, or the column's command field): one field,
//! results from providers as it is typed. Version 0: the calculator, then
//! applications by fuzzy match. Up and Down choose, Enter goes, Escape
//! closes.
//!
//! The same card shows the clipboard history (Super+V: newest first, Tab
//! pins, Delete forgets) and the emoji picker (Super+Period: in English and
//! French, recent ones first, Tab changes the skin tone).
//!
//! And notes (Super+N, #79): what is typed is added as a line (one typed
//! as a to-do, `- [ ] …`, becomes a task, #255); Delete removes the line
//! chosen.
//!
//! And a task (#255), from the column's field or a task's menu: what is
//! typed is added to the tasks, or replaces the text of the one edited.
//! With CalDAV task lists (#256), the lists it can go to are listed under
//! the field, the local one first and chosen; the arrows choose another.
//! The command field adds one too: "todo …", "task: …".
//!
//! And the action palette (Super+P, #68): every action, with its keys as
//! caps and what it acts on, recent ones first; searched by name in either
//! language, or by keys typed ("Maj+S") or pressed.
//!
//! And the AI (#44): a request understood offline ("capture de slack dans
//! 3 secondes") shows as a card to confirm before it runs; a question can
//! be asked, and its answer streams into the card, with buttons to copy it
//! or keep it in the notes. The other results stay below, under "Also".
//! The actions it proposes (#197) come first: each shows what it will do,
//! and runs only once confirmed.
//!
//! And documents (#45): what is typed is looked for in the documents
//! indexed, off the loop; those found come under "Documents", after the
//! applications, each with its folder and a line around the words that
//! matched. Enter opens one with its default application.
//!
//! With its field empty, it shows every application (#254): a grid in
//! alphabetical order, each with its icon, or its glyph on its colour as
//! the dock draws it. The arrows move, Enter opens, typing searches.

use std::{collections::BTreeMap, time::Duration};

use hocus_core::{Action, shortcuts::Combo};
use hocus_ui::{
    AccessNode, Align, Direction, Event, Icon, Input, Insets, Key, Kind, Length, Look, NodeId,
    Paint, Picture, PixelRect, Point, Property, Rect, Size, Style, Surface, TextField, Theme, Tree,
};

use crate::{
    answer::{self, Answer, Card, Source},
    column::View,
    i18n::Lang,
    icons,
    intents::Intent,
    panda::Mood,
    proposals::{Offer, Run},
    providers::{Context, Enabled, Hit, Provider, providers},
};

/// The hint's box: room under the list, or none.
fn hint_style(shown: bool) -> Style {
    Style {
        padding: if shown {
            Insets {
                top: 0.0,
                right: 28.0,
                bottom: 14.0,
                left: 28.0,
            }
        } else {
            Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

/// The line under the field, with a card.
fn rule_style(shown: bool) -> Style {
    Style {
        height: Length::Px(if shown { 1.0 } else { 0.0 }),
        grow: 0.0,
        ..Style::default()
    }
}

/// The card's room under the field.
fn panel_style(shown: bool) -> Style {
    Style {
        padding: if shown {
            Insets::symmetric(24.0, 28.0)
        } else {
            Insets::default()
        },
        gap: 16.0,
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

/// The results: the whole list, or a few under the card.
fn list_style(card: bool) -> Style {
    Style {
        padding: if card {
            Insets {
                top: 14.0,
                right: 12.0,
                bottom: 16.0,
                left: 12.0,
            }
        } else {
            Insets::symmetric(12.0, 12.0)
        },
        ..Style::default()
    }
}

/// The card's width, from the design.
pub const WIDTH: f32 = 860.0;
/// Room around the card for its shadow.
pub const ROOM: f32 = 40.0;
const FIELD: f32 = 76.0;
const ROW: f32 = 48.0;
const MAX_RESULTS: usize = 8;
/// Under an answer or a confirmation: the other results, fewer and closer.
const MAX_ALSO: usize = 3;
const ALSO_ROW: f32 = 44.0;
/// The card's height with an answer: the field, the answer, "Also".
const TALL: f32 = FIELD + 1.0 + 470.0 + 210.0;
/// A longer answer scrolls in a box this high.
const ANSWER_MAX: f32 = 300.0;
/// Page Up and Page Down scroll the answer by this much.
const ANSWER_PAGE: f32 = ANSWER_MAX - 40.0;
/// The card's room inside its padding, for its buttons.
const PANEL: f32 = WIDTH - 2.0 * 28.0;
/// The grid of every application (#254): so many tiles a row, so many
/// rows shown, in the room the list of results takes.
const COLUMNS: usize = 6;
const GRID_ROWS: usize = 3;
const TILE_GAP: f32 = 4.0;
/// A tile fills its share of the list's width.
const TILE_W: f32 = (WIDTH - 2.0 * 12.0 - (COLUMNS as f32 - 1.0) * TILE_GAP) / COLUMNS as f32;
const TILE_H: f32 = 116.0;
/// A tile's icon, logical pixels: icons are read at this size times the
/// screen's scale.
pub const GRID_ICON: f32 = 48.0;
/// The size of the grid's icons on a screen of `scale`, in pixels.
pub fn icon_size(scale: f32) -> u32 {
    (GRID_ICON * scale.max(1.0)).ceil() as u32
}
/// So much wheel moves the grid a row: a notch.
const WHEEL_ROW: f32 = 15.0;

/// An application the launcher can start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The desktop entry's id, `firefox.desktop`.
    pub id: String,
    pub name: String,
    pub generic: Option<String>,
    pub keywords: Vec<String>,
    /// The program and its arguments.
    pub exec: Vec<String>,
    /// Its icon's name (`Icon`); the pixels come on their own, read off
    /// the loop.
    pub icon: Option<String>,
}

/// A document the search found (#45).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub path: String,
    pub name: String,
    /// The folder it is in, by its name.
    pub folder: String,
    /// A line around the words matched, those marked.
    pub snippet: Vec<(String, bool)>,
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Launch(App),
    /// A result to put on the clipboard.
    Copy(String),
    /// An address to open in the browser.
    Open(String),
    /// A document, opened with its default application.
    OpenFile(String),
    /// Something to show over the whole screen.
    Show(Showcase),
    /// The settings app.
    Settings,
    /// Locking, sleeping or leaving the session.
    Session(SessionAction),
    /// A copy from the history, to paste into the focused application.
    Paste(String),
    /// Text to type into the focused application (an emoji).
    Insert(String),
    /// Forgets the history, but what is pinned.
    ClearHistory,
    /// An action of the palette, on what had the focus.
    Run(Action),
    /// The settings' shortcuts, to give an action keys.
    SetShortcut,
    /// A line typed in the notes: added.
    AddNote(String),
    /// A task to add, from the command field (#255).
    AddTask(String),
    /// A task typed in the card: added, to the list of this key when one
    /// was chosen, or the text of the one of this id.
    SaveTask {
        id: Option<String>,
        text: String,
        list: Option<String>,
    },
    /// A list a task can be added to, by its key: chosen in the card.
    TaskList(String),
    /// The notes, opened from the command field.
    OpenNotes,
    /// A request understood offline, confirmed (#44).
    Intent(Intent),
    /// What is typed, asked to the AI.
    Ask(String),
    /// The answer shown: Enter acts on it.
    Answer,
}

/// A list a task can be added to, as the card lists it (#256).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListChoice {
    /// What names it to the compositor.
    pub key: String,
    pub name: String,
    /// Where it is: its account, or this computer.
    pub place: String,
    pub color: Option<hocus_ui::Color>,
}

/// A line of the notes, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteItem {
    /// Its place in the file.
    pub index: usize,
    pub text: String,
}

/// What the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Commands,
    Clipboard,
    Emoji,
    Actions,
    Notes,
    /// A task to add or edit (#255).
    Tasks,
}

/// What the command field can do to the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Lock,
    Suspend,
    LogOut,
    Restart,
    ShutDown,
}

/// What the launcher can show over a whole screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Showcase {
    /// Words as large as they fit.
    LargeText(String),
    /// A QR code of the words.
    Qr(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherEvent {
    Chosen(Choice),
    Closed,
    /// A history entry is pinned or unpinned.
    Pin(String),
    /// A history entry is forgotten.
    Forget(String),
    /// The note at this place in the file goes.
    RemoveNote(usize),
    /// A question for the AI; its answer comes back by `id` through
    /// [`Launcher::answer_news`].
    Ask {
        id: u64,
        question: String,
    },
    /// An answer, kept in the notes.
    NoteAnswer(String),
    /// An action an answer proposed, confirmed (#197).
    Act(Run),
}

/// What a button of the card does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardAction {
    Confirm,
    Cancel,
    Copy,
    Note,
    Stop,
    Retry,
    /// The answer's proposal at this index: its confirmation shows.
    Offer(usize),
    /// The confirmation's button at this index: it runs.
    Run(usize),
    /// Back to the answer, nothing done.
    Back,
}

/// Where the launcher's parts are, logical from its corner, for the Matrix
/// theme to type them in and rain them away.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub card: Rect,
    /// The command field's row.
    pub head: Rect,
    /// The results shown, top first.
    pub rows: Vec<Rect>,
}

pub struct Launcher {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    card: NodeId,
    head: NodeId,
    field: NodeId,
    list: NodeId,
    /// What answers the query, best first; `rows` shows `MAX_RESULTS` of
    /// them from `first`.
    hits: Vec<Hit>,
    first: usize,
    rows: Vec<NodeId>,
    apps: Vec<App>,
    providers: Vec<Box<dyn Provider>>,
    selected: usize,
    open: bool,
    mode: Mode,
    /// The clipboard history: text, pinned.
    history: Vec<(String, bool)>,
    /// Emoji chosen lately, newest first.
    recents: Vec<String>,
    /// 0 none, 1 light to 5 dark.
    tone: u8,
    /// Every binding, for the palette.
    shortcuts: Vec<(Combo, Action)>,
    /// Actions run from the palette lately, newest first.
    action_recents: Vec<Action>,
    /// The notes, in the file's order.
    notes: Vec<NoteItem>,
    /// The task being edited, by id; `None` adds one.
    editing: Option<String>,
    /// The lists a task can be added to, the local one first (#256).
    task_lists: Vec<TaskListChoice>,
    /// The keys that open what the card shows, in the field's corner.
    chip_text: NodeId,
    /// The hint under the list in the history and the picker, and its box.
    hint: NodeId,
    hint_box: NodeId,
    /// The line under the field and the card's room, shown with an answer
    /// or a confirmation.
    rule: NodeId,
    panel: NodeId,
    /// The line between the card and "Also".
    also_rule: NodeId,
    /// The card's buttons, and what they do.
    card_buttons: Vec<(NodeId, CardAction)>,
    /// Which hit each row shows.
    row_hits: Vec<usize>,
    /// The answer's text box and its text, to scroll a long one.
    answer_text: Option<(NodeId, NodeId)>,
    /// The answer is too long for the card: it scrolls in its box.
    answer_tall: bool,
    /// How far down the answer is scrolled, logical pixels.
    answer_scroll: f32,
    /// Who answers questions, if anyone.
    ask: Option<Source>,
    answer: Option<Answer>,
    /// The last question's id.
    asked: u64,
    /// The answer changed since it was drawn.
    answer_changed: bool,
    /// What the answer proposes, once all there (#197).
    offers: Vec<Offer>,
    /// The proposal whose confirmation shows.
    confirming: Option<usize>,
    /// Documents are searched for what is typed.
    documents_on: bool,
    /// Those found for the last query asked, or one before.
    documents: Vec<Document>,
    /// The last query asked for documents, and its id.
    documents_query: String,
    documents_asked: u64,
    /// A query to ask, taken by the compositor.
    documents_wanted: Option<(u64, String)>,
    /// Applications' icons by name, read off the loop.
    icons: BTreeMap<String, Picture>,
    /// The room of the grid of every application, in place of the list
    /// while the field is empty; hidden otherwise.
    grid_box: NodeId,
    /// The grid, made once and kept, hidden or not, with its text laid
    /// out: opening again shows it as it was, nothing made again (#242).
    grid: Option<Grid>,
    /// The grid's tiles shown, and the application each is.
    tiles: Vec<NodeId>,
    tile_apps: Vec<usize>,
    /// The grid's first row shown.
    grid_first: usize,
    /// Wheel turned over the grid, not yet a row.
    wheel_left: f32,
    now: Duration,
}

impl std::fmt::Debug for Launcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launcher")
            .field("open", &self.open)
            .field("apps", &self.apps.len())
            .finish_non_exhaustive()
    }
}

impl Launcher {
    pub fn new(theme: Theme, lang: Lang, scale: f32) -> Self {
        let height = FIELD + MAX_RESULTS as f32 * ROW + 2.0 * 12.0;
        let size = Size::new(WIDTH + 2.0 * ROOM, height + 2.0 * ROOM);
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        let missing = || unreachable!("adding to a node of this tree");
        tree.set_look(root, Look::default());
        tree.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                align: Align::Stretch,
                ..Style::default()
            },
        );
        let card = tree
            .add(
                root,
                Style {
                    clip: true,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 24.0,
                    border: Some(Paint::Border),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(missing);
        let head = tree
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    height: Length::Px(FIELD),
                    padding: Insets::symmetric(0.0, 24.0),
                    gap: 14.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        tree.add(
            head,
            Style::default(),
            Kind::Icon {
                icon: Icon::stroked(icons::SEARCH, 22.0, 2.2)
                    .unwrap_or_else(|| unreachable!("a constant icon")),
                color: Paint::Accent,
                label: None,
            },
        )
        .unwrap_or_else(missing);
        let field = tree
            .add(
                head,
                Style {
                    width: Length::Fill,
                    height: Length::Px(32.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(TextField::new(
                    lang.command_field(),
                    lang.command_placeholder(),
                    theme.text.headline.with_weight(600),
                ))),
            )
            .unwrap_or_else(missing);
        let chip = tree
            .add(
                head,
                Style {
                    padding: Insets::symmetric(5.0, 10.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 8.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        let chip_text = tree
            .add(
                chip,
                Style::default(),
                Kind::text(
                    lang.launcher_shortcut(),
                    hocus_ui::TextStyle {
                        typeface: hocus_ui::Typeface::Mono,
                        ..theme.text.label.with_weight(500).with_color(Paint::Muted)
                    },
                ),
            )
            .unwrap_or_else(missing);
        let rule = tree
            .add(card, rule_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        let panel = tree
            .add(card, panel_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        let also_rule = tree
            .add(card, rule_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        let list = tree
            .add(card, list_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        let grid_box = tree
            .add(card, list_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        tree.set_hidden(grid_box, true);
        // Only in the history and the picker: commands have no hint.
        let hint_box = tree
            .add(card, hint_style(false), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        let hint = tree
            .add(
                hint_box,
                Style::default(),
                Kind::text("", theme.text.label.with_color(Paint::Muted)),
            )
            .unwrap_or_else(missing);
        Self {
            lang,
            tree,
            surface: Surface::new(size, scale),
            card,
            head,
            field,
            list,
            hits: Vec::new(),
            first: 0,
            rows: Vec::new(),
            apps: Vec::new(),
            providers: providers(&Enabled::default()),
            selected: 0,
            open: false,
            mode: Mode::Commands,
            history: Vec::new(),
            recents: Vec::new(),
            tone: 0,
            shortcuts: Vec::new(),
            action_recents: Vec::new(),
            notes: Vec::new(),
            editing: None,
            task_lists: Vec::new(),
            chip_text,
            hint,
            hint_box,
            rule,
            panel,
            also_rule,
            card_buttons: Vec::new(),
            row_hits: Vec::new(),
            answer_text: None,
            answer_tall: false,
            answer_scroll: 0.0,
            ask: None,
            answer: None,
            asked: 0,
            answer_changed: false,
            offers: Vec::new(),
            confirming: None,
            documents_on: false,
            documents: Vec::new(),
            documents_query: String::new(),
            documents_asked: 0,
            documents_wanted: None,
            icons: BTreeMap::new(),
            grid_box,
            grid: None,
            tiles: Vec::new(),
            tile_apps: Vec::new(),
            grid_first: 0,
            wheel_left: 0.0,
            now: Duration::ZERO,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Speaks `lang` from now on: built again, closed, keeping what it
    /// knows.
    pub fn set_lang(&mut self, lang: Lang) {
        if lang == self.lang {
            return;
        }
        let mut fresh = Self::new(self.tree.theme().clone(), lang, self.tree.scale());
        fresh.apps = std::mem::take(&mut self.apps);
        fresh.icons = std::mem::take(&mut self.icons);
        fresh.providers = std::mem::take(&mut self.providers);
        fresh.history = std::mem::take(&mut self.history);
        fresh.recents = std::mem::take(&mut self.recents);
        fresh.tone = self.tone;
        fresh.shortcuts = std::mem::take(&mut self.shortcuts);
        fresh.action_recents = std::mem::take(&mut self.action_recents);
        fresh.notes = std::mem::take(&mut self.notes);
        fresh.ask = self.ask.take();
        fresh.asked = self.asked;
        fresh.documents_on = self.documents_on;
        fresh.documents_asked = self.documents_asked;
        *self = fresh;
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.tree.scale() {
            let size = self.tree.size();
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if theme == *self.tree.theme() {
            return;
        }
        // The grid's text and corners follow the theme's: made again.
        let made = self.grid.take();
        if let Some(grid) = &made {
            self.tree.remove(grid.caption);
            self.tree.remove(grid.body);
        }
        self.tree.set_theme(theme);
        if made.is_some() && self.grid_shown() {
            self.render();
        }
    }

    /// The applications to search; an open launcher searches them again.
    /// The application whose windows have `app_id`: its desktop entry is
    /// `<app id>.desktop`, or ends with `.<app id>.desktop`.
    /// The applications it knows.
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    pub fn app_for(&self, app_id: &str) -> Option<&App> {
        let wanted = app_id.to_lowercase();
        self.apps.iter().find(|app| {
            let id = app.id.trim_end_matches(".desktop").to_lowercase();
            id == wanted || id.ends_with(&format!(".{wanted}"))
        })
    }

    /// The applications, found on a worker thread: they may come while
    /// the card is open. Only the command field lists them, and the line
    /// chosen there stays chosen. Kept in alphabetical order, for the grid;
    /// the same ones read again change nothing.
    pub fn set_apps(&mut self, mut apps: Vec<App>) {
        apps.sort_by_cached_key(|a| (crate::fuzzy::fold(&a.name), a.name.clone()));
        if apps == self.apps {
            return;
        }
        self.apps = apps;
        if self.open && self.mode == Mode::Commands {
            let selected = self.selected;
            self.search();
            self.selected = selected.min(self.hits.len().saturating_sub(1));
            self.show_selection();
        }
    }

    /// The size its icons are wanted at, in pixels: the grid's at its
    /// scale.
    pub fn icon_size(&self) -> u32 {
        icon_size(self.tree.scale())
    }

    /// Icons read off the loop, by name; the grid shows them at once.
    pub fn set_icons(&mut self, icons: Vec<(String, Picture)>) {
        if icons.is_empty() {
            return;
        }
        self.icons.extend(icons);
        if self.open && self.grid_shown() {
            self.render();
        }
    }

    /// Whether the card shows the grid of every application: the command
    /// field, empty.
    fn grid_shown(&self) -> bool {
        self.mode == Mode::Commands && !self.apps.is_empty() && self.query().trim().is_empty()
    }

    /// Which providers run, from the configuration.
    pub fn set_providers(&mut self, enabled: &Enabled) {
        self.providers = providers(enabled);
        self.ask = enabled.ask.clone();
        self.documents_on = enabled.documents;
        if !enabled.documents {
            self.documents.clear();
        }
    }

    /// The query to look for in the documents, once: the compositor
    /// hands it to the search.
    pub fn take_documents_query(&mut self) -> Option<(u64, String)> {
        self.documents_wanted.take()
    }

    /// The documents found for query `id`; those of an older query are
    /// dropped. The line chosen stays chosen.
    pub fn set_documents(&mut self, id: u64, documents: Vec<Document>) {
        if id != self.documents_asked || !self.open || self.mode != Mode::Commands {
            return;
        }
        if documents == self.documents {
            return;
        }
        self.documents = documents;
        let selected = self.selected;
        self.search();
        self.selected = selected.min(self.hits.len().saturating_sub(1));
        self.show_selection();
    }

    /// Opens on an empty field.
    pub fn open(&mut self) {
        self.open_mode(Mode::Commands);
    }

    /// Opens on an empty field, showing commands, the clipboard history or
    /// emoji.
    pub fn open_mode(&mut self, mode: Mode) {
        self.open = true;
        self.mode = mode;
        self.editing = None;
        // The tiles are kept: none stays under a pointer gone since.
        let _ = self.tree.input(Input::PointerLeave);
        self.answer = None;
        self.offers.clear();
        self.confirming = None;
        self.tree.set_field_value(self.field, "");
        self.tree.set_focus(Some(self.field));
        let keys = match mode {
            Mode::Commands => self.lang.launcher_shortcut(),
            Mode::Clipboard => "Super+V",
            Mode::Emoji => "Super+.",
            Mode::Actions => "Super+P",
            Mode::Notes => "Super+N",
            Mode::Tasks => self.lang.task_kind(),
        };
        self.tree.set_text(self.chip_text, keys);
        let placeholder = match mode {
            Mode::Actions => self.lang.palette_placeholder(),
            Mode::Notes => self.lang.notes_placeholder(),
            Mode::Tasks => self.lang.task_placeholder(),
            _ => self.lang.command_placeholder(),
        };
        self.tree.set_placeholder(self.field, placeholder);
        self.show_hint();
        self.search();
    }

    /// While closed, puts the card as it opens on commands, the field
    /// empty on every application, for the caller to draw at a quiet
    /// moment: opening then changes nothing, and draws nothing (#242).
    pub fn prepare(&mut self) {
        if self.open {
            return;
        }
        self.open_mode(Mode::Commands);
        self.open = false;
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Opens on a task to add, or on the text of the one of this id, to
    /// edit (#255).
    pub fn open_task(&mut self, editing: Option<(String, String)>) {
        self.open_mode(Mode::Tasks);
        if let Some((id, text)) = editing {
            self.tree.set_field_value(self.field, &text);
            self.editing = Some(id);
            self.show_hint();
            // A task edited stays in its list: none to choose.
            self.search();
        }
    }

    /// The lists a task can be added to, the local one first; with one
    /// only, none is listed.
    pub fn set_task_lists(&mut self, lists: Vec<TaskListChoice>) {
        if lists == self.task_lists {
            return;
        }
        self.task_lists = lists;
        if self.open && self.mode == Mode::Tasks {
            let selected = self.selected;
            let query = self.query().to_owned();
            self.hits = self.hits(&query);
            self.selected = selected.min(self.hits.len().saturating_sub(1));
            self.show_hint();
            self.render();
        }
    }

    /// The task card lists the lists to choose from.
    fn lists_shown(&self) -> bool {
        self.mode == Mode::Tasks && self.editing.is_none() && self.task_lists.len() > 1
    }

    /// The clipboard history, as it is shown: text and pinned.
    pub fn set_history(&mut self, history: Vec<(String, bool)>) {
        self.history = history;
        if self.open && self.mode == Mode::Clipboard {
            let selected = self.selected;
            self.search();
            self.selected = selected.min(self.hits.len().saturating_sub(1));
            self.show_selection();
        }
    }

    pub fn set_recents(&mut self, recents: Vec<String>) {
        self.recents = recents;
    }

    /// The notes as they are now; an open card shows them again, the
    /// chosen line kept.
    pub fn set_notes(&mut self, notes: Vec<NoteItem>) {
        self.notes = notes;
        if self.open && self.mode == Mode::Notes {
            let selected = self.selected;
            let query = self.query().to_owned();
            self.hits = self.hits(&query);
            self.selected = selected.min(self.hits.len().saturating_sub(1));
            self.render();
        }
    }

    /// The bindings in effect, for the palette.
    pub fn set_shortcuts(&mut self, shortcuts: Vec<(Combo, Action)>) {
        self.shortcuts = shortcuts;
    }

    pub fn set_action_recents(&mut self, recents: Vec<Action>) {
        self.action_recents = recents;
    }

    /// Keys pressed in the palette: they are what it looks for.
    pub fn press_keys(&mut self, combo: &Combo) {
        if !self.open || self.mode != Mode::Actions {
            return;
        }
        let text = crate::actions::caps(combo, self.lang).join("+");
        self.tree.set_field_value(self.field, &text);
        self.search();
    }

    /// The keys of `action`: those asked for when keys are searched, else
    /// its first binding's.
    fn keys_of(&self, action: &Action) -> Option<&Combo> {
        let asked = crate::actions::keys_query(self.query());
        self.shortcuts
            .iter()
            .filter(|(_, a)| a == action)
            .min_by_key(|(c, _)| {
                asked
                    .as_ref()
                    .and_then(|asked| crate::actions::keys_rank(asked, c))
                    .unwrap_or(u8::MAX)
            })
            .map(|(c, _)| c)
    }

    /// Every action, matching the query: by keys when it reads as keys,
    /// else by name in either language; recent ones first.
    fn action_hits(&self, query: &str) -> Vec<Hit> {
        use crate::actions::{catalog, describe, keys_query, keys_rank};
        let mut all: Vec<Action> = self.action_recents.clone();
        for action in catalog()
            .into_iter()
            .chain(self.shortcuts.iter().map(|(_, a)| a.clone()))
        {
            if !all.contains(&action) {
                all.push(action);
            }
        }
        let asked = keys_query(query);
        let mut ranked: Vec<(i64, usize, Action)> = all
            .into_iter()
            .enumerate()
            .filter_map(|(i, action)| {
                let recent = i < self.action_recents.len();
                let rank = if query.is_empty() {
                    // Recent ones, then by group.
                    if recent {
                        0
                    } else {
                        1 + describe(&action, self.lang).group as i64
                    }
                } else if let Some(asked) = &asked {
                    let best = self
                        .shortcuts
                        .iter()
                        .filter(|(_, a)| *a == action)
                        .filter_map(|(c, _)| keys_rank(asked, c))
                        .min()?;
                    i64::from(best)
                } else {
                    let names = [
                        describe(&action, Lang::En).name,
                        describe(&action, Lang::Fr).name,
                    ];
                    let score = names
                        .iter()
                        .filter_map(|n| crate::fuzzy::score(query, n))
                        .max()?;
                    -i64::from(score)
                };
                Some((rank, i, action))
            })
            .collect();
        ranked.sort_by_key(|(rank, i, _)| (*rank, *i));
        ranked
            .into_iter()
            .map(|(_, _, action)| {
                let d = describe(&action, self.lang);
                Hit {
                    title: d.name,
                    kind: format!("{} · {}", d.group.name(self.lang), d.scope),
                    color: None,
                    choice: Choice::Run(action),
                }
            })
            .collect()
    }

    fn show_hint(&mut self) {
        let hint = match self.mode {
            Mode::Commands => String::new(),
            Mode::Actions => self.lang.palette_hint().to_owned(),
            Mode::Clipboard => self.lang.history_hint().to_owned(),
            Mode::Notes => self.lang.notes_hint().to_owned(),
            Mode::Tasks if self.lists_shown() => self.lang.task_hint_lists().to_owned(),
            Mode::Tasks => self.lang.task_hint(self.editing.is_some()).to_owned(),
            Mode::Emoji => self.lang.emoji_hint(&self.tone_sample()),
        };
        self.tree
            .set_style(self.hint_box, hint_style(!hint.is_empty()));
        self.tree.set_text(self.hint, &hint);
    }

    fn tone_sample(&self) -> String {
        crate::emoji::search("thumbs up")
            .first()
            .map(|e| e.with_tone(self.tone))
            .unwrap_or_default()
    }

    /// What to list for the query, by mode.
    fn hits(&self, query: &str) -> Vec<Hit> {
        match self.mode {
            Mode::Commands => {
                let mut hits = Vec::new();
                if !query.is_empty() {
                    let cx = Context {
                        apps: &self.apps,
                        documents: &self.documents,
                        lang: self.lang,
                    };
                    for provider in &self.providers {
                        provider.hits(query, &cx, &mut hits);
                    }
                    // The answer to what is typed takes the place of asking it.
                    if let Some(answer) = self.answer.as_ref().filter(|a| a.question == query) {
                        hits.retain(|h| !matches!(h.choice, Choice::Ask(_) | Choice::Intent(_)));
                        hits.insert(
                            0,
                            Hit {
                                title: answer.caption(self.lang),
                                kind: String::new(),
                                color: None,
                                choice: Choice::Answer,
                            },
                        );
                    }
                    // Notes that say it open the notes.
                    let q = crate::fuzzy::fold(query);
                    if q.chars().count() >= 3 {
                        hits.extend(
                            self.notes
                                .iter()
                                .filter(|n| crate::fuzzy::fold(&n.text).contains(&q))
                                .take(3)
                                .map(|n| Hit {
                                    title: n.text.clone(),
                                    kind: self.lang.note_kind().into(),
                                    color: Some(hocus_ui::Color::rgb(0x2E, 0x7D, 0x4F)),
                                    choice: Choice::OpenNotes,
                                }),
                        );
                    }
                }
                hits
            }
            Mode::Clipboard => {
                let mut hits: Vec<Hit> = self
                    .history
                    .iter()
                    .filter(|(text, _)| {
                        query.is_empty() || crate::fuzzy::score(query, text).is_some()
                    })
                    .map(|(text, pinned)| {
                        let line: String = text
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or_default()
                            .chars()
                            .take(90)
                            .collect();
                        Hit {
                            title: line,
                            kind: if *pinned {
                                self.lang.pinned().into()
                            } else {
                                String::new()
                            },
                            color: None,
                            choice: Choice::Paste(text.clone()),
                        }
                    })
                    .collect();
                if query.is_empty() && self.history.iter().any(|(_, p)| !p) {
                    hits.truncate(MAX_RESULTS - 1);
                    hits.push(Hit {
                        title: self.lang.clear_history().into(),
                        kind: String::new(),
                        color: None,
                        choice: Choice::ClearHistory,
                    });
                }
                hits
            }
            Mode::Actions => self.action_hits(query),
            // Newest first; what is typed is a new line, not a search.
            Mode::Notes => self
                .notes
                .iter()
                .rev()
                .map(|n| Hit {
                    title: n.text.clone(),
                    kind: String::new(),
                    color: None,
                    choice: Choice::Copy(n.text.clone()),
                })
                .collect(),
            // What is typed is the task, not a search; the lists it can go
            // to are listed.
            Mode::Tasks if self.lists_shown() => self
                .task_lists
                .iter()
                .map(|l| Hit {
                    title: l.name.clone(),
                    kind: l.place.clone(),
                    color: l.color,
                    choice: Choice::TaskList(l.key.clone()),
                })
                .collect(),
            Mode::Tasks => Vec::new(),
            Mode::Emoji => {
                let emoji: Vec<&crate::emoji::Emoji> = if query.is_empty() {
                    self.recents
                        .iter()
                        .filter_map(|g| crate::emoji::find(g))
                        .collect()
                } else {
                    crate::emoji::search(query)
                };
                emoji
                    .into_iter()
                    .map(|e| {
                        let glyph = e.with_tone(self.tone);
                        Hit {
                            title: format!("{glyph}  {}", e.name(self.lang)),
                            kind: String::new(),
                            color: None,
                            choice: Choice::Insert(glyph),
                        }
                    })
                    .collect()
            }
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn query(&self) -> &str {
        self.tree.field_value(self.field).unwrap_or_default()
    }

    /// Fills the list from what is typed.
    fn search(&mut self) {
        let query = self.query().trim().to_owned();
        // Typing again leaves the answer.
        if self.answer.as_ref().is_some_and(|a| a.question != query) {
            self.answer = None;
            self.offers.clear();
            self.confirming = None;
            self.answer_tall = false;
            self.answer_scroll = 0.0;
        }
        self.ask_documents(&query);
        // The list chosen for a task stays chosen while it is typed.
        let kept = (self.mode == Mode::Tasks).then_some(self.selected);
        self.hits = self.hits(&query);
        // Only the palette scrolls: it lists everything.
        if self.has_card() {
            self.hits.truncate(1 + MAX_ALSO);
        } else if !matches!(self.mode, Mode::Actions | Mode::Notes | Mode::Tasks) {
            fit(&mut self.hits, MAX_RESULTS);
        }
        self.selected = kept.map_or(0, |k| k.min(self.hits.len().saturating_sub(1)));
        self.first = 0;
        self.grid_first = 0;
        self.wheel_left = 0.0;
        self.render();
    }

    /// Asks for the documents matching `query` when it changed; a query
    /// too short to search shows none.
    fn ask_documents(&mut self, query: &str) {
        if !self.documents_on || self.mode != Mode::Commands || query.chars().count() < 2 {
            self.documents.clear();
            self.documents_query.clear();
            return;
        }
        if query != self.documents_query {
            self.documents_query = query.to_owned();
            self.documents_asked += 1;
            self.documents_wanted = Some((self.documents_asked, query.to_owned()));
        }
    }

    /// Whether the first hit is drawn as the card: an answer, or a request
    /// to confirm.
    fn has_card(&self) -> bool {
        self.mode == Mode::Commands
            && self
                .hits
                .first()
                .is_some_and(|h| matches!(h.choice, Choice::Answer | Choice::Intent(_)))
    }

    /// Taller with a card, as short as before without.
    fn fit(&mut self, card: bool) {
        let height = if card {
            TALL
        } else {
            FIELD + MAX_RESULTS as f32 * ROW + 2.0 * 12.0
        };
        let size = Size::new(WIDTH + 2.0 * ROOM, height + 2.0 * ROOM);
        if self.tree.size() != size {
            let scale = self.tree.scale();
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    /// What the card shows for the first hit.
    fn card_for(&self, hit: &Hit) -> Option<(Card, Vec<CardAction>)> {
        let lang = self.lang;
        match &hit.choice {
            Choice::Intent(intent) => Some((
                Card {
                    mood: Mood::Wait,
                    caption: lang.intent_caption().into(),
                    blocks: vec![answer::Block::Paragraph(intent.title(lang))],
                    chips: intent.facts(lang),
                    error: None,
                    buttons: vec![intent.confirm(lang).into(), lang.cancel().into()],
                    width: PANEL,
                    compact: false,
                },
                vec![CardAction::Confirm, CardAction::Cancel],
            )),
            Choice::Answer => {
                let a = self.answer.as_ref()?;
                if let Some(offer) = self.confirming.and_then(|i| self.offers.get(i)) {
                    return Some(confirmation(offer, lang));
                }
                let mut blocks = answer::blocks(a.shown());
                if blocks.is_empty() && a.state == answer::State::Streaming {
                    blocks.push(answer::Block::Paragraph(lang.thinking().into()));
                }
                let (error, buttons, actions) = match &a.state {
                    answer::State::Streaming => {
                        (None, vec![lang.stop().into()], vec![CardAction::Stop])
                    }
                    // What it proposes first, the main one first (#197).
                    answer::State::Done(_) => (
                        None,
                        self.offers
                            .iter()
                            .map(|o| o.label.clone())
                            .chain([lang.copy().into(), lang.add_to_notes().into()])
                            .collect(),
                        (0..self.offers.len())
                            .map(CardAction::Offer)
                            .chain([CardAction::Copy, CardAction::Note])
                            .collect(),
                    ),
                    answer::State::Failed(why) => (
                        Some(why.clone()),
                        vec![lang.ask_again().into()],
                        vec![CardAction::Retry],
                    ),
                };
                Some((
                    Card {
                        mood: a.mood(),
                        caption: a.caption(lang),
                        blocks,
                        chips: Vec::new(),
                        error,
                        buttons,
                        width: PANEL,
                        compact: false,
                    },
                    actions,
                ))
            }
            _ => None,
        }
    }

    /// Shows `MAX_RESULTS` hits from `first`; or the card for the first,
    /// and a few others under "Also".
    fn render(&mut self) {
        for row in std::mem::take(&mut self.rows) {
            self.tree.remove(row);
        }
        for child in self.tree.children(self.list) {
            self.tree.remove(child);
        }
        for child in self.tree.children(self.panel) {
            self.tree.remove(child);
        }
        self.card_buttons.clear();
        self.row_hits.clear();
        self.tiles.clear();
        self.tile_apps.clear();
        self.answer_text = None;
        let card = if self.has_card() {
            self.hits.first().and_then(|h| self.card_for(h))
        } else {
            None
        };
        self.fit(card.is_some());
        let carded = card.is_some();
        for rule in [self.rule, self.also_rule] {
            self.tree.set_style(rule, rule_style(card.is_some()));
            self.tree.set_look(
                rule,
                Look {
                    background: card.is_some().then_some(Paint::Border),
                    ..Look::default()
                },
            );
        }
        self.tree.set_style(self.panel, panel_style(card.is_some()));
        self.tree.set_style(self.list, list_style(card.is_some()));
        if card.is_some() && self.hits.len() <= 1 {
            // Nothing else: no "Also".
            self.tree.set_style(self.also_rule, rule_style(false));
            self.tree.set_style(
                self.list,
                Style {
                    height: Length::Px(0.0),
                    ..Style::default()
                },
            );
        }
        let grid = card.is_none() && self.grid_shown();
        self.tree.set_hidden(self.list, grid);
        self.tree.set_hidden(self.grid_box, !grid);
        if grid {
            self.render_grid();
            return self.show_selection();
        }
        let theme = self.tree.theme().clone();
        let (start, shown, row_height) = match &card {
            Some(_) => (1, MAX_ALSO, ALSO_ROW),
            None => (self.first, MAX_RESULTS, ROW),
        };
        if let Some((card, actions)) = card {
            let filled = answer::fill(&mut self.tree, self.panel, &card);
            self.card_buttons = filled.buttons.into_iter().zip(actions).collect();
            if self
                .hits
                .first()
                .is_some_and(|h| h.choice == Choice::Answer)
            {
                self.answer_text = Some((filled.text, filled.content));
                if self.answer_tall {
                    self.tree
                        .set_style(filled.text, answer::text_box(ANSWER_MAX));
                    self.tree
                        .jump(filled.content, Property::OffsetY, -self.answer_scroll);
                }
            }
            if self.hits.len() > 1
                && let Some(caption) = self.tree.add(
                    self.list,
                    Style {
                        padding: Insets {
                            bottom: 6.0,
                            left: 16.0,
                            ..Insets::default()
                        },
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
            {
                let _ = self.tree.add(
                    caption,
                    Style::default(),
                    Kind::text(self.lang.also().to_uppercase(), theme.text.caption),
                );
            }
        }
        let query = self.query().trim().to_owned();
        let hits: Vec<(usize, Hit)> = self
            .hits
            .iter()
            .cloned()
            .enumerate()
            .skip(start)
            .take(shown)
            .collect();
        if hits.is_empty() && !query.is_empty() && start == 0 {
            let _ = self.tree.add(
                self.list,
                Style {
                    height: Length::Px(ROW),
                    padding: Insets::symmetric(0.0, 16.0),
                    justify: hocus_ui::Justify::Center,
                    ..Style::default()
                },
                Kind::text(
                    self.lang.nothing_found(),
                    theme.text.body.with_color(Paint::Muted),
                ),
            );
        }
        let mut captioned = false;
        for (index, hit) in hits {
            if let Choice::OpenFile(path) = &hit.choice {
                if !captioned && !carded {
                    captioned = true;
                    self.add_caption(self.lang.documents_caption(), &theme);
                }
                let document = self.documents.iter().find(|d| d.path == *path).cloned();
                if let Some(document) = document {
                    self.add_document_row(index, &document, row_height, &theme);
                    continue;
                }
            }
            let (name, kind) = (hit.title.clone(), hit.kind.clone());
            let color = hit.color.unwrap_or(theme.colors.accent);
            let Some(row) = self.tree.add(
                self.list,
                Style {
                    direction: Direction::Row,
                    height: Length::Px(row_height),
                    padding: Insets::symmetric(0.0, 16.0),
                    gap: 12.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: (index == self.selected).then_some(Paint::SurfaceHover),
                    radius: theme.radii.medium,
                    ..Look::default()
                }),
            ) else {
                continue;
            };
            // An action says its keys too, for screen readers.
            let said = match &hit.choice {
                Choice::Run(action) => match self.keys_of(action) {
                    Some(combo) => format!("{name}, {combo}"),
                    None => format!("{name}, {}", self.lang.no_shortcut()),
                },
                _ => name.clone(),
            };
            self.tree.set_clickable(row, &said);
            // Applications and results have their colour; copies and emoji
            // speak for themselves.
            if self.mode == Mode::Commands {
                let _ = self.tree.add(
                    row,
                    Style {
                        width: Length::Px(26.0),
                        height: Length::Px(26.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(color)),
                        radius: 7.0,
                        ..Look::default()
                    }),
                );
            }
            // A task's lists, by their colour (#256).
            if self.mode == Mode::Tasks {
                let _ = self.tree.add(
                    row,
                    Style {
                        width: Length::Px(10.0),
                        height: Length::Px(10.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(hit.color.map_or(Paint::Muted, Paint::Rgba)),
                        radius: 5.0,
                        ..Look::default()
                    }),
                );
            }
            let _ = self.tree.add(
                row,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::text_line(name, theme.text.title.with_weight(500)),
            );
            let _ = self.tree.add(
                row,
                Style::default(),
                Kind::text(
                    kind,
                    theme.text.label.with_weight(500).with_color(Paint::Muted),
                ),
            );
            if let Choice::Run(action) = &hit.choice {
                self.add_caps(row, action, &theme);
            }
            self.rows.push(row);
            self.row_hits.push(index);
        }
        self.show_selection();
    }

    /// A group's name above its rows, as "Also".
    fn add_caption(&mut self, text: &str, theme: &Theme) {
        self.caption_in(self.list, text, theme);
    }

    /// A group's name in `parent`.
    fn caption_in(&mut self, parent: NodeId, text: &str, theme: &Theme) -> Option<NodeId> {
        let caption = self.tree.add(
            parent,
            Style {
                padding: Insets {
                    top: 4.0,
                    bottom: 4.0,
                    left: 16.0,
                    ..Insets::default()
                },
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )?;
        let _ = self.tree.add(
            caption,
            Style::default(),
            Kind::text(text.to_uppercase(), theme.text.caption),
        );
        Some(caption)
    }

    /// Every application, `GRID_ROWS` rows of tiles from `grid_first`,
    /// under their caption; the ring on the chosen one. The tiles are kept
    /// and shown again as they are: only those that show another
    /// application change, and only what changed in them.
    fn render_grid(&mut self) {
        let Some(mut grid) = self.grid.take().or_else(|| self.new_grid()) else {
            return;
        };
        let start = self.grid_first * COLUMNS;
        let end = (start + COLUMNS * GRID_ROWS).min(self.apps.len());
        let wanted = end.saturating_sub(start);
        // Fewer tiles: the last ones go, and the rows left empty.
        while grid.tiles.len() > wanted {
            if let Some(tile) = grid.tiles.pop() {
                self.tree.remove(tile.node);
            }
        }
        while grid.rows.len() > wanted.div_ceil(COLUMNS) {
            if let Some(row) = grid.rows.pop() {
                self.tree.remove(row);
            }
        }
        while grid.tiles.len() < wanted {
            let i = grid.tiles.len();
            if i / COLUMNS == grid.rows.len() {
                // Before the ring, which is drawn over the tiles.
                let Some(row) = self.tree.insert(
                    grid.body,
                    grid.rows.len(),
                    Style {
                        direction: Direction::Row,
                        height: Length::Px(TILE_H),
                        gap: TILE_GAP,
                        grow: 0.0,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                ) else {
                    break;
                };
                grid.rows.push(row);
            }
            let Some(tile) = self.new_tile(grid.rows[i / COLUMNS]) else {
                break;
            };
            grid.tiles.push(tile);
        }
        for (i, tile) in grid.tiles.iter_mut().enumerate() {
            self.show_app(tile, start + i);
            self.tiles.push(tile.node);
            self.tile_apps.push(start + i);
        }
        self.grid = Some(grid);
    }

    /// The grid's caption, its body and its ring, without tiles yet.
    fn new_grid(&mut self) -> Option<Grid> {
        let theme = self.tree.theme().clone();
        let caption = self.caption_in(self.grid_box, self.lang.all_applications(), &theme)?;
        let body = self.tree.add(
            self.grid_box,
            Style {
                gap: TILE_GAP,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )?;
        let ring = self.tree.add(
            body,
            Style {
                at: Some(self.tile_at(self.selected)),
                width: Length::Px(TILE_W),
                height: Length::Px(TILE_H),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(Ring {
                    radius: theme.radii.medium,
                }),
                label: None,
            },
        )?;
        Some(Grid {
            caption,
            body,
            rows: Vec::new(),
            tiles: Vec::new(),
            ring,
        })
    }

    /// Where application `index`'s tile is in the grid shown, from its
    /// corner.
    fn tile_at(&self, index: usize) -> Point {
        let shown = index.saturating_sub(self.grid_first * COLUMNS);
        let (row, column) = (shown / COLUMNS, shown % COLUMNS);
        Point::new(
            column as f32 * (TILE_W + TILE_GAP),
            row as f32 * (TILE_H + TILE_GAP),
        )
    }

    /// A tile in `row`, showing no application yet: its icon over its name.
    fn new_tile(&mut self, row: NodeId) -> Option<GridTile> {
        let theme = self.tree.theme();
        let (radius, label) = (theme.radii.medium, theme.text.label.with_weight(500));
        let node = self.tree.add(
            row,
            Style {
                width: Length::Px(TILE_W),
                height: Length::Px(TILE_H),
                padding: Insets::symmetric(0.0, 10.0),
                gap: 10.0,
                grow: 0.0,
                align: Align::Center,
                justify: hocus_ui::Justify::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                radius,
                ..Look::default()
            }),
        )?;
        let icon = self
            .tree
            .add(node, icon_style(), Kind::Box(Look::default()))?;
        let name = self.tree.add(
            node,
            Style {
                grow: 0.0,
                ..Style::default()
            },
            Kind::text_line("", label),
        )?;
        Some(GridTile {
            node,
            icon,
            name,
            face: None,
        })
    }

    /// Application `index` in `tile`: its name, and its icon, or its
    /// glyph on its colour as the dock draws it; what it showed already
    /// stays as it is.
    fn show_app(&mut self, tile: &mut GridTile, index: usize) {
        let Some(app) = self.apps.get(index) else {
            return;
        };
        self.tree.set_clickable(tile.node, &app.name);
        self.tree.set_text(tile.name, &app.name);
        let picture = app.icon.as_ref().and_then(|n| self.icons.get(n));
        let same = match (&tile.face, picture) {
            (Some(Face::Picture(shown)), Some(picture)) => shown == picture,
            (Some(Face::Glyph { id, name }), None) => *id == app.id && *name == app.name,
            _ => false,
        };
        if same {
            return;
        }
        self.tree.remove(tile.icon);
        let icon = match picture {
            Some(picture) => {
                tile.face = Some(Face::Picture(picture.clone()));
                self.tree
                    .insert(tile.node, 0, icon_style(), Kind::picture(picture, ""))
            }
            None => {
                tile.face = Some(Face::Glyph {
                    id: app.id.clone(),
                    name: app.name.clone(),
                });
                let (glyph, color) = crate::dock::look_of(&app.id, &app.name);
                let square = self.tree.insert(
                    tile.node,
                    0,
                    icon_style(),
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(color)),
                        radius: 13.0,
                        ..Look::default()
                    }),
                );
                let white = Paint::Rgba(hocus_ui::Color::rgb(0xFF, 0xFF, 0xFF));
                let glyph = match glyph {
                    crate::dock::Glyph::Text(t) => Some(Kind::text_line(
                        t,
                        hocus_ui::TextStyle {
                            typeface: hocus_ui::Typeface::Mono,
                            size: 14.0,
                            weight: 700,
                            ..self.tree.theme().text.title
                        }
                        .with_color(white),
                    )),
                    crate::dock::Glyph::Path(path) => {
                        Icon::stroked(path, 22.0, 1.8).map(|icon| Kind::Icon {
                            icon,
                            color: white,
                            label: None,
                        })
                    }
                };
                if let (Some(square), Some(glyph)) = (square, glyph) {
                    let _ = self.tree.add(square, Style::default(), glyph);
                }
                square
            }
        };
        if let Some(icon) = icon {
            tile.icon = icon;
        }
    }

    /// Moves the grid's choice to `index`, the rows following it.
    fn grid_choose(&mut self, index: usize) {
        self.selected = index.min(self.apps.len().saturating_sub(1));
        self.show_selection();
    }

    /// An arrow, Tab, Home or End in the grid: where the choice goes.
    fn grid_move(&self, key: Key) -> Option<usize> {
        let (n, at) = (self.apps.len(), self.selected);
        let last = n.checked_sub(1)?;
        Some(match key {
            Key::Right => (at + 1).min(last),
            Key::Left => at.saturating_sub(1),
            Key::Tab => (at + 1) % n,
            Key::BackTab => (at + n - 1) % n,
            // Down from a row above a shorter last one: its last tile.
            Key::Down if at + COLUMNS <= last => at + COLUMNS,
            Key::Down if at / COLUMNS < last / COLUMNS => last,
            Key::Down => at,
            Key::Up => at.checked_sub(COLUMNS).unwrap_or(at),
            Key::Home => 0,
            Key::End => last,
            _ => return None,
        })
    }

    /// A document found: its name over a line around the words matched,
    /// those in the text's colour; its folder on the right.
    fn add_document_row(&mut self, index: usize, document: &Document, height: f32, theme: &Theme) {
        let Some(row) = self.tree.add(
            self.list,
            Style {
                direction: Direction::Row,
                height: Length::Px(height),
                padding: Insets::symmetric(0.0, 16.0),
                gap: 12.0,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: (index == self.selected).then_some(Paint::SurfaceHover),
                radius: theme.radii.medium,
                ..Look::default()
            }),
        ) else {
            return;
        };
        let line: String = document.snippet.iter().map(|(t, _)| t.as_str()).collect();
        let said = self.lang.document(&document.name, &document.folder, &line);
        self.tree.set_clickable(row, &said);
        if let Some(square) = self.tree.add(
            row,
            Style {
                width: Length::Px(26.0),
                height: Length::Px(26.0),
                align: Align::Center,
                justify: hocus_ui::Justify::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(crate::providers::DOCUMENT_COLOR)),
                radius: 7.0,
                ..Look::default()
            }),
        ) && let Some(icon) = Icon::stroked(icons::FILE, 16.0, 1.8)
        {
            let _ = self.tree.add(
                square,
                Style::default(),
                Kind::Icon {
                    icon,
                    color: Paint::Rgba(hocus_ui::Color::rgb(0xFF, 0xFF, 0xFF)),
                    label: None,
                },
            );
        }
        let Some(text) = self.tree.add(
            row,
            Style {
                width: Length::Fill,
                gap: 2.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        ) else {
            return;
        };
        let _ = self.tree.add(
            text,
            Style::default(),
            Kind::text_line(document.name.clone(), theme.text.body.with_weight(600)),
        );
        if let Some(snippet) = self.tree.add(
            text,
            Style {
                direction: Direction::Row,
                clip: true,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        ) {
            let pieces = document.snippet.len();
            for (i, (piece, matched)) in document.snippet.iter().enumerate() {
                let style = if *matched {
                    theme.text.label.with_weight(700)
                } else {
                    theme.text.label.with_weight(400).with_color(Paint::Muted)
                };
                // The last piece takes what is left, cut with "…".
                let width = if i + 1 == pieces {
                    Length::Fill
                } else {
                    Length::Auto
                };
                let _ = self.tree.add(
                    snippet,
                    Style {
                        width,
                        ..Style::default()
                    },
                    Kind::text_line(piece.clone(), style),
                );
            }
        }
        let _ = self.tree.add(
            row,
            Style::default(),
            Kind::text(
                document.folder.clone(),
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );
        self.rows.push(row);
        self.row_hits.push(index);
    }

    /// An action's keys as caps, or "no shortcut".
    fn add_caps(&mut self, row: NodeId, action: &Action, theme: &Theme) {
        let Some(keys) = self.tree.add(
            row,
            Style {
                direction: Direction::Row,
                width: Length::Px(200.0),
                gap: 6.0,
                align: Align::Center,
                justify: hocus_ui::Justify::End,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        ) else {
            return;
        };
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.label.with_weight(500)
        };
        let Some(combo) = self.keys_of(action) else {
            let _ = self.tree.add(
                keys,
                Style::default(),
                Kind::text(
                    self.lang.no_shortcut(),
                    theme.text.label.with_color(Paint::Muted),
                ),
            );
            return;
        };
        for cap in crate::actions::caps(combo, self.lang) {
            let Some(cap_box) = self.tree.add(
                keys,
                Style {
                    padding: Insets::symmetric(4.0, 9.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfacePressed),
                    radius: 7.0,
                    border: Some(Paint::Border),
                    ..Look::default()
                }),
            ) else {
                continue;
            };
            let _ = self
                .tree
                .add(cap_box, Style::default(), Kind::text(cap, mono));
        }
    }

    /// Marks the chosen row, scrolling the list to it.
    fn show_selection(&mut self) {
        if self.grid_shown() {
            return self.show_tile();
        }
        if !self.has_card() {
            if self.selected < self.first {
                self.first = self.selected;
                return self.render();
            }
            if self.selected >= self.first + MAX_RESULTS {
                self.first = self.selected + 1 - MAX_RESULTS;
                return self.render();
            }
        }
        let theme = self.tree.theme().clone();
        for (i, row) in self.rows.iter().enumerate() {
            let chosen = self.row_hits.get(i) == Some(&self.selected);
            self.tree.set_look(
                *row,
                Look {
                    background: chosen.then_some(Paint::SurfaceHover),
                    radius: theme.radii.medium,
                    ..Look::default()
                },
            );
            self.tree.set_selected_box(*row, chosen);
        }
    }

    /// Marks the chosen tile with the ring, scrolling the grid to its row.
    fn show_tile(&mut self) {
        let row = self.selected / COLUMNS;
        if row < self.grid_first {
            self.grid_first = row;
            return self.render();
        }
        if row >= self.grid_first + GRID_ROWS {
            self.grid_first = row + 1 - GRID_ROWS;
            return self.render();
        }
        let radius = self.tree.theme().radii.medium;
        for (tile, index) in self.tiles.iter().zip(&self.tile_apps) {
            let chosen = *index == self.selected;
            self.tree.set_look(
                *tile,
                Look {
                    background: chosen.then_some(Paint::SurfaceHover),
                    radius,
                    ..Look::default()
                },
            );
            self.tree.set_selected_box(*tile, chosen);
        }
        if let Some(ring) = self.grid.as_ref().map(|g| g.ring) {
            self.tree.set_style(
                ring,
                Style {
                    at: Some(self.tile_at(self.selected)),
                    width: Length::Px(TILE_W),
                    height: Length::Px(TILE_H),
                    ..Style::default()
                },
            );
        }
    }

    /// The application of the grid at `index`, launched.
    fn grid_launch(&mut self, index: usize) -> Vec<LauncherEvent> {
        let Some(app) = self.apps.get(index).cloned() else {
            return Vec::new();
        };
        self.close();
        vec![LauncherEvent::Chosen(Choice::Launch(app))]
    }

    fn selected_paste(&self) -> Option<String> {
        match &self.hits.get(self.selected)?.choice {
            Choice::Paste(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn choose(&self, i: usize) -> Option<LauncherEvent> {
        let hit = self.hits.get(i)?;
        Some(LauncherEvent::Chosen(hit.choice.clone()))
    }

    /// Text typed while open.
    pub fn text(&mut self, text: &str) {
        if !self.open {
            return;
        }
        let changed = self
            .tree
            .text_input(text)
            .iter()
            .any(|e| matches!(e, Event::Changed(_)));
        if changed {
            self.search();
        }
    }

    /// A key pressed while open.
    pub fn key(&mut self, key: Key) -> Vec<LauncherEvent> {
        if !self.open {
            return Vec::new();
        }
        if self.grid_shown() {
            if let Some(to) = self.grid_move(key) {
                self.grid_choose(to);
                return Vec::new();
            }
            if key == Key::Enter {
                return self.grid_launch(self.selected);
            }
        }
        match key {
            Key::Escape => {
                self.close();
                vec![LauncherEvent::Closed]
            }
            // Tab pins in the history, and changes the skin tone of emoji.
            Key::Tab if self.mode == Mode::Clipboard => self
                .selected_paste()
                .map(LauncherEvent::Pin)
                .into_iter()
                .collect(),
            // Notes: Enter adds what is typed; on an empty field, it copies
            // the line chosen.
            Key::Enter if self.mode == Mode::Notes && !self.query().trim().is_empty() => {
                let typed = self.query().to_owned();
                self.tree.set_field_value(self.field, "");
                vec![LauncherEvent::Chosen(Choice::AddNote(typed))]
            }
            Key::Delete if self.mode == Mode::Notes && self.query().is_empty() => self
                .notes
                .iter()
                .rev()
                .nth(self.selected)
                .map(|n| LauncherEvent::RemoveNote(n.index))
                .into_iter()
                .collect(),
            // A task: Enter keeps it and closes; nothing typed, nothing kept.
            Key::Enter if self.mode == Mode::Tasks => {
                let text = self.query().trim().to_owned();
                let list = match self.hits.get(self.selected).map(|h| &h.choice) {
                    Some(Choice::TaskList(key)) if self.lists_shown() => Some(key.clone()),
                    _ => None,
                };
                let id = self.editing.take();
                self.close();
                if text.is_empty() {
                    vec![LauncherEvent::Closed]
                } else {
                    vec![
                        LauncherEvent::Closed,
                        LauncherEvent::Chosen(Choice::SaveTask { id, text, list }),
                    ]
                }
            }
            Key::Delete if self.mode == Mode::Clipboard => self
                .selected_paste()
                .map(LauncherEvent::Forget)
                .into_iter()
                .collect(),
            Key::Tab if self.mode == Mode::Emoji => {
                self.tone = (self.tone + 1) % 6;
                self.show_hint();
                let selected = self.selected;
                self.search();
                self.selected = selected.min(self.hits.len().saturating_sub(1));
                self.show_selection();
                Vec::new()
            }
            // Tab in the palette: give the action keys in the settings.
            Key::Tab if self.mode == Mode::Actions => {
                self.close();
                vec![LauncherEvent::Chosen(Choice::SetShortcut)]
            }
            Key::Down | Key::Tab if !self.hits.is_empty() => {
                self.selected = (self.selected + 1) % self.hits.len();
                self.show_selection();
                Vec::new()
            }
            Key::Up | Key::BackTab if !self.hits.is_empty() => {
                self.selected = (self.selected + self.hits.len() - 1) % self.hits.len();
                self.show_selection();
                Vec::new()
            }
            Key::Enter if self.mode == Mode::Commands => self.act(self.selected),
            Key::Enter => {
                let chosen = self.choose(self.selected);
                if chosen.is_some() {
                    self.close();
                }
                chosen.into_iter().collect()
            }
            other => {
                let changed = self
                    .tree
                    .input(Input::Key(other))
                    .iter()
                    .any(|e| matches!(e, Event::Changed(_)));
                if changed {
                    self.search();
                }
                Vec::new()
            }
        }
    }

    /// The card, without the room for its shadow, relative to the surface.
    fn card(&self) -> Rect {
        let s = self.tree.size();
        self.tree
            .rect(self.card)
            .unwrap_or_else(|| Rect::new(0.0, 0.0, s.w, s.h).inset(Insets::all(ROOM)))
    }

    pub fn hit(&self, at: Point) -> bool {
        self.open && self.card().contains(at)
    }

    /// Pointer input relative to the surface; a click outside closes it.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<LauncherEvent> {
        if !self.open {
            return Vec::new();
        }
        if matches!(input, Input::PointerButton { pressed: true }) && !self.hit(at) {
            self.close();
            return vec![LauncherEvent::Closed];
        }
        let forwarded = match input {
            Input::PointerMove(_) if !self.hit(at) => Input::PointerLeave,
            other => other,
        };
        let mut events = Vec::new();
        for event in self.tree.input(forwarded) {
            let Event::Activated(node) = event else {
                continue;
            };
            if let Some((_, action)) = self.card_buttons.iter().find(|(b, _)| *b == node) {
                events.extend(self.card_action(*action));
                continue;
            }
            if let Some(i) = self.tiles.iter().position(|t| *t == node)
                && let Some(index) = self.tile_apps.get(i).copied()
            {
                events.extend(self.grid_launch(index));
                continue;
            }
            if self.mode == Mode::Commands
                && let Some(i) = self.rows.iter().position(|r| *r == node)
                && let Some(index) = self.row_hits.get(i).copied()
            {
                events.extend(self.act(index));
                continue;
            }
            // A list for the task: chosen, the card stays.
            if self.mode == Mode::Tasks
                && let Some(i) = self.rows.iter().position(|r| *r == node)
            {
                if let Some(index) = self.row_hits.get(i).copied() {
                    self.selected = index;
                    self.show_selection();
                }
                continue;
            }
            if let Some(i) = self.rows.iter().position(|r| *r == node)
                && let Some(chosen) = self.choose(self.first + i)
            {
                self.close();
                events.push(chosen);
            }
        }
        // A click focuses what it hits: the keyboard stays in the field.
        self.tree.set_focus(Some(self.field));
        events
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.now = now;
        // What streamed in since the last turn, laid out once.
        if self.answer_changed && self.open {
            self.answer_changed = false;
            self.render();
        }
        let mut damage = self.tree.update(now);
        if let Some((text, content)) = self.answer_text
            && !self.answer_tall
            && self.tree.rect(content).is_some_and(|r| r.h > ANSWER_MAX)
        {
            // Too long: it scrolls in a box of its own.
            self.answer_tall = true;
            self.tree.set_style(text, answer::text_box(ANSWER_MAX));
            damage.extend(self.tree.update(now));
        }
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Scrolls a long answer by `dy` logical pixels, down positive;
    /// whether there was one to scroll.
    pub fn scroll(&mut self, dy: f32) -> bool {
        let Some((_, content)) = self.answer_text.filter(|_| self.answer_tall && self.open) else {
            return false;
        };
        let Some(inner) = self.tree.rect(content) else {
            return false;
        };
        let room = (inner.h - ANSWER_MAX).max(0.0);
        let scroll = (self.answer_scroll + dy).clamp(0.0, room);
        if scroll != self.answer_scroll {
            self.answer_scroll = scroll;
            self.tree.jump(content, Property::OffsetY, -scroll);
        }
        true
    }

    /// The wheel over the card, `at` relative to the surface: a long
    /// answer scrolls; the grid moves a row a notch, its choice with it.
    pub fn wheel(&mut self, at: Point, dy: f32) -> bool {
        if !self.hit(at) {
            return false;
        }
        if self.open && self.grid_shown() {
            self.wheel_left += dy;
            while self.wheel_left.abs() >= WHEEL_ROW {
                let down = self.wheel_left > 0.0;
                self.wheel_left -= WHEEL_ROW.copysign(self.wheel_left);
                let key = if down { Key::Down } else { Key::Up };
                if let Some(to) = self.grid_move(key) {
                    self.grid_choose(to);
                }
            }
            return true;
        }
        self.scroll(dy)
    }

    /// Page Up or Page Down while the field has the keyboard: a long
    /// answer scrolls, the grid's choice goes a page of rows.
    pub fn page(&mut self, down: bool) -> bool {
        if self.open && self.grid_shown() {
            let page = COLUMNS * GRID_ROWS;
            let last = self.apps.len().saturating_sub(1);
            let to = if down {
                (self.selected + page).min(last)
            } else {
                self.selected.saturating_sub(page)
            };
            self.grid_choose(to);
            return true;
        }
        self.scroll(if down { ANSWER_PAGE } else { -ANSWER_PAGE })
    }

    /// Enter on a hit, or a click on its row, in the command field.
    fn act(&mut self, index: usize) -> Vec<LauncherEvent> {
        let Some(hit) = self.hits.get(index) else {
            return Vec::new();
        };
        match hit.choice.clone() {
            Choice::Ask(question) => self.start_answer(question),
            Choice::Answer => {
                let primary = self.card_buttons.first().map(|(_, a)| *a);
                primary.map(|a| self.card_action(a)).unwrap_or_default()
            }
            chosen => {
                self.close();
                vec![LauncherEvent::Chosen(chosen)]
            }
        }
    }

    /// Asks `question` again or for the first time: its answer shows in
    /// the card as it comes.
    fn start_answer(&mut self, question: String) -> Vec<LauncherEvent> {
        let Some(source) = self.ask.clone() else {
            return Vec::new();
        };
        self.asked += 1;
        self.answer = Some(Answer::new(question.clone(), source));
        self.offers.clear();
        self.confirming = None;
        self.answer_tall = false;
        self.answer_scroll = 0.0;
        self.search();
        vec![LauncherEvent::Ask {
            id: self.asked,
            question,
        }]
    }

    fn card_action(&mut self, action: CardAction) -> Vec<LauncherEvent> {
        let text = self.answer.as_ref().map(Answer::plain).unwrap_or_default();
        match action {
            CardAction::Confirm => match self.hits.first().map(|h| h.choice.clone()) {
                Some(intent @ Choice::Intent(_)) => {
                    self.close();
                    vec![LauncherEvent::Chosen(intent)]
                }
                _ => Vec::new(),
            },
            CardAction::Cancel => {
                self.close();
                vec![LauncherEvent::Closed]
            }
            CardAction::Copy => {
                self.close();
                vec![LauncherEvent::Chosen(Choice::Copy(text))]
            }
            CardAction::Note => {
                self.close();
                vec![LauncherEvent::NoteAnswer(text)]
            }
            CardAction::Stop => {
                if let Some(a) = self.answer.as_mut() {
                    a.apply(answer::News::Failed(self.lang.answer_stopped().into()));
                }
                self.render();
                Vec::new()
            }
            CardAction::Retry => {
                let question = self.answer.as_ref().map(|a| a.question.clone());
                question.map(|q| self.start_answer(q)).unwrap_or_default()
            }
            CardAction::Offer(i) => {
                if i < self.offers.len() {
                    self.confirming = Some(i);
                    self.answer_tall = false;
                    self.answer_scroll = 0.0;
                    self.render();
                }
                Vec::new()
            }
            CardAction::Run(j) => {
                let run = self
                    .confirming
                    .and_then(|i| self.offers.get(i))
                    .and_then(|o| o.confirms.get(j))
                    .map(|(_, run)| run.clone());
                match run {
                    Some(run) => {
                        self.close();
                        vec![LauncherEvent::Act(run)]
                    }
                    None => Vec::new(),
                }
            }
            CardAction::Back => {
                self.confirming = None;
                self.answer_tall = false;
                self.answer_scroll = 0.0;
                self.render();
                Vec::new()
            }
        }
    }

    /// What the answer to question `id` proposes, once it is all there,
    /// as the desktop can offer it.
    pub fn set_offers(&mut self, id: u64, offers: Vec<Offer>) {
        if id != self.asked || self.answer.is_none() {
            return;
        }
        self.offers = offers;
        self.confirming = None;
        self.answer_changed = true;
    }

    /// More of the answer to question `id`, its end, or why it stopped;
    /// an older question's is dropped.
    pub fn answer_news(&mut self, id: u64, news: answer::News) {
        if id != self.asked {
            return;
        }
        if let Some(answer) = self.answer.as_mut() {
            answer.apply(news);
            self.answer_changed = true;
        }
    }

    /// The question whose answer is awaited, while the card waits for it.
    pub fn awaited(&self) -> Option<u64> {
        let streaming = self
            .answer
            .as_ref()
            .is_some_and(|a| a.state == answer::State::Streaming);
        (self.open && streaming).then_some(self.asked)
    }

    /// The answer shown, if any.
    pub fn answer(&self) -> Option<&Answer> {
        self.answer.as_ref()
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    /// Where its card, field and results are, as last drawn.
    pub fn parts(&self) -> Option<Parts> {
        let card = self.tree.rect(self.card)?;
        let head = self.tree.rect(self.head)?;
        let rows = self
            .rows
            .iter()
            .chain(&self.tiles)
            .filter_map(|r| self.tree.rect(*r))
            .filter(|r| r.h > 0.0 && r.y < card.y + card.h)
            .collect();
        Some(Parts { card, head, rows })
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

/// The grid of every application, as made: kept between openings.
#[derive(Debug)]
struct Grid {
    caption: NodeId,
    body: NodeId,
    rows: Vec<NodeId>,
    tiles: Vec<GridTile>,
    ring: NodeId,
}

/// A tile of the grid and what it shows.
#[derive(Debug)]
struct GridTile {
    node: NodeId,
    icon: NodeId,
    name: NodeId,
    face: Option<Face>,
}

/// What a tile's icon shows: a picture, or the glyph of an application
/// without one.
#[derive(Debug)]
enum Face {
    Picture(Picture),
    Glyph { id: String, name: String },
}

/// A tile's icon, or its square.
fn icon_style() -> Style {
    Style {
        width: Length::Px(GRID_ICON),
        height: Length::Px(GRID_ICON),
        grow: 0.0,
        align: Align::Center,
        justify: hocus_ui::Justify::Center,
        ..Style::default()
    }
}

/// The keyboard's ring around the grid's chosen tile, in the accent and
/// the theme's width for focus, inside the tile's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ring {
    radius: f32,
}

impl hocus_ui::Draw for Ring {
    fn draw(&self, canvas: &mut hocus_ui::Canvas, rect: Rect, theme: &Theme, _: Duration) {
        canvas.stroke_rect(rect, self.radius, theme.focus_ring, theme.colors.accent);
    }

    fn size(&self) -> Size {
        Size::new(TILE_W, TILE_H)
    }
}

/// What a proposal's confirmation shows (#197): what it will do, the
/// diff or the text it sends, then its buttons and Cancel, which goes back
/// to the answer.
fn confirmation(offer: &Offer, lang: Lang) -> (Card, Vec<CardAction>) {
    let mut blocks = vec![answer::Block::Paragraph(offer.title.clone())];
    blocks.extend(offer.detail.clone().map(answer::Block::Code));
    let buttons = offer
        .confirms
        .iter()
        .map(|(label, _)| label.clone())
        .chain([lang.cancel().to_owned()])
        .collect();
    let actions = (0..offer.confirms.len())
        .map(CardAction::Run)
        .chain([CardAction::Back])
        .collect();
    (
        Card {
            mood: Mood::Wait,
            caption: lang.intent_caption().into(),
            blocks,
            chips: offer.chips.clone(),
            error: None,
            buttons,
            width: PANEL,
            compact: false,
        },
        actions,
    )
}

/// The command field's results within `max` rows: with documents, a row
/// goes to their caption, they keep up to three, and applications make
/// room from the last.
fn fit(hits: &mut Vec<Hit>, max: usize) {
    const MAX_DOCUMENTS: usize = 3;
    let is_document = |h: &Hit| matches!(h.choice, Choice::OpenFile(_));
    let documents = hits.iter().filter(|h| is_document(h)).count();
    if documents == 0 {
        hits.truncate(max);
        return;
    }
    let max = max - 1;
    let keep = documents.min(MAX_DOCUMENTS).min(max);
    let mut seen = 0;
    hits.retain(|h| {
        if is_document(h) {
            seen += 1;
            seen <= keep
        } else {
            true
        }
    });
    while hits.len() > max {
        match hits
            .iter()
            .rposition(|h| matches!(h.choice, Choice::Launch(_)))
        {
            Some(i) => {
                hits.remove(i);
            }
            None => {
                // Others before them: the documents' place stays.
                match hits.iter().rposition(|h| !is_document(h)) {
                    Some(i) => {
                        hits.remove(i);
                    }
                    None => hits.truncate(max),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, generic: Option<&str>) -> App {
        App {
            id: format!("{}.desktop", name.to_lowercase()),
            name: name.into(),
            generic: generic.map(str::to_owned),
            keywords: Vec::new(),
            exec: vec![name.to_lowercase()],
            icon: None,
        }
    }

    fn launcher() -> Launcher {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_apps(vec![
            app("Files", Some("File Manager")),
            app("Firefox", Some("Web Browser")),
            app("Fish", None),
            app("Terminal", None),
        ]);
        l.open();
        l
    }

    #[test]
    fn a_task_goes_to_the_local_list_unless_another_is_chosen() {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        let list = |key: &str, name: &str| TaskListChoice {
            key: key.into(),
            name: name.into(),
            place: "Nextcloud".into(),
            color: None,
        };
        // One list only: none listed, the local one.
        l.set_task_lists(vec![list("local", "Tasks")]);
        l.open_task(None);
        l.text("Buy milk");
        assert!(l.hits.is_empty());
        assert_eq!(
            l.key(Key::Enter),
            vec![
                LauncherEvent::Closed,
                LauncherEvent::Chosen(Choice::SaveTask {
                    id: None,
                    text: "Buy milk".into(),
                    list: None,
                })
            ]
        );
        l.set_task_lists(vec![list("local", "Tasks"), list("chores", "Chores")]);
        l.open_task(None);
        assert_eq!(l.hits.len(), 2);
        l.key(Key::Down);
        // Typing keeps the list chosen.
        l.text("Water the bamboo");
        assert_eq!(
            l.key(Key::Enter),
            vec![
                LauncherEvent::Closed,
                LauncherEvent::Chosen(Choice::SaveTask {
                    id: None,
                    text: "Water the bamboo".into(),
                    list: Some("chores".into()),
                })
            ]
        );
        // Editing, the task stays in its list.
        l.open_task(Some(("t1".into(), "Water the bamboo".into())));
        assert!(l.hits.is_empty());
    }

    #[test]
    fn applications_found_late_leave_the_history_s_chosen_line_alone() {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_history(vec![
            ("third copy".into(), false),
            ("second copy".into(), false),
            ("first copy".into(), false),
        ]);
        l.open_mode(Mode::Clipboard);
        l.key(Key::Down);
        // The worker's scan comes back now, on a loaded machine.
        l.set_apps(vec![app("Files", None), app("Firefox", None)]);
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Paste("second copy".into()))]
        );
    }

    #[test]
    fn applications_found_late_keep_the_command_field_s_chosen_line() {
        let mut l = launcher();
        l.text("fi");
        l.key(Key::Down);
        let before = l.selected;
        l.set_apps(vec![
            app("Files", Some("File Manager")),
            app("Firefox", Some("Web Browser")),
            app("Fish", None),
            app("Terminal", None),
        ]);
        assert_eq!(l.selected, before);
    }

    #[test]
    fn the_best_match_comes_first_and_enter_launches_it() {
        let mut l = launcher();
        l.text("fir");
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Launch(app(
                "Firefox",
                Some("Web Browser")
            )))]
        );
        assert!(!l.is_open());
    }

    #[test]
    fn arithmetic_shows_its_value_first() {
        let mut l = launcher();
        l.text("2*21");
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Copy("42".into()))]
        );
    }

    #[test]
    fn down_chooses_the_next_and_escape_closes() {
        let chosen = |downs: usize| {
            let mut l = launcher();
            l.text("fi");
            for _ in 0..downs {
                l.key(Key::Down);
            }
            match l.key(Key::Enter).pop() {
                Some(LauncherEvent::Chosen(Choice::Launch(app))) => app.name,
                other => format!("{other:?}"),
            }
        };
        // Shorter names first among equal matches: Fish, Files, Firefox.
        assert_eq!(chosen(0), "Fish");
        assert_eq!(chosen(1), "Files");
        let mut l = launcher();
        l.open();
        assert_eq!(l.key(Key::Escape), vec![LauncherEvent::Closed]);
    }

    fn palette(bindings: &[(&str, &str)]) -> Launcher {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_shortcuts(
            bindings
                .iter()
                .map(|(c, a)| (c.parse().expect("keys"), a.parse().expect("an action")))
                .collect(),
        );
        l.open_mode(Mode::Actions);
        l
    }

    #[test]
    fn the_palette_lists_every_action_with_its_keys() {
        let mut l = palette(&[("Super+M", "toggle-maximized"), ("Super+F12", "spawn foot")]);
        // Down goes through all of them, past what the card shows.
        let mut seen = Vec::new();
        for _ in 0..l.hits.len() {
            if let Some(Choice::Run(action)) = l.hits.get(l.selected).map(|h| h.choice.clone()) {
                seen.push(action);
            }
            l.key(Key::Down);
        }
        for action in crate::actions::catalog() {
            assert!(seen.contains(&action), "{action} is listed");
        }
        assert!(
            seen.contains(&Action::Spawn("foot".into())),
            "a bound command too"
        );
        let dump = l.access().dump();
        assert!(dump.contains("Maximize or restore, Super+M"), "{dump}");
        assert!(l.rows.len() <= MAX_RESULTS);
    }

    #[test]
    fn keys_typed_or_pressed_find_their_action_and_recent_ones_come_first() {
        let region = Action::screenshot(hocus_core::shortcuts::ScreenshotMode::Region);
        let mut l = palette(&[
            ("Shift+Print", "screenshot region"),
            ("Super+Shift+S", "screenshot region"),
        ]);
        l.text("Maj+S");
        assert_eq!(
            l.hits.first().map(|h| h.choice.clone()),
            Some(Choice::Run(region.clone()))
        );
        l.press_keys(&"Super+Shift+S".parse().expect("keys"));
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Run(region))]
        );
        let mut l = palette(&[]);
        l.set_action_recents(vec![Action::Lock]);
        l.open_mode(Mode::Actions);
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Run(Action::Lock))]
        );
        let mut l = palette(&[]);
        l.text("zone");
        assert!(matches!(
            l.key(Key::Enter).pop(),
            Some(LauncherEvent::Chosen(Choice::Run(
                Action::Screenshot { .. }
            )))
        ));
    }

    #[test]
    fn a_request_shows_as_a_card_to_confirm_and_runs_on_enter() {
        let mut l = launcher();
        l.set_apps(vec![app("Slack", None)]);
        l.text("capture de slack dans 3 secondes");
        let dump = l.access().dump();
        assert!(dump.contains("Screenshot of Slack in 3\u{a0}s"), "{dump}");
        assert!(dump.contains("Delay: 3\u{a0}s"), "{dump}");
        assert_eq!(l.size().h, TALL + 2.0 * ROOM, "the card makes room");
        assert!(matches!(
            l.key(Key::Enter).as_slice(),
            [LauncherEvent::Chosen(Choice::Intent(Intent::CaptureApp {
                delay: 3,
                ..
            }))]
        ));
        assert!(!l.is_open());
    }

    #[test]
    fn a_question_is_asked_and_its_answer_streams_into_the_card() {
        let mut l = launcher();
        l.set_providers(&Enabled {
            ask: Some(Source::Local),
            ..Enabled::default()
        });
        l.open();
        l.text("why is the build slow?");
        assert!(matches!(
            l.hits.first().map(|h| &h.choice),
            Some(Choice::Ask(_))
        ));
        let asked = l.key(Key::Enter);
        assert_eq!(
            asked,
            vec![LauncherEvent::Ask {
                id: 1,
                question: "why is the build slow?".into()
            }]
        );
        assert!(l.is_open(), "the answer comes in the field");
        assert_eq!(l.awaited(), Some(1));
        l.answer_news(1, answer::News::Text("The cache is **off**.".into()));
        l.answer_news(7, answer::News::Text("an older question's".into()));
        l.answer_news(1, answer::News::Done(Duration::from_secs(3)));
        l.draw(Duration::ZERO);
        let dump = l.access().dump();
        assert!(dump.contains("The cache is off."), "{dump}");
        assert!(!dump.contains("older"), "{dump}");
        assert_eq!(l.awaited(), None, "nothing more awaited");
        // Enter copies it.
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::Copy(
                "The cache is off.".into()
            ))]
        );
        // Typing again leaves the answer.
        l.open();
        l.text("why?");
        l.key(Key::Enter);
        l.text("x");
        assert_eq!(l.awaited(), None);
        assert!(l.answer().is_none());
    }

    #[test]
    fn a_proposed_action_shows_what_it_does_and_runs_only_once_confirmed() {
        let mut l = launcher();
        l.set_providers(&Enabled {
            ask: Some(Source::Local),
            ..Enabled::default()
        });
        l.open();
        l.text("how do I get a light theme?");
        l.key(Key::Enter);
        let text = "Switch the theme.\n\n```hocus-actions\n{\"action\":\"settings\",\"page\":\"appearance\"}\n```\n";
        l.answer_news(1, answer::News::Text(text.into()));
        l.answer_news(1, answer::News::Done(Duration::from_secs(1)));
        let found = crate::proposals::read(text, &[]);
        let offers =
            crate::proposals::offers(&found, &crate::proposals::Context::default(), Lang::En);
        l.set_offers(1, offers);
        l.draw(Duration::ZERO);
        let dump = l.access().dump();
        assert!(!dump.contains("hocus-actions"), "{dump}");
        assert!(dump.contains("Open the settings"), "{dump}");
        // The main button shows what it will do; Cancel goes back.
        assert_eq!(l.key(Key::Enter), vec![]);
        l.draw(Duration::ZERO);
        assert!(l.access().dump().contains("Open the settings: Appearance"));
        assert_eq!(l.card_action(CardAction::Back), vec![]);
        l.draw(Duration::ZERO);
        assert!(!l.access().dump().contains("Open the settings: Appearance"));
        // Chosen again and confirmed, it runs.
        assert_eq!(l.key(Key::Enter), vec![]);
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Act(crate::proposals::Run::Settings(
                "appearance".into()
            ))]
        );
        assert!(!l.is_open());
    }

    #[test]
    fn a_long_answer_scrolls_in_its_box() {
        let mut l = launcher();
        l.set_providers(&Enabled {
            ask: Some(Source::Local),
            ..Enabled::default()
        });
        l.open();
        l.text("why?");
        l.key(Key::Enter);
        let long: String = (1..=30).map(|i| format!("- step {i}\n")).collect();
        l.answer_news(1, answer::News::Text(format!("Because.\n\n{long}")));
        l.answer_news(1, answer::News::Done(Duration::from_secs(1)));
        l.draw(Duration::ZERO);
        let y = |l: &Launcher, label: &str| {
            l.access()
                .walk()
                .into_iter()
                .find(|n| n.label.as_deref() == Some(label))
                .map(|n| n.rect.y)
                .unwrap_or_else(|| panic!("{label} is shown"))
        };
        let size = l.size();
        let last = y(&l, "• step 30");
        assert!(l.page(true), "a long answer scrolls");
        l.draw(Duration::from_millis(16));
        let paged = y(&l, "• step 30");
        assert!(paged < last - 200.0, "{last} then {paged}");
        // As far as it goes, no further.
        assert!(l.scroll(10_000.0));
        l.draw(Duration::from_millis(32));
        let end = y(&l, "• step 30");
        assert!(l.scroll(500.0));
        l.draw(Duration::from_millis(48));
        assert_eq!(y(&l, "• step 30"), end);
        assert!(
            l.wheel(Point::new(400.0, 300.0), -10_000.0),
            "the wheel over the card"
        );
        l.draw(Duration::from_millis(64));
        assert_eq!(y(&l, "• step 30"), last, "back to the top");
        assert_eq!(l.size(), size, "the card does not grow");
        assert!(!l.wheel(Point::new(2.0, 2.0), 50.0), "not outside the card");
    }

    fn document(name: &str) -> Document {
        Document {
            path: format!("/home/alex/Documents/{name}"),
            name: name.into(),
            folder: "Documents".into(),
            snippet: vec![
                ("The ".into(), false),
                ("bamboo".into(), true),
                (" harvest".into(), false),
            ],
        }
    }

    #[test]
    fn documents_found_come_after_the_applications_and_enter_opens_one() {
        let mut l = launcher();
        l.set_providers(&Enabled {
            documents: true,
            ..Enabled::default()
        });
        l.open();
        l.text("f");
        assert_eq!(l.take_documents_query(), None, "one letter is too little");
        l.text("i");
        assert_eq!(l.take_documents_query(), Some((1, "fi".into())));
        assert_eq!(l.take_documents_query(), None, "asked once");
        l.text("l");
        let (id, query) = l.take_documents_query().expect("asked again");
        assert_eq!(query, "fil");
        // An older query's answer is dropped.
        l.set_documents(id - 1, vec![document("old.md")]);
        assert!(l.documents.is_empty());
        l.set_documents(id, vec![document("files.odt"), document("filters.pdf")]);
        let dump = l.access().dump();
        assert!(dump.contains("DOCUMENTS"), "{dump}");
        assert!(
            dump.contains("files.odt, in Documents: The bamboo harvest"),
            "{dump}"
        );
        let kinds: Vec<&str> = l
            .hits
            .iter()
            .map(|h| match h.choice {
                Choice::Launch(_) => "app",
                Choice::OpenFile(_) => "document",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["app", "document", "document", "other"]);
        l.key(Key::Down);
        assert_eq!(
            l.key(Key::Enter),
            vec![LauncherEvent::Chosen(Choice::OpenFile(
                "/home/alex/Documents/files.odt".into()
            ))]
        );
    }

    #[test]
    fn documents_keep_their_place_among_many_applications() {
        let mut hits: Vec<Hit> = (0..9)
            .map(|i| Hit {
                title: format!("App {i}"),
                kind: String::new(),
                color: None,
                choice: Choice::Launch(app(&format!("App{i}"), None)),
            })
            .collect();
        for name in ["a.md", "b.md", "c.md", "d.md"] {
            hits.push(Hit {
                title: name.into(),
                kind: String::new(),
                color: None,
                choice: Choice::OpenFile(name.into()),
            });
        }
        fit(&mut hits, MAX_RESULTS);
        assert_eq!(hits.len(), MAX_RESULTS - 1, "a row for the caption");
        let documents = hits
            .iter()
            .filter(|h| matches!(h.choice, Choice::OpenFile(_)))
            .count();
        assert_eq!(documents, 3);
    }

    fn launched(events: Vec<LauncherEvent>) -> Option<String> {
        match events.as_slice() {
            [LauncherEvent::Chosen(Choice::Launch(app))] => Some(app.name.clone()),
            _ => None,
        }
    }

    #[test]
    fn the_empty_field_shows_every_application_in_order_and_enter_opens_the_first() {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_apps(vec![
            app("Terminal", None),
            app("Éditeur", None),
            app("files", None),
            app("Firefox", None),
        ]);
        l.open();
        let names: Vec<&str> = l
            .tile_apps
            .iter()
            .map(|i| l.apps[*i].name.as_str())
            .collect();
        assert_eq!(names, ["Éditeur", "files", "Firefox", "Terminal"]);
        let dump = l.access().dump();
        assert!(dump.contains("ALL APPLICATIONS"), "{dump}");
        assert_eq!(launched(l.key(Key::Enter)), Some("Éditeur".into()));
        assert!(!l.is_open());
        // Typing searches; emptied, the field shows them all again.
        l.open();
        l.text("ter");
        assert!(l.tiles.is_empty());
        assert_eq!(l.hits.first().map(|h| h.title.as_str()), Some("Terminal"));
        l.key(Key::Backspace);
        l.key(Key::Backspace);
        l.key(Key::Backspace);
        assert_eq!(l.tiles.len(), 4);
        assert_eq!(l.selected, 0);
    }

    /// Twenty applications, the grid's eighteen tiles and two more.
    fn many() -> Launcher {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_apps((0..20).map(|i| app(&format!("App {i:02}"), None)).collect());
        l
    }

    #[test]
    fn opening_again_on_the_grid_keeps_its_tiles_and_draws_nothing() {
        let mut l = many();
        l.open();
        assert!(!l.draw(Duration::ZERO).is_empty());
        let tiles = l.tiles.clone();
        l.key(Key::Escape);
        l.open();
        assert_eq!(l.tiles, tiles, "the same tiles");
        assert_eq!(l.draw(Duration::ZERO), [], "nothing changed");
        // Scrolled a row, the tiles show the next applications; back at
        // the top, they show the first again.
        l.key(Key::End);
        assert_eq!(l.tile_apps.first(), Some(&6));
        assert_eq!(l.tiles.len(), 14);
        l.key(Key::Home);
        assert_eq!(l.tile_apps.first(), Some(&0));
        let dump = l.access().dump();
        assert!(dump.contains("App 00") && dump.contains("App 17"), "{dump}");
        assert!(!dump.contains("App 18"), "{dump}");
    }

    #[test]
    fn the_grid_is_made_again_in_a_new_theme() {
        let mut l = many();
        l.open();
        l.draw(Duration::ZERO);
        let tiles = l.tiles.clone();
        l.set_theme(Theme::default().with_text_size(120));
        assert_eq!(l.tiles.len(), 18, "shown at once");
        assert_ne!(l.tiles, tiles, "made again");
        assert!(l.access().dump().contains("App 00"));
    }

    #[test]
    fn prepared_while_closed_it_opens_drawing_nothing() {
        let mut l = many();
        l.open();
        l.text("app 19");
        l.draw(Duration::ZERO);
        assert!(l.tiles.is_empty(), "results in place of the grid");
        let dump = l.access().dump();
        assert!(
            !dump.contains("ALL APPLICATIONS"),
            "the grid is hidden: {dump}"
        );
        l.key(Key::Escape);
        l.prepare();
        assert!(!l.is_open(), "prepared, still closed");
        assert!(!l.draw(Duration::ZERO).is_empty(), "drawn as it opens");
        l.open();
        assert_eq!(l.draw(Duration::ZERO), []);
        assert_eq!(l.query(), "");
        assert_eq!(l.tiles.len(), 18);
        assert!(l.access().dump().contains("ALL APPLICATIONS"));
    }

    #[test]
    fn the_arrows_move_through_the_grid_and_it_scrolls_by_rows() {
        let mut l = Launcher::new(Theme::default(), Lang::En, 1.0);
        l.set_apps((0..26).map(|i| app(&format!("App {i:02}"), None)).collect());
        l.open();
        assert_eq!(l.tiles.len(), COLUMNS * GRID_ROWS);
        l.key(Key::Right);
        l.key(Key::Down);
        assert_eq!(l.selected, 1 + COLUMNS);
        l.key(Key::Left);
        l.key(Key::Left);
        assert_eq!(l.selected, COLUMNS - 1, "to the row above's last");
        l.key(Key::Up);
        assert_eq!(l.selected, COLUMNS - 1, "the first row stays");
        l.key(Key::Home);
        l.key(Key::Left);
        assert_eq!(l.selected, 0, "no further than the first");
        // Down past the rows shown: the grid follows, a row at a time.
        for _ in 0..3 {
            l.key(Key::Down);
        }
        assert_eq!(l.selected, 3 * COLUMNS);
        assert_eq!(l.grid_first, 1);
        assert_eq!(l.tile_apps.first(), Some(&COLUMNS));
        // From above the shorter last row: its last one.
        l.key(Key::Right);
        l.key(Key::Right);
        l.key(Key::Right);
        l.key(Key::Down);
        assert_eq!(l.selected, 25);
        assert_eq!(launched(l.key(Key::Enter)), Some("App 25".into()));
        // The wheel goes a row a notch; Page Up a page.
        l.open();
        l.draw(Duration::ZERO);
        assert!(l.wheel(Point::new(400.0, 300.0), 15.0));
        assert_eq!(l.selected, COLUMNS);
        assert!(l.page(true));
        assert_eq!(l.selected, COLUMNS + COLUMNS * GRID_ROWS);
        l.key(Key::Home);
        assert_eq!((l.selected, l.grid_first), (0, 0));
    }

    #[test]
    fn the_same_applications_read_again_change_nothing() {
        let mut l = launcher();
        l.key(Key::Right);
        l.key(Key::Right);
        let tiles = l.tiles.clone();
        l.set_apps(vec![
            app("Terminal", None),
            app("Fish", None),
            app("Firefox", Some("Web Browser")),
            app("Files", Some("File Manager")),
        ]);
        assert_eq!(l.selected, 2);
        assert_eq!(l.tiles, tiles, "not laid out again");
    }

    #[test]
    fn an_icon_read_shows_in_its_tile() {
        let mut l = launcher();
        l.apps[0].icon = Some("files".into());
        let red = [200u8, 0, 0, 255].repeat(48 * 48);
        let picture = Picture::new(48, 48, &red).expect("pixels");
        l.set_icons(vec![("files".into(), picture)]);
        let images = l
            .access()
            .walk()
            .into_iter()
            .filter(|n| n.role == hocus_ui::Role::Image)
            .count();
        assert_eq!(images, 1);
    }

    #[test]
    fn backspace_searches_again() {
        let mut l = launcher();
        l.text("fix");
        l.key(Key::Backspace);
        assert_eq!(l.query(), "fi");
    }
}
