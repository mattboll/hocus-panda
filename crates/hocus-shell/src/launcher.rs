//! The launcher (Super+Space, or the column's command field): one field,
//! results from providers as it is typed. Version 0: the calculator, then
//! applications by fuzzy match. Up and Down choose, Enter goes, Escape
//! closes.
//!
//! The same card shows the clipboard history (Super+V: newest first, Tab
//! pins, Delete forgets) and the emoji picker (Super+Period: in English and
//! French, recent ones first, Tab changes the skin tone).
//!
//! And notes (Super+N, #79): what is typed is added as a line, a to-do
//! when it starts `- [ ]`; Enter on an empty field ticks the to-do chosen,
//! Delete removes the line.
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

use std::time::Duration;

use hocus_core::{Action, shortcuts::Combo};
use hocus_ui::{
    AccessNode, Align, Direction, Event, Icon, Input, Insets, Key, Kind, Length, Look, NodeId,
    Paint, PixelRect, Point, Property, Rect, Size, Style, Surface, TextField, Theme, Tree,
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
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Launch(App),
    /// A result to put on the clipboard.
    Copy(String),
    /// An address to open in the browser.
    Open(String),
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
    /// The to-do at this place in the file: ticked, or not.
    ToggleNote(usize),
    /// The notes, opened from the command field.
    OpenNotes,
    /// A request understood offline, confirmed (#44).
    Intent(Intent),
    /// What is typed, asked to the AI.
    Ask(String),
    /// The answer shown: Enter acts on it.
    Answer,
}

/// A line of the notes, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteItem {
    /// Its place in the file.
    pub index: usize,
    pub text: String,
    /// A to-do, and whether it is done.
    pub todo: Option<bool>,
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
        fresh.providers = std::mem::take(&mut self.providers);
        fresh.history = std::mem::take(&mut self.history);
        fresh.recents = std::mem::take(&mut self.recents);
        fresh.tone = self.tone;
        fresh.shortcuts = std::mem::take(&mut self.shortcuts);
        fresh.action_recents = std::mem::take(&mut self.action_recents);
        fresh.notes = std::mem::take(&mut self.notes);
        fresh.ask = self.ask.take();
        fresh.asked = self.asked;
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
        self.tree.set_theme(theme);
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
    /// chosen there stays chosen.
    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.apps = apps;
        if self.open && self.mode == Mode::Commands {
            let selected = self.selected;
            self.search();
            self.selected = selected.min(self.hits.len().saturating_sub(1));
            self.show_selection();
        }
    }

    /// Which providers run, from the configuration.
    pub fn set_providers(&mut self, enabled: &Enabled) {
        self.providers = providers(enabled);
        self.ask = enabled.ask.clone();
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
        };
        self.tree.set_text(self.chip_text, keys);
        let placeholder = match mode {
            Mode::Actions => self.lang.palette_placeholder(),
            Mode::Notes => self.lang.notes_placeholder(),
            _ => self.lang.command_placeholder(),
        };
        self.tree.set_placeholder(self.field, placeholder);
        self.show_hint();
        self.search();
    }

    pub fn mode(&self) -> Mode {
        self.mode
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
                                    kind: self.lang.note_kind(n.todo.is_some()).into(),
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
                    choice: match n.todo {
                        Some(_) => Choice::ToggleNote(n.index),
                        None => Choice::Copy(n.text.clone()),
                    },
                })
                .collect(),
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
        self.hits = self.hits(&query);
        // Only the palette scrolls: it lists everything.
        if self.has_card() {
            self.hits.truncate(1 + MAX_ALSO);
        } else if !matches!(self.mode, Mode::Actions | Mode::Notes) {
            self.hits.truncate(MAX_RESULTS);
        }
        self.selected = 0;
        self.first = 0;
        self.render();
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
        self.answer_text = None;
        let card = if self.has_card() {
            self.hits.first().and_then(|h| self.card_for(h))
        } else {
            None
        };
        self.fit(card.is_some());
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
        for (index, hit) in hits {
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
            // A to-do's box, ticked when done.
            if let Choice::ToggleNote(index) = &hit.choice {
                let done = self
                    .notes
                    .iter()
                    .find(|n| n.index == *index)
                    .and_then(|n| n.todo)
                    .unwrap_or(false);
                let _ = self.tree.add(
                    row,
                    Style {
                        width: Length::Px(20.0),
                        height: Length::Px(20.0),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(crate::checkbox::Checkbox(done)),
                        label: None,
                    },
                );
            }
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
            // Notes: Enter adds what is typed; on an empty field, it ticks.
            Key::Enter if self.mode == Mode::Notes && !self.query().trim().is_empty() => {
                let typed = self.query().to_owned();
                self.tree.set_field_value(self.field, "");
                vec![LauncherEvent::Chosen(Choice::AddNote(typed))]
            }
            Key::Enter if self.mode == Mode::Notes => {
                self.choose(self.selected).into_iter().collect()
            }
            Key::Delete if self.mode == Mode::Notes && self.query().is_empty() => {
                match self.hits.get(self.selected).map(|h| &h.choice) {
                    Some(Choice::ToggleNote(i)) => vec![LauncherEvent::RemoveNote(*i)],
                    _ => self
                        .notes
                        .iter()
                        .rev()
                        .nth(self.selected)
                        .map(|n| LauncherEvent::RemoveNote(n.index))
                        .into_iter()
                        .collect(),
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
            if self.mode == Mode::Commands
                && let Some(i) = self.rows.iter().position(|r| *r == node)
                && let Some(index) = self.row_hits.get(i).copied()
            {
                events.extend(self.act(index));
                continue;
            }
            if let Some(i) = self.rows.iter().position(|r| *r == node)
                && let Some(chosen) = self.choose(self.first + i)
            {
                // Ticking a to-do keeps the notes open.
                if self.mode != Mode::Notes {
                    self.close();
                }
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
    /// answer scrolls.
    pub fn wheel(&mut self, at: Point, dy: f32) -> bool {
        self.hit(at) && self.scroll(dy)
    }

    /// Page Up or Page Down while the field has the keyboard.
    pub fn page(&mut self, down: bool) -> bool {
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

    pub fn access(&self) -> AccessNode {
        self.tree.access()
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

    #[test]
    fn backspace_searches_again() {
        let mut l = launcher();
        l.text("fix");
        l.key(Key::Backspace);
        assert_eq!(l.query(), "fi");
    }
}
