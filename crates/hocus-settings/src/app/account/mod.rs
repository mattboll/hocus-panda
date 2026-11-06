//! The Compte page (6.11, settings.md "Compte"): the avatar and its
//! pandas, the display name, the login and the account's type, the
//! password's sheet, the link to the automatic locking.
//!
//! AccountsService is followed and asked on a thread of its own
//! ([`crate::accounts`]), started the first time the page shows; the
//! password is changed by the system's `passwd` on another
//! ([`crate::passwd`]); nothing is waited for on the window's thread, and
//! nothing is written in the file. A panda, a picture or a name set toasts
//! with "Annuler" and joins `Ctrl+Z`; the password does not.

mod view;

use std::{path::PathBuf, sync::Arc};

use iced::{
    Task,
    futures::channel::mpsc::UnboundedSender,
    keyboard::{Key, key::Named},
    widget::{image::Handle, operation},
};

use super::{App, Doing, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{
    account as model,
    accounts::{self, Account, Ask, Asks, Heard},
    lang::{Lang, w},
    model::SectionId,
    passwd::{self, Outcome},
    secret::Secret,
};

/// The display name's field, and the sheet's three.
pub(super) const NAME_FIELD: &str = "account-name";
pub(super) const FIELDS: [&str; 3] = ["password-current", "password-new", "password-again"];

#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Heard),
    /// The display name typed, saved, put back.
    Name(String),
    SaveName,
    /// A press somewhere while the name is typed: whether its field still
    /// holds the keyboard.
    Pressed,
    NameFocused(bool),
    /// The avatar's popover opened or closed.
    Popover(bool),
    Panda(usize),
    /// "Choisir une image…": the portal asked, and what it chose.
    Choose,
    Chosen(Option<PathBuf>),
    Remove,
    /// The password's sheet opened or closed.
    Sheet(bool),
    Typed(usize, String),
    /// A field's eye.
    Show(usize),
    /// `Enter` in a field.
    Next(usize),
    Change,
    Changed(Outcome),
    /// "Énergie ›".
    Lock,
}

/// A choice made on the page, for its undo: the display name, or the
/// picture (its bytes; `None`: the initial).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Name {
        before: String,
        after: String,
    },
    Picture {
        before: Option<Arc<Vec<u8>>>,
        after: Option<Arc<Vec<u8>>>,
    },
}

impl Choice {
    fn reversed(self) -> Self {
        match self {
            Self::Name { before, after } => Self::Name {
                before: after,
                after: before,
            },
            Self::Picture { before, after } => Self::Picture {
                before: after,
                after: before,
            },
        }
    }
}

/// A control of the page that takes the keyboard, in `Tab`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Avatar,
    Password,
    Lock,
}

/// The password's sheet: what is typed, kept as [`Secret`]s.
#[derive(Debug, Default)]
pub struct Sheet {
    pub(super) typed: [Secret; 3],
    pub(super) shown: [bool; 3],
    /// The field that last held the keyboard.
    pub(super) at: usize,
    pub(super) running: bool,
    pub(super) wrong_current: bool,
    pub(super) refused: Option<String>,
    pub(super) failed: Option<String>,
}

impl Sheet {
    /// The two new ones typed and the same.
    pub(super) fn matching(&self) -> Option<bool> {
        let [_, new, again] = &self.typed;
        (!again.is_empty()).then(|| new.same(again))
    }

    pub(super) fn ready(&self) -> bool {
        !self.running
            && !self.typed[0].is_empty()
            && !self.typed[1].is_empty()
            && self.matching() == Some(true)
    }

    fn clear(&mut self) {
        for secret in &mut self.typed {
            secret.clear();
        }
    }
}

