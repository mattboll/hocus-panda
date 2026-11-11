//! The Bluetooth page (6.7, settings.md "Bluetooth"): the switch, the
//! paired devices, those around to pair, all in BlueZ.
//!
//! BlueZ is followed and asked on a thread of its own (`hocus-bluetooth`,
//! as the bar does) and comes to the window as messages; what the user
//! asks goes to that thread, never waited for on the window's. Nothing is
//! written in the file: BlueZ keeps the pairings, the names and the trust.
//!
//! While the page shows (and the window is open), BlueZ searches for
//! devices around, the computer is visible to others, and the Settings'
//! agent is BlueZ's default: what it asks (a code to compare, to type, a
//! PIN, a device pairing by itself) shows in a sheet over the page, and the
//! user's answer goes back to it. Leaving the page or closing the window
//! stops all three. Connecting, disconnecting and pairing say themselves
//! on their row; turning Bluetooth off, renaming a device and letting it
//! connect by itself toast with "Annuler" and join `Ctrl+Z`; forgetting
//! one asks first, and cannot be taken back.

mod view;

use hocus_words::settings::app_bluetooth as words;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use hocus_bluetooth::{Answer, Ask, Asked, Asks, Bluetooth, Device, Failure, Step};
use iced::{
    Task,
    futures::channel::mpsc::UnboundedSender,
    keyboard::{Key, Modifiers, key::Named},
    widget::operation,
};

use super::{App, MENU_IN, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{bluetooth as model, model::SectionId};

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
    /// A paired device's "⋯" menu opened (or closed), by its path.
    Menu(String),
    Item(Item),
    /// The name typed while a device is renamed, saved, or let go.
    RenameText(String),
    RenameSave,
    RenameCancel,
    /// Forgetting a device asked, confirmed, or not.
    Forget(String),
    ForgetConfirm,
    ForgetCancel,
    /// The sheet's answer to what the agent asks.
    Answer(Answer),
    /// The PIN typed in its sheet, and given.
    PinText(String),
    PinGive,
    /// The pairing under way given up, or what the agent asks refused.
    CancelPairing,
}

/// What a paired device's menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Rename,
    Trust,
    Forget,
}

impl Item {
    pub const ALL: [Self; 3] = [Self::Rename, Self::Trust, Self::Forget];
}

