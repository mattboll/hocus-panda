//! The "Captures et presse-papiers" page (6.17, variant A): where
//! screenshots and recordings go (a path button that opens the folder,
//! "Changer…" the desktop portal's folder chooser), their formats and the
//! shutter, then the clipboard's history: kept or not, for how long,
//! passwords left out (the applications as chips, "+ Ajouter" listing
//! those installed), and "Effacer…" asking in place.
//!
//! The history's count and whether a recording is under way come from the
//! compositor's line (`msg screens`), which also clears the history
//! (`clipboard-clear`). Turned off, the history is erased by the shell once
//! the toast is gone: "Annuler" within it brings everything back.

use crate::lang::French as _;
use hocus_words::settings::app_captures_page as words;
use std::time::{Duration, Instant};

use iced::{
    Alignment, Background, Color, Length, Padding, Shadow, Theme,
    keyboard::key::Named,
    widget::{Space, button, column, container, mouse_area, pin, row, stack},
};

use super::{
    App, Control, Message,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    captures::{self as model, Folder, Writable},
    lang::{Lang, pair, w},
    model::{Icon, SectionId, Setting, Value},
    screens::Said,
    store::Slot,
};

/// How long the header says "Effacé" after a clear.
const CLEARED: Duration = Duration::from_secs(4);

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// The history's items and pinned ones, as the compositor says them.
    count: Option<(u32, u32)>,
    /// A recording under way, as the compositor says.
    recording: bool,
    /// "Effacer…" turned into its confirmation.
    confirming: bool,
    /// Cleared then: the header says so a while.
    cleared: Option<(u64, Instant)>,
    serial: u64,
    /// "+ Ajouter"'s menu open, since then.
    adding: Option<Instant>,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A link to another page's row.
    Link(&'static str),
    /// The folder opened in the file manager.
    Open(Folder),
    /// "Changer…": the portal's folder chooser.
    Change(Folder),
    /// What it chose (none: cancelled, or no portal).
    Chosen(Folder, Option<std::path::PathBuf>),
    /// Application `i` (of the chips, after the built-in ones) taken out.
    Remove(usize),
    /// "+ Ajouter" opened or closed, a choice of it, closed.
    AddMenu,
    Add(usize),
    CloseAdd,
    /// "Effacer…", "Garder", "Effacer N éléments".
    Clear,
    Keep,
    Confirm,
    /// The header's "Effacé" goes.
    ClearedGone(u64),
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Link(&'static str),
    Open(Folder),
    Change(Folder),
    Remove(usize),
    Add,
    Clear,
    Keep,
    Confirm,
}

impl App {
    fn captures_slot(&self, id: &str) -> Option<Slot> {
        crate::model::setting(id).map(|s| Slot::of(s, self.source))
    }

    fn captures_bool(&self, id: &str) -> bool {
        self.captures_slot(id).and_then(|s| self.value(s)) != Some(Value::Bool(false))
    }

    fn captures_word(&self, id: &str) -> Option<&'static str> {
        match self.captures_slot(id).and_then(|s| self.value(s)) {
            Some(Value::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn captures_text(&self, id: &str) -> String {
        match self.captures_slot(id).and_then(|s| self.value(s)) {
            Some(Value::Text(text)) => text,
            _ => String::new(),
        }
    }

    /// The applications the file adds to the built-in password managers.
    fn ignored(&self) -> Vec<String> {
        match self
            .captures_slot(model::IGNORE)
            .and_then(|s| self.value(s))
        {
            Some(Value::List(list)) => list,
            _ => Vec::new(),
        }
    }

    fn history_on(&self) -> bool {
        self.captures_bool(model::HISTORY)
    }

    /// The items "Effacer…" and the history off erase: all but the pinned.
    fn erasable(&self) -> u32 {
        self.captures
            .count
            .map_or(0, |(items, pinned)| items.saturating_sub(pinned))
    }

    pub(super) fn captures_controls(&self) -> Vec<Control> {
        let mut out = vec![Control::Captures(Stop::Link(model::SHORTCUTS))];
        let slot = |id| self.captures_slot(id).map(Control::Setting);
        for folder in [Folder::Captures, Folder::Recordings] {
            out.push(Control::Captures(Stop::Open(folder)));
            out.push(Control::Captures(Stop::Change(folder)));
            let rest: &[&str] = match folder {
                Folder::Captures => &[model::CAPTURE_FORMAT, model::SOUND],
                Folder::Recordings => &[model::RECORD_FORMAT],
            };
            out.extend(rest.iter().filter_map(|id| slot(id)));
        }
        out.extend(
            [model::HISTORY, model::KEEP, model::SECRETS]
                .iter()
                .filter_map(|id| slot(id)),
        );
        for i in 0..self.ignored().len() {
            out.push(Control::Captures(Stop::Remove(i)));
        }
        out.push(Control::Captures(Stop::Add));
        if self.captures.confirming {
            out.push(Control::Captures(Stop::Keep));
            out.push(Control::Captures(Stop::Confirm));
        } else {
            out.push(Control::Captures(Stop::Clear));
        }
        out
    }

    fn captures_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Captures(stop.clone())))
    }

    // --- What changes it --------------------------------------------------------------

    pub(super) fn captures_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Link(target) => self.show_row(target),
            Msg::Open(folder) => {
                let path = self.folder_used(folder).0;
                tracing::info!(folder = %path.display(), "the folder is opened");
                super::open_folder(path, false);
                iced::Task::none()
            }
            Msg::Change(folder) => {
                if !self.writable() {
                    return iced::Task::none();
                }
                let title = folder.title(self.lang).to_owned();
                let from = self.folder_used(folder).0;
                iced::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::portal::open_folder(&title, &from)
                        })
                        .await
                        .ok()
                    },
                    move |chosen| {
                        let path = match chosen {
                            Some(crate::portal::Chosen::Path(path)) => Some(path),
                            _ => None,
                        };
                        Message::Captures(Msg::Chosen(folder, path))
                    },
                )
            }
            Msg::Chosen(folder, path) => {
                let Some(path) = path else {
                    tracing::info!("no folder chosen");
                    return iced::Task::none();
                };
                let Some(slot) = self.captures_slot(folder.setting()) else {
                    return iced::Task::none();
                };
                let said = folder.toast(&model::shown(&path), self.lang);
                tracing::info!(folder = %path.display(), "a folder chosen");
                self.change_saying(slot, Value::Text(model::written(&path)), Some(said))
            }
            Msg::Remove(i) => {
                let mut list = self.ignored();
                if i >= list.len() || !self.writable() {
                    return iced::Task::none();
                }
                let gone = list.remove(i);
                let said = match self.lang {
                    Lang::French => format!("Retirée des mots de passe : {}", app_name(&gone)),
                    Lang::English => format!("No longer left out: {}", app_name(&gone)),
                };
                match self.captures_slot(model::IGNORE) {
                    Some(slot) => self.change_saying(slot, Value::List(list), Some(said)),
                    None => iced::Task::none(),
                }
            }
            Msg::AddMenu => {
                self.captures.adding = match self.captures.adding {
                    Some(_) => None,
                    None if self.writable() && self.history_on() => Some(Instant::now()),
                    None => None,
                };
                iced::Task::none()
            }
            Msg::CloseAdd => {
                self.captures.adding = None;
                iced::Task::none()
            }
            Msg::Add(i) => {
                self.captures.adding = None;
                let Some((id, name)) = self.captures_addable().get(i).cloned() else {
                    return iced::Task::none();
                };
                let mut list = self.ignored();
                list.push(id);
                let said = match self.lang {
                    Lang::French => format!("Laissée de côté : {name}"),
                    Lang::English => format!("Left out: {name}"),
                };
                match self.captures_slot(model::IGNORE) {
                    Some(slot) => self.change_saying(slot, Value::List(list), Some(said)),
                    None => iced::Task::none(),
                }
            }
            Msg::Clear => {
                if self.history_on() {
                    self.captures.confirming = true;
                }
                iced::Task::none()
            }
            Msg::Keep => {
                self.captures.confirming = false;
                iced::Task::none()
            }
            Msg::Confirm => {
                self.captures.confirming = false;
                if !self.screens_ask("clipboard-clear".to_owned()) {
                    tracing::warn!("no compositor to clear the history");
                    return iced::Task::none();
                }
                tracing::info!(items = self.erasable(), "the history is cleared");
                self.captures.serial += 1;
                let serial = self.captures.serial;
                self.captures.cleared = Some((serial, Instant::now()));
                iced::Task::perform(tokio::time::sleep(CLEARED), move |()| {
                    Message::Captures(Msg::ClearedGone(serial))
                })
            }
            Msg::ClearedGone(serial) => {
                if self.captures.cleared.is_some_and(|(s, _)| s == serial) {
                    self.captures.cleared = None;
                }
                iced::Task::none()
            }
        }
    }

    /// `Esc`: the menu or the confirmation goes.
    pub(super) fn captures_escape(&mut self) -> bool {
        if self.captures.adding.take().is_some() {
            return true;
        }
        std::mem::take(&mut self.captures.confirming)
    }

    pub(super) fn captures_key(&mut self, stop: &Stop, named: Named) -> iced::Task<Message> {
        let msg = match (stop, named) {
            (Stop::Remove(i), Named::Delete | Named::Backspace) => Msg::Remove(*i),
            (_, Named::Space | Named::Enter) => match stop {
                Stop::Link(target) => Msg::Link(target),
                Stop::Open(f) => Msg::Open(*f),
                Stop::Change(f) => Msg::Change(*f),
                Stop::Remove(i) => Msg::Remove(*i),
                Stop::Add => Msg::AddMenu,
                Stop::Clear => Msg::Clear,
                Stop::Keep => Msg::Keep,
                Stop::Confirm => Msg::Confirm,
            },
            _ => return iced::Task::none(),
        };
        self.captures_update(msg)
    }

    /// What the compositor's line says of the history and the recording.
    pub(super) fn captures_said(&mut self, said: &Said) -> bool {
        match said {
            Said::Clipboard { items, pinned } => {
                self.captures.count = Some((*items, *pinned));
                true
            }
            Said::Recording(on) => {
                self.captures.recording = *on;
                true
            }
            _ => false,
        }
    }

    /// The toast of a switch of the page's, when it says more: the history
    /// off says what it erases.
    pub(super) fn captures_toggle_toast(&self, slot: Slot, on: bool) -> Option<String> {
        (slot.setting == model::HISTORY && !on).then(|| {
            let n = model::items_said(self.erasable(), self.lang);
            match self.lang {
                Lang::French => format!("Historique coupé : {n} effacés"),
                Lang::English => format!("History off: {n} erased"),
            }
        })
    }

    /// The header's word after a clear.
    pub(super) fn captures_cleared(&self) -> bool {
        self.captures.cleared.is_some()
    }

    // --- What it says -----------------------------------------------------------------

    /// The folder used now, and whether the one chosen cannot be written
    /// (gone or read-only): the default one is used meanwhile.
    fn folder_used(&self, folder: Folder) -> (std::path::PathBuf, Option<Writable>) {
        let default = folder.default_path(self.lang);
        match model::chosen(&self.captures_text(folder.setting())) {
            Some(path) => match model::writable(&path) {
                Writable::Yes => (path, None),
                state => (default, Some(state)),
            },
            None => (default, None),
        }
    }

    /// A model row's line when it is the page's: the formats' by their
    /// value, the history off, a recording under way; and whether it
    /// warns.
    pub(super) fn captures_line(&self, id: &str) -> Option<(String, bool)> {
        let lang = self.lang;
        let said = |fr: &'static str, en: &'static str| pair(fr, en).get(lang).to_owned();
        Some(match id {
            model::CAPTURE_FORMAT => match self.captures_word(id) {
                Some("jpeg") => (
                    said(
                        "Plus léger, pour les photos ; le presse-papiers reçoit toujours du PNG.",
                        "Lighter, for photos; the clipboard always gets PNG.",
                    ),
                    false,
                ),
                _ => (
                    said(
                        "Sans perte : net pour du texte et du code.",
                        "Lossless: sharp for text and code.",
                    ),
                    false,
                ),
            },
            model::RECORD_FORMAT if self.captures.recording => (
                said("Pour le prochain enregistrement", "For the next recording"),
                false,
            ),
            model::RECORD_FORMAT => match self.captures_word(id) {
                Some("webm") => (
                    said(
                        "WebM (VP9 et Opus) : libre, plus léger, moins lu par les téléphones.",
                        "WebM (VP9 and Opus): free, lighter, less played by phones.",
                    ),
                    false,
                ),
                _ => (
                    said(
                        "MP4 (H.264 et AAC) : se lit et s’envoie partout.",
                        "MP4 (H.264 and AAC): plays and sends anywhere.",
                    ),
                    false,
                ),
            },
            model::HISTORY if !self.history_on() => (
                said(
                    "Coupé : Super+V ne montre que les épinglés.",
                    "Off: Super+V shows the pinned items only.",
                ),
                false,
            ),
            model::SECRETS if !self.captures_bool(id) => {
                let keep = match (self.captures_word(model::KEEP), lang) {
                    (Some("session"), Lang::French) => "jusqu’à la fin de la session",
                    (Some("session"), Lang::English) => "until the session ends",
                    (Some("1d"), Lang::French) => "pendant 1 jour",
                    (Some("1d"), Lang::English) => "for 1 day",
                    (Some("7d"), Lang::French) => "pendant 7 jours",
                    (Some("7d"), Lang::English) => "for 7 days",
                    (_, Lang::French) => "pendant 30 jours",
                    (_, Lang::English) => "for 30 days",
                };
                let text = match lang {
                    Lang::French => format!(
                        "Coupé : un mot de passe copié sera gardé dans l’historique, chiffré, {keep}."
                    ),
                    Lang::English => format!(
                        "Off: a copied password will be kept in the history, encrypted, {keep}."
                    ),
                };
                (text, true)
            }
            _ => return None,
        })
    }

    /// A row of the page's drawn at 45 %: those that need the history, off.
    pub(super) fn captures_inert(&self, setting: &Setting) -> bool {
        setting.section == SectionId::Captures && setting.needs.is_some() && !self.history_on()
    }

    /// "Nouveau" beside the shutter.
    pub(super) fn captures_new(&self, setting: &Setting) -> bool {
        setting.id == model::SOUND
    }

    /// "Nouveau" (10.5/700, `#8FB3FF` on its 16 %, radius 5).
    pub(super) fn new_chip<'a>(&self) -> El<'a> {
        container(t(w(words::NEW).get(self.lang).to_owned(), 10.5, BLUE.get()).font(BOLD))
            .padding([1, 6])
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    143, 179, 255, 0.16,
                )))),
                border: border(5.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            })
            .into()
    }

    /// Under the passwords' row: the applications left out.
    pub(super) fn captures_row_extra(&self, setting: &Setting) -> Option<El<'_>> {
        (setting.id == model::SECRETS).then(|| self.chips())
    }

    /// The applications "+ Ajouter" lists: those installed not left out
    /// yet, by name.
    fn captures_addable(&self) -> Vec<(String, String)> {
        let ignored = self.ignored();
        crate::apps::list()
            .iter()
            .filter(|a| {
                !ignored.iter().any(|i| i.eq_ignore_ascii_case(&a.id))
                    && !model::BUILT_IN
                        .iter()
                        .any(|(id, _)| a.id.to_lowercase().contains(&id.to_lowercase()))
            })
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect()
    }

    // --- What it draws ----------------------------------------------------------------

    pub(super) fn captures_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        let lang = self.lang;
        page = page.push(
            row![
                icon(Icon::Keyboard, 15.0, FAINT.get()),
                t(
                    w(words::PRINT_SUPER_SHIFT_R_AND_SUPER).get(lang),
                    13.0,
                    FAINT.get()
                ),
                self.captures_link(model::SHORTCUTS, w(words::KEYBOARD_AND_SHORTCUTS).get(lang),),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .wrap(),
        );
        let controls = self.controls();
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        let head = |words: &'static str, note: String| -> El<'_> {
            row![
                label(words),
                Space::new().width(Length::Fill),
                t(note, 12.0, FAINT.get())
            ]
            .align_y(Alignment::Center)
            .height(28)
            .into()
        };
        let group = |head: El<'a>, rows: Vec<El<'a>>| -> El<'a> {
            let mut body = column![];
            for (i, r) in rows.into_iter().enumerate() {
                body = body.push(divided(i > 0, r));
            }
            column![head, card_box(body.into())].spacing(8).into()
        };
        page = page.push(group(
            head(
                model::CAPTURES.label.get(lang),
                w(words::COPIED_AND_SAVED).get(lang).to_owned(),
            ),
            vec![
                self.folder_row(Folder::Captures),
                model_row(model::CAPTURE_FORMAT),
                model_row(model::SOUND),
            ],
        ));
        page = page.push(group(
            head(
                model::RECORDING.label.get(lang),
                w(words::SOUND_AND_MICROPHONE_ARE_CHOSEN_EACH)
                    .get(lang)
                    .to_owned(),
            ),
            vec![
                self.folder_row(Folder::Recordings),
                model_row(model::RECORD_FORMAT),
            ],
        ));
        let count = self
            .captures
            .count
            .map(|(items, pinned)| model::count_said(items, pinned, lang))
            .unwrap_or_default();
        let clipboard = group(
            head(model::CLIPBOARD.label.get(lang), count),
            vec![
                model_row(model::HISTORY),
                model_row(model::KEEP),
                model_row(model::SECRETS),
                self.clear_row(),
            ],
        );
        let foot = if self.key_in_a_file() {
            w(words::KEPT_ENCRYPTED_ON_THIS_COMPUTER_ITS)
        } else {
            w(words::KEPT_ENCRYPTED_ON_THIS_COMPUTER_ITS_2)
        };
        page = page.push(
            column![
                clipboard,
                container(t(foot.get(lang), 12.0, FAINT.get())).padding([0, 4])
            ]
            .spacing(8),
        );
        container(page).id("page:captures".to_owned()).into()
    }

    /// Whether the history's key fell back to a file (5.6's
    /// `clipboard.key`).
    fn key_in_a_file(&self) -> bool {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/state"))
            });
        state.is_some_and(|s| s.join("hocuspanda/clipboard.key").exists())
    }

    /// A row's frame, lit when found.
    fn captures_frame<'a>(&self, id: &'static str, body: El<'a>) -> El<'a> {
        let lit = self.lit(id);
        container(body)
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .center_y(Length::Shrink)
            .height(Length::Shrink)
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(format!("row:{id}"))
            .into()
    }

    /// "Dossier": its next file's name, or why it is passed over; the path
    /// button and "Changer…".
    fn folder_row(&self, folder: Folder) -> El<'_> {
        let lang = self.lang;
        let (used, state) = self.folder_used(folder);
        let chosen = model::chosen(&self.captures_text(folder.setting()));
        let extension = match folder {
            Folder::Captures if self.captures_word(model::CAPTURE_FORMAT) == Some("jpeg") => "jpg",
            Folder::Captures => "png",
            Folder::Recordings if self.captures_word(model::RECORD_FORMAT) == Some("webm") => {
                "webm"
            }
            Folder::Recordings => "mp4",
        };
        let now = jiff::Zoned::now();
        let (day, time) = (
            now.strftime("%Y-%m-%d").to_string(),
            now.strftime("%H-%M-%S").to_string(),
        );
        let (line, warn) = match state {
            Some(state) => (
                folder.unwritable(state == Writable::Gone, &model::shown(&used), lang),
                true,
            ),
            None if folder == Folder::Recordings && self.captures.recording => {
                (w(words::FOR_THE_NEXT_RECORDING).get(lang).to_owned(), false)
            }
            None => (folder.sample((&day, &time), extension, lang), false),
        };
        let words = column![
            t(w(words::FOLDER).get(lang).to_owned(), 14.0, TEXT.get()).font(SEMIBOLD),
            t(line, 12.5, if warn { PEACH.get() } else { DIM.get() }),
        ]
        .spacing(2)
        .width(Length::Fill);
        // The path shown: the one chosen, at 60 % while it is passed over.
        let shown_path = match (&chosen, state) {
            (Some(path), Some(_)) => path.clone(),
            _ => used.clone(),
        };
        let faded = if state.is_some() { 0.6 } else { 1.0 };
        let open_focused = self.captures_focused(&Stop::Open(folder));
        let path = container(
            button(
                row![
                    icon(Icon::Folder, 15.0, fade(DIM.get(), faded)),
                    t(model::shown(&shown_path), 13.0, fade(TEXT.get(), faded)).font(SEMIBOLD),
                ]
                .spacing(7)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([0, 11])
            .on_press(Message::Captures(Msg::Open(folder)))
            .style(control_style(open_focused)),
        )
        .id(format!("{}:open", folder.word()));
        let change_focused = self.captures_focused(&Stop::Change(folder));
        let mut change = button(
            container(t(w(words::CHANGE).get(lang).to_owned(), 13.0, TEXT.get()).font(SEMIBOLD))
                .height(Length::Fill)
                .center_y(Length::Fill),
        )
        .height(32)
        .padding([0, 12])
        .style(control_style(change_focused));
        if self.writable() {
            change = change.on_press(Message::Captures(Msg::Change(folder)));
        }
        let change = container(change).id(format!("{}:change", folder.word()));
        self.captures_frame(
            match folder {
                Folder::Captures => model::CAPTURE_FOLDER,
                Folder::Recordings => model::RECORD_FOLDER,
            },
            row![words, row![path, change].spacing(8)]
                .spacing(16)
                .align_y(Alignment::Center)
                .into(),
        )
    }

    /// The applications left out: the built-in ones, then the file's (with
    /// ✕), then "+ Ajouter".
    fn chips(&self) -> El<'_> {
        let lang = self.lang;
        let inert = !self.history_on() || !self.captures_bool(model::SECRETS);
        let alpha = if inert { INERT } else { 1.0 };
        let chip = |id: &str, name: String, remove: Option<usize>| -> El<'_> {
            let tile = container(
                t(
                    name.chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string(),
                    10.5,
                    fade(rgb(0x10141E), alpha),
                )
                .font(BOLD),
            )
            .width(16)
            .height(16)
            .center_x(16)
            .center_y(16)
            .style(boxed(fade(tile_colour(id), alpha), 4.0, None));
            let mut inside = row![tile, t(name, 12.5, fade(TEXT.get(), alpha)).font(SEMIBOLD)]
                .spacing(6)
                .align_y(Alignment::Center);
            if let Some(i) = remove {
                let focused = self.captures_focused(&Stop::Remove(i));
                let mut x = button(icon(Icon::Close, 12.0, fade(DIM.get(), alpha)))
                    .width(20)
                    .height(20)
                    .padding(4)
                    .style(flat(None, HOVER.get(), 5.0, focused));
                if !inert && self.writable() {
                    x = x.on_press(Message::Captures(Msg::Remove(i)));
                }
                inside = inside.push(container(x).id(format!("chip:{i}:remove")));
            }
            container(inside)
                .height(26)
                .padding(Padding {
                    top: 0.0,
                    right: if remove.is_some() { 3.0 } else { 9.0 },
                    bottom: 0.0,
                    left: 5.0,
                })
                .center_y(26)
                .style(boxed(
                    crate::theme::ink(rgba(255, 255, 255, 0.06 * alpha)),
                    8.0,
                    Some((1.0, crate::theme::ink(rgba(255, 255, 255, 0.10 * alpha)))),
                ))
                .id(format!("chip:{id}"))
                .into()
        };
        let mut chips = row![].spacing(6).align_y(Alignment::Center);
        for (id, name) in model::BUILT_IN {
            chips = chips.push(chip(id, name.to_owned(), None));
        }
        for (i, id) in self.ignored().iter().enumerate() {
            chips = chips.push(chip(id, app_name(id), Some(i)));
        }
        let focused = self.captures_focused(&Stop::Add);
        let mut add = button(
            container(
                t(
                    w(words::ADD).get(lang).to_owned(),
                    12.5,
                    fade(MINT.get(), alpha),
                )
                .font(SEMIBOLD),
            )
            .height(Length::Fill)
            .center_y(Length::Fill),
        )
        .height(26)
        .padding([0, 8])
        .style(flat(None, HOVER.get(), 7.0, focused));
        if !inert && self.writable() {
            add = add.on_press(Message::Captures(Msg::AddMenu));
        }
        chips = chips.push(container(add).id("captures:add".to_owned()));
        container(chips.wrap())
            .padding(Padding::default().top(8))
            .into()
    }

    /// "Effacer l'historique", and its confirmation in place.
    fn clear_row(&self) -> El<'_> {
        let lang = self.lang;
        let inert = !self.history_on() || !self.writable();
        let alpha = if inert { INERT } else { 1.0 };
        let erasable = self.erasable();
        let pinned = self.captures.count.map_or(0, |(_, p)| p);
        let line = if self.captures.confirming {
            let n = model::items_said(erasable, lang);
            match (lang, pinned) {
                (Lang::French, 0) => format!("{n}, sur cet ordinateur."),
                (Lang::English, 0) => format!("{n}, on this computer."),
                (Lang::French, 1) => format!("{n}, sauf l’épinglé."),
                (Lang::English, 1) => format!("{n}, but the pinned one."),
                (Lang::French, p) => format!("{n}, sauf les {p} épinglés."),
                (Lang::English, p) => format!("{n}, but the {p} pinned."),
            }
        } else {
            w(words::EVERYTHING_BUT_THE_PINNED_ITEMS_ON)
                .get(lang)
                .to_owned()
        };
        let words = column![
            t(
                w(words::CLEAR_THE_HISTORY).get(lang).to_owned(),
                14.0,
                fade(TEXT.get(), alpha)
            )
            .font(SEMIBOLD),
            t(line, 12.5, fade(DIM.get(), alpha)),
        ]
        .spacing(2)
        .width(Length::Fill);
        let plain = |said: String, stop: Stop, msg: Msg, ghost: bool| -> El<'_> {
            let focused = self.captures_focused(&stop);
            let mut b = button(
                container(t(said, 13.0, fade(TEXT.get(), alpha)).font(SEMIBOLD))
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            )
            .height(32)
            .padding([0, 12])
            .style(if ghost {
                Box::new(flat(None, HOVER.get(), 8.0, focused))
                    as Box<dyn Fn(&Theme, button::Status) -> button::Style>
            } else {
                Box::new(control_style(focused))
            });
            if !inert {
                b = b.on_press(Message::Captures(msg));
            }
            container(b)
                .id(format!(
                    "clear:{}",
                    match stop {
                        Stop::Keep => "keep",
                        Stop::Confirm => "confirm",
                        _ => "clear",
                    }
                ))
                .into()
        };
        let controls: El<'_> = if self.captures.confirming {
            let focused = self.captures_focused(&Stop::Confirm);
            let confirm = button(
                container(
                    t(
                        match lang {
                            Lang::French => {
                                format!("Effacer {}", model::items_said(erasable, lang))
                            }
                            Lang::English => {
                                format!("Clear {}", model::items_said(erasable, lang))
                            }
                        },
                        13.0,
                        rgb(0xFFB3B6),
                    )
                    .font(SEMIBOLD),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .height(32)
            .padding([0, 12])
            .on_press(Message::Captures(Msg::Confirm))
            .style(move |_, status| button::Style {
                background: Some(Background::Color(rgba(
                    242,
                    84,
                    91,
                    if matches!(status, button::Status::Hovered) {
                        0.2
                    } else {
                        0.12
                    },
                ))),
                text_color: rgb(0xFFB3B6),
                border: if focused {
                    ring(8.0)
                } else {
                    border(8.0, 1.0, rgba(242, 84, 91, 0.35))
                },
                ..button::Style::default()
            });
            row![
                plain(
                    w(words::KEEP).get(lang).to_owned(),
                    Stop::Keep,
                    Msg::Keep,
                    true
                ),
                container(confirm).id("clear:confirm".to_owned()),
            ]
            .spacing(8)
            .into()
        } else {
            plain(
                w(words::CLEAR).get(lang).to_owned(),
                Stop::Clear,
                Msg::Clear,
                false,
            )
        };
        self.captures_frame(
            model::CLEAR,
            row![words, controls]
                .spacing(16)
                .align_y(Alignment::Center)
                .into(),
        )
    }

    fn captures_link<'a>(&self, target: &'static str, said: &str) -> El<'a> {
        let focused = self.captures_focused(&Stop::Link(target));
        container(
            button(t(said.to_owned(), 12.5, MINT.get()).font(SEMIBOLD))
                .padding([1, 2])
                .on_press(Message::Captures(Msg::Link(target)))
                .style(move |_, status| button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(5.0)
                    } else {
                        border(5.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }),
        )
        .id(format!("link:{target}"))
        .into()
    }

    /// "+ Ajouter"'s menu, under its button.
    pub(super) fn captures_menu_list(&self) -> Option<El<'_>> {
        let since = self.captures.adding?;
        let at = *self.places.at.get("captures:add")?;
        let opened = if self.reduced_motion {
            1.0
        } else {
            super::view::ease_out(self.now.saturating_duration_since(since), super::MENU_IN)
        };
        let choices = self.captures_addable();
        let width = 240.0;
        let mut list = column![];
        for (i, (_, name)) in choices.iter().enumerate() {
            list = list.push(
                container(
                    button(
                        container(t(name.clone(), 13.5, fade(TEXT.get(), opened)))
                            .height(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 10])
                    .on_press(Message::Captures(Msg::Add(i)))
                    .style(flat(
                        None,
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        if choices.is_empty() {
            list = list.push(
                container(t(
                    w(words::NO_OTHER_APPLICATION_INSTALLED).get(self.lang),
                    13.0,
                    FAINT.get(),
                ))
                .padding([8, 10]),
            );
        }
        let rows = choices.len().clamp(1, 10) as f32;
        let height = 10.0 + 32.0 * rows;
        let panel = container(iced::widget::scrollable(list).height(height - 10.0))
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let bottom = self
            .places
            .at
            .get(&format!(
                "sidebar:{}",
                if self.lang.fr() { "fr" } else { "en" }
            ))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + at.height + 6.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0) + 4.0 * (1.0 - opened)
        } else {
            under - 4.0 * (1.0 - opened)
        };
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Captures(Msg::CloseAdd)),
                pin(panel).x(at.x.max(0.0)).y(y),
            ]
            .into(),
        )
    }

    pub(super) fn captures_moving(&self, now: Instant) -> bool {
        !self.reduced_motion
            && self
                .captures
                .adding
                .is_some_and(|since| now.saturating_duration_since(since) < super::MENU_IN)
    }

    /// What the page shows, for the log.
    pub(super) fn captures_shown(&self) -> serde_json::Value {
        let folder = |f: Folder| {
            let (used, state) = self.folder_used(f);
            serde_json::json!({
                "used": used.display().to_string(),
                "state": state.map(|s| format!("{s:?}")),
            })
        };
        let line = |id| self.captures_line(id).map(|(l, _)| l);
        serde_json::json!({
            "count": self.captures.count,
            "recording": self.captures.recording,
            "confirming": self.captures.confirming,
            "cleared": self.captures.cleared.is_some(),
            "adding": self.captures.adding.map(|_| self.captures_addable().into_iter().map(|(_, n)| n).collect::<Vec<_>>()),
            "capture": folder(Folder::Captures),
            "recording_folder": folder(Folder::Recordings),
            "chips": self.ignored(),
            "lines": {
                "capture.format": line(model::CAPTURE_FORMAT),
                "recording.format": line(model::RECORD_FORMAT),
                "clipboard.history": line(model::HISTORY),
                "clipboard.skip-secrets": line(model::SECRETS),
            },
            "inert": !self.history_on(),
        })
    }
}

/// An application's name: its desktop entry's, else its id's last part.
fn app_name(id: &str) -> String {
    crate::apps::named(id).map_or_else(
        || {
            id.rsplit('.')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(id)
                .to_owned()
        },
        |a| a.name.clone(),
    )
}

/// A chip's tile's colour, the same for an id each time.
fn tile_colour(id: &str) -> Color {
    const COLOURS: [u32; 6] = [0x6CC287, 0x5B8DEF, 0x8FB3FF, 0xE8A87C, 0xE58AC8, 0x8FD3D9];
    let sum = id
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    rgb(COLOURS[(sum % COLOURS.len() as u32) as usize])
}

/// A 6.1 control's look: a button on the card, its rim, lit hovered.
fn control_style(focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255,
                255,
                255,
                if hovered { 0.09 } else { 0.05 },
            )))),
            text_color: TEXT.get(),
            border: if focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, RIM_12.get())
            },
            ..button::Style::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_is_named_by_its_ids_last_part_when_not_installed() {
        assert_eq!(app_name("com.protonpass.ProtonPass"), "ProtonPass");
        assert_eq!(tile_colour("a"), tile_colour("a"));
    }
}