/// What a picture asked becomes, said when AccountsService answers.
#[derive(Debug, Clone)]
enum Asked {
    Name(String, Doing, Option<Choice>),
    Picture(Doing, Option<Choice>),
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    to: Option<UnboundedSender<Message>>,
    asks: Option<Asks>,
    /// AccountsService as last heard: `None` until it is, `Some(None)`
    /// when it is not there.
    pub(super) heard: Option<Option<Account>>,
    /// The pandas, and the account's picture, as the window draws them.
    pub(super) pandas: Vec<Handle>,
    pub(super) picture: Option<(Arc<Vec<u8>>, Handle)>,
    /// The display name as typed, while it is.
    pub(super) draft: Option<String>,
    asked: Vec<Asked>,
    /// The popover: the panda the keyboard is on.
    pub(super) popover: Option<usize>,
    /// The file chooser is open.
    pub(super) choosing: bool,
    pub(super) sheet: Option<Sheet>,
    /// `passwd` is there.
    pub(super) passwd: bool,
    /// The picture before the last one asked, for its undo.
    before: Option<Arc<Vec<u8>>>,
    /// The popover and the sheet as last drawn.
    drawn: Drawn,
}

type Drawn = (
    Option<usize>,
    Option<(bool, bool, bool, bool, Option<bool>, [bool; 3])>,
);

impl State {
    pub fn new(to: UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to),
            passwd: passwd::program().is_some(),
            ..Self::default()
        }
    }

    pub(super) fn account(&self) -> Option<&Account> {
        self.heard.as_ref()?.as_ref()
    }

    fn ask(&self, ask: Ask) -> bool {
        let sent = self
            .asks
            .as_ref()
            .is_some_and(|a| a.unbounded_send(ask).is_ok());
        if !sent {
            tracing::warn!("AccountsService's thread has stopped: dropped");
        }
        sent
    }

    fn picture_bytes(&self) -> Option<Arc<Vec<u8>>> {
        self.account()?.picture.as_ref().map(|p| p.bytes.clone())
    }
}

