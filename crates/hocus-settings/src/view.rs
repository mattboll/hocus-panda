//! The settings window's content, after boards R1 to R4: a home with one
//! large search field and the pages as tiles in four families, and pages
//! with the families on the side. Everything is built again when what it
//! shows changes; a change of setting comes out as a [`Change`] for the
//! configuration file, and shows once the compositor has applied it.

use std::{collections::HashMap, time::Duration};

use hocus_core::Action;
use hocus_shell::{Lang, icons};
use hocus_ui::{
    Accent, AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Icon,
    Input, Insets, Justify, Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Property,
    Rect, Size, Style, Surface, TextField, Theme, ThemeName, Tree,
};

use crate::{
    catalogue::{self, Context, Family, Hit, Page, Setting},
    edit::Change,
    words::{self, ShortcutGroup},
};

/// What the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Page(Page),
}

/// What the user asked for.
#[derive(Debug, Clone)]
pub enum Outcome {
    Change(Box<Change>),
    Close,
}

/// What a node does when clicked.
#[derive(Debug, Clone)]
enum Target {
    Home,
    Page(Page),
    Hit(usize),
    Close,
    Set(Change),
    Group(Option<ShortcutGroup>),
}

/// A switch, drawn: a pill with its knob, the accent when on.
struct Switch(bool);

impl Draw for Switch {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let c = &theme.colors;
        let (w, h) = (40.0, 24.0);
        let pill = Rect::new(rect.x + rect.w - w, rect.y + (rect.h - h) / 2.0, w, h);
        let (ground, knob_x) = if self.0 {
            (c.accent, pill.x + w - h + 3.0)
        } else {
            (c.border.over(c.surface), pill.x + 3.0)
        };
        canvas.fill_rect(pill, h / 2.0, ground);
        let knob = if self.0 { c.on_accent } else { c.text };
        canvas.fill_rect(
            Rect::new(knob_x, pill.y + 3.0, h - 6.0, h - 6.0),
            (h - 6.0) / 2.0,
            knob,
        );
    }

    fn size(&self) -> Size {
        Size::new(40.0, 24.0)
    }
}

/// A theme's miniature: its background, a window, the column, the accent.
struct Miniature(Theme);

impl Draw for Miniature {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, _now: Duration) {
        let c = &self.0.colors;
        canvas.fill_rect(rect, 12.0, c.background);
        let window = Rect::new(rect.x + 14.0, rect.y + 16.0, rect.w * 0.55, rect.h * 0.56);
        canvas.shadow(window, 8.0, 12.0, c.shadow);
        canvas.fill_rect(window, 8.0, c.surface);
        canvas.fill_rect(
            Rect::new(window.x + 8.0, window.y + 10.0, window.w * 0.55, 6.0),
            3.0,
            c.accent,
        );
        canvas.fill_rect(
            Rect::new(rect.x + rect.w - 36.0, rect.y + 8.0, 28.0, rect.h - 16.0),
            8.0,
            c.surface,
        );
    }

    fn size(&self) -> Size {
        Size::new(0.0, 110.0)
    }
}

/// An accent's swatch, ringed when chosen.
struct Swatch {
    color: Color,
    chosen: bool,
}

impl Draw for Swatch {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let d = rect.w.min(rect.h);
        let r = Rect::new(
            rect.x + (rect.w - d) / 2.0,
            rect.y + (rect.h - d) / 2.0,
            d,
            d,
        );
        if self.chosen {
            canvas.fill_rect(r, d / 2.0, theme.colors.text);
        }
        let inner = Rect::new(r.x + 3.0, r.y + 3.0, d - 6.0, d - 6.0);
        canvas.fill_rect(inner, inner.w / 2.0, theme.colors.surface);
        let dot = Rect::new(r.x + 6.0, r.y + 6.0, d - 12.0, d - 12.0);
        canvas.fill_rect(dot, dot.w / 2.0, self.color);
    }

    fn size(&self) -> Size {
        Size::new(48.0, 48.0)
    }
}

pub struct SettingsView {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    ctx: Context,
    screen: Screen,
    query: String,
    hits: Vec<Hit>,
    /// The result Enter opens.
    chosen: usize,
    /// The setting a search led to, on its page.
    highlight: Option<Setting>,
    group: Option<ShortcutGroup>,
    targets: HashMap<NodeId, Target>,
    /// The search field, and the field that had the keyboard.
    field: Option<NodeId>,
    typing: bool,
    /// The scrolled box and its content, and how far down it is.
    scroller: Option<(NodeId, NodeId)>,
    scroll: f32,
    /// The text size slider, and the size it last asked for.
    text_size: Option<(NodeId, u32)>,
    pointer: Option<Point>,
    dirty: bool,
    now: Duration,
}

impl std::fmt::Debug for SettingsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsView")
            .field("screen", &self.screen)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    Icon::stroked(path, size, 2.1).unwrap_or_else(|| unreachable!("icon {path}"))
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

fn row(gap: f32) -> Style {
    Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    }
}

fn column(gap: f32) -> Style {
    Style {
        gap,
        ..Style::default()
    }
}

