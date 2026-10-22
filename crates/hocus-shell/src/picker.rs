//! Choosing what a screenshot takes (#69): a region drawn with the pointer,
//! its size in pixels beside it, or the window under the pointer. The rest
//! of the screen dims. The view only draws; the compositor follows the
//! pointer and the keys.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Color, Direction, Icon, Insets, Justify, Kind, Length,
    Look, NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang, icons};

/// What is being chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    Region,
    Window,
    /// What to record: a region drawn, a window clicked, or the screen.
    Record,
}

/// What a recording takes besides the picture (#131), offered while
/// picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordOptions {
    pub microphone: bool,
    pub system_sound: bool,
    /// `None`: no camera here.
    pub camera: Option<bool>,
    pub show_keys: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOption {
    Microphone,
    SystemSound,
    Camera,
    ShowKeys,
}

impl RecordOptions {
    pub fn toggle(&mut self, option: RecordOption) {
        match option {
            RecordOption::Microphone => self.microphone = !self.microphone,
            RecordOption::SystemSound => self.system_sound = !self.system_sound,
            RecordOption::Camera => self.camera = self.camera.map(|on| !on),
            RecordOption::ShowKeys => self.show_keys = !self.show_keys,
        }
    }
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");
const DIM: Color = Color::rgba(0, 0, 0, 120);

pub struct Picker {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    mode: PickMode,
    /// What is chosen on this screen, in its logical pixels.
    chosen: Option<Rect>,
    options: RecordOptions,
    /// The options' buttons, while they show.
    option_nodes: Vec<(NodeId, RecordOption)>,
}

impl std::fmt::Debug for Picker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picker")
            .field("mode", &self.mode)
            .field("chosen", &self.chosen)
            .finish_non_exhaustive()
    }
}

