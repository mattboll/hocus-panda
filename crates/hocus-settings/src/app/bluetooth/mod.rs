//! The Bluetooth page (6.7, settings.md "Bluetooth"): the switch, the
//! paired devices, those around to pair, all in BlueZ.
//!
//! BlueZ is followed and asked on a thread of its own (`hocus-bluetooth`,
//! as the bar does) and comes to the window as messages; what the user
//! asks goes to that thread, never waited for on the window's. Nothing is
//! written in the file: BlueZ keeps the pairings, the names and the trust.
//!
//! While the page shows (and the window is open), BlueZ searches for
//! devices around and the computer is visible to others; leaving the page
//! or closing the window stops both. Connecting, disconnecting and pairing
//! say themselves on their row; turning Bluetooth off toasts with
//! "Annuler" and joins `Ctrl+Z`.

mod view;

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use hocus_bluetooth::{Ask, Asks, Bluetooth, Device, Failure, Step};
use iced::{Task, futures::channel::mpsc::UnboundedSender, keyboard::key::Named};

use super::{App, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{
    bluetooth as model,
    lang::{Lang, w},
    model::SectionId,
};

/// The spinner's turn.
const TURN: Duration = Duration::from_millis(900);

/// What the window's thread asks through, for [`stop`].
static ASKS: OnceLock<Asks> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Box<Bluetooth>),
    /// The switch flipped.
    Power,
    Connect(String),
    Disconnect(String),
    Pair(String),
    /// The pointer over the devices around, or not: they hold their
    /// order meanwhile.
    Over(bool),
}

/// A choice made on the page, for its undo: Bluetooth turned on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub on: bool,
}

/// A control of the page that takes the keyboard, in `Tab`'s order: the
/// switch, then each list, one stop each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Switch,
    Mine,
    Nearby,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// Bluetooth as BlueZ last said it; `None` until it is heard.
    pub(super) bt: Option<Bluetooth>,
    asks: Option<Asks>,
    /// The search and being visible, as last asked.
    shown: bool,
    /// The devices around, by path, in the order shown: held while the
    /// pointer is over them, the keyboard is on them or something runs.
    order: Vec<String>,
    over: bool,
    /// The keyboard's row in the paired devices' and those around.
    pub(super) cursor: (usize, usize),
    /// The failures of attempts up to this one are no longer shown.
    dismissed: u64,
}

impl State {
    /// Follows BlueZ on a thread of its own: what it says comes to the
    /// window through `to`.
    pub fn start(to: UnboundedSender<Message>) -> Self {
        let (asks, incoming) = hocus_bluetooth::channel();
        let _ = ASKS.set(asks.clone());
        // Only what changed reaches the window: each message is an update
        // and a frame drawn.
        let mut last: Option<Bluetooth> = None;
        hocus_bluetooth::start(incoming, hocus_bluetooth::Options::default(), move |bt| {
            if last.as_ref() == Some(&bt) {
                return;
            }
            last = Some(bt.clone());
            let _ = to.unbounded_send(Message::Bluetooth(Msg::Heard(Box::new(bt))));
        });
        Self {
            asks: Some(asks),
            ..Self::default()
        }
    }

    fn ask(&self, ask: Ask) {
        let sent = self.asks.as_ref().is_some_and(|a| a.send(ask).is_ok());
        if !sent {
            tracing::warn!("BlueZ's thread has stopped: dropped");
        }
    }

    /// The devices around, in the order shown.
    pub(super) fn nearby(&self) -> Vec<&Device> {
        let Some(bt) = self.bt.as_ref() else {
            return Vec::new();
        };
        self.order
            .iter()
            .filter_map(|p| bt.nearby.iter().find(|d| d.path == *p))
            .collect()
    }

    /// The paired devices, those connected first.
    pub(super) fn mine(&self) -> Vec<&Device> {
        self.bt
            .as_ref()
            .map(|b| b.mine().collect())
            .unwrap_or_default()
    }

