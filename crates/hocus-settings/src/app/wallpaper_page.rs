//! The "Fond d'écran" page (6.3, variant A, the gallery): the preview of
//! the screens, the gallery of backgrounds, the fit and the plain colour,
//! the lock screen's choices.
//!
//! What it writes goes through the file's worker as any page's: a tile, a
//! screen's own, the per-screen switch and the lock's picture as keys the
//! page writes itself (with their toasts' words), the fit, the colour and
//! the lock as rows of the window's. The thumbnails, the folder of one's
//! own pictures and the file chooser are the page's thread's
//! ([`crate::wallpaper`]).

use std::{path::PathBuf, sync::Arc};

use hocus_link::{
    backdrop::Backdrop,
    moving::Moving,
    theme::AccentColours,
    wallpaper::{Fit, LockLook, Look, Picture},
};
use iced::{
    Alignment, Background, Color, ContentFit, Length, Padding, Shadow, Theme, Vector,
    keyboard::key::Named,
    widget::{Space, button, column, container, image, pin, row, stack, text},
};

use super::{
    App, Control, Doing, KeysSaid, Message, Page, ToastAction, Undo,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    lang::{Lang, w},
    model::{Icon, SectionId, Value},
    store::{KeyEdit, Slot},
    wallpaper::{self as model, Heard, Mine, Picked, Thumb, Thumbs},
};

/// A tile's thumbnail, in pixels (16:10).
const TILE: (u32, u32) = (256, 160);
/// A tile's thumbnail on the page, logical pixels high.
const TILE_HIGH: f32 = 76.0;
/// The preview's drawing area.
const AREA: (f32, f32) = (702.0, 200.0);
/// The lock's drawing.
const LOCK_SIZE: (f32, f32) = (208.0, 130.0);
/// Columns of the gallery.
const COLUMNS: usize = 5;

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    worker: Option<model::Worker>,
    thumbs: Thumbs,
    mine: Vec<Mine>,
    listed: bool,
    /// The screen the gallery applies to, with per-screen.
    selected: Option<String>,
    /// "Copié dans …", after a picture is copied.
    copied: Option<String>,
    /// The lock's picture's list, open.
    lock_menu: bool,
    /// The gallery's tile that holds the keyboard's ring.
    ring: usize,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Heard),
    /// "Tous les écrans" (`false`) or "Chaque écran".
    Scope(bool),
    Screen(String),
    Tile(Tile),
    Remove(PathBuf),
    Lock(&'static str),
    LockMenu,
    LockPicture(Tile),
}

/// A tile of the gallery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tile {
    Builtin(Backdrop),
    /// A wallpaper that moves (#433), its tile its still picture.
    Moving(Moving),
    Plain,
    Mine(String),
    Choose,
}

impl Tile {
    fn picture(&self) -> Option<Picture> {
        match self {
            Self::Builtin(b) => Some(Picture::Builtin(*b)),
            Self::Moving(m) => Some(Picture::Moving(*m)),
            Self::Plain => Some(Picture::Plain),
            Self::Mine(word) => Some(Picture::File(word.clone())),
            Self::Choose => None,
        }
    }

    fn id(&self, mine: &[Mine]) -> String {
        match self {
            Self::Builtin(b) => format!("tile:{}", b.word()),
            Self::Moving(m) => format!("tile:{}", m.word()),
            Self::Plain => "tile:plain".to_owned(),
            Self::Mine(word) => format!(
                "tile:mine:{}",
                mine.iter()
                    .find(|m| &m.word == word)
                    .map_or(word.as_str(), |m| m.name.as_str())
            ),
            Self::Choose => "tile:choose".to_owned(),
        }
    }
}

/// The page's keyboard stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Scope,
    Screen(String),
    Gallery,
    Lock,
    LockPicture,
}

/// A screen as the preview draws it.
#[derive(Debug, Clone)]
struct Drawn {
    name: String,
    title: String,
    /// Its logical size, and whether the bar is on it.
    size: (f32, f32),
    primary: bool,
    plugged: bool,
}

/// `a` and `b` name the same picture (a path with or without its `~/`).
fn same(a: &Picture, b: &Picture) -> bool {
    match (a, b) {
        (Picture::File(_), Picture::File(_)) => a.path() == b.path(),
        _ => a == b,
    }
}

/// A word as a TOML string.
fn quoted(word: &str) -> String {
    toml_edit::Value::from(word).to_string().trim().to_owned()
}

impl App {
    /// The page's thread, started with the window.
    pub(super) fn wallpaper_start(
        &mut self,
        to_window: iced::futures::channel::mpsc::UnboundedSender<Message>,
    ) {
        self.wallpaper.worker = model::start(move |heard| {
            to_window
                .unbounded_send(Message::Wallpaper(Msg::Heard(heard)))
                .is_ok()
        });
    }

