//! The discreet dock (#38), as board Bureau draws it: a pill at the bottom
//! of the screen with an app per square, its glyph on its colour, a dot
//! under those open. The one under the pointer rises on a spring; one
//! launched bounces. Hidden, only a thin hint shows where it is.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Color, Direction, Icon, Insets, Justify, Kind, Length, Look, NodeId, Paint,
    PixelRect, Point, Property, Size, Style, Surface, TextStyle, Theme, Tree, Typeface,
    motion::{Curve, Spring},
};

use crate::column::View;

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// An app's square, logical.
const ICON: f32 = 68.0;
const GAP: f32 = 14.0;
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 12.0;
/// Room above for an icon that rises, and around for the shadow.
const ROOM: f32 = 24.0;
const RISE: f32 = 10.0;

/// An app in the dock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockApp {
    /// What names it: its desktop entry, or its app id.
    pub key: String,
    pub name: String,
    pub glyph: Glyph,
    pub color: Color,
    pub open: bool,
}

/// What an app's square shows, as board Bureau draws them: a few
/// characters, or a sign the typeface lacks, drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Glyph {
    Text(String),
    /// A path in a 24×24 box, stroked.
    Path(&'static str),
}

/// The files sign, ▤: a sheet with lines.
const FILES: &str = "M5 5h14v14H5zM5 9.5h14M5 14h14";
/// The notes sign, ✓.
const NOTES: &str = "M5 12.5l4.5 4.5L19 7";

/// How an app looks in the dock, from its desktop entry or app id and its
/// name: common apps as the board draws them, others by their initials on
/// the colour their notifications use.
pub fn look_of(id: &str, name: &str) -> (Glyph, Color) {
    let stem = id.trim_end_matches(".desktop").to_ascii_lowercase();
    // `org.gnome.Nautilus` is `nautilus`.
    let last = stem.rsplit('.').next().unwrap_or(&stem);
    let text = |s: &str| Glyph::Text(s.into());
    match last {
        "foot" | "footclient" | "kitty" | "alacritty" | "wezterm" | "ghostty" | "konsole"
        | "xterm" | "terminal" | "console" | "ptyxis" => (text(">_"), Color::rgb(0x2B, 0x2D, 0x33)),
        "firefox" | "firefox-esr" | "librewolf" => (text("Fx"), Color::rgb(0xE8, 0x64, 0x2C)),
        "slack" => (text("#"), Color::rgb(0x8E, 0x6C, 0xD8)),
        "nautilus" | "thunar" | "dolphin" | "nemo" | "pcmanfm" | "files" => {
            (Glyph::Path(FILES), Color::rgb(0x8F, 0x74, 0x20))
        }
        "notes" | "gnome-notes" | "xournalpp" => (Glyph::Path(NOTES), Color::rgb(0x2E, 0x7D, 0x4F)),
        "thunderbird" | "evolution" | "geary" => (text("@"), Color::rgb(0x2F, 0x6F, 0xD6)),
        _ => (Glyph::Text(glyph_of(name)), crate::toasts::app_color(&stem)),
    }
}

/// Two letters for an app without an icon: its words' initials.
pub fn glyph_of(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.as_slice() {
        [] => "?".into(),
        // One word: its first two letters, as a name is written.
        [one] => {
            let mut chars = one.chars();
            let first: String = chars
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            let second: String = chars
                .next()
                .map(|c| c.to_lowercase().collect())
                .unwrap_or_default();
            format!("{first}{second}")
        }
        // Several: their initials.
        many => many
            .iter()
            .filter_map(|w| w.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect(),
    }
}

/// The squares' characters: the board's 700 20 px monospace.
fn mono(theme: &Theme) -> TextStyle {
    TextStyle {
        typeface: Typeface::Mono,
        size: 20.0,
        weight: 700,
        ..theme.text.title
    }
}

pub struct Dock {
    tree: Tree,
    surface: Surface,
    icons: Vec<NodeId>,
    hovered: Option<usize>,
}

impl std::fmt::Debug for Dock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dock")
            .field("icons", &self.icons.len())
            .finish_non_exhaustive()
    }
}

impl Dock {
    /// Its size, shadow and rise included, for `count` apps.
    pub fn size(count: usize) -> Size {
        let n = count.max(1) as f32;
        Size::new(
            n * ICON + (n - 1.0) * GAP + 2.0 * PAD_X + 2.0 * ROOM,
            ICON + 2.0 * PAD_Y + 2.0 * ROOM,
        )
    }