impl Picker {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, mode: PickMode) -> Self {
        let mut picker = Self {
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            lang,
            mode,
            chosen: None,
            options: RecordOptions::default(),
            option_nodes: Vec::new(),
        };
        picker.build();
        picker
    }

    /// The recording's options, shown under the hint.
    pub fn set_options(&mut self, options: RecordOptions) {
        if options != self.options {
            self.options = options;
            self.build();
        }
    }

    /// The option whose button is at `at`, in this screen's logical
    /// pixels; an unavailable one is none.
    pub fn option_at(&self, at: Point) -> Option<RecordOption> {
        self.option_nodes
            .iter()
            .find(|(id, option)| {
                self.tree.rect(*id).is_some_and(|r| r.contains(at))
                    && !(*option == RecordOption::Camera && self.options.camera.is_none())
            })
            .map(|(_, option)| *option)
    }

    /// The region drawn, or the window under the pointer, on this screen.
    pub fn set(&mut self, chosen: Option<Rect>) {
        if chosen != self.chosen {
            self.chosen = chosen;
            self.build();
        }
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let size = self.tree.size();
        let scale = self.tree.scale();
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        self.option_nodes.clear();
        // Only the dimming and the marks: what is chosen shows through.
        t.set_look(root, Look::default());
        let dim = |t: &mut Tree, r: Rect| {
            if r.w > 0.0 && r.h > 0.0 {
                t.add(
                    root,
                    Style {
                        at: Some(Point::new(r.x, r.y)),
                        width: Length::Px(r.w),
                        height: Length::Px(r.h),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(DIM)),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            }
        };
        let Some(c) = self.chosen else {
            dim(t, Rect::new(0.0, 0.0, size.w, size.h));
            let hint = match self.mode {
                PickMode::Region => self.lang.pick_region(),
                PickMode::Window => self.lang.pick_window(),
                PickMode::Record => self.lang.pick_record(),
            };
            let pill = t
                .add(
                    root,
                    Style {
                        at: Some(Point::new(0.0, size.h / 2.0 - 24.0)),
                        width: Length::Px(size.w),
                        justify: Justify::Center,
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            let label = t
                .add(
                    pill,
                    Style {
                        padding: Insets::symmetric(12.0, 18.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Surface),
                        radius: 14.0,
                        border: Some(Paint::Border),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            t.add(
                label,
                Style::default(),
                Kind::text_line(hint, theme.text.body),
            )
            .unwrap_or_else(MISSING);
            if self.mode == PickMode::Record {
                self.option_nodes = options_row(t, &theme, self.lang, size, self.options);
            }
            return;
        };
        // Dim around what is chosen.
        dim(t, Rect::new(0.0, 0.0, size.w, c.y));
        dim(t, Rect::new(0.0, c.y + c.h, size.w, size.h - c.y - c.h));
        dim(t, Rect::new(0.0, c.y, c.x, c.h));
        dim(t, Rect::new(c.x + c.w, c.y, size.w - c.x - c.w, c.h));
        t.add(
            root,
            Style {
                at: Some(Point::new(c.x - 1.0, c.y - 1.0)),
                width: Length::Px(c.w + 2.0),
                height: Length::Px(c.h + 2.0),
                ..Style::default()
            },
            Kind::Box(Look {
                border: Some(Paint::Accent),
                ..Look::default()
            }),
        )
        .unwrap_or_else(MISSING);
        if self.mode != PickMode::Window {
            // Its size in pixels, under it, or inside when there is no room.
            let text = format!(
                "{} × {}",
                (c.w * scale).round() as i32,
                (c.h * scale).round() as i32
            );
            let below = c.y + c.h + 8.0;
            let y = if below + 30.0 < size.h {
                below
            } else {
                c.y + c.h - 38.0
            };
            let pill = t
                .add(
                    root,
                    Style {
                        at: Some(Point::new(c.x.max(4.0), y.max(4.0))),
                        padding: Insets::symmetric(4.0, 10.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Surface),
                        radius: 8.0,
                        border: Some(Paint::Border),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            t.add(
                pill,
                Style::default(),
                Kind::text_line(text, theme.text.label.with_weight(600)),
            )
            .unwrap_or_else(MISSING);
        }
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

/// The recording's options under the hint: on ones in the accent; the
/// camera unavailable when there is none.
fn options_row(
    t: &mut Tree,
    theme: &Theme,
    lang: Lang,
    size: Size,
    options: RecordOptions,
) -> Vec<(NodeId, RecordOption)> {
    let row = t
        .add(
            t.root(),
            Style {
                at: Some(Point::new(0.0, size.h / 2.0 + 34.0)),
                width: Length::Px(size.w),
                direction: Direction::Row,
                justify: Justify::Center,
                gap: 6.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(MISSING);
    let camera_label = match options.camera {
        Some(_) => lang.camera(),
        None => lang.no_camera(),
    };
    let all = [
        (
            RecordOption::Microphone,
            icons::MICROPHONE,
            lang.microphone(),
            options.microphone,
        ),
        (
            RecordOption::SystemSound,
            icons::SOUND,
            lang.system_sound(),
            options.system_sound,
        ),
        (
            RecordOption::Camera,
            icons::CAMERA,
            camera_label,
            options.camera == Some(true),
        ),
        (
            RecordOption::ShowKeys,
            icons::KEYBOARD,
            lang.show_keys(),
            options.show_keys,
        ),
    ];
    let mut out = Vec::new();
    for (option, icon, label, on) in all {
        let kind = if on {
            ButtonKind::Primary
        } else {
            ButtonKind::Plain
        };
        let mut button = Button::new(label, kind, theme);
        if let Some(icon) = Icon::stroked(icon, 16.0, 2.0) {
            button = button.with_icon(icon);
        }
        let id = t
            .add(
                row,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(MISSING);
        t.set_selected(id, on);
        if option == RecordOption::Camera && options.camera.is_none() {
            t.set_enabled(id, false);
        }
        out.push((id, option));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_region_says_its_size_in_pixels() {
        let mut picker = Picker::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            2.0,
            PickMode::Region,
        );
        picker.draw(Duration::ZERO);
        assert!(picker.access().dump().contains("Drag over a region"));
        picker.set(Some(Rect::new(100.0, 100.0, 320.0, 240.0)));
        picker.draw(Duration::ZERO);
        assert!(picker.access().dump().contains("640 × 480"));
    }
}
