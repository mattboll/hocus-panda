//! The Réseau page (6.6, settings.md "Réseau"): airplane mode, the Wi-Fi,
//! the network in use, the saved networks (a sub-page), the networks
//! around and the sheet that joins a protected one.
//!
//! NetworkManager is followed on a thread of its own (`hocus-network`, as
//! the bar does) and comes to the window as messages; what the user asks
//! goes back to that thread, never run on the window's. The radios'
//! rfkill is read and set on threads of their own too. Nothing is written
//! in the file: the page shows what NetworkManager and rfkill last said,
//! never what it asked. Airplane mode, the Wi-Fi's switch and a network
//! forgotten toast with "Annuler" and join `Ctrl+Z`; connecting and
//! disconnecting say themselves on their rows.

mod view;

use std::{
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Instant, SystemTime},
};

use hocus_network::{
    Ask, Details, Heard, Kept, Reply,
    model::{Failure, Progress, Security, Wifi},
    rfkill::{self, Block, Radios},
    secret::{self, Secret},
};
use iced::{
    Task, futures::channel::mpsc::UnboundedSender, keyboard::key::Named, widget::operation,
};

use super::{App, Doing, Focus, Message, Page, TOAST, ToastAction, UNDONE, Undo};
use crate::{
    lang::{Lang, w},
    model::SectionId,
    network as model,
};

/// The text fields' ids: the sheet's password, a saved network's.
pub(super) const SHEET_FIELD: &str = "network-sheet-password";
pub(super) const PASSWORD_FIELD: &str = "network-password";

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Arc<Details>),
    /// The radios as rfkill says them; `None`: they cannot be read.
    Radios(Option<Radios>),
    Reply(Replied),
    Airplane,
    Wifi,
    Disconnect,
    /// "Détails": the saved networks, the one in use unfolded.
    Details,
    /// The saved networks' sub-page, and back.
    Saved,
    Back,
    /// A network around: joined at once, or its sheet opened.
    Connect(Vec<u8>),
    /// A saved network unfolded (or folded), by its UUID.
    Unfold(String),
    Autoconnect(String),
    /// A saved network's password shown or hidden.
    Eye(String),
    /// "Changer…": its password typed anew.
    Change(String),
    Password(String),
    SavePassword,
    Forget(String),
    /// The pointer over the networks around, or not: their order held.
    Hover(bool),
    SheetInput(String),
    SheetEye,
    SheetConnect,
    SheetCancel,
}

/// An answer of NetworkManager's thread, taken once (it may hold a
/// password, which is not copied).
#[derive(Clone)]
pub struct Replied(Arc<Mutex<Option<Reply>>>);

impl Replied {
    fn take(&self) -> Option<Reply> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl std::fmt::Debug for Replied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Replied")
    }
}

/// A connection forgotten, for its undo: one is another only if it is
/// the same.
#[derive(Debug, Clone)]
pub struct Forgot(Arc<Kept>);

impl PartialEq for Forgot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Forgot {}

/// A change made on the page, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Airplane mode turned on or off.
    Airplane(bool),
    /// The Wi-Fi turned on or off; on, from airplane mode.
    Wifi {
        on: bool,
        airplane: bool,
    },
    Forget(Forgot),
}

/// A control of the page that takes the keyboard, in `Tab`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Airplane,
    Wifi,
    Disconnect,
    Details,
    Saved,
    /// The networks around: one stop, `↑`/`↓` through them.
    Around,
    Back,
    /// The saved networks: one stop, `↑`/`↓` through them.
    SavedList,
    Autoconnect,
    Eye,
    Change,
    Forget,
    SheetField,
    SheetCancel,
    SheetConnect,
}

/// A saved network's password, unfolded.
#[derive(Debug, Default)]
pub(super) struct Password {
    pub uuid: String,
    /// What the field holds: the saved one once read, or what is typed.
    pub text: String,
    pub shown: bool,
    /// Typed anew ("Changer…"): `Enter` saves it, `Esc` cancels.
    pub editing: bool,
}

/// The sheet over the page: a protected network joined with its
/// password.
#[derive(Debug)]
pub(super) struct Sheet {
    pub ssid: Vec<u8>,
    pub name: String,
    pub security: Security,
    pub text: String,
    pub shown: bool,
    /// Sent: the attempts before it, by their number, are not its.
    pub sent: Option<u64>,
    pub failed: Option<Failure>,
}