impl App {
    fn account_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Account)
    }

    /// The name the page shows: as typed, else AccountsService's.
    pub(super) fn account_name(&self) -> String {
        self.account
            .draft
            .clone()
            .or_else(|| self.account.account().map(|a| a.real_name.clone()))
            .unwrap_or_default()
    }

    /// The name said: the real name, else the login.
    pub(super) fn account_said_name(&self) -> String {
        match self.account.account() {
            Some(a) if !a.real_name.trim().is_empty() => a.real_name.clone(),
            Some(a) => a.login.clone(),
            None => self.user.clone(),
        }
    }

    /// The panda the account's picture is, if one.
    pub(super) fn account_panda(&self) -> Option<usize> {
        self.account.account()?.picture.as_ref()?.panda
    }

    pub(super) fn account_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(heard) => self.account_heard(heard),
            Msg::Name(name) => {
                self.account.draft = Some(name);
                Task::none()
            }
            Msg::SaveName => self.account_save_name(),
            Msg::Pressed => operation::is_focused(NAME_FIELD)
                .map(|focused| Message::Account(Msg::NameFocused(focused))),
            Msg::NameFocused(focused) => {
                if focused {
                    Task::none()
                } else {
                    self.account_save_name()
                }
            }
            Msg::Popover(open) => {
                self.account.popover = (open && self.account.account().is_some())
                    .then(|| self.account_panda().unwrap_or(0));
                Task::none()
            }
            Msg::Panda(index) => {
                self.account.popover = None;
                if self.account_panda() == Some(index) {
                    return Task::none();
                }
                self.account_picture(Ask::Panda(index), Doing::New, None);
                Task::none()
            }
            Msg::Choose => {
                self.account.popover = None;
                if self.account.choosing {
                    return Task::none();
                }
                self.account.choosing = true;
                let lang = self.lang;
                let (title, filter) = match lang {
                    Lang::French => ("Choisir un avatar", "Images (JPEG, PNG, WebP)"),
                    Lang::English => ("Choose an avatar", "Pictures (JPEG, PNG, WebP)"),
                };
                let to = self.account.to.clone();
                let started = std::thread::Builder::new()
                    .name("avatar: file chooser".to_owned())
                    .spawn(move || {
                        let folder = std::env::var_os("XDG_PICTURES_DIR")
                            .filter(|d| !d.is_empty())
                            .map(PathBuf::from)
                            .or_else(|| {
                                std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures"))
                            })
                            .unwrap_or_default();
                        let chosen = match crate::portal::open_file(title, filter, &folder) {
                            crate::portal::Chosen::Path(path) => Some(path),
                            _ => None,
                        };
                        if let Some(to) = to {
                            let _ = to.unbounded_send(Message::Account(Msg::Chosen(chosen)));
                        }
                    });
                if let Err(err) = started {
                    tracing::warn!(%err, "no file chooser");
                    self.account.choosing = false;
                }
                Task::none()
            }
            Msg::Chosen(chosen) => {
                self.account.choosing = false;
                if let Some(path) = chosen {
                    self.account_picture(Ask::Chosen(path), Doing::New, None);
                }
                Task::none()
            }
            Msg::Remove => {
                self.account.popover = None;
                if self.account.picture.is_none() {
                    return Task::none();
                }
                self.account_picture(Ask::Remove, Doing::New, None);
                Task::none()
            }
            Msg::Sheet(open) => {
                if !open {
                    if self.account.sheet.as_ref().is_some_and(|s| s.running) {
                        return Task::none();
                    }
                    self.account.sheet = None;
                    return Task::none();
                }
                if !self.account.passwd {
                    return Task::none();
                }
                self.account.sheet = Some(Sheet::default());
                operation::focus(FIELDS[0])
            }
            Msg::Typed(i, text) => {
                if let Some(sheet) = self.account.sheet.as_mut()
                    && !sheet.running
                    && let Some(secret) = sheet.typed.get_mut(i)
                {
                    secret.set(&text);
                    sheet.at = i;
                    match i {
                        0 => sheet.wrong_current = false,
                        _ => sheet.refused = None,
                    }
                    sheet.failed = None;
                }
                Task::none()
            }
            Msg::Show(i) => {
                if let Some(shown) = self.account.sheet.as_mut().and_then(|s| s.shown.get_mut(i)) {
                    *shown = !*shown;
                }
                Task::none()
            }
            Msg::Next(i) => {
                let ready = self.account.sheet.as_ref().is_some_and(Sheet::ready);
                if i + 1 < FIELDS.len() && !ready {
                    if let Some(sheet) = self.account.sheet.as_mut() {
                        sheet.at = i + 1;
                    }
                    return operation::focus(FIELDS[i + 1]);
                }
                self.account_update(Msg::Change)
            }
            Msg::Change => {
                let Some(sheet) = self.account.sheet.as_mut() else {
                    return Task::none();
                };
                if !sheet.ready() {
                    return Task::none();
                }
                sheet.running = true;
                sheet.failed = None;
                sheet.refused = None;
                sheet.wrong_current = false;
                let current = sheet.typed[0].duplicate();
                let new = sheet.typed[1].duplicate();
                let to = self.account.to.clone();
                tracing::info!("the password is asked of passwd");
                let started =
                    std::thread::Builder::new()
                        .name("passwd".to_owned())
                        .spawn(move || {
                            let outcome = passwd::change(&current, &new);
                            drop((current, new));
                            if let Some(to) = to {
                                let _ = to.unbounded_send(Message::Account(Msg::Changed(outcome)));
                            }
                        });
                if let Err(err) = started {
                    tracing::warn!(%err, "passwd could not start");
                    if let Some(sheet) = self.account.sheet.as_mut() {
                        sheet.running = false;
                        sheet.failed = Some(err.to_string());
                    }
                }
                Task::none()
            }
            Msg::Changed(outcome) => self.account_changed(outcome),
            Msg::Lock => self.show_row("power.lock-after"),
        }
    }

    fn account_heard(&mut self, heard: Heard) -> Task<Message> {
        match heard {
            Heard::Pandas(pandas) => {
                self.account.pandas = pandas
                    .into_iter()
                    .map(|rgba| Handle::from_rgba(model::SHOWN, model::SHOWN, rgba.to_vec()))
                    .collect();
                Task::none()
            }
            Heard::State(state) => {
                // The picture's handle made once per picture.
                let picture = state.as_ref().and_then(|a| a.picture.clone());
                match picture {
                    Some(p)
                        if self
                            .account
                            .picture
                            .as_ref()
                            .is_none_or(|(b, _)| *b != p.bytes) =>
                    {
                        let handle = Handle::from_rgba(model::SHOWN, model::SHOWN, p.rgba.to_vec());
                        self.account.picture = Some((p.bytes, handle));
                    }
                    Some(_) => {}
                    None => self.account.picture = None,
                }
                self.account.heard = Some(state);
                if self.account.account().is_none() {
                    self.account.popover = None;
                }
                Task::none()
            }
            Heard::Done(ask, result) => {
                if self.account.asked.is_empty() {
                    return Task::none();
                }
                let asked = self.account.asked.remove(0);
                // The name as typed gives way to AccountsService's.
                if matches!(asked, Asked::Name(..)) {
                    self.account.draft = None;
                }
                if let Err(why) = result {
                    tracing::info!(why, ?ask, "AccountsService refused");
                    let text = match self.lang {
                        Lang::French => "Le système a refusé.".to_owned(),
                        Lang::English => "The system refused.".to_owned(),
                    };
                    return self.toast(text, ToastAction::None, TOAST);
                }
                match asked {
                    Asked::Name(before, doing, choice) => {
                        let after = self.account_said_name();
                        let choice = choice.unwrap_or(Choice::Name { before, after });
                        self.account_done(choice, doing)
                    }
                    Asked::Picture(doing, choice) => {
                        let after = self.account.picture_bytes();
                        let choice = choice.unwrap_or_else(|| Choice::Picture {
                            before: self.account_before_picture(),
                            after,
                        });
                        self.account_done(choice, doing)
                    }
                }
            }
        }
    }

    /// The picture before the last one asked, kept for its undo.
    fn account_before_picture(&mut self) -> Option<Arc<Vec<u8>>> {
        self.account.before.take()
    }

    /// A change of the page made: kept for its undo, and said.
    fn account_done(&mut self, choice: Choice, doing: Doing) -> Task<Message> {
        let lang = self.lang;
        let (done, undone, redone) = match (&choice, lang) {
            (Choice::Name { after, .. }, Lang::French) => (
                format!("Nom affiché : {after}"),
                format!("Annulé : le nom affiché revient à {after}."),
                format!("Rétabli : nom affiché : {after}."),
            ),
            (Choice::Name { after, .. }, Lang::English) => (
                format!("Display name: {after}"),
                format!("Undone: the display name is back to {after}."),
                format!("Redone: display name: {after}."),
            ),
            (Choice::Picture { after, .. }, _) => {
                let removed = after.is_none();
                let said = |fr: &'static str, en: &'static str| w_owned(fr, en, lang);
                (
                    if removed {
                        said("Avatar retiré.", "Avatar removed.")
                    } else {
                        said("Avatar changé.", "Avatar changed.")
                    },
                    said("Annulé : l’avatar revient.", "Undone: the avatar is back."),
                    said("Rétabli : avatar changé.", "Redone: avatar changed."),
                )
            }
        };
        let row = match choice {
            Choice::Name { .. } => model::NAME,
            Choice::Picture { .. } => model::AVATAR,
        };
        match doing {
            Doing::New => {
                self.undo.push(super::Undo::Account(choice));
                self.redo.clear();
                self.toast(done, ToastAction::Undo, TOAST)
            }
            Doing::Undo => {
                self.redo.push(super::Undo::Account(choice.reversed()));
                self.light = Some((row, std::time::Instant::now()));
                self.toast(undone, ToastAction::None, UNDONE)
            }
            Doing::Redo => {
                self.undo.push(super::Undo::Account(choice));
                self.light = Some((row, std::time::Instant::now()));
                self.toast(redone, ToastAction::None, UNDONE)
            }
        }
    }

    /// `choice` undone (or redone): its value asked back.
    pub(super) fn account_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let (choice, doing) = if redo {
            (choice, Doing::Redo)
        } else {
            (choice.reversed(), Doing::Undo)
        };
        self.go(Page::Section(SectionId::Account));
        match &choice {
            Choice::Name { after, .. } => {
                let before = self.account_said_name();
                if self.account.ask(Ask::Name(after.clone())) {
                    self.account
                        .asked
                        .push(Asked::Name(before, doing, Some(choice)));
                }
            }
            Choice::Picture { after, .. } => {
                let ask = after.clone().map_or(Ask::Remove, Ask::Bytes);
                self.account_picture(ask, doing, Some(choice));
            }
        }
        Task::none()
    }

    /// A picture asked of AccountsService.
    fn account_picture(&mut self, ask: Ask, doing: Doing, choice: Option<Choice>) {
        let before = self.account.picture_bytes();
        if self.account.ask(ask) {
            self.account.before = before;
            self.account.asked.push(Asked::Picture(doing, choice));
        }
    }

    /// The name typed, saved if it changed and is not empty.
    fn account_save_name(&mut self) -> Task<Message> {
        // One at a time: the one asked shows until it is answered.
        if self
            .account
            .asked
            .iter()
            .any(|a| matches!(a, Asked::Name(..)))
        {
            return Task::none();
        }
        let Some(draft) = self.account.draft.take() else {
            return Task::none();
        };
        let name = draft.trim().to_owned();
        let Some(account) = self.account.account() else {
            return Task::none();
        };
        if name.is_empty() || name == account.real_name {
            return Task::none();
        }
        let before = self.account_said_name();
        if self.account.ask(Ask::Name(name.clone())) {
            self.account
                .asked
                .push(Asked::Name(before, Doing::New, None));
            // Shown as typed until AccountsService says it.
            self.account.draft = Some(name);
        }
        Task::none()
    }

    /// The password's outcome said on the sheet, or the sheet closed.
    fn account_changed(&mut self, outcome: Outcome) -> Task<Message> {
        let Some(sheet) = self.account.sheet.as_mut() else {
            return Task::none();
        };
        sheet.running = false;
        let lang = self.lang;
        match outcome {
            Outcome::Changed => {
                tracing::info!("the password is changed");
                sheet.clear();
                self.account.sheet = None;
                let text = w_owned("Mot de passe changé.", "Password changed.", lang);
                return self.toast(text, ToastAction::None, TOAST);
            }
            Outcome::WrongCurrent => {
                tracing::info!("passwd: not the current password");
                sheet.wrong_current = true;
                sheet.at = 0;
                return Task::batch([
                    operation::focus(FIELDS[0]),
                    operation::select_all(FIELDS[0]),
                ]);
            }
            Outcome::Refused(why) => {
                tracing::info!("passwd: the new password is refused");
                sheet.refused = Some(why);
                sheet.at = 1;
                return operation::focus(FIELDS[1]);
            }
            Outcome::Failed(why) => {
                sheet.failed = Some(match lang {
                    Lang::French => format!("Le mot de passe n’a pas changé : {why}."),
                    Lang::English => format!("The password has not changed: {why}."),
                });
            }
            Outcome::NoAnswer => {
                sheet.failed = Some(w_owned(
                    "Le système ne répond pas.",
                    "The system does not answer.",
                    lang,
                ));
            }
        }
        Task::none()
    }

    /// After each message: the worker started the first time the page
    /// shows; the name typed saved and the popover closed when it goes.
    pub(super) fn account_after(&mut self) -> Task<Message> {
        if !self.account_shows() {
            self.account.popover = None;
            if self.account.sheet.as_ref().is_some_and(|s| !s.running) {
                self.account.sheet = None;
            }
            if self.account.draft.is_some() && self.account.asked.is_empty() {
                return self.account_save_name();
            }
            return Task::none();
        }
        // Whole frames while the popover or the sheet changes (as the
        // picker's, `background_of`).
        let drawn = (
            self.account.popover,
            self.account.sheet.as_ref().map(|s| {
                (
                    s.running,
                    s.wrong_current,
                    s.refused.is_some(),
                    s.failed.is_some(),
                    s.matching(),
                    s.shown,
                )
            }),
        );
        if drawn != self.account.drawn {
            self.account.drawn = drawn;
            self.fresh = Some(std::time::Instant::now() + super::FRESH);
        }
        if self.account.asks.is_none() {
            let to = self.account.to.clone();
            self.account.asks = Some(accounts::start(move |heard| {
                to.as_ref().is_some_and(|to| {
                    to.unbounded_send(Message::Account(Msg::Heard(heard)))
                        .is_ok()
                })
            }));
            tracing::info!("AccountsService followed");
        }
        Task::none()
    }

    /// While the name is typed: a press anywhere asks whether its field
    /// still holds the keyboard.
    pub(super) fn account_subscription(&self) -> Option<iced::Subscription<Message>> {
        (self.account_shows() && self.account.draft.is_some()).then(|| {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => {
                    Some(Message::Account(Msg::Pressed))
                }
                _ => None,
            })
        })
    }

    /// A key the page takes before the window does: the popover's, the
    /// sheet's, `Esc` and `Tab` in the name's field.
    pub(super) fn account_key_first(&mut self, key: &Key) -> Option<Task<Message>> {
        if !self.account_shows() {
            return None;
        }
        if let Some(sheet) = self.account.sheet.as_mut() {
            let task = match key.as_ref() {
                Key::Named(Named::Escape) if !sheet.running => {
                    self.account.sheet = None;
                    Task::none()
                }
                Key::Named(Named::Tab) => {
                    sheet.at = (sheet.at + 1) % FIELDS.len();
                    operation::focus(FIELDS[sheet.at])
                }
                _ => Task::none(),
            };
            return Some(task);
        }
        if let Some(at) = self.account.popover {
            let task = match key.as_ref() {
                Key::Named(Named::Escape) => {
                    self.account.popover = None;
                    Task::none()
                }
                Key::Named(Named::ArrowRight) => {
                    self.account.popover = Some((at + 1).min(model::PANDAS.len() - 1));
                    Task::none()
                }
                Key::Named(Named::ArrowLeft) => {
                    self.account.popover = Some(at.saturating_sub(1));
                    Task::none()
                }
                Key::Named(Named::ArrowDown) => {
                    self.account.popover = Some((at + 4).min(model::PANDAS.len() - 1));
                    Task::none()
                }
                Key::Named(Named::ArrowUp) => {
                    self.account.popover = Some(at.saturating_sub(4));
                    Task::none()
                }
                Key::Named(Named::Enter | Named::Space) => {
                    return Some(self.account_update(Msg::Panda(at)));
                }
                _ => Task::none(),
            };
            return Some(task);
        }
        if self.account.draft.is_some() {
            match key.as_ref() {
                // Put back as it was, the field let go.
                Key::Named(Named::Escape) => {
                    self.account.draft = None;
                    return Some(operation::focus("nothing"));
                }
                Key::Named(Named::Tab) => {
                    let _ = self.account_save_name();
                }
                _ => {}
            }
        }
        None
    }

    /// The page's controls, in `Tab`'s order.
    pub(super) fn account_stops(&self) -> Vec<Stop> {
        let mut stops = Vec::new();
        if self.account.account().is_some() {
            stops.push(Stop::Avatar);
        }
        if self.account.passwd {
            stops.push(Stop::Password);
        }
        stops.push(Stop::Lock);
        stops
    }

    pub(super) fn account_key(&mut self, stop: Stop, named: Named) -> Task<Message> {
        if !matches!(named, Named::Enter | Named::Space) {
            return Task::none();
        }
        self.account_update(match stop {
            Stop::Avatar => Msg::Popover(true),
            Stop::Password => Msg::Sheet(true),
            Stop::Lock => Msg::Lock,
        })
    }

    pub(super) fn account_focused(&self, stop: Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i) if self.controls().get(i) == Some(&super::Control::Account(stop)))
    }
}

fn w_owned(fr: &'static str, en: &'static str, lang: Lang) -> String {
    w(fr, en).get(lang).to_owned()
}
