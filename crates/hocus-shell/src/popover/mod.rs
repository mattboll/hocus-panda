//! Popovers beside the bar (bar.md §9): what opens them, what closes them,
//! one at a time. The sound's ([`sound`]), the Wi-Fi's ([`wifi`]),
//! Bluetooth's ([`bluetooth`]), the brightness's ([`brightness`]), the
//! battery's ([`battery`]), a desktop's list of windows ([`desktop`]), the
//! agents' tools' ([`agents`]), the secretary's briefing ([`briefing`]),
//! the notifications' history ([`history`]), and the footer's clipboard's
//! history ([`clipboard`], which `Super+V` opens too) and user's
//! ([`session`], the session menu, 7.4), drawn and handled with the pieces of [`kit`].
//!
//! - **By a click** on its item (the whole row in the full form, the gauge
//!   in the mini): it stays open, whatever the pointer does, until a click
//!   outside it, `Esc`, or another click on the item. It takes the
//!   keyboard: `Esc` closes it and gives the keyboard to the item, where
//!   `Enter` or `Space` opens it again and `Esc` lets the keyboard go.
//! - **The keyboard** on an item (after `Esc` in its popover): `Tab` and
//!   `Shift+Tab` (or the arrows up and down) take it to the next and the
//!   previous item of the bar, from its top (the desktops' tiles) to its
//!   bottom (the levels), round its ends, from one surface to the next.
//! - **By hovering** the item for 150 ms in the mini form (the footer's
//!   clipboard, bell and user in both forms; its menu never): it closes 300 ms
//!   after the pointer has really left the item and the popover, and stays
//!   while the pointer moves toward it, within the cone from where the
//!   pointer was to the popover's near corners. A click on the item then
//!   keeps it open, as a click would have opened it.
//! - **A desktop's tile** (or dot) opens its list by hovering only, in both
//!   forms; a click on it goes to its desktop and closes the list. The
//!   list open, another tile hovered shows that one's at once.
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

pub mod agents;
pub mod battery;
pub mod bluetooth;
pub mod briefing;
pub mod brightness;
pub mod calendar;
pub mod clipboard;
pub mod desktop;
pub mod history;
pub mod in_use;
pub mod kit;
pub mod meeting;
pub mod music;
pub mod session;
pub mod setup;
pub mod sound;
pub mod tasks;
pub mod tray;
pub mod wifi;
pub mod window_menu;
pub mod windows;

mod carry;
mod events;
mod item;

pub use carry::*;
pub use events::*;
pub use item::*;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    AGENTS_NAMESPACE, CLOCK_NAMESPACE, CONNECTIONS_NAMESPACE, DESKTOPS_NAMESPACE, DesktopTo, EDGE,
    FOOTER_NAMESPACE, Form, IN_USE_NAMESPACE, LEVELS_NAMESPACE, MEETING_NAMESPACE, MUSIC_NAMESPACE,
    POPOVER_CLOSE, POPOVER_NAMESPACE, POPOVER_WIDTH, TASKS_NAMESPACE, TODAY_NAMESPACE,
    TRAY_NAMESPACE, ToCompositor, WINDOWS_NAMESPACE,
};
use iced::{Point, Rectangle, Size, window};

use crate::ui::{
    cone::Cone,
    ring::{Declared, Ring},
};
use iced_layershell::reexport::KeyboardInteractivity;

/// Hovering a mini form's item this long opens its popover.
pub const HOVER_OPEN: Duration = Duration::from_millis(150);
/// A popover opened by hovering closes this long after the pointer left.
pub const LEAVE_CLOSE: Duration = Duration::from_millis(300);

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
    pub desktops: window::Id,
    pub agents: window::Id,
    /// The bar's own surface, whose size gives its height.
    pub surface: window::Id,
    /// The footer's (5.5), once the bar has told it.
    pub footer: Option<window::Id>,
    /// The sign of what is in use (5.11), while it shows.
    pub in_use: Option<window::Id>,
    /// The tray's (5.14), while icons show.
    pub tray: Option<window::Id>,
    /// The next meeting's (Day.2), while one is left today.
    pub meeting: Option<window::Id>,
    /// Today's (Day.3), while it lists something.
    pub today: Option<window::Id>,
    /// The to-do list's (Day.4), once the bar has told it.
    pub tasks: Option<window::Id>,
    /// The windows' over the desktops (#604), while they show.
    pub windows: Option<window::Id>,
}

/// A bar's height until its surface's is known.
const HEIGHT: f32 = 1064.0;

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
    /// Where the pointer goes, toward it or not (the kit's cone).
    cone: Cone,
    /// Hovered: when it closes unless the pointer comes back.
    leave: Option<Instant>,
    /// Opened by a shortcut (`Super+V`): `Esc` gives the keyboard back to
    /// the window that had it, not to the item.
    shortcut: bool,
}

/// Which popover shows, and what is under way.
#[derive(Debug)]
pub struct Controller {
    phase: Phase,
    /// Surfaces fading out, what they show, and when they go.
    closing: Vec<(window::Id, Item, Instant)>,
    bars: HashMap<String, Place>,
    /// The controls each bar's surfaces declare, and the one holding the
    /// keyboard's ring (after `Esc` in its popover, or `Super+B`): the
    /// kit's ring across the bar's surfaces.
    pub ring: Ring,
    /// The bars' surfaces whose height is known (their first size came).
    sized: std::collections::HashSet<window::Id>,
    /// Popovers move in and out (else they show and go at once).
    pub motion: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            closing: Vec::new(),
            bars: HashMap::new(),
            ring: Ring::default(),
            sized: std::collections::HashSet::new(),
            motion: true,
        }
    }
}

impl Controller {
    /// Bar `bar` in `form`, its surfaces `surface`, `levels`,
    /// `connections`, `desktops` and `agents`; its height stays as known.
    pub fn place(
        &mut self,
        bar: &str,
        form: Form,
        surface: window::Id,
        (levels, connections, desktops, agents): (window::Id, window::Id, window::Id, window::Id),
    ) {
        let height = self.bars.get(bar).map_or(HEIGHT, |p| p.height);
        let footer = self.bars.get(bar).and_then(|p| p.footer);
        let in_use = self.bars.get(bar).and_then(|p| p.in_use);
        let tray = self.bars.get(bar).and_then(|p| p.tray);
        let meeting = self.bars.get(bar).and_then(|p| p.meeting);
        let today = self.bars.get(bar).and_then(|p| p.today);
        let tasks = self.bars.get(bar).and_then(|p| p.tasks);
        let windows = self.bars.get(bar).and_then(|p| p.windows);
        let place = Place {
            form,
            height,
            levels,
            connections,
            desktops,
            agents,
            surface,
            footer,
            in_use,
            tray,
            meeting,
            today,
            tasks,
            windows,
        };
        self.bars.insert(bar.to_owned(), place);
        // The notifications' history waits for a bar's form (its icons).
        crate::notifications::came();
    }