/// A choice made on the page, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Bluetooth turned on or off.
    Power { on: bool },
    /// A device's name given: what it was, and is.
    Rename {
        path: String,
        before: String,
        after: String,
    },
    /// A device let to connect by itself, or not.
    Trust {
        path: String,
        name: String,
        on: bool,
    },
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
    /// The "⋯" menu open, by its device's path, since then, and the
    /// keyboard's item in it.
    pub(super) menu: Option<(String, Instant)>,
    pub(super) menu_index: usize,
    /// The device renamed: its path, the name typed, its name before.
    pub(super) renaming: Option<(String, String, String)>,
    /// The device whose forgetting is asked: its path and its name.
    pub(super) forgetting: Option<(String, String)>,
    /// The PIN typed in its sheet, and the question it answers.
    pub(super) pin: (u64, String),
    /// The question last answered here: its sheet closes at once, before
    /// BlueZ's thread says it is over (a click beside it then reaches the
    /// page, not the sheet's scrim).
    answered: Option<u64>,
    /// A device to pair once BlueZ lists it, by its address (`--pair`,
    /// the bar's "dans les Réglages").
    pair_wanted: Option<String>,
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
        hocus_bluetooth::start(incoming, hocus_bluetooth::Options::CODES, move |bt| {
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

    /// The paired device `path`.
    pub(super) fn device(&self, path: &str) -> Option<&Device> {
        self.bt.as_ref()?.mine().find(|d| d.path == path)
    }

    /// Why `path`'s last attempt failed, while it shows.
    pub(super) fn failure(&self, path: &str) -> Option<Failure> {
        let attempt = self.bt.as_ref()?.attempt.as_ref()?;
        match attempt.step {
            // Cancelled by the user: said nowhere.
            Step::Failed(Failure::Stopped) => None,
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
                if before.as_ref().is_some_and(|b| b.powered != powered) {
                    self.knobs.insert(model::SWITCH, Instant::now());
                }
                let held = self.bluetooth_held();
                self.bluetooth.relist(held);
                // Gone (forgotten elsewhere): its menu, its field, its sheet.
                let known: Vec<String> = self
                    .bluetooth
                    .mine()
                    .iter()
                    .map(|d| d.path.clone())
                    .collect();
                let gone = |path: &String| !known.contains(path);
                if self.bluetooth.menu.as_ref().is_some_and(|(p, _)| gone(p)) {
                    self.bluetooth.menu = None;
                }
                if self
                    .bluetooth
                    .renaming
                    .as_ref()
                    .is_some_and(|(p, ..)| gone(p))
                {
                    self.bluetooth.renaming = None;
                }
                if self
                    .bluetooth
                    .forgetting
                    .as_ref()
                    .is_some_and(|(p, _)| gone(p))
                {
                    self.bluetooth.forgetting = None;
                }
                self.bluetooth_pair_wanted();
                // A PIN asked: its field takes the keyboard.
                let request = self.bluetooth.bt.as_ref().and_then(|b| b.request.clone());
                let was = before.and_then(|b| b.request).map(|r| r.id);
                match request {
                    Some(r)
                        if Some(r.id) != was && matches!(r.asked, Asked::Pin | Asked::Passkey) =>
                    {
                        self.bluetooth.pin = (r.id, String::new());
                        operation::focus(view::PIN)
                    }
                    _ => Task::none(),
                }
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
                self.undo.push(super::Undo::Bluetooth(Choice::Power { on }));
                self.redo.clear();
                let text = words::BLUETOOTH_OFF.get(self.lang).to_owned();
                self.toast(text, ToastAction::Undo, TOAST)
            }
            Msg::Menu(path) => {
                self.menu = None;
                self.bluetooth.menu = match self.bluetooth.menu.take() {
                    Some((open, _)) if open == path => None,
                    _ => {
                        self.bluetooth.menu_index = 0;
                        Some((path, Instant::now()))
                    }
                };
                Task::none()
            }
            Msg::Item(item) => {
                let Some((path, _)) = self.bluetooth.menu.take() else {
                    return Task::none();
                };
                self.bluetooth_item(&path, item)
            }
            Msg::RenameText(text) => {
                if let Some((_, typed, _)) = self.bluetooth.renaming.as_mut() {
                    *typed = text;
                }
                Task::none()
            }
            Msg::RenameSave => {
                let Some((path, typed, before)) = self.bluetooth.renaming.take() else {
                    return Task::none();
                };
                let after = typed.trim().to_owned();
                if after == before {
                    return Task::none();
                }
                self.bluetooth_choose(Choice::Rename {
                    path,
                    before,
                    after,
                })
            }
            Msg::RenameCancel => {
                self.bluetooth.renaming = None;
                Task::none()
            }
            Msg::Forget(path) => {
                self.bluetooth.menu = None;
                if let Some(device) = self.bluetooth.device(&path) {
                    self.bluetooth.forgetting = Some((path, device.name.clone()));
                }
                Task::none()
            }
            Msg::ForgetConfirm => {
                let Some((path, name)) = self.bluetooth.forgetting.take() else {
                    return Task::none();
                };
                self.bluetooth.ask(Ask::Forget(path));
                // A pairing's key cannot be brought back: no "Annuler".
                let text = words::forgotten(self.lang, &name);
                self.toast(text, ToastAction::None, TOAST)
            }
            Msg::ForgetCancel => {
                self.bluetooth.forgetting = None;
                Task::none()
            }
            Msg::Answer(answer) => {
                if let Some(id) = self
                    .bluetooth
                    .bt
                    .as_ref()
                    .and_then(|b| b.request.as_ref())
                    .map(|r| r.id)
                {
                    self.bluetooth.ask(Ask::Answer(id, answer));
                    self.bluetooth.answered = Some(id);
                }
                Task::none()
            }
            Msg::PinText(text) => {
                let digits = matches!(
                    self.bluetooth
                        .bt
                        .as_ref()
                        .and_then(|b| b.request.as_ref())
                        .map(|r| &r.asked),
                    Some(Asked::Passkey)
                );
                if !digits || text.chars().all(|c| c.is_ascii_digit()) {
                    self.bluetooth.pin.1 = text.chars().take(16).collect();
                }
                Task::none()
            }
            Msg::PinGive => {
                let Some(request) = self.bluetooth.bt.as_ref().and_then(|b| b.request.clone())
                else {
                    return Task::none();
                };
                let typed = self.bluetooth.pin.1.trim().to_owned();
                let answer = match request.asked {
                    Asked::Pin if !typed.is_empty() => Answer::Pin(typed),
                    Asked::Passkey => match typed.parse::<u32>() {
                        Ok(key) if key <= 999_999 => Answer::Passkey(key),
                        _ => return Task::none(),
                    },
                    _ => return Task::none(),
                };
                self.bluetooth.ask(Ask::Answer(request.id, answer));
                self.bluetooth.answered = Some(request.id);
                Task::none()
            }
            Msg::CancelPairing => {
                self.bluetooth.ask(Ask::CancelPairing);
                self.bluetooth.answered = self
                    .bluetooth
                    .bt
                    .as_ref()
                    .and_then(|b| b.request.as_ref())
                    .map(|r| r.id);
                Task::none()
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

    /// What a paired device's menu does.
    fn bluetooth_item(&mut self, path: &str, item: Item) -> Task<Message> {
        let Some(device) = self.bluetooth.device(path).cloned() else {
            return Task::none();
        };
        match item {
            Item::Rename => {
                self.bluetooth.renaming =
                    Some((device.path.clone(), device.name.clone(), device.name));
                Task::batch([
                    operation::focus(view::RENAME),
                    operation::select_all(view::RENAME),
                ])
            }
            Item::Trust => self.bluetooth_choose(Choice::Trust {
                path: device.path,
                name: device.name,
                on: !device.trusted,
            }),
            Item::Forget => self.bluetooth_update(Msg::Forget(device.path)),
        }
    }

    /// A rename or a trust made: asked of BlueZ, toasted with "Annuler",
    /// kept for `Ctrl+Z`.
    fn bluetooth_choose(&mut self, choice: Choice) -> Task<Message> {
        let text = format!("{}.", self.bluetooth_apply(&choice, false, false));
        self.undo.push(super::Undo::Bluetooth(choice));
        self.redo.clear();
        self.toast(text, ToastAction::Undo, TOAST)
    }

    /// `choice` asked of BlueZ (its other side, `back`); what its toast
    /// says (as an undo or a redo says it, `undoing`).
    fn bluetooth_apply(&mut self, choice: &Choice, back: bool, undoing: bool) -> String {
        let lang = self.lang;
        match choice {
            Choice::Power { on } => {
                let on = on ^ back;
                self.bluetooth.ask(Ask::Power(on));
                match on {
                    true => words::BLUETOOTH_IS_BACK_ON.get(lang),
                    false => words::BLUETOOTH_IS_OFF_AGAIN.get(lang),
                }
                .to_owned()
            }
            Choice::Rename {
                path,
                before,
                after,
            } => {
                let name = if back { before } else { after };
                self.bluetooth.ask(Ask::Rename(path.clone(), name.clone()));
                // Empty: its own name again, which BlueZ says next.
                let said = if name.is_empty() {
                    words::ITS_OWN_NAME.get(lang).to_owned()
                } else {
                    words::text(lang, name)
                };
                match undoing {
                    false => words::renamed(lang, &said),
                    true => words::the_name_is_back_to(lang, &said),
                }
            }
            Choice::Trust { path, name, on } => {
                let on = on ^ back;
                self.bluetooth.ask(Ask::Trust(path.clone(), on));
                match on {
                    true => words::connects_automatically(lang, name),
                    false => words::no_longer_connects_automatically(lang, name),
                }
            }
        }
    }

    /// `choice` undone (or redone).
    pub(super) fn bluetooth_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let said = self.bluetooth_apply(&choice, !redo, true);
        if redo {
            self.undo.push(super::Undo::Bluetooth(choice));
        } else {
            self.redo.push(super::Undo::Bluetooth(choice));
        }
        self.go(Page::Section(SectionId::Bluetooth));
        let text = match redo {
            false => words::undone(self.lang, &said),
            true => words::redone(self.lang, &said),
        };
        self.toast(text, ToastAction::None, UNDONE)
    }

    /// The device `--pair` named paired as soon as BlueZ lists it.
    fn bluetooth_pair_wanted(&mut self) {
        let Some(address) = self.bluetooth.pair_wanted.clone() else {
            return;
        };
        if self.bluetooth.running().is_some() {
            return;
        }
        let found = self.bluetooth.bt.as_ref().and_then(|b| {
            b.nearby
                .iter()
                .chain(b.mine())
                .find(|d| d.address.eq_ignore_ascii_case(&address))
                .map(|d| (d.path.clone(), d.paired))
        });
        if let Some((path, paired)) = found {
            self.bluetooth.pair_wanted = None;
            if !paired {
                tracing::info!("the device asked for from the bar is paired");
                self.bluetooth.ask(Ask::Pair(path));
            }
        }
    }

    /// The page asked to pair device `address` (the bar's hand-off): it
    /// shows, and pairs it as soon as BlueZ lists it.
    pub(super) fn bluetooth_pair_address(&mut self, address: String) {
        self.go(Page::Section(SectionId::Bluetooth));
        self.bluetooth.pair_wanted = Some(address);
        self.bluetooth_pair_wanted();
    }

    /// Keys taken before the page's: the sheet's (`Enter` accepts, `Esc`
    /// cancels), the rename field's `Esc`, the menu's.
    pub(super) fn bluetooth_key_first(&mut self, key: &Key) -> Option<Task<Message>> {
        let Key::Named(named) = key.as_ref() else {
            return None;
        };
        if self.bluetooth.forgetting.is_some() {
            return match named {
                Named::Enter => Some(self.bluetooth_update(Msg::ForgetConfirm)),
                Named::Escape => Some(self.bluetooth_update(Msg::ForgetCancel)),
                _ => None,
            };
        }
        if let Some(request) = self.bluetooth_sheet_request() {
            let typed = matches!(request.asked, Asked::Pin | Asked::Passkey);
            return match (named, &request.asked) {
                (Named::Escape, _) => Some(self.bluetooth_update(if request.ours {
                    Msg::CancelPairing
                } else {
                    Msg::Answer(Answer::No)
                })),
                // A PIN's field gives it itself.
                (Named::Enter, Asked::Confirm(_) | Asked::Authorize) => {
                    Some(self.bluetooth_update(Msg::Answer(Answer::Yes)))
                }
                (Named::Enter, _) if typed => None,
                _ => None,
            };
        }
        if self.bluetooth.renaming.is_some() && named == Named::Escape {
            return Some(self.bluetooth_update(Msg::RenameCancel));
        }
        let path = self.bluetooth.menu.as_ref().map(|(p, _)| p.clone())?;
        match named {
            Named::ArrowDown => {
                self.bluetooth.menu_index =
                    (self.bluetooth.menu_index + 1).min(Item::ALL.len() - 1);
            }
            Named::ArrowUp => {
                self.bluetooth.menu_index = self.bluetooth.menu_index.saturating_sub(1)
            }
            Named::Enter | Named::Space => {
                let item = Item::ALL[self.bluetooth.menu_index.min(Item::ALL.len() - 1)];
                self.bluetooth.menu = None;
                return Some(self.bluetooth_item(&path, item));
            }
            Named::Escape => self.bluetooth.menu = None,
            _ => {}
        }
        Some(Task::none())
    }

    /// What the agent asks that shows in a sheet now (the page shown).
    pub(super) fn bluetooth_sheet_request(&self) -> Option<hocus_bluetooth::Request> {
        if self.page != Page::Section(SectionId::Bluetooth) {
            return None;
        }
        self.bluetooth
            .bt
            .as_ref()?
            .request
            .clone()
            .filter(|r| Some(r.id) != self.bluetooth.answered)
    }

    /// Whether a sheet's countdown shows: it draws each second.
    pub(super) fn bluetooth_ticking(&self) -> bool {
        self.bluetooth_sheet_request()
            .is_some_and(|r| matches!(r.asked, Asked::Confirm(_)) && r.until.is_some())
    }

    /// Whether the menu opens now.
    pub(super) fn bluetooth_menu_moving(&self, now: Instant) -> bool {
        !self.reduced_motion
            && self
                .bluetooth
                .menu
                .as_ref()
                .is_some_and(|(_, since)| now.saturating_duration_since(*since) < MENU_IN)
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
            self.bluetooth.menu = None;
            self.bluetooth.renaming = None;
            self.bluetooth.forgetting = None;
            self.bluetooth.pair_wanted = None;
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
    pub(super) fn bluetooth_key(
        &mut self,
        stop: Stop,
        named: Named,
        modifiers: Modifiers,
    ) -> Task<Message> {
        let press = matches!(named, Named::Enter | Named::Space);
        // On a paired device: its menu, rename it, forget it.
        if stop == Stop::Mine
            && let Some(path) = self
                .bluetooth
                .mine()
                .get(self.bluetooth.cursor.0)
                .map(|d| d.path.clone())
        {
            match named {
                Named::ContextMenu => return self.bluetooth_update(Msg::Menu(path)),
                Named::F10 if modifiers.shift() => return self.bluetooth_update(Msg::Menu(path)),
                Named::F2 => return self.bluetooth_item(&path, Item::Rename),
                Named::Delete => return self.bluetooth_update(Msg::Forget(path)),
                _ => {}
            }
        }
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
