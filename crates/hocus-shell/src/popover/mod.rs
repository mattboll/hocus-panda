//! Popovers beside the bar (bar.md §9): what opens them, what closes them,
//! one at a time. The sound's ([`sound`]), the Wi-Fi's ([`wifi`]),
//! Bluetooth's ([`bluetooth`]), the brightness's ([`brightness`]) and the
//! battery's ([`battery`]), drawn and handled with the pieces of [`kit`].
//!
//! - **By a click** on its item (the whole row in the full form, the gauge
//!   in the mini): it stays open, whatever the pointer does, until a click
//!   outside it, `Esc`, or another click on the item. It takes the
//!   keyboard: `Esc` closes it and gives the keyboard to the item, where
//!   `Enter` or `Space` opens it again and `Esc` lets the keyboard go.
//! - **The keyboard** on an item (after `Esc` in its popover): `Tab` and
//!   `Shift+Tab` (or the arrows up and down) take it to the next and the
//!   previous item of the same surface, round its ends.
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

pub mod battery;
pub mod bluetooth;
pub mod brightness;
pub mod kit;
pub mod sound;
pub mod wifi;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    CONNECTIONS_NAMESPACE, EDGE, Form, LEVELS_NAMESPACE, POPOVER_CLOSE, POPOVER_NAMESPACE,
    POPOVER_WIDTH, ToCompositor,
};
use iced::{Point, Rectangle, Size, window};
use iced_layershell::reexport::KeyboardInteractivity;

/// Hovering a mini form's item this long opens its popover.
pub const HOVER_OPEN: Duration = Duration::from_millis(150);
/// A popover opened by hovering closes this long after the pointer left.
pub const LEAVE_CLOSE: Duration = Duration::from_millis(300);

/// What opens a popover, and the keyboard layout's item, which opens none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Sound,
    Wifi,
    Bluetooth,
    Brightness,
    /// The battery and the power modes (the modes alone without a
    /// battery).
    Battery,
    /// The keyboard's layout: a click, or `Enter` while it holds the
    /// keyboard, asks for the next one (3.8).
    Keyboard,
}

impl Item {
    /// Whether it opens a popover.
    fn has_popover(self) -> bool {
        self != Self::Keyboard
    }

    /// Its popover's height, logical pixels, for what it shows now: as it
    /// opens (`opening`), or after a change.
    fn height(self, opening: bool) -> u32 {
        match self {
            Self::Sound => sound::layout(opening).height(),
            Self::Wifi => wifi::height(),
            Self::Bluetooth => bluetooth::height(),
            Self::Brightness => brightness::height(),
            Self::Battery => battery::height(),
            Self::Keyboard => 0,
        }
    }

    /// Its popover opens: what it shows starts afresh.
    fn opening(self) {
        match self {
            Self::Sound => {}
            Self::Wifi => wifi::opening(),
            Self::Bluetooth => bluetooth::opening(),
            Self::Brightness => brightness::opening(),
            Self::Battery => battery::opening(),
            Self::Keyboard => {}
        }
    }

    /// Its popover closes: what it kept goes (a password typed).
    fn closing(self) {
        match self {
            Self::Sound | Self::Brightness | Self::Battery | Self::Keyboard => {}
            Self::Wifi => wifi::closing(),
            Self::Bluetooth => bluetooth::closing(),
        }
    }

    /// The namespace of the surface it is on in `form`, drawn again as
    /// its popover opens and closes: the keyboard layout's is the
    /// connections' row in the full form, the levels' button in the mini.
    fn namespace(self, form: Form) -> &'static str {
        match (self, form) {
            (Self::Sound | Self::Brightness | Self::Battery, _) | (Self::Keyboard, Form::Mini) => {
                LEVELS_NAMESPACE
            }
            (Self::Wifi | Self::Bluetooth, _) | (Self::Keyboard, Form::Full) => {
                CONNECTIONS_NAMESPACE
            }
        }
    }

    /// Whether its popover's bottom sits on the bar's whatever its height,
    /// as the 3.5, 3.6 and 3.7 mockups draw the Wi-Fi's, Bluetooth's, the
    /// brightness's and the battery's (else its top is on the item's,
    /// pushed up to stay inside the bar).
    fn on_bottom(self) -> bool {
        !matches!(self, Self::Sound)
    }

    /// Its surface on the bar at `place`, which holds the keyboard after
    /// `Esc` in its popover.
    fn surface(self, place: &Place) -> window::Id {
        match self.namespace(place.form) {
            LEVELS_NAMESPACE => place.levels,
            _ => place.connections,
        }
    }
}

