//! The curtain before locking and the lock screen, after board D11. The
//! curtain dims the screen with a countdown; any input lifts it (so it has
//! no "lock now" button: Super+L locks at once). The lock
//! screen shows the clock, the date, who is logged in and the password
//! field, with a warning when Caps Lock is on.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Color, Input, Insets, Justify, Key, Kind, Length, Look, NodeId, Paint,
    PixelRect, Point, Size, Style, Surface, TextField, Theme, Tree,
};

use crate::{
    column::{View, WallTime},
    i18n::Lang,
    panda::{Character, Mood, Panda},
};

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

fn view(surface: &Surface) -> View<'_> {
    View {
        pixels: surface.pixels(),
        width: surface.width(),
        height: surface.height(),
        at: Point::default(),
    }
}

/// The curtain over a screen before it locks.
pub struct Curtain {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    countdown: NodeId,
    seconds: Option<u32>,
}

impl std::fmt::Debug for Curtain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Curtain")
            .field("seconds", &self.seconds)
            .finish_non_exhaustive()
    }
}

impl Curtain {
    /// `pandas`: since when the curtain falls, for the panda falling
    /// asleep over it; `None` without pandas.
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, pandas: Option<Duration>) -> Self {
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(Color::rgba(0, 0, 0, 158))),
                ..Look::default()
            },
        );
        tree.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                gap: 14.0,
                ..Style::default()
            },
        );
        if let Some(since) = pandas {
            let mood = Mood::Sleep;
            tree.add(
                root,
                Style {
                    width: Length::Px(140.0),
                    height: Length::Px(140.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(
                        Panda::themed(Character::Apprentice, mood, &theme).moving_from(since),
                    ),
                    label: Some(mood.label(lang).into()),
                },
            )
            .unwrap_or_else(MISSING);
        }
        let countdown = tree
            .add(
                root,
                Style::default(),
                Kind::text(
                    "",
                    theme
                        .text
                        .headline
                        .with_weight(700)
                        .with_color(Paint::Rgba(Color::WHITE)),
                ),
            )
            .unwrap_or_else(MISSING);
        tree.add(
            root,
            Style::default(),
            Kind::text(
                lang.stay_hint(),
                theme
                    .text
                    .title
                    .with_weight(500)
                    .with_color(Paint::Rgba(Color::rgb(0xc9, 0xcb, 0xd0))),
            ),
        )
        .unwrap_or_else(MISSING);
        // No "lock now" button: moving the pointer to it would lift the
        // curtain; Super+L locks at once.
        Self {
            surface: Surface::new(size, scale),
            tree,
            lang,
            countdown,
            seconds: None,
        }
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme);
    }

    /// The seconds left before locking.
    pub fn set_seconds(&mut self, seconds: u32) {
        if self.seconds != Some(seconds) {
            self.seconds = Some(seconds);
            self.tree
                .set_text(self.countdown, &self.lang.locking_in(seconds));
        }
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn view(&self) -> View<'_> {
        view(&self.surface)
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

/// Where the password is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Check {
    #[default]
    Typing,
    Checking,
    Failed,
}

/// The lock screen of one screen.
pub struct LockScreen {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    clock: NodeId,
    date: NodeId,
    field: NodeId,
    note: NodeId,
    time: Option<WallTime>,
    caps_lock: bool,
    check: Check,
    /// The two asleep in the corner, when there are pandas.
    pandas: Vec<NodeId>,
    top: f32,
}

impl std::fmt::Debug for LockScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockScreen")
            .field("check", &self.check)
            .finish_non_exhaustive()
    }
}

