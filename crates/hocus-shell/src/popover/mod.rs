//! Popovers beside the bar (bar.md §9): what opens them, what closes them,
//! one at a time. The sound's ([`sound`]) is the first; Wi-Fi, Bluetooth,
//! the brightness and the battery will open the same way.
//!
//! - **By a click** on its item (the whole row in the full form, the gauge
//!   in the mini): it stays open, whatever the pointer does, until a click
//!   outside it, `Esc`, or another click on the item. It takes the
//!   keyboard: `Esc` closes it and gives the keyboard to the item, where
//!   `Enter` or `Space` opens it again and `Esc` lets the keyboard go.
//! - **By hovering** the item for 150 ms in the mini form: it closes 300 ms
//!   after the pointer has really left the item and the popover, and stays
//!   while the pointer moves toward it, within the cone from where the
//!   pointer was to the popover's near corners. A click on the item then
//!   keeps it open, as a click would have opened it.
//!
//! The popover is a layer surface of its own, beside the bar, aligned with
//! its item and kept 8 px from the screen's bottom. Nothing here goes
//! through iced's messages, which would draw every surface again: the
//! surfaces report what the pointer and the keys do ([`event`]), the
//! [`Controller`] decides, and [`effects`] carries out what it says as
//! layer-shell actions, which iced runs without drawing anything else, and
//! as lines to the compositor (`wake`, to draw the item's surface again;
//! `closing`, to fade the popover out). The compositor moves the popover in
//! and out, blurs behind it and draws its shadow; it tells the shell about
//! presses on none of its surfaces (`outside`).

pub mod sound;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    EDGE, Form, LEVELS_NAMESPACE, POPOVER_CLOSE, POPOVER_NAMESPACE, POPOVER_WIDTH, ToCompositor,
};
use iced::{Point, Rectangle, Size, window};
use iced_layershell::reexport::KeyboardInteractivity;

/// Hovering a mini form's item this long opens its popover.
pub const HOVER_OPEN: Duration = Duration::from_millis(150);
/// A popover opened by hovering closes this long after the pointer left.
pub const LEAVE_CLOSE: Duration = Duration::from_millis(300);

/// What opens a popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Sound,
}

impl Item {
    /// Its popover's height, logical pixels.
    fn height(self) -> u32 {
        match self {
            Self::Sound => sound::height(crate::levels::sound::sinks().len()),
        }
    }
}

/// How a popover opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Click,
    Hover,
}

/// A bar, as the popovers need it: its form, its height, and its levels'
/// surface (which holds the keyboard after `Esc`). Points are in its frame:
/// from its outer right edge (negative inside it) and from its top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub form: Form,
    pub height: f32,
    pub levels: window::Id,
    /// The bar's own surface, whose size gives its height.
    pub surface: window::Id,
}

/// A bar's height until its surface's is known.
const HEIGHT: f32 = 1064.0;