/// How a popover opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Click,
    Hover,
}

/// A bar, as the popovers need it: its form, its height, and its items'
/// surfaces (which hold the keyboard after `Esc`). Points are in its frame:
/// from its outer right edge (negative inside it) and from its top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub form: Form,
    pub height: f32,
    pub levels: window::Id,
    pub connections: window::Id,
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
    /// The popover, opened by hovering, is now worked in (a password to
    /// type): it stays and takes the keyboard, as if a click had opened it.
    Hold,
    /// `Enter` or `Space` on the item holding the keyboard.
    ItemActivate,
    /// `Tab` (or down) on the item holding the keyboard: to the next item
    /// of its surface; `back`, `Shift+Tab` (or up): to the previous.
    ItemStep {
        back: bool,
    },
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
    /// The popover's surface to a new size, and its new margins if they
    /// change.
    Resize {
        id: window::Id,
        size: (u32, u32),
        margin: Option<(i32, i32, i32, i32)>,
    },
    /// The compositor fades it out (`closing`).
    Closing,
    Remove(window::Id),
    /// The surfaces of this namespace show something new.
    Wake(&'static str),
    /// Surface `id` takes the pointer (`takes`), or lets it through.
    Input {
        id: window::Id,
        takes: bool,
    },
    /// The compositor is asked for the next keyboard layout.
    NextLayout,
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
    /// The items each bar's surfaces show, by namespace, from the top, and
    /// where (the bar's frame): the keyboard steps through them.
    items: HashMap<(String, &'static str), Vec<(Item, Rectangle)>>,
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
            items: HashMap::new(),
            motion: true,
        }
    }
}

impl Controller {
    /// Bar `bar` in `form`, its surfaces `surface`, `levels` and
    /// `connections`; its height stays as known.
    pub fn place(
        &mut self,
        bar: &str,
        form: Form,
        surface: window::Id,
        (levels, connections): (window::Id, window::Id),
    ) {
        let height = self.bars.get(bar).map_or(HEIGHT, |p| p.height);
        let place = Place {
            form,
            height,
            levels,
            connections,
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
        self.items.retain(|(b, _), _| b != bar);
    }

    /// The forms the bars are in.
    pub fn forms(&self) -> Vec<Form> {
        self.bars.values().map(|p| p.form).collect()
    }

    /// Bar `bar`'s surface of `namespace` shows `items` (in the bar's
    /// frame).
    pub fn set_items(
        &mut self,
        bar: &str,
        namespace: &'static str,
        mut items: Vec<(Item, Rectangle)>,
    ) {
        items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        self.items.insert((bar.to_owned(), namespace), items);
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

    /// The item whose popover is open, if one is.
    pub fn open_item(&self) -> Option<Item> {
        match &self.phase {
            Phase::Open(open) => Some(open.item),
            _ => None,
        }
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
                Phase::Closed
                    if item.has_popover()
                        && self.bars.get(&bar).map(|p| p.form) == Some(Form::Mini) =>
                {
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
            Event::ItemPress { item, .. } if !item.has_popover() => {
                self.unfocus(&mut effects);
                self.close(now, &mut effects);
                effects.push(Effect::NextLayout);
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
                            item.surface(place),
                            KeyboardInteractivity::Exclusive,
                        ));
                        self.focused = Some((bar, item, from));
                    }
                }
            }
            Event::Hold => {
                if let Phase::Open(open) = &mut self.phase
                    && open.how == Opened::Hover
                {
                    open.how = Opened::Click;
                    open.leave = None;
                    effects.push(Effect::Keyboard(
                        open.surface,
                        KeyboardInteractivity::Exclusive,
                    ));
                }
            }
            Event::ItemActivate => match self.focused.clone() {
                Some((_, item, _)) if !item.has_popover() => effects.push(Effect::NextLayout),
                Some((bar, item, from)) => {
                    self.unfocus(&mut effects);
                    self.open(&bar, item, from, Opened::Click, &mut effects);
                }
                None => {}
            },
            Event::ItemStep { back } => self.step(back, &mut effects),
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
        item.opening();
        let size = (POPOVER_WIDTH.unsigned_abs(), item.height(true));
        let rect = placed(place, from, size, item.on_bottom());
        let margin = margins(place, rect);
        let surface = window::Id::unique();
        tracing::debug!(?item, ?how, bar, "a popover opens");
        effects.push(Effect::Open {
            id: surface,
            bar: bar.to_owned(),
            item,
            size,
            margin,
            keyboard: match how {
                Opened::Click => KeyboardInteractivity::Exclusive,
                Opened::Hover => KeyboardInteractivity::OnDemand,
            },
        });
        effects.push(Effect::Wake(item.namespace(place.form)));
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

