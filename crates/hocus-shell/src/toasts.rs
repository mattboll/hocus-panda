//! Toasts: notifications shown at the top right of the screen being worked
//! on, left of the column, newest on top. They slide in from the edge, the
//! stack reflows with springs, a thin line counts down their time and stops
//! while the pointer is over them; critical ones stay until dismissed. A
//! toast whose actions include [`REPLY`] takes a reply typed into it, and
//! waits while it is typed. In the Matrix theme a toast comes in where it
//! goes, the compositor typing it in over its place, and says where the
//! ones that go were, for their glyphs to scatter from there.

use std::{collections::VecDeque, time::Duration};

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Input, Insets,
    Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Property, Rect, Size, Style, Surface,
    TextField, Theme, Tree,
    motion::{self, Curve, Easing, Spring},
};

pub use crate::phone::REPLY;
use crate::{column::View, i18n::Lang, icons};

/// A toast's width, from the design.
pub const WIDTH: f32 = 388.0;
/// Room around the stack for the cards' shadows.
pub const ROOM: f32 = 24.0;
const GAP: f32 = 10.0;
/// Toasts shown at once; older ones go to the history.
const MAX: usize = 5;
/// What the history keeps unless told otherwise.
const HISTORY: usize = 20;

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// Keys and labels; `default` is a click on the toast itself,
    /// [`REPLY`] opens a field to reply in.
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
    /// The user sent this reply, then the toast went away.
    Replied(u32, String),
}

/// An entry of the history the column shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Past {
    pub id: u32,
    pub app: String,
    pub summary: String,
    /// Not closed for its client yet: its toast shows, or it never showed
    /// (do not disturb, the history only). Taken from the history, it is
    /// closed then, once.
    pub open: bool,
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
    /// The row of its actions, which a reply's field takes.
    row: Option<NodeId>,
    /// While a reply is typed: its field and its Send.
    reply: Option<(NodeId, NodeId)>,
    countdown: Option<NodeId>,
    /// When it goes, unless the pointer is over it.
    deadline: Option<Duration>,
    total: Duration,
    paused_left: Option<Duration>,
}

/// How a new toast comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Entrance {
    /// From the screen's edge.
    #[default]
    Slide,
    /// Where it goes, at once: the Matrix theme materializes it there.
    InPlace,
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
    /// How many the history keeps.
    history_limit: usize,
    hovered: Option<u32>,
    /// The toast a reply is typed in: it has the keyboard.
    replying: Option<u32>,
    now: Duration,
    entrance: Entrance,
    /// Toasts gone since asked last, and where their cards were.
    gone: Vec<(u32, Rect)>,
}

impl std::fmt::Debug for Toasts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toasts")
            .field("shown", &self.cards.len())
            .finish_non_exhaustive()
    }
}

