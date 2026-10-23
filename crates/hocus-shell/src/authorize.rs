//! The system's password dialog (#83): polkit asks for a password to let an
//! application do something as an administrator. The compositor draws it
//! over every window, with the system's label, so that no client can draw
//! one like it or cover it.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Color, Direction, Event, Icon, Input, Insets, Justify,
    Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Size, Style, Surface, TextField,
    Theme, Tree,
};

use crate::{column::View, i18n::Lang, icons};

/// What polkit asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub message: String,
    /// The program run, for pkexec.
    pub command: Option<String>,
    /// Whose password.
    pub user: String,
}

/// Where the password stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Typing,
    Checking,
    /// Refused, with what the system said.
    Refused(String),
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthChoice {
    Submit(String),
    Cancel,
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

pub struct Authorize {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    check: Check,
    field: NodeId,
    note: NodeId,
    go: NodeId,
    cancel: NodeId,
}

impl std::fmt::Debug for Authorize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authorize")
            .field("check", &self.check)
            .finish_non_exhaustive()
    }
}

impl Authorize {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, prompt: &Prompt) -> Self {
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(Color::rgba(0, 0, 0, 150))),
                ..Look::default()
            },
        );
        tree.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                ..Style::default()
            },
        );
        let card = tree
            .add(
                root,
                Style {
                    width: Length::Px(480.0),
                    padding: Insets::all(28.0),
                    gap: 14.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 24.0,
                    border: Some(Paint::Accent),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(MISSING);
        let label = tree
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    gap: 8.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        if let Some(icon) = Icon::stroked(icons::SHIELD, 16.0, 2.1) {
            tree.add(
                label,
                Style::default(),
                Kind::Icon {
                    icon,
                    color: Paint::Accent,
                    label: None,
                },
            )
            .unwrap_or_else(MISSING);
        }
        tree.add(
            label,
            Style::default(),
            Kind::text_line(
                lang.system_asks(),
                theme
                    .text
                    .caption
                    .with_weight(700)
                    .with_color(Paint::Accent),
            ),
        )
        .unwrap_or_else(MISSING);
        tree.add(
            card,
            Style::default(),
            Kind::text(
                lang.authentication_required(),
                theme.text.headline.with_weight(700),
            ),
        )
        .unwrap_or_else(MISSING);
        tree.add(
            card,
            Style::default(),
            Kind::text(prompt.message.as_str(), theme.text.body),
        )
        .unwrap_or_else(MISSING);
        if let Some(command) = &prompt.command {
            tree.add(
                card,
                Style::default(),
                Kind::text(
                    command.as_str(),
                    hocus_ui::TextStyle {
                        typeface: hocus_ui::Typeface::Mono,
                        ..theme.text.label.with_color(Paint::Muted)
                    },
                ),
            )
            .unwrap_or_else(MISSING);
        }
        let field_box = tree
            .add(
                card,
                Style {
                    width: Length::Fill,
                    padding: Insets::symmetric(10.0, 14.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 12.0,
                    border: Some(Paint::Border),
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let placeholder = lang.password_for(&prompt.user);
        let field = tree
            .add(
                field_box,
                Style {
                    width: Length::Fill,
                    height: Length::Px(26.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(
                    TextField::new(
                        &placeholder,
                        &placeholder,
                        theme.text.title.with_weight(500),
                    )
                    .secret(),
                )),
            )
            .unwrap_or_else(MISSING);
        let note = tree
            .add(
                card,
                Style::default(),
                Kind::text("", theme.text.label.with_color(Paint::Error)),
            )
            .unwrap_or_else(MISSING);
        let row = tree
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    gap: 10.0,
                    justify: Justify::End,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let button = |tree: &mut Tree, label: &str, kind: ButtonKind| {
            tree.add(
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
        let cancel = button(&mut tree, lang.cancel(), ButtonKind::Plain);
        let go = button(&mut tree, lang.authenticate(), ButtonKind::Primary);
        tree.set_focus(Some(field));
        Self {
            tree,
            surface: Surface::new(size, scale),
            lang,
            check: Check::Typing,
            field,
            note,
            go,
            cancel,
        }
    }

    pub fn set_check(&mut self, check: Check) {
        let note = match &check {
            Check::Typing => String::new(),
            Check::Checking => self.lang.checking().to_owned(),
            Check::Refused(said) if said.is_empty() => self.lang.wrong_password().to_owned(),
            Check::Refused(said) => said.clone(),
        };
        self.tree.set_text(self.note, &note);
        self.check = check;
    }

    /// Typed text goes to the password field.
    pub fn text(&mut self, text: &str) {
        if self.check == Check::Checking {
            return;
        }
        self.tree.set_focus(Some(self.field));
        self.tree.text_input(text);
    }

    pub fn key(&mut self, key: Key) -> Option<AuthChoice> {
        match key {
            Key::Escape => Some(AuthChoice::Cancel),
            _ if self.check == Check::Checking => None,
            Key::Enter if self.tree.focused() == Some(self.cancel) => Some(AuthChoice::Cancel),
            Key::Enter => self.submit(),
            key => {
                let events = self.tree.input(Input::Key(key));
                self.handle(events)
            }
        }
    }

    fn submit(&mut self) -> Option<AuthChoice> {
        let password = self
            .tree
            .field_value(self.field)
            .unwrap_or_default()
            .to_owned();
        self.tree.set_field_value(self.field, "");
        if password.is_empty() {
            return None;
        }
        self.set_check(Check::Checking);
        Some(AuthChoice::Submit(password))
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Option<AuthChoice> {
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Option<AuthChoice> {
        for event in events {
            match event {
                Event::Activated(id) if id == self.cancel => return Some(AuthChoice::Cancel),
                Event::Activated(id)
                    if (id == self.go || id == self.field) && self.check != Check::Checking =>
                {
                    return self.submit();
                }
                _ => {}
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
    fn a_password_is_given_once_and_a_refusal_said() {
        let mut dialog = Authorize::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            &Prompt {
                message: "Authentication is needed to run `/usr/bin/true' as the super user".into(),
                command: Some("/usr/bin/true".into()),
                user: "alex".into(),
            },
        );
        dialog.draw(Duration::ZERO);
        let dump = dialog.access().dump();
        assert!(dump.contains("SYSTEM · HOCUSPANDA"), "{dump}");
        assert!(dump.contains("Password for alex"), "{dump}");
        dialog.text("panda");
        assert_eq!(
            dialog.key(Key::Enter),
            Some(AuthChoice::Submit("panda".into()))
        );
        // While it is checked, nothing more goes.
        dialog.text("x");
        assert_eq!(dialog.key(Key::Enter), None);
        dialog.set_check(Check::Refused("Authentication failure".into()));
        dialog.draw(Duration::ZERO);
        assert!(dialog.access().dump().contains("Authentication failure"));
        assert_eq!(dialog.key(Key::Escape), Some(AuthChoice::Cancel));
    }
}