    /// What the open popover shows changed: to its new size, if it
    /// changed, still beside its item and within the bar.
    pub fn refit(&mut self) -> Vec<Effect> {
        let Phase::Open(open) = &mut self.phase else {
            return Vec::new();
        };
        let height = open.item.height(false);
        let Some(place) = self.bars.get(&open.bar).copied() else {
            return Vec::new();
        };
        if open.rect.height.round() as u32 == height {
            return Vec::new();
        }
        let size = (POPOVER_WIDTH.unsigned_abs(), height);
        let before = margins(place, open.rect);
        open.rect = placed(place, open.from, size, open.item.on_bottom());
        let margin = margins(place, open.rect);
        tracing::debug!(height, "the popover's size changes");
        vec![Effect::Resize {
            id: open.surface,
            size,
            margin: (margin != before).then_some(margin),
        }]
    }

    fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        let phase = std::mem::replace(&mut self.phase, Phase::Closed);
        let Phase::Open(open) = phase else {
            return;
        };
        tracing::debug!(item = ?open.item, "a popover closes");
        open.item.closing();
        // What the clipboard said while it held the keyboard.
        crate::paste::tidy();
        if self.motion {
            effects.push(Effect::Closing);
            self.closing
                .push((open.surface, open.item, now + POPOVER_CLOSE));
        } else {
            effects.push(Effect::Remove(open.surface));
        }
        let form = self.bars.get(&open.bar).map_or(Form::Full, |p| p.form);
        effects.push(Effect::Wake(open.item.namespace(form)));
    }

    fn unfocus(&mut self, effects: &mut Vec<Effect>) {
        if let Some((bar, item, _)) = self.focused.take()
            && let Some(place) = self.bars.get(&bar)
        {
            effects.push(Effect::Keyboard(
                item.surface(place),
                KeyboardInteractivity::None,
            ));
            effects.push(Effect::Wake(item.namespace(place.form)));
        }
    }

    /// The keyboard from the item holding it to the next item of its
    /// surface (the previous, `back`), round the ends: the same surface
    /// keeps it.
    fn step(&mut self, back: bool, effects: &mut Vec<Effect>) {
        let Some((bar, item, _)) = self.focused.clone() else {
            return;
        };
        let Some(place) = self.bars.get(&bar) else {
            return;
        };
        let namespace = item.namespace(place.form);
        let Some(items) = self.items.get(&(bar.clone(), namespace)) else {
            return;
        };
        let Some(at) = items.iter().position(|(i, _)| *i == item) else {
            return;
        };
        let n = items.len();
        let next = if back { (at + n - 1) % n } else { (at + 1) % n };
        let (item, rect) = items[next];
        self.focused = Some((bar, item, rect));
        effects.push(Effect::Wake(namespace));
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

/// A popover's layer-shell margins at `rect` (the bar's frame) on the bar
/// at `place`: from the screen's right edge, past the bar and the gap;
/// from the screen's bottom (the bar's frame starts 8 px down).
fn margins(place: Place, rect: Rectangle) -> (i32, i32, i32, i32) {
    let right = EDGE + place.form.width() + EDGE;
    let bottom =
        (place.height + 2.0 * EDGE as f32 - (rect.y + rect.height) - EDGE as f32).round() as i32;
    (0, right, bottom, 0)
}

/// Where a popover of `size` opened from `item` on the bar at `place` is,
/// in the bar's frame: 8 px left of the bar, its top on the item's (or its
/// bottom on the bar's, `on_bottom`), pushed up to stay 8 px from the
/// screen's bottom (the bar's own bottom), and down to stay 8 px from its
/// top.
pub fn placed(place: Place, item: Rectangle, (w, h): (u32, u32), on_bottom: bool) -> Rectangle {
    let (w, h) = (w as f32, h as f32);
    let x = -(place.form.width() as f32) - EDGE as f32 - w;
    let top = if on_bottom { place.height } else { item.y };
    let y = top.min(place.height - h).max(0.0);
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

/// A key pressed: the key, the modifiers held, and the text it typed, as
/// the keymap and the modifiers the compositor gives say (the user's
/// layout, AltGr, dead keys composed): never made again from the key here.
#[derive(Clone)]
pub struct Pressed {
    pub key: iced::keyboard::Key,
    pub modifiers: iced::keyboard::Modifiers,
    pub text: Option<String>,
}

impl std::fmt::Debug for Pressed {
    // What is typed may be a password: never shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pressed").finish_non_exhaustive()
    }
}

/// Keys for the popover that reached another of the shell's surfaces.
static KEYS: Mutex<Vec<Pressed>> = Mutex::new(Vec::new());

/// A key pressed on one of the shell's surfaces other than the popover,
/// with the `modifiers` held and the `text` it typed, for what holds the
/// keyboard. layershellev hands keys to the surface last clicked, not to
/// the one the keyboard entered: a click on an item whose release comes
/// after its popover took the keyboard sends the keys to the item's
/// surface. So the popover's keys go to it (woken to take them), and the
/// item's are its own. Whether the key was taken.
pub fn key(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    text: Option<&str>,
) -> bool {
    use iced::keyboard::{Key, key::Named};
    let (menu, focused) = with(|c| {
        let menu = matches!(&c.phase, Phase::Open(open) if open.how == Opened::Click);
        (menu, c.focused.is_some())
    });
    if menu {
        KEYS.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Pressed {
                key: key.clone(),
                modifiers,
                text: text.map(str::to_owned),
            });
        send(vec![Effect::Wake(POPOVER_NAMESPACE)]);
        return true;
    }
    if !focused {
        return false;
    }
    // The item holding the keyboard: Enter or Space opens its popover
    // again (the layout's goes to the next layout), Esc lets the keyboard
    // go, Tab and the arrows go through its surface's items.
    match key {
        Key::Named(Named::Enter | Named::Space) => event(Event::ItemActivate),
        Key::Named(Named::Escape) => event(Event::ItemRelease),
        Key::Named(Named::Tab) => event(Event::ItemStep {
            back: modifiers.shift(),
        }),
        Key::Named(Named::ArrowDown) => event(Event::ItemStep { back: false }),
        Key::Named(Named::ArrowUp) => event(Event::ItemStep { back: true }),
        _ => return false,
    }
    true
}