    pub fn new(theme: Theme, scale: f32, apps: &[DockApp]) -> Self {
        let size = Self::size(apps.len());
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        tree.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                ..Style::default()
            },
        );
        let pill = tree
            .add(
                root,
                Style {
                    direction: Direction::Row,
                    gap: GAP,
                    padding: Insets::symmetric(PAD_Y, PAD_X),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    border: Some(Paint::Border),
                    radius: 26.0,
                    shadow: 18.0,
                }),
            )
            .unwrap_or_else(MISSING);
        let mut icons = Vec::new();
        for app in apps {
            let icon = tree
                .add(
                    pill,
                    Style {
                        width: Length::Px(ICON),
                        height: Length::Px(ICON),
                        align: Align::Center,
                        justify: Justify::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(app.color)),
                        radius: 18.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            tree.set_clickable(icon, &app.name);
            let white = Paint::Rgba(Color::rgb(0xFF, 0xFF, 0xFF));
            let glyph = match &app.glyph {
                Glyph::Text(t) => Kind::text_line(t.clone(), mono(&theme).with_color(white)),
                Glyph::Path(path) => Kind::Icon {
                    icon: Icon::stroked(path, 30.0, 2.4)
                        .unwrap_or_else(|| unreachable!("a constant icon")),
                    color: white,
                    label: None,
                },
            };
            tree.add(icon, Style::default(), glyph)
                .unwrap_or_else(MISSING);
            // Open: a dot at the square's foot.
            if app.open {
                tree.add(
                    icon,
                    Style {
                        at: Some(Point::new(ICON / 2.0 - 3.0, ICON - 9.0)),
                        width: Length::Px(6.0),
                        height: Length::Px(6.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(Color::rgb(0xFF, 0xFF, 0xFF))),
                        radius: 3.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            }
            icons.push(icon);
        }
        let mut surface = Surface::new(size, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self {
            tree,
            surface,
            icons,
            hovered: None,
        }
    }

    /// The app at `at`, from the corner.
    pub fn app_at(&self, at: Point) -> Option<usize> {
        self.icons
            .iter()
            .position(|i| self.tree.rect(*i).is_some_and(|r| r.contains(at)))
    }

    /// Whether `at`, from the corner, is on the pill.
    pub fn hit(&self, at: Point) -> bool {
        let s = self.tree.size();
        at.x >= ROOM && at.y >= ROOM && at.x <= s.w - ROOM && at.y <= s.h - ROOM
    }

    /// The app under the pointer rises; the others settle.
    pub fn hover(&mut self, app: Option<usize>) {
        if app == self.hovered {
            return;
        }
        let curve = Curve::Spring(Spring::SMOOTH);
        if let Some(old) = self.hovered.and_then(|i| self.icons.get(i)) {
            self.tree.animate(*old, Property::OffsetY, 0.0, curve);
        }
        if let Some(new) = app.and_then(|i| self.icons.get(i)) {
            self.tree.animate(*new, Property::OffsetY, -RISE, curve);
        }
        self.hovered = app;
    }

    /// An app launched: its square jumps and bounces back.
    pub fn bounce(&mut self, app: usize) {
        if let Some(icon) = self.icons.get(app) {
            self.tree.jump(*icon, Property::OffsetY, -2.0 * RISE);
            self.tree
                .animate(*icon, Property::OffsetY, 0.0, Curve::Spring(Spring::SETTLE));
        }
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
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

/// The hint where the dock hides: a short bar.
pub struct DockHint {
    surface: Surface,
}

impl std::fmt::Debug for DockHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockHint").finish_non_exhaustive()
    }
}

impl DockHint {
    pub const SIZE: Size = Size::new(140.0, 5.0);

    pub fn new(theme: Theme, scale: f32) -> Self {
        let mut tree = Tree::new(theme, Self::SIZE, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(Color::rgba(0xA6, 0xA9, 0xB1, 115))),
                radius: 2.5,
                ..Look::default()
            },
        );
        let mut surface = Surface::new(Self::SIZE, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self { surface }
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_initials() {
        assert_eq!(glyph_of("Firefox"), "Fi");
        assert_eq!(glyph_of("Visual Studio Code"), "VS");
        assert_eq!(glyph_of(""), "?");
    }

    #[test]
    fn common_apps_look_as_the_board_draws_them() {
        assert_eq!(
            look_of("org.codeberg.dnkl.foot.desktop", "Foot").0,
            Glyph::Text(">_".into())
        );
        assert_eq!(
            look_of("firefox-esr.desktop", "Firefox ESR").0,
            Glyph::Text("Fx".into())
        );
        assert_eq!(
            look_of("org.gnome.Nautilus.desktop", "Files").0,
            Glyph::Path(FILES)
        );
        assert_eq!(
            look_of("org.example.Viewer.desktop", "Image Viewer").0,
            Glyph::Text("IV".into())
        );
    }

    #[test]
    fn apps_are_found_under_the_pointer() {
        let apps: Vec<DockApp> = ["Files", "Mail"]
            .iter()
            .map(|n| DockApp {
                key: (*n).into(),
                name: (*n).into(),
                glyph: Glyph::Text(glyph_of(n)),
                color: Color::rgb(80, 80, 200),
                open: *n == "Mail",
            })
            .collect();
        let mut dock = Dock::new(Theme::default(), 1.0, &apps);
        let second = ROOM + PAD_X + ICON + GAP + ICON / 2.0;
        assert_eq!(
            dock.app_at(Point::new(second, ROOM + PAD_Y + 10.0)),
            Some(1)
        );
        dock.hover(Some(1));
        assert!(dock.is_animating(Duration::from_millis(10)));
    }
}
