//! The launcher (Super+Space, or the column's command field): one field,
//! results from providers as it is typed. Version 0: the calculator, then
//! applications by fuzzy match. Up and Down choose, Enter goes, Escape
//! closes.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Direction, Event, Icon, Input, Insets, Key, Kind, Length, Look, NodeId,
    Paint, PixelRect, Point, Rect, Size, Style, Surface, TextField, Theme, Tree,
};

use crate::{calc, column::View, fuzzy, i18n::Lang, icons, toasts::app_color};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherEvent {
    Chosen(Choice),
    Closed,
}

#[derive(Debug, Clone)]
enum Hit {
    App(usize),
    Calc(String),
}

pub struct Launcher {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    field: NodeId,
    list: NodeId,
    rows: Vec<(NodeId, Hit)>,
    apps: Vec<App>,
    selected: usize,
    open: bool,
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
        Self {
            lang,
            tree,
            surface: Surface::new(size, scale),
            field,
            list,
            rows: Vec::new(),
            apps: Vec::new(),
            selected: 0,
            open: false,
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
    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.apps = apps;
        if self.open {
            self.search();
        }
    }

    /// Opens on an empty field.
    pub fn open(&mut self) {
        self.open = true;
        self.tree.set_field_value(self.field, "");
        self.tree.set_focus(Some(self.field));
        self.search();
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
        let mut hits = Vec::new();
        if let Some(value) = calc::evaluate(&query) {
            hits.push(Hit::Calc(value));
        }
        if !query.is_empty() {
            let mut scored: Vec<(i32, usize)> = self
                .apps
                .iter()
                .enumerate()
                .filter_map(|(i, app)| {
                    let by_name = fuzzy::score(&query, &app.name);
                    let by_generic = app
                        .generic
                        .as_deref()
                        .and_then(|g| fuzzy::score(&query, g))
                        .map(|s| s - 500);
                    let by_keyword = app
                        .keywords
                        .iter()
                        .filter_map(|k| fuzzy::score(&query, k))
                        .max()
                        .map(|s| s - 800);
                    [by_name, by_generic, by_keyword]
                        .into_iter()
                        .flatten()
                        .max()
                        .map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|(a, i), (b, j)| {
                b.cmp(a)
                    .then_with(|| self.apps[*i].name.cmp(&self.apps[*j].name))
            });
            hits.extend(scored.into_iter().map(|(_, i)| Hit::App(i)));
        }
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
            let (name, kind, color) = match &hit {
                Hit::Calc(v) => (
                    format!("= {v}"),
                    self.lang.calculator(),
                    theme.colors.accent,
                ),
                Hit::App(a) => {
                    let app = &self.apps[*a];
                    (
                        app.name.clone(),
                        self.lang.application(),
                        app_color(&app.name),
                    )
                }
            };
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

    fn choose(&self, i: usize) -> Option<LauncherEvent> {
        let (_, hit) = self.rows.get(i)?;
        Some(LauncherEvent::Chosen(match hit {
            Hit::App(a) => Choice::Launch(self.apps.get(*a)?.clone()),
            Hit::Calc(v) => Choice::Copy(v.clone()),
        }))
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
