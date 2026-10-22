//! Toasts: notifications shown at the top right of the screen being worked
//! on, left of the column, newest on top. They slide in from the edge, the
//! stack reflows with springs, a thin line counts down their time and stops
//! while the pointer is over them; critical ones stay until dismissed.

use std::{collections::VecDeque, time::Duration};

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Input, Insets,
    Kind, Length, Look, NodeId, Paint, PixelRect, Point, Property, Rect, Size, Style, Surface,
    Theme, Tree,
    motion::{self, Curve, Easing, Spring},
};

use crate::{column::View, i18n::Lang, icons};

/// A toast's width, from the design.
pub const WIDTH: f32 = 388.0;
/// Room around the stack for the cards' shadows.
pub const ROOM: f32 = 24.0;
const GAP: f32 = 10.0;
/// Toasts shown at once; older ones go to the history.
const MAX: usize = 5;
const HISTORY: usize = 20;

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// Keys and labels; `default` is a click on the toast itself.
    pub actions: Vec<(String, String)>,
    /// Stays until dismissed, with the waiting colour.
    pub critical: bool,
    /// How long it stays; `None` for until dismissed.
    pub timeout: Option<Duration>,
}

/// What became of a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastEvent {
    Expired(u32),
    Dismissed(u32),
    /// The user chose an action, then the toast went away.
    Action(u32, String),
}

/// An entry of the history the column shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Past {
    pub app: String,
    pub summary: String,
}

/// The thin line at the bottom of a toast: the time it has left.
struct Countdown {
    start: Duration,
    end: Duration,
    /// Where it stopped while the pointer is over the toast.
    frozen: Option<f32>,
}

impl Countdown {
    fn left(&self, now: Duration) -> f32 {
        if let Some(f) = self.frozen {
            return f;
        }
        let total = self.end.saturating_sub(self.start).as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (self.end.saturating_sub(now).as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

impl Draw for Countdown {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, now: Duration) {
        let w = rect.w * self.left(now);
        if w > 0.0 {
            canvas.fill_rect(
                Rect::new(rect.x, rect.y, w, rect.h),
                rect.h / 2.0,
                theme.colors.accent,
            );
        }
    }

    fn size(&self) -> Size {
        Size::new(0.0, 2.0)
    }

    fn is_moving(&self, now: Duration) -> bool {
        self.frozen.is_none() && now < self.end
    }
}

struct Card {
    toast: Toast,
    card: NodeId,
    dismiss: NodeId,
    buttons: Vec<(NodeId, String)>,
    countdown: Option<NodeId>,
    /// When it goes, unless the pointer is over it.
    deadline: Option<Duration>,
    total: Duration,
    paused_left: Option<Duration>,
}

/// The toasts of one screen: their tree, pixels and timers.
pub struct Toasts {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    stack: NodeId,
    /// Top first.
    cards: Vec<Card>,
    history: VecDeque<Past>,
    hovered: Option<u32>,
    now: Duration,
}

impl std::fmt::Debug for Toasts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toasts")
            .field("shown", &self.cards.len())
            .finish_non_exhaustive()
    }
}