/// The keys the popover was sent elsewhere.
pub fn take_keys() -> Vec<Pressed> {
    std::mem::take(&mut *KEYS.lock().unwrap_or_else(PoisonError::into_inner))
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
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: k,
                modifiers,
                text,
                ..
            }) => {
                key(k, *modifiers, text.as_deref());
            }
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

/// Effects to carry out, from outside the controller (a surface moving).
pub fn send(effects: Vec<Effect>) {
    if let Some(sender) = EFFECTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(effects);
    }
}

/// What `item`'s popover shows changed: if it is open, it draws again, to
/// its new size if that changed.
pub fn changed(item: Item) {
    let (open, mut effects) = with(|c| {
        let open = c.open_item() == Some(item);
        (open, if open { c.refit() } else { Vec::new() })
    });
    if open {
        effects.push(Effect::Wake(POPOVER_NAMESPACE));
        send(effects);
    }
}

/// `item`'s popover, as it draws, finds it needs another size: resized,
/// without being woken again.
pub fn refit(item: Item) {
    let effects = with(|c| {
        if c.open_item() == Some(item) {
            c.refit()
        } else {
            Vec::new()
        }
    });
    if !effects.is_empty() {
        send(effects);
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
            for message in effects.into_iter().flat_map(carry_out) {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: i32 = 1 << 20;

type WlRegion = wayland_client::protocol::wl_region::WlRegion;

/// An effect as messages for iced, or a line to the compositor.
fn carry_out(effect: Effect) -> Vec<crate::Message> {
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
        } => vec![Message::NewLayerShell {
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
        }],
        Effect::Keyboard(id, keyboard_interactivity) => {
            vec![Message::KeyboardInteractivityChange {
                id,
                keyboard_interactivity,
            }]
        }
        Effect::Resize { id, size, margin } => std::iter::once(Message::SizeChange { id, size })
            .chain(margin.map(|margin| Message::MarginChange { id, margin }))
            .collect(),
        Effect::Closing => {
            tell(ToCompositor::Closing(POPOVER_NAMESPACE.to_owned()));
            Vec::new()
        }
        Effect::Remove(id) => vec![Message::RemoveWindow(id)],
        Effect::Wake(namespace) => {
            if tell(ToCompositor::Wake(namespace.to_owned())) {
                Vec::new()
            } else {
                vec![Message::Levels]
            }
        }
        Effect::Input { id, takes } => {
            // Whatever region it had (each bar's surfaces share one): all
            // of it, or none.
            let callback =
                iced_layershell::actions::ActionCallback::new(move |region: &WlRegion| {
                    region.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
                    if takes {
                        region.add(0, 0, EVERYWHERE, EVERYWHERE);
                    }
                });
            vec![Message::SetInputRegion { id, callback }]
        }
        Effect::NextLayout => {
            tell(ToCompositor::NextLayout);
            Vec::new()
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
        c.place(
            BAR,
            form,
            window::Id::unique(),
            (levels, window::Id::unique()),
        );
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
    fn the_wifis_popover_gives_the_keyboard_back_to_its_row_and_holds_when_worked_in() {
        let mut c = Controller::default();
        let (levels, connections) = (window::Id::unique(), window::Id::unique());
        c.place(BAR, Form::Mini, window::Id::unique(), (levels, connections));
        c.motion = false;
        let t = Instant::now();
        let wifi = Event::ItemPress {
            bar: BAR.to_owned(),
            item: Item::Wifi,
            rect: gauge(),
        };
        let effects = c.handle(wifi, t);
        // Its row's surface is drawn again, not the levels'.
        assert!(effects.contains(&Effect::Wake(CONNECTIONS_NAMESPACE)));
        assert!(!effects.contains(&Effect::Wake(LEVELS_NAMESPACE)));
        let effects = c.handle(Event::Escape, t);
        assert!(effects.contains(&Effect::Keyboard(
            connections,
            KeyboardInteractivity::Exclusive
        )));
        assert!(c.focused(BAR, Item::Wifi));
        // Opened by hovering, then a password to type: it stays, and takes
        // the keyboard.
        c.handle(Event::ItemRelease, t);
        c.handle(
            Event::ItemEnter {
                bar: BAR.to_owned(),
                item: Item::Wifi,
                rect: gauge(),
            },
            t,
        );
        let (id, keyboard) = opened(&c.tick(t + MS(150))).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::OnDemand);
        assert_eq!(
            c.handle(Event::Hold, t + MS(200)),
            vec![Effect::Keyboard(id, KeyboardInteractivity::Exclusive)]
        );
        c.handle(
            Event::ItemLeave {
                at: Point::new(-300.0, 0.0),
            },
            t + MS(300),
        );
        assert!(c.tick(t + Duration::from_secs(5)).is_empty());
        assert!(c.shows(BAR, Item::Wifi));
        // Held already: nothing more.
        assert!(c.handle(Event::Hold, t + MS(400)).is_empty());
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
            connections: window::Id::unique(),
            surface: window::Id::unique(),
        };
        // At the bar's foot: bottom-aligned, 8 px from the screen's bottom.
        let item = Rectangle::new(Point::new(-190.0, 900.0), Size::new(184.0, 30.0));
        let rect = placed(place, item, (280, 261), false);
        assert_eq!((rect.x, rect.y, rect.width), (-484.0, 803.0, 280.0));
        // Higher up: its top on the item's.
        let item = Rectangle::new(Point::new(-190.0, 300.0), Size::new(184.0, 30.0));
        assert_eq!(placed(place, item, (280, 261), false).y, 300.0);
        // Taller than the bar: from the top.
        assert_eq!(placed(place, item, (280, 2000), false).y, 0.0);
        // The Wi-Fi's: its bottom on the bar's, short as it may be.
        let short = placed(place, item, (280, 90), true);
        assert_eq!(short.y + short.height, 1064.0);
    }

    #[test]
    fn a_popover_growing_at_the_bars_foot_keeps_its_bottom_and_margins() {
        let place = Place {
            form: Form::Full,
            height: 1064.0,
            levels: window::Id::unique(),
            connections: window::Id::unique(),
            surface: window::Id::unique(),
        };
        let item = Rectangle::new(Point::new(-190.0, 900.0), Size::new(184.0, 30.0));
        let small = placed(place, item, (280, 332), false);
        let tall = placed(place, item, (280, 568), false);
        assert_eq!(small.y + small.height, tall.y + tall.height);
        // 8 px left of the 196 px bar, 8 px from the screen's bottom.
        assert_eq!(margins(place, small), (0, 212, 8, 0));
        assert_eq!(margins(place, tall), margins(place, small));
        // An open popover whose contents did not change is not resized.
        let mut c = Controller::default();
        c.place(
            BAR,
            Form::Full,
            window::Id::unique(),
            (window::Id::unique(), window::Id::unique()),
        );
        c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Sound,
                rect: item,
            },
            Instant::now(),
        );
        assert!(c.refit().is_empty());
    }

    #[test]
    fn the_layouts_item_asks_for_the_next_layout_and_opens_nothing() {
        let (mut c, levels) = controller(Form::Mini);
        let t = Instant::now();
        c.motion = false;
        let key = Rectangle::new(Point::new(-50.0, 1010.0), Size::new(44.0, 30.0));
        let at = |item| Event::ItemPress {
            bar: BAR.to_owned(),
            item,
            rect: key,
        };
        // Hovered in the mini form: nothing opens.
        c.handle(
            Event::ItemEnter {
                bar: BAR.to_owned(),
                item: Item::Keyboard,
                rect: key,
            },
            t,
        );
        assert_eq!(c.deadline(), None);
        assert!(c.tick(t + MS(500)).is_empty());
        // A click: the next layout, and an open popover closes.
        c.handle(press(), t);
        let effects = c.handle(at(Item::Keyboard), t);
        assert!(effects.contains(&Effect::NextLayout));
        assert!(opened(&effects).is_none());
        assert_eq!(c.open_item(), None);
        // The keyboard on the sound's gauge (Esc in its popover), then Tab
        // through the surface's items, round its ends; Enter on the
        // layout's: the next layout, the keyboard kept.
        c.set_items(
            BAR,
            LEVELS_NAMESPACE,
            vec![
                (Item::Keyboard, key),
                (Item::Battery, gauge()),
                (
                    Item::Sound,
                    Rectangle::new(Point::new(-50.0, 850.0), Size::new(44.0, 64.0)),
                ),
            ],
        );
        c.handle(press(), t);
        c.handle(Event::Escape, t);
        assert!(c.focused(BAR, Item::Sound));
        let effects = c.handle(Event::ItemStep { back: false }, t);
        assert_eq!(effects, vec![Effect::Wake(LEVELS_NAMESPACE)]);
        assert!(c.focused(BAR, Item::Battery));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Keyboard));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Sound));
        c.handle(Event::ItemStep { back: true }, t);
        assert!(c.focused(BAR, Item::Keyboard));
        let effects = c.handle(Event::ItemActivate, t);
        assert_eq!(effects, vec![Effect::NextLayout]);
        assert!(c.focused(BAR, Item::Keyboard));
        // Esc lets it go, from the levels' surface.
        let effects = c.handle(Event::ItemRelease, t);
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::None)));
        // In the full form the layout's row is the connections'.
        assert_eq!(Item::Keyboard.namespace(Form::Full), CONNECTIONS_NAMESPACE);
        assert_eq!(Item::Keyboard.namespace(Form::Mini), LEVELS_NAMESPACE);
    }
}
