//! The Écrans page (6.9, variant A: the arrangement on top): the drawing
//! of the screens, a click choosing one and a drag placing it; the chosen
//! screen's rows, then all screens', then the night light's (#436); the
//! notes for a screen missing; the "Garder ?" dialog of a mode tried.
//!
//! The screens are the compositor's, over its line ([`crate::screens`]).
//! A place, a scale, the primary screen, VRR and a screen turned off are
//! written at once, with the toast's "Annuler"; a resolution or a refresh
//! rate is tried first: the compositor applies it without the file and
//! takes it back by itself after 15 s, unless "Garder" keeps it (then
//! written).

mod drawing;
mod view;

use crate::lang::French as _;
use drawing::*;

use std::time::{Duration, Instant};

use iced::{
    Alignment, Background, Color, Element, Length, Padding, Point, Rectangle, Renderer, Shadow,
    Size, Theme, Vector, mouse,
    widget::{Space, button, canvas, column, container, mouse_area, pin, row, text},
};

use super::{App, Doing, Focus, Message, ToastAction, style::*};
use crate::theme::Ink;
use crate::{
    lang::{Lang, w},
    model::Icon,
    screens::{
        self, ARRANGE, Align, IDENTIFY, MIRROR, Mode3, PRIMARY, RATE, RESOLUTION, ROTATION, Rect,
        SCALE, SCALES, SETS, Said, Screen, Screens, Side, USE, VRR,
    },
    store::{self, KeyEdit},
};

type El<'a> = Element<'a, Message>;

/// The drawing's height, and its margins.
const DRAWING_HEIGHT: f32 = 196.0;
const MARGIN_X: f32 = 24.0;
const MARGIN_Y: f32 = 20.0;
/// At most 0.1 px per logical px.
const MOST_F: f32 = 0.1;
/// A press moves this far before it is a drag.
const DRAG_FROM: f32 = 4.0;
/// A screen dropped slides to its place.
const SLIDE: Duration = Duration::from_millis(200);
/// The numbers shown by "Identifier".
const IDENTIFY_FOR: Duration = Duration::from_secs(3);
/// The bar's widths, logical: full from this wide.
const FULL_FROM: i32 = 1796;
const AMBER: Ink = Ink::Dark(rgb(0xF5B44A));

