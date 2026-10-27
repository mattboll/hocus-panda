//! The card at start when a session was saved: put everything back,
//! choose which applications, or start fresh.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Color, Event, Input, Insets, Justify, Key, Kind, Length,
    Look, NodeId, Paint, PixelRect, Point, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeChoice {
    PutBack,
    /// Only these applications, by app id.
    Only(Vec<String>),
    StartFresh,
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

pub struct Welcome {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    /// Each application: its app id, its name, and whether it is chosen.
    apps: Vec<(String, String, bool)>,
    windows: usize,
    choosing: bool,
    put_back: Option<NodeId>,
    choose: Option<NodeId>,
    fresh: Option<NodeId>,
    toggles: Vec<NodeId>,
}

impl std::fmt::Debug for Welcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Welcome")
            .field("choosing", &self.choosing)
            .finish_non_exhaustive()
    }
}

impl Welcome {
    /// `apps` by app id and name; `windows` how many were open.
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        scale: f32,
        apps: Vec<(String, String)>,
        windows: usize,
    ) -> Self {
        let mut welcome = Self {
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            lang,
            apps: apps
                .into_iter()
                .map(|(id, name)| (id, name, true))
                .collect(),
            windows,
            choosing: false,
            put_back: None,
            choose: None,
            fresh: None,
            toggles: Vec::new(),
        };
        welcome.build();
        welcome
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        t.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(Color::rgba(0, 0, 0, 110))),
                ..Look::default()
            },
        );
        t.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                ..Style::default()
            },
        );
        let card = t
            .add(
                root,
                Style {
                    width: Length::Px(520.0),
                    padding: Insets::all(28.0),
                    gap: 18.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 24.0,
                    border: Some(Paint::Border),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(lang.welcome_back(), theme.text.headline.with_weight(700)),
        )
        .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(
                lang.windows_were_open(self.windows),
                theme.text.body.with_color(Paint::Muted),
            ),
        )
        .unwrap_or_else(MISSING);
        self.toggles.clear();
        if self.choosing {
            let list = t
                .add(
                    card,
                    Style {
                        direction: hocus_ui::Direction::Row,
                        gap: 8.0,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            for (_, name, chosen) in &self.apps {
                let mut b = Button::new(name.as_str(), ButtonKind::Chip, &theme);
                b.selected = *chosen;
                let id = t
                    .add(
                        list,
                        Style {
                            height: Length::Px(40.0),
                            padding: Insets::symmetric(0.0, 14.0),
                            ..Style::default()
                        },
                        Kind::Button(b),
                    )
                    .unwrap_or_else(MISSING);
                self.toggles.push(id);
            }
        }
        let row = t
            .add(
                card,
                Style {
                    direction: hocus_ui::Direction::Row,
                    gap: 10.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let button = |t: &mut Tree, label: &str, kind: ButtonKind| {
            t.add(
                row,
                Style {
                    height: Length::Px(44.0),
                    padding: Insets::symmetric(0.0, 18.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(label, kind, &theme)),
            )
            .unwrap_or_else(MISSING)
        };
        let first = if self.choosing {
            lang.put_these_back()
        } else {
            lang.put_everything_back()
        };
        self.put_back = Some(button(t, first, ButtonKind::Primary));
        self.choose = (!self.choosing).then(|| button(t, lang.choose(), ButtonKind::Plain));
        self.fresh = Some(button(t, lang.start_fresh(), ButtonKind::Ghost));
        t.set_focus(self.put_back);
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Option<WelcomeChoice> {
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    pub fn key(&mut self, key: Key) -> Option<WelcomeChoice> {
        if key == Key::Escape {
            return Some(WelcomeChoice::StartFresh);
        }
        let events = self.tree.input(Input::Key(key));
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Option<WelcomeChoice> {
        for event in events {
            let Event::Activated(id) = event else {
                continue;
            };
            if Some(id) == self.put_back {
                return Some(if self.choosing {
                    WelcomeChoice::Only(
                        self.apps
                            .iter()
                            .filter(|(_, _, c)| *c)
                            .map(|(a, _, _)| a.clone())
                            .collect(),
                    )
                } else {
                    WelcomeChoice::PutBack
                });
            }
            if Some(id) == self.fresh {
                return Some(WelcomeChoice::StartFresh);
            }
            if Some(id) == self.choose {
                self.choosing = true;
                self.build();
                return None;
            }
            if let Some(i) = self.toggles.iter().position(|t| *t == id) {
                let chosen = &mut self.apps[i].2;
                *chosen = !*chosen;
                let on = *chosen;
                self.tree.set_selected(id, on);
            }
        }
        None
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37, #162).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
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

    fn press(w: &mut Welcome, label: &str) -> Option<WelcomeChoice> {
        let dump = w.access().dump();
        let line = dump
            .lines()
            .find(|l| l.contains(&format!("\"{label}\"")))
            .unwrap_or_else(|| panic!("{label}:\n{dump}"));
        let xy: Vec<f32> = line
            .split_whitespace()
            .find(|w| w.contains(',') && w.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .map(|w| w.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default();
        w.pointer_move(Point::new(xy[0] + 10.0, xy[1] + 10.0));
        w.pointer_button(true);
        let out = w.pointer_button(false);
        w.draw(Duration::ZERO);
        out
    }

    #[test]
    fn choosing_puts_back_only_what_stays_chosen() {
        let mut w = Welcome::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            vec![
                ("foot".into(), "Foot".into()),
                ("files".into(), "Files".into()),
            ],
            3,
        );
        w.draw(Duration::ZERO);
        assert!(w.access().dump().contains("3 windows were open."));
        assert_eq!(press(&mut w, "Choose"), None);
        assert_eq!(press(&mut w, "Files"), None);
        assert_eq!(
            press(&mut w, "Put these back"),
            Some(WelcomeChoice::Only(vec!["foot".into()]))
        );
        assert_eq!(w.key(Key::Escape), Some(WelcomeChoice::StartFresh));
    }
}
