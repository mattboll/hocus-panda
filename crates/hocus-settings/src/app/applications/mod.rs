//! The Applications page (6.15, settings.md "Applications", variant A):
//! the default applications (a menu per row: the browser, the mail, the
//! terminal, the videos, the images), then the start-up applications (a
//! switch each, a ✕ for those the user added, "+ Ajouter une application"
//! and its sheet).
//!
//! What the page shows is read by its thread ([`crate::applications`]),
//! started the first time the page shows; every write is done there, and
//! the page shows the files as last read. A choice toasts with "Annuler"
//! and joins `Ctrl+Z`; the terminal is the file's `[programs] terminal`,
//! written as the other settings are.

mod view;

use hocus_words::settings::app_applications as words;
use std::{
    collections::{BTreeMap, HashMap},
    time::Instant,
};

use iced::{
    Task,
    futures::channel::mpsc::UnboundedSender,
    keyboard::{Key, Modifiers, key::Named},
    widget::{image, operation, svg},
};

use super::{App, Doing, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{
    applications::{
        self as model, ADDED, Ask, Done, Heard, Known, Opens, Picture, TERMINAL, Worker,
    },
    lang::{Lang, pair},
    model::{SectionId, Value},
    store::Slot,
};

/// The terminal's field ("Autre commande…") and the sheet's search.
pub(super) const FIELD: &str = "applications-terminal";
pub(super) const SEARCH: &str = "applications-search";

/// A menu of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Default(Opens),
    Terminal,
}

impl Menu {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Default(opens) => opens.id(),
            Self::Terminal => TERMINAL,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Heard),
    Menu(Menu),
    CloseMenu,
    /// Choice `n` of a menu.
    Choose(Menu, usize),
    /// The terminal's command typed, written (`Enter`, or the field let
    /// go).
    Typed(String),
    Commit,
    /// A press while the command is typed: whether its field still holds
    /// the keyboard.
    Pressed,
    FieldFocused(bool),
    /// A start-up entry's switch, its ✕.
    Switch(String),
    Remove(String),
    /// "Les voir".
    Others,
    /// The sheet opened or closed, its search, an application picked
    /// (by its place in the list), "Ajouter".
    Sheet(bool),
    Query(String),
    Pick(usize),
    Add,
}

/// A control of the page that takes the keyboard, in `Tab`'s order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Menu(Menu),
    Add,
    Switch(String),
    Remove(String),
    Others,
}

/// What the toasts say of a change: done, undone, redone; the row lit by
/// an undo (a start-up entry's id, or a default row's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    done: String,
    undone: String,
    redone: String,
    row: Row,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Default(Opens),
    Startup(String),
}

/// A change of the page, for its undo: the file before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Defaults {
        opens: Opens,
        before: Option<String>,
        after: Option<String>,
        lines_before: Vec<(String, Option<String>)>,
        lines_after: Vec<(String, Option<String>)>,
        said: Said,
    },
    Startup {
        id: String,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
        said: Said,
    },
}

impl Choice {
    fn reversed(self) -> Self {
        match self {
            Self::Defaults {
                opens,
                before,
                after,
                lines_before,
                lines_after,
                said,
            } => Self::Defaults {
                opens,
                before: after,
                after: before,
                lines_before: lines_after,
                lines_after: lines_before,
                said,
            },
            Self::Startup {
                id,
                before,
                after,
                said,
            } => Self::Startup {
                id,
                before: after,
                after: before,
                said,
            },
        }
    }

    fn said(&self) -> &Said {
        match self {
            Self::Defaults { said, .. } | Self::Startup { said, .. } => said,
        }
    }
}

/// An application's icon, as the window draws it.
#[derive(Debug, Clone)]
pub(super) enum Icon {
    Image(image::Handle),
    Svg(svg::Handle),
}

