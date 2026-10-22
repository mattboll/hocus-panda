//! A window's title bar, when title bars are on (decision 0006: off by
//! default): the title, and minimize, maximize and close on the side
//! chosen. The compositor draws it above the window and hands it the
//! pointer; a press outside the buttons is the compositor's (moving,
//! maximizing on a double click).

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, ButtonKind, Direction, Event, Icon, Input, Insets, Justify, Kind, Length,
    Look, NodeId, Paint, PixelRect, Point, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang, icons};

/// The bar's height, logical pixels.
pub const HEIGHT: f32 = 34.0;

/// What a press on the bar does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    Minimize,
    ToggleMaximized,
    Close,
}

/// Where a press landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// On a button: its action comes on release.
    Button,
    /// Anywhere else: the window is grabbed.
    Bar,
}

/// What the bar shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub title: String,
    pub focused: bool,
    pub maximized: bool,
}

pub struct TitleBar {
    tree: Tree,
    surface: Surface,
    card: NodeId,
    title: NodeId,
    minimize: NodeId,
    maximize: NodeId,
    close: NodeId,
    lang: Lang,
    radius: f32,
    shown: Shown,
}

impl std::fmt::Debug for TitleBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TitleBar")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str) -> Icon {
    Icon::stroked(path, 14.0, 2.1).unwrap_or_else(|| unreachable!("icon {path}"))
}

impl TitleBar {
    /// A bar `width` logical pixels wide. It is drawn taller than the bar
    /// by `radius`, its top corners rounded as the window's; the window
    /// hides the rest.
    pub fn new(
        theme: Theme,
        lang: Lang,
        width: f32,
        scale: f32,
        radius: f32,
        buttons_left: bool,
    ) -> Self {
        let size = Size::new(width.max(1.0), HEIGHT + radius);
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        let missing = || unreachable!("adding to a node of this tree");
        let card = tree
            .add(
                root,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    direction: Direction::Row,
                    align: Align::Start,
                    padding: Insets {
                        top: 3.0,
                        right: 4.0,
                        bottom: 0.0,
                        left: 4.0,
                    },
                    gap: 2.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let square = Style {
            width: Length::Px(28.0),
            height: Length::Px(28.0),
            ..Style::default()
        };
        let buttons = |tree: &mut Tree| {
            let mut b = |path: &str, name: &str| {
                tree.add(
                    card,
                    square,
                    Kind::icon_button(stroked(path), name, ButtonKind::Ghost, &theme),
                )
                .unwrap_or_else(missing)
            };
            [
                b(icons::MINIMIZE, lang.minimize()),
                b(icons::MAXIMIZE, lang.maximize()),
                b(icons::CLOSE, lang.close()),
            ]
        };
        let first = buttons_left.then(|| buttons(&mut tree));
        let middle = tree
            .add(
                card,
                Style {
                    width: Length::Fill,
                    height: Length::Px(28.0),
                    justify: Justify::Center,
                    align: Align::Center,
                    padding: Insets {
                        left: 8.0,
                        right: 8.0,
                        ..Insets::default()
                    },
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let title = tree
            .add(
                middle,
                Style::default(),
                Kind::text_line("", theme.text.label),
            )
            .unwrap_or_else(missing);
        let [minimize, maximize, close] = match first {
            Some(b) => b,
            None => buttons(&mut tree),
        };
        let mut bar = Self {
            surface: Surface::new(size, scale),
            tree,
            card,
            title,
            minimize,
            maximize,
            close,
            lang,
            radius,
            // Different from any shown, so that `show` paints it.
            shown: Shown {
                title: "\0".into(),
                ..Shown::default()
            },
        };
        bar.show(Shown::default());
        bar
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme);
        let shown = std::mem::take(&mut self.shown);
        self.shown.title = "\0".into();
        self.show(shown);
    }

    pub fn show(&mut self, shown: Shown) {
        if shown == self.shown {
            return;
        }
        let theme = self.tree.theme().clone();
        self.tree.set_text(self.title, &shown.title);
        let ink = if shown.focused {
            Paint::Text
        } else {
            Paint::Muted
        };
        self.tree
            .set_text_style(self.title, theme.text.label.with_color(ink));
        self.tree.set_look(
            self.card,
            Look {
                // A step lighter on the window with the keyboard.
                background: Some(if shown.focused {
                    Paint::SurfaceHover
                } else {
                    Paint::Surface
                }),
                radius: self.radius,
                ..Look::default()
            },
        );
        let (path, name) = if shown.maximized {
            (icons::RESTORE, self.lang.restore())
        } else {
            (icons::MAXIMIZE, self.lang.maximize())
        };
        self.tree.set_icon(self.maximize, stroked(path));
        self.tree.set_name(self.maximize, name);
        self.shown = shown;
    }

    pub fn theme(&self) -> &Theme {
        self.tree.theme()
    }

    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_leave(&mut self) {
        self.tree.input(Input::PointerLeave);
    }

    /// The primary button pressed at `at`, the pointer already there.
    pub fn press(&mut self, at: Point) -> Press {
        let on_button = self
            .tree
            .hit(at, true)
            .is_some_and(|n| [self.minimize, self.maximize, self.close].contains(&n));
        self.tree.input(Input::PointerButton { pressed: true });
        if on_button { Press::Button } else { Press::Bar }
    }

    /// The primary button released: a button's action when it was
    /// pressed and released on it.
    pub fn release(&mut self) -> Option<BarAction> {
        let events = self.tree.input(Input::PointerButton { pressed: false });
        events.into_iter().find_map(|e| match e {
            Event::Activated(n) if n == self.minimize => Some(BarAction::Minimize),
            Event::Activated(n) if n == self.maximize => Some(BarAction::ToggleMaximized),
            Event::Activated(n) if n == self.close => Some(BarAction::Close),
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

    fn bar(left: bool) -> TitleBar {
        let mut bar = TitleBar::new(Theme::default(), Lang::En, 400.0, 1.0, 12.0, left);
        bar.show(Shown {
            title: "notes.txt".into(),
            focused: true,
            maximized: false,
        });
        bar.draw(Duration::ZERO);
        bar
    }

    #[test]
    fn it_shows_the_title_and_the_buttons_on_the_side_chosen() {
        let right = bar(false).access().dump();
        assert!(right.contains("\"notes.txt\""), "{right}");
        let (title, close) = (right.find("notes.txt"), right.find("Close"));
        assert!(title < close, "{right}");
        let left = bar(true).access().dump();
        let (title, close) = (left.find("notes.txt"), left.find("Close"));
        assert!(close < title, "{left}");
    }

    #[test]
    fn a_button_acts_on_release_and_the_rest_grabs() {
        let mut bar = bar(false);
        let r = bar.tree.rect(bar.close).expect("laid out");
        let close = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
        bar.pointer_move(close);
        assert_eq!(bar.press(close), Press::Button);
        assert_eq!(bar.release(), Some(BarAction::Close));
        let middle = Point::new(150.0, 17.0);

        bar.pointer_move(middle);
        assert_eq!(bar.press(middle), Press::Bar);
        assert_eq!(bar.release(), None);
    }

    #[test]
    fn maximized_it_offers_to_restore() {
        let mut bar = bar(false);
        bar.show(Shown {
            title: "notes.txt".into(),
            focused: true,
            maximized: true,
        });
        bar.draw(Duration::ZERO);
        assert!(bar.access().dump().contains("Restore"));
    }
}
