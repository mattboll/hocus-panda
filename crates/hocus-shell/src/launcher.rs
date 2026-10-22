//! The launcher (Super+Space, or the column's command field): one field,
//! results from providers as it is typed. Version 0: the calculator, then
//! applications by fuzzy match. Up and Down choose, Enter goes, Escape
//! closes.
//!
//! The same card shows the clipboard history (Super+V: newest first, Tab
//! pins, Delete forgets) and the emoji picker (Super+Period: in English and
//! French, recent ones first, Tab changes the skin tone).

use std::time::Duration;

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
}

/// What the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Commands,
    Clipboard,
    Emoji,
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
}

pub struct Launcher {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    field: NodeId,
    list: NodeId,
    rows: Vec<(NodeId, Hit)>,
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
                    background: Some(Paint::Surface),
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
        tree.add(
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
            field,
            list,
            rows: Vec::new(),
            apps: Vec::new(),
            providers: providers(&Enabled::default()),
            selected: 0,
            open: false,
            mode: Mode::Commands,
            history: Vec::new(),
            recents: Vec::new(),
            tone: 0,
            hint,
            hint_box,
            now: Duration::ZERO,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
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

    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.apps = apps;
        if self.open {
            self.search();
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
            self.selected = selected.min(self.rows.len().saturating_sub(1));
            self.show_selection();
        }
    }

    pub fn set_recents(&mut self, recents: Vec<String>) {
        self.recents = recents;
    }

    fn show_hint(&mut self) {
        let hint = match self.mode {
            Mode::Commands => String::new(),
            Mode::Clipboard => self.lang.history_hint().to_owned(),
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
        for (row, _) in std::mem::take(&mut self.rows) {
            self.tree.remove(row);
        }
        for child in self.tree.children(self.list) {
            self.tree.remove(child);
        }
        let query = self.query().trim().to_owned();
        let mut hits = self.hits(&query);
        hits.truncate(MAX_RESULTS);
        self.selected = 0;
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
            self.tree.set_clickable(row, &name);
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
            self.rows.push((row, hit));
        }
        self.show_selection();
    }

    fn show_selection(&mut self) {
        let theme = self.tree.theme().clone();
        for (i, (row, _)) in self.rows.iter().enumerate() {
            self.tree.set_look(
                *row,
                Look {
                    background: (i == self.selected).then_some(Paint::SurfaceHover),
                    radius: theme.radii.medium,
                    ..Look::default()
                },
            );
            self.tree.set_selected_box(*row, i == self.selected);
        }
    }

    fn selected_paste(&self) -> Option<String> {
        match &self.rows.get(self.selected)?.1.choice {
            Choice::Paste(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn choose(&self, i: usize) -> Option<LauncherEvent> {
        let (_, hit) = self.rows.get(i)?;
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
                self.selected = selected.min(self.rows.len().saturating_sub(1));
                self.show_selection();
                Vec::new()
            }
            Key::Down | Key::Tab if !self.rows.is_empty() => {
                self.selected = (self.selected + 1) % self.rows.len();
                self.show_selection();
                Vec::new()
            }
            Key::Up | Key::BackTab if !self.rows.is_empty() => {
                self.selected = (self.selected + self.rows.len() - 1) % self.rows.len();
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
            if let Some(i) = self.rows.iter().position(|(r, _)| *r == node)
                && let Some(chosen) = self.choose(i)
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

    #[test]
    fn backspace_searches_again() {
        let mut l = launcher();
        l.text("fix");
        l.key(Key::Backspace);
        assert_eq!(l.query(), "fi");
    }
}