impl Sheet {
    /// Whether what is typed can be tried: 8 characters at least for WPA,
    /// a WEP key of 5 or 13 characters, 10 or 26 hex digits, or any
    /// passphrase.
    pub(super) fn filled(&self) -> bool {
        let n = self.text.chars().count();
        match self.security {
            Security::Wep => n > 0,
            _ => (secret::SHORTEST..secret::LONGEST).contains(&n) || n == secret::LONGEST,
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.sent.is_some()
    }
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    pub(super) details: Option<Arc<Details>>,
    asks: Option<tokio::sync::mpsc::UnboundedSender<Ask>>,
    radio_asks: Option<mpsc::Sender<rfkill::Ask>>,
    /// The radios; `None`: not read (yet, or at all).
    pub(super) radios: Option<Radios>,
    /// The saved networks' sub-page shows.
    pub(super) sub: bool,
    pub(super) unfolded: Option<String>,
    pub(super) password: Option<Password>,
    /// The networks around in the order held while the pointer is over
    /// them or the keyboard is in them.
    held: Option<Vec<Vec<u8>>>,
    hovered: bool,
    pub(super) sheet: Option<Sheet>,
    /// The keyboard's row among the networks around, and the saved ones.
    pub(super) cursor: (usize, usize),
    /// Connections asked forgotten, and given back: by UUID, and why.
    forgetting: Vec<(String, Doing)>,
    restoring: Vec<(String, Doing)>,
    /// A scan was asked since the page showed.
    scanned: bool,
}

impl State {
    /// Follows NetworkManager and the radios, on threads of their own:
    /// what they say comes to the window through `to`; what was on before
    /// airplane mode is kept in `state` (`$XDG_STATE_HOME/hocuspanda`).
    pub fn start(to: &UnboundedSender<Message>, state: Option<std::path::PathBuf>) -> Self {
        let (asks, incoming) = tokio::sync::mpsc::unbounded_channel();
        let heard = to.clone();
        let mut last: Option<Details> = None;
        hocus_network::nm::start(incoming, move |said| match said {
            Heard::Now(view) => {
                let details = view.details();
                if last.as_ref() != Some(&details) {
                    last = Some(details.clone());
                    let _ = heard.unbounded_send(Message::Network(Msg::Heard(Arc::new(details))));
                }
            }
            Heard::Reply(reply) => {
                let replied = Replied(Arc::new(Mutex::new(Some(reply))));
                let _ = heard.unbounded_send(Message::Network(Msg::Reply(replied)));
            }
        });
        let (radio_asks, receive) = mpsc::channel();
        let radios = to.clone();
        rfkill::start(
            rfkill::device(),
            state.map(|dir| dir.join("airplane")),
            receive,
            move |now| {
                radios
                    .unbounded_send(Message::Network(Msg::Radios(now)))
                    .is_ok()
            },
        );
        Self {
            asks: Some(asks),
            radio_asks: Some(radio_asks),
            ..Self::default()
        }
    }

    fn ask(&self, ask: Ask) {
        let sent = self.asks.as_ref().is_some_and(|a| a.send(ask).is_ok());
        if !sent {
            tracing::warn!("NetworkManager's thread has stopped: dropped");
        }
    }

    fn radio(&self, ask: rfkill::Ask) {
        let sent = self
            .radio_asks
            .as_ref()
            .is_some_and(|a| a.send(ask).is_ok());
        if !sent {
            tracing::warn!(?ask, "the radios' thread has stopped: dropped");
        }
    }

    /// NetworkManager is there.
    pub(super) fn manager(&self) -> bool {
        self.details.as_ref().is_some_and(|d| d.network.manager)
    }

    pub(super) fn airplane(&self) -> bool {
        self.radios.as_ref().is_some_and(Radios::airplane)
    }

    /// How the Wi-Fi's radio is blocked: by a switch (rfkill's or
    /// NetworkManager's word), by software, or not.
    pub(super) fn wifi_block(&self) -> Block {
        let hardware = self.details.as_ref().is_some_and(|d| !d.hardware_enabled);
        match self.radios.as_ref().map(|r| r.block(rfkill::WLAN)) {
            _ if hardware => Block::Hard,
            Some(block) => block,
            None => Block::No,
        }
    }