/// What the shell's surfaces report.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The pointer is on `item` of `bar`, which is at `rect` (bar frame).
    ItemEnter {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// It left the item; last seen at `at`.
    ItemLeave {
        at: Point,
    },
    /// A press on `item` of `bar`, at `rect`.
    ItemPress {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// The pointer seen at `at` on a bar's surfaces, off its items.
    Pointer {
        at: Point,
    },
    PopoverEnter,
    PopoverLeave,
    /// A press on the bar's surfaces, off its items and the popover.
    PressElsewhere,
    /// A press on none of the shell's surfaces (the compositor's
    /// `outside`).
    Outside,
    /// `Esc` in the popover.
    Escape,
    /// `Enter` or `Space` on the item holding the keyboard.
    ItemActivate,
    /// `Esc` on the item holding the keyboard.
    ItemRelease,
    /// `bar` changed form, or went.
    BarChanged {
        bar: String,
    },
}

/// What the controller asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A popover's surface for `item`, on `bar`'s output: `size`, and its
    /// layer-shell margins (top, right, bottom, left).
    Open {
        id: window::Id,
        bar: String,
        item: Item,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
        keyboard: KeyboardInteractivity,
    },
    Keyboard(window::Id, KeyboardInteractivity),
    /// The compositor fades it out (`closing`).
    Closing,
    Remove(window::Id),
    /// The surfaces of this namespace show something new.
    Wake(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Closed,
    /// The pointer rests on a mini form's item, since then.
    Hovering {
        bar: String,
        item: Item,
        rect: Rectangle,
        since: Instant,
    },
    Open(Open),
}

#[derive(Debug, Clone, PartialEq)]
struct Open {
    bar: String,
    item: Item,
    how: Opened,
    surface: window::Id,
    /// The popover, and its item, in the bar's frame.
    rect: Rectangle,
    from: Rectangle,
    /// Whether the pointer is on the item or the popover.
    inside: bool,
    /// Hovered: when it closes unless the pointer comes back, and where
    /// the pointer was last seen.
    leave: Option<Instant>,
    last: Option<Point>,
}

/// Which popover shows, and what is under way.
#[derive(Debug)]
pub struct Controller {
    phase: Phase,
    /// Surfaces fading out, what they show, and when they go.
    closing: Vec<(window::Id, Item, Instant)>,
    /// The item holding the keyboard (after `Esc` in its popover): its bar,
    /// and where it is.
    focused: Option<(String, Item, Rectangle)>,
    bars: HashMap<String, Place>,
    /// Popovers move in and out (else they show and go at once).
    pub motion: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            closing: Vec::new(),
            focused: None,
            bars: HashMap::new(),
            motion: true,
        }
    }
}

impl Controller {
    /// Bar `bar` in `form`, its surfaces `surface` and `levels`; its
    /// height stays as known.
    pub fn place(&mut self, bar: &str, form: Form, surface: window::Id, levels: window::Id) {
        let height = self.bars.get(bar).map_or(HEIGHT, |p| p.height);
        let place = Place {
            form,
            height,
            levels,
            surface,
        };
        self.bars.insert(bar.to_owned(), place);
    }

    /// The bar whose surface is `surface` is `height` tall.
    pub fn height(&mut self, surface: window::Id, height: f32) {
        for place in self.bars.values_mut().filter(|p| p.surface == surface) {
            place.height = height;
        }
    }

    /// Bar `bar`'s height.
    pub fn bar_height(&self, bar: &str) -> f32 {
        self.bars.get(bar).map_or(HEIGHT, |p| p.height)
    }

    pub fn forget(&mut self, bar: &str) {
        self.bars.remove(bar);
    }

    /// What the popover's surface `id` shows, open or closing.
    pub fn item_of(&self, id: window::Id) -> Option<Item> {
        match &self.phase {
            Phase::Open(open) if open.surface == id => Some(open.item),
            _ => self
                .closing
                .iter()
                .find(|(c, _, _)| *c == id)
                .map(|(_, item, _)| *item),
        }
    }

    /// Whether the popover `id` is open (not closing): it reacts.
    pub fn is_open(&self, id: window::Id) -> bool {
        matches!(&self.phase, Phase::Open(open) if open.surface == id)
    }

    /// Whether `item` of `bar` has its popover open.
    pub fn shows(&self, bar: &str, item: Item) -> bool {
        matches!(&self.phase, Phase::Open(o) if o.bar == bar && o.item == item)
    }

    /// Whether `item` of `bar` holds the keyboard.
    pub fn focused(&self, bar: &str, item: Item) -> bool {
        self.focused
            .as_ref()
            .is_some_and(|(b, i, _)| b == bar && *i == item)
    }

    /// When [`Self::tick`] has something to do.
    pub fn deadline(&self) -> Option<Instant> {
        let phase = match &self.phase {
            Phase::Hovering { since, .. } => Some(*since + HOVER_OPEN),
            Phase::Open(open) => open.leave,
            Phase::Closed => None,
        };
        self.closing.iter().map(|(_, _, at)| *at).chain(phase).min()
    }