/// What the page's pieces send.
#[derive(Debug, Clone)]
pub enum Msg {
    Said(Said),
    /// A press on the drawing, over screen `name`, at this point of it.
    Press(String, Point),
    /// The pointer, on the drawing's coordinates, while a press holds.
    Drag(Point),
    /// The pointer, on the window's: whatever is under it (iced gives a
    /// widget the batch's last place for every event of a batch).
    Pointer(Point),
    /// The left button pressed where the pointer is.
    PressAt,
    Release,
    Select(String),
    Toggle(&'static str),
    Menu(&'static str),
    Choose(&'static str, Choice),
    CloseMenu,
    Keep,
    Revert,
    Identify,
    Identified(u64),
    /// A set's row's menu, by the set's name.
    SetMenu(String),
}

/// A menu's choice on this page.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Size((i32, i32)),
    Rate(i32),
    /// `None`: automatic.
    Scale(Option<f64>),
    /// `None`: automatic.
    Primary(Option<String>),
    /// "Oublier cet ensemble": the set, by its name.
    Forget(String),
}

/// A press on a screen of the drawing, until it is let go.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    name: String,
    /// Where it was pressed, and where the pointer is, on the drawing.
    from: Point,
    at: Point,
    /// It moved far enough to be a drag.
    dragging: bool,
}

/// A screen sliding to where it landed.
#[derive(Debug, Clone, PartialEq)]
struct Sliding {
    name: String,
    /// From where it was let go, on the drawing.
    from: Point,
    since: Instant,
}

/// A trial under way, as the compositor tells it.
#[derive(Debug, Clone, PartialEq)]
pub struct Trial {
    id: u64,
    output: String,
    mode: String,
    before: String,
    left: u64,
    /// What it tries: "résolution" or "fréquence".
    what: &'static str,
    /// The focus on "Garder" (else "Revenir").
    on_keep: bool,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct Page {
    line: Option<screens::Line>,
    known: Option<Screens>,
    /// The compositor's line is gone, or never opened.
    unavailable: bool,
    selected: Option<String>,
    held: Option<Held>,
    sliding: Option<Sliding>,
    menu: Option<(&'static str, Instant)>,
    menu_index: usize,
    trial: Option<Trial>,
    /// A trial asked, its answer not come.
    asked: Option<(String, &'static str)>,
    identify: Option<(u64, Instant)>,
    /// The set whose row's menu is open.
    set_menu: Option<String>,
    serial: u64,
    /// Where the pointer is on the window, as its events said it.
    pointer: Option<Point>,
}

/// A control of the page that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Identify,
    Screen(String),
    /// A set of screens' "⋯", by the set's name.
    Set(String),
    Row(&'static str),
}

/// How a screen is drawn.
#[derive(Debug, Clone, PartialEq)]
struct Drawn {
    name: String,
    number: usize,
    rect: Rectangle,
    selected: bool,
    /// Dashed, with this word: "Éteint", "Couvercle fermé", "Débranché".
    dashed: Option<String>,
    title: String,
    line: String,
    bar: Option<Rectangle>,
    lifted: bool,
}

impl App {
    /// The compositor's line, opened once.
    pub(super) fn screens_open(
        &mut self,
        to_window: iced::futures::channel::mpsc::UnboundedSender<Message>,
    ) {
        let line = screens::open(move |said| {
            to_window
                .unbounded_send(Message::Screens(Msg::Said(said)))
                .is_ok()
        });
        self.screens.line = Some(line);
    }

    /// `line` to the compositor's screens' line; whether it took it.
    pub(super) fn screens_ask(&self, line: String) -> bool {
        self.screens.line.as_ref().is_some_and(|l| l.ask(line))
    }

    pub(super) fn screens_list(&self) -> Option<&Screens> {
        self.screens.known.as_ref()
    }

    /// The screen whose rows show: the one chosen, else the first plugged.
    fn chosen(&self) -> Option<&Screen> {
        let known = self.screens_list()?;
        self.screens
            .selected
            .as_deref()
            .and_then(|n| known.get(n))
            .filter(|s| s.connected)
            .or_else(|| known.plugged().next())
    }

    pub(super) fn screens_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Said(said) => self.screens_said(said),
            Msg::Press(name, at) => {
                if self.chosen_inert_all() {
                    return iced::Task::none();
                }
                let pluggedin = self
                    .screens_list()
                    .and_then(|k| k.get(&name))
                    .is_some_and(|s| s.connected);
                if pluggedin {
                    self.screens.menu = None;
                    self.screens.held = Some(Held {
                        name,
                        from: at,
                        at,
                        dragging: false,
                    });
                }
                iced::Task::none()
            }
            Msg::Drag(at) => {
                if let Some(held) = &mut self.screens.held {
                    held.at = at;
                    if !held.dragging && (at.x - held.from.x).hypot(at.y - held.from.y) >= DRAG_FROM
                    {
                        held.dragging = true;
                        self.screens.selected = Some(held.name.clone());
                    }
                }
                iced::Task::none()
            }
            Msg::Pointer(at) => {
                self.screens.pointer = Some(at);
                if self.screens.held.is_none() {
                    return iced::Task::none();
                }
                let Some(drawing) = self.places.at.get("drawing") else {
                    return iced::Task::none();
                };
                let at = Point::new(at.x - drawing.x, at.y - drawing.y);
                self.screens_update(Msg::Drag(at))
            }
            Msg::PressAt => {
                // Over the dialog or a menu, the press is theirs.
                if self.screens.trial.is_some()
                    || self.screens.menu.is_some()
                    || self.menu.is_some()
                {
                    return iced::Task::none();
                }
                let (Some(pointer), Some(drawing)) =
                    (self.screens.pointer, self.places.at.get("drawing").copied())
                else {
                    return iced::Task::none();
                };
                if !drawing.contains(pointer) {
                    return iced::Task::none();
                }
                let at = Point::new(pointer.x - drawing.x, pointer.y - drawing.y);
                // The one on top: the last drawn.
                let hit = self.drawn().into_iter().rev().find(|d| d.rect.contains(at));
                match hit {
                    Some(hit) => self.screens_update(Msg::Press(hit.name, at)),
                    None => iced::Task::none(),
                }
            }
            Msg::Release => {
                let Some(held) = self.screens.held.take() else {
                    return iced::Task::none();
                };
                self.screens.selected = Some(held.name.clone());
                if held.dragging {
                    return self.dropped(&held);
                }
                iced::Task::none()
            }
            Msg::Select(name) => {
                self.screens.selected = Some(name);
                iced::Task::none()
            }
            Msg::Toggle(row) => self.screens_toggle(row),
            Msg::Menu(row) => {
                if self.screens_row_inert(row) {
                    return iced::Task::none();
                }
                self.screens.menu = match self.screens.menu {
                    Some((open, _)) if open == row => None,
                    _ => Some((row, Instant::now())),
                };
                let current = self.screens_current(row);
                self.screens.menu_index = self
                    .screens_choices(row)
                    .iter()
                    .position(|(c, _)| Some(c) == current.as_ref())
                    .unwrap_or(0);
                iced::Task::none()
            }
            Msg::CloseMenu => {
                self.screens.menu = None;
                iced::Task::none()
            }
            Msg::Choose(_, Choice::Forget(name)) => {
                self.screens.menu = None;
                self.forget_set(&name)
            }
            Msg::Choose(row, choice) => {
                self.screens.menu = None;
                self.screens_choose(row, choice)
            }
            Msg::SetMenu(name) => {
                let open = self.screens.menu.is_some_and(|(r, _)| r == SETS)
                    && self.screens.set_menu.as_deref() == Some(name.as_str());
                if open {
                    self.screens.menu = None;
                } else {
                    self.screens.menu = Some((SETS, Instant::now()));
                    self.screens.set_menu = Some(name);
                    self.screens.menu_index = 0;
                }
                iced::Task::none()
            }
            Msg::Keep => {
                if let Some(trial) = &self.screens.trial
                    && let Some(line) = &self.screens.line
                {
                    line.ask(format!("keep {}", trial.id));
                }
                iced::Task::none()
            }
            Msg::Revert => {
                if let Some(trial) = &self.screens.trial
                    && let Some(line) = &self.screens.line
                {
                    line.ask(format!("revert {}", trial.id));
                }
                iced::Task::none()
            }
            Msg::Identify => {
                self.screens.serial += 1;
                let serial = self.screens.serial;
                self.screens.identify = Some((serial, Instant::now()));
                iced::Task::perform(tokio::time::sleep(IDENTIFY_FOR), move |()| {
                    Message::Screens(Msg::Identified(serial))
                })
            }
            Msg::Identified(serial) => {
                if self.screens.identify.is_some_and(|(s, _)| s == serial) {
                    self.screens.identify = None;
                }
                iced::Task::none()
            }
        }
    }

    fn screens_said(&mut self, said: Said) -> iced::Task<Message> {
        // The desktops' (6.14).
        if self.desktops_said(&said) || self.agents_heard(&said) {
            return iced::Task::none();
        }
        // The clipboard's history and the recording (6.17).
        if self.captures_said(&said) {
            return iced::Task::none();
        }
        match said {
            Said::Screens(known) => {
                self.screens.unavailable = false;
                if let Some(name) = &self.screens.selected
                    && !known.get(name).is_some_and(|s| s.connected)
                {
                    self.screens.selected = None;
                }
                self.screens.known = Some(known);
                iced::Task::none()
            }
            Said::Trial {
                id,
                output,
                mode,
                before,
                left,
            } => {
                let what = match (&self.screens.trial, &self.screens.asked) {
                    (Some(t), _) if t.id == id => t.what,
                    (_, Some((_, what))) => what,
                    _ => RESOLUTION,
                };
                let on_keep = self
                    .screens
                    .trial
                    .as_ref()
                    .filter(|t| t.id == id)
                    .is_none_or(|t| t.on_keep);
                self.screens.asked = None;
                self.screens.menu = None;
                self.screens.trial = Some(Trial {
                    id,
                    output,
                    mode,
                    before,
                    left,
                    what,
                    on_keep,
                });
                iced::Task::none()
            }
            Said::Ended { id, output, kept } => {
                let Some(trial) = self.screens.trial.take().filter(|t| t.id == id) else {
                    return iced::Task::none();
                };
                if !kept {
                    tracing::info!(output, "the mode tried is taken back");
                    return iced::Task::none();
                }
                self.kept(&trial)
            }
            Said::Refused(why) => {
                self.screens.asked = None;
                let text = format!("{} {why}", self.say(w("Refusé :", "Refused:")));
                self.toast(text, ToastAction::None, super::TOAST)
            }
            Said::Gone => {
                self.screens.unavailable = true;
                self.screens.trial = None;
                self.screens.asked = None;
                iced::Task::none()
            }
            Said::Desktops { .. }
            | Said::DesktopRemoved { .. }
            | Said::DesktopRestored { .. }
            | Said::Apps(_)
            | Said::AgentTools(_)
            | Said::Clipboard { .. }
            | Said::Recording(_) => iced::Task::none(),
        }
    }

    /// The mode tried, kept: written, with its undo.
    fn kept(&mut self, trial: &Trial) -> iced::Task<Message> {
        let Some(screen) = self
            .screens_list()
            .and_then(|k| k.get(&trial.output))
            .cloned()
        else {
            return iced::Task::none();
        };
        let lang = self.lang;
        let mut edits = vec![KeyEdit {
            key: self.file_key(&screen.name, "mode"),
            value: Some(format!("\"{}\"", trial.mode)),
        }];
        let size = mode_size(&trial.mode).map(|(w, h)| logical((w, h), screen.scale));
        if let Some(size) = size {
            edits.extend(self.relaid(&screen.name, size));
        }
        let now = say_mode_text(&trial.mode, lang);
        let then = say_mode_text(&trial.before, lang);
        let title = screens::lower_first(&screen.title(lang));
        let said = super::KeysSaid {
            done: match lang {
                Lang::French => format!("Gardé : {now} sur l’{title}"),
                Lang::English => format!("Kept: {now} on the {title}"),
            },
            undone: match lang {
                Lang::French => format!("Annulé : l’{title} revient à {then}."),
                Lang::English => format!("Undone: the {title} is back to {then}."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {now} sur l’{title}."),
                Lang::English => format!("Redone: {now} on the {title}."),
            },
            row: if trial.what == RATE { RATE } else { RESOLUTION },
        };
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    /// Whether the whole page is inert: the file does not read, or the
    /// compositor says nothing.
    fn chosen_inert_all(&self) -> bool {
        !self.writable() || self.screens.unavailable || self.screens.known.is_none()
    }

    /// Whether `row` of the chosen screen does not apply now.
    pub(super) fn screens_row_inert(&self, row: &str) -> bool {
        if self.chosen_inert_all() || self.screens.trial.is_some() {
            return true;
        }
        let Some(known) = self.screens_list() else {
            return true;
        };
        let Some(screen) = self.chosen() else {
            return true;
        };
        let on = known.on().count();
        let lid = screen.lid_closed && on > 0;
        match row {
            USE => lid || (screen.enabled && on <= 1),
            RESOLUTION | SCALE => !screen.enabled || lid,
            RATE => {
                !screen.enabled
                    || lid
                    || screen
                        .mode
                        .is_none_or(|m| screen.rates((m.0, m.1)).len() <= 1)
            }
            VRR => !screen.enabled || lid || !screen.vrr_capable,
            PRIMARY => false,
            _ => true,
        }
    }

    // --- Changes ------------------------------------------------------------------------

    fn screens_toggle(&mut self, row: &'static str) -> iced::Task<Message> {
        if self.screens_row_inert(row) {
            return iced::Task::none();
        }
        let Some(screen) = self.chosen().cloned() else {
            return iced::Task::none();
        };
        let lang = self.lang;
        let title = screen.title(lang);
        let lower = screens::lower_first(&title);
        self.knobs.insert(row, Instant::now());
        let (edits, done, undone) = match row {
            USE => {
                let off = screen.enabled;
                let edit = KeyEdit {
                    key: self.file_key(&screen.name, "enabled"),
                    value: off.then(|| "false".to_owned()),
                };
                let done = match (off, lang) {
                    (true, Lang::French) => {
                        format!("L’{lower} éteint ; ses fenêtres passent sur l’autre")
                    }
                    (true, Lang::English) => {
                        format!("The {lower} off; its windows go to the other")
                    }
                    (false, Lang::French) => format!("L’{lower} rallumé"),
                    (false, Lang::English) => format!("The {lower} on again"),
                };
                let undone = match (off, lang) {
                    (true, Lang::French) => format!("Annulé : l’{lower} se rallume."),
                    (true, Lang::English) => format!("Undone: the {lower} is on again."),
                    (false, Lang::French) => format!("Annulé : l’{lower} s’éteint."),
                    (false, Lang::English) => format!("Undone: the {lower} is off again."),
                };
                (vec![edit], done, undone)
            }
            VRR => {
                let on = !screen.vrr;
                let edit = KeyEdit {
                    key: self.file_key(&screen.name, "vrr"),
                    value: on.then(|| "true".to_owned()),
                };
                let yes = |b: bool| match (b, lang) {
                    (true, Lang::French) => "oui",
                    (false, Lang::French) => "non",
                    (true, Lang::English) => "on",
                    (false, Lang::English) => "off",
                };
                let done = match lang {
                    Lang::French => format!("Fréquence variable de l’{lower} : {}", yes(on)),
                    Lang::English => format!("Variable refresh rate of the {lower}: {}", yes(on)),
                };
                let undone = match lang {
                    Lang::French => {
                        format!("Annulé : fréquence variable de l’{lower} : {}.", yes(!on))
                    }
                    Lang::English => format!(
                        "Undone: variable refresh rate of the {lower}: {}.",
                        yes(!on)
                    ),
                };
                (vec![edit], done, undone)
            }
            _ => return iced::Task::none(),
        };
        let said = super::KeysSaid {
            redone: redone_of(&done, lang),
            done,
            undone,
            row,
        };
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    fn screens_choose(&mut self, row: &'static str, choice: Choice) -> iced::Task<Message> {
        if self.screens_row_inert(row) {
            return iced::Task::none();
        }
        if self.screens_current(row).as_ref() == Some(&choice) {
            return iced::Task::none();
        }
        let Some(screen) = self.chosen().cloned() else {
            return iced::Task::none();
        };
        let lang = self.lang;
        let lower = screens::lower_first(&screen.title(lang));
        match choice {
            Choice::Size(size) => {
                // The rate kept when the new size has it, else its highest.
                let rate = screen.mode.map_or(60_000, |m| m.2);
                let rates = screen.rates(size);
                let rate = if rates.contains(&rate) {
                    rate
                } else {
                    rates.first().copied().unwrap_or(rate)
                };
                self.try_mode(&screen, (size.0, size.1, rate), RESOLUTION)
            }
            Choice::Rate(rate) => {
                let Some(mode) = screen.mode else {
                    return iced::Task::none();
                };
                self.try_mode(&screen, (mode.0, mode.1, rate), RATE)
            }
            Choice::Scale(scale) => {
                let applies = scale.unwrap_or(screen.auto_scale);
                let mut edits = vec![KeyEdit {
                    key: self.file_key(&screen.name, "scale"),
                    value: scale.map(toml_float),
                }];
                if let Some(mode) = screen.mode {
                    edits.extend(self.relaid(&screen.name, logical((mode.0, mode.1), applies)));
                }
                let said_now = self.say_scale_choice(scale, &screen);
                let said_then =
                    self.say_scale_choice((!screen.scale_auto).then_some(screen.scale), &screen);
                let done = match lang {
                    Lang::French => format!("Échelle de l’{lower} : {said_now}"),
                    Lang::English => format!("Scale of the {lower}: {said_now}"),
                };
                let said = super::KeysSaid {
                    redone: redone_of(&done, lang),
                    undone: match lang {
                        Lang::French => {
                            format!("Annulé : l’échelle de l’{lower} revient à {said_then}.")
                        }
                        Lang::English => {
                            format!("Undone: the {lower}'s scale is back to {said_then}.")
                        }
                    },
                    done,
                    row: SCALE,
                };
                self.ask_keys(edits, said, Doing::New, None);
                iced::Task::none()
            }
            Choice::Forget(_) => iced::Task::none(),
            Choice::Primary(name) => {
                let said_now = self.say_primary(name.as_deref());
                let said_then = self.say_primary(self.primary_named().as_deref());
                // A set of several screens keeps its own (windows.md §12),
                // by identity.
                let known = self.screens_list();
                let edits = match known.and_then(|k| Some((k, k.set_in_use()?))) {
                    Some((known, set)) => vec![KeyEdit {
                        key: vec![store::SCREEN_SETS.to_owned(), set, "primary".to_owned()],
                        value: name
                            .and_then(|n| known.get(&n).map(|s| s.key.clone()))
                            .map(|k| toml_edit::Value::from(k).to_string()),
                    }],
                    None => vec![KeyEdit {
                        key: vec!["panel".to_owned(), "output".to_owned()],
                        value: name.map(|n| toml_edit::Value::from(n).to_string()),
                    }],
                };
                let done = match lang {
                    Lang::French => format!("Écran principal : {said_now}"),
                    Lang::English => format!("Primary screen: {said_now}"),
                };
                let said = super::KeysSaid {
                    redone: redone_of(&done, lang),
                    undone: match lang {
                        Lang::French => {
                            format!("Annulé : l’écran principal revient à {said_then}.")
                        }
                        Lang::English => {
                            format!("Undone: the primary screen is back to {said_then}.")
                        }
                    },
                    done,
                    row: PRIMARY,
                };
                self.ask_keys(edits, said, Doing::New, None);
                iced::Task::none()
            }
        }
    }

    /// A mode tried: the compositor applies it and counts 15 s.
    fn try_mode(
        &mut self,
        screen: &Screen,
        mode: Mode3,
        what: &'static str,
    ) -> iced::Task<Message> {
        if screen.mode == Some(mode) {
            return iced::Task::none();
        }
        let Some(line) = &self.screens.line else {
            return iced::Task::none();
        };
        let asked = format!("try {} mode={}", screen.name, screens::mode_word(mode));
        if line.ask(asked) {
            self.screens.asked = Some((screen.name.clone(), what));
        }
        iced::Task::none()
    }

    /// The other keys a size change of `name` writes: the places, when the
    /// file holds them, so that the others keep their side and alignment.
    fn relaid(&self, name: &str, size: (i32, i32)) -> Vec<KeyEdit> {
        let Some(known) = self.screens_list() else {
            return Vec::new();
        };
        if !self.file_places() {
            return Vec::new();
        }
        let Some(moving) = known.get(name) else {
            return Vec::new();
        };
        let Some(anchor) = known
            .plugged()
            .filter(|s| s.name != name)
            .min_by_key(|s| distance(s.rect(), moving.rect()))
        else {
            return Vec::new();
        };
        let side = screens::side_of(moving.rect(), anchor.rect());
        let align = screens::align_of(moving.rect(), anchor.rect(), side);
        let at = screens::placed(size, anchor.rect(), side, align);
        let places: Vec<(String, (i32, i32))> = known
            .plugged()
            .map(|s| (s.name.clone(), if s.name == name { at } else { s.position }))
            .collect();
        self.position_edits(&screens::normalised(&places))
    }

    /// Whether the file places the screens (every screen plugged has a
    /// `position`, in the set in use's table when it has several).
    fn file_places(&self) -> bool {
        let Some(known) = self.screens_list() else {
            return false;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return false;
        };
        if let Some(set) = known.set_in_use() {
            let placed = snapshot.screen_set(&set).map(|s| s.placed.clone());
            return placed.is_some_and(|placed| known.set.iter().all(|k| placed.contains(k)));
        }
        let names: Vec<&str> = known.plugged().map(|s| s.name.as_str()).collect();
        !names.is_empty()
            && names
                .iter()
                .all(|n| snapshot.screens_placed.iter().any(|p| p == n))
    }

    /// The primary screen as the file names it: the set in use's when it
    /// has several screens (by identity), else `[panel] output`.
    fn primary_named(&self) -> Option<String> {
        let snapshot = self.snapshot.as_ref()?;
        if let Some(known) = self.screens_list()
            && let Some(set) = known.set_in_use()
        {
            let key = snapshot.screen_set(&set)?.primary.clone()?;
            return known
                .list
                .iter()
                .find(|s| s.key == key && s.connected)
                .map(|s| s.name.clone());
        }
        snapshot.primary_output.clone().filter(|n| !n.is_empty())
    }

    /// The key screen `name`'s `what` is written at: the set in use's
    /// table when it has several screens, else `[outputs.NAME]`.
    fn file_key(&self, name: &str, what: &str) -> Vec<String> {
        match self.screens_list() {
            Some(known) => known.key_of(name, what),
            None => vec!["outputs".to_owned(), name.to_owned(), what.to_owned()],
        }
    }

    /// Every place, as edits.
    fn position_edits(&self, places: &[(String, (i32, i32))]) -> Vec<KeyEdit> {
        places
            .iter()
            .map(|(name, (x, y))| KeyEdit {
                key: self.file_key(name, "position"),
                value: Some(format!("[{x}, {y}]")),
            })
            .collect()
    }

    /// "Oublier cet ensemble": the compositor forgets it, and its table
    /// leaves the file (with "Annuler" then).
    fn forget_set(&mut self, name: &str) -> iced::Task<Message> {
        let Some(set) = self
            .screens_list()
            .and_then(|k| k.sets.iter().find(|s| s.name == name))
            .cloned()
        else {
            return iced::Task::none();
        };
        if set.current {
            return iced::Task::none();
        }
        if let Some(line) = &self.screens.line {
            let screens: String = set.name.split('\n').map(|k| format!("\t{k}")).collect();
            line.ask(format!("forget-set{screens}"));
        }
        let lang = self.lang;
        let title = set.title(lang);
        if !set.in_file {
            let text = match lang {
                Lang::French => format!("Ensemble oublié : {title}"),
                Lang::English => format!("Set forgotten: {title}"),
            };
            return self.toast(text, ToastAction::None, super::UNDONE);
        }
        let done = match lang {
            Lang::French => format!("Ensemble oublié : {title}"),
            Lang::English => format!("Set forgotten: {title}"),
        };
        let said = super::KeysSaid {
            redone: redone_of(&done, lang),
            undone: match lang {
                Lang::French => format!("Annulé : la disposition de « {title} » est revenue."),
                Lang::English => format!("Undone: the arrangement of “{title}” is back."),
            },
            done,
            row: SETS,
        };
        let edits = vec![KeyEdit {
            key: vec![store::SCREEN_SETS.to_owned(), set.name.clone()],
            value: None,
        }];
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    /// A screen dropped: where it lands, written with the other's place.
    fn dropped(&mut self, held: &Held) -> iced::Task<Message> {
        let Some((moving, other, landing)) = self.landing(held) else {
            return iced::Task::none();
        };
        let lang = self.lang;
        if let Some(view) = self.view_of() {
            let from = Point::new(
                held.at.x - (held.from.x - view.rect_of(moving.rect()).x),
                held.at.y - (held.from.y - view.rect_of(moving.rect()).y),
            );
            self.screens.sliding = Some(Sliding {
                name: moving.name.clone(),
                from,
                since: Instant::now(),
            });
        }
        if landing == moving.position {
            let text = self
                .say(w(
                    "Lâché à sa place : rien ne change.",
                    "Dropped where it was: nothing changes.",
                ))
                .to_owned();
            return self.toast(text, ToastAction::None, super::UNDONE);
        }
        let landed = Rect {
            x: landing.0,
            y: landing.1,
            ..moving.rect()
        };
        let side = screens::side_of(landed, other.rect());
        let align = screens::align_of(landed, other.rect(), side);
        let place = screens::say_place(side, align, &other.title(lang), lang);
        let Some(known) = self.screens_list() else {
            return iced::Task::none();
        };
        let places: Vec<(String, (i32, i32))> = known
            .plugged()
            .map(|s| {
                (
                    s.name.clone(),
                    if s.name == moving.name {
                        landing
                    } else {
                        s.position
                    },
                )
            })
            .collect();
        let edits = self.position_edits(&screens::normalised(&places));
        let done = format!("{} {place}", moving.title(lang));
        let said = super::KeysSaid {
            redone: redone_of(&done, lang),
            undone: self
                .say(w(
                    "Annulé : les écrans reprennent leur place.",
                    "Undone: the screens are back in their places.",
                ))
                .to_owned(),
            done,
            row: ARRANGE,
        };
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    /// The screen held, the other, and where it would land now.
    fn landing(&self, held: &Held) -> Option<(Screen, Screen, (i32, i32))> {
        let known = self.screens_list()?;
        let moving = known.get(&held.name)?.clone();
        let other = known
            .plugged()
            .filter(|s| s.name != moving.name)
            .min_by_key(|s| distance(s.rect(), moving.rect()))?
            .clone();
        let view = self.view_of()?;
        let at = view.rect_of(moving.rect());
        let x = at.x + held.at.x - held.from.x;
        let y = at.y + held.at.y - held.from.y;
        let (lx, ly) = view.logical(Point::new(x, y));
        let dropped = Rect {
            x: lx,
            y: ly,
            ..moving.rect()
        };
        let landing = screens::land(dropped, other.rect());
        Some((moving, other, landing))
    }

    /// Arrows on a screen: put on that side of the other, tops or lefts
    /// aligned, written as a drag is.
    fn arrow(&mut self, name: &str, side: Side) -> iced::Task<Message> {
        if self.chosen_inert_all() {
            return iced::Task::none();
        }
        let Some(known) = self.screens_list() else {
            return iced::Task::none();
        };
        let Some(moving) = known.get(name).cloned() else {
            return iced::Task::none();
        };
        let Some(other) = known
            .plugged()
            .filter(|s| s.name != name)
            .min_by_key(|s| distance(s.rect(), moving.rect()))
            .cloned()
        else {
            return iced::Task::none();
        };
        let landing = screens::placed(moving.size, other.rect(), side, Align::Start);
        if landing == moving.position {
            return iced::Task::none();
        }
        let lang = self.lang;
        let place = screens::say_place(side, Align::Start, &other.title(lang), lang);
        let places: Vec<(String, (i32, i32))> = known
            .plugged()
            .map(|s| {
                (
                    s.name.clone(),
                    if s.name == name { landing } else { s.position },
                )
            })
            .collect();
        let done = format!("{} {place}", moving.title(lang));
        let said = super::KeysSaid {
            redone: redone_of(&done, lang),
            undone: self
                .say(w(
                    "Annulé : les écrans reprennent leur place.",
                    "Undone: the screens are back in their places.",
                ))
                .to_owned(),
            done,
            row: ARRANGE,
        };
        self.ask_keys(
            self.position_edits(&screens::normalised(&places)),
            said,
            Doing::New,
            None,
        );
        iced::Task::none()
    }

    // --- Menus --------------------------------------------------------------------------

    /// A menu's choices, with their labels.
    fn screens_choices(&self, row: &str) -> Vec<(Choice, String)> {
        let lang = self.lang;
        if row == SETS {
            return self
                .screens
                .set_menu
                .clone()
                .map(|name| {
                    vec![(
                        Choice::Forget(name),
                        self.say(w("Oublier cet ensemble", "Forget this set"))
                            .to_owned(),
                    )]
                })
                .unwrap_or_default();
        }
        let Some(screen) = self.chosen() else {
            return Vec::new();
        };
        match row {
            RESOLUTION => {
                let native = screen.native();
                screen
                    .sizes()
                    .into_iter()
                    .map(|size| {
                        let mut label = screens::say_size(size);
                        if Some(size) == native {
                            label.push_str(" (native)");
                        }
                        (Choice::Size(size), label)
                    })
                    .collect()
            }
            RATE => screen
                .mode
                .map(|m| screen.rates((m.0, m.1)))
                .unwrap_or_default()
                .into_iter()
                .map(|r| (Choice::Rate(r), screens::say_rate(r, lang)))
                .collect(),
            SCALE => {
                let mut list = vec![(Choice::Scale(None), self.say_scale_choice(None, screen))];
                for scale in SCALES {
                    list.push((Choice::Scale(Some(scale)), screens::say_scale(scale)));
                }
                list
            }
            PRIMARY => {
                let mut list = vec![(Choice::Primary(None), self.say_primary(None))];
                if let Some(known) = self.screens_list() {
                    for s in &known.list {
                        let mut label = s.title(lang);
                        if !s.connected {
                            label.push_str(self.say(w(" (débranché)", " (unplugged)")));
                        }
                        list.push((Choice::Primary(Some(s.name.clone())), label));
                    }
                }
                list
            }
            _ => Vec::new(),
        }
    }

    /// The choice a menu shows now.
    fn screens_current(&self, row: &str) -> Option<Choice> {
        let screen = self.chosen()?;
        match row {
            RESOLUTION => screen.mode.map(|m| Choice::Size((m.0, m.1))),
            RATE => screen.mode.map(|m| Choice::Rate(m.2)),
            SCALE => Some(Choice::Scale((!screen.scale_auto).then_some(screen.scale))),
            PRIMARY => Some(Choice::Primary(self.primary_named())),
            _ => None,
        }
    }

    fn say_scale_choice(&self, scale: Option<f64>, screen: &Screen) -> String {
        match scale {
            Some(scale) => screens::say_scale(scale),
            None => match self.lang {
                Lang::French => format!("Automatique ({})", screens::say_scale(screen.auto_scale)),
                Lang::English => format!("Automatic ({})", screens::say_scale(screen.auto_scale)),
            },
        }
    }

    fn say_primary(&self, name: Option<&str>) -> String {
        match name {
            None => self.say(w("Automatique", "Automatic")).to_owned(),
            Some(name) => self
                .screens_list()
                .and_then(|k| k.get(name))
                .map_or_else(|| name.to_owned(), |s| s.title(self.lang)),
        }
    }

    // --- The keyboard -------------------------------------------------------------------

    /// The page's controls, in the keyboard's order.
    pub(super) fn screens_controls(&self) -> Vec<Control> {
        let mut out = vec![Control::Identify];
        if let Some(known) = self.screens_list() {
            out.extend(known.plugged().map(|s| Control::Screen(s.name.clone())));
            if known.sets.len() >= 2 {
                out.extend(
                    known
                        .sets
                        .iter()
                        .filter(|s| !s.current)
                        .map(|s| Control::Set(s.name.clone())),
                );
            }
        }
        out.extend([USE, RESOLUTION, RATE, SCALE, VRR, PRIMARY].map(Control::Row));
        out
    }

    /// Whether the page takes every key first ([`App::screens_key_first`]).
    pub(super) fn screens_take_keys(&self) -> bool {
        self.screens.trial.is_some() || self.screens.menu.is_some()
    }

    /// A key the page takes first: the dialog's, a menu's. Whether it did.
    pub(super) fn screens_key_first(
        &mut self,
        key: &iced::keyboard::Key,
    ) -> Option<iced::Task<Message>> {
        use iced::keyboard::{Key, key::Named};
        if let Some(trial) = &mut self.screens.trial {
            return Some(match key.as_ref() {
                Key::Named(Named::Escape) => self.screens_update(Msg::Revert),
                Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                    trial.on_keep = !trial.on_keep;
                    iced::Task::none()
                }
                Key::Named(Named::Enter | Named::Space) => {
                    let keep = trial.on_keep;
                    self.screens_update(if keep { Msg::Keep } else { Msg::Revert })
                }
                _ => iced::Task::none(),
            });
        }
        let (row, _) = self.screens.menu?;
        let choices = self.screens_choices(row);
        Some(match key.as_ref() {
            Key::Named(Named::Escape) => {
                self.screens.menu = None;
                iced::Task::none()
            }
            Key::Named(Named::ArrowDown) => {
                self.screens.menu_index =
                    (self.screens.menu_index + 1).min(choices.len().saturating_sub(1));
                iced::Task::none()
            }
            Key::Named(Named::ArrowUp) => {
                self.screens.menu_index = self.screens.menu_index.saturating_sub(1);
                iced::Task::none()
            }
            Key::Named(Named::Enter | Named::Space) => match choices.get(self.screens.menu_index) {
                Some((choice, _)) => self.screens_update(Msg::Choose(row, choice.clone())),
                None => iced::Task::none(),
            },
            _ => iced::Task::none(),
        })
    }

    /// A key on one of the page's controls.
    pub(super) fn screens_control_key(
        &mut self,
        control: &Control,
        named: iced::keyboard::key::Named,
        alt: bool,
    ) -> iced::Task<Message> {
        use iced::keyboard::key::Named;
        match (control, named) {
            (Control::Identify, Named::Enter | Named::Space) => self.screens_update(Msg::Identify),
            (Control::Screen(name), Named::Enter | Named::Space) => {
                self.screens_update(Msg::Select(name.clone()))
            }
            (Control::Screen(name), arrow) => {
                let side = match arrow {
                    Named::ArrowLeft => Side::Left,
                    Named::ArrowRight => Side::Right,
                    Named::ArrowUp => Side::Above,
                    Named::ArrowDown => Side::Below,
                    _ => return iced::Task::none(),
                };
                let name = name.clone();
                self.screens.selected = Some(name.clone());
                self.arrow(&name, side)
            }
            (Control::Set(name), Named::Enter | Named::Space) => {
                self.screens_update(Msg::SetMenu(name.clone()))
            }
            (Control::Set(name), Named::ArrowDown) if alt => {
                self.screens_update(Msg::SetMenu(name.clone()))
            }
            (Control::Row(row @ (USE | VRR)), Named::Enter | Named::Space) => {
                self.screens_update(Msg::Toggle(row))
            }
            (Control::Row(row), Named::Enter | Named::Space) => self.screens_update(Msg::Menu(row)),
            (Control::Row(row), Named::ArrowDown) if alt => self.screens_update(Msg::Menu(row)),
            _ => iced::Task::none(),
        }
    }

    fn focused(&self, control: &Control) -> bool {
        match self.focus {
            Focus::Control(i) => self
                .controls()
                .get(i)
                .is_some_and(|c| matches!(c, super::Control::Screens(c) if c == control)),
            _ => false,
        }
    }

    /// Something of the page moves at `now`: frames are needed.
    pub(super) fn screens_moving(&self, now: Instant) -> bool {
        if self.reduced_motion {
            return false;
        }
        self.screens
            .sliding
            .as_ref()
            .is_some_and(|s| now.saturating_duration_since(s.since) < SLIDE)
            || self
                .screens
                .menu
                .is_some_and(|(_, since)| now.saturating_duration_since(since) < super::MENU_IN)
    }

    /// Whether the page shows: the pointer and the left button are
    /// followed over the whole window, for the drawing.
    pub(super) fn screens_shown_now(&self) -> bool {
        self.page == super::Page::Section(crate::model::SectionId::Screens)
    }

    /// Whether a trial waits for its answer: the header says "À confirmer".
    pub(super) fn screens_to_confirm(&self) -> bool {
        self.screens.trial.is_some()
    }

    // --- What it draws ------------------------------------------------------------------
}

/// The trial's length, as the compositor counts it.
const TRIAL_SECONDS: u64 = 15;

/// `1.25` as TOML: a float that says no more than it is.
fn toml_float(scale: f64) -> String {
    let text = format!("{scale}");
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

/// A size in logical pixels at `scale`.
fn logical(size: (i32, i32), scale: f64) -> (i32, i32) {
    (
        (f64::from(size.0) / scale).round() as i32,
        (f64::from(size.1) / scale).round() as i32,
    )
}

fn mode_size(mode: &str) -> Option<(i32, i32)> {
    let size = mode.split('@').next()?;
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `2560 × 1080 à 60 Hz`.
fn say_mode_text(mode: &str, lang: Lang) -> String {
    let Some(size) = mode_size(mode) else {
        return mode.to_owned();
    };
    let mhz = mode
        .split_once('@')
        .and_then(|(_, r)| r.parse::<f64>().ok())
        .map_or(60_000, |hz| (hz * 1000.0).round() as i32);
    say_mode3((size.0, size.1, mhz), lang)
}

fn say_mode3(m: Mode3, lang: Lang) -> String {
    let at = if lang.fr() { "à" } else { "at" };
    format!(
        "{} {at} {}",
        screens::say_size((m.0, m.1)),
        screens::say_rate(m.2, lang)
    )
}

fn redone_of(done: &str, lang: Lang) -> String {
    match lang {
        Lang::French => format!("Rétabli : {done}."),
        Lang::English => format!("Redone: {done}."),
    }
}

/// How far two rectangles' centres are, squared.
fn distance(a: Rect, b: Rect) -> i64 {
    let dx = i64::from(2 * a.x + a.w - 2 * b.x - b.w);
    let dy = i64::from(2 * a.y + a.h - 2 * b.y - b.h);
    dx * dx + dy * dy
}

/// `13:12`, in the user's time zone.
fn local_time(at: i64) -> Option<String> {
    let stamp = jiff::Timestamp::from_second(at).ok()?;
    Some(
        stamp
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%H:%M")
            .to_string(),
    )
}

// --- Pieces, as the other pages draw them -----------------------------------------------

fn t<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> text::Text<'a> {
    text(content).size(size).color(color).font(MANROPE)
}

fn icon<'a>(which: Icon, size: f32, color: Color) -> El<'a> {
    iced::widget::svg(super::style::icon(which, color, 1.8))
        .width(size)
        .height(size)
        .into()
}

fn label<'a>(words: &'static str) -> El<'a> {
    text(words).size(11.5).font(BOLD).color(DIM).into()
}

fn label_owned<'a>(words: String) -> El<'a> {
    text(words).size(11.5).font(BOLD).color(DIM).into()
}

fn card_box(body: El<'_>) -> El<'_> {
    container(body)
        .width(Length::Fill)
        .style(boxed(CARD.get(), 12.0, Some((1.0, CARD_RIM.get()))))
        .into()
}

/// The 1 px line between two rows, 16 px in.
fn rule<'a>() -> El<'a> {
    container(
        container(Space::new().width(Length::Fill).height(1)).style(boxed(LINE.get(), 0.0, None)),
    )
    .padding([0, 16])
    .into()
}

fn divided(divider: bool, row: El<'_>) -> El<'_> {
    if !divider {
        return row;
    }
    column![
        container(
            container(Space::new().width(Length::Fill).height(1)).style(boxed(
                LINE.get(),
                0.0,
                None
            ))
        )
        .padding([0, 16]),
        row,
    ]
    .into()
}

/// "Plus tard", amber.
fn later<'a>(lang: Lang) -> El<'a> {
    container(
        text(w("Plus tard", "Later").get(lang))
            .size(10.5)
            .font(BOLD)
            .color(AMBER),
    )
    .padding([1, 7])
    .style(boxed(
        crate::theme::ink(rgba(245, 180, 74, 0.12)),
        8.0,
        None,
    ))
    .into()
}

/// A note for a screen missing: blue.
fn note<'a>(head: String, body: String, id: String) -> El<'a> {
    container(
        column![
            text(head).size(13.5).font(BOLD).color(TEXT),
            text(body)
                .size(12.5)
                .font(MANROPE)
                .color(SOFT)
                .line_height(1.5),
        ]
        .spacing(3),
    )
    .width(Length::Fill)
    .padding([12, 14])
    .style(boxed(
        crate::theme::ink(rgba(143, 179, 255, 0.08)),
        10.0,
        Some((1.0, crate::theme::ink(rgba(143, 179, 255, 0.22)))),
    ))
    .id(id)
    .into()
}

pub(super) fn switch<'a>(
    on: bool,
    inert: bool,
    focused: bool,
    moved: f32,
    press: Option<Message>,
) -> El<'a> {
    let alpha = if inert { INERT } else { 1.0 };
    let at = if on { moved } else { 1.0 - moved };
    let track = if on {
        ACCENT.get()
    } else {
        crate::theme::Ink::Token(|t| t.switch_off).get()
    };
    let knob =
        container(Space::new().width(18).height(18)).style(move |_: &Theme| container::Style {
            background: Some(Background::Color(fade(Color::WHITE, alpha))),
            border: border(9.0, 0.0, Color::TRANSPARENT),
            shadow: Shadow {
                color: rgba(0, 0, 0, 0.3 * alpha),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            ..container::Style::default()
        });
    let mut switch =
        button(container(knob).padding(Padding::default().left(2.0 + 18.0 * at).top(2)))
            .width(40)
            .height(22)
            .padding(0)
            .style(move |_, _| button::Style {
                background: Some(Background::Color(fade(track, alpha))),
                text_color: TEXT.get(),
                border: if focused {
                    ring(11.0)
                } else {
                    border(11.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            });
    if !inert && let Some(press) = press {
        switch = switch.on_press(press);
    }
    switch.into()
}

pub(super) fn menu_button<'a>(
    said: String,
    inert: bool,
    focused: bool,
    open: bool,
    press: Option<Message>,
) -> El<'a> {
    let alpha = if inert { INERT } else { 1.0 };
    let width = (measure(&said, 13.5, SEMIBOLD).ceil() + 12.0 + 10.0 + 24.0 + 8.0).max(158.0);
    let mut menu = button(
        row![
            text(said)
                .size(13.5)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), alpha)),
            Space::new().width(Length::Fill),
            icon(Icon::Chevron, 12.0, fade(DIM.get(), alpha)),
        ]
        .spacing(10)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(32)
    .width(width)
    .padding([0, 12])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(fade(
                crate::theme::ink(rgba(
                    255,
                    255,
                    255,
                    if hovered || open { 0.09 } else { 0.06 },
                )),
                alpha,
            ))),
            text_color: TEXT.get(),
            border: if focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, fade(RIM_12.get(), alpha))
            },
            ..button::Style::default()
        }
    });
    if !inert && let Some(press) = press {
        menu = menu.on_press(press);
    }
    menu.into()
}

fn solid(fill: Color, ink: Color, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            Color {
                a: (fill.a + 0.04).min(1.0),
                ..fill
            }
        } else {
            fill
        })),
        text_color: ink,
        border: border(8.0, 0.0, Color::TRANSPARENT),
        ..button::Style::default()
    }
}

fn ring_border(radius: f32) -> iced::Border {
    ring(radius)
}