    /// Bar `bar`'s to-do list's surface is `tasks`.
    pub fn set_tasks(&mut self, bar: &str, tasks: window::Id) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.tasks = Some(tasks);
        }
    }

    /// Bar `bar`'s footer's surface is `footer`.
    pub fn set_footer(&mut self, bar: &str, footer: window::Id) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.footer = Some(footer);
        }
    }

    /// Bar `bar`'s sign of what is in use is `in_use` (none: it went).
    pub fn set_in_use(&mut self, bar: &str, in_use: Option<window::Id>) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.in_use = in_use;
        }
        if in_use.is_none() {
            self.ring.forget(bar, IN_USE_NAMESPACE, false);
        }
    }

    /// Bar `bar`'s next meeting's surface is `meeting` (none: it went).
    pub fn set_meeting(&mut self, bar: &str, meeting: Option<window::Id>) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.meeting = meeting;
        }
        if meeting.is_none() {
            self.ring.forget(bar, MEETING_NAMESPACE, true);
        }
    }

    /// Bar `bar`'s Today's surface is `today` (none: it went).
    pub fn set_today(&mut self, bar: &str, today: Option<window::Id>) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.today = today;
        }
        if today.is_none() {
            self.ring.forget(bar, TODAY_NAMESPACE, true);
        }
    }

    /// Bar `bar`'s windows' surface is `windows` (none: it went).
    pub fn set_windows(&mut self, bar: &str, windows: Option<window::Id>) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.windows = windows;
        }
        if windows.is_none() {
            self.ring.forget(bar, WINDOWS_NAMESPACE, true);
        }
    }

    /// Bar `bar`'s tray's surface is `tray`, or it has none.
    pub fn set_tray(&mut self, bar: &str, tray: Option<window::Id>) {
        if let Some(place) = self.bars.get_mut(bar) {
            place.tray = tray;
        }
    }

    /// The bar whose surface is `surface` is `height` tall.
    pub fn height(&mut self, surface: window::Id, height: f32) {
        for place in self.bars.values_mut().filter(|p| p.surface == surface) {
            place.height = height;
            if height > 0.0 {
                self.sized.insert(surface);
            }
        }
    }

    /// Bar `bar`'s height.
    pub fn bar_height(&self, bar: &str) -> f32 {
        self.bars.get(bar).map_or(HEIGHT, |p| p.height)
    }

    /// Bar `bar`'s height once its surface has had its size: what is
    /// placed from the bar's bottom waits for it (#617).
    pub fn known_height(&self, bar: &str) -> Option<f32> {
        self.bars
            .get(bar)
            .filter(|p| self.sized.contains(&p.surface))
            .map(|p| p.height)
    }

    pub fn forget(&mut self, bar: &str) {
        if let Some(place) = self.bars.get(bar) {
            self.sized.remove(&place.surface);
        }
        self.bars.remove(bar);
        self.ring.forget_bar(bar);
    }

    /// The bars, by their screen's name, and their forms.
    pub fn bar_forms(&self) -> Vec<(String, Form)> {
        self.bars
            .iter()
            .map(|(bar, place)| (bar.clone(), place.form))
            .collect()
    }

    /// The forms the bars are in.
    pub fn forms(&self) -> Vec<Form> {
        self.bars.values().map(|p| p.form).collect()
    }

    /// Bar `bar`'s surface of `namespace` shows `items` (in the bar's
    /// frame), without their nodes: the tests' surfaces.
    #[cfg(test)]
    pub fn set_items(&mut self, bar: &str, namespace: &'static str, items: Vec<(Item, Rectangle)>) {
        let spots = items
            .into_iter()
            .map(|(i, r)| crate::ui::ring::Spot::at(i, r))
            .collect();
        self.declare(
            bar,
            namespace,
            Declared {
                spots,
                ..Declared::default()
            },
        );
    }

    /// Bar `bar`'s surface of `namespace` declares `declared` (the kit's
    /// ring, [`crate::ui::items`]).
    pub fn declare(&mut self, bar: &str, namespace: &'static str, declared: Declared) {
        self.ring.declare(bar, namespace, declared);
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

    /// The bar and the item whose popover is open, if one is.
    pub fn open_on(&self) -> Option<(&str, Item)> {
        match &self.phase {
            Phase::Open(open) => Some((&open.bar, open.item)),
            _ => None,
        }
    }

    /// The open popover's surface, if one is open.
    pub fn open_surface(&self) -> Option<window::Id> {
        match &self.phase {
            Phase::Open(open) => Some(open.surface),
            _ => None,
        }
    }

    /// The item holding the keyboard (after `Esc` in its popover), and its
    /// bar.
    pub fn focused_on(&self) -> Option<(&str, Item)> {
        self.ring
            .holder()
            .map(|(bar, item, _)| (bar.as_str(), *item))
    }

    /// What bar `bar`'s surfaces show, each with its surface's namespace
    /// and where it is in the bar's frame, from the top (screen readers'
    /// tree, crate::a11y).
    pub fn items_of(&self, bar: &str) -> Vec<(&'static str, Item, Rectangle)> {
        self.ring
            .spots(bar)
            .into_iter()
            .map(|(namespace, s)| (namespace, s.item, s.rect))
            .collect()
    }

    /// The form of the bar whose popover `id` is open.
    pub fn form_of(&self, id: window::Id) -> Option<Form> {
        match &self.phase {
            Phase::Open(open) if open.surface == id => self.bars.get(&open.bar).map(|p| p.form),
            _ => None,
        }
    }

    /// Whether `item` of `bar` has its popover open.
    pub fn shows(&self, bar: &str, item: Item) -> bool {
        matches!(&self.phase, Phase::Open(o) if o.bar == bar && o.item == item)
    }

    /// Whether `item` of `bar` holds the keyboard.
    pub fn focused(&self, bar: &str, item: Item) -> bool {
        self.ring.holds(bar, item)
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
                Phase::Closed if self.bars.get(&bar).is_some_and(|p| item.hovers(p.form)) => {
                    self.phase = Phase::Hovering {
                        bar,
                        item,
                        rect,
                        since: now,
                    };
                }
                Phase::Open(open) if open.bar == bar && open.item == item => {
                    open.cone.entered();
                    open.leave = None;
                }
                // A desktop's list shows: another tile's, at once.
                Phase::Open(open)
                    if open.bar == bar
                        && open.how == Opened::Hover
                        && matches!(
                            (open.item, item),
                            (Item::Desktop { .. }, Item::Desktop { .. })
                        ) =>
                {
                    tracing::debug!(?item, "the list shows another desktop");
                    open.item = item;
                    open.from = rect;
                    open.cone.entered();
                    open.leave = None;
                    effects.extend(self.refit());
                    effects.push(Effect::Wake(POPOVER_NAMESPACE));
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
                    open.cone.entered();
                    open.leave = None;
                }
            }
            Event::PopoverLeave => {
                if let Phase::Open(open) = &mut self.phase {
                    open.cone.left_popover();
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
            Event::ItemPress { bar, item, rect } if !item.clicks() => {
                self.unfocus(&mut effects);
                self.close(now, &mut effects);
                effects.extend(acts(bar, item, rect));
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
                    let shortcut = open.shortcut;
                    self.close(now, &mut effects);
                    if shortcut {
                        return effects;
                    }
                    if let Some(place) = self.bars.get(&bar) {
                        effects.push(Effect::Keyboard(
                            item.surface(place),
                            KeyboardInteractivity::Exclusive,
                        ));
                        self.ring.put(&bar, item, from);
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
            Event::ItemActivate => match self.ring.holder().cloned() {
                Some((bar, item, from)) if !item.clicks() => {
                    effects.extend(acts(bar, item, from));
                }
                Some((bar, item, from)) => {
                    self.unfocus(&mut effects);
                    self.open(&bar, item, from, Opened::Click, &mut effects);
                    // Opened by the keyboard: its first control holds it.
                    if let Phase::Open(open) = &self.phase {
                        effects.push(Effect::FirstControl(open.surface));
                    }
                }
                None => {}
            },
            Event::ItemStep { back } => self.step(back, &mut effects),
            Event::ItemEdge { last } => self.edge(last, &mut effects),
            Event::ItemSide { right } => self.side(right, &mut effects),
            Event::ItemRelease => self.unfocus(&mut effects),
            Event::Chosen => self.close(now, &mut effects),
            Event::Replace(item) => {
                if let Phase::Open(open) = &self.phase {
                    // A window's menu from a popover's line: its top on
                    // that popover's, where the line was (#726).
                    let from = match item {
                        Item::WindowMenu(_) => Rectangle {
                            y: open.rect.y,
                            ..open.from
                        },
                        _ => open.from,
                    };
                    let bar = open.bar.clone();
                    self.close(now, &mut effects);
                    self.open(&bar, item, from, Opened::Click, &mut effects);
                }
            }
            Event::Open { bar, item, rect } => {
                self.unfocus(&mut effects);
                self.close(now, &mut effects);
                self.open(&bar, item, rect, Opened::Click, &mut effects);
            }
            Event::GoTo { id, row } => {
                if let Phase::Open(open) = &self.phase {
                    let (bar, at) = (open.bar.clone(), open.rect.position());
                    let row = Rectangle::new(Point::new(at.x + row.x, at.y + row.y), row.size());
                    self.close(now, &mut effects);
                    effects.push(Effect::GoToAgent { id, bar, row });
                }
            }
            Event::BarChanged { bar } => {
                let on_it = match &self.phase {
                    Phase::Open(open) => open.bar == bar,
                    Phase::Hovering { bar: b, .. } => *b == bar,
                    Phase::Closed => false,
                };
                if on_it {
                    self.close(now, &mut effects);
                }
                if self.ring.holder().is_some_and(|(b, _, _)| *b == bar) {
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
                tracing::debug!(
                    ?item,
                    hovered_ms = now.saturating_duration_since(*since).as_millis() as u64,
                    "hovered long enough"
                );
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
        let size = (item.width(), item.height(bar, place.form, true));
        let rect = placed(
            place,
            self.anchor(bar, place.form, item, from, size.1),
            size,
            item.on_bottom(),
        );
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
        if item.lit_while_open() {
            effects.push(Effect::Wake(item.namespace(place.form)));
        }
        if item == Item::Secretary {
            effects.push(Effect::Briefing(true));
        }
        self.phase = Phase::Open(Open {
            bar: bar.to_owned(),
            item,
            how,
            surface,
            rect,
            from,
            cone: Cone::inside(),
            leave: None,
            shortcut: false,
        });
    }

    /// `Super+V` (5.6): the clipboard's history opens on `output`'s bar
    /// (else the first bar), beside the footer's clipboard, its search
    /// holding the keyboard; open, it closes (opened by hovering, it takes
    /// the keyboard).
    pub fn toggle_clipboard(&mut self, output: &str, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Phase::Open(open) = &mut self.phase
            && open.item == Item::Clipboard
        {
            // Opened by hovering: its search takes the keyboard.
            if open.how == Opened::Hover {
                open.how = Opened::Click;
                open.leave = None;
                open.shortcut = true;
                effects.push(Effect::Keyboard(
                    open.surface,
                    KeyboardInteractivity::Exclusive,
                ));
                effects.push(Effect::Wake(POPOVER_NAMESPACE));
            } else {
                self.close(now, &mut effects);
            }
            return effects;
        }
        let bar = if self.bars.contains_key(output) {
            output.to_owned()
        } else {
            match self.bars.keys().min() {
                Some(bar) => bar.clone(),
                None => return effects,
            }
        };
        let from = self
            .ring
            .find(&bar, FOOTER_NAMESPACE, Item::Clipboard)
            .unwrap_or_else(|| {
                let height = self.bar_height(&bar);
                Rectangle::new(Point::new(-50.0, height - 46.0), Size::new(40.0, 34.0))
            });
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        self.open(&bar, Item::Clipboard, from, Opened::Click, &mut effects);
        if let Phase::Open(open) = &mut self.phase {
            open.shortcut = true;
        }
        effects
    }

    /// The session menu (7.4) opens on `output`'s bar (else the first),
    /// beside the footer's user, as a click opens it, for a confirmation
    /// the compositor started (`Super+Shift+E`); open already, it stays,
    /// as if clicked.
    pub fn open_session(&mut self, output: &str, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Phase::Open(open) = &mut self.phase
            && open.item == Item::User
        {
            if open.how == Opened::Hover {
                open.how = Opened::Click;
                open.leave = None;
                effects.push(Effect::Keyboard(
                    open.surface,
                    KeyboardInteractivity::Exclusive,
                ));
            }
            return effects;
        }
        let bar = if self.bars.contains_key(output) {
            output.to_owned()
        } else {
            match self.bars.keys().min() {
                Some(bar) => bar.clone(),
                None => return effects,
            }
        };
        let from = self
            .ring
            .find(&bar, FOOTER_NAMESPACE, Item::User)
            .unwrap_or_else(|| {
                let height = self.bar_height(&bar);
                Rectangle::new(Point::new(-50.0, height - 46.0), Size::new(40.0, 34.0))
            });
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        self.open(&bar, Item::User, from, Opened::Click, &mut effects);
        if let Phase::Open(open) = &mut self.phase {
            open.shortcut = true;
        }
        effects
    }

    /// The notifications' history opens on `output`'s bar (else the
    /// first), beside the footer's bell: the summary of what was held
    /// while sharing links to it (5.11).
    pub fn open_history(&mut self, output: &str, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.open_item() == Some(Item::Notifications) {
            return effects;
        }
        let bar = if self.bars.contains_key(output) {
            output.to_owned()
        } else {
            match self.bars.keys().min() {
                Some(bar) => bar.clone(),
                None => return effects,
            }
        };
        let from = self
            .ring
            .find(&bar, FOOTER_NAMESPACE, Item::Notifications)
            .unwrap_or_else(|| {
                let height = self.bar_height(&bar);
                Rectangle::new(Point::new(-50.0, height - 46.0), Size::new(40.0, 34.0))
            });
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        self.open(&bar, Item::Notifications, from, Opened::Click, &mut effects);
        effects
    }

    /// The calendar opens on `output`'s bar (else the first), beside its
    /// clock, on what was asked of it (`calendar::open_on`): open already
    /// on that bar, it turns to it.
    pub fn open_calendar(&mut self, output: &str, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let bar = if self.bars.contains_key(output) {
            output.to_owned()
        } else {
            match self.bars.keys().min() {
                Some(bar) => bar.clone(),
                None => return effects,
            }
        };
        if self.shows(&bar, Item::Calendar) {
            Item::Calendar.opening();
            effects.extend(self.refit());
            effects.push(Effect::Wake(POPOVER_NAMESPACE));
            return effects;
        }
        let from = self
            .ring
            .find(&bar, CLOCK_NAMESPACE, Item::Calendar)
            .unwrap_or_else(|| Rectangle::new(Point::new(-184.0, 14.0), Size::new(172.0, 77.0)));
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        self.open(&bar, Item::Calendar, from, Opened::Click, &mut effects);
        effects
    }

    /// The actions palette (#736) opens `item`'s popover on `output`'s bar
    /// (else the first), as a click on it does: beside its item, else
    /// beside `near` (the Sources beside the secretary).
    pub fn open_asked(
        &mut self,
        output: &str,
        item: Item,
        near: Option<Item>,
        now: Instant,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        let Some(bar) = self.bar_for(output) else {
            return effects;
        };
        let items = self.bar_items(&bar);
        let from = items
            .iter()
            .find(|(i, _)| *i == item)
            .or_else(|| items.iter().find(|(i, _)| Some(*i) == near))
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| {
                let height = self.bar_height(&bar);
                Rectangle::new(Point::new(-50.0, height / 2.0), Size::new(40.0, 34.0))
            });
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        self.open(&bar, item, from, Opened::Click, &mut effects);
        effects
    }

    /// The bar a popover for `output` opens on: its own, else the first.
    pub fn bar_for(&self, output: &str) -> Option<String> {
        if self.bars.contains_key(output) {
            Some(output.to_owned())
        } else {
            self.bars.keys().min().cloned()
        }
    }

    /// Whether the popover `id` holds the keyboard: opened by a click (or
    /// a shortcut), or clicked in.
    pub fn holds_keyboard(&self, id: window::Id) -> bool {
        matches!(&self.phase, Phase::Open(open) if open.surface == id && open.how == Opened::Click)
    }

    /// What the open popover shows changed: to its new size, if it
    /// changed, still beside its item and within the bar.
    pub fn refit(&mut self) -> Vec<Effect> {
        let Phase::Open(open) = &mut self.phase else {
            return Vec::new();
        };
        let Some(place) = self.bars.get(&open.bar).copied() else {
            return Vec::new();
        };
        let height = open.item.height(&open.bar, place.form, false);
        if open.rect.height.round() as u32 == height {
            return Vec::new();
        }
        let size = (open.item.width(), height);
        let before = margins(place, open.rect);
        let anchor = anchor(
            &self.ring, &open.bar, place.form, open.item, open.from, height,
        );
        open.rect = placed(place, anchor, size, open.item.on_bottom());
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
        if open.item.lit_while_open() {
            effects.push(Effect::Wake(open.item.namespace(form)));
        }
        if open.item == Item::Secretary {
            effects.push(Effect::Briefing(false));
        }
    }

    /// What a popover of `item` opened from `from` on `bar` lines up with.
    fn anchor(&self, bar: &str, form: Form, item: Item, from: Rectangle, height: u32) -> Rectangle {
        anchor(&self.ring, bar, form, item, from, height)
    }

    fn unfocus(&mut self, effects: &mut Vec<Effect>) {
        if let Some((bar, item, _)) = self.ring.take()
            && let Some(place) = self.bars.get(&bar)
        {
            effects.push(Effect::Keyboard(
                item.surface(place),
                KeyboardInteractivity::None,
            ));
            effects.push(Effect::Wake(item.namespace(place.form)));
        }
    }

    /// The items of bar `bar`, in the drawing's order: top to bottom, and
    /// along a row left to right.
    fn bar_items(&self, bar: &str) -> Vec<(Item, Rectangle)> {
        self.ring.items(bar)
    }

    /// The keyboard to the next item of the bar (the previous, `back`),
    /// as the kit's ring goes ([`Ring::step`]).
    fn step(&mut self, back: bool, effects: &mut Vec<Effect>) {
        if let Some(next) = self.ring.step(back) {
            self.focus_on(next, effects);
        }
    }

    /// `Home` or `End` (`last`): the bar's first item, or its last.
    fn edge(&mut self, last: bool, effects: &mut Vec<Effect>) {
        if let Some(next) = self.ring.edge(last) {
            self.focus_on(next, effects);
        }
    }

    /// `←` or `→` (`right`): a level's value by 5 %; else the item beside
    /// along the same row (the desktops' tiles, the footer's buttons, the
    /// tray's icons), stopping at its ends.
    fn side(&mut self, right: bool, effects: &mut Vec<Effect>) {
        use hocus_link::LevelKey;
        let Some(&(_, item, _)) = self.ring.holder() else {
            return;
        };
        match item {
            Item::Sound => {
                effects.push(Effect::Level(if right {
                    LevelKey::VolumeUp
                } else {
                    LevelKey::VolumeDown
                }));
                return;
            }
            Item::Brightness => {
                effects.push(Effect::Level(if right {
                    LevelKey::BrightnessUp
                } else {
                    LevelKey::BrightnessDown
                }));
                return;
            }
            _ => {}
        }
        if let Some(next) = self.ring.side(right) {
            self.focus_on(next, effects);
        }
    }

    /// The keyboard from the item holding it to `next` (its bar, its item,
    /// where it is): `next`'s surface takes it when it is another; both
    /// draw again.
    fn focus_on(
        &mut self,
        (bar, next, rect): (String, Item, Rectangle),
        effects: &mut Vec<Effect>,
    ) {
        let Some(place) = self.bars.get(&bar).copied() else {
            return;
        };
        let Some((_, item, _)) = self.ring.holder().cloned() else {
            return;
        };
        let (from, to) = (item.surface(&place), next.surface(&place));
        if from != to {
            effects.push(Effect::Keyboard(from, KeyboardInteractivity::None));
            effects.push(Effect::Keyboard(to, KeyboardInteractivity::Exclusive));
            effects.push(Effect::Wake(item.namespace(place.form)));
        }
        self.ring.put(&bar, next, rect);
        effects.push(Effect::Wake(next.namespace(place.form)));
        effects.push(Effect::Ring { bar, rect });
    }

    /// `Super+B` (bar.md §11): the keyboard into bar `bar`, on the first
    /// of `wanted` it shows (the agents waiting, then those stuck), else
    /// on its clock's group, else on its first item. What was open
    /// closes, and an item of another bar lets the keyboard go.
    pub fn enter(&mut self, bar: &str, wanted: &[Item], now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if matches!(self.phase, Phase::Hovering { .. }) {
            self.phase = Phase::Closed;
        }
        self.unfocus(&mut effects);
        self.close(now, &mut effects);
        let Some(place) = self.bars.get(bar).copied() else {
            return effects;
        };
        let Some((item, rect)) = self.ring.landing(bar, wanted, Item::Calendar) else {
            tracing::debug!(bar, "no item in the bar to take the keyboard");
            return effects;
        };
        tracing::debug!(bar, ?item, "the keyboard into the bar");
        effects.push(Effect::Keyboard(
            item.surface(&place),
            KeyboardInteractivity::Exclusive,
        ));
        self.ring.put(bar, item, rect);
        effects.push(Effect::Wake(item.namespace(place.form)));
        effects.push(Effect::Ring {
            bar: bar.to_owned(),
            rect,
        });
        effects
    }

    /// The bar holding the keyboard lets it go (`Super+B` past the last
    /// bar, the session locking): its popover closes, its item lets go.
    pub fn leave(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.unfocus(&mut effects);
        if matches!(&self.phase, Phase::Open(open) if open.how == Opened::Click) {
            self.close(now, &mut effects);
        }
        effects
    }
}

/// What a popover of `item`, `height` tall, opened from `from` on `bar`
/// lines up with: the item, or for a desktop's list the tiles' top (their
/// first row); a tray icon's menu has its bottom on the icon's.
fn anchor(
    ring: &Ring,
    bar: &str,
    form: Form,
    item: Item,
    from: Rectangle,
    height: u32,
) -> Rectangle {
    // A tray icon's menu, and the windows' popovers (their item's over
    // the desktops, near the bar's foot, #604): their bottom on the item's.
    if matches!(item, Item::Tray(_) | Item::Windows | Item::WindowsMore) {
        return Rectangle {
            y: from.y + from.height - height as f32,
            ..from
        };
    }
    // The agents' popovers' top where the mockup has it beside their
    // group, wherever the bar's column puts the group (its first item's
    // top: her row, or the mini item).
    if item.agents() {
        let group = ring
            .top_of(bar, AGENTS_NAMESPACE)
            .unwrap_or_else(|| crate::column::start(form));
        return Rectangle {
            y: group + agents::TOP - crate::column::start(form),
            ..from
        };
    }
    // The sources' popover: its top on the first-run card's when opened
    // from it (from the calendar's "Choisir des agendas…", where the
    // calendar's was).
    if item == Item::DayChoose {
        let card = crate::tasks::draw::full_top(bar);
        if from.y >= card {
            return Rectangle { y: card, ..from };
        }
    }
    if !matches!(item, Item::Desktop { .. }) {
        return from;
    }
    let top = ring.top_of(bar, DESKTOPS_NAMESPACE).unwrap_or(from.y);
    Rectangle { y: top, ..from }
}

/// What an item at `rect` does when clicked, or on `Enter`, when that
/// opens no popover: the next layout, the tile's desktop, the row's agent.
fn acts(bar: String, item: Item, rect: Rectangle) -> Option<Effect> {
    match item {
        Item::Keyboard => Some(Effect::NextLayout),
        Item::Desktop { row, index } => Some(Effect::Desktop {
            bar: crate::desktops::output_of(&bar, row),
            to: DesktopTo::At(index),
        }),
        Item::Decline => Some(Effect::Decline),
        Item::Agent(id) => Some(Effect::GoToAgent { id, bar, row: rect }),
        Item::Menu => Some(Effect::ToggleLauncher(bar)),
        Item::Tray(key) => Some(Effect::TrayMenu { bar, key, rect }),
        Item::Calendar => None,
        Item::Meeting => crate::meeting::open_in_calendar(&bar),
        Item::Join => crate::meeting::join(),
        Item::TodayLine(row) => crate::today::open(&bar, usize::from(row)),
        Item::TodayMore => crate::today::open_more(&bar),
        Item::TaskAdd => Some(Effect::Tasks(crate::tasks::Act::Add { bar })),
        Item::Task(slot) => Some(Effect::Tasks(crate::tasks::Act::Toggle { bar, slot })),
        Item::DayLater => {
            crate::setup::later();
            None
        }
        Item::Window(id) => {
            tracing::debug!(window = id, "the bar's windows bring a window");
            Some(Effect::Bring(id))
        }
        Item::WindowClose(id) => {
            tracing::debug!(window = id, "the bar's windows ask a window to close");
            Some(Effect::Tell(ToCompositor::Close(id)))
        }
        Item::WindowsMore => {
            tracing::debug!("the bar's windows ask for the overview");
            Some(Effect::Tell(ToCompositor::Overview))
        }
        Item::Sound
        | Item::Wifi
        | Item::Bluetooth
        | Item::Brightness
        | Item::Battery
        | Item::Sources
        | Item::AnotherTool
        | Item::Plug(_)
        | Item::Secretary
        | Item::Notifications
        | Item::Clipboard
        | Item::User
        | Item::InUse
        | Item::MeetingMark
        | Item::Tasks
        | Item::TasksMore
        | Item::DayChoose
        | Item::Windows
        | Item::WindowMenu(_)
        | Item::Music => None,
    }
}

impl Open {
    /// The pointer left the item at `at`.
    fn left(&mut self, at: Point, now: Instant) {
        self.cone.left(at);
        if self.how == Opened::Hover {
            self.leave = Some(now + LEAVE_CLOSE);
        }
    }

    /// The pointer seen at `at`, off the item and the popover: moving
    /// toward the popover, within the kit's cone, it keeps a hovered one
    /// open a while longer.
    fn seen(&mut self, at: Point, now: Instant) {
        if self.cone.seen(at, self.rect) && self.how == Opened::Hover {
            self.leave = Some(now + LEAVE_CLOSE);
        }
    }
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
/// in the bar's frame (from its right edge): 8 px left of the bar (right
/// of it on the left, `crate::side`), its top on the item's (or its
/// bottom on the bar's, `on_bottom`), pushed up to stay 8 px from the
/// screen's bottom (the bar's own bottom), and down to stay 8 px from its
/// top.
pub fn placed(place: Place, item: Rectangle, (w, h): (u32, u32), on_bottom: bool) -> Rectangle {
    let (w, h) = (w as f32, h as f32);
    let x = if crate::side::left() {
        EDGE as f32
    } else {
        -(place.form.width() as f32) - EDGE as f32 - w
    };
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
    // The field to add a task, open, sees the presses elsewhere: it
    // closes when empty, else lets the keyboard go.
    if matches!(
        &event,
        Event::PressElsewhere | Event::Outside | Event::Open { .. }
    ) || matches!(&event, Event::ItemPress { item, .. } if *item != Item::TaskAdd)
    {
        crate::tasks::pressed_elsewhere();
    }
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
    if crate::launcher::key(key, modifiers, text)
        || crate::emoji::key(key, modifiers, text)
        || crate::actions::key(key, modifiers, text)
    {
        return true;
    }
    let (menu, focused) = with(|c| {
        let menu = matches!(&c.phase, Phase::Open(open) if open.how == Opened::Click);
        (menu, c.ring.holder().is_some())
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
    // go, Tab, the arrows, Home and End go through the bar's items (left
    // and right along a row, or a level's value).
    match key {
        Key::Named(Named::Enter | Named::Space) => event(Event::ItemActivate),
        Key::Named(Named::Escape) => event(Event::ItemRelease),
        Key::Named(Named::Tab) => event(Event::ItemStep {
            back: modifiers.shift(),
        }),
        Key::Named(Named::ArrowDown) => event(Event::ItemStep { back: false }),
        Key::Named(Named::ArrowUp) => event(Event::ItemStep { back: true }),
        Key::Named(Named::Home) => event(Event::ItemEdge { last: false }),
        Key::Named(Named::End) => event(Event::ItemEdge { last: true }),
        Key::Named(Named::ArrowLeft) => event(Event::ItemSide { right: false }),
        Key::Named(Named::ArrowRight) => event(Event::ItemSide { right: true }),
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
/// items; and the clock's group (Day.5), whose surface lets the pointer
/// through: the pointer on it, a hand, and a press opening the calendar.
/// It never draws, nor asks to.
#[derive(Debug, Clone)]
pub struct BarPointer {
    /// The bar's screen's name.
    pub bar: String,
}

impl BarPointer {
    /// Where the clock's group is, in the bar's frame, while it shows.
    fn calendar(&self) -> Option<Rectangle> {
        with(|c| c.ring.find(&self.bar, CLOCK_NAMESPACE, Item::Calendar))
    }

    /// `position` on the surface at `bounds`, in the bar's frame.
    fn in_bar(position: Point, bounds: Rectangle) -> Point {
        Point::new(position.x - bounds.x - bounds.width, position.y - bounds.y)
    }
}

/// The bar's surface's pointer: on the clock's group, and where it was.
#[derive(Debug, Default)]
pub struct BarPointerState {
    on_clock: bool,
    at: Option<Point>,
}

impl<Message> iced::widget::canvas::Program<Message> for BarPointer {
    type State = BarPointerState;

    fn update(
        &self,
        state: &mut BarPointerState,
        seen: &iced::Event,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<Message>> {
        use iced::mouse::Event as Mouse;
        match seen {
            iced::Event::Mouse(Mouse::CursorMoved { position }) => {
                let at = Self::in_bar(*position, bounds);
                state.at = Some(at);
                let group = self.calendar().filter(|r| r.contains(at));
                match (group, state.on_clock) {
                    (Some(rect), false) => {
                        state.on_clock = true;
                        crate::clock::item::hover(&self.bar, true);
                        event(Event::ItemEnter {
                            bar: self.bar.clone(),
                            item: Item::Calendar,
                            rect,
                        });
                    }
                    (None, true) => {
                        state.on_clock = false;
                        crate::clock::item::hover(&self.bar, false);
                        event(Event::ItemLeave { at });
                    }
                    (Some(_), true) => {}
                    (None, false) => event(Event::Pointer { at }),
                }
            }
            iced::Event::Mouse(Mouse::CursorLeft) if state.on_clock => {
                state.on_clock = false;
                crate::clock::item::hover(&self.bar, false);
                event(Event::ItemLeave {
                    at: state.at.unwrap_or(Point::ORIGIN),
                });
            }
            iced::Event::Mouse(Mouse::ButtonPressed(iced::mouse::Button::Left))
                if state.on_clock =>
            {
                if let Some(rect) = self.calendar() {
                    event(Event::ItemPress {
                        bar: self.bar.clone(),
                        item: Item::Calendar,
                        rect,
                    });
                }
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
        _state: &BarPointerState,
        _renderer: &iced::Renderer,
        _theme: &iced::Theme,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        Vec::new()
    }

    fn mouse_interaction(
        &self,
        state: &BarPointerState,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if state.on_clock {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::default()
        }
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

/// `Super+B` (bar.md §11, [`ToShell::BarKeyboard`]): the keyboard into
/// `output`'s bar, on the agent waiting longest, else the first stuck,
/// else (in the mini form) the secretary while her badge shows, else the
/// clock; `None`, the bar holding it lets it go.
///
/// [`ToShell::BarKeyboard`]: hocus_link::ToShell::BarKeyboard
pub fn bar_keyboard(output: Option<&str>) {
    let now = Instant::now();
    let effects = match output {
        Some(bar) => {
            let brief = crate::agents::Brief::of(&crate::agents::said().agents);
            let mini = with(|c| c.bars.get(bar).map(|p| p.form)) == Some(Form::Mini);
            let mut wanted: Vec<Item> = brief
                .waiting
                .iter()
                .chain(&brief.stuck)
                .map(|e| Item::Agent(e.id))
                .collect();
            if mini && brief.needs() > 0 {
                wanted.push(Item::Secretary);
            }
            with(|c| c.enter(bar, &wanted, now))
        }
        None => with(|c| c.leave(now)),
    };
    send(effects);
}

/// The actions palette (#736) opens `item`'s popover on `output`'s bar.
pub fn open_asked(output: &str, item: Item, near: Option<Item>) {
    let effects = with(|c| c.open_asked(output, item, near, Instant::now()));
    send(effects);
}

/// The bar a popover for `output` opens on (its name; `output` when the
/// shell has no bar yet).
pub fn bar_for(output: &str) -> String {
    with(|c| c.bar_for(output)).unwrap_or_else(|| output.to_owned())
}

/// The Settings open on their page `slug` (the actions palette, #736).
pub(crate) fn open_settings_page(slug: &str) {
    session::open_settings(Some(slug));
}

/// The Settings open searching for `query` (the actions palette, #736).
pub(crate) fn open_settings_searching(query: &str) {
    session::open_settings_with(vec!["--search".to_owned(), query.to_owned()]);
}

/// The notifications' history opens on `output`'s bar (5.11's summary).
pub fn open_history(output: &str) {
    let effects = with(|c| c.open_history(output, Instant::now()));
    send(effects);
}

/// `Super+V` (5.6): the clipboard's history opens on `output`'s bar, or
/// closes.
pub fn toggle_clipboard(output: &str) {
    let effects = with(|c| c.toggle_clipboard(output, Instant::now()));
    send(effects);
}

/// What `item`'s popover shows changed: if it is open, it draws again, to
/// its new size if that changed.
pub fn changed(item: Item) {
    let (open, mut effects) = with(|c| {
        let open = c.open_item() == Some(item);
        (open, if open { c.refit() } else { Vec::new() })
    });
    if open {
        // A kit surface's view is built again as it next draws (#770).
        crate::ui::touch();
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

/// Whether the shell is linked to a compositor.
pub(crate) fn linked_now() -> bool {
    LINK.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

/// Tells the compositor; `false` without it.
pub(crate) fn tell(message: ToCompositor) -> bool {
    LINK.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|link| link.unbounded_send(message.line()).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "HDMI-A-1";

    /// `effects` but the ring's place told for the magnifier.
    fn no_ring(effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|e| !matches!(e, Effect::Ring { .. }))
            .collect()
    }
    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn controller(form: Form) -> (Controller, window::Id) {
        let mut c = Controller::default();
        let levels = window::Id::unique();
        c.place(
            BAR,
            form,
            window::Id::unique(),
            (
                levels,
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
            ),
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
    fn super_v_opens_the_clipboards_history_holding_the_keyboard() {
        let (mut c, _) = controller(Form::Full);
        let t = Instant::now();
        c.motion = false;
        // On a screen without a bar: on the bar there is.
        let (id, keyboard) = opened(&c.toggle_clipboard("DP-9", t)).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::Exclusive);
        assert!(c.shows(BAR, Item::Clipboard));
        assert!(c.holds_keyboard(id));
        // Esc closes it, and the keyboard goes back to the window, not to
        // the footer's button.
        let effects = c.handle(Event::Escape, t);
        assert!(effects.contains(&Effect::Remove(id)));
        assert!(!c.focused(BAR, Item::Clipboard));
        // Again, it closes.
        c.toggle_clipboard(BAR, t);
        assert!(c.shows(BAR, Item::Clipboard));
        c.toggle_clipboard(BAR, t);
        assert!(!c.shows(BAR, Item::Clipboard));
        // Opened by hovering its button, it takes the keyboard.
        let button = Rectangle::new(Point::new(-183.0, 1006.0), Size::new(40.0, 34.0));
        c.handle(
            Event::ItemEnter {
                bar: BAR.to_owned(),
                item: Item::Clipboard,
                rect: button,
            },
            t,
        );
        let (id, keyboard) = opened(&c.tick(t + MS(150))).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::OnDemand);
        assert!(!c.holds_keyboard(id));
        let effects = c.toggle_clipboard(BAR, t + MS(200));
        assert!(effects.contains(&Effect::Keyboard(id, KeyboardInteractivity::Exclusive)));
        assert!(c.holds_keyboard(id));
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
        c.place(
            BAR,
            Form::Mini,
            window::Id::unique(),
            (
                levels,
                connections,
                window::Id::unique(),
                window::Id::unique(),
            ),
        );
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
    fn a_popover_sits_beside_its_item_and_inside_the_screen() {
        let place = Place {
            form: Form::Full,
            height: 1064.0,
            levels: window::Id::unique(),
            connections: window::Id::unique(),
            desktops: window::Id::unique(),
            agents: window::Id::unique(),
            surface: window::Id::unique(),
            footer: None,
            in_use: None,
            tray: None,
            meeting: None,
            today: None,
            tasks: None,
            windows: None,
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
            desktops: window::Id::unique(),
            agents: window::Id::unique(),
            surface: window::Id::unique(),
            footer: None,
            in_use: None,
            tray: None,
            meeting: None,
            today: None,
            tasks: None,
            windows: None,
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
            (
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
            ),
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
        let effects = no_ring(c.handle(Event::ItemStep { back: false }, t));
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

    #[test]
    fn resting_on_a_tile_shows_its_list_and_another_tile_shows_its_own_at_once() {
        let (mut c, _) = controller(Form::Full);
        c.motion = false;
        let t = Instant::now();
        let tile = |i: u8| {
            Rectangle::new(
                Point::new(
                    -185.0 + 44.0 * f32::from(i % 4),
                    700.0 + 36.0 * f32::from(i / 4),
                ),
                Size::new(38.0, 30.0),
            )
        };
        c.set_items(
            BAR,
            DESKTOPS_NAMESPACE,
            (0..6)
                .map(|i| (Item::Desktop { row: 0, index: i }, tile(i)))
                .collect(),
        );
        let enter = |i: u8| Event::ItemEnter {
            bar: BAR.to_owned(),
            item: Item::Desktop { row: 0, index: i },
            rect: tile(i),
        };
        // In the full form too, 150 ms after; its top on the tiles' first
        // row, even from the second; the tiles not drawn for it.
        c.handle(enter(5), t);
        assert!(c.tick(t + MS(149)).is_empty());
        let effects = c.tick(t + MS(150));
        let (id, keyboard) = opened(&effects).expect("opened");
        assert_eq!(keyboard, KeyboardInteractivity::OnDemand);
        assert!(!effects.contains(&Effect::Wake(DESKTOPS_NAMESPACE)));
        let rect = match &c.phase {
            Phase::Open(open) => open.rect,
            other => panic!("{other:?}"),
        };
        assert_eq!((rect.y, rect.width), (700.0, 240.0));
        // Another tile: its list at once, on the same surface.
        c.handle(
            Event::ItemLeave {
                at: Point::new(-60.0, 750.0),
            },
            t + MS(200),
        );
        let effects = c.handle(enter(4), t + MS(220));
        assert!(effects.contains(&Effect::Wake(POPOVER_NAMESPACE)));
        assert!(c.shows(BAR, Item::Desktop { row: 0, index: 4 }));
        assert!(c.is_open(id));
        assert_eq!(c.deadline(), None);
        // A row chosen: it closes.
        assert_eq!(
            c.handle(Event::Chosen, t + MS(300)),
            vec![Effect::Remove(id)]
        );
        // A click on a tile goes there and closes the list.
        c.handle(enter(1), t + MS(400));
        let (id, _) = opened(&c.tick(t + MS(550))).expect("opened");
        let effects = c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Desktop { row: 0, index: 1 },
                rect: tile(1),
            },
            t + MS(600),
        );
        assert_eq!(
            effects,
            vec![
                Effect::Remove(id),
                Effect::Desktop {
                    bar: BAR.to_owned(),
                    to: DesktopTo::At(1),
                }
            ]
        );
    }

    #[test]
    fn an_agents_row_goes_to_the_agent_on_a_click_or_enter() {
        let mut c = Controller::default();
        let agents = window::Id::unique();
        c.place(
            BAR,
            Form::Full,
            window::Id::unique(),
            (
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
                agents,
            ),
        );
        c.motion = false;
        let t = Instant::now();
        let sources = Rectangle::new(Point::new(-60.0, 100.0), Size::new(45.0, 16.0));
        let row =
            |i: f32| Rectangle::new(Point::new(-190.0, 126.0 + 28.0 * i), Size::new(184.0, 28.0));
        c.set_items(
            BAR,
            AGENTS_NAMESPACE,
            vec![
                (Item::Sources, sources),
                (Item::Agent(3), row(0.0)),
                (Item::Agent(5), row(1.0)),
            ],
        );
        // A click: to the agent, no popover.
        let effects = c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Agent(5),
                rect: row(1.0),
            },
            t,
        );
        assert_eq!(
            effects,
            vec![Effect::GoToAgent {
                id: 5,
                bar: BAR.to_owned(),
                row: row(1.0)
            }]
        );
        assert_eq!(c.open_item(), None);
        // The keyboard on Sources (Esc in its popover), down to the first
        // row, Enter: to that agent.
        c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Sources,
                rect: sources,
            },
            t,
        );
        c.handle(Event::Escape, t);
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Agent(3)));
        assert_eq!(
            c.handle(Event::ItemActivate, t),
            vec![Effect::GoToAgent {
                id: 3,
                bar: BAR.to_owned(),
                row: row(0.0)
            }]
        );
    }

    #[test]
    fn the_clocks_group_opens_the_calendar_by_a_click_only_and_holds_the_keyboard_after_esc() {
        let mut c = Controller::default();
        let surface = window::Id::unique();
        c.place(
            BAR,
            Form::Mini,
            surface,
            (
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
            ),
        );
        c.motion = false;
        let t = Instant::now();
        let group = Rectangle::new(Point::new(-52.0, 12.0), Size::new(48.0, 46.0));
        c.set_items(BAR, CLOCK_NAMESPACE, vec![(Item::Calendar, group)]);
        // Resting on it opens nothing, in the mini form too.
        assert!(!Item::Calendar.hovers(Form::Mini) && !Item::Calendar.hovers(Form::Full));
        c.handle(
            Event::ItemEnter {
                bar: BAR.to_owned(),
                item: Item::Calendar,
                rect: group,
            },
            t,
        );
        c.tick(t + HOVER_OPEN * 4);
        assert_eq!(c.open_item(), None);
        // A click: 320 wide, its top on the group's, the group lit.
        let press = || Event::ItemPress {
            bar: BAR.to_owned(),
            item: Item::Calendar,
            rect: group,
        };
        let effects = c.handle(press(), t);
        let opened = effects.iter().find_map(|e| match e {
            Effect::Open { size, .. } => Some(*size),
            _ => None,
        });
        assert_eq!(opened.map(|s| s.0), Some(320), "{effects:?}");
        assert!(effects.contains(&Effect::Wake(CLOCK_NAMESPACE)));
        assert!(c.shows(BAR, Item::Calendar));
        assert!(!Item::Calendar.on_bottom());
        // `Esc`: closed, the keyboard on the bar's surface, which hears
        // the group's keys; `Enter` opens it again.
        let effects = c.handle(Event::Escape, t);
        assert!(effects.contains(&Effect::Keyboard(surface, KeyboardInteractivity::Exclusive)));
        assert!(c.focused(BAR, Item::Calendar) && c.open_item().is_none());
        c.handle(Event::ItemActivate, t);
        assert!(c.shows(BAR, Item::Calendar));
        // A second click closes it.
        c.handle(press(), t);
        assert_eq!(c.open_item(), None);
    }

    #[test]
    fn tab_reaches_the_meetings_card_then_its_button_then_the_agents_group() {
        let mut c = Controller::default();
        let agents = window::Id::unique();
        let card = window::Id::unique();
        c.place(
            BAR,
            Form::Full,
            window::Id::unique(),
            (
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
                agents,
            ),
        );
        c.set_meeting(BAR, Some(card));
        c.motion = false;
        let t = Instant::now();
        let card_rect = Rectangle::new(Point::new(-185.0, 100.0), Size::new(172.0, 44.0));
        let join = Rectangle::new(Point::new(-50.0, 107.0), Size::new(30.0, 30.0));
        let sources = Rectangle::new(Point::new(-60.0, 158.0), Size::new(45.0, 16.0));
        // As the card's surface tells them, the button first.
        c.set_items(
            BAR,
            MEETING_NAMESPACE,
            vec![(Item::Join, join), (Item::Meeting, card_rect)],
        );
        c.set_items(BAR, AGENTS_NAMESPACE, vec![(Item::Sources, sources)]);
        // Neither opens a popover: they act.
        assert!(!Item::Meeting.clicks() && !Item::Join.clicks());
        assert!(!Item::Meeting.hovers(Form::Full) && !Item::Join.hovers(Form::Full));
        assert!(Item::MeetingMark.clicks() && Item::MeetingMark.hovers(Form::Mini));
        // The keyboard on Sources (Esc in its popover), back up: the card,
        // which its surface takes, then its button, then Sources again.
        c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Sources,
                rect: sources,
            },
            t,
        );
        c.handle(Event::Escape, t);
        let effects = no_ring(c.handle(Event::ItemStep { back: false }, t));
        assert!(c.focused(BAR, Item::Meeting));
        assert!(effects.contains(&Effect::Keyboard(card, KeyboardInteractivity::Exclusive)));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Join));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Sources));
        // The card gone, its items go with it.
        c.set_meeting(BAR, None);
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Sources));
    }

    #[test]
    fn tab_goes_through_todays_lines_and_its_more_which_act() {
        let mut c = Controller::default();
        let today = window::Id::unique();
        c.place(
            BAR,
            Form::Full,
            window::Id::unique(),
            (
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
                window::Id::unique(),
            ),
        );
        c.set_today(BAR, Some(today));
        c.motion = false;
        let t = Instant::now();
        let line =
            |i: f32| Rectangle::new(Point::new(-190.0, 176.0 + 20.0 * i), Size::new(184.0, 20.0));
        let card = Rectangle::new(Point::new(-185.0, 100.0), Size::new(172.0, 44.0));
        c.set_items(BAR, MEETING_NAMESPACE, vec![(Item::Meeting, card)]);
        c.set_items(
            BAR,
            TODAY_NAMESPACE,
            vec![
                (Item::TodayLine(0), line(0.0)),
                (Item::TodayLine(1), line(1.0)),
                (Item::TodayMore, line(2.0)),
            ],
        );
        assert!(!Item::TodayLine(0).clicks() && !Item::TodayMore.clicks());
        assert!(!Item::TodayLine(0).hovers(Form::Full));
        // From the card (it had the keyboard), down Today's rows and round.
        c.ring.put(BAR, Item::Meeting, card);
        let effects = no_ring(c.handle(Event::ItemStep { back: false }, t));
        assert!(c.focused(BAR, Item::TodayLine(0)));
        assert!(effects.contains(&Effect::Keyboard(today, KeyboardInteractivity::Exclusive)));
        c.handle(Event::ItemStep { back: false }, t);
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::TodayMore));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Meeting));
    }

    #[test]
    fn a_tile_goes_to_its_desktop_and_the_keyboard_reaches_the_tiles_from_the_rows_under_them() {
        let mut c = Controller::default();
        let (levels, connections, desktops) = (
            window::Id::unique(),
            window::Id::unique(),
            window::Id::unique(),
        );
        c.place(
            BAR,
            Form::Full,
            window::Id::unique(),
            (levels, connections, desktops, window::Id::unique()),
        );
        c.motion = false;
        let t = Instant::now();
        let tile = |i: u8| {
            Rectangle::new(
                Point::new(
                    -185.0 + 44.0 * f32::from(i % 4),
                    700.0 + 36.0 * f32::from(i / 4),
                ),
                Size::new(38.0, 30.0),
            )
        };
        let wifi = Rectangle::new(Point::new(-190.0, 800.0), Size::new(184.0, 30.0));
        c.set_items(
            BAR,
            DESKTOPS_NAMESPACE,
            (0..6)
                .map(|i| (Item::Desktop { row: 0, index: i }, tile(i)))
                .collect(),
        );
        c.set_items(BAR, CONNECTIONS_NAMESPACE, vec![(Item::Wifi, wifi)]);
        c.set_items(BAR, LEVELS_NAMESPACE, vec![(Item::Sound, gauge())]);
        // A click: that desktop, no popover.
        let effects = c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Desktop { row: 0, index: 4 },
                rect: tile(4),
            },
            t,
        );
        let go = |i| Effect::Desktop {
            bar: BAR.to_owned(),
            to: DesktopTo::At(i),
        };
        assert_eq!(effects, vec![go(4)]);
        assert_eq!(c.open_item(), None);
        // The keyboard on the Wi-Fi's row; Shift+Tab: up to the last tile,
        // its surface taking the keyboard.
        c.handle(
            Event::ItemPress {
                bar: BAR.to_owned(),
                item: Item::Wifi,
                rect: wifi,
            },
            t,
        );
        c.handle(Event::Escape, t);
        assert!(c.focused(BAR, Item::Wifi));
        let effects = no_ring(c.handle(Event::ItemStep { back: true }, t));
        assert_eq!(
            effects,
            vec![
                Effect::Keyboard(connections, KeyboardInteractivity::None),
                Effect::Keyboard(desktops, KeyboardInteractivity::Exclusive),
                Effect::Wake(CONNECTIONS_NAMESPACE),
                Effect::Wake(DESKTOPS_NAMESPACE),
            ]
        );
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 5 }));
        // Up again, along the row: the same surface.
        assert_eq!(
            no_ring(c.handle(Event::ItemStep { back: true }, t)),
            vec![Effect::Wake(DESKTOPS_NAMESPACE)]
        );
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 4 }));
        // Enter: that desktop, the keyboard kept.
        assert_eq!(c.handle(Event::ItemActivate, t), vec![go(4)]);
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 4 }));
        // From the first tile back round to the levels' last item.
        for _ in 0..4 {
            c.handle(Event::ItemStep { back: true }, t);
        }
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 0 }));
        let effects = no_ring(c.handle(Event::ItemStep { back: true }, t));
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::Exclusive)));
        assert!(c.focused(BAR, Item::Sound));
        // Esc lets it go from there.
        let effects = c.handle(Event::ItemRelease, t);
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::None)));
    }

    /// A window's menu (#726): its top on the line right-clicked, wherever
    /// the line is in the bar; low in it, pushed up to stay whole, never
    /// sent to the bar's foot.
    #[test]
    fn a_windows_menu_opens_with_its_top_on_its_line() {
        let (mut c, _) = controller(Form::Full);
        let t = Instant::now();
        // Not shown yet: 200 px tall.
        let height = Item::WindowMenu(7).height(BAR, Form::Full, true) as f32;
        assert!(!Item::WindowMenu(7).on_bottom());
        let line = |y: f32| Rectangle::new(Point::new(-190.0, y), Size::new(184.0, 30.0));
        let mut tops = Vec::new();
        for y in [120.0, 500.0, HEIGHT - 40.0] {
            let opened = c.handle(
                Event::Open {
                    bar: BAR.to_owned(),
                    item: Item::WindowMenu(7),
                    rect: line(y),
                },
                t,
            );
            assert!(
                opened.iter().any(|e| matches!(e, Effect::Open { .. })),
                "{opened:?}"
            );
            let Phase::Open(open) = &c.phase else {
                panic!("not open");
            };
            tops.push(open.rect.y);
            assert!(open.rect.y + open.rect.height <= HEIGHT, "{:?}", open.rect);
        }
        assert_eq!(tops, [120.0, 500.0, HEIGHT - height]);
    }

    /// A full bar for `Super+B` (bar.md §11): its clock's group on the bar's
    /// own surface, three agents' rows, the desktops' tiles in a row, the
    /// sound and the brightness, the footer's four buttons in a row.
    fn keyboard_bar(form: Form) -> (Controller, [window::Id; 5]) {
        let mut c = Controller::default();
        let ids = [(); 5].map(|()| window::Id::unique());
        let [surface, levels, connections, desktops, agents] = ids;
        c.place(BAR, form, surface, (levels, connections, desktops, agents));
        let footer = window::Id::unique();
        c.set_footer(BAR, footer);
        c.motion = false;
        let at = |x: f32, y: f32, w: f32, h: f32| Rectangle::new(Point::new(x, y), Size::new(w, h));
        c.set_items(
            BAR,
            CLOCK_NAMESPACE,
            vec![(Item::Calendar, at(-184.0, 14.0, 172.0, 77.0))],
        );
        if form == Form::Full {
            c.set_items(
                BAR,
                AGENTS_NAMESPACE,
                vec![
                    (Item::Secretary, at(-190.0, 300.0, 184.0, 40.0)),
                    (Item::Agent(7), at(-190.0, 344.0, 184.0, 30.0)),
                    (Item::Agent(8), at(-190.0, 378.0, 184.0, 30.0)),
                    (Item::Agent(9), at(-190.0, 412.0, 184.0, 30.0)),
                ],
            );
        } else {
            c.set_items(
                BAR,
                AGENTS_NAMESPACE,
                vec![(Item::Secretary, at(-50.0, 300.0, 44.0, 44.0))],
            );
        }
        c.set_items(
            BAR,
            DESKTOPS_NAMESPACE,
            (0..3)
                .map(|i| {
                    let x = -185.0 + 44.0 * f32::from(i);
                    (Item::Desktop { row: 0, index: i }, at(x, 700.0, 38.0, 30.0))
                })
                .collect(),
        );
        c.set_items(
            BAR,
            LEVELS_NAMESPACE,
            vec![
                (Item::Sound, at(-190.0, 900.0, 184.0, 30.0)),
                (Item::Brightness, at(-190.0, 934.0, 184.0, 30.0)),
            ],
        );
        c.set_items(
            BAR,
            FOOTER_NAMESPACE,
            [Item::Clipboard, Item::Notifications, Item::Menu, Item::User]
                .into_iter()
                .enumerate()
                .map(|(i, item)| (item, at(-190.0 + 46.0 * i as f32, 1020.0, 40.0, 34.0)))
                .collect(),
        );
        (c, ids)
    }

    #[test]
    fn super_b_lands_on_the_waiting_agent_else_the_stuck_one_else_the_clock() {
        let t = Instant::now();
        let (mut c, [surface, _, _, _, agents]) = keyboard_bar(Form::Full);
        // Agent 8 waits, 9 is stuck: the ring on 8, the agents' surface
        // holding the keyboard.
        let effects = no_ring(c.enter(BAR, &[Item::Agent(8), Item::Agent(9)], t));
        assert_eq!(
            effects,
            vec![
                Effect::Keyboard(agents, KeyboardInteractivity::Exclusive),
                Effect::Wake(AGENTS_NAMESPACE),
            ]
        );
        assert!(c.focused(BAR, Item::Agent(8)));
        // Only a stuck one: on it.
        c.enter(BAR, &[Item::Agent(9)], t);
        assert!(c.focused(BAR, Item::Agent(9)));
        // Nobody waits nor is stuck: on the clock, the bar's own surface
        // taking the keyboard, the agents' letting it go.
        let effects = no_ring(c.enter(BAR, &[], t));
        assert_eq!(
            effects,
            vec![
                Effect::Keyboard(agents, KeyboardInteractivity::None),
                Effect::Wake(AGENTS_NAMESPACE),
                Effect::Keyboard(surface, KeyboardInteractivity::Exclusive),
                Effect::Wake(CLOCK_NAMESPACE),
            ]
        );
        assert!(c.focused(BAR, Item::Calendar));
        // An agent asked for that the bar does not show (hidden in "+ N"):
        // the clock.
        c.enter(BAR, &[Item::Agent(42)], t);
        assert!(c.focused(BAR, Item::Calendar));
        // A bar unknown: nothing.
        assert!(
            c.enter("DP-9", &[], t)
                .iter()
                .all(|e| !matches!(e, Effect::Keyboard(_, KeyboardInteractivity::Exclusive)))
        );
    }

    #[test]
    fn in_the_mini_form_super_b_lands_on_the_secretary_while_her_badge_shows() {
        let t = Instant::now();
        let (mut c, _) = keyboard_bar(Form::Mini);
        c.enter(BAR, &[Item::Agent(8), Item::Secretary], t);
        assert!(c.focused(BAR, Item::Secretary));
        c.enter(BAR, &[], t);
        assert!(c.focused(BAR, Item::Calendar));
    }

    #[test]
    fn the_keys_walk_every_item_in_the_drawn_order_and_round() {
        let t = Instant::now();
        let (mut c, _) = keyboard_bar(Form::Full);
        c.enter(BAR, &[], t);
        let order = [
            Item::Calendar,
            Item::Secretary,
            Item::Agent(7),
            Item::Agent(8),
            Item::Agent(9),
            Item::Desktop { row: 0, index: 0 },
            Item::Desktop { row: 0, index: 1 },
            Item::Desktop { row: 0, index: 2 },
            Item::Sound,
            Item::Brightness,
            Item::Clipboard,
            Item::Notifications,
            Item::Menu,
            Item::User,
        ];
        // Down (or Tab) visits each once, then round to the clock.
        for item in order.iter().skip(1).chain([&Item::Calendar]) {
            c.handle(Event::ItemStep { back: false }, t);
            assert!(c.focused(BAR, *item), "{item:?}");
        }
        // Up the reverse.
        for item in order.iter().rev() {
            c.handle(Event::ItemStep { back: true }, t);
            assert!(c.focused(BAR, *item), "{item:?}");
        }
        // End, Home.
        c.handle(Event::ItemEdge { last: true }, t);
        assert!(c.focused(BAR, Item::User));
        c.handle(Event::ItemEdge { last: false }, t);
        assert!(c.focused(BAR, Item::Calendar));
    }

    #[test]
    fn left_and_right_go_along_a_row_and_step_a_level() {
        let t = Instant::now();
        let (mut c, _) = keyboard_bar(Form::Full);
        c.enter(BAR, &[], t);
        for _ in 0..6 {
            c.handle(Event::ItemStep { back: false }, t);
        }
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 1 }));
        // Along the tiles, stopping at their ends.
        assert_eq!(
            no_ring(c.handle(Event::ItemSide { right: true }, t)),
            vec![Effect::Wake(DESKTOPS_NAMESPACE)]
        );
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 2 }));
        assert!(no_ring(c.handle(Event::ItemSide { right: true }, t)).is_empty());
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 2 }));
        c.handle(Event::ItemSide { right: false }, t);
        c.handle(Event::ItemSide { right: false }, t);
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 0 }));
        // Enter switches desktop, the ring stays.
        let go = c.handle(Event::ItemActivate, t);
        assert!(matches!(go[..], [Effect::Desktop { .. }]), "{go:?}");
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 0 }));
        // On the sound: its level by 5 %, nothing opening.
        c.handle(Event::ItemStep { back: false }, t);
        c.handle(Event::ItemStep { back: false }, t);
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Sound));
        assert_eq!(
            no_ring(c.handle(Event::ItemSide { right: true }, t)),
            vec![Effect::Level(hocus_link::LevelKey::VolumeUp)]
        );
        assert_eq!(
            no_ring(c.handle(Event::ItemSide { right: false }, t)),
            vec![Effect::Level(hocus_link::LevelKey::VolumeDown)]
        );
        assert_eq!(c.open_item(), None);
        // The footer's row.
        c.handle(Event::ItemEdge { last: true }, t);
        c.handle(Event::ItemSide { right: false }, t);
        assert!(c.focused(BAR, Item::Menu));
    }

    #[test]
    fn enter_opens_with_the_first_control_and_esc_steps_back() {
        let t = Instant::now();
        let (mut c, [_, levels, _, _, agents]) = keyboard_bar(Form::Full);
        c.enter(BAR, &[Item::Agent(7)], t);
        // To the sound, then Enter: its popover opens as a click's, its
        // first control under the ring.
        for _ in 0..6 {
            c.handle(Event::ItemStep { back: false }, t);
        }
        assert!(c.focused(BAR, Item::Sound));
        let effects = c.handle(Event::ItemActivate, t);
        let (id, keyboard) = opened(&effects).expect("open");
        assert_eq!(keyboard, KeyboardInteractivity::Exclusive);
        assert!(effects.contains(&Effect::FirstControl(id)), "{effects:?}");
        assert_eq!(c.open_item(), Some(Item::Sound));
        // Esc: closed, the ring back on the sound, its surface holding the
        // keyboard.
        let effects = c.handle(Event::Escape, t);
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::Exclusive)));
        assert!(c.focused(BAR, Item::Sound));
        assert_eq!(c.open_item(), None);
        // Esc again: the keyboard goes back.
        let effects = c.handle(Event::ItemRelease, t);
        assert!(effects.contains(&Effect::Keyboard(levels, KeyboardInteractivity::None)));
        assert!(!c.focused(BAR, Item::Sound));
        // `Super+B` past the last bar, with a popover open: it closes, the
        // item lets go.
        c.enter(BAR, &[Item::Agent(7)], t);
        c.handle(Event::ItemActivate, t);
        let effects = c.leave(t);
        assert!(effects.contains(&Effect::Remove(id)) || c.open_item().is_none());
        assert_eq!(c.open_item(), None);
        assert!(!c.focused(BAR, Item::Agent(7)));
        let _ = agents;
    }

    #[test]
    fn an_item_gone_under_the_ring_steps_from_where_it_was() {
        let t = Instant::now();
        let (mut c, _) = keyboard_bar(Form::Full);
        c.enter(BAR, &[Item::Agent(8)], t);
        // Agent 8 ends: its row goes.
        let at = |y: f32| Rectangle::new(Point::new(-190.0, y), Size::new(184.0, 30.0));
        c.set_items(
            BAR,
            AGENTS_NAMESPACE,
            vec![(Item::Agent(7), at(344.0)), (Item::Agent(9), at(378.0))],
        );
        assert!(c.focused(BAR, Item::Agent(9)));
        // The whole group goes but one row: the ring on it.
        c.set_items(BAR, AGENTS_NAMESPACE, vec![(Item::Agent(7), at(344.0))]);
        assert!(c.focused(BAR, Item::Agent(7)));
        c.handle(Event::ItemStep { back: false }, t);
        assert!(c.focused(BAR, Item::Desktop { row: 0, index: 0 }));
    }
}