    /// What `event` at `now` does.
    pub fn handle(&mut self, event: Event, now: Instant) -> Vec<Effect> {
        tracing::trace!(?event, phase = ?self.phase, "popover event");
        let mut effects = Vec::new();
        match event {
            Event::ItemEnter { bar, item, rect } => match &mut self.phase {
                Phase::Closed if self.bars.get(&bar).map(|p| p.form) == Some(Form::Mini) => {
                    self.phase = Phase::Hovering {
                        bar,
                        item,
                        rect,
                        since: now,
                    };
                }
                Phase::Open(open) if open.bar == bar && open.item == item => {
                    open.inside = true;
                    open.leave = None;
                }
                _ => {}
            },
            Event::ItemLeave { at } => match &mut self.phase {
                Phase::Hovering { .. } => self.phase = Phase::Closed,
                Phase::Open(open) => open.left(at, now),
                Phase::Closed => {}
            },
            Event::PopoverEnter => {
                if let Phase::Open(open) = &mut self.phase {
                    open.inside = true;
                    open.leave = None;
                }
            }
            Event::PopoverLeave => {
                if let Phase::Open(open) = &mut self.phase {
                    // Where it left is the popover's own: the next move seen
                    // on the bar starts the cone again.
                    open.inside = false;
                    open.last = None;
                    if open.how == Opened::Hover {
                        open.leave = Some(now + LEAVE_CLOSE);
                    }
                }
            }
            Event::Pointer { at } => {
                if let Phase::Open(open) = &mut self.phase {
                    open.seen(at, now);
                }
            }
            Event::ItemPress { bar, item, rect } => {
                self.unfocus(&mut effects);
                match &mut self.phase {
                    Phase::Open(open) if open.bar == bar && open.item == item => {
                        if open.how == Opened::Hover {
                            // A click keeps it, as a click would have
                            // opened it.
                            open.how = Opened::Click;
                            open.leave = None;
                            effects.push(Effect::Keyboard(
                                open.surface,
                                KeyboardInteractivity::Exclusive,
                            ));
                        } else {
                            self.close(now, &mut effects);
                        }
                    }
                    _ => {
                        self.close(now, &mut effects);
                        self.open(&bar, item, rect, Opened::Click, &mut effects);
                    }
                }
            }
            Event::PressElsewhere | Event::Outside => {
                self.unfocus(&mut effects);
                self.close(now, &mut effects);
            }
            Event::Escape => {
                if let Phase::Open(open) = &self.phase {
                    let (bar, item, from) = (open.bar.clone(), open.item, open.from);
                    self.close(now, &mut effects);
                    if let Some(place) = self.bars.get(&bar) {
                        effects.push(Effect::Keyboard(
                            place.levels,
                            KeyboardInteractivity::Exclusive,
                        ));
                        self.focused = Some((bar, item, from));
                    }
                }
            }
            Event::ItemActivate => {
                if let Some((bar, item, from)) = self.focused.clone() {
                    self.unfocus(&mut effects);
                    self.open(&bar, item, from, Opened::Click, &mut effects);
                }
            }
            Event::ItemRelease => self.unfocus(&mut effects),
            Event::BarChanged { bar } => {
                let on_it = match &self.phase {
                    Phase::Open(open) => open.bar == bar,
                    Phase::Hovering { bar: b, .. } => *b == bar,
                    Phase::Closed => false,
                };
                if on_it {
                    self.close(now, &mut effects);
                }
                if self.focused.as_ref().is_some_and(|(b, _, _)| *b == bar) {
                    self.unfocus(&mut effects);
                }
            }
        }
        effects
    }