/// The sheet: what is typed, the application the keyboard is on.
#[derive(Debug, Clone, Default)]
pub(super) struct Sheet {
    pub(super) query: String,
    pub(super) at: usize,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    to: Option<UnboundedSender<Message>>,
    worker: Option<Worker>,
    pub(super) known: Option<Known>,
    pub(super) icons: HashMap<String, Icon>,
    /// A menu open, since then, the choice the keyboard is on.
    pub(super) menu: Option<(Menu, Instant)>,
    pub(super) menu_at: usize,
    /// The terminal's command as typed ("Autre commande…"), while it is.
    pub(super) typing: Option<String>,
    /// A command whose program is not in `PATH`: not written.
    pub(super) missing: Option<String>,
    /// "Les voir": the entries for other desktops shown.
    pub(super) others: bool,
    pub(super) sheet: Option<Sheet>,
    /// The writes asked, by serial: what their toasts say, why.
    asked: BTreeMap<u64, (Said, Doing)>,
    serial: u64,
    /// The start-up entry added last: it comes last, lit.
    pub(super) added: Option<String>,
    /// The start-up entry lit by an undo.
    pub(super) lit: Option<String>,
}

impl State {
    pub fn new(to: UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to),
            ..Self::default()
        }
    }

    fn ask(&mut self, ask: impl FnOnce(u64) -> Ask, said: Said, doing: Doing) -> bool {
        self.serial += 1;
        let serial = self.serial;
        let sent = self.worker.as_ref().is_some_and(|w| w.ask(ask(serial)));
        if sent {
            self.asked.insert(serial, (said, doing));
        } else {
            tracing::warn!("the applications' thread has stopped: nothing written");
        }
        sent
    }
}

fn words(fr: &'static str, en: &'static str, lang: Lang) -> String {
    pair(fr, en).get(lang).to_owned()
}

