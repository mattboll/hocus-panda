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

use std::time::Duration;

use hocus_core::{Action, shortcuts::Combo};
use hocus_ui::{
    AccessNode, Align, Direction, Event, Icon, Input, Insets, Key, Kind, Length, Look, NodeId,
    Paint, PixelRect, Point, Rect, Size, Style, Surface, TextField, Theme, Tree,
};

use crate::{
    column::View,
    i18n::Lang,
    icons,
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

/// The card's width, from the design.
pub const WIDTH: f32 = 860.0;
/// Room around the card for its shadow.
pub const ROOM: f32 = 40.0;
const FIELD: f32 = 76.0;
const ROW: f32 = 48.0;
const MAX_RESULTS: usize = 8;

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
        let list = tree
            .add(
                card,
                Style {
                    padding: Insets::symmetric(12.0, 12.0),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
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
        self.hits = self.hits(&query);
        // Only the palette scrolls: it lists everything.
        if !matches!(self.mode, Mode::Actions | Mode::Notes) {
            self.hits.truncate(MAX_RESULTS);
        }
        self.selected = 0;
        self.first = 0;
        self.render();
    }

    /// Shows `MAX_RESULTS` hits from `first`.
    fn render(&mut self) {
        for row in std::mem::take(&mut self.rows) {
            self.tree.remove(row);
        }
        for child in self.tree.children(self.list) {
            self.tree.remove(child);
        }
        let query = self.query().trim().to_owned();
        let hits: Vec<Hit> = self
            .hits
            .iter()
            .skip(self.first)
            .take(MAX_RESULTS)
            .cloned()
            .collect();
        let theme = self.tree.theme().clone();
        if hits.is_empty() && !query.is_empty() {
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
        for (i, hit) in hits.into_iter().enumerate() {
            let (name, kind) = (hit.title.clone(), hit.kind.clone());
            let color = hit.color.unwrap_or(theme.colors.accent);
            let Some(row) = self.tree.add(
                self.list,
                Style {
                    direction: Direction::Row,
                    height: Length::Px(ROW),
                    padding: Insets::symmetric(0.0, 16.0),
                    gap: 12.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: (i == 0).then_some(Paint::SurfaceHover),
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
        if self.selected < self.first {
            self.first = self.selected;
            return self.render();
        }
        if self.selected >= self.first + MAX_RESULTS {
            self.first = self.selected + 1 - MAX_RESULTS;
            return self.render();
        }
        let theme = self.tree.theme().clone();
        for (i, row) in self.rows.iter().enumerate() {
            let chosen = self.first + i == self.selected;
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
        Rect::new(0.0, 0.0, s.w, s.h).inset(Insets::all(ROOM))
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
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
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
    fn backspace_searches_again() {
        let mut l = launcher();
        l.text("fix");
        l.key(Key::Backspace);
        assert_eq!(l.query(), "fi");
    }
}