fn fill_width(style: Style) -> Style {
    Style {
        width: Length::Fill,
        ..style
    }
}

impl SettingsView {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, ctx: Context) -> Self {
        Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            ctx,
            screen: Screen::Home,
            query: String::new(),
            hits: Vec::new(),
            chosen: 0,
            highlight: None,
            group: None,
            targets: HashMap::new(),
            field: None,
            typing: true,
            scroller: None,
            scroll: 0.0,
            text_size: None,
            pointer: None,
            dirty: true,
            now: Duration::ZERO,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if *self.tree.theme() != theme {
            self.tree.set_theme(theme);
            self.dirty = true;
        }
    }

    /// What is in effect changed (the file, the screens).
    pub fn set_context(&mut self, ctx: Context) {
        self.ctx = ctx;
        if !self.query.is_empty() {
            self.hits = catalogue::search(&self.query, &self.ctx, self.lang);
        }
        self.dirty = true;
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.dirty = true;
        }
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    /// Shows a page, or the home; a deep link.
    pub fn open(&mut self, page: Option<Page>) {
        self.query.clear();
        self.hits.clear();
        self.highlight = None;
        self.go(page.map_or(Screen::Home, Screen::Page));
    }

    fn go(&mut self, screen: Screen) {
        if screen != self.screen {
            self.scroll = 0.0;
            self.group = None;
        }
        self.screen = screen;
        self.typing = screen == Screen::Home;
        self.dirty = true;
    }

    fn open_hit(&mut self, i: usize) {
        let Some(hit) = self.hits.get(i).cloned() else {
            return;
        };
        if !hit.page.available() {
            return;
        }
        self.query.clear();
        self.hits.clear();
        self.highlight = hit.setting;
        if let Some(Setting::Shortcut(_)) = &self.highlight {
            self.group = None;
        }
        self.go(Screen::Page(hit.page));
        self.typing = false;
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.pointer = Some(at);
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_leave(&mut self) {
        self.pointer = None;
        self.tree.input(Input::PointerLeave);
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Vec<Outcome> {
        let events = self.tree.input(Input::PointerButton { pressed });
        self.typing = self.tree.focused() == self.field && self.field.is_some();
        self.handle(events)
    }

    /// The wheel, logical pixels, down positive.
    pub fn scroll(&mut self, dy: f32) {
        let Some((viewport, content)) = self.scroller else {
            return;
        };
        let (Some(view), Some(inner)) = (self.tree.rect(viewport), self.tree.rect(content)) else {
            return;
        };
        let room = (inner.h - view.h).max(0.0);
        let scroll = (self.scroll + dy).clamp(0.0, room);
        if scroll != self.scroll {
            self.scroll = scroll;
            self.tree.jump(content, Property::OffsetY, -scroll);
        }
    }

    pub fn key(&mut self, key: Key) -> Vec<Outcome> {
        let results = self.screen == Screen::Home && !self.hits.is_empty();
        match key {
            Key::Escape if !self.query.is_empty() => {
                self.query.clear();
                self.hits.clear();
                self.dirty = true;
                Vec::new()
            }
            Key::Escape if self.screen != Screen::Home => {
                self.highlight = None;
                self.go(Screen::Home);
                Vec::new()
            }
            Key::Escape => vec![Outcome::Close],
            Key::Down if results => {
                self.chosen = (self.chosen + 1).min(self.hits.len() - 1);
                self.dirty = true;
                Vec::new()
            }
            Key::Up if results => {
                self.chosen = self.chosen.saturating_sub(1);
                self.dirty = true;
                Vec::new()
            }
            Key::Enter if results && self.tree.focused() == self.field => {
                self.open_hit(self.chosen);
                Vec::new()
            }
            key => {
                let events = self.tree.input(Input::Key(key));
                self.handle(events)
            }
        }
    }

    /// Typed text goes to the search field.
    pub fn text(&mut self, text: &str) -> Vec<Outcome> {
        if self.tree.focused().is_none_or(|f| Some(f) != self.field) {
            self.tree.set_focus(self.field);
        }
        let events = self.tree.text_input(text);
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Vec<Outcome> {
        let mut out = Vec::new();
        for event in events {
            match event {
                Event::Changed(id) if Some(id) == self.field => {
                    let text = self.tree.field_value(id).unwrap_or_default().to_owned();
                    if text != self.query {
                        self.query = text;
                        self.hits = catalogue::search(&self.query, &self.ctx, self.lang);
                        self.chosen = 0;
                        self.typing = true;
                        self.screen = Screen::Home;
                        self.dirty = true;
                    }
                }
                Event::Changed(id) if self.text_size.is_some_and(|(s, _)| s == id) => {
                    let value = self.tree.slider_value(id).unwrap_or(0.0);
                    let percent = (80.0 + value * 70.0) as u32 / 5 * 5;
                    if let Some((_, last)) = &mut self.text_size
                        && *last != percent
                    {
                        *last = percent;
                        out.push(Outcome::Change(Box::new(Change::new(
                            "appearance",
                            "text_size",
                            i64::from(percent),
                        ))));
                    }
                }
                Event::Activated(id) => match self.targets.get(&id).cloned() {
                    Some(Target::Home) => {
                        self.highlight = None;
                        self.go(Screen::Home);
                    }
                    Some(Target::Page(page)) => {
                        self.highlight = None;
                        self.go(Screen::Page(page));
                    }
                    Some(Target::Hit(i)) => self.open_hit(i),
                    Some(Target::Close) => out.push(Outcome::Close),
                    Some(Target::Set(change)) => out.push(Outcome::Change(Box::new(change))),
                    Some(Target::Group(group)) => {
                        self.group = group;
                        self.dirty = true;
                    }
                    None if Some(id) == self.field && !self.hits.is_empty() => {
                        self.open_hit(self.chosen);
                    }
                    None => {}
                },
                Event::Changed(_) => {}
            }
        }
        out
    }

    /// Samples animations, builds again what changed, and draws: the
    /// regions drawn, in physical pixels.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.now = now;
        if self.dirty {
            self.build();
            self.dirty = false;
        }
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn is_animating(&self) -> bool {
        self.dirty || self.tree.is_animating(self.now)
    }

    /// Premultiplied RGBA, row by row.
    pub fn pixels(&self) -> &[u8] {
        self.surface.pixels()
    }

    pub fn pixel_size(&self) -> (u32, u32) {
        (self.surface.width(), self.surface.height())
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }

    // Building.

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let root = self.tree.root();
        for child in self.tree.children(root) {
            self.tree.remove(child);
        }
        self.targets.clear();
        self.field = None;
        self.scroller = None;
        self.text_size = None;
        self.tree.set_look(
            root,
            Look {
                background: Some(Paint::Surface),
                ..Look::default()
            },
        );
        match self.screen {
            Screen::Home => {
                self.tree.set_style(
                    root,
                    Style {
                        padding: Insets::symmetric(30.0, 40.0),
                        gap: 26.0,
                        ..Style::default()
                    },
                );
                self.home(&theme, root);
            }
            Screen::Page(page) => {
                self.tree.set_style(
                    root,
                    Style {
                        direction: Direction::Row,
                        ..Style::default()
                    },
                );
                self.nav(&theme, root, page);
                self.page(&theme, root, page);
            }
        }
        // What had the keyboard keeps it; what the pointer was over is
        // hovered again.
        if self.typing {
            self.tree.set_focus(self.field);
        }
        if let Some(at) = self.pointer {
            self.tree.layout();
            self.tree.input(Input::PointerMove(at));
        }
    }

    fn add(&mut self, parent: NodeId, style: Style, kind: Kind) -> NodeId {
        self.tree.add(parent, style, kind).unwrap_or_else(MISSING)
    }

    fn line(&mut self, parent: NodeId, text: &str, style: hocus_ui::TextStyle) -> NodeId {
        self.add(parent, Style::default(), Kind::text_line(text, style))
    }

    fn close_button(&mut self, theme: &Theme, parent: NodeId) {
        let close = self.add(
            parent,
            Style {
                width: Length::Px(44.0),
                height: Length::Px(44.0),
                ..Style::default()
            },
            Kind::icon_button(
                stroked(icons::CLOSE, 18.0),
                words::CLOSE.get(self.lang),
                ButtonKind::Plain,
                theme,
            ),
        );
        self.targets.insert(close, Target::Close);
    }

    fn search_field(&mut self, theme: &Theme, parent: NodeId, large: bool) {
        let lang = self.lang;
        let height = if large { 52.0 } else { 44.0 };
        let border = if large { Some(Paint::Accent) } else { None };
        let box_ = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Px(height),
                padding: Insets::symmetric(0.0, 16.0),
                ..row(12.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                border,
                ..Look::default()
            }),
        );
        self.add(
            box_,
            Style::default(),
            Kind::Icon {
                icon: stroked(icons::SEARCH, if large { 20.0 } else { 17.0 }),
                color: Paint::Muted,
                label: None,
            },
        );
        let hint = if large {
            words::SEARCH_HINT
        } else {
            words::SEARCH
        };
        let style = if large {
            theme.text.title.with_weight(500)
        } else {
            theme.text.body
        };
        let field = self.add(
            box_,
            Style {
                width: Length::Fill,
                height: Length::Px(28.0),
                ..Style::default()
            },
            Kind::TextField(Box::new(TextField::new(
                words::SEARCH_NAME.get(lang),
                hint.get(lang),
                style,
            ))),
        );
        self.tree.set_field_value(field, &self.query);
        self.field = Some(field);
        if large {
            self.keycap(theme, box_, "Super+,");
        }
    }

    fn keycap(&mut self, theme: &Theme, parent: NodeId, label: &str) {
        let cap = self.add(
            parent,
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
        );
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.label.with_weight(500)
        };
        self.line(cap, label, mono);
    }

    fn caption(&mut self, theme: &Theme, parent: NodeId, words: words::Words) {
        let text = words.get(self.lang).to_uppercase();
        self.line(parent, &text, theme.text.caption);
    }

    fn home(&mut self, theme: &Theme, root: NodeId) {
        let lang = self.lang;
        let head = self.add(root, fill_width(row(20.0)), Kind::Box(Look::default()));
        self.line(
            head,
            words::SETTINGS.get(lang),
            theme.text.headline.with_weight(700),
        );
        self.search_field(theme, head, true);
        self.close_button(theme, head);
        let body = self.scrolled(root);
        if self.query.trim().is_empty() {
            self.families(theme, body);
        } else {
            self.results(theme, body);
        }
    }

    /// A box that scrolls with the wheel: what goes in it.
    fn scrolled(&mut self, parent: NodeId) -> NodeId {
        let viewport = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                clip: true,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        let content = self.add(
            viewport,
            fill_width(column(0.0)),
            Kind::Box(Look::default()),
        );
        self.scroller = Some((viewport, content));
        self.tree.jump(content, Property::OffsetY, -self.scroll);
        content
    }

    fn families(&mut self, theme: &Theme, body: NodeId) {
        self.tree.set_style(body, fill_width(column(28.0)));
        // Two families side by side when there is room.
        let per_line = if self.tree.size().w >= 1180.0 { 2 } else { 1 };
        let mut line = None;
        for (i, family) in Family::ALL.into_iter().enumerate() {
            if i % per_line == 0 {
                line = Some(self.add(
                    body,
                    Style {
                        align: Align::Start,
                        ..fill_width(row(40.0))
                    },
                    Kind::Box(Look::default()),
                ));
            }
            let Some(line) = line else { continue };
            let section = self.add(line, fill_width(column(12.0)), Kind::Box(Look::default()));
            self.caption(theme, section, family.label());
            let pages: Vec<Page> = family.pages().collect();
            for three in pages.chunks(3) {
                let tiles = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
                for page in three {
                    self.tile(theme, tiles, *page);
                }
            }
        }
    }

    fn tile(&mut self, theme: &Theme, parent: NodeId, page: Page) {
        let lang = self.lang;
        let available = page.available();
        let card = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Px(64.0),
                padding: Insets::symmetric(10.0, 14.0),
                ..row(14.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        if available {
            self.tree.set_clickable(card, page.label().get(lang));
            self.targets.insert(card, Target::Page(page));
        }
        let tint = theme.accents.get(page.family().tint());
        let badge = self.add(
            card,
            Style {
                width: Length::Px(40.0),
                height: Length::Px(40.0),
                justify: Justify::Center,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(tint.with_alpha(if available {
                    40
                } else {
                    18
                }))),
                radius: 12.0,
                ..Look::default()
            }),
        );
        let ink = if available {
            tint
        } else {
            tint.with_alpha(110)
        };
        self.add(
            badge,
            Style::default(),
            Kind::Icon {
                icon: stroked(page.icon(), 20.0),
                color: Paint::Rgba(ink),
                label: None,
            },
        );
        let words_box = self.add(card, fill_width(column(1.0)), Kind::Box(Look::default()));
        let name_ink = if available { Paint::Text } else { Paint::Muted };
        self.line(
            words_box,
            page.label().get(lang),
            theme.text.title.with_weight(700).with_color(name_ink),
        );
        let summary = page.summary(&self.ctx, lang);
        self.line(
            words_box,
            &summary,
            theme.text.caption.with_weight(500).with_color(Paint::Muted),
        );
    }

    fn results(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(6.0)));
        if self.hits.is_empty() {
            self.line(
                body,
                words::NO_RESULT.get(lang),
                theme.text.title.with_color(Paint::Muted),
            );
            return;
        }
        for (i, hit) in self.hits.clone().into_iter().enumerate() {
            let chosen = i == self.chosen;
            let line = self.add(
                body,
                Style {
                    height: Length::Px(56.0),
                    padding: Insets::symmetric(8.0, 16.0),
                    ..fill_width(row(14.0))
                },
                Kind::Box(Look {
                    background: Some(if chosen {
                        Paint::SurfacePressed
                    } else {
                        Paint::SurfaceHover
                    }),
                    radius: 14.0,
                    border: chosen.then_some(Paint::Accent),
                    ..Look::default()
                }),
            );
            let ink = if hit.page.available() {
                Paint::Text
            } else {
                Paint::Muted
            };
            if hit.page.available() {
                self.tree.set_clickable(line, &hit.label);
                self.targets.insert(line, Target::Hit(i));
            }
            self.add(
                line,
                Style::default(),
                Kind::Icon {
                    icon: stroked(hit.page.icon(), 20.0),
                    color: Paint::Rgba(theme.accents.get(hit.page.family().tint())),
                    label: None,
                },
            );
            let words_box = self.add(line, fill_width(column(1.0)), Kind::Box(Look::default()));
            self.line(
                words_box,
                &hit.label,
                theme.text.title.with_weight(700).with_color(ink),
            );
            self.line(
                words_box,
                &hit.detail,
                theme.text.caption.with_weight(500).with_color(Paint::Muted),
            );
        }
    }

    fn nav(&mut self, theme: &Theme, root: NodeId, current: Page) {
        let lang = self.lang;
        let nav = self.add(
            root,
            Style {
                width: Length::Px(280.0),
                height: Length::Fill,
                padding: Insets::symmetric(22.0, 14.0),
                ..column(4.0)
            },
            Kind::Box(Look {
                background: Some(Paint::Background),
                ..Look::default()
            }),
        );
        self.search_field(theme, nav, false);
        self.add(
            nav,
            Style {
                height: Length::Px(8.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        for family in Family::ALL {
            let head = self.add(
                nav,
                Style {
                    height: Length::Px(38.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    justify: Justify::SpaceBetween,
                    ..fill_width(row(8.0))
                },
                Kind::Box(Look {
                    radius: 10.0,
                    ..Look::default()
                }),
            );
            self.tree.set_clickable(head, family.label().get(lang));
            self.targets.insert(head, Target::Home);
            let label = family.label().get(lang).to_uppercase();
            self.line(head, &label, theme.text.caption);
            if family != current.family() {
                self.line(head, "6", theme.text.caption.with_weight(600));
                continue;
            }
            for page in family.pages() {
                let here = page == current;
                let item = self.add(
                    nav,
                    Style {
                        height: Length::Px(42.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        ..fill_width(row(12.0))
                    },
                    Kind::Box(Look {
                        background: here.then(|| Paint::Rgba(theme.colors.accent.with_alpha(36))),
                        radius: 12.0,
                        ..Look::default()
                    }),
                );
                let ink = match (here, page.available()) {
                    (true, _) => Paint::Accent,
                    (false, true) => Paint::Text,
                    (false, false) => Paint::Muted,
                };
                if page.available() {
                    self.tree.set_clickable(item, page.label().get(lang));
                    self.tree.set_selected_box(item, here);
                    self.targets.insert(item, Target::Page(page));
                }
                self.add(
                    item,
                    Style::default(),
                    Kind::Icon {
                        icon: stroked(page.icon(), 19.0),
                        color: ink,
                        label: None,
                    },
                );
                let weight = if here { 700 } else { 500 };
                self.line(
                    item,
                    page.label().get(lang),
                    theme.text.body.with_weight(weight).with_color(ink),
                );
            }
        }
    }

    fn page(&mut self, theme: &Theme, root: NodeId, page: Page) {
        let lang = self.lang;
        let main = self.add(
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(28.0, 40.0),
                ..column(24.0)
            },
            Kind::Box(Look::default()),
        );
        let head = self.add(
            main,
            Style {
                justify: Justify::SpaceBetween,
                align: Align::Start,
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        let titles = self.add(head, column(4.0), Kind::Box(Look::default()));
        self.line(
            titles,
            page.label().get(lang),
            theme.text.headline.with_weight(700),
        );
        let sub = match page {
            Page::Keyboard => words::SHORTCUTS_NOTE,
            Page::Screens => words::SCREENS_NOTE,
            _ => words::LIVE,
        };
        self.line(
            titles,
            sub.get(lang),
            theme.text.body.with_color(Paint::Muted),
        );
        self.close_button(theme, head);
        let body = self.scrolled(main);
        match page {
            Page::Appearance => self.appearance(theme, body),
            Page::Keyboard => self.keyboard(theme, body),
            Page::Screens => self.screens(theme, body),
            _ => {}
        }
    }

    fn section(&mut self, theme: &Theme, parent: NodeId, title: words::Words) -> NodeId {
        let section = self.add(parent, fill_width(column(12.0)), Kind::Box(Look::default()));
        self.caption(theme, section, title);
        section
    }

    /// A box ringed with the accent when a search led to `setting`.
    fn highlighted(&mut self, id: NodeId, setting: &Setting, look: Look) {
        if self.highlight.as_ref() == Some(setting) {
            self.tree.set_look(
                id,
                Look {
                    border: Some(Paint::Accent),
                    background: Some(Paint::Rgba(self.tree.theme().colors.accent.with_alpha(28))),
                    ..look
                },
            );
            self.tree.set_focus(Some(id));
        }
    }

    fn appearance(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let a = self.ctx.config.appearance.clone();

        // Theme.
        let section = self.section(theme, body, words::THEME);
        let grid = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        for (id, name, hint) in words::THEMES {
            let chosen = a.theme == id;
            let look = Look {
                background: Some(Paint::SurfaceHover),
                radius: 18.0,
                border: chosen.then_some(Paint::Accent),
                ..Look::default()
            };
            let card = self.add(
                grid,
                Style {
                    padding: Insets {
                        top: 8.0,
                        right: 8.0,
                        bottom: 12.0,
                        left: 8.0,
                    },
                    ..fill_width(column(10.0))
                },
                Kind::Box(look),
            );
            self.tree.set_clickable(card, name.get(lang));
            self.tree.set_selected_box(card, chosen);
            self.targets
                .insert(card, Target::Set(Change::new("appearance", "theme", id)));
            let mut preview = Theme::builtin(id.parse().unwrap_or(ThemeName::Dark));
            if let Ok(accent) = a.accent.parse::<Accent>() {
                preview = preview.with_accent(accent);
            }
            self.add(
                card,
                Style {
                    width: Length::Fill,
                    height: Length::Px(96.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Miniature(preview)),
                    label: None,
                },
            );
            let words_box = self.add(
                card,
                Style {
                    padding: Insets::symmetric(0.0, 4.0),
                    ..column(1.0)
                },
                Kind::Box(Look::default()),
            );
            self.line(words_box, name.get(lang), theme.text.title.with_weight(700));
            self.line(
                words_box,
                hint.get(lang),
                theme.text.caption.with_weight(500).with_color(Paint::Muted),
            );
            if chosen {
                self.highlighted(card, &Setting::Theme, look);
            }
        }

        // Accent and text, side by side when there is room.
        let wide = self.tree.size().w >= 1180.0;
        let pair = self.add(
            body,
            Style {
                align: Align::Start,
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                gap: if wide { 40.0 } else { 26.0 },
                ..fill_width(Style::default())
            },
            Kind::Box(Look::default()),
        );
        let accent = self.section(theme, pair, words::ACCENT);
        let swatches = self.add(accent, row(12.0), Kind::Box(Look::default()));
        // The theme's own accent shows as its colour.
        let own = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark))
            .accents
            .default;
        for (id, name) in words::ACCENTS {
            let chosen = a.accent == id || (a.accent == "theme" && own.name() == id);
            let color = id
                .parse::<Accent>()
                .map_or(theme.colors.accent, |acc| theme.accents.get(acc));
            let swatch = self.add(
                swatches,
                Style {
                    width: Length::Px(48.0),
                    height: Length::Px(48.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    radius: 24.0,
                    ..Look::default()
                }),
            );
            let label = format!("{} {}", words::ACCENT.get(lang), name.get(lang));
            self.tree.set_clickable(swatch, &label);
            self.tree.set_selected_box(swatch, chosen);
            self.targets
                .insert(swatch, Target::Set(Change::new("appearance", "accent", id)));
            self.add(
                swatch,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Swatch { color, chosen }),
                    label: None,
                },
            );
        }
        let mut from_wallpaper =
            Button::new(words::FROM_WALLPAPER.get(lang), ButtonKind::Plain, theme);
        from_wallpaper.selected = a.accent == "wallpaper";
        let b = self.add(
            swatches,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(from_wallpaper),
        );
        self.targets.insert(
            b,
            Target::Set(Change::new("appearance", "accent", "wallpaper")),
        );
        let note = self.add(
            accent,
            Style::default(),
            Kind::text(
                words::ACCENT_NOTE.get(lang),
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );
        let _ = note;
        let accent_look = Look::default();
        self.highlighted(accent, &Setting::Accent, accent_look);

        let text = self.section(theme, pair, words::TEXT);
        let fonts = self.add(text, fill_width(row(10.0)), Kind::Box(Look::default()));
        for w in [words::FONT_UI, words::FONT_MONO] {
            let mut b = Button::new(w.get(lang), ButtonKind::Plain, theme);
            // Choosing the faces comes with the font work of #103's sequel.
            b.enabled = false;
            self.add(
                fonts,
                Style {
                    width: Length::Fill,
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
        }
        let size_row = self.add(text, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.add(
            size_row,
            Style {
                width: Length::Px(70.0),
                ..Style::default()
            },
            Kind::text_line(words::SIZE.get(lang), theme.text.body),
        );
        let slider = self.add(
            size_row,
            Style {
                width: Length::Fill,
                height: Length::Px(32.0),
                ..Style::default()
            },
            Kind::Slider {
                value: (a.text_size.clamp(80, 150) - 80) as f32 / 70.0,
                step: 5.0 / 70.0,
                name: words::TEXT_SIZE.get(lang).to_owned(),
            },
        );
        self.text_size = Some((slider, a.text_size));
        self.add(
            size_row,
            Style {
                width: Length::Px(56.0),
                ..Style::default()
            },
            Kind::text_line(
                words::percent(a.text_size, lang),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        self.highlighted(text, &Setting::TextSize, Look::default());

        // Theme per desktop, with #49.
        let per_desktop = self.add(body, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.caption(theme, per_desktop, words::THEME_PER_DESKTOP);
        self.add(
            per_desktop,
            Style::default(),
            Kind::text(
                words::THEME_PER_DESKTOP_NOTE.get(lang),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        self.highlighted(per_desktop, &Setting::ThemePerDesktop, Look::default());

        // Switches, one under another when there is no room.
        let switches = self.add(
            body,
            Style {
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        for (setting, key, name, sub, on, works) in [
            (
                Setting::ReducedMotion,
                "reduced_motion",
                words::REDUCED_MOTION,
                words::REDUCED_MOTION_SUB,
                a.reduced_motion,
                true,
            ),
            (
                Setting::HighContrast,
                "high_contrast",
                words::HIGH_CONTRAST,
                words::HIGH_CONTRAST_SUB,
                a.high_contrast,
                true,
            ),
            (
                Setting::Transparency,
                "transparency",
                words::TRANSPARENCY,
                words::TRANSPARENCY_SUB,
                a.transparency,
                false,
            ),
        ] {
            let look = Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            };
            let card = self.add(
                switches,
                Style {
                    height: Length::Px(68.0),
                    padding: Insets::symmetric(10.0, 16.0),
                    ..fill_width(row(14.0))
                },
                Kind::Box(look),
            );
            let ink = if works { Paint::Text } else { Paint::Muted };
            if works {
                self.tree.set_clickable(card, name.get(lang));
                self.tree.set_selected_box(card, on);
                self.targets
                    .insert(card, Target::Set(Change::new("appearance", key, !on)));
            }
            let words_box = self.add(card, fill_width(column(1.0)), Kind::Box(Look::default()));
            self.line(
                words_box,
                name.get(lang),
                theme.text.title.with_weight(700).with_color(ink),
            );
            self.line(
                words_box,
                sub.get(lang),
                theme.text.caption.with_weight(500).with_color(Paint::Muted),
            );
            self.add(
                card,
                Style {
                    width: Length::Px(40.0),
                    height: Length::Px(24.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Switch(on)),
                    label: None,
                },
            );
            self.highlighted(card, &setting, look);
        }
    }

    fn keyboard(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(22.0)));
        let config = self.ctx.config.clone();
        let shortcuts = config.shortcuts();

        // Layouts.
        let layouts = self.add(body, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.caption(theme, layouts, words::LAYOUTS);
        let names: Vec<String> = if config.keyboard.layouts.is_empty() {
            vec!["us".into()]
        } else {
            config.keyboard.layouts.clone()
        };
        for (i, layout) in names.iter().enumerate() {
            let mut b = Button::new(layout.to_uppercase(), ButtonKind::Chip, theme);
            b.selected = i == 0;
            self.add(
                layouts,
                Style {
                    height: Length::Px(44.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
        }
        if let Some((combo, _)) = shortcuts
            .bindings
            .iter()
            .find(|(_, a)| **a == Action::SwitchLayout)
        {
            let spacer = self.add(
                layouts,
                fill_width(Style::default()),
                Kind::Box(Look::default()),
            );
            let _ = spacer;
            self.line(
                layouts,
                words::SWITCH_LAYOUT.get(lang),
                theme.text.body.with_color(Paint::Muted),
            );
            self.keycap(theme, layouts, &combo.to_string());
        }
        self.highlighted(layouts, &Setting::Layouts, Look::default());

        // Groups.
        let chips = self.add(body, fill_width(row(6.0)), Kind::Box(Look::default()));
        let groups = std::iter::once((None, words::ALL_GROUPS))
            .chain(ShortcutGroup::ALL.into_iter().map(|g| (Some(g), g.label())));
        for (group, label) in groups {
            let mut b = Button::new(label.get(lang), ButtonKind::Chip, theme);
            b.selected = self.group == group;
            let id = self.add(
                chips,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(id, Target::Group(group));
        }

        // The table.
        let table = self.add(
            body,
            Style {
                padding: Insets::symmetric(4.0, 0.0),
                ..fill_width(column(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        let header = self.add(
            table,
            Style {
                height: Length::Px(38.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..fill_width(row(16.0))
            },
            Kind::Box(Look::default()),
        );
        for (w, width) in [
            (words::ACTION, None),
            (words::ACTS_ON, Some(240.0)),
            (words::KEYS, Some(220.0)),
        ] {
            let cell = self.add(
                header,
                Style {
                    width: width.map_or(Length::Fill, Length::Px),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            );
            self.caption(theme, cell, w);
        }
        let mut rows: Vec<(String, String, &str, words::ShortcutGroup)> = shortcuts
            .bindings
            .iter()
            .map(|(combo, action)| {
                let (label, group, on) = words::action(action, lang);
                (combo.to_string(), label, on, group)
            })
            .filter(|r| self.group.is_none_or(|g| g == r.3))
            .collect();
        rows.sort_by(|a, b| (a.3 as u8, &a.1).cmp(&(b.3 as u8, &b.1)));
        for (combo, label, on, _) in rows {
            let look = Look::default();
            let line = self.add(
                table,
                Style {
                    height: Length::Px(50.0),
                    padding: Insets::symmetric(0.0, 18.0),
                    ..fill_width(row(16.0))
                },
                Kind::Box(look),
            );
            self.add(
                line,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::text_line(label, theme.text.body.with_weight(600)),
            );
            let scope = self.add(
                line,
                Style {
                    width: Length::Px(240.0),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            );
            self.line(
                scope,
                on,
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            );
            let keys = self.add(
                line,
                Style {
                    width: Length::Px(220.0),
                    ..row(6.0)
                },
                Kind::Box(Look::default()),
            );
            for part in combo.split('+') {
                self.keycap(theme, keys, part);
            }
            let setting = Setting::Shortcut(combo);
            if self.highlight.as_ref() == Some(&setting) {
                self.tree.set_clickable(line, &setting_name(&setting));
            }
            self.highlighted(
                line,
                &setting,
                Look {
                    radius: 12.0,
                    ..look
                },
            );
        }
    }

    fn screens(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(22.0)));
        let cards = self.add(body, fill_width(row(14.0)), Kind::Box(Look::default()));
        for (i, output) in self.ctx.outputs.clone().iter().enumerate() {
            let look = Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                border: Some(Paint::Border),
                ..Look::default()
            };
            let card = self.add(
                cards,
                Style {
                    padding: Insets::symmetric(14.0, 18.0),
                    ..column(4.0)
                },
                Kind::Box(look),
            );
            let name_row = self.add(card, row(10.0), Kind::Box(Look::default()));
            self.line(name_row, &output.name, theme.text.title.with_weight(700));
            if i == 0 {
                self.line(
                    name_row,
                    words::PRIMARY.get(lang),
                    theme.text.caption.with_color(Paint::Accent),
                );
            }
            let detail = format!(
                "{} × {} · {} {}",
                output.pixel_width,
                output.pixel_height,
                words::SCALE.get(lang).to_lowercase(),
                words::percent((output.scale * 100.0).round() as u32, lang)
            );
            self.line(
                card,
                &detail,
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            );
            self.highlighted(card, &Setting::Screen(output.name.clone()), look);
        }

        // Desktops on a new screen.
        let line = self.add(body, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.line(
            line,
            words::DESKTOPS_PER_SCREEN.get(lang),
            theme.text.body.with_weight(600),
        );
        let n = self.ctx.config.desktops.per_output;
        for (label, name, to) in [
            ("−", "−1", n.saturating_sub(1).max(1)),
            ("+", "+1", (n + 1).min(8)),
        ] {
            let b = Button::new(label, ButtonKind::Plain, theme).with_name(name);
            let id = self.add(
                line,
                Style {
                    width: Length::Px(44.0),
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(
                id,
                Target::Set(Change::new("desktops", "per_output", i64::from(to))),
            );
            if label == "−" {
                self.line(line, &n.to_string(), theme.text.title.with_weight(700));
            }
        }
    }
}

/// How a highlighted row is named for screen readers.
fn setting_name(setting: &Setting) -> String {
    match setting {
        Setting::Shortcut(combo) | Setting::Screen(combo) => combo.clone(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(lang: Lang) -> SettingsView {
        let mut v = SettingsView::new(
            Theme::default(),
            lang,
            Size::new(1200.0, 820.0),
            1.0,
            Context::default(),
        );
        v.draw(Duration::ZERO);
        v
    }

    fn find(node: &AccessNode, label: &str) -> Option<AccessNode> {
        if node.label.as_deref() == Some(label) {
            return Some(node.clone());
        }
        node.children.iter().find_map(|c| find(c, label))
    }

    fn click(v: &mut SettingsView, label: &str) -> Vec<Outcome> {
        let node = find(&v.access(), label)
            .unwrap_or_else(|| panic!("no {label}:\n{}", v.access().dump()));
        let at = Point::new(
            node.rect.x + node.rect.w / 2.0,
            node.rect.y + node.rect.h / 2.0,
        );
        v.pointer_move(at);
        let mut out = v.pointer_button(true);
        out.extend(v.pointer_button(false));
        v.draw(Duration::ZERO);
        out
    }

    #[test]
    fn contrast_typed_then_enter_opens_appearance_with_the_switch_focused() {
        let mut v = view(Lang::Fr);
        v.text("contraste");
        v.draw(Duration::ZERO);
        let dump = v.access().dump();
        let first = dump.lines().position(|l| l.contains("\"Contraste élevé\""));
        assert!(first.is_some(), "{dump}");
        v.key(Key::Enter);
        v.draw(Duration::ZERO);
        assert_eq!(v.screen(), Screen::Page(Page::Appearance));
        let switch = find(&v.access(), "Contraste élevé").expect("the switch");
        assert!(switch.focused, "{}", v.access().dump());
    }

    #[test]
    fn clicking_an_accent_asks_for_it_in_the_file() {
        let mut v = view(Lang::En);
        v.open(Some(Page::Appearance));
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Accent Amber");
        let [Outcome::Change(change)] = out.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!((change.section, change.key), ("appearance", "accent"));
        assert_eq!(change.value.as_str(), Some("amber"));
        let out = click(&mut v, "High contrast");
        assert!(
            matches!(out.as_slice(), [Outcome::Change(c)] if c.key == "high_contrast" && c.value.as_bool() == Some(true)),
            "{out:?}"
        );
    }

    #[test]
    fn a_value_changed_elsewhere_shows() {
        let mut v = view(Lang::En);
        v.open(Some(Page::Appearance));
        v.draw(Duration::ZERO);
        assert!(!find(&v.access(), "Light").expect("light").selected);
        let mut ctx = Context::default();
        ctx.config.appearance.theme = "light".into();
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Light").expect("light").selected);
    }

    #[test]
    fn the_home_tiles_lead_to_their_pages_and_escape_comes_back() {
        let mut v = view(Lang::En);
        click(&mut v, "Keyboard and shortcuts");
        assert_eq!(v.screen(), Screen::Page(Page::Keyboard));
        assert!(v.access().dump().contains("Quick settings"));
        v.key(Key::Escape);
        v.draw(Duration::ZERO);
        assert_eq!(v.screen(), Screen::Home);
        assert_eq!(v.key(Key::Escape).len(), 1, "Escape on the home closes");
    }
}