impl App {
    fn apps_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Apps)
    }

    fn terminal_slot(&self) -> Option<Slot> {
        crate::model::setting(TERMINAL).map(|s| Slot::of(s, self.source))
    }

    /// `[programs] terminal` as the file says it.
    pub(super) fn terminal_value(&self) -> String {
        match self.terminal_slot().and_then(|slot| self.value(slot)) {
            Some(Value::Text(text)) => text,
            _ => String::new(),
        }
    }

    /// The application named `id`, else the id.
    pub(super) fn apps_name(&self, id: &str) -> String {
        self.apps
            .known
            .as_ref()
            .and_then(|k| k.app(id))
            .map_or_else(|| id.to_owned(), |a| a.name.clone())
    }

    /// What the terminal's menu offers: "Automatique", the terminals
    /// installed, "Autre commande…"; by their ids (`""` automatic, `None`
    /// another command).
    pub(super) fn terminal_choices(&self) -> Vec<Option<String>> {
        let mut out = vec![Some(String::new())];
        if let Some(known) = &self.apps.known {
            out.extend(known.terminals().into_iter().map(|a| Some(a.id.clone())));
        }
        out.push(None);
        out
    }

    /// The terminal the file names: `Some("")` automatic, an installed
    /// terminal's id, or `None`: another command.
    pub(super) fn terminal_chosen(&self) -> Option<String> {
        let value = self.terminal_value();
        if value.trim().is_empty() {
            return Some(String::new());
        }
        let known = self.apps.known.as_ref()?;
        known
            .terminals()
            .into_iter()
            .find(|a| a.command == value.trim() || a.program == value.trim())
            .map(|a| a.id.clone())
    }

    /// The applications of a menu, by id.
    pub(super) fn menu_ids(&self, menu: Menu) -> Vec<Option<String>> {
        match menu {
            Menu::Terminal => self.terminal_choices(),
            Menu::Default(opens) => self
                .apps
                .known
                .as_ref()
                .map(|k| {
                    k.handlers(opens)
                        .into_iter()
                        .map(|a| Some(a.id.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The choice a menu shows chosen.
    pub(super) fn menu_chosen(&self, menu: Menu) -> Option<usize> {
        let ids = self.menu_ids(menu);
        let chosen = match menu {
            Menu::Terminal => self.terminal_chosen(),
            Menu::Default(opens) => self
                .apps
                .known
                .as_ref()
                .and_then(|k| k.chosen(opens))
                .map(str::to_owned),
        };
        ids.iter().position(|id| *id == chosen)
    }

    /// The sheet's applications: those not at start-up, matching what is
    /// typed (every word, accents and case aside), those starting with it
    /// first.
    pub(super) fn sheet_list(&self) -> Vec<&model::App> {
        let (Some(known), Some(sheet)) = (self.apps.known.as_ref(), self.apps.sheet.as_ref())
        else {
            return Vec::new();
        };
        let query = crate::search::fold(sheet.query.trim());
        let mut list: Vec<(bool, &model::App)> = known
            .addable()
            .into_iter()
            .filter_map(|a| {
                let name = crate::search::fold(&a.name);
                let id = crate::search::fold(&a.id);
                let all = query
                    .split_whitespace()
                    .all(|word| name.contains(word) || id.contains(word));
                all.then(|| (!name.starts_with(&query), a))
            })
            .collect();
        list.sort_by_key(|(later, _)| *later);
        list.into_iter().map(|(_, a)| a).collect()
    }

    // --- What changes it ----------------------------------------------------------------

    pub(super) fn apps_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(heard) => return self.apps_heard(heard),
            Msg::Menu(menu) => {
                let same = self.apps.menu.is_some_and(|(m, _)| m == menu);
                let empty = self.menu_ids(menu).is_empty();
                self.apps.menu =
                    (!same && !empty && self.apps_writable(menu)).then(|| (menu, Instant::now()));
                self.apps.menu_at = self.menu_chosen(menu).unwrap_or(0);
            }
            Msg::CloseMenu => self.apps.menu = None,
            Msg::Choose(menu, n) => {
                self.apps.menu = None;
                return self.apps_choose(menu, n);
            }
            Msg::Typed(text) => {
                self.apps.typing = Some(text);
                self.apps.missing = None;
            }
            Msg::Commit => return self.terminal_commit(),
            Msg::Pressed => {
                return operation::is_focused(FIELD)
                    .map(|focused| Message::Apps(Msg::FieldFocused(focused)));
            }
            Msg::FieldFocused(focused) => {
                if !focused {
                    return self.terminal_commit();
                }
            }
            Msg::Switch(id) => self.startup_switch(&id),
            Msg::Remove(id) => self.startup_remove(&id),
            Msg::Others => self.apps.others = !self.apps.others,
            Msg::Sheet(open) => {
                self.apps.menu = None;
                if !open {
                    self.apps.sheet = None;
                    return Task::none();
                }
                self.apps.sheet = Some(model_sheet());
                return operation::focus(SEARCH);
            }
            Msg::Query(query) => {
                if let Some(sheet) = self.apps.sheet.as_mut() {
                    sheet.query = query;
                    sheet.at = 0;
                }
            }
            Msg::Pick(i) => {
                if let Some(sheet) = self.apps.sheet.as_mut() {
                    sheet.at = i;
                }
            }
            Msg::Add => self.startup_add(),
        }
        Task::none()
    }

    /// Whether a menu can change: the file reads for the terminal, the
    /// thread is there for the others.
    fn apps_writable(&self, menu: Menu) -> bool {
        match menu {
            Menu::Terminal => self.writable(),
            Menu::Default(_) => self.apps.worker.is_some(),
        }
    }

    fn apps_choose(&mut self, menu: Menu, n: usize) -> Task<Message> {
        let lang = self.lang;
        let Some(choice) = self.menu_ids(menu).get(n).cloned() else {
            return Task::none();
        };
        match (menu, choice) {
            (Menu::Default(opens), Some(id)) => {
                let Some(known) = self.apps.known.as_ref() else {
                    return Task::none();
                };
                // Already every type's: nothing to write.
                if known
                    .defaults_of(opens)
                    .iter()
                    .all(|(_, app)| app.as_deref() == Some(id.as_str()))
                {
                    return Task::none();
                }
                let before = known
                    .chosen(opens)
                    .map(|i| self.apps_name(i))
                    .unwrap_or_else(|| words("aucune", "none", lang));
                let name = self.apps_name(&id);
                let row = opens.name().get(lang);
                let said = Said {
                    done: format!("{row}{}{name}", sep(lang)),
                    undone: words::undone(lang, &before, row),
                    redone: words::redone(lang, &name, row),
                    row: Row::Default(opens),
                };
                self.apps.ask(
                    |serial| Ask::Default { opens, id, serial },
                    said,
                    Doing::New,
                );
                Task::none()
            }
            (Menu::Default(_), None) => Task::none(),
            (Menu::Terminal, Some(id)) => {
                self.apps.typing = None;
                self.apps.missing = None;
                let Some(slot) = self.terminal_slot() else {
                    return Task::none();
                };
                let (value, name) = if id.is_empty() {
                    (String::new(), words("Automatique", "Automatic", lang))
                } else {
                    let Some(app) = self.apps.known.as_ref().and_then(|k| k.app(&id)) else {
                        return Task::none();
                    };
                    (app.command.clone(), app.name.clone())
                };
                let say = format!("Terminal{}{name}", sep(lang));
                self.change_saying(slot, Value::Text(value), Some(say))
            }
            (Menu::Terminal, None) => {
                // "Autre commande…": the field, with what the file holds
                // when it is another command.
                let value = self.terminal_value();
                let text = if self.terminal_chosen().is_none() {
                    value
                } else {
                    String::new()
                };
                self.apps.typing = Some(text);
                self.apps.missing = None;
                operation::focus(FIELD)
            }
        }
    }

    /// The command typed: its program looked for in `PATH` (on the
    /// thread), then written if it is there.
    fn terminal_commit(&mut self) -> Task<Message> {
        let Some(text) = self.apps.typing.clone() else {
            return Task::none();
        };
        let command = text.trim().to_owned();
        if command.is_empty() || command == self.terminal_value().trim() {
            self.apps.typing = None;
            return Task::none();
        }
        if let Some(worker) = &self.apps.worker {
            worker.ask(Ask::Check { command });
        }
        Task::none()
    }

    fn apps_checked(&mut self, command: String, found: bool) -> Task<Message> {
        if self.apps.typing.as_deref().map(str::trim) != Some(command.as_str()) {
            return Task::none();
        }
        if !found {
            let program = model::program_of(&command).unwrap_or(command);
            tracing::info!(program, "a terminal not in PATH: not written");
            self.apps.missing = Some(program);
            return Task::none();
        }
        self.apps.typing = None;
        self.apps.missing = None;
        let Some(slot) = self.terminal_slot() else {
            return Task::none();
        };
        let say = format!("Terminal{}{command}", sep(self.lang));
        self.change_saying(slot, Value::Text(command), Some(say))
    }

    fn startup_switch(&mut self, id: &str) {
        let lang = self.lang;
        let Some(entry) = self
            .apps
            .known
            .as_ref()
            .and_then(|k| k.startup.iter().find(|e| e.id == id))
        else {
            return;
        };
        let on = entry.hidden;
        let name = entry.name.clone();
        let yes = |on: bool| {
            if on {
                words("oui", "on", lang)
            } else {
                words("non", "off", lang)
            }
        };
        let said = match lang {
            Lang::French => Said {
                done: format!("{name} au démarrage : {}", yes(on)),
                undone: format!("Annulé : {name} au démarrage : {}", yes(!on)),
                redone: format!("Rétabli : {name} au démarrage : {}", yes(on)),
                row: Row::Startup(id.to_owned()),
            },
            Lang::English => Said {
                done: format!("{name} at start-up: {}", yes(on)),
                undone: format!("Undone: {name} at start-up: {}", yes(!on)),
                redone: format!("Redone: {name} at start-up: {}", yes(on)),
                row: Row::Startup(id.to_owned()),
            },
        };
        let id = id.to_owned();
        self.apps
            .ask(|serial| Ask::Switch { id, on, serial }, said, Doing::New);
    }

    fn startup_remove(&mut self, id: &str) {
        let lang = self.lang;
        let Some(entry) = self
            .apps
            .known
            .as_ref()
            .and_then(|k| k.startup.iter().find(|e| e.id == id))
        else {
            return;
        };
        if !model::users_own(entry) {
            return;
        }
        let name = entry.name.clone();
        let said = match lang {
            Lang::French => Said {
                done: format!("{name} enlevée du démarrage"),
                undone: format!("Annulé : {name} revient au démarrage"),
                redone: format!("Rétabli : {name} enlevée du démarrage"),
                row: Row::Startup(id.to_owned()),
            },
            Lang::English => Said {
                done: format!("{name} removed from start-up"),
                undone: format!("Undone: {name} is back at start-up"),
                redone: format!("Redone: {name} removed from start-up"),
                row: Row::Startup(id.to_owned()),
            },
        };
        let id = id.to_owned();
        self.apps
            .ask(|serial| Ask::Remove { id, serial }, said, Doing::New);
    }

    fn startup_add(&mut self) {
        let lang = self.lang;
        let at = self.apps.sheet.as_ref().map_or(0, |s| s.at);
        let Some(app) = self
            .sheet_list()
            .get(at)
            .map(|a| (a.id.clone(), a.name.clone()))
        else {
            return;
        };
        let (id, name) = app;
        self.apps.sheet = None;
        let said = match lang {
            Lang::French => Said {
                done: format!("{name} ajoutée au démarrage"),
                undone: format!("Annulé : {name} n’est plus au démarrage"),
                redone: format!("Rétabli : {name} ajoutée au démarrage"),
                row: Row::Startup(id.clone()),
            },
            Lang::English => Said {
                done: format!("{name} added to start-up"),
                undone: format!("Undone: {name} is no longer at start-up"),
                redone: format!("Redone: {name} added to start-up"),
                row: Row::Startup(id.clone()),
            },
        };
        self.apps.added = Some(id.clone());
        self.apps
            .ask(|serial| Ask::Add { id, serial }, said, Doing::New);
    }

    fn apps_heard(&mut self, heard: Heard) -> Task<Message> {
        match heard {
            Heard::Known(known) => {
                self.apps.known = Some(*known);
                // A menu whose choices went, closed.
                if let Some((menu, _)) = self.apps.menu
                    && self.apps.menu_at >= self.menu_ids(menu).len()
                {
                    self.apps.menu = None;
                }
                Task::none()
            }
            Heard::Icons(icons) => {
                for (name, picture) in icons {
                    let icon = match picture {
                        Picture::Rgba {
                            width,
                            height,
                            pixels,
                        } => Icon::Image(image::Handle::from_rgba(width, height, pixels)),
                        Picture::Svg(bytes) => Icon::Svg(svg::Handle::from_memory(bytes)),
                    };
                    self.apps.icons.insert(name, icon);
                }
                Task::none()
            }
            Heard::Checked { command, found } => self.apps_checked(command, found),
            Heard::Done { serial, result } => {
                let Some((said, doing)) = self.apps.asked.remove(&serial) else {
                    return Task::none();
                };
                let done = match result {
                    Ok(done) => done,
                    Err(why) => {
                        tracing::warn!(why, "an application's change not written");
                        let text = words::not_written(self.lang, &why);
                        return self.toast(text, ToastAction::None, TOAST);
                    }
                };
                let choice = match done {
                    Done::Defaults {
                        opens,
                        before,
                        after,
                        lines_before,
                        lines_after,
                    } => Choice::Defaults {
                        opens,
                        before,
                        after,
                        lines_before,
                        lines_after,
                        said,
                    },
                    Done::Startup { id, before, after } => Choice::Startup {
                        id,
                        before,
                        after,
                        said,
                    },
                };
                self.apps_done(choice, doing)
            }
        }
    }

    /// A change written: kept for its undo, and said.
    fn apps_done(&mut self, choice: Choice, doing: Doing) -> Task<Message> {
        let said = choice.said().clone();
        let light = |app: &mut Self| {
            let now = Instant::now();
            match &said.row {
                Row::Default(opens) => app.light = Some((opens.id(), now)),
                Row::Startup(id) => {
                    app.apps.lit = Some(id.clone());
                    app.light = Some((ADDED, now));
                }
            }
        };
        match doing {
            Doing::New => {
                // The entry just added comes last, lit as a search result.
                if let (
                    Choice::Startup {
                        id,
                        before: None,
                        after: Some(_),
                        ..
                    },
                    Some(added),
                ) = (&choice, &self.apps.added)
                    && id == added
                {
                    self.apps.lit = Some(id.clone());
                    self.light = Some((ADDED, Instant::now()));
                }
                self.undo.push(super::Undo::Apps(choice));
                self.redo.clear();
                self.toast(said.done, ToastAction::Undo, TOAST)
            }
            Doing::Undo => {
                self.redo.push(super::Undo::Apps(choice.reversed()));
                light(self);
                self.toast(said.undone, ToastAction::None, UNDONE)
            }
            Doing::Redo => {
                self.undo.push(super::Undo::Apps(choice));
                light(self);
                self.toast(said.redone, ToastAction::None, UNDONE)
            }
        }
    }

    /// `choice` undone (or redone): the file put back as it was.
    pub(super) fn apps_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let (choice, doing) = if redo {
            (choice, Doing::Redo)
        } else {
            (choice.reversed(), Doing::Undo)
        };
        self.go(Page::Section(SectionId::Apps));
        let said = choice.said().clone();
        match choice {
            Choice::Defaults {
                opens,
                before,
                after,
                lines_after,
                ..
            } => {
                self.apps.ask(
                    |serial| Ask::DefaultsBack {
                        opens,
                        to: after,
                        expect: before,
                        lines: lines_after,
                        serial,
                    },
                    said,
                    doing,
                );
            }
            Choice::Startup { id, after, .. } => {
                self.apps.ask(
                    |serial| Ask::Put {
                        id,
                        bytes: after,
                        serial,
                    },
                    said,
                    doing,
                );
            }
        }
        Task::none()
    }

    /// After each message: the thread started the first time the page
    /// shows; the menus and the sheet closed when it goes.
    pub(super) fn apps_after(&mut self) -> Task<Message> {
        if !self.apps_shows() {
            self.apps.menu = None;
            self.apps.sheet = None;
            if self.apps.typing.is_some() {
                return self.terminal_commit();
            }
            return Task::none();
        }
        if self.apps.worker.is_none() {
            let to = self.apps.to.clone();
            self.apps.worker = model::start(move |heard| {
                to.as_ref()
                    .is_some_and(|to| to.unbounded_send(Message::Apps(Msg::Heard(heard))).is_ok())
            });
            tracing::info!("the applications followed");
        }
        Task::none()
    }

    pub(super) fn apps_moving(&self, now: Instant) -> bool {
        !self.reduced_motion
            && self
                .apps
                .menu
                .is_some_and(|(_, since)| now.saturating_duration_since(since) < super::MENU_IN)
    }

    /// While the command is typed: a press anywhere asks whether its field
    /// still holds the keyboard.
    pub(super) fn apps_subscription(&self) -> Option<iced::Subscription<Message>> {
        (self.apps_shows() && self.apps.typing.is_some()).then(|| {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => {
                    Some(Message::Apps(Msg::Pressed))
                }
                _ => None,
            })
        })
    }

    /// A key the page takes before the window does: its menu's, the
    /// sheet's, the command's field.
    pub(super) fn apps_key_first(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
    ) -> Option<Task<Message>> {
        if !self.apps_shows() {
            return None;
        }
        if self.apps.sheet.is_some() {
            let len = self.sheet_list().len();
            let sheet = self.apps.sheet.as_mut()?;
            let task = match key.as_ref() {
                Key::Named(Named::Escape) => {
                    self.apps.sheet = None;
                    Task::none()
                }
                Key::Named(Named::ArrowDown) => {
                    sheet.at = (sheet.at + 1).min(len.saturating_sub(1));
                    Task::none()
                }
                Key::Named(Named::ArrowUp) => {
                    sheet.at = sheet.at.saturating_sub(1);
                    Task::none()
                }
                Key::Named(Named::Enter) => {
                    self.startup_add();
                    Task::none()
                }
                _ => return Some(Task::none()),
            };
            return Some(task);
        }
        if let Some((menu, _)) = self.apps.menu {
            let len = self.menu_ids(menu).len();
            let task = match key.as_ref() {
                Key::Named(Named::Escape) => {
                    self.apps.menu = None;
                    Task::none()
                }
                Key::Named(Named::ArrowDown) => {
                    self.apps.menu_at = (self.apps.menu_at + 1).min(len.saturating_sub(1));
                    Task::none()
                }
                Key::Named(Named::ArrowUp) => {
                    self.apps.menu_at = self.apps.menu_at.saturating_sub(1);
                    Task::none()
                }
                Key::Named(Named::Enter | Named::Space) => {
                    let at = self.apps.menu_at;
                    return Some(self.apps_update(Msg::Choose(menu, at)));
                }
                Key::Named(Named::Tab) => {
                    self.apps.menu = None;
                    return None;
                }
                _ => Task::none(),
            };
            return Some(task);
        }
        if self.apps.typing.is_some() {
            match key.as_ref() {
                // Put back as it was, the field let go.
                Key::Named(Named::Escape) => {
                    self.apps.typing = None;
                    self.apps.missing = None;
                    return Some(operation::focus("nothing"));
                }
                Key::Named(Named::Tab) => {
                    let _ = self.terminal_commit();
                }
                _ => {}
            }
        }
        let _ = modifiers;
        None
    }

    /// The page's controls, in `Tab`'s order: its own, and the last
    /// session's switch (a setting of the model) after "+ Ajouter".
    pub(super) fn apps_controls(&self) -> Vec<super::Control> {
        let mut out = Vec::new();
        for stop in self.apps_stops() {
            let add = stop == Stop::Add;
            out.push(super::Control::Apps(stop));
            if add && let Some(setting) = crate::model::setting(model::RESTORE) {
                out.push(super::Control::Setting(Slot::of(setting, self.source)));
            }
        }
        out
    }

    /// The page's own controls, in `Tab`'s order.
    pub(super) fn apps_stops(&self) -> Vec<Stop> {
        let mut stops = Vec::new();
        let Some(known) = &self.apps.known else {
            return stops;
        };
        for opens in [Opens::Browser, Opens::Mail] {
            if !known.handlers(opens).is_empty() {
                stops.push(Stop::Menu(Menu::Default(opens)));
            }
        }
        if self.apps.typing.is_none() {
            stops.push(Stop::Menu(Menu::Terminal));
        }
        for opens in [Opens::Videos, Opens::Images] {
            if !known.handlers(opens).is_empty() {
                stops.push(Stop::Menu(Menu::Default(opens)));
            }
        }
        stops.push(Stop::Add);
        for entry in self.startup_rows() {
            stops.push(Stop::Switch(entry.id.clone()));
            if model::users_own(entry) {
                stops.push(Stop::Remove(entry.id.clone()));
            }
        }
        if !known.others().is_empty() {
            stops.push(Stop::Others);
        }
        stops
    }

    /// The start-up rows in their order: by name, the one added last at
    /// the end.
    pub(super) fn startup_rows(&self) -> Vec<&hocus_xdg::autostart::Entry> {
        let Some(known) = &self.apps.known else {
            return Vec::new();
        };
        let mut rows = known.rows();
        if let Some(added) = &self.apps.added
            && let Some(at) = rows.iter().position(|e| &e.id == added)
        {
            let entry = rows.remove(at);
            rows.push(entry);
        }
        rows
    }

    pub(super) fn apps_key(
        &mut self,
        stop: Stop,
        named: Named,
        modifiers: Modifiers,
    ) -> Task<Message> {
        let msg = match (&stop, named) {
            (Stop::Menu(menu), Named::Enter | Named::Space) => Msg::Menu(*menu),
            (Stop::Menu(menu), Named::ArrowDown) if modifiers.alt() => Msg::Menu(*menu),
            (Stop::Switch(id), Named::Enter | Named::Space) => Msg::Switch(id.clone()),
            // `Delete` on a user's row's switch removes it, as its ✕.
            (Stop::Switch(id), Named::Delete) => Msg::Remove(id.clone()),
            (Stop::Remove(id), Named::Enter | Named::Space | Named::Delete) => {
                Msg::Remove(id.clone())
            }
            (Stop::Add, Named::Enter | Named::Space) => Msg::Sheet(true),
            (Stop::Others, Named::Enter | Named::Space) => Msg::Others,
            _ => return Task::none(),
        };
        self.apps_update(msg)
    }

    pub(super) fn apps_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&super::Control::Apps(stop.clone())))
    }
}

/// The separator of a toast's row and value: " : " in French, ": " in
/// English.
fn sep(lang: Lang) -> &'static str {
    words::TEXT.get(lang)
}

fn model_sheet() -> Sheet {
    Sheet::default()
}