    /// What is due at `now`.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.closing.retain(|(id, _, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        match &self.phase {
            Phase::Hovering {
                bar,
                item,
                rect,
                since,
            } if *since + HOVER_OPEN <= now => {
                let (bar, item, rect) = (bar.clone(), *item, *rect);
                self.phase = Phase::Closed;
                self.open(&bar, item, rect, Opened::Hover, &mut effects);
            }
            Phase::Open(open) if open.leave.is_some_and(|at| at <= now) => {
                self.close(now, &mut effects);
            }
            _ => {}
        }
        effects
    }

    fn open(
        &mut self,
        bar: &str,
        item: Item,
        from: Rectangle,
        how: Opened,
        effects: &mut Vec<Effect>,
    ) {
        let Some(place) = self.bars.get(bar).copied() else {
            return;
        };
        let size = (POPOVER_WIDTH.unsigned_abs(), item.height());
        let rect = placed(place, from, size);
        // Its layer-shell margins: from the screen's right edge, past the
        // bar and the gap; from the screen's bottom (the bar's frame starts
        // 8 px down).
        let right = EDGE + place.form.width() + EDGE;
        let bottom = (place.height + 2.0 * EDGE as f32 - (rect.y + rect.height) - EDGE as f32)
            .round() as i32;
        let surface = window::Id::unique();
        tracing::debug!(?item, ?how, bar, "a popover opens");
        effects.push(Effect::Open {
            id: surface,
            bar: bar.to_owned(),
            item,
            size,
            margin: (0, right, bottom, 0),
            keyboard: match how {
                Opened::Click => KeyboardInteractivity::Exclusive,
                Opened::Hover => KeyboardInteractivity::OnDemand,
            },
        });
        effects.push(Effect::Wake(LEVELS_NAMESPACE));
        self.phase = Phase::Open(Open {
            bar: bar.to_owned(),
            item,
            how,
            surface,
            rect,
            from,
            inside: true,
            leave: None,
            last: None,
        });
    }

    fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        let phase = std::mem::replace(&mut self.phase, Phase::Closed);
        let Phase::Open(open) = phase else {
            return;
        };
        tracing::debug!(item = ?open.item, "a popover closes");
        if self.motion {
            effects.push(Effect::Closing);
            self.closing
                .push((open.surface, open.item, now + POPOVER_CLOSE));
        } else {
            effects.push(Effect::Remove(open.surface));
        }
        effects.push(Effect::Wake(LEVELS_NAMESPACE));
    }

    fn unfocus(&mut self, effects: &mut Vec<Effect>) {
        if let Some((bar, _, _)) = self.focused.take()
            && let Some(place) = self.bars.get(&bar)
        {
            effects.push(Effect::Keyboard(place.levels, KeyboardInteractivity::None));
            effects.push(Effect::Wake(LEVELS_NAMESPACE));
        }
    }
}

impl Open {
    /// The pointer left the item or the popover at `at`.
    fn left(&mut self, at: Point, now: Instant) {
        self.inside = false;
        self.last = Some(at);
        if self.how == Opened::Hover {
            self.leave = Some(now + LEAVE_CLOSE);
        }
    }

    /// The pointer seen at `at`, off the item and the popover: moving
    /// toward the popover, it keeps a hovered one open a while longer.
    fn seen(&mut self, at: Point, now: Instant) {
        if self.inside || self.how != Opened::Hover {
            self.last = Some(at);
            return;
        }
        if let Some(from) = self.last
            && toward(from, at, self.rect)
        {
            self.leave = Some(now + LEAVE_CLOSE);
        }
        self.last = Some(at);
    }
}

/// Whether a move from `from` to `to` heads for `popover`, on the bar's
/// left: `to` is within the cone from `from` to the popover's near (right)
/// corners.
pub fn toward(from: Point, to: Point, popover: Rectangle) -> bool {
    let near = popover.x + popover.width;
    let (a, b) = (
        Point::new(near, popover.y),
        Point::new(near, popover.y + popover.height),
    );
    if from == to {
        return false;
    }
    let side = |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2, d3) = (side(from, a, to), side(a, b, to), side(b, from, to));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// Where a popover of `size` opened from `item` on the bar at `place` is,