    fn wallpaper_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Wallpaper)
    }

    fn file(&self) -> model::File {
        self.snapshot
            .as_ref()
            .map(|s| s.wallpaper.as_ref().clone())
            .unwrap_or_default()
    }

    /// The screens the preview draws, by their places' order; one of the
    /// file's when the compositor says none.
    fn wallpaper_screens(&self) -> Vec<Drawn> {
        let lang = self.lang;
        let Some(known) = self.screens_list() else {
            return vec![Drawn {
                name: String::new(),
                title: w("Écran", "Screen").get(lang).to_owned(),
                size: (1600.0, 1000.0),
                primary: true,
                plugged: true,
            }];
        };
        let mut list: Vec<&crate::screens::Screen> = known.list.iter().collect();
        list.sort_by_key(|s| (!s.connected, s.position.0, s.position.1));
        list.into_iter()
            .map(|s| Drawn {
                name: s.name.clone(),
                title: s.title(lang),
                size: (s.size.0.max(1) as f32, s.size.1.max(1) as f32),
                primary: s.primary,
                plugged: s.connected,
            })
            .collect()
    }

    /// The screen the gallery applies to, with per-screen: the one
    /// chosen, else the primary.
    fn wallpaper_target(&self) -> Option<Drawn> {
        if !self.file().per_screen {
            return None;
        }
        let screens = self.wallpaper_screens();
        let plugged = || screens.iter().filter(|s| s.plugged && !s.name.is_empty());
        self.wallpaper
            .selected
            .as_ref()
            .and_then(|n| plugged().find(|s| &s.name == n))
            .or_else(|| plugged().find(|s| s.primary))
            .or_else(|| plugged().next())
            .cloned()
    }

    /// What the gallery's target shows now, as the file writes it.
    fn target_word(&self) -> Option<String> {
        let file = self.file();
        match self.wallpaper_target() {
            Some(screen) if file.per_screen => file
                .outputs
                .iter()
                .find(|(n, _)| *n == screen.name)
                .map(|(_, p)| p.clone())
                .or(file.picture),
            _ => file.picture,
        }
    }

    fn target_picture(&self) -> Picture {
        self.target_word()
            .and_then(|p| Picture::of(&p).ok())
            .unwrap_or_default()
    }

    /// The primary screen's look: the wallpaper's accent comes from it.
    fn primary_look(&self) -> Look {
        let file = self.file();
        let primary = self
            .wallpaper_screens()
            .into_iter()
            .find(|s| s.primary)
            .map(|s| s.name)
            .or_else(|| self.snapshot.as_ref()?.primary_output.clone())
            .unwrap_or_default();
        file.look(file.picture_of(&primary))
    }

    /// The accent `accent = "wallpaper"` takes: the primary screen's.
    pub(super) fn wallpaper_accent(&self) -> AccentColours {
        let look = self.primary_look();
        model::accent_of(&look)
            .or_else(|| {
                self.wallpaper
                    .thumbs
                    .pictures
                    .get(look.picture.word())
                    .and_then(|r| r.clone().ok())
            })
            .unwrap_or_default()
    }

    /// The window's theme, the file's with the wallpaper's accent.
    pub(super) fn wallpaper_theme(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let mut choice = snapshot.theme;
        if choice.accent == hocus_link::theme::Accent::Wallpaper {
            choice.wallpaper = self.wallpaper_accent();
        }
        if crate::theme::set(choice) {
            tracing::info!(theme = %choice.scheme, accent = %choice.accent, colours = %choice, "the window's theme");
        }
    }

    /// The thumbnails the page shows now, each asked of the thread once.
    pub(super) fn wallpaper_want(&mut self) {
        let Some(worker) = self.wallpaper.worker.clone() else {
            return;
        };
        // The primary's picture gives the accent wherever the window is.
        let mut wanted: Vec<Thumb> = Vec::new();
        let primary = self.primary_look();
        if matches!(primary.picture, Picture::File(_)) {
            wanted.push(Thumb {
                look: primary,
                size: TILE,
                soft: false,
            });
        }
        if self.wallpaper_shows() && self.snapshot.is_some() {
            if !self.wallpaper.listed {
                self.wallpaper.listed = worker.ask(model::Ask::List);
            }
            wanted.extend(self.wallpaper_thumbs());
        }
        for thumb in wanted {
            if self.wallpaper.thumbs.drawn.contains_key(&thumb)
                || self.wallpaper.thumbs.asked.contains(&thumb)
            {
                continue;
            }
            if worker.ask(model::Ask::Draw(thumb.clone())) {
                self.wallpaper.thumbs.asked.insert(thumb);
            }
        }
    }

    /// Every thumbnail of the page.
    fn wallpaper_thumbs(&self) -> Vec<Thumb> {
        let file = self.file();
        let mut out: Vec<Thumb> = self
            .wallpaper_tiles()
            .iter()
            .filter_map(|tile| Some(self.tile_thumb(&tile.picture()?)))
            .collect();
        for (screen, size) in self.preview_sizes() {
            out.push(Thumb {
                look: file.look(file.picture_of(&screen.name)),
                size: ((size.0 * 2.0).round() as u32, (size.1 * 2.0).round() as u32),
                soft: false,
            });
        }
        out.push(self.lock_thumb());
        if file.lock() == LockLook::Picture {
            for tile in self.wallpaper_tiles() {
                if let Some(picture) = tile.picture() {
                    out.push(self.tile_thumb(&picture));
                }
            }
        }
        out
    }

    fn tile_thumb(&self, picture: &Picture) -> Thumb {
        let file = self.file();
        Thumb {
            look: Look {
                picture: picture.clone(),
                fit: Fit::Cover,
                color: file.color(),
            },
            size: TILE,
            soft: false,
        }
    }

    fn lock_thumb(&self) -> Thumb {
        let file = self.file();
        let picture = match file.lock() {
            LockLook::Picture => file.lock_picture(),
            _ => self.primary_look().picture,
        };
        Thumb {
            look: file.look(picture),
            size: ((LOCK_SIZE.0 * 2.0) as u32, (LOCK_SIZE.1 * 2.0) as u32),
            soft: file.lock() == LockLook::Blurred,
        }
    }

    /// The gallery's tiles, in order.
    fn wallpaper_tiles(&self) -> Vec<Tile> {
        let mut tiles: Vec<Tile> = model::BUILTIN_TILES
            .iter()
            .map(|b| Tile::Builtin(*b))
            .collect();
        tiles.extend(model::MOVING_TILES.iter().map(|m| Tile::Moving(*m)));
        tiles.push(Tile::Plain);
        tiles.extend(
            self.wallpaper
                .mine
                .iter()
                .map(|m| Tile::Mine(m.word.clone())),
        );
        tiles.push(Tile::Choose);
        tiles
    }

    /// The preview's screens at their drawn sizes.
    fn preview_sizes(&self) -> Vec<(Drawn, (f32, f32))> {
        let screens = self.wallpaper_screens();
        let total: f32 = screens.iter().map(|s| s.size.0).sum::<f32>()
            + 16.0 / 0.094 * (screens.len().saturating_sub(1)) as f32;
        let tallest = screens.iter().map(|s| s.size.1).fold(1.0, f32::max);
        let scale = ((AREA.0 - 48.0) / total.max(1.0))
            .min((AREA.1 - 50.0) / tallest)
            .min(0.12);
        screens
            .into_iter()
            .map(|s| {
                let size = (s.size.0 * scale, s.size.1 * scale);
                (s, size)
            })
            .collect()
    }

    /// The page's keyboard stops, in order.
    pub(super) fn wallpaper_controls(&self) -> Vec<Control> {
        let file = self.file();
        let mut out = vec![Control::Wallpaper(Stop::Scope)];
        if file.per_screen {
            for (screen, _) in self.preview_sizes() {
                if screen.plugged && !screen.name.is_empty() {
                    out.push(Control::Wallpaper(Stop::Screen(screen.name)));
                }
            }
        }
        out.push(Control::Wallpaper(Stop::Gallery));
        let picture = self.target_picture();
        if matches!(picture, Picture::File(_)) {
            out.push(self.setting_control(model::FIT));
        }
        if self.colour_row_shows() {
            out.push(self.setting_control(model::COLOR));
        }
        out.push(self.setting_control(model::ANIMATE));
        out.push(self.setting_control(model::ON_BATTERY));
        out.push(Control::Wallpaper(Stop::Lock));
        if file.lock() == LockLook::Picture {
            out.push(Control::Wallpaper(Stop::LockPicture));
        }
        out
    }

    fn setting_control(&self, id: &str) -> Control {
        let setting = model::settings().iter().find(|s| s.id == id);
        match setting {
            Some(s) => Control::Setting(Slot::of(s, self.source)),
            None => Control::Wallpaper(Stop::Gallery),
        }
    }

    fn colour_row_shows(&self) -> bool {
        match self.target_picture() {
            Picture::Plain => true,
            Picture::File(_) => self.file().fit() != Fit::Cover,
            Picture::Builtin(_) | Picture::Moving(_) => false,
        }
    }

    fn focused_stop(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Wallpaper(stop.clone())))
    }

    // --- What changes it --------------------------------------------------------------

    pub(super) fn wallpaper_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Heard(heard) => self.wallpaper_heard(heard),
            Msg::Scope(each) => self.wallpaper_scope(each),
            Msg::Screen(name) => {
                if self.file().per_screen {
                    self.wallpaper.selected = Some(name);
                    iced::Task::none()
                } else {
                    let text = self
                        .say(w(
                            "Choisis « Chaque écran » pour donner à cet écran un fond à lui.",
                            "Choose “Each screen” to give this screen a background of its own.",
                        ))
                        .to_owned();
                    self.toast(text, ToastAction::None, super::TOAST)
                }
            }
            Msg::Tile(Tile::Choose) => self.wallpaper_pick(false),
            Msg::Tile(tile) => {
                let Some(picture) = tile.picture() else {
                    return iced::Task::none();
                };
                self.wallpaper_choose(&picture)
            }
            Msg::Remove(path) => self.wallpaper_remove(path),
            Msg::Lock(word) => {
                self.wallpaper.lock_menu = false;
                let Some(setting) = model::settings().iter().find(|s| s.id == model::LOCK_ID)
                else {
                    return iced::Task::none();
                };
                let slot = Slot::of(setting, self.source);
                if self.value(slot) == Some(Value::Word(word)) {
                    return iced::Task::none();
                }
                self.update(Message::Choose(slot, Value::Word(word)))
            }
            Msg::LockMenu => {
                self.wallpaper.lock_menu = !self.wallpaper.lock_menu;
                iced::Task::none()
            }
            Msg::LockPicture(Tile::Choose) => {
                self.wallpaper.lock_menu = false;
                self.wallpaper_pick(true)
            }
            Msg::LockPicture(tile) => {
                self.wallpaper.lock_menu = false;
                let Some(picture) = tile.picture() else {
                    return iced::Task::none();
                };
                self.wallpaper_lock_picture(&picture);
                iced::Task::none()
            }
        }
    }

    fn wallpaper_heard(&mut self, heard: Heard) -> iced::Task<Message> {
        match heard {
            Heard::Drawn {
                thumb,
                handle,
                accent,
                failed,
            } => {
                self.wallpaper.thumbs.asked.remove(&thumb);
                if let Picture::File(word) = &thumb.look.picture {
                    self.wallpaper
                        .thumbs
                        .pictures
                        .insert(word.clone(), failed.map_or(Ok(accent), Err));
                }
                // Those of other times go, past a few dozen.
                if self.wallpaper.thumbs.drawn.len() > 48 {
                    let wanted = self.wallpaper_thumbs();
                    self.wallpaper
                        .thumbs
                        .drawn
                        .retain(|t, _| wanted.contains(t));
                }
                self.wallpaper.thumbs.drawn.insert(thumb, handle);
                self.wallpaper_theme();
                iced::Task::none()
            }
            Heard::Listed(mine) => {
                self.wallpaper.mine = mine;
                iced::Task::none()
            }
            Heard::Picked { lock, picked } => self.wallpaper_picked(lock, picked),
            Heard::Removed { path, bytes } => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.undo.push(Undo::Picture { path, bytes });
                self.redo.clear();
                let text = match self.lang {
                    Lang::French => format!("{name} retirée de tes images"),
                    Lang::English => format!("{name} removed from your pictures"),
                };
                self.toast(text, ToastAction::Undo, super::TOAST)
            }
            Heard::PutBack(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let text = match self.lang {
                    Lang::French => format!("Annulé : {name} revient dans tes images."),
                    Lang::English => format!("Undone: {name} is back in your pictures."),
                };
                self.toast(text, ToastAction::None, super::UNDONE)
            }
            Heard::Failed(why) => {
                let text = format!(
                    "{} {why}",
                    self.say(w("Rien n’est changé :", "Nothing is changed:"))
                );
                self.toast(text, ToastAction::None, super::TOAST)
            }
        }
    }

    /// An undo of a picture removed: put back.
    pub(super) fn wallpaper_put_back(&mut self, path: PathBuf, bytes: Arc<Vec<u8>>) {
        if let Some(worker) = &self.wallpaper.worker {
            worker.ask(model::Ask::PutBack {
                path,
                bytes: bytes.as_ref().clone(),
            });
        }
    }

    /// "Tous les écrans | Chaque écran": `per-screen`; turned on the first
    /// time, each screen's own written from `picture`.
    fn wallpaper_scope(&mut self, each: bool) -> iced::Task<Message> {
        let file = self.file();
        if file.per_screen == each {
            return iced::Task::none();
        }
        let mut edits = vec![KeyEdit {
            key: vec!["wallpaper".into(), "per-screen".into()],
            value: Some(each.to_string()),
        }];
        if each && file.outputs.is_empty() {
            let picture = file.picture.clone().unwrap_or_else(|| "night".to_owned());
            for (screen, _) in self.preview_sizes() {
                if screen.plugged && !screen.name.is_empty() {
                    edits.push(KeyEdit {
                        key: vec!["outputs".into(), screen.name.clone(), "wallpaper".into()],
                        value: Some(quoted(&picture)),
                    });
                }
            }
        }
        let lang = self.lang;
        let (now, then) = if each {
            (
                w("Chaque écran", "Each screen"),
                w("Tous les écrans", "All screens"),
            )
        } else {
            (
                w("Tous les écrans", "All screens"),
                w("Chaque écran", "Each screen"),
            )
        };
        let said = KeysSaid {
            done: match lang {
                Lang::French => format!("Fond d’écran : {}", now.get(lang).to_lowercase()),
                Lang::English => format!("Wallpaper: {}", now.get(lang).to_lowercase()),
            },
            undone: match lang {
                Lang::French => format!("Annulé : {}.", then.get(lang).to_lowercase()),
                Lang::English => format!("Undone: {}.", then.get(lang).to_lowercase()),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {}.", now.get(lang).to_lowercase()),
                Lang::English => format!("Redone: {}.", now.get(lang).to_lowercase()),
            },
            row: model::PER_SCREEN,
        };
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    /// A background chosen for the gallery's target.
    fn wallpaper_choose(&mut self, picture: &Picture) -> iced::Task<Message> {
        let lang = self.lang;
        let before = self.target_picture();
        if same(&before, picture) && self.wallpaper_warning().is_none() {
            return iced::Task::none();
        }
        let target = self.wallpaper_target();
        let key: Vec<String> = match &target {
            Some(screen) => vec!["outputs".into(), screen.name.clone(), "wallpaper".into()],
            None => vec!["wallpaper".into(), "picture".into()],
        };
        let now = model::name_of(picture, lang);
        let then = model::name_of(&before, lang);
        let whose = target.map(|s| crate::screens::lower_first(&s.title));
        let said = KeysSaid {
            done: match (lang, &whose) {
                (Lang::French, None) => format!("Fond d’écran : {now}"),
                (Lang::French, Some(s)) => format!("Fond de l’{s} : {now}"),
                (Lang::English, None) => format!("Wallpaper: {now}"),
                (Lang::English, Some(s)) => format!("Background of the {s}: {now}"),
            },
            undone: match lang {
                Lang::French => format!("Annulé : le fond revient à {then}."),
                Lang::English => format!("Undone: the background is back to {then}."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {now}."),
                Lang::English => format!("Redone: {now}."),
            },
            row: model::PICTURE,
        };
        self.ask_keys(
            vec![KeyEdit {
                key,
                value: Some(quoted(picture.word())),
            }],
            said,
            Doing::New,
            None,
        );
        iced::Task::none()
    }

    /// The lock's own picture, and `lock = "picture"` with it.
    fn wallpaper_lock_picture(&mut self, picture: &Picture) {
        let lang = self.lang;
        let file = self.file();
        let now = model::name_of(picture, lang);
        let then = model::name_of(&file.lock_picture(), lang);
        let said = KeysSaid {
            done: match lang {
                Lang::French => format!("Image du verrouillage : {now}"),
                Lang::English => format!("Lock screen picture: {now}"),
            },
            undone: match lang {
                Lang::French => format!("Annulé : le verrouillage revient à {then}."),
                Lang::English => format!("Undone: the lock screen is back to {then}."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {now}."),
                Lang::English => format!("Redone: {now}."),
            },
            row: model::LOCK_PICTURE,
        };
        let mut edits = vec![KeyEdit {
            key: vec!["wallpaper".into(), "lock-picture".into()],
            value: Some(quoted(picture.word())),
        }];
        if file.lock() != LockLook::Picture {
            edits.push(KeyEdit {
                key: vec!["wallpaper".into(), "lock".into()],
                value: Some(quoted("picture")),
            });
        }
        self.ask_keys(edits, said, Doing::New, None);
    }

    /// "Choisir une image…": the file chooser, on the page's thread.
    fn wallpaper_pick(&mut self, lock: bool) -> iced::Task<Message> {
        if !self.writable() {
            return iced::Task::none();
        }
        if let Some(worker) = &self.wallpaper.worker {
            worker.ask(model::Ask::Pick {
                lock,
                lang: self.lang,
            });
        }
        iced::Task::none()
    }

    fn wallpaper_picked(&mut self, lock: bool, picked: Picked) -> iced::Task<Message> {
        let lang = self.lang;
        match picked {
            Picked::Copied { copy, name: _ } => {
                let written = model::written(&copy);
                tracing::info!(copy = written, "the chosen picture is the background");
                self.wallpaper.copied = Some(match lang {
                    Lang::French => format!("Copié dans {written}"),
                    Lang::English => format!("Copied to {written}"),
                });
                let picture = Picture::File(written);
                if lock {
                    self.wallpaper_lock_picture(&picture);
                    iced::Task::none()
                } else {
                    self.wallpaper_choose(&picture)
                }
            }
            Picked::Cancelled => iced::Task::none(),
            Picked::NotAPicture(name) => {
                let text = match lang {
                    Lang::French => format!("{name} ne s’ouvre pas comme une image."),
                    Lang::English => format!("{name} does not open as a picture."),
                };
                self.toast(text, ToastAction::None, super::TOAST)
            }
            Picked::NoPortal => {
                let text = self
                    .say(w(
                        "Aucun sélecteur de fichiers ne répond : le portail du bureau n’est pas là.",
                        "No file chooser answers: the desktop portal is not there.",
                    ))
                    .to_owned();
                self.toast(text, ToastAction::None, super::TOAST)
            }
            Picked::Failed(why) => {
                let text = format!(
                    "{} {why}",
                    self.say(w("Rien n’est écrit :", "Nothing is written:"))
                );
                self.toast(text, ToastAction::None, super::TOAST)
            }
        }
    }

    /// ✕ on a picture of one's own: refused while it is in use, else
    /// removed (with "Annuler").
    fn wallpaper_remove(&mut self, path: PathBuf) -> iced::Task<Message> {
        let file = self.file();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let this = Picture::File(path.display().to_string());
        let mut used = vec![file.picture(), file.lock_picture()];
        used.extend(file.outputs.iter().filter_map(|(_, p)| Picture::of(p).ok()));
        if used.iter().any(|p| same(p, &this)) {
            let text = match self.lang {
                Lang::French => format!("{name} sert encore : choisis d’abord un autre fond."),
                Lang::English => {
                    format!("{name} is still in use: choose another background first.")
                }
            };
            return self.toast(text, ToastAction::None, super::TOAST);
        }
        if let Some(worker) = &self.wallpaper.worker {
            worker.ask(model::Ask::Remove(path));
        }
        iced::Task::none()
    }

    /// The page's keys, on its own stops.
    pub(super) fn wallpaper_key(&mut self, stop: &Stop, named: Named) -> iced::Task<Message> {
        match (stop, named) {
            (Stop::Scope, Named::ArrowLeft) => self.wallpaper_scope(false),
            (Stop::Scope, Named::ArrowRight) => self.wallpaper_scope(true),
            (Stop::Screen(name), Named::Space | Named::Enter) => {
                self.wallpaper_update(Msg::Screen(name.clone()))
            }
            (Stop::Gallery, _) => {
                let tiles = self.wallpaper_tiles();
                let last = tiles.len().saturating_sub(1);
                let ring = self.wallpaper.ring.min(last);
                self.wallpaper.ring = match named {
                    Named::ArrowRight => (ring + 1).min(last),
                    Named::ArrowLeft => ring.saturating_sub(1),
                    Named::ArrowDown => (ring + COLUMNS).min(last),
                    Named::ArrowUp => ring.saturating_sub(COLUMNS),
                    _ => ring,
                };
                match (named, tiles.get(self.wallpaper.ring)) {
                    (Named::Space | Named::Enter, Some(tile)) => {
                        self.wallpaper_update(Msg::Tile(tile.clone()))
                    }
                    (Named::Delete, Some(Tile::Mine(word))) => {
                        let path = hocus_link::wallpaper::expand(word);
                        self.wallpaper_remove(path)
                    }
                    _ => iced::Task::none(),
                }
            }
            (
                Stop::Lock,
                Named::ArrowDown | Named::ArrowRight | Named::ArrowUp | Named::ArrowLeft,
            ) => {
                let now = self.file().lock();
                let at = LockLook::ALL.iter().position(|l| *l == now).unwrap_or(0);
                let next = if matches!(named, Named::ArrowDown | Named::ArrowRight) {
                    (at + 1).min(LockLook::ALL.len() - 1)
                } else {
                    at.saturating_sub(1)
                };
                self.wallpaper_update(Msg::Lock(LockLook::ALL[next].word()))
            }
            (Stop::LockPicture, Named::Space | Named::Enter) => {
                self.wallpaper_update(Msg::LockMenu)
            }
            _ => iced::Task::none(),
        }
    }

    /// The file's picture for the target does not read: what the page
    /// says above the gallery.
    fn wallpaper_warning(&self) -> Option<String> {
        let word = self.target_word()?;
        let lang = self.lang;
        let not_found = match Picture::of(&word) {
            Err(_) => Some(match lang {
                Lang::French => "non compris",
                Lang::English => "not understood",
            }),
            Ok(Picture::File(_)) => match self.wallpaper.thumbs.pictures.get(&word) {
                Some(Err(_)) => Some(match lang {
                    Lang::French => "introuvable",
                    Lang::English => "not found",
                }),
                _ => None,
            },
            Ok(_) => None,
        }?;
        Some(match lang {
            Lang::French => format!(
                "Dans le fichier : « {word} », {not_found} ; Nuit s’affiche. Choisis un fond pour remplacer cette ligne."
            ),
            Lang::English => format!(
                "In the file: “{word}”, {not_found}; Night is drawn. Choose a background to replace this line."
            ),
        })
    }

    // --- What it draws ----------------------------------------------------------------

    pub(super) fn wallpaper_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        page = page.push(self.preview_group());
        page = page.push(self.gallery_group());
        page = page.push(self.animation_group());
        page = page.push(self.lock_group());
        container(page).id("page:wallpaper".to_owned()).into()
    }

    fn thumb<'a>(&self, thumb: &Thumb, width: f32, height: f32) -> El<'a> {
        match self.wallpaper.thumbs.drawn.get(thumb) {
            Some(handle) => image(handle.clone())
                .width(width)
                .height(height)
                .content_fit(ContentFit::Fill)
                .into(),
            None => container(Space::new().width(width).height(height))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(CARD.get())),
                    ..container::Style::default()
                })
                .into(),
        }
    }

    fn preview_group(&self) -> El<'_> {
        let lang = self.lang;
        let file = self.file();
        let inert = !self.writable();
        let focused = self.focused_stop(&Stop::Scope);
        let segment = |each: bool, words: crate::lang::Words| {
            let chosen = file.per_screen == each;
            let mut b = button(
                t(
                    words.get(lang),
                    12.5,
                    if chosen { TEXT.get() } else { DIM.get() },
                )
                .font(SEMIBOLD),
            )
            .height(26)
            .padding([4, 12])
            .style(move |_, _| button::Style {
                background: chosen.then(|| {
                    Background::Color(crate::theme::Ink::Token(|t| t.segment_chosen).get())
                }),
                text_color: TEXT.get(),
                border: border(7.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            });
            if !inert && !chosen {
                b = b.on_press(Message::Wallpaper(Msg::Scope(each)));
            }
            container(b).id(format!(
                "wallpaper-scope:{}",
                if each { "each" } else { "all" }
            ))
        };
        let scope = container(row![
            segment(false, w("Tous les écrans", "All screens")),
            segment(true, w("Chaque écran", "Each screen")),
        ])
        .padding(2)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(
                crate::theme::Ink::Token(|t| t.segmented).get(),
            )),
            border: if focused {
                ring(9.0)
            } else {
                border(9.0, 0.0, Color::TRANSPARENT)
            },
            ..container::Style::default()
        });
        let head = row![
            label(w("APERÇU", "PREVIEW").get(lang)),
            Space::new().width(Length::Fill),
            scope
        ]
        .align_y(Alignment::Center)
        .height(28);
        let target = self.wallpaper_target().map(|s| s.name);
        let mut screens = row![].spacing(16).align_y(Alignment::End);
        for (screen, (sw, sh)) in self.preview_sizes() {
            let picture = file.picture_of(&screen.name);
            let caption = if screen.plugged {
                format!("{} · {}", screen.title, model::name_of(&picture, lang))
            } else {
                screen.title.clone()
            };
            let chosen = file.per_screen && target.as_deref() == Some(screen.name.as_str());
            let body: El<'_> = if screen.plugged {
                let thumb = Thumb {
                    look: file.look(picture),
                    size: ((sw * 2.0).round() as u32, (sh * 2.0).round() as u32),
                    soft: false,
                };
                let mut layers = stack![self.thumb(&thumb, sw, sh)];
                if screen.primary {
                    // The bar: its 196 logical px, 8 from the right edge.
                    let scale = sw / screen.size.0;
                    let bar_w = (196.0 * scale).max(3.0);
                    let inset = 8.0 * scale;
                    layers = layers.push(
                        pin(container(
                            Space::new()
                                .width(bar_w)
                                .height((sh - 2.0 * inset).max(1.0)),
                        )
                        .style(|_: &Theme| container::Style {
                            background: Some(Background::Color(rgba(22, 27, 42, 0.72))),
                            border: border(2.0, 1.0, rgba(255, 255, 255, 0.10)),
                            ..container::Style::default()
                        }))
                        .x(sw - inset - bar_w)
                        .y(inset),
                    );
                }
                layers.width(sw).height(sh).into()
            } else {
                container(t(w("débranché", "unplugged").get(lang), 11.0, FAINT.get()))
                    .width(sw)
                    .height(sh)
                    .center_x(sw)
                    .center_y(sh)
                    .into()
            };
            let rim = if chosen {
                border(4.0, 2.0, ACCENT.get())
            } else if screen.plugged {
                border(4.0, 1.0, rgba(255, 255, 255, 0.16))
            } else {
                border(4.0, 1.5, rgba(255, 255, 255, 0.18))
            };
            let focused = self.focused_stop(&Stop::Screen(screen.name.clone()));
            let mut press = button(container(body).padding(if chosen { 2 } else { 1 }).style(
                move |_: &Theme| container::Style {
                    border: rim,
                    shadow: if chosen {
                        Shadow {
                            color: crate::theme::Ink::Accent(0.22).get(),
                            offset: Vector::ZERO,
                            blur_radius: 3.0,
                        }
                    } else {
                        Shadow::default()
                    },
                    ..container::Style::default()
                },
            ))
            .padding(0)
            .style(move |_, _| button::Style {
                border: if focused {
                    ring(6.0)
                } else {
                    border(6.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            });
            if screen.plugged && !screen.name.is_empty() && !inert {
                press = press.on_press(Message::Wallpaper(Msg::Screen(screen.name.clone())));
            }
            let caption = t(caption, 11.5, if chosen { MINT.get() } else { DIM.get() })
                .font(if chosen { BOLD } else { MANROPE });
            screens = screens.push(
                container(
                    column![press, caption]
                        .spacing(4)
                        .align_x(Alignment::Center),
                )
                .id(format!("wallpaper-screen:{}", screen.name)),
            );
        }
        let area = container(screens)
            .width(Length::Fill)
            .height(AREA.1)
            .center_x(Length::Fill)
            .center_y(AREA.1)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(rgba(0, 0, 0, 0.20))),
                border: border(8.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            });
        let line = if file.per_screen {
            w(
                "Clique un écran pour lui choisir un fond. La bande claire est la barre.",
                "Click a screen to choose its background. The light strip is the bar.",
            )
        } else {
            w(
                "Le même fond sur tous les écrans. La bande claire est la barre.",
                "The same background on every screen. The light strip is the bar.",
            )
        };
        let body = column![area, t(line.get(lang), 12.0, FAINT.get())]
            .spacing(8)
            .padding(8);
        column![
            head,
            card_box(
                container(body)
                    .id("row:wallpaper.per-screen".to_owned())
                    .into()
            )
        ]
        .spacing(8)
        .into()
    }

    fn gallery_group(&self) -> El<'_> {
        let lang = self.lang;
        let inert = !self.writable();
        let target = self.wallpaper_target();
        let whom = match (&target, lang) {
            (None, Lang::French) => "Pour tous les écrans".to_owned(),
            (None, Lang::English) => "For all screens".to_owned(),
            (Some(s), Lang::French) => {
                format!("Pour l’{}", crate::screens::lower_first(&s.title))
            }
            (Some(s), Lang::English) => {
                format!("For the {}", crate::screens::lower_first(&s.title))
            }
        };
        let head = row![
            label(w("FONDS", "BACKGROUNDS").get(lang)),
            Space::new().width(Length::Fill),
            t(whom, 12.5, SOFT.get()),
        ]
        .align_y(Alignment::Center)
        .height(28);
        let warning = self.wallpaper_warning();
        let current = self.target_picture();
        let tiles = self.wallpaper_tiles();
        let gallery_focused = self.focused_stop(&Stop::Gallery);
        let mut grid = column![].spacing(14);
        for (r, chunk) in tiles.chunks(COLUMNS).enumerate() {
            let mut line = row![].spacing(12);
            for (c, tile) in chunk.iter().enumerate() {
                let index = r * COLUMNS + c;
                let chosen =
                    warning.is_none() && tile.picture().is_some_and(|p| same(&p, &current));
                let ringed = gallery_focused && self.wallpaper.ring == index;
                line = line.push(self.tile_view(tile, chosen, ringed, inert));
            }
            for _ in chunk.len()..COLUMNS {
                line = line.push(Space::new().width(Length::Fill));
            }
            grid = grid.push(line);
        }
        let mut body = column![].spacing(10);
        if let Some(warning) = warning {
            body = body.push(
                row![
                    icon(Icon::Warning, 15.0, PEACH.get()),
                    t(warning, 12.5, PEACH.get())
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        // The search's rows: the gallery is the background's and the
        // built-ins'.
        body = body.push(
            container(container(grid).id("row:wallpaper.picture".to_owned()))
                .id("row:wallpaper.builtins".to_owned()),
        );
        if let Some(copied) = &self.wallpaper.copied {
            body = body.push(t(copied.clone(), 12.5, MINT.get()));
        }
        let controls = self.controls();
        let mut rows = column![];
        let mut any = false;
        let row_of = |id: &str| model::settings().iter().find(|s| s.id == id);
        if matches!(current, Picture::File(_))
            && let Some(setting) = row_of(model::FIT)
        {
            rows = rows.push(divided(any, self.row(setting, &controls)));
            any = true;
        }
        if self.colour_row_shows()
            && let Some(setting) = row_of(model::COLOR)
        {
            rows = rows.push(divided(any, self.row(setting, &controls)));
            any = true;
        }
        let mut card = column![container(body).padding(12)];
        if any {
            card = card.push(divided(true, rows.into()));
        }
        column![head, card_box(card.into())].spacing(8).into()
    }

    fn tile_view<'a>(&'a self, tile: &Tile, chosen: bool, ringed: bool, inert: bool) -> El<'a> {
        let lang = self.lang;
        let choose = *tile == Tile::Choose;
        let id = tile.id(&self.wallpaper.mine);
        let (name, fresh) = match tile {
            Tile::Builtin(b) => (
                model::name_of(&Picture::Builtin(*b), lang),
                *b != Backdrop::Night,
            ),
            Tile::Moving(m) => (model::name_of(&Picture::Moving(*m), lang), true),
            Tile::Plain => (model::name_of(&Picture::Plain, lang), false),
            Tile::Mine(word) => (model::name_of(&Picture::File(word.clone()), lang), false),
            Tile::Choose => (
                w("Choisir une image…", "Choose a picture…")
                    .get(lang)
                    .to_owned(),
                false,
            ),
        };
        let thumbnail: El<'a> = match tile.picture() {
            Some(picture) => {
                let thumb = self.tile_thumb(&picture);
                match self.wallpaper.thumbs.drawn.get(&thumb) {
                    Some(handle) => image(handle.clone())
                        .width(Length::Fill)
                        .height(TILE_HIGH)
                        .content_fit(ContentFit::Cover)
                        .into(),
                    None => container(Space::new().width(Length::Fill).height(TILE_HIGH))
                        .style(|_: &Theme| container::Style {
                            background: Some(Background::Color(CARD.get())),
                            ..container::Style::default()
                        })
                        .into(),
                }
            }
            None => container(t("+", 20.0, MINT.get()).font(SEMIBOLD))
                .width(Length::Fill)
                .height(TILE_HIGH)
                .center_x(Length::Fill)
                .center_y(TILE_HIGH)
                .style(|_: &Theme| container::Style {
                    border: border(7.0, 1.5, crate::theme::ink(rgba(255, 255, 255, 0.22))),
                    ..container::Style::default()
                })
                .into(),
        };
        // The ring around the thumbnail, 2 px out: the picture inside.
        let mut layers =
            stack![
                container(thumbnail)
                    .width(Length::Fill)
                    .padding(2)
                    .style(move |_: &Theme| container::Style {
                        border: if chosen {
                            border(8.0, 2.0, ACCENT.get())
                        } else {
                            border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.12)))
                        },
                        shadow: if chosen {
                            Shadow {
                                color: crate::theme::Ink::Accent(0.22).get(),
                                offset: Vector::ZERO,
                                blur_radius: 3.0,
                            }
                        } else {
                            Shadow::default()
                        },
                        ..container::Style::default()
                    })
            ];
        if chosen {
            layers = layers.push(
                pin(container(icon(Icon::Check, 12.0, DEEP.get()))
                    .width(18)
                    .height(18)
                    .center_x(18)
                    .center_y(18)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(ACCENT.get())),
                        border: border(9.0, 0.0, Color::TRANSPARENT),
                        ..container::Style::default()
                    }))
                .x(5)
                .y(5),
            );
        }
        if let Tile::Mine(word) = tile {
            let path = hocus_link::wallpaper::expand(word);
            let mut remove = button(
                container(icon(Icon::Close, 12.0, Color::WHITE))
                    .center_x(22)
                    .center_y(22),
            )
            .width(22)
            .height(22)
            .padding(0)
            .style(|_, _| button::Style {
                background: Some(Background::Color(rgba(11, 16, 32, 0.72))),
                border: border(6.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            });
            if !inert {
                remove = remove.on_press(Message::Wallpaper(Msg::Remove(path)));
            }
            let name = model::name_of(&Picture::File(word.clone()), lang);
            layers = layers.push(
                container(container(remove).id(format!("remove:{name}")))
                    .width(Length::Fill)
                    .align_right(Length::Fill)
                    .padding(5),
            );
        }
        let colour = if matches!(tile, Tile::Choose) {
            MINT.get()
        } else if chosen {
            TEXT.get()
        } else {
            SOFT.get()
        };
        let mut words = row![t(name, 12.5, colour).font(if chosen { BOLD } else { SEMIBOLD })]
            .spacing(6)
            .align_y(Alignment::Center);
        if fresh {
            // A wallpaper that moves says so; the other new ones are new.
            let badge = if matches!(tile, Tile::Moving(_)) {
                w("animé", "animated")
            } else {
                w("Nouveau", "New")
            };
            words = words.push(
                container(t(badge.get(lang), 10.0, BLUE.get()).font(SEMIBOLD))
                    .padding([1, 5])
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(rgba(143, 179, 255, 0.16))),
                        border: border(4.0, 0.0, Color::TRANSPARENT),
                        ..container::Style::default()
                    }),
            );
        }
        let content = column![layers, words].spacing(6);
        let message = Message::Wallpaper(Msg::Tile(tile.clone()));
        let mut press = button(content)
            .padding(0)
            .width(Length::Fill)
            .style(move |_, _| button::Style {
                border: if ringed {
                    ring(8.0)
                } else {
                    border(8.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            });
        if !inert {
            press = press.on_press(message);
        }
        let tile = container(press).width(Length::FillPortion(1)).id(id);
        if choose {
            return container(tile)
                .width(Length::FillPortion(1))
                .id("row:wallpaper.choose".to_owned())
                .into();
        }
        tile.into()
    }

    /// The wallpapers that move (#433): whether they move, on battery,
    /// and when they stop by themselves.
    fn animation_group(&self) -> El<'_> {
        let lang = self.lang;
        let head = row![label(w("ANIMATION", "ANIMATION").get(lang))]
            .align_y(Alignment::Center)
            .height(28);
        let controls = self.controls();
        let mut rows = column![];
        for (i, id) in [model::ANIMATE, model::ON_BATTERY].into_iter().enumerate() {
            if let Some(setting) = model::settings().iter().find(|s| s.id == id) {
                rows = rows.push(divided(i > 0, self.row(setting, &controls)));
            }
        }
        let stops = w(
            "Elle s’arrête d’elle-même : une fenêtre en plein écran, le fond couvert à plus de 90 %, l’écran verrouillé ou éteint, l’économie d’énergie, les mouvements réduits, 2 min sans un geste. Elle reste immobile sous la barre.",
            "It stops by itself: a fullscreen window, the background more than 90 % covered, the screen locked or off, the power saver, reduced motion, 2 min without input. It stays still under the bar.",
        );
        let card = column![
            rows,
            divided(
                true,
                container(t(stops.get(lang), 12.0, FAINT.get()))
                    .padding([10, 12])
                    .into()
            ),
        ];
        column![head, card_box(card.into())].spacing(8).into()
    }

    fn lock_group(&self) -> El<'_> {
        let lang = self.lang;
        let file = self.file();
        let inert = !self.writable();
        let head = row![label(w("ÉCRAN DE VERROUILLAGE", "LOCK SCREEN").get(lang))]
            .align_y(Alignment::Center)
            .height(28);
        // The drawing: the source, the veil, the clock.
        let thumb = self.lock_thumb();
        let drawing = stack![
            self.thumb(&thumb, LOCK_SIZE.0, LOCK_SIZE.1),
            container(Space::new().width(LOCK_SIZE.0).height(LOCK_SIZE.1)).style(|_: &Theme| {
                container::Style {
                    background: Some(Background::Color(rgba(5, 7, 12, 0.45))),
                    ..container::Style::default()
                }
            }),
            container(
                column![
                    text("14:02").size(26).font(MONO).color(Color::WHITE),
                    t(
                        w("jeudi 5 novembre", "Thursday 5 November").get(lang),
                        9.0,
                        rgba(238, 241, 246, 0.8)
                    ),
                ]
                .align_x(Alignment::Center)
                .spacing(2)
            )
            .width(LOCK_SIZE.0)
            .padding(Padding::default().top(22))
            .center_x(LOCK_SIZE.0),
        ]
        .width(LOCK_SIZE.0)
        .height(LOCK_SIZE.1);
        let now = file.lock();
        let focused = self.focused_stop(&Stop::Lock);
        let mut radios = column![].spacing(0);
        for (look, name, line, fresh) in [
            (
                LockLook::Dimmed,
                w(
                    "Le fond du bureau, assombri",
                    "The desktop’s background, dimmed",
                ),
                w("Comme aujourd’hui.", "As today."),
                false,
            ),
            (
                LockLook::Blurred,
                w(
                    "Le fond du bureau, flouté",
                    "The desktop’s background, blurred",
                ),
                w(
                    "Le fond seul, jamais les fenêtres.",
                    "The background alone, never the windows.",
                ),
                true,
            ),
            (
                LockLook::Picture,
                w("Une autre image", "Another picture"),
                w(
                    "Choisie pour le verrouillage seulement.",
                    "Chosen for the lock screen only.",
                ),
                true,
            ),
        ] {
            let on = look == now;
            let dot = container(if on {
                El::from(
                    container(Space::new().width(8).height(8)).style(|_: &Theme| {
                        container::Style {
                            background: Some(Background::Color(ACCENT.get())),
                            border: border(4.0, 0.0, Color::TRANSPARENT),
                            ..container::Style::default()
                        }
                    }),
                )
            } else {
                Space::new().width(8).height(8).into()
            })
            .width(18)
            .height(18)
            .center_x(18)
            .center_y(18)
            .style(move |_: &Theme| container::Style {
                border: border(
                    9.0,
                    2.0,
                    if on {
                        ACCENT.get()
                    } else {
                        crate::theme::ink(rgba(255, 255, 255, 0.3))
                    },
                ),
                ..container::Style::default()
            });
            let mut title = row![t(name.get(lang), 14.0, TEXT.get()).font(SEMIBOLD)]
                .spacing(6)
                .align_y(Alignment::Center);
            if fresh {
                title = title.push(
                    container(t(w("Nouveau", "New").get(lang), 10.0, BLUE.get()).font(SEMIBOLD))
                        .padding([1, 5])
                        .style(|_: &Theme| container::Style {
                            background: Some(Background::Color(rgba(143, 179, 255, 0.16))),
                            border: border(4.0, 0.0, Color::TRANSPARENT),
                            ..container::Style::default()
                        }),
                );
            }
            let content = row![
                dot,
                column![title, t(line.get(lang), 12.5, DIM.get())].spacing(1)
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            let ringed = focused && on;
            let mut press =
                button(content)
                    .width(Length::Fill)
                    .padding([6, 8])
                    .style(move |_, _| button::Style {
                        border: if ringed {
                            ring(8.0)
                        } else {
                            border(8.0, 0.0, Color::TRANSPARENT)
                        },
                        ..button::Style::default()
                    });
            if !inert && !on {
                press = press.on_press(Message::Wallpaper(Msg::Lock(look.word())));
            }
            radios = radios.push(container(press).id(format!("lock:{}", look.word())));
        }
        let top = row![
            container(drawing).style(|_: &Theme| container::Style {
                border: border(7.0, 1.0, rgba(255, 255, 255, 0.12)),
                ..container::Style::default()
            }),
            container(radios).width(Length::Fill)
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        let mut card = column![
            container(top)
                .id("row:wallpaper.lock".to_owned())
                .padding(12)
        ];
        if now == LockLook::Picture {
            card = card.push(divided(true, self.lock_picture_row(&file, inert)));
        }
        column![head, card_box(card.into())].spacing(8).into()
    }

    fn lock_picture_row<'a>(&'a self, file: &model::File, inert: bool) -> El<'a> {
        let lang = self.lang;
        let current = file.lock_picture();
        let focused = self.focused_stop(&Stop::LockPicture);
        let mut menu = button(
            row![
                t(model::name_of(&current, lang), 13.5, TEXT.get()).font(SEMIBOLD),
                icon(Icon::Chevron, 12.0, DIM.get()),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([7, 12])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(
                crate::theme::Ink::Token(|t| t.control).get(),
            )),
            text_color: TEXT.get(),
            border: if focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, RIM_12.get())
            },
            ..button::Style::default()
        });
        if !inert {
            menu = menu.on_press(Message::Wallpaper(Msg::LockMenu));
        }
        let head = row![
            column![
                t(
                    w("Image du verrouillage", "Lock screen picture").get(lang),
                    14.0,
                    TEXT.get()
                )
                .font(SEMIBOLD)
            ]
            .width(Length::Fill),
            container(menu).id("control:wallpaper.lock-picture".to_owned()),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        let mut out = column![head].spacing(8);
        if self.wallpaper.lock_menu {
            let mut list = column![].spacing(2);
            for tile in self.wallpaper_tiles() {
                let words: El<'a> = match tile.picture() {
                    Some(picture) => {
                        let thumb = self.tile_thumb(&picture);
                        let on = same(&picture, &current);
                        row![
                            self.thumb(&thumb, 30.0, 19.0),
                            t(
                                model::name_of(&picture, lang),
                                13.0,
                                if on { MINT.get() } else { TEXT.get() }
                            )
                            .font(SEMIBOLD),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .into()
                    }
                    None => t(
                        w("Choisir une image…", "Choose a picture…").get(lang),
                        13.0,
                        MINT.get(),
                    )
                    .font(SEMIBOLD)
                    .into(),
                };
                let id = format!(
                    "lock-option:{}",
                    tile.id(&self.wallpaper.mine).trim_start_matches("tile:")
                );
                list = list.push(
                    container(
                        button(words)
                            .width(Length::Fill)
                            .padding([4, 8])
                            .on_press(Message::Wallpaper(Msg::LockPicture(tile)))
                            .style(|_, status| button::Style {
                                background: matches!(
                                    status,
                                    button::Status::Hovered | button::Status::Pressed
                                )
                                .then(|| Background::Color(HOVER.get())),
                                text_color: TEXT.get(),
                                border: border(6.0, 0.0, Color::TRANSPARENT),
                                ..button::Style::default()
                            }),
                    )
                    .id(id),
                );
            }
            out = out.push(
                container(list)
                    .padding(6)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(MENU.get())),
                        border: border(10.0, 1.0, RIM_12.get()),
                        ..container::Style::default()
                    }),
            );
        }
        container(out)
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .id("row:wallpaper.lock-picture".to_owned())
            .into()
    }

    /// The plain colour's six swatches, a radio group: the window's row
    /// for `[wallpaper] color`.
    pub(super) fn colour_swatches(&self, slot: Slot, inert: bool, focused: bool) -> El<'_> {
        let alpha = if inert { INERT } else { 1.0 };
        let now = self.value(slot);
        let mut swatches = row![].spacing(4).align_y(Alignment::Center);
        for (rgb, _, _) in hocus_link::wallpaper::SWATCHES {
            let word = hocus_link::wallpaper::colour_word(rgb);
            let on = now == Some(Value::Text(word.clone()));
            let fill = Color::from_rgb8(rgb[0], rgb[1], rgb[2]);
            let mut swatch = button(Space::new().width(24).height(24))
                .width(24)
                .height(24)
                .padding(0)
                .style(move |_, _| button::Style {
                    background: Some(Background::Color(fade(fill, alpha))),
                    border: border(12.0, 1.0, rgba(255, 255, 255, 0.16)),
                    ..button::Style::default()
                });
            if !inert && !on {
                swatch = swatch.on_press(Message::Choose(slot, Value::Text(word.clone())));
            }
            // The accent's ring, lit while the group holds the keyboard.
            let ring_colour = if focused {
                crate::theme::Ink::AccentLit.get()
            } else {
                ACCENT.get()
            };
            let ringed = container(swatch)
                .padding(4)
                .style(move |_: &Theme| container::Style {
                    border: if on {
                        border(16.0, 2.0, fade(ring_colour, alpha))
                    } else {
                        border(16.0, 0.0, Color::TRANSPARENT)
                    },
                    ..container::Style::default()
                });
            swatches = swatches.push(container(ringed).id(format!("colour:{word}")));
        }
        swatches.into()
    }

    /// The colour's row: its line says what it fills.
    pub(super) fn colour_line(&self) -> &'static str {
        match self.target_picture() {
            Picture::File(_) => self.say(w("Autour de l’image.", "Around the picture.")),
            _ => self.say(w("Le fond tout entier.", "The whole background.")),
        }
    }

    /// The fit's row: its line says what it does.
    pub(super) fn fit_line(&self) -> &'static str {
        match self.file().fit() {
            Fit::Cover => self.say(w(
                "L’image couvre l’écran ; ce qui dépasse est coupé.",
                "The picture covers the screen; what overflows is cut.",
            )),
            Fit::Contain => self.say(w(
                "L’image entière ; la couleur unie remplit les bords.",
                "The whole picture; the solid colour fills the edges.",
            )),
            Fit::Center => self.say(w(
                "À sa taille, au milieu ; la couleur unie autour.",
                "At its size, in the middle; the solid colour around it.",
            )),
        }
    }

    /// What the page shows, for the log (tests).
    pub(super) fn wallpaper_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let file = self.file();
        let current = self.target_picture();
        let warning = self.wallpaper_warning();
        let tiles: Vec<serde_json::Value> = self
            .wallpaper_tiles()
            .iter()
            .map(|tile| {
                let picture = tile.picture();
                serde_json::json!({
                    "id": tile.id(&self.wallpaper.mine),
                    "chosen": warning.is_none() && picture.as_ref().is_some_and(|p| same(p, &current)),
                    "drawn": picture.is_some_and(|p| self.wallpaper.thumbs.drawn.contains_key(&self.tile_thumb(&p))),
                })
            })
            .collect();
        let screens: Vec<serde_json::Value> = self
            .preview_sizes()
            .into_iter()
            .map(|(s, _)| {
                serde_json::json!({
                    "name": s.name,
                    "picture": file.picture_of(&s.name).word(),
                    "caption": format!("{} · {}", s.title, model::name_of(&file.picture_of(&s.name), lang)),
                })
            })
            .collect();
        let accent = self.wallpaper_accent().base;
        serde_json::json!({
            "per_screen": file.per_screen,
            "target": self.wallpaper_target().map(|s| s.name),
            "picture": current.word(),
            "tiles": tiles,
            "screens": screens,
            "fit_row": matches!(current, Picture::File(_)),
            "colour_row": self.colour_row_shows(),
            "lock": file.lock().word(),
            "lock_picture": file.lock_picture().word(),
            "lock_drawn": self.wallpaper.thumbs.drawn.contains_key(&self.lock_thumb()),
            "lock_menu": self.wallpaper.lock_menu,
            "warning": warning,
            "copied": self.wallpaper.copied,
            "mine": self.wallpaper.mine.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            "accent": format!("{:02x}{:02x}{:02x}", accent[0], accent[1], accent[2]),
        })
    }
}