    /// The devices around listed again: in BlueZ's order (the strongest
    /// first), unless `held`, when those listed keep their places and
    /// newcomers join at the end.
    fn relist(&mut self, held: bool) {
        let now: Vec<String> = self
            .bt
            .as_ref()
            .map(|b| b.nearby.iter().map(|d| d.path.clone()).collect())
            .unwrap_or_default();
        if held {
            self.order.retain(|p| now.contains(p));
            for path in now {
                if !self.order.contains(&path) {
                    self.order.push(path);
                }
            }
        } else {
            self.order = now;
        }
    }

    /// The device something runs for, if anything does.
    pub(super) fn running(&self) -> Option<(&str, hocus_bluetooth::Action)> {
        self.bt.as_ref()?.running()
    }

    /// Why `path`'s last attempt failed, while it shows.
    pub(super) fn failure(&self, path: &str) -> Option<Failure> {
        let attempt = self.bt.as_ref()?.attempt.as_ref()?;
        match attempt.step {
            Step::Failed(failure) if attempt.path == path && attempt.id > self.dismissed => {
                Some(failure)
            }
            _ => None,
        }
    }
}

/// The window has closed: the search stops and the computer is hidden
/// before the program ends. Waits a little for it, never long.
pub fn stop() {
    let Some(asks) = ASKS.get() else {
        return;
    };
    let (done, said) = std::sync::mpsc::channel();
    if asks.send(Ask::Quit(done)).is_ok() {
        let _ = said.recv_timeout(Duration::from_millis(500));
    }
}