/// in the bar's frame: 8 px left of the bar, its top on the item's, pushed
/// up to stay 8 px from the screen's bottom (the bar's own bottom), and
/// down to stay 8 px from its top.
pub fn placed(place: Place, item: Rectangle, (w, h): (u32, u32)) -> Rectangle {
    let (w, h) = (w as f32, h as f32);
    let x = -(place.form.width() as f32) - EDGE as f32 - w;
    let y = item.y.min(place.height - h).max(0.0);
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

// --- The one controller, and the surfaces' way to it --------------------------

static CONTROLLER: Mutex<Option<Controller>> = Mutex::new(None);
/// Where effects go: [`effects`]' stream.
static EFFECTS: Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<Effect>>>> = Mutex::new(None);

/// `with` on the controller.
pub fn with<T>(with: impl FnOnce(&mut Controller) -> T) -> T {
    let mut controller = CONTROLLER.lock().unwrap_or_else(PoisonError::into_inner);
    with(controller.get_or_insert_with(Controller::default))
}

/// What a surface saw; the effects go to [`effects`]' stream, and so does
/// a new deadline, which the stream then waits for.
pub fn event(event: Event) {
    let (effects, moved) = with(|c| {
        let before = c.deadline();
        let effects = c.handle(event, Instant::now());
        (effects, c.deadline() != before)
    });
    if !effects.is_empty() || moved {
        send(effects);
    }
}

/// The bar's own surface, under its clock and its levels, for the
/// popovers: where the pointer goes (the cone), and presses off the
/// items. It never draws, nor asks to.
#[derive(Debug, Clone, Copy)]
pub struct BarPointer;

impl<Message> iced::widget::canvas::Program<Message> for BarPointer {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        seen: &iced::Event,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<Message>> {
        use iced::mouse::Event as Mouse;
        match seen {
            iced::Event::Mouse(Mouse::CursorMoved { position }) => {
                let at = Point::new(position.x - bounds.x - bounds.width, position.y - bounds.y);
                event(Event::Pointer { at });
            }
            iced::Event::Mouse(Mouse::ButtonPressed(_)) => event(Event::PressElsewhere),
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        _state: &(),
        _renderer: &iced::Renderer,
        _theme: &iced::Theme,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        Vec::new()
    }
}

fn send(effects: Vec<Effect>) {
    if let Some(sender) = EFFECTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(effects);
    }
}

/// The popover shows something new (the sound changed): it draws again.
pub fn changed() {
    let open = with(|c| matches!(c.phase, Phase::Open(_)));
    if open {
        send(vec![Effect::Wake(POPOVER_NAMESPACE)]);
    }
}

/// Lines to the compositor, once linked; `None`: a shell alone.
static LINK: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<String>>> =
    Mutex::new(None);

pub fn linked(sender: iced::futures::channel::mpsc::UnboundedSender<String>) {
    *LINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
}

/// Tells the compositor; `false` without it.
fn tell(message: ToCompositor) -> bool {
    LINK.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|link| link.unbounded_send(message.to_string()).is_ok())
}