    /// The Wi-Fi is on: NetworkManager's switch, its radio not blocked.
    pub(super) fn wifi_on(&self) -> bool {
        self.details
            .as_ref()
            .is_some_and(|d| d.network.manager && d.network.hardware && d.network.enabled)
            && self.wifi_block() == Block::No
    }

    /// The networks around but the one in use, in the order held while
    /// the pointer or the keyboard is in them (new ones after).
    pub(super) fn around(&self) -> Vec<Wifi> {
        let Some(details) = self.details.as_deref() else {
            return Vec::new();
        };
        let mut around: Vec<Wifi> = details.network.others().cloned().collect();
        if let Some(held) = &self.held {
            around.sort_by_key(|n| held.iter().position(|s| *s == n.ssid).unwrap_or(usize::MAX));
        }
        around
    }

    /// The saved networks, the one in use then the most recent first.
    pub(super) fn saved(&self) -> Vec<hocus_network::model::Saved> {
        self.details
            .as_deref()
            .map(|d| model::by_recent(&d.saved))
            .unwrap_or_default()
    }

    /// The order of the networks around is held, or let go.
    fn hold(&mut self, hold: bool) {
        if hold {
            if self.held.is_none() {
                self.held = Some(self.around().into_iter().map(|n| n.ssid).collect());
            }
        } else {
            self.held = None;
        }
    }
}

/// Now, in seconds since the epoch, for "Il y a 2 h".
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl App {
    pub(super) fn network_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(details) => {
                self.network_heard(details);
                Task::none()
            }
            Msg::Radios(radios) => {
                self.network.radios = radios;
                Task::none()
            }
            Msg::Reply(replied) => match replied.take() {
                Some(reply) => self.network_reply(reply),
                None => Task::none(),
            },
            Msg::Airplane => {
                if !self.network.manager() || self.network.radios.is_none() {
                    return Task::none();
                }
                let on = !self.network.airplane();
                self.knobs.insert("network.airplane", Instant::now());
                self.network.radio(rfkill::Ask::Airplane(on));
                self.network_done(Choice::Airplane(on))
            }
            Msg::Wifi => {
                let Some(details) = self.network.details.clone() else {
                    return Task::none();
                };
                let block = self.network.wifi_block();
                if !details.network.manager || !details.network.hardware || block == Block::Hard {
                    return Task::none();
                }
                let on = !self.network.wifi_on();
                let airplane = on && self.network.airplane();
                self.knobs.insert("network.wifi", Instant::now());
                self.network_wifi(on, block == Block::Soft);
                self.network_done(Choice::Wifi { on, airplane })
            }
            Msg::Disconnect => {
                self.network.ask(Ask::Disconnect);
                Task::none()
            }
            Msg::Details => {
                let uuid = self
                    .network
                    .details
                    .as_ref()
                    .and_then(|d| d.current.as_ref())
                    .and_then(|c| c.uuid.clone());
                self.network_sub(true);
                self.network.unfolded.clone_from(&uuid);
                if let Some(at) =
                    uuid.and_then(|u| self.network.saved().iter().position(|s| s.uuid == u))
                {
                    self.network.cursor.1 = at;
                }
                Task::none()
            }
            Msg::Saved => {
                self.network_sub(true);
                Task::none()
            }
            Msg::Back => {
                self.network_sub(false);
                Task::none()
            }
            Msg::Connect(ssid) => self.network_connect(ssid),
            Msg::Unfold(uuid) => {
                self.network.password = None;
                self.network.unfolded = if self.network.unfolded.as_deref() == Some(&uuid) {
                    None
                } else {
                    Some(uuid)
                };
                Task::none()
            }
            Msg::Autoconnect(uuid) => {
                if let Some(saved) = self.network.saved().into_iter().find(|s| s.uuid == uuid) {
                    self.knobs.insert("network.autoconnect", Instant::now());
                    self.network.ask(Ask::Autoconnect {
                        uuid,
                        on: !saved.autoconnect,
                    });
                }
                Task::none()
            }
            Msg::Eye(uuid) => {
                match &mut self.network.password {
                    Some(p) if p.uuid == uuid && p.editing => p.shown = !p.shown,
                    Some(p) if p.uuid == uuid && p.shown => {
                        // Hidden again: forgotten by the window too.
                        wipe(&mut p.text);
                        p.shown = false;
                    }
                    _ => {
                        self.network.password = Some(Password {
                            uuid: uuid.clone(),
                            ..Password::default()
                        });
                        self.network.ask(Ask::ReadPassword { uuid });
                    }
                }
                Task::none()
            }
            Msg::Change(uuid) => {
                if let Some(p) = &mut self.network.password {
                    wipe(&mut p.text);
                }
                self.network.password = Some(Password {
                    uuid,
                    editing: true,
                    ..Password::default()
                });
                operation::focus(PASSWORD_FIELD)
            }
            Msg::Password(text) => {
                if let Some(p) = self.network.password.as_mut().filter(|p| p.editing) {
                    p.text = text;
                }
                Task::none()
            }
            Msg::SavePassword => {
                let Some(mut p) = self.network.password.take() else {
                    return Task::none();
                };
                let n = p.text.chars().count();
                let wep = self
                    .network
                    .saved()
                    .iter()
                    .any(|s| s.uuid == p.uuid && s.security == Security::Wep);
                if !p.editing
                    || n == 0
                    || (!wep && !(secret::SHORTEST..=secret::LONGEST).contains(&n))
                {
                    self.network.password = Some(p);
                    return Task::none();
                }
                let mut secret = Secret::longest(n);
                secret.push(&p.text);
                wipe(&mut p.text);
                self.network.ask(Ask::SetPassword {
                    uuid: p.uuid.clone(),
                    secret,
                });
                Task::none()
            }
            Msg::Forget(uuid) => {
                self.network.forgetting.push((uuid.clone(), Doing::New));
                self.network.ask(Ask::Forget { uuid });
                Task::none()
            }
            Msg::Hover(over) => {
                self.network.hovered = over;
                let keyboard = self.network_focused(Stop::Around);
                self.network.hold(over || keyboard);
                Task::none()
            }
            Msg::SheetInput(text) => {
                if let Some(sheet) = self.network.sheet.as_mut().filter(|s| !s.waiting()) {
                    sheet.text = text;
                    sheet.failed = None;
                }
                Task::none()
            }
            Msg::SheetEye => {
                if let Some(sheet) = &mut self.network.sheet {
                    sheet.shown = !sheet.shown;
                }
                Task::none()
            }
            Msg::SheetConnect => self.network_sheet_connect(),
            Msg::SheetCancel => {
                self.network_close_sheet();
                Task::none()
            }
        }
    }

    /// NetworkManager said something new: the page shows it; the sheet
    /// hears how its attempt ended.
    fn network_heard(&mut self, details: Arc<Details>) {
        if !details.network.manager {
            self.network.sheet = None;
            self.network.password = None;
        }
        if let Some(sheet) = &mut self.network.sheet
            && let Some(attempt) = &details.network.attempt
            && attempt.ssid == sheet.ssid
            && sheet.sent.is_some_and(|before| attempt.id > before)
        {
            match attempt.state {
                Progress::Connecting => {}
                Progress::Connected => {
                    tracing::info!("the sheet's network joined");
                    self.network_close_sheet();
                }
                Progress::Failed(failure) => {
                    tracing::info!(?failure, "the sheet's network did not join");
                    sheet.failed = Some(failure);
                    sheet.sent = None;
                }
            }
        }
        // A saved network gone: folded.
        if let Some(uuid) = &self.network.unfolded
            && !details.saved.iter().any(|s| s.uuid == *uuid)
        {
            self.network.unfolded = None;
            self.network.password = None;
        }
        self.network.details = Some(details);
        let count = self.network.around().len();
        self.network.cursor.0 = self.network.cursor.0.min(count.saturating_sub(1));
        let saved = self.network.saved().len();
        self.network.cursor.1 = self.network.cursor.1.min(saved.saturating_sub(1));
    }

    /// An answer of NetworkManager's thread.
    fn network_reply(&mut self, reply: Reply) -> Task<Message> {
        let lang = self.lang;
        match reply {
            Reply::Password { uuid, secret } => {
                if let Some(p) = self
                    .network
                    .password
                    .as_mut()
                    .filter(|p| p.uuid == uuid && !p.editing)
                {
                    p.text = secret
                        .as_ref()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();
                    p.shown = true;
                }
                Task::none()
            }
            Reply::Forgotten(kept) => {
                let doing = take(&mut self.network.forgetting, &kept.uuid).unwrap_or(Doing::New);
                if self.network.unfolded.as_deref() == Some(kept.uuid.as_str()) {
                    self.network.unfolded = None;
                    self.network.password = None;
                }
                let name = kept.name.clone();
                self.undo.push(Undo::Network(Choice::Forget(Forgot(kept))));
                if doing == Doing::Redo {
                    let text = match lang {
                        Lang::French => format!("Rétabli : « {name} » oublié."),
                        Lang::English => format!("Redone: \u{201c}{name}\u{201d} forgotten."),
                    };
                    return self.toast(text, ToastAction::None, UNDONE);
                }
                self.redo.clear();
                let text = match lang {
                    Lang::French => format!("« {name} » oublié."),
                    Lang::English => format!("\u{201c}{name}\u{201d} forgotten."),
                };
                self.toast(text, ToastAction::Undo, TOAST)
            }
            Reply::Restored(kept) => {
                let doing = take(&mut self.network.restoring, &kept.uuid).unwrap_or(Doing::Undo);
                let name = kept.name.clone();
                let secrets = kept.secrets;
                if doing == Doing::Undo {
                    self.redo.push(Undo::Network(Choice::Forget(Forgot(kept))));
                }
                let text = match (lang, secrets) {
                    (Lang::French, true) => format!("Annulé : « {name} » revient."),
                    (Lang::English, true) => format!("Undone: \u{201c}{name}\u{201d} is back."),
                    (Lang::French, false) => {
                        format!("Annulé : « {name} » revient, sans son mot de passe.")
                    }
                    (Lang::English, false) => {
                        format!("Undone: \u{201c}{name}\u{201d} comes back, without its password.")
                    }
                };
                self.toast(text, ToastAction::None, UNDONE)
            }
            Reply::Refused { denied, message } => {
                self.network.forgetting.clear();
                self.network.restoring.clear();
                let text = match (lang, denied) {
                    (Lang::French, true) => format!("Le système a refusé : {message}"),
                    (Lang::English, true) => format!("The system refused: {message}"),
                    (Lang::French, false) => format!("NetworkManager n’a rien changé : {message}"),
                    (Lang::English, false) => format!("NetworkManager changed nothing: {message}"),
                };
                self.toast(text, ToastAction::None, TOAST)
            }
        }
    }

    /// The Wi-Fi turned on or off; on, its radio unblocked first if
    /// software blocks it (airplane mode: it turns on alone).
    fn network_wifi(&mut self, on: bool, blocked: bool) {
        if on && blocked {
            self.network.radio(rfkill::Ask::Unblock(rfkill::WLAN));
        }
        self.network.ask(Ask::Enable(on));
    }

    /// A change of the page made: toasted with "Annuler", kept for
    /// `Ctrl+Z`.
    fn network_done(&mut self, choice: Choice) -> Task<Message> {
        let text = network_said(&choice, self.lang, Doing::New);
        self.undo.push(Undo::Network(choice));
        self.redo.clear();
        self.toast(text, ToastAction::Undo, TOAST)
    }

    /// `choice` undone (or redone).
    pub(super) fn network_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        self.go(Page::Section(SectionId::Network));
        match &choice {
            Choice::Airplane(on) => {
                let to = if redo { *on } else { !*on };
                self.knobs.insert("network.airplane", Instant::now());
                self.network.radio(rfkill::Ask::Airplane(to));
                self.light = Some(("network.airplane", Instant::now()));
            }
            Choice::Wifi { on, airplane } => {
                self.knobs.insert("network.wifi", Instant::now());
                match (redo, *on, *airplane) {
                    (false, true, true) => self.network.radio(rfkill::Ask::Airplane(true)),
                    (false, on, _) => self.network.ask(Ask::Enable(!on)),
                    (true, on, airplane) => self.network_wifi(on, airplane),
                }
                self.light = Some(("network.wifi", Instant::now()));
            }
            Choice::Forget(Forgot(kept)) => {
                self.network_sub(true);
                if redo {
                    self.network
                        .forgetting
                        .push((kept.uuid.clone(), Doing::Redo));
                    self.network.ask(Ask::Forget {
                        uuid: kept.uuid.clone(),
                    });
                } else {
                    self.network
                        .restoring
                        .push((kept.uuid.clone(), Doing::Undo));
                    self.network.ask(Ask::Restore(kept.clone()));
                }
                // Its stack and its toast once NetworkManager answers.
                return Task::none();
            }
        }
        let text = network_said(
            &choice,
            self.lang,
            if redo { Doing::Redo } else { Doing::Undo },
        );
        if redo {
            self.undo.push(Undo::Network(choice));
        } else {
            self.redo.push(Undo::Network(choice));
        }
        self.toast(text, ToastAction::None, UNDONE)
    }

    /// A network around: an open or saved one joined at once, a
    /// protected one's sheet opened.
    fn network_connect(&mut self, ssid: Vec<u8>) -> Task<Message> {
        if self
            .network
            .details
            .as_ref()
            .is_some_and(|d| d.network.connecting_to().is_some())
        {
            return Task::none();
        }
        let Some(wifi) = self.network.around().into_iter().find(|n| n.ssid == ssid) else {
            return Task::none();
        };
        if model::joins_at_once(&wifi) {
            self.network.ask(Ask::Connect { ssid });
            return Task::none();
        }
        if !wifi.security.joinable() {
            // 802.1X and WEP: their sheet's fields come with #406's
            // second part.
            return Task::none();
        }
        self.network.sheet = Some(Sheet {
            ssid,
            name: wifi.name,
            security: wifi.security,
            text: String::new(),
            shown: false,
            sent: None,
            failed: None,
        });
        self.focus = Focus::Control(0);
        operation::focus(SHEET_FIELD)
    }

    fn network_sheet_connect(&mut self) -> Task<Message> {
        let before = self
            .network
            .details
            .as_ref()
            .and_then(|d| d.network.attempt.as_ref())
            .map_or(0, |a| a.id);
        let Some(sheet) = self.network.sheet.as_mut() else {
            return Task::none();
        };
        if sheet.waiting() || !sheet.filled() {
            return Task::none();
        }
        let mut secret = Secret::longest(sheet.text.chars().count());
        secret.push(&sheet.text);
        sheet.sent = Some(before);
        sheet.failed = None;
        let ssid = sheet.ssid.clone();
        self.network.ask(Ask::ConnectWith { ssid, secret });
        Task::none()
    }

    /// The sheet closed: what was typed forgotten, an attempt under way
    /// given up.
    fn network_close_sheet(&mut self) {
        if let Some(mut sheet) = self.network.sheet.take() {
            if sheet.waiting() {
                self.network.ask(Ask::Abandon);
            }
            wipe(&mut sheet.text);
        }
        self.focus = Focus::Nothing;
    }

    /// The saved networks' sub-page shown, or the page.
    fn network_sub(&mut self, sub: bool) {
        self.go(Page::Section(SectionId::Network));
        if self.network.sub != sub {
            self.network.sub = sub;
            self.focus = Focus::Nothing;
        }
        if !sub {
            self.network.unfolded = None;
            if let Some(p) = &mut self.network.password {
                wipe(&mut p.text);
            }
            self.network.password = None;
        }
    }

    /// Each time the page shows, NetworkManager is asked to scan; when it
    /// goes, what it showed of passwords is forgotten.
    pub(super) fn network_visit(&mut self) {
        let shown = self.page == Page::Section(SectionId::Network);
        if shown && !self.network.scanned && self.network.manager() {
            self.network.scanned = true;
            self.network.ask(Ask::Scan);
        } else if !shown && self.network.scanned {
            self.network.scanned = false;
            self.network.sub = false;
            self.network.unfolded = None;
            if let Some(p) = &mut self.network.password {
                wipe(&mut p.text);
            }
            self.network.password = None;
            self.network_close_sheet_quietly();
        }
    }

    fn network_close_sheet_quietly(&mut self) {
        if self.network.sheet.is_some() {
            self.network_close_sheet();
        }
    }

    /// `Esc` on the page: the sheet closes, a password typed is given up.
    pub(super) fn network_escape(&mut self) -> Option<Task<Message>> {
        if self.page != Page::Section(SectionId::Network) {
            return None;
        }
        if self.network.sheet.is_some() {
            self.network_close_sheet();
            return Some(operation::focus("nothing"));
        }
        if let Some(p) = self.network.password.as_mut().filter(|p| p.editing) {
            wipe(&mut p.text);
            self.network.password = None;
            return Some(operation::focus("nothing"));
        }
        None
    }

    /// A key typed while no field holds the keyboard: into the sheet's
    /// field, or the password typed anew, which then takes the keyboard.
    pub(super) fn network_typed(&mut self, typed: &str) -> Option<Task<Message>> {
        if self.page != Page::Section(SectionId::Network) {
            return None;
        }
        let field = if let Some(sheet) = self.network.sheet.as_mut().filter(|s| !s.waiting()) {
            sheet.text.push_str(typed);
            sheet.failed = None;
            self.focus = Focus::Control(0);
            SHEET_FIELD
        } else {
            let p = self.network.password.as_mut().filter(|p| p.editing)?;
            p.text.push_str(typed);
            PASSWORD_FIELD
        };
        Some(Task::batch([
            operation::focus(field),
            operation::move_cursor_to_end(field),
        ]))
    }

    /// Whether the keyboard is on `stop`.
    pub(super) fn network_focused(&self, stop: Stop) -> bool {
        matches!(self.focus, Focus::Control(i) if self.network_controls().get(i) == Some(&stop))
    }

    /// The page's controls, in `Tab`'s order: the sheet's alone while it
    /// is open.
    pub(super) fn network_controls(&self) -> Vec<Stop> {
        if self.network.sheet.is_some() {
            return vec![Stop::SheetField, Stop::SheetCancel, Stop::SheetConnect];
        }
        if !self.network.manager() {
            return Vec::new();
        }
        if self.network.sub {
            let mut stops = vec![Stop::Back];
            if !self.network.saved().is_empty() {
                stops.push(Stop::SavedList);
            }
            if let Some(saved) = self
                .network
                .unfolded
                .as_ref()
                .and_then(|u| self.network.saved().into_iter().find(|s| s.uuid == *u))
            {
                stops.push(Stop::Autoconnect);
                if matches!(saved.security, Security::Psk { .. } | Security::Wep) {
                    stops.extend([Stop::Eye, Stop::Change]);
                }
                stops.push(Stop::Forget);
            }
            return stops;
        }
        let mut stops = Vec::new();
        if self.network.radios.is_some() {
            stops.push(Stop::Airplane);
        }
        let details = self.network.details.as_deref();
        if details.is_some_and(|d| d.network.hardware) && self.network.wifi_block() != Block::Hard {
            stops.push(Stop::Wifi);
        }
        if self.network.wifi_on() && details.is_some_and(|d| d.network.current.is_some()) {
            stops.extend([Stop::Disconnect, Stop::Details]);
        }
        stops.push(Stop::Saved);
        if self.network.wifi_on() && !self.network.around().is_empty() {
            stops.push(Stop::Around);
        }
        stops
    }

    /// `Tab` in the sheet: through its field and its buttons, never out.
    pub(super) fn network_sheet_tab(&mut self, back: bool) -> Task<Message> {
        let count = self.network_controls().len();
        let at = match self.focus {
            Focus::Control(i) if i < count => i,
            _ => 0,
        };
        let next = if back {
            (at + count - 1) % count
        } else {
            (at + 1) % count
        };
        self.focus = Focus::Control(next);
        if next == 0 {
            operation::focus(SHEET_FIELD)
        } else {
            operation::focus("nothing")
        }
    }

    /// A key on the page's control `stop`.
    pub(super) fn network_key(&mut self, stop: Stop, named: Named) -> Task<Message> {
        let press = matches!(named, Named::Enter | Named::Space);
        match stop {
            Stop::Around => {
                let around = self.network.around();
                match named {
                    Named::ArrowDown => {
                        self.network.cursor.0 =
                            (self.network.cursor.0 + 1).min(around.len().saturating_sub(1));
                    }
                    Named::ArrowUp => {
                        self.network.cursor.0 = self.network.cursor.0.saturating_sub(1)
                    }
                    _ if press => {
                        if let Some(wifi) = around.get(self.network.cursor.0) {
                            return self.network_update(Msg::Connect(wifi.ssid.clone()));
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Stop::SavedList => {
                let saved = self.network.saved();
                match named {
                    Named::ArrowDown => {
                        self.network.cursor.1 =
                            (self.network.cursor.1 + 1).min(saved.len().saturating_sub(1));
                    }
                    Named::ArrowUp => {
                        self.network.cursor.1 = self.network.cursor.1.saturating_sub(1)
                    }
                    Named::Delete => {
                        if let Some(s) = saved.get(self.network.cursor.1) {
                            return self.network_update(Msg::Forget(s.uuid.clone()));
                        }
                    }
                    _ if press => {
                        if let Some(s) = saved.get(self.network.cursor.1) {
                            return self.network_update(Msg::Unfold(s.uuid.clone()));
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            _ if !press => Task::none(),
            Stop::Airplane => self.network_update(Msg::Airplane),
            Stop::Wifi => self.network_update(Msg::Wifi),
            Stop::Disconnect => self.network_update(Msg::Disconnect),
            Stop::Details => self.network_update(Msg::Details),
            Stop::Saved => self.network_update(Msg::Saved),
            Stop::Back => self.network_update(Msg::Back),
            Stop::Autoconnect | Stop::Eye | Stop::Change | Stop::Forget => {
                let Some(uuid) = self.network.unfolded.clone() else {
                    return Task::none();
                };
                let msg = match stop {
                    Stop::Autoconnect => Msg::Autoconnect(uuid),
                    Stop::Eye => Msg::Eye(uuid),
                    Stop::Change => Msg::Change(uuid),
                    _ => Msg::Forget(uuid),
                };
                self.network_update(msg)
            }
            Stop::SheetCancel => self.network_update(Msg::SheetCancel),
            Stop::SheetConnect => self.network_update(Msg::SheetConnect),
            Stop::SheetField => Task::none(),
        }
    }

    /// The keyboard moved: the networks around keep their order while it
    /// is in them.
    pub(super) fn network_after_key(&mut self) {
        let keyboard = self.network_focused(Stop::Around);
        let hovered = self.network.hovered;
        self.network.hold(keyboard || hovered);
    }

    /// The home's card.
    pub(super) fn network_card(&self) -> String {
        model::card(
            self.network.details.as_deref(),
            self.network.airplane(),
            self.lang,
        )
    }

    /// Whether something of the page moves at `now`: a scan's or a
    /// connection's spinner.
    pub(super) fn network_moving(&self) -> bool {
        if self.page != Page::Section(SectionId::Network) || self.reduced_motion {
            return false;
        }
        let Some(details) = self.network.details.as_deref() else {
            return false;
        };
        (!self.network.sub && self.network.wifi_on() && details.network.scanning)
            || details.network.connecting_to().is_some()
            || self.network.sheet.as_ref().is_some_and(Sheet::waiting)
    }
}

/// What a change's toast says, done, undone or redone.
fn network_said(choice: &Choice, lang: Lang, doing: Doing) -> String {
    let said = match choice {
        Choice::Airplane(true) => w("Mode avion activé", "Airplane mode on"),
        Choice::Airplane(false) => w("Mode avion désactivé", "Airplane mode off"),
        Choice::Wifi { on: true, .. } => w("Wi-Fi allumé", "Wi-Fi on"),
        Choice::Wifi { on: false, .. } => w("Wi-Fi coupé", "Wi-Fi off"),
        Choice::Forget(_) => w("Réseau oublié", "Network forgotten"),
    }
    .get(lang);
    let back = match choice {
        Choice::Airplane(true) => w("mode avion désactivé", "airplane mode off"),
        Choice::Airplane(false) => w("mode avion activé", "airplane mode on"),
        Choice::Wifi { on: true, .. } => w("Wi-Fi coupé", "Wi-Fi off"),
        Choice::Wifi { on: false, .. } => w("Wi-Fi allumé", "Wi-Fi on"),
        Choice::Forget(_) => w("réseau revenu", "network back"),
    }
    .get(lang);
    match (doing, lang) {
        (Doing::New, _) => said.to_owned(),
        (Doing::Undo, Lang::French) => format!("Annulé : {back}."),
        (Doing::Undo, Lang::English) => format!("Undone: {back}."),
        (Doing::Redo, Lang::French) => format!("Rétabli : {}.", said.to_lowercase()),
        (Doing::Redo, Lang::English) => format!("Redone: {}.", said.to_lowercase()),
    }
}

/// The entry for `uuid` taken out of `list`: why it was asked.
fn take(list: &mut Vec<(String, Doing)>, uuid: &str) -> Option<Doing> {
    let at = list.iter().position(|(u, _)| u == uuid)?;
    Some(list.remove(at).1)
}

/// A password the window held, overwritten before it goes.
fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}
