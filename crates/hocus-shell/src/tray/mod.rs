//! The tray (bar.md §4 group 11, 5.14, variant A): the applications'
//! icons, StatusNotifierItem's, just above the footer, always visible: a
//! row in the full form, a column in the mini.
//!
//! The desktop serves `org.kde.StatusNotifierWatcher` (and
//! `org.freedesktop.StatusNotifierWatcher`) on the session bus the
//! compositor names, as the notifications' server does, and is its own
//! host ([`watcher`], a thread of its own). Each item's properties are read
//! there and its icon decoded there, at the bars' screens' scale, never on
//! the thread that draws; what the bar shows is published here
//! ([`Shown`]), in the order the items first registered, `Passive` ones
//! left out. The bar draws them on a surface of its own ([`draw`]), opened
//! while an icon shows; a change of icon draws it alone, an icon coming or
//! going places the bar's groups again ([`effects`]). A click asks the
//! thread ([`ask`]): `Activate`, `SecondaryActivate`, `Scroll`, the menu
//! (`com.canonical.dbusmenu`, [`menu`]) read before its popover opens
//! ([`crate::popover::tray`]). Hovering an icon 400 ms shows its tooltip
//! ([`tip`]).

pub mod draw;
pub mod menu;
mod pixmap;
pub mod tip;
mod watcher;

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{TRAY_NAMESPACE, ToCompositor};
use iced::{Rectangle, advanced::image};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::Message;

/// An icon's size, logical pixels.
pub const ICON: f32 = 22.0;

/// An application's icon, as the bar shows it.
#[derive(Debug, Clone)]
pub struct Shown {
    /// The item's number, from 1 as they register: its popover's item.
    pub key: u32,
    /// Its `Title`, else its `Id`.
    pub name: String,
    /// What its tooltip says after its name: `ToolTip`'s title, else its
    /// text; empty, its name alone.
    pub tip: String,
    /// The icon it shows now (its attention icon while it needs
    /// attention, when it has one), decoded at the bars' scale.
    pub icon: Option<image::Handle>,
    /// Its overlay, drawn over the icon's bottom right.
    pub overlay: Option<image::Handle>,
    /// When it first showed, for its pop-in.
    pub since: Instant,
}

impl PartialEq for Shown {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Option<image::Handle>, b: &Option<image::Handle>| {
            a.as_ref().map(image::Handle::id) == b.as_ref().map(image::Handle::id)
        };
        self.key == other.key
            && self.name == other.name
            && self.tip == other.tip
            && same(&self.icon, &other.icon)
            && same(&self.overlay, &other.overlay)
    }
}

static SHOWN: Mutex<Vec<Shown>> = Mutex::new(Vec::new());

/// What the bar shows now, in order.
pub fn shown() -> Vec<Shown> {
    SHOWN.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The icons the bar shows are now `items`: those that came pop in from
/// now; one coming or going places the bar's groups again, an icon's
/// change draws the tray alone.
fn publish(mut items: Vec<Shown>) {
    let now = Instant::now();
    let rows = {
        let mut shown = SHOWN.lock().unwrap_or_else(PoisonError::into_inner);
        for item in &mut items {
            item.since = shown
                .iter()
                .find(|s| s.key == item.key)
                .map_or(now, |s| s.since);
        }
        if *shown == items {
            return;
        }
        let keys = |list: &[Shown]| list.iter().map(|s| s.key).collect::<Vec<_>>();
        let rows = keys(&shown) != keys(&items);
        *shown = items;
        rows
    };
    let names: Vec<String> = shown().iter().map(|s| s.name.clone()).collect();
    tracing::debug!(?names, "the tray's icons");
    let rows_changed = rows;
    if rows_changed {
        change(Change::Rows);
    } else {
        tip::changed();
        wake();
    }
}

/// The tray's surfaces draw again.
fn wake() {
    if !crate::popover::tell(ToCompositor::Wake(TRAY_NAMESPACE.to_owned())) {
        change(Change::Alone);
    }
}

// --- What the bar asks --------------------------------------------------------------

/// What the bar asks of an item, by its key; `bar` and `rect` (the icon,
/// in the bar's frame) say where its menu opens.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// A left click: `Activate`, or its menu for an item that is one (or
    /// does not activate).
    Activate {
        key: u32,
        bar: String,
        rect: Rectangle,
    },
    /// A middle click.
    Secondary { key: u32 },
    /// The wheel: `delta` (120 a notch, up positive), across or down.
    Scroll {
        key: u32,
        delta: i32,
        horizontal: bool,
    },
    /// A right click: its menu, read then opened (else `ContextMenu`).
    Menu {
        key: u32,
        bar: String,
        rect: Rectangle,
    },
    /// A row of its menu chosen.
    Event { key: u32, id: i32 },
    /// The bars' screens' largest scale changed: the icons are decoded
    /// again.
    Rescale,
}

