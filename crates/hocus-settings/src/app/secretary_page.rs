//! The "Secrétaire" page (#759, the approved mockup ux2 §6,
//! docs/design/secretary.md §22): her own page, after Agents.
//!
//! LA SECRÉTAIRE: her switch (`[secretary] mode`), her name (a field of its
//! own, written as it is left), her key (given to her on her socket, never
//! shown, never in a file of the window's) and what she knows. In classic
//! mode only her switch shows.
//!
//! CE QU'ELLE LIT, by account: each Google account of Online Accounts a
//! header row (its address, Travail | Privé) with a switch for each kind
//! she may read of it (her calendar, with « Choisir » for its calendars;
//! her mail; her contacts' birthdays); then each application that
//! notified, its switch, and Travail | Privé once she reads it. Switching
//! one on opens its sheet (§10.1: what she reads, what she never does
//! there, what leaves the machine): nothing is written before its
//! « Autoriser ». Switching it off writes a revocation, and so does
//! nothing else; its toast's « Annuler » (and `Ctrl+Z`) reads it again as
//! it was, without the sheet: it was consented to. Travail | Privé writes
//! the notebook of each kind read of the account; on an account she reads
//! nothing of yet, it is the notebook its sheets propose. Her consent book
//! is read and written on threads of the page's, never the window's.
//!
//! LE TRI SUR CET ORDINATEUR: the triage's state as her status says it,
//! when it runs (`[secretary.triage] battery`), its model and whether it
//! answers.
//!
//! Her drawer has no way in from here yet: "Ce qu'elle sait" and her queue
//! say where they are in it.

use std::{collections::HashMap, time::Duration};

use iced::{
    Alignment, Background, Color, Length, Padding, Theme,
    keyboard::{Key, key::Named},
    widget::{Space, button, column, container, row, text, text_input},
};

use super::{
    App, Control, Message, Page, TOAST, ToastAction, UNDONE, Undo,
    style::*,
    view::{El, card_box, divided, label, t},
};
use crate::{
    google_sources::{self, Source as GoogleSource},
    lang::{Lang, w},
    model::{SectionId, Value},
    notifiers::{self, Notifier},
    secretary as model,
    store::Slot,
};
use hocus_secretary_link::{
    KeyOutcome, Notebook, Secret, TriageStatus,
    consent::{ConsentBook, SourceKey, SourceKind},
};

/// Her key's field and her name's (#752).
const KEY_FIELD: &str = "secretary-key";
const NAME_FIELD: &str = "secretary-name";

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The applications whose notifications she may read, as found.
    Notifiers(Vec<Notifier>),
    /// Her Google sources, as found.
    Google(Vec<GoogleSource>),
    /// The `n`th application's switch.
    NotifierSwitch(usize),
    /// Travail | Privé on the `n`th application's row.
    NotifierNotebook(usize, Notebook),
    /// The `n`th Google source's switch.
    GoogleSwitch(usize),
    /// Travail | Privé on an account's line (its Online Accounts id).
    AccountNotebook(String, Notebook),
    /// A sheet's notebook.
    Notebook(Notebook),
    /// The notifications' sheet's "Autoriser" (`true`) or "Pas
    /// maintenant".
    Allowed(bool),
    /// A Google source's sheet's "Autoriser" (`true`) or "Pas maintenant".
    GoogleAllowed(bool),
    /// The sheet's calendar `n`, read or left out (#752).
    SheetCalendar(usize),
    /// « Choisir » on the `n`th Google source's row: its calendars' list.
    Choose(usize),
    /// The calendars' list closed.
    Chosen,
    /// Calendar `n` of the `i`th Google source (an Agenda read), read or
    /// left out.
    RowCalendar(usize, usize),
    /// Her status as she says it; none: she does not answer.
    Status(Option<Heard>),
    /// Her key's field, as typed or pasted.
    KeyTyped(Secret),
    /// "Enregistrer": the key goes to her.
    KeySave,
    /// "Remplacer": the field shows; "Annuler" hides it again.
    KeyReplace(bool),
    /// "Effacer".
    KeyErase,
    /// What she did with the key given.
    Keyed(Result<KeyOutcome, String>),
    /// GNOME's Online Accounts panel, installed or not (#752).
    Panel(Option<std::path::PathBuf>),
    /// « Ajouter un compte Google »: the panel opens.
    AddAccount,
    /// The private notebook's quiet hours, as the file says: what the
    /// sheets' « C'est privé » says.
    Quiet(hocus_secretary_link::gate::Hours),
    /// Her name's field, as typed (#752).
    NameTyped(String),
    /// Her name written (`Enter`, `Tab`, or the field left).
    NameCommit,
}

/// What her status says that the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    /// Her answers' provider and whether her keyring misses its key.
    pub key: Option<(String, bool)>,
    pub triage: Option<TriageStatus>,
    /// The triage's provider's kind, as the file says (`ollama`).
    pub kind: Option<String>,
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The `n`th application's switch.
    Notifier(usize),
    /// The `n`th Google source's switch.
    Google(usize),
    /// Travail | Privé of the `n`th Google source's account (on its first
    /// kind's index).
    Account(usize),
}

/// A source she read as it was, to read it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub source: SourceKey,
    pub identity: String,
    pub notebook: Notebook,
    pub left_out: Vec<String>,
    /// As its toast names it: "Agenda · alex@…", "Slack".
    pub title: String,
}

/// A change of her consent book, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Switched off: undone, she reads it again as she did.
    Revoked(Grant),
    /// Travail | Privé: each source with its notebook before, and the
    /// notebook after; `title`, what the toast names.
    Notebooks {
        to: Notebook,
        before: Vec<(SourceKey, Notebook)>,
        title: String,
    },
}

/// The notifications' consent sheet: for which application, into which
/// notebook, and which button has the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Notify {
    name: String,
    slack: bool,
    notebook: Notebook,
    on_allow: bool,
}

/// A Google source's consent sheet: which, into which notebook, and
/// which button has the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GoogleSheet {
    source: GoogleSource,
    notebook: Notebook,
    on_allow: bool,
    /// An Agenda's calendars left out on it, by their ids (#752).
    left_out: std::collections::BTreeSet<String>,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// The window's messages: what the threads read comes back there.
    to: Option<iced::futures::channel::mpsc::UnboundedSender<Message>>,
    /// The applications whose notifications she may read, as last read.
    notifiers: Vec<Notifier>,
    notify: Option<Notify>,
    /// Her Google sources, as last read.
    google: Vec<GoogleSource>,
    google_sheet: Option<GoogleSheet>,
    /// The calendars' list of this Google source (its word), open.
    choosing: Option<String>,
    /// The notebook chosen on an account she reads nothing of yet, by its
    /// Online Accounts id: what its sheets propose.
    leaning: HashMap<String, Notebook>,
    /// Her answers' key (#752).
    key: KeyRow,
    /// Her status as last said; none before, or when she does not answer.
    heard: Option<Heard>,
    /// Her name as it is being typed, before it is written (#752).
    name: Option<String>,
    /// The private notebook's quiet hours (`[secretary.speak]
    /// quiet-perso`), none before they are read: hers by default.
    quiet: Option<hocus_secretary_link::gate::Hours>,
    /// GNOME's Online Accounts panel, when installed.
    panel: Option<std::path::PathBuf>,
    /// The page was last drawn shown: it reads again as it opens.
    seen: bool,
}

/// The "Clé Anthropic" row: the field (never drawn as it is, overwritten
/// once sent), what became of the last one.
#[derive(Debug, Default)]
struct KeyRow {
    field: Secret,
    /// "Remplacer" clicked: the field shows over a key kept.
    replacing: bool,
    /// The key is with her, being checked.
    busy: bool,
    said: Option<Result<KeyOutcome, String>>,
}

impl State {
    pub fn with(to: &iced::futures::channel::mpsc::UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to.clone()),
            ..Self::default()
        }
    }
}

/// `source`'s kind as its row names it.
fn kind_title(kind: SourceKind, lang: Lang) -> &'static str {
    match kind {
        SourceKind::Calendar => w("Ton agenda", "Your calendar"),
        SourceKind::Mail => w("Tes mails", "Your mail"),
        _ => w(
            "Les anniversaires de tes contacts",
            "Your contacts’ birthdays",
        ),
    }
    .get(lang)
}

/// The accounts of `google`, in their order: each one's index of its first
/// kind, and the indices of its kinds as the mockup lists them (her
/// calendar, her mail, the birthdays).
fn accounts(google: &[GoogleSource]) -> Vec<(usize, Vec<usize>)> {
    let mut out: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, source) in google.iter().enumerate() {
        match out
            .iter_mut()
            .find(|(first, _)| google[*first].account == source.account)
        {
            Some((_, kinds)) => kinds.push(i),
            None => out.push((i, vec![i])),
        }
    }
    let rank = |i: &usize| match google[*i].kind {
        SourceKind::Calendar => 0,
        SourceKind::Mail => 1,
        _ => 2,
    };
    for (_, kinds) in &mut out {
        kinds.sort_by_key(rank);
    }
    out
}