impl LockScreen {
    /// For `user`, whose initial shows; `pandas`, two asleep in a corner.
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, user: &str, pandas: bool) -> Self {
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Background),
                ..Look::default()
            },
        );
        tree.set_style(
            root,
            Style {
                align: Align::Center,
                padding: Insets {
                    top: (size.h * 0.12).round(),
                    ..Insets::default()
                },
                gap: 6.0,
                ..Style::default()
            },
        );
        let big = hocus_ui::TextStyle {
            size: 96.0,
            line_height: 1.0,
            ..theme.text.display.with_weight(600)
        };
        let clock = tree
            .add(root, Style::default(), Kind::text("", big))
            .unwrap_or_else(MISSING);
        let date = tree
            .add(
                root,
                Style::default(),
                Kind::text(
                    "",
                    theme.text.title.with_weight(500).with_color(Paint::Muted),
                ),
            )
            .unwrap_or_else(MISSING);
        tree.add(
            root,
            Style {
                height: Length::Px(54.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(MISSING);
        let avatar = tree
            .add(
                root,
                Style {
                    width: Length::Px(72.0),
                    height: Length::Px(72.0),
                    justify: Justify::Center,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Accent),
                    radius: 36.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let initial: String = user
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        tree.add(
            avatar,
            Style::default(),
            Kind::text(
                initial,
                theme
                    .text
                    .headline
                    .with_weight(700)
                    .with_color(Paint::OnAccent),
            ),
        )
        .unwrap_or_else(MISSING);
        tree.add(
            root,
            Style {
                height: Length::Px(12.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(MISSING);
        let field_box = tree
            .add(
                root,
                Style {
                    width: Length::Px(300.0),
                    height: Length::Px(52.0),
                    padding: Insets::symmetric(0.0, 16.0),
                    justify: Justify::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 14.0,
                    border: Some(Paint::Border),
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
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
                        lang.password(),
                        lang.password(),
                        theme.text.title.with_weight(500),
                    )
                    .secret(),
                )),
            )
            .unwrap_or_else(MISSING);
        let note = tree
            .add(
                root,
                Style::default(),
                Kind::text("", theme.text.label.with_color(Paint::Waiting)),
            )
            .unwrap_or_else(MISSING);
        tree.set_focus(Some(field));
        let mut sleepers = Vec::new();
        if pandas {
            for character in [Character::Apprentice, Character::Watcher] {
                let mood = Mood::Sleep;
                sleepers.push(
                    tree.add(
                        root,
                        Style::default(),
                        Kind::Custom {
                            draw: Box::new(Panda::themed(character, mood, &theme)),
                            label: Some(mood.label(lang).into()),
                        },
                    )
                    .unwrap_or_else(MISSING),
                );
            }
        }
        let mut screen = Self {
            surface: Surface::new(size, scale),
            tree,
            lang,
            clock,
            date,
            field,
            note,
            time: None,
            caps_lock: false,
            check: Check::Typing,
            pandas: sleepers,
            top: (size.h * 0.12).round(),
        };
        screen.place_pandas(size);
        screen
    }

    /// The pandas in the bottom right corner, the larger first.
    fn place_pandas(&mut self, size: Size) {
        let mut right = size.w - 18.0;
        for (&id, side) in self.pandas.clone().iter().zip([96.0_f32, 72.0]).rev() {
            right -= side;
            self.tree.set_style(
                id,
                Style {
                    at: Some(Point::new(right, size.h - 14.0 - side - self.top)),
                    width: Length::Px(side),
                    height: Length::Px(side),
                    ..Style::default()
                },
            );
        }
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.place_pandas(size);
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme);
    }

    pub fn set_time(&mut self, time: WallTime) {
        if self.time == Some(time) {
            return;
        }
        self.tree.set_text(self.clock, &time.text());
        self.tree.set_text(
            self.date,
            &self.lang.date(time.weekday, time.day, time.month),
        );
        self.time = Some(time);
    }

    pub fn set_caps_lock(&mut self, on: bool) {
        if self.caps_lock != on {
            self.caps_lock = on;
            self.show_note();
        }
    }

    pub fn set_check(&mut self, check: Check) {
        if self.check != check {
            self.check = check;
            self.show_note();
        }
    }

    pub fn check(&self) -> Check {
        self.check
    }

    fn show_note(&mut self) {
        let note = match self.check {
            Check::Checking => self.lang.checking(),
            Check::Failed => self.lang.wrong_password(),
            Check::Typing if self.caps_lock => self.lang.caps_lock_on(),
            Check::Typing => "",
        };
        self.tree.set_text(self.note, note);
    }

    /// Typed text goes to the password field.
    pub fn text(&mut self, text: &str) {
        if self.check == Check::Checking {
            return;
        }
        self.tree.set_focus(Some(self.field));
        self.tree.text_input(text);
        self.set_check(Check::Typing);
    }

    /// Enter gives the password to check, and empties the field.
    pub fn key(&mut self, key: Key) -> Option<String> {
        if self.check == Check::Checking {
            return None;
        }
        if key == Key::Enter {
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
            return Some(password);
        }
        if key == Key::Escape {
            self.tree.set_field_value(self.field, "");
            return None;
        }
        self.tree.set_focus(Some(self.field));
        self.tree.input(Input::Key(key));
        None
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn view(&self) -> View<'_> {
        view(&self.surface)
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> LockScreen {
        let mut s = LockScreen::new(
            Theme::default(),
            Lang::En,
            Size::new(800.0, 600.0),
            1.0,
            "alex",
            true,
        );
        s.set_time(WallTime {
            hour: 23,
            minute: 48,
            weekday: 2,
            day: 21,
            month: 10,
            ..WallTime::default()
        });
        s.draw(Duration::ZERO);
        s
    }

    #[test]
    fn the_password_is_given_on_enter_and_never_shown() {
        let mut s = screen();
        s.text("panda");
        s.draw(Duration::ZERO);
        let dump = s.access().dump();
        assert!(!dump.contains("panda"), "{dump}");
        assert_eq!(s.key(Key::Enter), Some("panda".into()));
        assert_eq!(s.check(), Check::Checking);
        // Nothing more is taken while it is checked.
        s.text("x");
        assert_eq!(s.key(Key::Enter), None);
        s.set_check(Check::Failed);
        s.draw(Duration::ZERO);
        assert!(s.access().dump().contains("Wrong password"));
    }

    #[test]
    fn caps_lock_is_said() {
        let mut s = screen();
        s.set_caps_lock(true);
        s.draw(Duration::ZERO);
        let dump = s.access().dump();
        assert!(dump.contains("Caps Lock is on"), "{dump}");
        assert!(dump.contains("\"23:48\""), "{dump}");
    }

    #[test]
    fn the_curtain_counts_down() {
        let mut c = Curtain::new(
            Theme::default(),
            Lang::En,
            Size::new(800.0, 600.0),
            1.0,
            None,
        );
        c.set_seconds(28);
        c.draw(Duration::ZERO);
        let dump = c.access().dump();
        assert!(dump.contains("Locking in 28"), "{dump}");
    }
}
