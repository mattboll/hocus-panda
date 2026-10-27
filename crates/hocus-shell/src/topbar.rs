//! The panel as a bar at the top (#263, decision 0011), as board D12
//! draws its form C: one line across the primary screen, with this
//! screen's desktops, the active window and its buttons, the time, the
//! date and the next meeting in the middle, then the active agents, the
//! tasks, the notifications and the battery. Its buttons act on the
//! active window; a click elsewhere on it asks for the whole column.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, ButtonKind, Direction, Event, Input, Insets, Justify, Kind, Length, Look,
    NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, TextStyle, Theme, Tree,
};

use crate::{
    agents::AgentState,
    column::{Action, ColumnState, Dots, View, stroked},
    i18n::Lang,
    icons,
    strip::{Glance, Others, agent_mark},
};

/// The bar's height, from board D12.
pub const HEIGHT: f32 = 52.0;
/// From the screen's top edge.
pub const TOP: f32 = 12.0;
/// A window's title is cut after so many characters.
const TITLE: usize = 48;

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// The bar.
pub struct TopBar {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    shown: Glance,
    /// Minimize, maximize and close, while there is a window.
    buttons: Option<[NodeId; 3]>,
    /// A press began on a button: its release is the button's.
    on_button: bool,
}

impl std::fmt::Debug for TopBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TopBar")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl TopBar {
    /// A bar `width` logical pixels wide, drawn at `scale`.
    pub fn new(theme: Theme, lang: Lang, width: f32, scale: f32) -> Self {
        let size = Size::new(width, HEIGHT);
        let mut bar = Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            shown: Glance::default(),
            buttons: None,
            on_button: false,
        };
        bar.build();
        bar
    }

    /// Shows what `state` shows at a glance; built again only when that
    /// changes.
    pub fn set_state(&mut self, state: &ColumnState) {
        if self.shown.matches(state, true) {
            return;
        }
        let glance = Glance::of(state);
        if glance != self.shown {
            self.shown = glance;
            self.build();
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if *self.tree.theme() != theme {
            self.tree.set_theme(theme);
            self.build();
        }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if lang != self.lang {
            self.lang = lang;
            self.build();
        }
    }

    pub fn resize(&mut self, width: f32, scale: f32) {
        let size = Size::new(width, HEIGHT);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.build();
        }
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    /// Whether `at`, from its corner, is on it.
    pub fn hit(&self, at: Point) -> bool {
        let s = self.tree.size();
        Rect::new(0.0, 0.0, s.w, s.h).contains(at)
    }

    /// The pointer left it.
    pub fn pointer_left(&mut self) {
        self.tree.input(Input::PointerLeave);
    }

    /// Pointer input at `at`, from its corner: its buttons act on the
    /// active window; a press elsewhere on it asks for the column.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<Action> {
        let mut actions = Vec::new();
        let forwarded = match input {
            Input::PointerMove(_) if !self.hit(at) => Input::PointerLeave,
            Input::PointerMove(_) => Input::PointerMove(at),
            other => other,
        };
        if let Input::PointerButton { pressed: true } = input {
            self.on_button = self.tree.hit(at, true).is_some();
            if !self.on_button {
                actions.push(Action::OpenPanel);
            }
        }
        let events = self.tree.input(forwarded);
        if let Some([minimize, maximize, close]) = self.buttons {
            for event in events {
                let Event::Activated(id) = event else {
                    continue;
                };
                if id == minimize {
                    actions.push(Action::Minimize);
                } else if id == maximize {
                    actions.push(Action::ToggleMaximize);
                } else if id == close {
                    actions.push(Action::Close);
                }
            }
        }
        actions
    }

    /// Draws what changed at `now`.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    /// Its see-through panel, for the compositor to blur what is behind.
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }

    fn build(&mut self) {
        let tree = &mut self.tree;
        let root = tree.root();
        for child in tree.children(root) {
            tree.remove(child);
        }
        let theme = tree.theme().clone();
        let theme = &theme
            .clone()
            .with_reduced_motion(theme.reduced_motion || self.shown.still);
        let lang = self.lang;
        let g = &self.shown;
        let add = |tree: &mut Tree, parent, style, kind| {
            tree.add(parent, style, kind).unwrap_or_else(MISSING)
        };
        let row = |gap: f32| Style {
            direction: Direction::Row,
            gap,
            align: Align::Center,
            ..Style::default()
        };
        let line = TextStyle {
            size: 15.0,
            weight: 500,
            ..theme.text.label
        };
        tree.set_look(root, Look::default());
        let card = add(
            tree,
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(0.0, 16.0),
                ..row(22.0)
            },
            Kind::Box(Look {
                background: Some(Paint::Glass),
                radius: 16.0,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        );

        // This screen's desktops, the active window and its buttons.
        let left = add(
            tree,
            card,
            Style {
                width: Length::Fill,
                ..row(22.0)
            },
            Kind::Box(Look::default()),
        );
        let (count, shown) = g.desktops;
        add(
            tree,
            left,
            Style::default(),
            Kind::Custom {
                draw: Box::new(Dots {
                    count,
                    shown,
                    badges: g.badges.clone(),
                }),
                label: Some(lang.desktop_of(shown, count)),
            },
        );
        self.buttons = None;
        match &g.active {
            Some(active) => {
                let title = if active.title.chars().count() > TITLE {
                    let cut: String = active.title.chars().take(TITLE - 1).collect();
                    format!("{}…", cut.trim_end())
                } else {
                    active.title.clone()
                };
                add(
                    tree,
                    left,
                    Style::default(),
                    Kind::text_line(title, line.with_weight(700)),
                );
                let buttons = add(tree, left, row(4.0), Kind::Box(Look::default()));
                let square = Style {
                    width: Length::Px(40.0),
                    height: Length::Px(40.0),
                    ..Style::default()
                };
                let (maximize_icon, maximize_name) = if active.maximized {
                    (icons::RESTORE, lang.restore())
                } else {
                    (icons::MAXIMIZE, lang.maximize())
                };
                let button = |tree: &mut Tree, path: &str, name: &str| {
                    add(
                        tree,
                        buttons,
                        square,
                        Kind::icon_button(stroked(path, 14.0), name, ButtonKind::Ghost, theme),
                    )
                };
                let minimize = button(tree, icons::MINIMIZE, lang.minimize());
                let maximize = button(tree, maximize_icon, maximize_name);
                let close = button(tree, icons::CLOSE, lang.close());
                self.buttons = Some([minimize, maximize, close]);
            }
            None => {
                add(
                    tree,
                    left,
                    Style::default(),
                    Kind::text_line(lang.no_window(), line.with_color(Paint::Muted)),
                );
            }
        }

        // The time, the date and the next meeting, in the middle.
        let middle = add(tree, card, row(12.0), Kind::Box(Look::default()));
        add(
            tree,
            middle,
            Style::default(),
            Kind::text_line(
                g.time.text(),
                TextStyle {
                    size: 22.0,
                    weight: 700,
                    line_height: 1.1,
                    ..theme.text.display
                },
            ),
        );
        add(
            tree,
            middle,
            Style::default(),
            Kind::text_line(
                lang.bar_day(&g.time, g.meeting.as_ref()),
                line.with_color(Paint::Muted),
            ),
        );

        // The active agents, the tasks, the notifications, the battery.
        let right = add(
            tree,
            card,
            Style {
                width: Length::Fill,
                justify: Justify::End,
                ..row(22.0)
            },
            Kind::Box(Look::default()),
        );
        if g.agents_total.is_some() {
            for agent in &g.agents {
                let waits = agent.state == AgentState::Wait;
                let chip = add(
                    tree,
                    right,
                    Style {
                        padding: Insets {
                            top: 4.0,
                            right: 12.0,
                            bottom: 4.0,
                            left: 4.0,
                        },
                        ..row(6.0)
                    },
                    Kind::Box(Look {
                        background: Some(if waits {
                            Paint::Rgba(theme.colors.waiting.with_alpha(31))
                        } else {
                            Paint::SurfaceHover
                        }),
                        radius: 999.0,
                        border: waits.then_some(Paint::Waiting),
                        shadow: 0.0,
                    }),
                );
                agent_mark(tree, chip, theme, lang, agent, g.pandas, 40.0);
                add(
                    tree,
                    chip,
                    Style::default(),
                    Kind::text_line(agent.name.clone(), line.with_weight(600)),
                );
            }
            if let Some(others) = g.others() {
                let text = match others {
                    Others::More(n) => format!("+{n}"),
                    Others::AllResting(n) => lang.bar_agents(n),
                };
                add(
                    tree,
                    right,
                    Style::default(),
                    Kind::text_line(text, line.with_color(Paint::Muted)),
                );
            }
        }
        add(
            tree,
            right,
            Style::default(),
            Kind::text_line(
                lang.bar_tasks(g.todos),
                line.with_weight(700).with_color(if g.late > 0 {
                    Paint::Error
                } else {
                    Paint::Text
                }),
            ),
        );
        add(
            tree,
            right,
            Style::default(),
            Kind::text_line(lang.bar_notifications(g.notifications), line),
        );
        if let Some(battery) = g.battery {
            add(
                tree,
                right,
                Style::default(),
                Kind::text_line(lang.battery_level(battery), line.with_color(Paint::Muted)),
            );
        }
        tree.mark(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::ActiveWindow;

    fn labels(node: &AccessNode, out: &mut Vec<String>) {
        out.extend(node.label.clone());
        for c in &node.children {
            labels(c, out);
        }
    }

    #[test]
    fn its_buttons_act_on_the_window_and_elsewhere_it_asks_for_the_column() {
        let mut bar = TopBar::new(Theme::default(), Lang::En, 1248.0, 1.0);
        bar.set_state(&ColumnState {
            active: Some(ActiveWindow {
                title: "Editor".into(),
                maximized: false,
            }),
            ..ColumnState::default()
        });
        bar.draw(Duration::ZERO);
        let mut said = Vec::new();
        labels(&bar.access(), &mut said);
        assert!(said.iter().any(|l| l == "Editor"), "{said:?}");
        assert!(said.iter().any(|l| l == "Tasks ✓"), "{said:?}");
        let close = bar
            .buttons
            .and_then(|[_, _, close]| bar.tree.rect(close))
            .expect("a close button");
        let at = Point::new(close.x + close.w / 2.0, close.y + close.h / 2.0);
        bar.input(at, Input::PointerMove(at));
        assert!(
            bar.input(at, Input::PointerButton { pressed: true })
                .is_empty()
        );
        assert_eq!(
            bar.input(at, Input::PointerButton { pressed: false }),
            vec![Action::Close]
        );
        let middle = Point::new(624.0, 26.0);
        bar.input(middle, Input::PointerMove(middle));
        assert_eq!(
            bar.input(middle, Input::PointerButton { pressed: true }),
            vec![Action::OpenPanel]
        );
    }
}