/// A colour for an application, the same every time.
pub(crate) fn app_color(app: &str) -> Color {
    const PALETTE: [Color; 7] = [
        Color::rgb(0x2f, 0x6f, 0xd6),
        Color::rgb(0x8e, 0x6c, 0xd8),
        Color::rgb(0x7b, 0xc4, 0x7f),
        Color::rgb(0xf2, 0xa6, 0x5a),
        Color::rgb(0xf2, 0x8d, 0xb2),
        Color::rgb(0x38, 0xd9, 0xf5),
        Color::rgb(0xc9, 0xa2, 0x27),
    ];
    let hash = app
        .bytes()
        .fold(5381_u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    PALETTE[hash as usize % PALETTE.len()]
}

impl Toasts {
    /// Toasts on a screen whose work area is `height` logical pixels tall.
    pub fn new(theme: Theme, lang: Lang, height: f32, scale: f32) -> Self {
        let size = Size::new(WIDTH + 2.0 * ROOM, height + 2.0 * ROOM);
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        tree.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                ..Style::default()
            },
        );
        let stack = tree
            .add(
                root,
                Style {
                    gap: GAP,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(|| unreachable!("the root takes children"));
        Self {
            lang,
            tree,
            surface: Surface::new(size, scale),
            stack,
            cards: Vec::new(),
            history: VecDeque::new(),
            hovered: None,
            now: Duration::ZERO,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// The ids shown, top first.
    pub fn shown(&self) -> Vec<u32> {
        self.cards.iter().map(|c| c.toast.id).collect()
    }

    /// The newest first.
    pub fn history(&self) -> impl Iterator<Item = &Past> {
        self.history.iter()
    }

    pub fn resize(&mut self, height: f32, scale: f32) {
        let size = Size::new(WIDTH + 2.0 * ROOM, height + 2.0 * ROOM);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme);
    }

    /// Where every card is now, to move them from there after a change.
    fn places(&mut self) -> Vec<(NodeId, f32)> {
        self.tree.layout();
        self.cards
            .iter()
            .filter_map(|c| self.tree.rect(c.card).map(|r| (c.card, r.y)))
            .collect()
    }

    /// Cards that moved spring from where they were.
    fn reflow(&mut self, before: &[(NodeId, f32)]) {
        self.tree.layout();
        for (id, y) in before {
            if let Some(r) = self.tree.rect(*id) {
                let delta = y - r.y;
                if delta.abs() > 0.5 {
                    self.tree.jump(*id, Property::OffsetY, delta);
                    self.tree
                        .animate(*id, Property::OffsetY, 0.0, Curve::Spring(Spring::SMOOTH));
                }
            }
        }
    }

    /// Shows a toast, or replaces the one with its id, at `now`.
    pub fn show(&mut self, toast: Toast, now: Duration) -> Vec<ToastEvent> {
        self.now = now;
        let mut events = Vec::new();
        if let Some(i) = self.cards.iter().position(|c| c.toast.id == toast.id) {
            // Replaced in place: it keeps its place in the stack.
            let before = self.places();
            let old = self.cards.remove(i);
            self.tree.remove(old.card);
            let card = self.build(toast, i, now);
            self.cards.insert(i, card);
            self.reflow(&before);
            return events;
        }
        let before = self.places();
        let card = self.build(toast.clone(), 0, now);
        self.cards.insert(0, card);
        self.history.push_front(Past {
            app: toast.app.clone(),
            summary: toast.summary.clone(),
        });
        self.history.truncate(HISTORY);
        // It slides in from the edge while the others make room.
        let id = self.cards[0].card;
        self.tree.jump(id, Property::OffsetX, WIDTH + ROOM);
        self.tree.jump(id, Property::Opacity, 0.0);
        let enter = Curve::Ease(Easing::OutCubic, motion::STANDARD);
        self.tree.animate(id, Property::OffsetX, 0.0, enter);
        self.tree.animate(id, Property::Opacity, 1.0, enter);
        self.reflow(&before);
        while self.cards.len() > MAX {
            let last = self.cards.len() - 1;
            events.push(ToastEvent::Expired(self.cards[last].toast.id));
            self.take(last);
        }
        events
    }

    /// Closes a toast, as its client asked: no event.
    pub fn close(&mut self, id: u32) -> bool {
        match self.cards.iter().position(|c| c.toast.id == id) {
            Some(i) => {
                self.take(i);
                true
            }
            None => false,
        }
    }

    /// Removes the card at `i`; the others close the gap.
    fn take(&mut self, i: usize) {
        let card = self.cards.remove(i);
        let before: Vec<(NodeId, f32)> = self
            .cards
            .iter()
            .filter_map(|c| self.tree.rect(c.card).map(|r| (c.card, r.y)))
            .collect();
        self.tree.remove(card.card);
        if self.hovered == Some(card.toast.id) {
            self.hovered = None;
        }
        self.reflow(&before);
    }

    fn build(&mut self, toast: Toast, index: usize, now: Duration) -> Card {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let t = &mut self.tree;
        let missing = || unreachable!("adding to a node of this tree");
        let card = t
            .insert(
                self.stack,
                index,
                Style {
                    padding: Insets::symmetric(16.0, 18.0),
                    gap: 8.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 18.0,
                    border: Some(if toast.critical {
                        Paint::Waiting
                    } else {
                        Paint::Border
                    }),
                    shadow: 18.0,
                }),
            )
            .unwrap_or_else(missing);
        if toast.actions.iter().any(|(k, _)| k == "default") {
            t.set_clickable(card, &format!("{}: {}", toast.app, toast.summary));
        }
        let head = t
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    gap: 10.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        t.add(
            head,
            Style {
                width: Length::Px(22.0),
                height: Length::Px(22.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(app_color(&toast.app))),
                radius: 6.0,
                ..Look::default()
            }),
        )
        .unwrap_or_else(missing);
        t.add(
            head,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(toast.app.as_str(), theme.text.label.with_weight(700)),
        )
        .unwrap_or_else(missing);
        let dismiss = t
            .add(
                head,
                Style {
                    width: Length::Px(32.0),
                    height: Length::Px(32.0),
                    ..Style::default()
                },
                Kind::Button(
                    Button::new("", ButtonKind::Ghost, &theme)
                        .with_icon(
                            hocus_ui::Icon::stroked(icons::CLOSE, 14.0, 2.1)
                                .unwrap_or_else(|| unreachable!("a constant icon")),
                        )
                        .with_name(lang.dismiss(&toast.summary)),
                ),
            )
            .unwrap_or_else(missing);
        if !toast.summary.is_empty() {
            t.add(
                card,
                Style::default(),
                Kind::text(toast.summary.as_str(), theme.text.title.with_weight(600)),
            )
            .unwrap_or_else(missing);
        }
        if !toast.body.is_empty() {
            t.add(
                card,
                Style::default(),
                Kind::text(toast.body.as_str(), theme.text.body),
            )
            .unwrap_or_else(missing);
        }
        let mut buttons = Vec::new();
        let shown: Vec<_> = toast
            .actions
            .iter()
            .filter(|(k, _)| k != "default")
            .collect();
        if !shown.is_empty() {
            let row = t
                .add(
                    card,
                    Style {
                        direction: Direction::Row,
                        gap: 8.0,
                        padding: Insets {
                            top: 4.0,
                            ..Insets::default()
                        },
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(missing);
            for (i, (key, label)) in shown.into_iter().enumerate() {
                let kind = if i == 0 {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Plain
                };
                let id = t
                    .add(
                        row,
                        Style {
                            height: Length::Px(40.0),
                            ..Style::default()
                        },
                        Kind::button(label.as_str(), kind, &theme),
                    )
                    .unwrap_or_else(missing);
                buttons.push((id, key.clone()));
            }
        }
        let (deadline, total, countdown) = match toast.timeout {
            Some(total) => {
                let id = t
                    .add(
                        card,
                        Style {
                            width: Length::Fill,
                            ..Style::default()
                        },
                        Kind::Custom {
                            draw: Box::new(Countdown {
                                start: now,
                                end: now + total,
                                frozen: None,
                            }),
                            label: None,
                        },
                    )
                    .unwrap_or_else(missing);
                (Some(now + total), total, Some(id))
            }
            None => (None, Duration::ZERO, None),
        };
        Card {
            toast,
            card,
            dismiss,
            buttons,
            countdown,
            deadline,
            total,
            paused_left: None,
        }
    }

    // Time.

    /// Toasts whose time is up go; returns them.
    pub fn update(&mut self, now: Duration) -> Vec<ToastEvent> {
        self.now = now;
        let expired: Vec<u32> = self
            .cards
            .iter()
            .filter(|c| c.paused_left.is_none() && c.deadline.is_some_and(|d| now >= d))
            .map(|c| c.toast.id)
            .collect();
        for id in &expired {
            self.close(*id);
        }
        expired.into_iter().map(ToastEvent::Expired).collect()
    }

    /// The next time something expires, for a timer.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.cards
            .iter()
            .filter(|c| c.paused_left.is_none())
            .filter_map(|c| c.deadline)
            .min()
    }

    fn pause(&mut self, id: u32, paused: bool) {
        let now = self.now;
        let Some(card) = self.cards.iter_mut().find(|c| c.toast.id == id) else {
            return;
        };
        let (Some(deadline), Some(line)) = (card.deadline, card.countdown) else {
            return;
        };
        let countdown = if paused {
            let left = deadline.saturating_sub(now);
            card.paused_left = Some(left);
            let fraction = left.as_secs_f32() / card.total.as_secs_f32().max(f32::EPSILON);
            Countdown {
                start: now,
                end: now + left,
                frozen: Some(fraction.clamp(0.0, 1.0)),
            }
        } else {
            let left = card.paused_left.take().unwrap_or_default();
            card.deadline = Some(now + left);
            Countdown {
                start: now + left - card.total,
                end: now + left,
                frozen: None,
            }
        };
        self.tree.set_draw(line, Box::new(countdown));
    }

    // Input, relative to the stack's surface.

    fn card_at(&self, at: Point) -> Option<u32> {
        self.cards.iter().find_map(|c| {
            self.tree
                .rect(c.card)
                .filter(|r| r.contains(at))
                .map(|_| c.toast.id)
        })
    }

    pub fn hit(&self, at: Point) -> bool {
        self.card_at(at).is_some()
    }

    pub fn input(&mut self, at: Point, input: Input) -> Vec<ToastEvent> {
        let over = match input {
            Input::PointerLeave => None,
            _ => self.card_at(at),
        };
        if over != self.hovered {
            if let Some(old) = self.hovered {
                self.pause(old, false);
            }
            if let Some(new) = over {
                self.pause(new, true);
            }
            self.hovered = over;
        }
        let forwarded = match input {
            Input::PointerMove(_) if over.is_none() => Input::PointerLeave,
            other => other,
        };
        let mut events = Vec::new();
        for Event::Activated(node) in self.tree.input(forwarded) {
            let Some(i) = self.cards.iter().position(|c| {
                c.card == node || c.dismiss == node || c.buttons.iter().any(|(b, _)| *b == node)
            }) else {
                continue;
            };
            let card = &self.cards[i];
            let id = card.toast.id;
            if node == card.dismiss {
                events.push(ToastEvent::Dismissed(id));
            } else if node == card.card {
                events.push(ToastEvent::Action(id, "default".into()));
            } else if let Some((_, key)) = card.buttons.iter().find(|(b, _)| *b == node) {
                events.push(ToastEvent::Action(id, key.clone()));
            }
            self.take(i);
        }
        events
    }

    // Drawing.

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.now = now;
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

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }

    /// How tall the stack is now, without the room for shadows.
    pub fn stack_rect(&self) -> Rect {
        self.tree.rect(self.stack).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_keeps_its_colour() {
        assert_eq!(app_color("mail"), app_color("mail"));
    }
}