static ASKS: Mutex<Option<UnboundedSender<Ask>>> = Mutex::new(None);

/// Asks the tray's thread, if it runs.
pub fn ask(ask: Ask) {
    tracing::debug!(?ask, "the tray asks");
    if let Some(asks) = ASKS.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let _ = asks.send(ask);
    }
}

/// Serves the watcher on the bus the compositor names, if it names one.
pub fn start() {
    let (sender, asks) = unbounded_channel();
    *ASKS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let (changes, taken) = unbounded_channel();
    *CHANGES.lock().unwrap_or_else(PoisonError::into_inner) = Some(changes);
    *TAKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(taken);
    watcher::start(asks);
}

/// The largest scale of the bars' screens, which the icons are decoded
/// for.
pub(crate) fn scale() -> f32 {
    crate::popover::with(|c| c.bar_forms())
        .iter()
        .map(|(bar, _)| crate::popover::kit::scale(bar))
        .fold(1.0_f32, f32::max)
}

static SCALE: Mutex<f32> = Mutex::new(1.0);

/// A bar came, or its screen's scale changed: the icons follow the
/// largest.
pub fn scales_changed() {
    let now = scale();
    let mut was = SCALE.lock().unwrap_or_else(PoisonError::into_inner);
    if (*was - now).abs() > f32::EPSILON {
        *was = now;
        drop(was);
        ask(Ask::Rescale);
    }
}

// --- The menu open --------------------------------------------------------------------

/// The menu read last, and whose it is.
static MENU: Mutex<Option<(u32, menu::Entry)>> = Mutex::new(None);

/// Item `key`'s menu, as read last.
pub fn menu(key: u32) -> Option<menu::Entry> {
    MENU.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(k, _)| *k == key)
        .map(|(_, m)| m.clone())
}

/// Item `key`'s menu is `root`: shown if its popover is open.
fn set_menu(key: u32, root: menu::Entry) {
    *MENU.lock().unwrap_or_else(PoisonError::into_inner) = Some((key, root));
    crate::popover::changed(crate::popover::Item::Tray(key));
}

/// Whether item `key`'s menu shows.
fn menu_open(key: u32) -> bool {
    crate::popover::with(|c| c.open_item()) == Some(crate::popover::Item::Tray(key))
}

// --- To the shell -----------------------------------------------------------------------

/// What the tray tells the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// An icon came or went: the bars' groups are placed again.
    Rows,
    /// A shell alone: every surface draws again.
    Alone,
}

static CHANGES: Mutex<Option<UnboundedSender<Change>>> = Mutex::new(None);
static TAKEN: Mutex<Option<UnboundedReceiver<Change>>> = Mutex::new(None);

fn change(change: Change) {
    if let Some(changes) = CHANGES.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let _ = changes.send(change);
    }
}

/// The tray's changes, as messages: [`Message::Tray`] when an icon comes
/// or goes.
pub fn effects() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(4, async |mut output| {
        let Some(mut changes) = TAKEN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            std::future::pending::<()>().await;
            return;
        };
        while let Some(change) = changes.recv().await {
            let message = match change {
                Change::Rows => Message::Tray,
                Change::Alone => Message::Levels,
            };
            if output.send(message).await.is_err() {
                return;
            }
        }
    })
}

/// The tray's surfaces draw again, after their places changed.
pub fn placed() {
    wake();
}