/// Carries out the controller's effects, and its deadlines: layer-shell
/// actions (iced runs them without drawing the other surfaces) and lines
/// to the compositor. A shell alone wakes its surfaces with a message.
pub fn effects() -> impl iced::futures::Stream<Item = crate::Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(16, async |mut output| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        *EFFECTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        loop {
            let deadline = with(|c| c.deadline());
            let effects = match deadline {
                Some(at) => {
                    use iced::futures::future::{Either, select};
                    let due = std::pin::pin!(tokio::time::sleep_until(at.into()));
                    let next = std::pin::pin!(receiver.recv());
                    match select(next, due).await {
                        Either::Left((effects, _)) => effects,
                        Either::Right(((), _)) => Some(with(|c| c.tick(Instant::now()))),
                    }
                }
                None => receiver.recv().await,
            };
            let Some(effects) = effects else {
                return;
            };
            for message in effects.into_iter().filter_map(carry_out) {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

/// An effect as a message for iced, or a line to the compositor.
fn carry_out(effect: Effect) -> Option<crate::Message> {
    use crate::Message;
    use iced_layershell::reexport::{Anchor, Layer, NewLayerShellSettings, OutputOption};
    match effect {
        Effect::Open {
            id,
            bar,
            size,
            margin,
            keyboard,
            ..
        } => Some(Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Top,
                anchor: Anchor::Right | Anchor::Bottom,
                exclusive_zone: Some(0),
                margin: Some(margin),
                keyboard_interactivity: keyboard,
                output_option: if bar == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(bar)
                },
                events_transparent: false,
                namespace: Some(POPOVER_NAMESPACE.to_owned()),
            },
            id,
        }),
        Effect::Keyboard(id, keyboard_interactivity) => {
            Some(Message::KeyboardInteractivityChange {
                id,
                keyboard_interactivity,
            })
        }
        Effect::Closing => {
            tell(ToCompositor::Closing(POPOVER_NAMESPACE.to_owned()));
            None
        }
        Effect::Remove(id) => Some(Message::RemoveWindow(id)),
        Effect::Wake(namespace) => {
            if tell(ToCompositor::Wake(namespace.to_owned())) {
                None
            } else {
                Some(Message::Levels)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "HDMI-A-1";
    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn controller(form: Form) -> (Controller, window::Id) {
        let mut c = Controller::default();
        let levels = window::Id::unique();
        c.place(BAR, form, window::Id::unique(), levels);
        (c, levels)
    }

    fn gauge() -> Rectangle {
        Rectangle::new(Point::new(-50.0, 900.0), Size::new(44.0, 64.0))
    }

    fn enter() -> Event {
        Event::ItemEnter {
            bar: BAR.to_owned(),
            item: Item::Sound,
            rect: gauge(),
        }
    }

    fn press() -> Event {
        Event::ItemPress {
            bar: BAR.to_owned(),
            item: Item::Sound,
            rect: gauge(),
        }
    }

    fn opened(effects: &[Effect]) -> Option<(window::Id, KeyboardInteractivity)> {
        effects.iter().find_map(|e| match e {
            Effect::Open { id, keyboard, .. } => Some((*id, *keyboard)),
            _ => None,
        })
    }

    #[test]
    fn a_click_opens_it_and_it_stays_until_a_click_outside_or_another_on_its_item() {
        let (mut c, _) = controller(Form::Full);
        let t = Instant::now();
        let effects = c.handle(press(), t);
        let (id, keyboard) = opened(&effects).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::Exclusive);
        assert!(effects.contains(&Effect::Wake(LEVELS_NAMESPACE)));
        assert!(c.shows(BAR, Item::Sound));
        // The pointer goes away for good: it stays.
        c.handle(
            Event::ItemLeave {
                at: Point::new(-300.0, 100.0),
            },
            t,
        );
        c.handle(
            Event::Pointer {
                at: Point::new(-900.0, 10.0),
            },
            t + MS(10),
        );
        assert_eq!(c.deadline(), None);
        assert!(c.tick(t + Duration::from_secs(10)).is_empty());
        assert!(c.shows(BAR, Item::Sound));
        // A click outside: it fades out, and goes 120 ms later.
        let effects = c.handle(Event::Outside, t + MS(20));
        assert_eq!(
            effects,
            vec![Effect::Closing, Effect::Wake(LEVELS_NAMESPACE)]
        );
        assert!(!c.shows(BAR, Item::Sound));
        assert_eq!(c.deadline(), Some(t + MS(140)));
        assert_eq!(c.tick(t + MS(139)), vec![]);
        assert_eq!(c.tick(t + MS(140)), vec![Effect::Remove(id)]);
        // Again, closed by a second click on its item; without motion, it
        // goes at once.
        c.motion = false;
        let (id, _) = opened(&c.handle(press(), t + MS(200))).expect("opened");
        let effects = c.handle(press(), t + MS(300));
        assert_eq!(
            effects,
            vec![Effect::Remove(id), Effect::Wake(LEVELS_NAMESPACE)]
        );
        // And by a press elsewhere on the bar.
        c.handle(press(), t + MS(400));
        c.handle(Event::PressElsewhere, t + MS(500));
        assert!(!c.shows(BAR, Item::Sound));
    }

    #[test]
    fn esc_closes_it_and_gives_the_keyboard_to_its_item() {
        let (mut c, levels) = controller(Form::Full);
        let t = Instant::now();
        c.motion = false;
        let (id, _) = opened(&c.handle(press(), t)).expect("opened");
        let effects = c.handle(Event::Escape, t);
        assert!(effects.contains(&Effect::Remove(id)));
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::Exclusive)));
        assert!(c.focused(BAR, Item::Sound));
        // Enter opens it again, taking the keyboard back from the item.
        let effects = c.handle(Event::ItemActivate, t);
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::None)));
        assert!(opened(&effects).is_some());
        assert!(!c.focused(BAR, Item::Sound));
        // Esc, then Esc on the item: the keyboard goes back.
        c.handle(Event::Escape, t);
        let effects = c.handle(Event::ItemRelease, t);
        assert_eq!(
            effects,
            vec![
                Effect::Keyboard(levels, KeyboardInteractivity::None),
                Effect::Wake(LEVELS_NAMESPACE)
            ]
        );
        // A press outside lets it go too.
        c.handle(press(), t);
        c.handle(Event::Escape, t);
        assert!(c.focused(BAR, Item::Sound));
        c.handle(Event::Outside, t);
        assert!(!c.focused(BAR, Item::Sound));
    }

    #[test]
    fn hovering_the_mini_gauge_opens_it_after_150_ms_and_leaving_closes_it_300_ms_after() {
        let (mut c, _) = controller(Form::Mini);
        let t = Instant::now();
        // Passing over the gauge opens nothing.
        c.handle(enter(), t);
        c.handle(
            Event::ItemLeave {
                at: Point::new(-60.0, 920.0),
            },
            t + MS(100),
        );
        assert!(c.tick(t + MS(200)).is_empty());
        assert!(!c.shows(BAR, Item::Sound));
        // Resting on it 150 ms does, without the keyboard.
        c.handle(enter(), t + MS(1000));
        assert_eq!(c.deadline(), Some(t + MS(1150)));
        assert!(c.tick(t + MS(1149)).is_empty());
        let (_, keyboard) = opened(&c.tick(t + MS(1150))).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::OnDemand);
        // The pointer leaves for the windows: closed 300 ms later.
        c.handle(
            Event::ItemLeave {
                at: Point::new(-56.0, 1000.0),
            },
            t + MS(1500),
        );
        c.handle(
            Event::Pointer {
                at: Point::new(-56.0, 1040.0),
            },
            t + MS(1510),
        );
        assert!(c.tick(t + MS(1799)).is_empty());
        assert!(c.shows(BAR, Item::Sound));
        assert!(c.tick(t + MS(1800)).contains(&Effect::Closing));
        assert!(!c.shows(BAR, Item::Sound));
        // The full form does not open on hover.
        let (mut c, _) = controller(Form::Full);
        c.handle(enter(), t);
        assert!(c.tick(t + MS(500)).is_empty());
        assert!(!c.shows(BAR, Item::Sound));
    }

    #[test]
    fn moving_toward_a_hovered_popover_keeps_it_open() {
        let (mut c, _) = controller(Form::Mini);
        let t = Instant::now();
        c.handle(enter(), t);
        c.tick(t + MS(150));
        // The popover: 8 px left of the 56 px bar, bottom on the bar's.
        let popover = match &c.phase {
            Phase::Open(open) => open.rect,
            other => panic!("{other:?}"),
        };
        assert_eq!(popover.x + popover.width, -64.0);
        assert!(popover.y <= 900.0 && popover.y + popover.height <= 1064.0);
        // Out of the gauge's left side, slowly, a little up: every move
        // within the cone keeps it 300 ms more.
        c.handle(
            Event::ItemLeave {
                at: Point::new(-50.0, 920.0),
            },
            t + MS(200),
        );
        let mut now = t + MS(200);
        for step in 1..=6 {
            now += MS(250);
            let at = Point::new(-50.0 - 2.0 * step as f32, 920.0 - step as f32);
            c.handle(Event::Pointer { at }, now);
            assert!(c.tick(now).is_empty(), "closed at step {step}");
        }
        // Then it reaches the popover: no deadline.
        c.handle(Event::PopoverEnter, now + MS(100));
        assert_eq!(c.deadline(), None);
        // Leaving the popover away from it closes it 300 ms later, even
        // when the pointer goes on moving (away).
        c.handle(Event::PopoverLeave, now + MS(200));
        c.handle(
            Event::Pointer {
                at: Point::new(-62.0, 700.0),
            },
            now + MS(250),
        );
        c.handle(
            Event::Pointer {
                at: Point::new(-60.0, 650.0),
            },
            now + MS(400),
        );
        assert!(c.tick(now + MS(500)).contains(&Effect::Closing));
    }

    #[test]
    fn a_click_on_a_hovered_popovers_item_keeps_it_open() {
        let (mut c, _) = controller(Form::Mini);
        let t = Instant::now();
        c.handle(enter(), t);
        let (id, _) = opened(&c.tick(t + MS(150))).expect("opened");
        let effects = c.handle(press(), t + MS(300));
        assert_eq!(
            effects,
            vec![Effect::Keyboard(id, KeyboardInteractivity::Exclusive)]
        );
        c.handle(
            Event::ItemLeave {
                at: Point::new(-300.0, 0.0),
            },
            t + MS(400),
        );
        assert!(c.tick(t + Duration::from_secs(5)).is_empty());
        assert!(c.shows(BAR, Item::Sound));
        // A click before the 150 ms opens it at once, as a click.
        let (mut c, _) = controller(Form::Mini);
        c.handle(enter(), t);
        let (_, keyboard) = opened(&c.handle(press(), t + MS(50))).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::Exclusive);
        assert!(c.tick(t + MS(150)).is_empty());
    }

    #[test]
    fn a_form_switch_closes_it() {
        let (mut c, _) = controller(Form::Full);
        let t = Instant::now();
        c.handle(press(), t);
        c.handle(
            Event::BarChanged {
                bar: "other".to_owned(),
            },
            t,
        );
        assert!(c.shows(BAR, Item::Sound));
        c.handle(
            Event::BarChanged {
                bar: BAR.to_owned(),
            },
            t,
        );
        assert!(!c.shows(BAR, Item::Sound));
    }

    #[test]
    fn the_cone_reaches_the_popovers_near_corners() {
        let popover = Rectangle::new(Point::new(-344.0, 800.0), Size::new(280.0, 264.0));
        let from = Point::new(-40.0, 1000.0);
        // Left and a little up: in. Straight up, or right: out.
        assert!(toward(from, Point::new(-50.0, 995.0), popover));
        assert!(toward(from, Point::new(-50.0, 960.0), popover));
        assert!(!toward(from, Point::new(-40.0, 950.0), popover));
        assert!(!toward(from, Point::new(-30.0, 1000.0), popover));
        // Up beyond its top corner's line: out.
        assert!(!toward(from, Point::new(-45.0, 900.0), popover));
        assert!(!toward(from, from, popover));
    }

    #[test]
    fn a_popover_sits_beside_its_item_and_inside_the_screen() {
        let place = Place {
            form: Form::Full,
            height: 1064.0,
            levels: window::Id::unique(),
            surface: window::Id::unique(),
        };
        // At the bar's foot: bottom-aligned, 8 px from the screen's bottom.
        let item = Rectangle::new(Point::new(-190.0, 900.0), Size::new(184.0, 30.0));
        let rect = placed(place, item, (280, 261));
        assert_eq!((rect.x, rect.y, rect.width), (-484.0, 803.0, 280.0));
        // Higher up: its top on the item's.
        let item = Rectangle::new(Point::new(-190.0, 300.0), Size::new(184.0, 30.0));
        assert_eq!(placed(place, item, (280, 261)).y, 300.0);
        // Taller than the bar: from the top.
        assert_eq!(placed(place, item, (280, 2000)).y, 0.0);
    }
}