/// `book` changed by `change` on a thread, then her sources read again.
fn book_job(
    to: Option<iced::futures::channel::mpsc::UnboundedSender<Message>>,
    what: &'static str,
    change: impl FnOnce(&mut ConsentBook, jiff::Timestamp) + Send + 'static,
) {
    let Some(to) = to else {
        return;
    };
    let started = std::thread::Builder::new()
        .name("her consent book".to_owned())
        .spawn(move || {
            let written = notifiers::book_path()
                .ok_or_else(|| std::io::Error::other("no data folder"))
                .and_then(|path| {
                    let mut book = ConsentBook::load(&path);
                    change(&mut book, jiff::Timestamp::now());
                    book.save(&path)
                });
            match written {
                Ok(()) => tracing::info!(what, "the secretary's consent book written"),
                Err(err) => tracing::warn!(what, %err, "her consent book is not written"),
            }
            let _ = to.unbounded_send(Message::Secretary(Msg::Notifiers(notifiers::read())));
            let _ = to.unbounded_send(Message::Secretary(Msg::Google(google_sources::read())));
        });
    if let Err(err) = started {
        tracing::warn!(%err, "her consent book is not written");
    }
}

impl App {
    fn secretary_slot(&self, id: &str) -> Option<Slot> {
        crate::model::setting(id).map(|s| Slot::of(s, self.source))
    }