impl App {
    pub(super) fn bluetooth_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(bt) => {
                let before = self.bluetooth.bt.replace(*bt);
                let powered = self.bluetooth.bt.as_ref().is_some_and(|b| b.powered);
                if before.is_some_and(|b| b.powered != powered) {
                    self.knobs.insert(model::SWITCH, Instant::now());
                }
                let held = self.bluetooth_held();
                self.bluetooth.relist(held);
                Task::none()
            }
            Msg::Power => {
                let Some(bt) = self.bluetooth.bt.as_ref().filter(|b| b.adapter) else {
                    return Task::none();
                };
                let on = !bt.powered;
                self.bluetooth.ask(Ask::Power(on));
                if on {
                    return Task::none();
                }
                // Turned off: said, and taken back with "Annuler".
                self.undo.push(super::Undo::Bluetooth(Choice { on }));
                self.redo.clear();
                let text = w("Bluetooth coupé", "Bluetooth off")
                    .get(self.lang)
                    .to_owned();
                self.toast(text, ToastAction::Undo, TOAST)
            }
            Msg::Connect(path) => {
                if self.bluetooth.running().is_none() {
                    self.bluetooth.ask(Ask::Connect(path));
                }
                Task::none()
            }
            Msg::Disconnect(path) => {
                self.bluetooth.ask(Ask::Disconnect(path));
                Task::none()
            }
            Msg::Pair(path) => {
                if self.bluetooth.running().is_none() {
                    self.bluetooth.ask(Ask::Pair(path));
                }
                Task::none()
            }
            Msg::Over(over) => {
                self.bluetooth.over = over;
                let held = self.bluetooth_held();
                self.bluetooth.relist(held);
                Task::none()
            }
        }
    }

    /// Whether the devices around hold their order now.
    fn bluetooth_held(&self) -> bool {
        self.bluetooth.over
            || self.bluetooth.running().is_some()
            || self.focused_on(&super::Control::Bluetooth(Stop::Nearby))
    }

    fn focused_on(&self, control: &super::Control) -> bool {
        matches!(self.focus, super::Focus::Control(i) if self.controls().get(i) == Some(control))
    }

    /// `choice` undone (or redone): Bluetooth back on, or off again.
    pub(super) fn bluetooth_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let on = if redo { choice.on } else { !choice.on };
        self.bluetooth.ask(Ask::Power(on));
        if redo {
            self.undo.push(super::Undo::Bluetooth(choice));
        } else {
            self.redo.push(super::Undo::Bluetooth(choice));
        }
        self.go(Page::Section(SectionId::Bluetooth));
        let text = match (self.lang, on) {
            (Lang::French, true) => "Annulé : le Bluetooth est rallumé.",
            (Lang::English, true) => "Undone: Bluetooth is back on.",
            (Lang::French, false) => "Rétabli : le Bluetooth est coupé.",
            (Lang::English, false) => "Redone: Bluetooth is off again.",
        };
        self.toast(text.to_owned(), ToastAction::None, UNDONE)
    }

    /// The search and being visible follow the page: on while it shows in
    /// the open window, off otherwise. A failure said under a row is no
    /// longer said once the page is left.
    pub(super) fn bluetooth_want(&mut self) {
        let want =
            self.page == Page::Section(SectionId::Bluetooth) && self.life == super::Life::Open;
        if want == self.bluetooth.shown {
            return;
        }
        self.bluetooth.shown = want;
        self.bluetooth.ask(Ask::Discover(want));
        self.bluetooth.ask(Ask::Visible(want));
        if !want {
            self.bluetooth.dismissed = self
                .bluetooth
                .bt
                .as_ref()
                .and_then(|b| b.attempt.as_ref())
                .map_or(self.bluetooth.dismissed, |a| a.id);
        }
        tracing::info!(shown = want, "the Bluetooth page asks for the search");
    }

    /// Whether the page moves at `now`: a spinner turns (the search's
    /// first pass, a device asked for).
    pub(super) fn bluetooth_moving(&self) -> bool {
        if self.page != Page::Section(SectionId::Bluetooth) {
            return false;
        }
        self.bluetooth
            .bt
            .as_ref()
            .is_some_and(|b| b.powered && (b.searching || b.running().is_some()))
    }

    /// The spinners' angle at the frame's time, in turns.
    pub(super) fn bluetooth_turn(&self) -> f32 {
        static START: OnceLock<Instant> = OnceLock::new();
        if self.reduced_motion {
            return 0.0;
        }
        let start = *START.get_or_init(Instant::now);
        (self.now.saturating_duration_since(start).as_secs_f32() / TURN.as_secs_f32()).fract()
    }

    /// The page's controls, in `Tab`'s order.
    pub(super) fn bluetooth_stops(&self) -> Vec<Stop> {
        let Some(bt) = self.bluetooth.bt.as_ref().filter(|b| b.adapter) else {
            return Vec::new();
        };
        let mut stops = vec![Stop::Switch];
        if !bt.powered {
            return stops;
        }
        if !self.bluetooth.mine().is_empty() {
            stops.push(Stop::Mine);
        }
        if !self.bluetooth.nearby().is_empty() {
            stops.push(Stop::Nearby);
        }
        stops
    }

    /// A key on the page's control `stop`.
    pub(super) fn bluetooth_key(&mut self, stop: Stop, named: Named) -> Task<Message> {
        let press = matches!(named, Named::Enter | Named::Space);
        match stop {
            Stop::Switch if press => self.bluetooth_update(Msg::Power),
            Stop::Mine | Stop::Nearby => {
                let mine = stop == Stop::Mine;
                let count = if mine {
                    self.bluetooth.mine().len()
                } else {
                    self.bluetooth.nearby().len()
                };
                let cursor = if mine {
                    &mut self.bluetooth.cursor.0
                } else {
                    &mut self.bluetooth.cursor.1
                };
                match named {
                    Named::ArrowDown => *cursor = (*cursor + 1).min(count.saturating_sub(1)),
                    Named::ArrowUp => *cursor = cursor.saturating_sub(1),
                    _ if press => {
                        let at = *cursor;
                        let msg = if mine {
                            self.bluetooth.mine().get(at).map(|d| {
                                if d.connected {
                                    Msg::Disconnect(d.path.clone())
                                } else {
                                    Msg::Connect(d.path.clone())
                                }
                            })
                        } else {
                            self.bluetooth
                                .nearby()
                                .get(at)
                                .map(|d| Msg::Pair(d.path.clone()))
                        };
                        if let Some(msg) = msg {
                            return self.bluetooth_update(msg);
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Stop::Switch => Task::none(),
        }
    }

    /// The home's card.
    pub(super) fn bluetooth_card(&self) -> String {
        model::card(self.bluetooth.bt.as_ref(), self.lang)
    }

    /// The window goes: BlueZ stops searching and hides the computer now.
    pub(super) fn bluetooth_close(&mut self) {
        if self.bluetooth.shown {
            self.bluetooth.shown = false;
            self.bluetooth.ask(Ask::Discover(false));
            self.bluetooth.ask(Ask::Visible(false));
        }
    }
}