/// A colour for an application, the same every time.
pub fn app_color(app: &str) -> Color {
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
            history_limit: HISTORY,
            hovered: None,
            replying: None,
            now: Duration::ZERO,
            entrance: Entrance::Slide,
            gone: Vec::new(),
        }
    }

    pub fn set_entrance(&mut self, entrance: Entrance) {
        self.entrance = entrance;
    }

    /// Where the card of toast `id` is, from the corner, once laid out.
    pub fn card_rect(&mut self, id: u32) -> Option<Rect> {
        self.tree.layout();
        let card = self.cards.iter().find(|c| c.toast.id == id)?;
        self.tree.rect(card.card)
    }

    /// The toasts gone since the last call, and where their cards were.
    pub fn take_gone(&mut self) -> Vec<(u32, Rect)> {
        std::mem::take(&mut self.gone)
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

    /// Speaks `lang` for the toasts to come.
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
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
        events.extend(self.keep(&toast));
        // It slides in from the edge while the others make room.
        if self.entrance == Entrance::Slide {
            let id = self.cards[0].card;
            self.tree.jump(id, Property::OffsetX, WIDTH + ROOM);
            self.tree.jump(id, Property::Opacity, 0.0);
            let enter = Curve::Ease(Easing::OutCubic, motion::STANDARD);
            self.tree.animate(id, Property::OffsetX, 0.0, enter);
            self.tree.animate(id, Property::Opacity, 1.0, enter);
        }
        self.reflow(&before);
        while self.cards.len() > MAX {
            let last = self.cards.len() - 1;
            events.push(ToastEvent::Expired(self.cards[last].toast.id));
            self.take(last);
        }
        events
    }

    /// Keeps a notification in the history without showing it (do not
    /// disturb); those it pushes out of the history expire.
    pub fn remember(&mut self, toast: &Toast) -> Vec<ToastEvent> {
        self.keep(toast)
    }

    /// The newest of the history, in place of an older one of its id;
    /// those pushed out of it expire if nothing else holds them.
    fn keep(&mut self, toast: &Toast) -> Vec<ToastEvent> {
        self.history.retain(|p| p.id != toast.id);
        self.history.push_front(Past {
            id: toast.id,
            app: toast.app.clone(),
            summary: toast.summary.clone(),
            open: true,
        });
        self.trim()
    }

    /// The history keeps `limit` at most from now on; those it lets go
    /// expire.
    pub fn set_history_limit(&mut self, limit: usize) -> Vec<ToastEvent> {
        self.history_limit = limit;
        self.trim()
    }

    /// Down to the limit: what was open there, and shows no more, expires.
    fn trim(&mut self) -> Vec<ToastEvent> {
        let mut events = Vec::new();
        while self.history.len() > self.history_limit {
            let Some(past) = self.history.pop_back() else {
                break;
            };
            if past.open && !self.cards.iter().any(|c| c.toast.id == past.id) {
                events.push(ToastEvent::Expired(past.id));
            }
        }
        events
    }

    /// The user cleared the history: each notification goes, its toast
    /// too, and those still open are dismissed.
    pub fn clear_history(&mut self) -> Vec<ToastEvent> {
        let ids: Vec<u32> = self.history.iter().map(|p| p.id).collect();
        ids.into_iter().flat_map(|id| self.dismiss(id)).collect()
    }

    /// The user dismissed notification `id` from the history: it goes, its
    /// toast too; dismissed if it was still open.
    pub fn dismiss(&mut self, id: u32) -> Vec<ToastEvent> {
        let shown = self.cards.iter().position(|c| c.toast.id == id);
        let open = shown.is_some() || self.history.iter().any(|p| p.id == id && p.open);
        self.history.retain(|p| p.id != id);
        if let Some(i) = shown {
            self.take(i);
        }
        if open {
            vec![ToastEvent::Dismissed(id)]
        } else {
            Vec::new()
        }
    }

    /// Closes a notification, as its client asked: no event. Whether it
    /// was open, shown or in the history.
    pub fn close(&mut self, id: u32) -> bool {
        let kept = self.history.iter().any(|p| p.id == id && p.open);
        match self.cards.iter().position(|c| c.toast.id == id) {
            Some(i) => {
                self.take(i);
                true
            }
            None => {
                self.closed(id);
                kept
            }
        }
    }

    /// Notification `id` is closed for its client: the history keeps it,
    /// closed.
    fn closed(&mut self, id: u32) {
        for past in self.history.iter_mut().filter(|p| p.id == id) {
            past.open = false;
        }
    }

    /// Removes the card at `i`; the others close the gap. Every toast that
    /// goes is closed for its client (expired, dismissed, acted on, or as
    /// it asked).
    fn take(&mut self, i: usize) {
        let card = self.cards.remove(i);
        self.closed(card.toast.id);
        if let Some(rect) = self.tree.rect(card.card) {
            // Kept for the compositor, which takes them after each change.
            self.gone.truncate(MAX * 2);
            self.gone.push((card.toast.id, rect));
        }
        let before: Vec<(NodeId, f32)> = self
            .cards
            .iter()
            .filter_map(|c| self.tree.rect(c.card).map(|r| (c.card, r.y)))
            .collect();
        self.tree.remove(card.card);
        if self.hovered == Some(card.toast.id) {
            self.hovered = None;
        }
        if self.replying == Some(card.toast.id) {
            self.replying = None;
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
                    background: Some(Paint::Glass),
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
        let mut row = None;
        let shown: Vec<_> = toast
            .actions
            .iter()
            .filter(|(k, _)| k != "default")
            .collect();
        if !shown.is_empty() {
            let actions = t
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
                        actions,
                        Style {
                            height: Length::Px(40.0),
                            ..Style::default()
                        },
                        Kind::button(label.as_str(), kind, &theme),
                    )
                    .unwrap_or_else(missing);
                buttons.push((id, key.clone()));
            }
            row = Some(actions);
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
            row,
            reply: None,
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
        // A reply being typed holds its toast until it is sent.
        if !paused && self.replying == Some(id) {
            return;
        }
        let now = self.now;
        let Some(card) = self.cards.iter_mut().find(|c| c.toast.id == id) else {
            return;
        };
        let (Some(deadline), Some(line)) = (card.deadline, card.countdown) else {
            return;
        };
        let countdown = if paused {
            if card.paused_left.is_some() {
                return;
            }
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
        for event in self.tree.input(forwarded) {
            let Event::Activated(node) = event else {
                continue;
            };
            let Some(i) = self.cards.iter().position(|c| {
                c.card == node
                    || c.dismiss == node
                    || c.buttons.iter().any(|(b, _)| *b == node)
                    || c.reply.is_some_and(|(_, send)| send == node)
            }) else {
                continue;
            };
            let card = &self.cards[i];
            let id = card.toast.id;
            if node == card.dismiss {
                events.push(ToastEvent::Dismissed(id));
            } else if card.reply.is_some_and(|(_, send)| send == node) {
                events.extend(self.send_reply(i));
                continue;
            } else if node == card.card {
                events.push(ToastEvent::Action(id, "default".into()));
            } else if let Some((_, key)) = card.buttons.iter().find(|(b, _)| *b == node) {
                if key == REPLY {
                    self.open_reply(i);
                    continue;
                }
                events.push(ToastEvent::Action(id, key.clone()));
            }
            self.take(i);
        }
        events
    }

    // Replies.

    /// Whether a reply is being typed: the keys are for it.
    pub fn replying(&self) -> bool {
        self.replying.is_some()
    }

    /// The card at `i` takes a reply: its actions make room for a field
    /// with the keyboard, and it waits until the reply goes.
    fn open_reply(&mut self, i: usize) {
        // One reply at a time: one typed elsewhere is left as it was.
        if self.replying.is_some() {
            return;
        }
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let id = self.cards[i].toast.id;
        let Some(row) = self.cards[i].row else {
            return;
        };
        let t = &mut self.tree;
        for child in t.children(row) {
            t.remove(child);
        }
        let missing = || unreachable!("adding to a node of this tree");
        let field_box = t
            .add(
                row,
                Style {
                    width: Length::Fill,
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    align: Align::Center,
                    direction: Direction::Row,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 12.0,
                    border: Some(Paint::Border),
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        let field = t
            .add(
                field_box,
                Style {
                    width: Length::Fill,
                    height: Length::Px(22.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(TextField::new(
                    lang.reply(),
                    lang.reply_placeholder(),
                    theme.text.body,
                ))),
            )
            .unwrap_or_else(missing);
        let send = t
            .add(
                row,
                Style {
                    height: Length::Px(40.0),
                    ..Style::default()
                },
                Kind::button(lang.send(), ButtonKind::Primary, &theme),
            )
            .unwrap_or_else(missing);
        t.set_focus(Some(field));
        self.cards[i].buttons.clear();
        self.cards[i].reply = Some((field, send));
        self.pause(id, true);
        self.replying = Some(id);
    }

    /// The reply goes, unless nothing was typed.
    fn send_reply(&mut self, i: usize) -> Vec<ToastEvent> {
        let Some((field, _)) = self.cards[i].reply else {
            return Vec::new();
        };
        let text = self
            .tree
            .field_value(field)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if text.is_empty() {
            return Vec::new();
        }
        let id = self.cards[i].toast.id;
        self.take(i);
        vec![ToastEvent::Replied(id, text)]
    }

    /// Text typed into the reply.
    pub fn text(&mut self, text: &str) {
        let field = self
            .cards
            .iter()
            .find(|c| Some(c.toast.id) == self.replying)
            .and_then(|c| c.reply)
            .map(|(field, _)| field);
        if let Some(field) = field {
            self.tree.set_focus(Some(field));
            self.tree.text_input(text);
        }
    }

    /// A key while a reply is typed: Enter sends it, Escape dismisses the
    /// toast.
    pub fn key(&mut self, key: Key) -> Vec<ToastEvent> {
        let Some(i) = self
            .cards
            .iter()
            .position(|c| Some(c.toast.id) == self.replying)
        else {
            return Vec::new();
        };
        match key {
            Key::Enter => self.send_reply(i),
            Key::Escape => {
                let id = self.cards[i].toast.id;
                self.take(i);
                vec![ToastEvent::Dismissed(id)]
            }
            key => {
                let mut events = Vec::new();
                for event in self.tree.input(Input::Key(key)) {
                    // Tab onto Send, then Space.
                    if let Event::Activated(node) = event
                        && self.cards[i].reply.is_some_and(|(_, send)| send == node)
                    {
                        events.extend(self.send_reply(i));
                        break;
                    }
                }
                events
            }
        }
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

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
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

    #[test]
    fn in_place_a_toast_does_not_slide_and_says_where_it_was() {
        let mut toasts = Toasts::new(Theme::default(), Lang::En, 800.0, 1.0);
        toasts.set_entrance(Entrance::InPlace);
        let toast = Toast {
            id: 7,
            app: "Mail".into(),
            summary: "Lunch".into(),
            body: String::new(),
            actions: Vec::new(),
            critical: false,
            timeout: None,
        };
        let now = Duration::from_secs(1);
        toasts.show(toast, now);
        toasts.draw(now);
        assert!(!toasts.is_animating(now), "no slide");
        let rect = toasts.card_rect(7).expect("laid out");
        assert_eq!(rect.x, ROOM);
        assert!(toasts.close(7));
        assert_eq!(toasts.take_gone(), vec![(7, rect)]);
        assert!(toasts.take_gone().is_empty());
    }
}
