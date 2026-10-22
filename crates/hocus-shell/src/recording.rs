//! The recording's bar (#70): a small pill by what is recorded, with the
//! time, pause and stop; before it starts, the countdown and cancel. The
//! compositor keeps it out of the video.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, ButtonKind, Color, Event, Icon, Input, Insets, Kind, Length, Look, NodeId,
    Paint, PixelRect, Point, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

/// The bar's logical size, its shadow's room included.
pub const SIZE: Size = Size::new(260.0, 76.0);
const ROOM: f32 = 14.0;
/// The red of the recording, as everywhere it is shown.
pub const RED: Color = Color::rgb(0xE5, 0x48, 0x4D);

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");
const PAUSE: &str = "M8 5v14M16 5v14";
const RESUME: &str = "M7 5l12 7-12 7z";
const STOP: &str = "M6 6h12v12H6z";

/// What the bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarState {
    /// Seconds before it starts.
    Counting(u32),
    Recording {
        elapsed: Duration,
        paused: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarEvent {
    Pause,
    Resume,
    /// Stops, or before it starts, gives up.
    Stop,
}

pub struct RecordBar {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    state: Option<BarState>,
    label: NodeId,
    pause: Option<NodeId>,
    stop: NodeId,
}

impl std::fmt::Debug for RecordBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordBar")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    Icon::stroked(path, size, 2.2).unwrap_or_else(|| unreachable!("icon {path}"))
}

impl RecordBar {
    pub fn new(theme: Theme, lang: Lang, scale: f32) -> Self {
        let tree = Tree::new(theme, SIZE, scale);
        // Placeholders until the first build.
        let root = tree.root();
        let mut bar = Self {
            tree,
            surface: Surface::new(SIZE, scale),
            lang,
            state: None,
            label: root,
            pause: None,
            stop: root,
        };
        bar.set(BarState::Counting(3));
        bar
    }

    pub fn set(&mut self, state: BarState) {
        if self.state == Some(state) {
            return;
        }
        let rebuild = match (self.state, state) {
            (Some(BarState::Counting(_)), BarState::Counting(_)) => false,
            (
                Some(BarState::Recording { paused: a, .. }),
                BarState::Recording { paused: b, .. },
            ) => a != b,
            _ => true,
        };
        self.state = Some(state);
        if rebuild {
            self.build();
        }
        let text = match state {
            BarState::Counting(n) => self.lang.recording_in(n),
            BarState::Recording { elapsed, paused } => {
                let s = elapsed.as_secs();
                let time = format!("{}:{:02}", s / 60, s % 60);
                if paused {
                    self.lang.recording_paused(&time)
                } else {
                    time
                }
            }
        };
        self.tree.set_text(self.label, &text);
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        t.set_look(root, Look::default());
        t.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                ..Style::default()
            },
        );
        let pill = t
            .add(
                root,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    direction: hocus_ui::Direction::Row,
                    align: Align::Center,
                    gap: 10.0,
                    padding: Insets {
                        left: 18.0,
                        right: 6.0,
                        ..Insets::default()
                    },
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 24.0,
                    border: Some(Paint::Border),
                    shadow: 12.0,
                }),
            )
            .unwrap_or_else(MISSING);
        let paused = matches!(self.state, Some(BarState::Recording { paused: true, .. }));
        t.add(
            pill,
            Style {
                width: Length::Px(10.0),
                height: Length::Px(10.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(if paused {
                    Paint::Muted
                } else {
                    Paint::Rgba(RED)
                }),
                radius: 5.0,
                ..Look::default()
            }),
        )
        .unwrap_or_else(MISSING);
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.label.with_weight(600)
        };
        self.label = t
            .add(
                pill,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::text_line("", mono),
            )
            .unwrap_or_else(MISSING);
        let button = |t: &mut Tree, icon: &str, name: &str| {
            t.add(
                pill,
                Style {
                    width: Length::Px(36.0),
                    height: Length::Px(36.0),
                    ..Style::default()
                },
                Kind::icon_button(stroked(icon, 16.0), name, ButtonKind::Ghost, &theme),
            )
            .unwrap_or_else(MISSING)
        };
        self.pause = match self.state {
            Some(BarState::Recording { paused, .. }) => Some(if paused {
                button(t, RESUME, lang.recording_resume())
            } else {
                button(t, PAUSE, lang.recording_pause())
            }),
            _ => None,
        };
        let stop = match self.state {
            Some(BarState::Counting(_)) => lang.recording_cancel(),
            _ => lang.recording_stop(),
        };
        self.stop = button(t, STOP, stop);
    }

    /// Whether `at`, from the bar's corner, is on the pill.
    pub fn hit(&self, at: Point) -> bool {
        at.x >= ROOM && at.y >= ROOM && at.x <= SIZE.w - ROOM && at.y <= SIZE.h - ROOM
    }

    /// Pointer input from the bar's corner.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<BarEvent> {
        let forwarded = match input {
            Input::PointerMove(_) if !self.hit(at) => Input::PointerLeave,
            other => other,
        };
        let mut out = Vec::new();
        for event in self.tree.input(forwarded) {
            let Event::Activated(id) = event else {
                continue;
            };
            if id == self.stop {
                out.push(BarEvent::Stop);
            } else if Some(id) == self.pause {
                out.push(match self.state {
                    Some(BarState::Recording { paused: true, .. }) => BarEvent::Resume,
                    _ => BarEvent::Pause,
                });
            }
        }
        out
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

    fn click(bar: &mut RecordBar, name: &str) -> Vec<BarEvent> {
        bar.draw(Duration::ZERO);
        let access = bar.access();
        let node = access
            .walk()
            .into_iter()
            .find(|n| n.label.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no {name}:\n{}", access.dump()));
        let at = Point::new(
            node.rect.x + node.rect.w / 2.0,
            node.rect.y + node.rect.h / 2.0,
        );
        bar.input(at, Input::PointerMove(at));
        bar.input(at, Input::PointerButton { pressed: true });
        bar.input(at, Input::PointerButton { pressed: false })
    }

    #[test]
    fn the_bar_counts_down_then_pauses_and_stops() {
        let mut bar = RecordBar::new(Theme::default(), Lang::En, 1.0);
        bar.set(BarState::Counting(2));
        bar.draw(Duration::ZERO);
        assert!(bar.access().dump().contains("Recording in 2"));
        assert_eq!(click(&mut bar, "Cancel the recording"), [BarEvent::Stop]);
        bar.set(BarState::Recording {
            elapsed: Duration::from_secs(65),
            paused: false,
        });
        bar.draw(Duration::ZERO);
        assert!(bar.access().dump().contains("1:05"));
        assert_eq!(click(&mut bar, "Pause the recording"), [BarEvent::Pause]);
        bar.set(BarState::Recording {
            elapsed: Duration::from_secs(65),
            paused: true,
        });
        assert_eq!(click(&mut bar, "Resume the recording"), [BarEvent::Resume]);
        assert_eq!(click(&mut bar, "Stop the recording"), [BarEvent::Stop]);
    }
}