    fn secretary_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Secretary)
    }

    /// The secretary is the assistant (`[secretary] mode`): her rows show.
    fn secretary_assistant(&self) -> bool {
        self.secretary_slot(model::MODE)
            .and_then(|s| self.value(s))
            .is_some_and(|v| v == Value::Bool(true))
    }

    /// The keyboard's stops, in `Tab`'s order.
    pub(super) fn secretary_controls(&self) -> Vec<Control> {
        let slot = |id| self.secretary_slot(id).map(Control::Setting);
        let mut out: Vec<Control> = slot(model::MODE).into_iter().collect();
        if !self.secretary_assistant() {
            return out;
        }
        for (first, kinds) in accounts(&self.secretary.google) {
            out.push(Control::Secretary(Stop::Account(first)));
            out.extend(
                kinds
                    .into_iter()
                    .map(|i| Control::Secretary(Stop::Google(i))),
            );
        }
        out.extend(
            (0..self.secretary.notifiers.len()).map(|i| Control::Secretary(Stop::Notifier(i))),
        );
        out.extend(slot(model::BATTERY));
        out
    }

    /// What she reads, her key, the quiet hours and the panel, read on
    /// threads, said back to the window.
    fn secretary_read(&self) {
        self.secretary_key_job(None);
        let Some(to) = self.secretary.to.clone() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("her sources".to_owned())
            .spawn(move || {
                let _ = to.unbounded_send(Message::Secretary(Msg::Notifiers(notifiers::read())));
                let _ = to.unbounded_send(Message::Secretary(Msg::Quiet(google_sources::quiet())));
                let _ = to.unbounded_send(Message::Secretary(Msg::Panel(google_sources::panel())));
                let _ = to.unbounded_send(Message::Secretary(Msg::Google(google_sources::read())));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "her sources are not read");
        }
    }

    /// Her name as written in the file now.
    fn secretary_name(&self) -> String {
        match self.secretary_slot(model::NAME).and_then(|s| self.value(s)) {
            Some(Value::Text(name)) => name,
            _ => String::new(),
        }
    }

    /// Her name being typed, written if it changed (#752).
    pub(super) fn secretary_name_commit(&mut self) {
        let Some(typed) = self.secretary.name.take() else {
            return;
        };
        let name = typed.trim().to_owned();
        if name == self.secretary_name().trim() || !self.writable() {
            return;
        }
        let Some(slot) = self.secretary_slot(model::NAME) else {
            return;
        };
        let said = match (self.lang, name.is_empty()) {
            (Lang::French, false) => format!("Elle s’appelle {name}"),
            (Lang::French, true) => "Elle redevient « Ta secrétaire »".to_owned(),
            (Lang::English, false) => format!("Her name is {name}"),
            (Lang::English, true) => "She is “Your secretary” again".to_owned(),
        };
        tracing::info!("her name written");
        self.ask(
            slot,
            crate::store::Raw::Set(Value::Text(name)),
            super::Doing::New,
            None,
            None,
            Some(said),
        );
    }

    /// Whether her name is being typed.
    pub(super) fn secretary_naming(&self) -> bool {
        self.secretary.name.is_some()
    }

    /// Her key given (`Some(Some(key))`), erased (`Some(None)`), or only
    /// asked about, on her socket from a thread; then her status.
    fn secretary_key_job(&self, give: Option<Option<Secret>>) {
        let Some(to) = self.secretary.to.clone() else {
            return;
        };
        let Some(socket) = hocus_secretary_link::session_socket() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("her status".to_owned())
            .spawn(move || {
                use hocus_secretary_link::{Client, Connection};
                if let Some(key) = give {
                    let provider =
                        Connection::open(&socket, Client::Settings, Duration::from_secs(5))
                            .and_then(|mut c| c.status())
                            .ok()
                            .and_then(|s| s.key_provider)
                            .unwrap_or_else(|| "anthropic".to_owned());
                    let outcome =
                        hocus_secretary_link::give_key(&socket, Client::Settings, &provider, key);
                    if let Err(err) = &outcome {
                        tracing::info!(%err, "her key could not be given");
                    }
                    let _ = to.unbounded_send(Message::Secretary(Msg::Keyed(outcome)));
                }
                let heard = Connection::open(&socket, Client::Settings, Duration::from_secs(5))
                    .and_then(|mut c| c.status())
                    .ok()
                    .map(|s| {
                        let kind = s.triage.as_ref().and_then(|t| {
                            model::provider_kind(t.model.split_once('/').map_or("", |(p, _)| p))
                        });
                        Heard {
                            key: s.key_provider.map(|p| (p, s.key_missing)),
                            triage: s.triage,
                            kind,
                        }
                    });
                let _ = to.unbounded_send(Message::Secretary(Msg::Status(heard)));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "her status is not asked");
        }
    }

    /// The private notebook's quiet hours, as last read.
    fn secretary_quiet(&self) -> hocus_secretary_link::gate::Hours {
        self.secretary
            .quiet
            .unwrap_or(hocus_secretary_link::gate::Restraint::default().perso)
    }

    fn secretary_focused(&self, stop: Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Secretary(stop)))
    }

    /// The notebook of an account's line: that of the kinds she reads of
    /// it, none when they differ; the one chosen, else suggested, while
    /// she reads none.
    fn secretary_account_notebook(&self, first: usize) -> Option<Notebook> {
        let google = &self.secretary.google;
        let account = google.get(first)?;
        let read: Vec<Notebook> = google
            .iter()
            .filter(|s| s.account == account.account)
            .filter_map(|s| s.granted)
            .collect();
        match read.first() {
            None => Some(
                self.secretary
                    .leaning
                    .get(&account.account)
                    .copied()
                    .unwrap_or(account.suggested),
            ),
            Some(n) if read.iter().all(|m| m == n) => Some(*n),
            Some(_) => None,
        }
    }

    /// A book's change done, its toast with « Annuler », on `Ctrl+Z`'s
    /// stack.
    fn secretary_chose(&mut self, choice: Choice) -> iced::Task<Message> {
        let text = self.secretary_said(&choice, true);
        self.secretary_apply(&choice, false);
        self.undo.push(Undo::Secretary(choice));
        self.redo.clear();
        self.toast(text, ToastAction::Undo, TOAST)
    }

    /// `choice` written (`back`: its other side).
    fn secretary_apply(&self, choice: &Choice, back: bool) {
        let to = self.secretary.to.clone();
        match (choice.clone(), back) {
            (Choice::Revoked(grant), false) => {
                book_job(to, "a source no longer read", move |b, at| {
                    b.revoke(&grant.source, at);
                })
            }
            (Choice::Revoked(grant), true) => book_job(to, "a source read again", move |b, at| {
                b.allow_leaving_out(
                    grant.source,
                    &grant.identity,
                    grant.notebook,
                    grant.left_out,
                    at,
                );
            }),
            (
                Choice::Notebooks {
                    to: notebook,
                    before,
                    ..
                },
                false,
            ) => {
                book_job(to, "the sources' notebooks", move |b, at| {
                    for (source, _) in &before {
                        b.set_notebook(source, notebook, at);
                    }
                });
            }
            (Choice::Notebooks { before, .. }, true) => {
                book_job(to, "the sources' notebooks back", move |b, at| {
                    for (source, notebook) in &before {
                        b.set_notebook(source, *notebook, at);
                    }
                });
            }
        }
    }

    /// What `choice`'s toast says, done (`done`) or taken back.
    fn secretary_said(&self, choice: &Choice, done: bool) -> String {
        let fr = self.lang == Lang::French;
        match (choice, done, fr) {
            (Choice::Revoked(g), true, true) => {
                format!(
                    "{} : elle ne le lit plus et oublie ce qu’elle en savait",
                    g.title
                )
            }
            (Choice::Revoked(g), true, false) => {
                format!(
                    "{}: she no longer reads it and forgets what she knew of it",
                    g.title
                )
            }
            (Choice::Revoked(g), false, true) => format!("{} : elle le lit à nouveau", g.title),
            (Choice::Revoked(g), false, false) => format!("{}: she reads it again", g.title),
            (Choice::Notebooks { to, title, .. }, true, _) => {
                format!("{title}{} {}", if fr { " :" } else { ":" }, to.said(fr))
            }
            (Choice::Notebooks { before, title, .. }, false, _) => {
                let was = before.first().map_or(Notebook::Pro, |(_, n)| *n);
                format!("{title}{} {}", if fr { " :" } else { ":" }, was.said(fr))
            }
        }
    }

    /// `choice` undone (`redo`: done again), without its sheet: it was
    /// consented to.
    pub(super) fn secretary_undo(&mut self, choice: Choice, redo: bool) -> iced::Task<Message> {
        self.secretary_apply(&choice, !redo);
        let said = self.secretary_said(&choice, redo);
        if redo {
            self.undo.push(Undo::Secretary(choice));
        } else {
            self.redo.push(Undo::Secretary(choice));
        }
        self.go(Page::Section(SectionId::Secretary));
        let text = match (self.lang, redo) {
            (Lang::French, false) => format!("Annulé : {said}."),
            (Lang::English, false) => format!("Undone: {said}."),
            (Lang::French, true) => format!("Rétabli : {said}."),
            (Lang::English, true) => format!("Redone: {said}."),
        };
        self.toast(text, ToastAction::None, UNDONE)
    }

    // --- What changes it -------------------------------------------------------------

    pub(super) fn secretary_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Notifiers(found) => {
                self.secretary.notifiers = found;
                iced::Task::none()
            }
            Msg::Google(found) => {
                self.secretary.google = found;
                if let Some(word) = &self.secretary.choosing
                    && !self
                        .secretary
                        .google
                        .iter()
                        .any(|s| s.word() == *word && s.granted.is_some())
                {
                    self.secretary.choosing = None;
                }
                iced::Task::none()
            }
            Msg::NotifierSwitch(i) => {
                let Some(n) = self.secretary.notifiers.get(i).cloned() else {
                    return iced::Task::none();
                };
                if !self.secretary_assistant() {
                    return iced::Task::none();
                }
                match n.granted {
                    None => {
                        tracing::debug!(app = n.name, "the notifications' consent sheet opens");
                        self.secretary.notify = Some(Notify {
                            slack: n.is_slack(),
                            notebook: notifiers::proposed(&n),
                            name: n.name,
                            on_allow: true,
                        });
                        iced::Task::none()
                    }
                    Some(notebook) => self.secretary_chose(Choice::Revoked(Grant {
                        source: SourceKey::notifications(&n.name, None),
                        identity: n.name.trim().to_owned(),
                        notebook,
                        left_out: Vec::new(),
                        title: n.name.clone(),
                    })),
                }
            }
            Msg::NotifierNotebook(i, notebook) => {
                let Some((name, now)) = self
                    .secretary
                    .notifiers
                    .get(i)
                    .and_then(|n| n.granted.map(|g| (n.name.clone(), g)))
                else {
                    return iced::Task::none();
                };
                if now == notebook {
                    return iced::Task::none();
                }
                self.secretary_chose(Choice::Notebooks {
                    to: notebook,
                    before: vec![(SourceKey::notifications(&name, None), now)],
                    title: name,
                })
            }
            Msg::GoogleSwitch(i) => {
                let Some(source) = self.secretary.google.get(i).cloned() else {
                    return iced::Task::none();
                };
                if !self.secretary_assistant() {
                    return iced::Task::none();
                }
                match source.granted {
                    None => {
                        tracing::debug!(
                            source = source.word(),
                            "a Google source's consent sheet opens"
                        );
                        let notebook = self
                            .secretary
                            .leaning
                            .get(&source.account)
                            .copied()
                            .unwrap_or(source.suggested);
                        self.secretary.google_sheet = Some(GoogleSheet {
                            notebook,
                            source,
                            on_allow: true,
                            left_out: std::collections::BTreeSet::new(),
                        });
                        iced::Task::none()
                    }
                    Some(notebook) => self.secretary_chose(Choice::Revoked(Grant {
                        title: source.title(self.lang),
                        source: source.key(),
                        identity: source.identity.clone(),
                        notebook,
                        left_out: source.left_out.clone(),
                    })),
                }
            }
            Msg::AccountNotebook(account, notebook) => {
                let google = &self.secretary.google;
                let before: Vec<(SourceKey, Notebook)> = google
                    .iter()
                    .filter(|s| s.account == account)
                    .filter_map(|s| s.granted.map(|g| (s.key(), g)))
                    .filter(|(_, g)| *g != notebook)
                    .collect();
                let read = google
                    .iter()
                    .any(|s| s.account == account && s.granted.is_some());
                let Some(identity) = google
                    .iter()
                    .find(|s| s.account == account)
                    .map(|s| s.identity.clone())
                else {
                    return iced::Task::none();
                };
                if !read {
                    // Nothing read of it yet: what its sheets propose.
                    tracing::debug!(notebook = notebook.word(), "an account's notebook leant");
                    self.secretary.leaning.insert(account, notebook);
                    return iced::Task::none();
                }
                if before.is_empty() {
                    return iced::Task::none();
                }
                self.secretary_chose(Choice::Notebooks {
                    to: notebook,
                    before,
                    title: identity,
                })
            }
            Msg::Notebook(notebook) => {
                if let Some(sheet) = &mut self.secretary.google_sheet {
                    sheet.notebook = notebook;
                } else if let Some(notify) = &mut self.secretary.notify {
                    notify.notebook = notebook;
                }
                iced::Task::none()
            }
            Msg::Allowed(yes) => {
                let Some(notify) = self.secretary.notify.take() else {
                    return iced::Task::none();
                };
                if !yes {
                    tracing::debug!(app = notify.name, "not read");
                    return iced::Task::none();
                }
                let (name, notebook) = (notify.name.clone(), notify.notebook);
                book_job(
                    self.secretary.to.clone(),
                    "an application's notifications read",
                    move |b, at| {
                        b.allow(
                            SourceKey::notifications(&name, None),
                            name.trim(),
                            notebook,
                            at,
                        );
                    },
                );
                let text = match self.lang {
                    Lang::French => format!(
                        "{} : elle lit ses notifications ({})",
                        notify.name,
                        notify.notebook.said(true)
                    ),
                    Lang::English => format!(
                        "{}: she reads its notifications ({})",
                        notify.name,
                        notify.notebook.said(false)
                    ),
                };
                self.toast(text, ToastAction::None, TOAST)
            }
            Msg::GoogleAllowed(yes) => {
                let Some(sheet) = self.secretary.google_sheet.take() else {
                    return iced::Task::none();
                };
                if !yes {
                    tracing::debug!(source = sheet.source.word(), "not read");
                    return iced::Task::none();
                }
                let title = sheet.source.title(self.lang);
                let text = match self.lang {
                    Lang::French => {
                        format!("{title} : elle le lit ({})", sheet.notebook.said(true))
                    }
                    Lang::English => {
                        format!("{title}: she reads it ({})", sheet.notebook.said(false))
                    }
                };
                let (key, identity, notebook) = (
                    sheet.source.key(),
                    sheet.source.identity.clone(),
                    sheet.notebook,
                );
                let left_out: Vec<String> = sheet.left_out.into_iter().collect();
                book_job(
                    self.secretary.to.clone(),
                    "a Google source read",
                    move |b, at| b.allow_leaving_out(key, &identity, notebook, left_out, at),
                );
                self.toast(text, ToastAction::None, TOAST)
            }
            Msg::SheetCalendar(n) => {
                if let Some(sheet) = &mut self.secretary.google_sheet
                    && let Some(id) = sheet.source.calendars.get(n).map(|c| c.id.clone())
                    && !sheet.left_out.remove(&id)
                {
                    sheet.left_out.insert(id);
                }
                iced::Task::none()
            }
            Msg::Choose(i) => {
                if let Some(source) = self.secretary.google.get(i)
                    && source.granted.is_some()
                    && !source.calendars.is_empty()
                {
                    tracing::debug!(source = source.word(), "the calendars' list opens");
                    self.secretary.choosing = Some(source.word());
                }
                iced::Task::none()
            }
            Msg::Chosen => {
                self.secretary.choosing = None;
                iced::Task::none()
            }
            Msg::RowCalendar(i, n) => {
                let Some(source) = self.secretary.google.get(i).filter(|s| s.granted.is_some())
                else {
                    return iced::Task::none();
                };
                let Some(id) = source.calendars.get(n).map(|c| c.id.clone()) else {
                    return iced::Task::none();
                };
                let mut left_out = source.left_out.clone();
                if let Some(at) = left_out.iter().position(|l| *l == id) {
                    left_out.remove(at);
                } else {
                    left_out.push(id);
                }
                let key = source.key();
                book_job(
                    self.secretary.to.clone(),
                    "the calendars left out",
                    move |b, at| b.leave_out(&key, left_out, at),
                );
                iced::Task::none()
            }
            Msg::Status(heard) => {
                self.secretary.heard = heard;
                iced::Task::none()
            }
            Msg::KeyTyped(typed) => {
                // The text before goes, overwritten (Secret's drop).
                self.secretary.key.field = typed;
                iced::Task::none()
            }
            Msg::KeySave => {
                let typed = std::mem::take(&mut self.secretary.key.field);
                if typed.expose().trim().is_empty() || self.secretary.key.busy {
                    return iced::Task::none();
                }
                tracing::info!("her key given in the Settings");
                self.secretary.key.busy = true;
                self.secretary.key.said = None;
                self.secretary_key_job(Some(Some(typed)));
                iced::Task::none()
            }
            Msg::KeyReplace(on) => {
                self.secretary.key.replacing = on;
                self.secretary.key.field = Secret::default();
                self.secretary.key.said = None;
                if on {
                    iced::widget::operation::focus(KEY_FIELD)
                } else {
                    iced::Task::none()
                }
            }
            Msg::KeyErase => {
                if self.secretary.key.busy {
                    return iced::Task::none();
                }
                tracing::info!("her key erased in the Settings");
                self.secretary.key.busy = true;
                self.secretary.key.said = None;
                self.secretary_key_job(Some(None));
                iced::Task::none()
            }
            Msg::Keyed(outcome) => {
                self.secretary.key.busy = false;
                if matches!(outcome, Ok(KeyOutcome::Kept | KeyOutcome::Erased)) {
                    self.secretary.key.replacing = false;
                }
                let toast = match &outcome {
                    Ok(o @ (KeyOutcome::Kept | KeyOutcome::Erased)) => {
                        Some(o.words("anthropic", self.lang == Lang::French))
                    }
                    _ => None,
                };
                self.secretary.key.said = Some(outcome);
                match toast {
                    Some(text) => self.toast(text, ToastAction::None, TOAST),
                    None => iced::Task::none(),
                }
            }
            Msg::Panel(panel) => {
                self.secretary.panel = panel;
                iced::Task::none()
            }
            Msg::AddAccount => {
                let Some(panel) = self.secretary.panel.clone() else {
                    return iced::Task::none();
                };
                tracing::info!("Online Accounts' panel opens");
                let to = self.secretary.to.clone();
                let started = std::thread::Builder::new()
                    .name("online accounts".to_owned())
                    .spawn(move || {
                        let ran = std::process::Command::new(&panel)
                            .arg("online-accounts")
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                        if let Err(err) = ran {
                            tracing::warn!(%err, "Online Accounts' panel does not run");
                        }
                        // Closed: an account added there shows as rows.
                        if let Some(to) = to {
                            let _ = to.unbounded_send(Message::Secretary(Msg::Google(
                                google_sources::read(),
                            )));
                        }
                    });
                if let Err(err) = started {
                    tracing::warn!(%err, "Online Accounts' panel does not run");
                }
                iced::Task::none()
            }
            Msg::Quiet(hours) => {
                self.secretary.quiet = Some(hours);
                iced::Task::none()
            }
            Msg::NameTyped(typed) => {
                self.secretary.name = Some(typed.chars().take(40).collect());
                iced::Task::none()
            }
            Msg::NameCommit => {
                self.secretary_name_commit();
                iced::Task::none()
            }
        }
    }

    /// Her sheets take the keyboard: `Esc` cancels, `Enter` acts, `Tab`
    /// and the arrows go from one button to the other, up and down change
    /// the notebook.
    pub(super) fn secretary_key_first(&mut self, key: &Key) -> Option<iced::Task<Message>> {
        // Her key's field being typed in: `Esc` empties it (#752).
        if !self.secretary.key.field.expose().is_empty()
            && key.as_ref() == Key::Named(Named::Escape)
        {
            self.secretary.key.field = Secret::default();
            self.secretary.key.replacing = false;
            return Some(iced::widget::operation::focus("nothing"));
        }
        if self.secretary.choosing.is_some() {
            if matches!(
                key.as_ref(),
                Key::Named(Named::Escape | Named::Enter | Named::Space)
            ) {
                return Some(self.secretary_update(Msg::Chosen));
            }
            return Some(iced::Task::none());
        }
        let flip = |n: Notebook| match n {
            Notebook::Pro => Notebook::Perso,
            Notebook::Perso => Notebook::Pro,
        };
        if let Some(sheet) = self.secretary.google_sheet.as_mut() {
            match key.as_ref() {
                Key::Named(Named::Escape) => {
                    return Some(self.secretary_update(Msg::GoogleAllowed(false)));
                }
                Key::Named(Named::Enter | Named::Space) => {
                    let yes = sheet.on_allow;
                    return Some(self.secretary_update(Msg::GoogleAllowed(yes)));
                }
                Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                    sheet.on_allow = !sheet.on_allow;
                }
                Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                    sheet.notebook = flip(sheet.notebook);
                }
                _ => {}
            }
            return Some(iced::Task::none());
        }
        let notify = self.secretary.notify.as_mut()?;
        match key.as_ref() {
            Key::Named(Named::Escape) => return Some(self.secretary_update(Msg::Allowed(false))),
            Key::Named(Named::Enter | Named::Space) => {
                let yes = notify.on_allow;
                return Some(self.secretary_update(Msg::Allowed(yes)));
            }
            Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                notify.on_allow = !notify.on_allow;
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                notify.notebook = flip(notify.notebook);
            }
            _ => {}
        }
        Some(iced::Task::none())
    }

    pub(super) fn secretary_key(&mut self, stop: Stop, named: Named) -> iced::Task<Message> {
        match (stop, named) {
            (Stop::Notifier(i), Named::Space | Named::Enter) => {
                self.secretary_update(Msg::NotifierSwitch(i))
            }
            (Stop::Google(i), Named::Space | Named::Enter) => {
                self.secretary_update(Msg::GoogleSwitch(i))
            }
            (
                Stop::Account(first),
                Named::Space | Named::Enter | Named::ArrowLeft | Named::ArrowRight,
            ) => {
                let Some(account) = self.secretary.google.get(first).map(|s| s.account.clone())
                else {
                    return iced::Task::none();
                };
                let next = match (named, self.secretary_account_notebook(first)) {
                    (Named::ArrowLeft, _) => Notebook::Pro,
                    (Named::ArrowRight, _) => Notebook::Perso,
                    (_, Some(Notebook::Pro)) => Notebook::Perso,
                    _ => Notebook::Pro,
                };
                self.secretary_update(Msg::AccountNotebook(account, next))
            }
            _ => iced::Task::none(),
        }
    }

    /// Opened, the page reads again what she reads and her status (one
    /// may have changed meanwhile, in her drawer).
    pub(super) fn secretary_visit(&mut self) {
        let shows = self.secretary_shows();
        if shows && !self.secretary.seen {
            self.secretary_read();
        }
        self.secretary.seen = shows;
    }

    /// Her switch turned: on, what she reads is read again for her rows;
    /// off, her sheets close.
    pub(super) fn secretary_switched(&mut self, slot: Slot, value: &Value) {
        match (slot.setting, value) {
            (model::MODE, Value::Bool(true)) => self.secretary_read(),
            (model::MODE, Value::Bool(false)) => {
                self.secretary.notify = None;
                self.secretary.google_sheet = None;
                self.secretary.choosing = None;
            }
            _ => {}
        }
    }

    // --- What it draws ---------------------------------------------------------------

    /// A group: its label, what is on its right, its card.
    fn secretary_group<'a>(
        &self,
        label_words: &'static str,
        right: El<'a>,
        body: El<'a>,
    ) -> El<'a> {
        column![
            row![label(label_words), Space::new().width(Length::Fill), right]
                .align_y(Alignment::Center)
                .height(28),
            card_box(body),
        ]
        .spacing(8)
        .into()
    }

    /// A row's box: its padding, its light when a search opened it, `id`.
    fn secretary_box<'a>(&self, body: El<'a>, id: &'static str, sub: bool) -> El<'a> {
        let lit = self.lit(id);
        self.secretary_framed(body, format!("row:{id}"), sub, lit)
    }

    fn secretary_framed<'a>(&self, body: El<'a>, id: String, sub: bool, lit: f32) -> El<'a> {
        container(body)
            .padding(Padding {
                top: if sub { 8.0 } else { 11.0 },
                right: 14.0,
                bottom: if sub { 8.0 } else { 11.0 },
                left: if sub { 56.0 } else { 16.0 },
            })
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(id)
            .into()
    }

    /// A row's words: its name, and its line when it has one.
    fn secretary_words<'a>(&self, name: String, line: String, size: f32) -> El<'a> {
        let mut words = column![t(name, size, TEXT.get()).font(SEMIBOLD)].spacing(3);
        if !line.is_empty() {
            words = words.push(t(line, 12.5, DIM.get()).line_height(1.4));
        }
        words.width(Length::Fill).into()
    }

    pub(super) fn secretary_page<'a>(&'a self, page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let controls = self.controls();
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        let mut page = page;
        // LA SECRÉTAIRE
        let mut her = column![model_row(model::MODE)];
        let assistant = self.secretary_assistant();
        if assistant {
            her = her.push(divided(true, self.secretary_name_row()));
            if self
                .secretary
                .heard
                .as_ref()
                .is_some_and(|h| h.key.is_some())
            {
                her = her.push(divided(true, self.secretary_key_row()));
            }
            her = her.push(divided(true, self.secretary_knows_row()));
        }
        page = page.push(
            self.secretary_group(
                model::HER.label.get(lang),
                t(
                    self.say(w(
                        "Sa mémoire reste sur cet ordinateur",
                        "Her memory stays on this computer",
                    )),
                    12.0,
                    DIM.get(),
                )
                .into(),
                her.into(),
            ),
        );
        if !assistant {
            return container(page).id("page:secretary".to_owned()).into();
        }
        // CE QU'ELLE LIT
        let add: El<'_> = if self.secretary.panel.is_some() {
            container(
                button(
                    t(
                        self.say(w(
                            "+ Ajouter un compte Google ›",
                            "+ Add a Google account ›",
                        )),
                        12.5,
                        MINT.get(),
                    )
                    .font(BOLD),
                )
                .padding([2, 6])
                .on_press(Message::Secretary(Msg::AddAccount))
                .style(|_, status| button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                    text_color: TEXT.get(),
                    border: border(6.0, 0.0, Color::TRANSPARENT),
                    ..button::Style::default()
                }),
            )
            .id("secretary:add-google".to_owned())
            .into()
        } else {
            Space::new().into()
        };
        page = page.push(self.secretary_group(
            model::READS.label.get(lang),
            add,
            self.secretary_reads(),
        ));
        // LE TRI SUR CET ORDINATEUR
        page = page.push(
            self.secretary_group(
                model::TRIAGE.label.get(lang),
                Space::new().into(),
                column![
                    self.secretary_queue_row(),
                    divided(true, model_row(model::BATTERY)),
                    divided(true, self.secretary_model_row()),
                ]
                .into(),
            ),
        );
        container(page).id("page:secretary".to_owned()).into()
    }

    /// CE QU'ELLE LIT's card: each Google account and its kinds, then the
    /// applications.
    fn secretary_reads(&self) -> El<'_> {
        let mut body = column![];
        let mut first = true;
        let google = &self.secretary.google;
        let mut accounts_box = column![];
        if google.is_empty() {
            accounts_box = accounts_box.push(self.secretary_note(
                self.say(w(
                    "Aucun compte Google n’est connecté à cet ordinateur : son agenda et ses mails apparaîtront ici dès que tu en ajouteras un.",
                    "No Google account is connected to this computer: its calendar and mail will show here as soon as you add one.",
                )),
                "secretary:no-google",
            ));
            first = false;
        }
        for (head, kinds) in accounts(google) {
            accounts_box = accounts_box.push(divided(!first, self.secretary_account_row(head)));
            first = false;
            for i in kinds {
                if let Some(source) = google.get(i) {
                    accounts_box = accounts_box.push(self.secretary_kind_row(i, source));
                }
            }
        }
        body = body.push(container(accounts_box).id(format!("row:{}", model::GOOGLE)));
        let mut apps = column![];
        if self.secretary.notifiers.is_empty() {
            apps = apps.push(divided(
                true,
                self.secretary_note(
                    self.say(w(
                        "Aucune application ne t’a encore envoyé de notification : Slack, par exemple, apparaîtra ici dès qu’il en enverra une.",
                        "No application has sent you a notification yet: Slack, for one, will show here as soon as it sends one.",
                    )),
                    "secretary:no-notifier",
                ),
            ));
        }
        for (i, notifier) in self.secretary.notifiers.iter().enumerate() {
            apps = apps.push(divided(true, self.secretary_notifier_row(i, notifier)));
        }
        body = body.push(container(apps).id(format!("row:{}", model::NOTIFIERS)));
        body.into()
    }

    fn secretary_note<'a>(&self, said: &'static str, id: &'static str) -> El<'a> {
        container(t(said, 12.5, DIM.get()).line_height(1.4))
            .padding([12, 16])
            .id(id.to_owned())
            .into()
    }

    /// An account's line: its mark, its address, Travail | Privé.
    fn secretary_account_row(&self, first: usize) -> El<'_> {
        let Some(source) = self.secretary.google.get(first) else {
            return Space::new().into();
        };
        let line = if source.attention {
            self.say(w(
                "Google · demande de te reconnecter",
                "Google · asks you to sign in again",
            ))
        } else {
            self.say(w(
                "Google · déjà connecté à cet ordinateur",
                "Google · already connected to this computer",
            ))
        };
        let focused = self.secretary_focused(Stop::Account(first));
        let seg = self.secretary_segmented(
            self.secretary_account_notebook(first),
            focused,
            &format!("notebook:{}", source.account),
            |n| Message::Secretary(Msg::AccountNotebook(source.account.clone(), n)),
        );
        self.secretary_framed(
            row![
                Self::secretary_google_tile(28.0),
                self.secretary_words(source.identity.clone(), line.to_owned(), 14.0),
                seg,
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            format!("row:secretary.account.{}", source.account),
            false,
            0.0,
        )
    }

    /// A kind of an account under its line: its name, what she reads of
    /// it, « Choisir » for an Agenda's calendars, its switch.
    fn secretary_kind_row<'a>(&'a self, i: usize, source: &'a GoogleSource) -> El<'a> {
        let lang = self.lang;
        let read = source.granted.is_some();
        let line = match (source.kind, read) {
            (_, false) => String::new(),
            (SourceKind::Calendar, true) => {
                let names: Vec<(String, bool)> = source
                    .calendars
                    .iter()
                    .map(|c| (c.name.clone(), !source.left_out.contains(&c.id)))
                    .collect();
                model::calendars_said(&names, lang).unwrap_or_else(|| {
                    self.say(w(
                        "Tous tes agendas, masqués et partagés compris",
                        "All your calendars, hidden and shared ones included",
                    ))
                    .to_owned()
                })
            }
            (SourceKind::Mail, true) => self
                .say(w(
                    "Les 30 derniers jours, en lecture seule : rien n’est marqué lu",
                    "The last 30 days, read-only: nothing is marked read",
                ))
                .to_owned(),
            (_, true) => self
                .say(w(
                    "Le nom et la date, rien d’autre",
                    "The name and the date, nothing else",
                ))
                .to_owned(),
        };
        let mut line_row =
            row![self.secretary_words(kind_title(source.kind, lang).to_owned(), line, 13.5)]
                .spacing(12)
                .align_y(Alignment::Center);
        if read && source.kind == SourceKind::Calendar && !source.calendars.is_empty() {
            line_row = line_row.push(
                container(self.secretary_link(
                    self.say(w("Choisir ›", "Choose ›")),
                    Message::Secretary(Msg::Choose(i)),
                ))
                .id(format!("choose:{}", source.word())),
            );
        }
        let focused = self.secretary_focused(Stop::Google(i));
        line_row = line_row.push(
            container(self.secretary_switch(
                read,
                focused,
                Message::Secretary(Msg::GoogleSwitch(i)),
            ))
            .id(format!("switch:{}", source.word())),
        );
        self.secretary_framed(
            line_row.into(),
            format!("row:secretary.google.{}", source.word()),
            true,
            0.0,
        )
    }

    /// An application's row: its mark, its name, Travail | Privé once she
    /// reads it, its switch.
    fn secretary_notifier_row<'a>(&'a self, i: usize, notifier: &'a Notifier) -> El<'a> {
        let line = if notifier.is_slack() {
            self.say(w(
                "Ses notifications : qui écrit, où, et le début du message. Rien n’est marqué lu.",
                "Its notifications: who writes, where, and the start of the message. Nothing is marked read.",
            ))
        } else {
            self.say(w(
                "Ses notifications : leur titre et leur texte. Rien n’est marqué lu.",
                "Its notifications: their title and their text. Nothing is marked read.",
            ))
        };
        let mut line_row = row![
            Self::secretary_letter(&notifier.name, 28.0),
            self.secretary_words(notifier.name.clone(), line.to_owned(), 14.0),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if notifier.granted.is_some() {
            line_row = line_row.push(self.secretary_segmented(
                notifier.granted,
                false,
                &format!("notebook:app:{}", notifier.key),
                move |n| Message::Secretary(Msg::NotifierNotebook(i, n)),
            ));
        }
        let focused = self.secretary_focused(Stop::Notifier(i));
        line_row = line_row.push(
            container(self.secretary_switch(
                notifier.granted.is_some(),
                focused,
                Message::Secretary(Msg::NotifierSwitch(i)),
            ))
            .id(format!("switch:app:{}", notifier.key)),
        );
        self.secretary_framed(
            line_row.into(),
            format!("row:secretary.notifier.{}", notifier.key),
            false,
            0.0,
        )
    }

    /// "Ce qu'elle sait": where to see it (her drawer has no way in from
    /// here yet).
    fn secretary_knows_row(&self) -> El<'_> {
        self.secretary_box(
            self.secretary_words(
                self.say(w("Ce qu’elle sait", "What she knows")).to_owned(),
                self.say(w(
                    "De toi et de ton travail, chiffré sur cet ordinateur. Tu le vois et le corriges dans son tiroir, onglet « Ce qu’elle sait ».",
                    "Of you and your work, encrypted on this computer. You see and correct it in her drawer, under “What she knows”.",
                ))
                .to_owned(),
                14.0,
            ),
            model::KNOWS,
            false,
        )
    }

    /// "Le tri de tes mails": what it is for, and where it is.
    fn secretary_queue_row(&self) -> El<'_> {
        let lang = self.lang;
        let what = self.say(w(
            "Un petit modèle lit chaque mail ici, avant Claude, pour masquer les mots de passe et repérer ce qui attend une réponse.",
            "A small model reads each mail here, before Claude, to mask passwords and spot what awaits an answer.",
        ));
        let state = match &self.secretary.heard {
            Some(Heard {
                triage: Some(triage),
                ..
            }) => model::triage_said(&triage.state, triage.pending, lang),
            Some(_) => self
                .say(w(
                    "Rien n’est trié sur cet ordinateur.",
                    "Nothing is sorted on this computer.",
                ))
                .to_owned(),
            None => self
                .say(w(
                    "Elle ne répond pas pour l’instant.",
                    "She does not answer for now.",
                ))
                .to_owned(),
        };
        let where_ = self.say(w(
            "Sa file se voit dans son tiroir, sous son statut.",
            "Her queue shows in her drawer, under her status.",
        ));
        self.secretary_box(
            self.secretary_words(
                self.say(w("Le tri de tes mails", "Sorting your mail"))
                    .to_owned(),
                format!("{what} {state} {where_}"),
                14.0,
            ),
            model::QUEUE,
            false,
        )
    }

    /// "Le modèle": who runs the triage, where, and whether it answers.
    fn secretary_model_row(&self) -> El<'_> {
        let line = match &self.secretary.heard {
            Some(Heard {
                triage: Some(triage),
                kind,
                ..
            }) => model::model_said(&triage.model, kind.as_deref(), &triage.state, self.lang),
            _ => self
                .say(w(
                    "Pas connu tant qu’elle ne répond pas",
                    "Unknown while she does not answer",
                ))
                .to_owned(),
        };
        self.secretary_box(
            self.secretary_words(self.say(w("Le modèle", "The model")).to_owned(), line, 14.0),
            model::MODEL,
            false,
        )
    }

    /// Her name's row: a field, her name or « Ta secrétaire » greyed.
    fn secretary_name_row<'a>(&'a self) -> El<'a> {
        let lang = self.lang;
        let Some(setting) = crate::model::setting(model::NAME) else {
            return Space::new().into();
        };
        let value = self
            .secretary
            .name
            .clone()
            .unwrap_or_else(|| self.secretary_name());
        let field = text_input(self.say(w("Ta secrétaire", "Your secretary")), &value)
            .id(NAME_FIELD)
            .on_input(|typed| Message::Secretary(Msg::NameTyped(typed)))
            .on_submit(Message::Secretary(Msg::NameCommit))
            .size(13.5)
            .width(200)
            .padding([7, 10])
            .style(field_style);
        self.secretary_box(
            row![
                self.secretary_words(
                    setting.name.get(lang).to_owned(),
                    setting
                        .line
                        .map(|l| l.get(lang))
                        .unwrap_or_default()
                        .to_owned(),
                    14.0,
                ),
                container(field).id("secretary:name-field".to_owned()),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            model::NAME,
            false,
        )
    }

    /// "Clé Anthropic" (#752): « ✓ Enregistrée » with « Remplacer » and
    /// « Effacer », never the key; or, missing (or being replaced), a
    /// masked field and « Enregistrer ». What became of the last one under
    /// it, said plainly.
    fn secretary_key_row<'a>(&'a self) -> El<'a> {
        let fr = self.lang == Lang::French;
        let key = &self.secretary.key;
        let Some((provider, missing)) = self.secretary.heard.as_ref().and_then(|h| h.key.clone())
        else {
            return Space::new().into();
        };
        let name = if provider == "anthropic" {
            "Anthropic".to_owned()
        } else {
            provider.clone()
        };
        let title = if fr {
            format!("Clé {name}")
        } else {
            format!("{name} key")
        };
        let field_shown = missing || key.replacing;
        let mut words = column![t(title, 14.0, TEXT.get()).font(SEMIBOLD)].spacing(3);
        let line = match (field_shown, key.busy) {
            (_, true) => self.say(w("Elle la vérifie…", "She is checking it…")),
            (true, false) => self.say(w(
                "Pour réfléchir avec Claude. Elle va dans le trousseau de ta session, avec tes mots de passe, jamais dans un fichier.",
                "To think with Claude. It goes to your session’s keyring, with your passwords, never to a file.",
            )),
            (false, false) => self.say(w(
                "Pour réfléchir avec Claude. Gardée dans le trousseau de ta session, avec tes mots de passe.",
                "To think with Claude. Kept in your session’s keyring, with your passwords.",
            )),
        };
        words = words.push(t(line, 12.5, DIM.get()).line_height(1.4));
        if let Some(said) = &key.said {
            let (text, colour) = match said {
                Ok(KeyOutcome::Kept | KeyOutcome::Erased) => (String::new(), DIM.get()),
                Ok(outcome) => (outcome.words(&provider, fr), rgb(0xF7A77A)),
                Err(_) => (
                    self.say(w(
                        "Elle ne répond pas : réessaie dans un instant.",
                        "She does not answer: try again in a moment.",
                    ))
                    .to_owned(),
                    rgb(0xF7A77A),
                ),
            };
            if !text.is_empty() {
                words = words.push(t(text, 12.5, colour));
            }
        }
        let actions: El<'a> = if field_shown {
            let placeholder = if fr {
                format!("Colle ta clé {name} (sk-ant-…)")
            } else {
                format!("Paste your {name} key (sk-ant-…)")
            };
            let mut input = text_input(&placeholder, key.field.expose())
                .id(KEY_FIELD)
                .secure(true)
                .size(13.0)
                .width(250)
                .padding([7, 10])
                .style(field_style);
            if !key.busy {
                input = input
                    .on_input(|typed| Message::Secretary(Msg::KeyTyped(Secret::new(typed))))
                    .on_submit(Message::Secretary(Msg::KeySave));
            }
            let ready = !key.busy && !key.field.expose().trim().is_empty();
            let mut row = row![
                container(input).id("secretary:key-field".to_owned()),
                container(self.agents_button(
                    self.say(w("Enregistrer", "Save")),
                    true,
                    false,
                    ready.then_some(Message::Secretary(Msg::KeySave)),
                ))
                .id("secretary:key-save".to_owned()),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if key.replacing && !missing {
                row = row.push(
                    container(self.agents_button(
                        self.say(w("Annuler", "Cancel")),
                        false,
                        false,
                        Some(Message::Secretary(Msg::KeyReplace(false))),
                    ))
                    .id("secretary:key-cancel".to_owned()),
                );
            }
            row.into()
        } else {
            row![
                t(self.say(w("✓ Enregistrée", "✓ Saved")), 13.0, rgb(0x7EE0A0)).font(BOLD),
                container(self.agents_button(
                    self.say(w("Remplacer", "Replace")),
                    false,
                    false,
                    (!key.busy).then_some(Message::Secretary(Msg::KeyReplace(true))),
                ))
                .id("secretary:key-replace".to_owned()),
                container(self.agents_button(
                    self.say(w("Effacer", "Erase")),
                    false,
                    false,
                    (!key.busy).then_some(Message::Secretary(Msg::KeyErase)),
                ))
                .id("secretary:key-erase".to_owned()),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        };
        self.secretary_box(
            row![words.width(Length::Fill), actions]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
            model::KEY,
            false,
        )
    }

    /// A link in the accent: « Choisir › ».
    fn secretary_link<'a>(&self, said: &'static str, press: Message) -> El<'a> {
        button(t(said, 13.0, MINT.get()).font(BOLD))
            .padding([2, 4])
            .on_press(press)
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered)
                    .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                text_color: TEXT.get(),
                border: border(5.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            })
            .into()
    }

    /// A switch, as the model's rows draw theirs.
    fn secretary_switch<'a>(&self, on: bool, focused: bool, press: Message) -> El<'a> {
        let track = if on {
            ACCENT.get()
        } else {
            crate::theme::Ink::Token(|t| t.switch_off).get()
        };
        let knob =
            container(Space::new().width(18).height(18)).style(boxed(Color::WHITE, 9.0, None));
        button(container(knob).padding(Padding::default().left(if on { 20.0 } else { 2.0 }).top(2)))
            .width(40)
            .height(22)
            .padding(0)
            .on_press(press)
            .style(move |_, _| button::Style {
                background: Some(Background::Color(track)),
                text_color: TEXT.get(),
                border: if focused {
                    ring(11.0)
                } else {
                    border(11.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            })
            .into()
    }

    /// Travail | Privé: `chosen` lit (none: neither, the kinds differ),
    /// each part `{id}:pro` and `{id}:perso`.
    fn secretary_segmented<'a>(
        &self,
        chosen: Option<Notebook>,
        focused: bool,
        id: &str,
        press: impl Fn(Notebook) -> Message,
    ) -> El<'a> {
        let part = |notebook: Notebook| {
            let lit = chosen == Some(notebook);
            let said = match notebook {
                Notebook::Pro => self.say(w("Travail", "Work")),
                Notebook::Perso => self.say(w("Privé", "Private")),
            };
            container(
                button(t(said, 12.5, if lit { TEXT.get() } else { DIM.get() }).font(SEMIBOLD))
                    .height(24)
                    .padding([3, 10])
                    .on_press(press(notebook))
                    .style(move |_, _| button::Style {
                        background: lit.then_some(Background::Color(crate::theme::ink(rgba(
                            255, 255, 255, 0.13,
                        )))),
                        text_color: TEXT.get(),
                        border: border(7.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    }),
            )
            .id(format!("{id}:{}", notebook.word()))
        };
        container(row![part(Notebook::Pro), part(Notebook::Perso)])
            .padding(2)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.05,
                )))),
                border: if focused {
                    ring(9.0)
                } else {
                    border(9.0, 0.0, Color::TRANSPARENT)
                },
                ..container::Style::default()
            })
            .into()
    }

    /// A Google account's mark: its letter on a blue tile (no brand
    /// drawing).
    fn secretary_google_tile<'a>(size: f32) -> El<'a> {
        container(text("G").size(size * 0.46).font(BOLD).color(Color::WHITE))
            .width(size)
            .height(size)
            .center(size)
            .style(boxed(rgb(0x4A6FD6), size * 0.28, None))
            .into()
    }

    /// An application's mark: its first letter on the neutral tile the
    /// notifications draw for an application without an icon.
    fn secretary_letter<'a>(name: &str, size: f32) -> El<'a> {
        let letter: String = name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        container(
            text(letter)
                .size(size * 0.46)
                .font(BOLD)
                .color(Color::WHITE),
        )
        .width(size)
        .height(size)
        .center(size)
        .style(boxed(rgb(0x5C6576), size * 0.28, None))
        .into()
    }

    /// A Google source's consent sheet (§10.1): the words of her card in
    /// the drawer, the notebook proposed.
    fn secretary_google_sheet<'a>(&self, sheet: &GoogleSheet) -> El<'a> {
        use hocus_secretary_link::sheets::Mark;
        let lang = self.lang;
        let words = sheet.source.sheet(lang, self.secretary_quiet());
        let mut body = column![
            row![
                Self::secretary_google_tile(32.0),
                column![
                    text(words.title.clone()).size(17).font(BOLD).color(TEXT),
                    t(words.from.clone(), 12.0, FAINT.get()),
                ]
                .spacing(2),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(12);
        for section in &words.sections {
            let (mark, colour) = match section.mark {
                Mark::Yes => ("✓", rgb(0x7EE0A0)),
                Mark::No => ("✕", rgb(0xF7A77A)),
                Mark::Plain => ("•", FAINT.get()),
            };
            let mut lines = column![].spacing(3);
            for line in &section.lines {
                lines = lines.push(
                    row![
                        container(t(mark, 13.0, colour).font(BOLD)).width(16),
                        t(line.clone(), 13.0, SOFT.get()).line_height(1.45),
                    ]
                    .spacing(2),
                );
            }
            body = body.push(
                column![
                    t(section.label.clone(), 11.0, FAINT.get()).font(BOLD),
                    lines
                ]
                .spacing(5),
            );
        }
        if !sheet.source.calendars.is_empty() {
            let fr = lang == Lang::French;
            let mut list = column![
                t(
                    hocus_secretary_link::sheets::calendars_label(fr),
                    11.0,
                    FAINT.get()
                )
                .font(BOLD)
            ]
            .spacing(4);
            for (n, calendar) in sheet.source.calendars.iter().enumerate() {
                let read = !sheet.left_out.contains(&calendar.id);
                list = list.push(self.secretary_calendar(
                    calendar,
                    read,
                    Message::Secretary(Msg::SheetCalendar(n)),
                    format!("sheet:calendar:{n}"),
                ));
            }
            body = body.push(list);
        }
        body = body.push(self.secretary_private(sheet.notebook, &words.private));
        self.secretary_sheet_box(
            body,
            Message::Secretary(Msg::GoogleAllowed(false)),
            Message::Secretary(Msg::GoogleAllowed(true)),
            sheet.on_allow,
            460,
        )
    }

    /// A sheet's buttons under its body, primary first, on the left (the
    /// audit's rule), over the window.
    fn secretary_sheet_box<'a>(
        &self,
        body: iced::widget::Column<'a, Message>,
        not_now: Message,
        allow: Message,
        on_allow: bool,
        width: u32,
    ) -> El<'a> {
        let buttons = row![
            container(self.agents_button(
                self.say(w("Autoriser", "Allow")),
                true,
                on_allow,
                Some(allow),
            ))
            .id("sheet:allow".to_owned()),
            container(self.agents_button(
                self.say(w("Pas maintenant", "Not now")),
                false,
                !on_allow,
                Some(not_now),
            ))
            .id("sheet:not-now".to_owned()),
        ]
        .spacing(8);
        Self::agents_over(
            container(column![body, buttons].spacing(20))
                .width(width)
                .padding(20)
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(rgb(0x1B2130))),
                    border: border(14.0, 1.0, RIM_12.get()),
                    shadow: shadow(),
                    ..container::Style::default()
                })
                .id("sheet".to_owned())
                .into(),
        )
    }

    /// « C'est privé » on a sheet (#752): its box, checked for the private
    /// notebook, and what it does under it.
    fn secretary_private<'a>(
        &self,
        notebook: Notebook,
        private: &hocus_secretary_link::sheets::Private,
    ) -> El<'a> {
        let on = notebook == Notebook::Perso;
        let other = if on { Notebook::Pro } else { Notebook::Perso };
        let check = Self::secretary_check(on, 18.0);
        let label = button(
            row![
                check,
                t(private.label.clone(), 13.5, TEXT.get()).font(SEMIBOLD)
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding([3, 4])
        .on_press(Message::Secretary(Msg::Notebook(other)))
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
            text_color: TEXT.get(),
            border: border(6.0, 0.0, Color::TRANSPARENT),
            ..button::Style::default()
        });
        let mut lines = column![].spacing(3).padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 31.0,
        });
        for line in &private.lines {
            lines = lines.push(
                t(line.clone(), 12.5, if on { SOFT.get() } else { DIM.get() }).line_height(1.4),
            );
        }
        column![container(label).id("sheet:private".to_owned()), lines]
            .spacing(4)
            .into()
    }

    /// A calendar of an Agenda, read (ticked) or left out: its name, who
    /// shares it, hidden, busy times only (#752).
    fn secretary_calendar<'a>(
        &self,
        calendar: &hocus_secretary_link::calendars::CalendarInfo,
        read: bool,
        press: Message,
        id: String,
    ) -> El<'a> {
        let note = calendar.note(self.lang == Lang::French);
        let mut words = row![
            t(
                calendar.name.clone(),
                13.0,
                if read { TEXT.get() } else { DIM.get() }
            )
            .font(SEMIBOLD)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if !note.is_empty() {
            words = words.push(t(note, 12.0, DIM.get()));
        }
        container(
            button(
                row![Self::secretary_check(read, 15.0), words.wrap()]
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .padding([3, 4])
            .on_press(press)
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered)
                    .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                text_color: TEXT.get(),
                border: border(6.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            }),
        )
        .id(id)
        .into()
    }

    /// A check box, ticked or not.
    fn secretary_check<'a>(on: bool, size: f32) -> El<'a> {
        let mark: El<'a> = if on {
            text("✓")
                .size(size * 0.72)
                .font(BOLD)
                .color(DEEP.get())
                .into()
        } else {
            Space::new().into()
        };
        container(mark)
            .width(size)
            .height(size)
            .center(size)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(if on {
                    ACCENT.get()
                } else {
                    crate::theme::ink(rgba(255, 255, 255, 0.04))
                })),
                border: border(
                    5.0,
                    if on { 0.0 } else { 1.5 },
                    crate::theme::ink(rgba(255, 255, 255, 0.35)),
                ),
                ..container::Style::default()
            })
            .into()
    }

    /// The notifications' consent sheet (§10.1): what she reads, what she
    /// never does there, what leaves the machine; the notebook.
    fn secretary_notify_sheet<'a>(&self, notify: &Notify) -> El<'a> {
        let lang = self.lang;
        let title = match lang {
            Lang::French => format!("Lire les notifications de {} ?", notify.name),
            Lang::English => format!("Read {}’s notifications?", notify.name),
        };
        let mut body = column![
            row![
                Self::secretary_letter(&notify.name, 32.0),
                text(title).size(17).font(BOLD).color(TEXT),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(14);
        for (heading, said) in notifiers::sheet(&notify.name, notify.slack, lang) {
            body = body.push(
                column![
                    t(heading.get(lang), 11.0, FAINT.get()).font(BOLD),
                    t(said, 13.0, SOFT.get()).line_height(1.45),
                ]
                .spacing(5),
            );
        }
        let private = hocus_secretary_link::sheets::private(
            self.lang == Lang::French,
            self.secretary_quiet(),
        );
        body = body.push(self.secretary_private(notify.notebook, &private));
        self.secretary_sheet_box(
            body,
            Message::Secretary(Msg::Allowed(false)),
            Message::Secretary(Msg::Allowed(true)),
            notify.on_allow,
            440,
        )
    }

    /// « Choisir »'s list: the Agenda's calendars, each read or left out
    /// at once.
    fn secretary_calendars_sheet<'a>(&self, word: &str) -> Option<El<'a>> {
        let (i, source) = self
            .secretary
            .google
            .iter()
            .enumerate()
            .find(|(_, s)| s.word() == word)?;
        let fr = self.lang == Lang::French;
        let mut body = column![
            row![
                Self::secretary_google_tile(32.0),
                column![
                    text(self.say(w("Les agendas qu’elle lit", "The calendars she reads")))
                        .size(17)
                        .font(BOLD)
                        .color(TEXT),
                    t(source.identity.clone(), 12.0, FAINT.get()),
                ]
                .spacing(2),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            t(
                hocus_secretary_link::sheets::calendars_label(fr),
                11.0,
                FAINT.get()
            )
            .font(BOLD),
        ]
        .spacing(10);
        for (n, calendar) in source.calendars.iter().enumerate() {
            let read = !source.left_out.contains(&calendar.id);
            body = body.push(self.secretary_calendar(
                calendar,
                read,
                Message::Secretary(Msg::RowCalendar(i, n)),
                format!("calendar:{}:{n}", source.word()),
            ));
        }
        let close = container(self.agents_button(
            self.say(w("Fermer", "Close")),
            true,
            true,
            Some(Message::Secretary(Msg::Chosen)),
        ))
        .id("sheet:close".to_owned());
        Some(Self::agents_over(
            container(column![body, close].spacing(20))
                .width(440)
                .padding(20)
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(rgb(0x1B2130))),
                    border: border(14.0, 1.0, RIM_12.get()),
                    shadow: shadow(),
                    ..container::Style::default()
                })
                .id("sheet".to_owned())
                .into(),
        ))
    }

    /// Her sheet over the window, if one is open.
    pub(super) fn secretary_sheet(&self) -> Option<El<'_>> {
        if !self.secretary_shows() {
            return None;
        }
        if let Some(sheet) = &self.secretary.google_sheet {
            return Some(self.secretary_google_sheet(sheet));
        }
        if let Some(notify) = &self.secretary.notify {
            return Some(self.secretary_notify_sheet(notify));
        }
        let word = self.secretary.choosing.as_deref()?;
        self.secretary_calendars_sheet(word)
    }

    /// What the page shows, for the log.
    pub(super) fn secretary_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let google = &self.secretary.google;
        let heard = self.secretary.heard.as_ref();
        serde_json::json!({
            "assistant": self.secretary_assistant(),
            "accounts": accounts(google).into_iter().filter_map(|(first, kinds)| {
                let head = google.get(first)?;
                Some(serde_json::json!({
                    "account": head.account,
                    "identity": head.identity,
                    "notebook": self.secretary_account_notebook(first).map(Notebook::word),
                    "kinds": kinds.into_iter().filter_map(|i| google.get(i)).map(|s| s.word()).collect::<Vec<_>>(),
                }))
            }).collect::<Vec<_>>(),
            "notifiers": self.secretary.notifiers.iter().map(|n| serde_json::json!({
                "key": n.key,
                "name": n.name,
                "granted": n.granted.map(Notebook::word),
            })).collect::<Vec<_>>(),
            "notify": self.secretary.notify.as_ref().map(|n| n.name.clone()),
            "notify_notebook": self.secretary.notify.as_ref().map(|n| n.notebook.word()),
            "notify_on": self.secretary.notify.as_ref().map(|n| if n.on_allow { "allow" } else { "not-now" }),
            "google": google.iter().map(|g| {
                let names: Vec<(String, bool)> = g.calendars.iter().map(|c| (c.name.clone(), !g.left_out.contains(&c.id))).collect();
                serde_json::json!({
                    "key": g.word(),
                    "title": g.title(lang),
                    "suggested": g.suggested.word(),
                    "granted": g.granted.map(Notebook::word),
                    "calendars": g.calendars.iter().map(|c| format!("{} {}", c.name, c.note(true)).trim().to_owned()).collect::<Vec<_>>(),
                    "calendars_said": if g.granted.is_some() { model::calendars_said(&names, lang) } else { None },
                    "left_out": g.left_out,
                })
            }).collect::<Vec<_>>(),
            "choosing": self.secretary.choosing,
            "add_account": self.secretary.panel.is_some(),
            "google_left_out": self.secretary.google_sheet.as_ref().map(|g| g.left_out.iter().cloned().collect::<Vec<_>>()),
            "google_sheet": self.secretary.google_sheet.as_ref().map(|g| g.source.word()),
            "google_notebook": self.secretary.google_sheet.as_ref().map(|g| g.notebook.word()),
            "name": self.secretary_name(),
            "naming": self.secretary.name.clone(),
            // Her key's row: never the key, nor its length.
            "key": heard.and_then(|h| h.key.as_ref()).map(|(provider, missing)| serde_json::json!({
                "provider": provider,
                "kept": !missing,
                "field": *missing || self.secretary.key.replacing,
                "typed": !self.secretary.key.field.expose().is_empty(),
                "busy": self.secretary.key.busy,
                "said": self.secretary.key.said.as_ref().map(|s| match s {
                    Ok(o) => o.words(provider, true),
                    Err(_) => "no answer".to_owned(),
                }),
            })),
            "answers": heard.is_some(),
            "triage": heard.and_then(|h| h.triage.as_ref()).map(|t| model::triage_said(&t.state, t.pending, lang)),
            "model": heard.and_then(|h| h.triage.as_ref().map(|t| model::model_said(&t.model, h.kind.as_deref(), &t.state, lang))),
        })
    }
}

/// A text field, as the page's fields are drawn.
fn field_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.06))),
        border: border(
            8.0,
            if focused { 1.5 } else { 1.0 },
            if focused { ACCENT.get() } else { RIM_12.get() },
        ),
        icon: DIM.get(),
        placeholder: FAINT.get(),
        value: TEXT.get(),
        selection: crate::theme::Ink::Accent(0.35).get(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(account: &str, kind: SourceKind) -> GoogleSource {
        GoogleSource {
            account: account.to_owned(),
            identity: format!("{account}@example.org"),
            kind,
            suggested: Notebook::Pro,
            attention: false,
            granted: None,
            calendars: Vec::new(),
            left_out: Vec::new(),
        }
    }

    #[test]
    fn the_kinds_are_grouped_under_their_account_in_order() {
        let google = [
            source("a", SourceKind::Calendar),
            source("a", SourceKind::Birthdays),
            source("a", SourceKind::Mail),
            source("b", SourceKind::Calendar),
            source("b", SourceKind::Mail),
        ];
        assert_eq!(accounts(&google), [(0, vec![0, 2, 1]), (3, vec![3, 4])]);
        assert!(accounts(&[]).is_empty());
    }
}
