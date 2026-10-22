//! Leaving the session (#72): a card that counts down before logging out,
//! restarting or shutting down; when applications hold it back, it says
//! who and why and waits for a decision.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Color, Event, Input, Insets, Justify, Key, Kind, Length,
    Look, NodeId, Paint, PixelRect, Point, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

/// What is being left for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    LogOut,
    Restart,
    ShutDown,
}

/// Where leaving stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Seconds before it goes on.
    CountingDown(u32),
    /// Applications ask to wait: who and why.
    HeldBack(Vec<(String, String)>),
    /// Windows were asked to close; these are still open.
    Closing(Vec<String>),
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveChoice {
    /// Now, or anyway.
    Go,
    Cancel,
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

pub struct Leave {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    what: Leaving,
    stage: Stage,
    go: Option<NodeId>,
    cancel: Option<NodeId>,
}

impl std::fmt::Debug for Leave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Leave")
            .field("what", &self.what)
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

impl Leave {
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        scale: f32,
        what: Leaving,
        stage: Stage,
    ) -> Self {
        let mut leave = Self {
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            lang,
            what,
            stage,
            go: None,
            cancel: None,
        };
        leave.build();
        leave
    }

    pub fn what(&self) -> Leaving {
        self.what
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// Shows where leaving stands; builds again only when it changed.
    pub fn set_stage(&mut self, stage: Stage) {
        if stage != self.stage {
            self.stage = stage;
            self.build();
        }
    }

    fn title(&self) -> &'static str {
        match self.what {
            Leaving::LogOut => self.lang.log_out(),
            Leaving::Restart => self.lang.restart(),
            Leaving::ShutDown => self.lang.shut_down(),
        }
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let title = self.title();
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
                    width: Length::Px(480.0),
                    padding: Insets::all(28.0),
                    gap: 14.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 24.0,
                    border: Some(Paint::Border),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(title, theme.text.headline.with_weight(700)),
        )
        .unwrap_or_else(MISSING);
        let muted = theme.text.body.with_color(Paint::Muted);
        let lines: Vec<(String, hocus_ui::TextStyle)> =
            match &self.stage {
                Stage::CountingDown(s) => {
                    vec![(lang.leaving_in(lang.leaving(self.what as usize), *s), muted)]
                }
                Stage::HeldBack(held) => std::iter::once((lang.held_back().to_owned(), muted))
                    .chain(held.iter().map(|(who, why)| {
                        (format!("{who} — {why}"), theme.text.body.with_weight(600))
                    }))
                    .collect(),
                Stage::Closing(open) if open.is_empty() => {
                    vec![(lang.closing_apps().to_owned(), muted)]
                }
                Stage::Closing(open) => std::iter::once((lang.still_open().to_owned(), muted))
                    .chain(
                        open.iter()
                            .map(|name| (name.clone(), theme.text.body.with_weight(600))),
                    )
                    .collect(),
            };
        for (text, style) in lines {
            t.add(card, Style::default(), Kind::text(text, style))
                .unwrap_or_else(MISSING);
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
        let go = match self.stage {
            Stage::CountingDown(_) => lang.now(title),
            _ => lang.anyway(title),
        };
        self.cancel = Some(button(t, lang.cancel(), ButtonKind::Plain));
        self.go = Some(button(t, &go, ButtonKind::Primary));
        // Enter goes on only while counting down; held back, a decision
        // is a click away, never a stray key.
        t.set_focus(if matches!(self.stage, Stage::CountingDown(_)) {
            self.go
        } else {
            self.cancel
        });
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

    pub fn pointer_button(&mut self, pressed: bool) -> Option<LeaveChoice> {
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    pub fn key(&mut self, key: Key) -> Option<LeaveChoice> {
        if key == Key::Escape {
            return Some(LeaveChoice::Cancel);
        }
        let events = self.tree.input(Input::Key(key));
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Option<LeaveChoice> {
        events.into_iter().find_map(|event| match event {
            Event::Activated(id) if Some(id) == self.go => Some(LeaveChoice::Go),
            Event::Activated(id) if Some(id) == self.cancel => Some(LeaveChoice::Cancel),
            _ => None,
        })
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
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

    #[test]
    fn it_counts_down_then_says_who_holds_it_back() {
        let mut leave = Leave::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            Leaving::Restart,
            Stage::CountingDown(10),
        );
        leave.draw(Duration::ZERO);
        let dump = leave.access().dump();
        assert!(dump.contains("Restarting in 10 s."), "{dump}");
        assert!(dump.contains("Restart now"), "{dump}");
        leave.set_stage(Stage::HeldBack(vec![(
            "Editor".into(),
            "Unsaved document in notes.txt".into(),
        )]));
        leave.draw(Duration::ZERO);
        let dump = leave.access().dump();
        assert!(
            dump.contains("Editor — Unsaved document in notes.txt"),
            "{dump}"
        );
        assert!(dump.contains("Restart anyway"), "{dump}");
        // Enter does not go on when held back: the focus is on Cancel.
        assert_eq!(leave.key(Key::Enter), Some(LeaveChoice::Cancel));
        assert_eq!(leave.key(Key::Escape), Some(LeaveChoice::Cancel));
    }
}
