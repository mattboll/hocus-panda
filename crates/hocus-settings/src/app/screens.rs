//! The Écrans page (6.9, variant A: the arrangement on top): the drawing
//! of the screens, a click choosing one and a drag placing it; the chosen
//! screen's rows, then all screens', then the night light (later); the
//! notes for a screen missing; the "Garder ?" dialog of a mode tried.
//!
//! The screens are the compositor's, over its line ([`crate::screens`]).
//! A place, a scale, the primary screen, VRR and a screen turned off are
//! written at once, with the toast's "Annuler"; a resolution or a refresh
//! rate is tried first: the compositor applies it without the file and
//! takes it back by itself after 15 s, unless "Garder" keeps it (then
//! written).

use std::time::{Duration, Instant};

use iced::{
    Alignment, Background, Color, Element, Length, Padding, Point, Rectangle, Renderer, Shadow,
    Size, Theme, Vector, mouse,
    widget::{Space, button, canvas, column, container, mouse_area, pin, row, stack, text},
};

use super::{App, Doing, Focus, Message, ToastAction, style::*};
use crate::theme::Ink;
use crate::{
    lang::{Lang, w},
    model::Icon,
    screens::{
        self, ARRANGE, Align, IDENTIFY, MIRROR, Mode3, NIGHT_LIGHT, PRIMARY, RATE, RESOLUTION,
        ROTATION, Rect, SCALE, SCALES, Said, Screen, Screens, Side, USE, VRR,
    },
    store::KeyEdit,
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
    serial: u64,
    /// Where the pointer is on the window, as its events said it.
    pointer: Option<Point>,
}

/// A control of the page that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Identify,
    Screen(String),
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
            Msg::Choose(row, choice) => {
                self.screens.menu = None;
                self.screens_choose(row, choice)
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
        if self.desktops_said(&said) {
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
            key: key(&screen.name, "mode"),
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
                    key: key(&screen.name, "enabled"),
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
                    key: key(&screen.name, "vrr"),
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
                    key: key(&screen.name, "scale"),
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
            Choice::Primary(name) => {
                let said_now = self.say_primary(name.as_deref());
                let said_then = self.say_primary(self.primary_named().as_deref());
                let edits = vec![KeyEdit {
                    key: vec!["panel".to_owned(), "output".to_owned()],
                    value: name.map(|n| toml_edit::Value::from(n).to_string()),
                }];
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
        position_edits(&screens::normalised(&places))
    }

    /// Whether the file places the screens (every screen plugged has a
    /// `position`).
    fn file_places(&self) -> bool {
        let Some(known) = self.screens_list() else {
            return false;
        };
        let names: Vec<&str> = known.plugged().map(|s| s.name.as_str()).collect();
        !names.is_empty()
            && names.iter().all(|n| {
                self.snapshot
                    .as_ref()
                    .is_some_and(|s| s.screens_placed.iter().any(|p| p == n))
            })
    }

    /// `[panel] output` as the file says it.
    fn primary_named(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.primary_output.clone())
            .filter(|n| !n.is_empty())
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
        let edits = position_edits(&screens::normalised(&places));
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
            position_edits(&screens::normalised(&places)),
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

    /// The drawing's frame: its scale and where its screens' box starts.
    fn view_of(&self) -> Option<View> {
        let known = self.screens_list()?;
        let width = self
            .places
            .at
            .get("drawing")
            .map_or(super::view::PAGE_WIDTH - 16.0, |r| r.width);
        View::of(known, width)
    }

    pub(super) fn screens_page(&self) -> El<'_> {
        let lang = self.lang;
        let mut page = column![
            column![
                text(self.say(w("Écrans", "Screens"))).size(22).font(BOLD).color(TEXT),
                t(
                    self.say(w(
                        "La place de chaque écran, sa résolution, sa fréquence et son échelle ; l’écran où vont la barre et les nouvelles fenêtres.",
                        "Where each screen sits, its resolution, refresh rate and scale; the screen the bar and new windows go to.",
                    )),
                    13.5,
                    DIM.get(),
                )
                .line_height(1.5),
            ]
            .spacing(6),
        ]
        .spacing(18)
        .padding(Padding::default().top(14));
        // The arrangement.
        let identify_focus = self.focused(&Control::Identify);
        let identify = container(
            button(
                row![
                    icon(Icon::Screens, 13.0, DIM.get()),
                    t(self.say(w("Identifier", "Identify")), 12.0, TEXT.get()).font(SEMIBOLD),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .height(26)
            .padding([0, 10])
            .on_press_maybe((!self.chosen_inert_all()).then_some(Message::Screens(Msg::Identify)))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if hovered { 0.09 } else { 0.06 },
                    )))),
                    text_color: TEXT.get(),
                    border: if identify_focus {
                        ring(7.0)
                    } else {
                        border(7.0, 1.0, RIM_12.get())
                    },
                    ..button::Style::default()
                }
            }),
        )
        .id(format!("control:{IDENTIFY}"));
        let head = row![
            label(self.say(w("DISPOSITION", "ARRANGEMENT"))),
            Space::new().width(Length::Fill),
            identify
        ]
        .align_y(Alignment::Center)
        .height(28);
        let drawing = container(
            canvas(Drawing {
                drawn: self.drawn(),
                landing: self.screens.held.as_ref().and_then(|h| {
                    let (moving, _, at) = self.landing(h)?;
                    h.dragging.then(|| {
                        let view = self.view_of()?;
                        Some(view.rect_of(Rect {
                            x: at.0,
                            y: at.1,
                            ..moving.rect()
                        }))
                    })?
                }),
                holding: self.screens.held.as_ref().map(|h| h.name.clone()),
                big: self.screens.identify.is_some(),
                empty: self.screens_empty(),
            })
            .width(Length::Fill)
            .height(DRAWING_HEIGHT),
        )
        .id("drawing".to_owned());
        let arrangement = column![
            container(drawing).style(boxed(crate::theme::ink(rgba(0, 0, 0, 0.20)), 8.0, None)),
            container(
                t(
                    self.say(w(
                        "Glisse un écran pour le placer contre l’autre ; un clic le choisit, ses réglages sont dessous. La bande claire est la barre.",
                        "Drag a screen against the other; click one to show its settings below. The light strip is the bar.",
                    )),
                    12.0,
                    FAINT.get(),
                ),
            )
            .padding([2, 6]),
        ]
        .spacing(8);
        page = page.push(
            container(column![head, card_box(container(arrangement).padding(8).into())].spacing(8))
                .id(format!("row:{ARRANGE}")),
        );
        for note in self.notes() {
            page = page.push(note);
        }
        // The chosen screen.
        if let Some(screen) = self.chosen() {
            page = page.push(self.screen_group(screen));
        }
        // All screens.
        let primary_line = self.primary_line();
        let mut all = column![];
        all = all.push(self.screen_row(
            PRIMARY,
            self.say(w("Écran principal", "Primary screen")),
            Some(primary_line.0),
            primary_line.1,
            None,
            self.menu_control(PRIMARY),
        ));
        all = all.push(divided(
            true,
            self.screen_row(
                MIRROR,
                self.say(w("Recopie", "Mirroring")),
                Some(
                    self.say(w(
                        "Montrer la même image sur les deux écrans, pour un projecteur ou une présentation. Pas encore construite.",
                        "Show the same picture on both screens, for a projector or a talk. Not built yet.",
                    ))
                    .to_owned(),
                ),
                None,
                Some(later(lang)),
                switch(false, true, false, 1.0, None),
            ),
        ));
        page = page.push(
            column![
                row![label(self.say(w("TOUS LES ÉCRANS", "ALL SCREENS")))]
                    .height(28)
                    .align_y(Alignment::Center),
                card_box(all.into()),
            ]
            .spacing(8),
        );
        // The night light, later.
        let mut night = column![];
        let rows = [
            (
                w("Éclairage nocturne", "Night light"),
                w(
                    "Des couleurs plus chaudes le soir.",
                    "Warmer colours in the evening.",
                ),
            ),
            (
                w("Horaires", "Schedule"),
                w("Du coucher au lever du soleil", "From sunset to sunrise"),
            ),
            (w("Température", "Temperature"), w("Chaude", "Warm")),
        ];
        for (i, (name, line)) in rows.into_iter().enumerate() {
            let id = if i == 0 {
                NIGHT_LIGHT
            } else {
                "screens.night-light.more"
            };
            night = night.push(divided(
                i > 0,
                container(self.screen_row(
                    id,
                    self.say(name),
                    Some(self.say(line).to_owned()),
                    None,
                    None,
                    if i == 0 {
                        switch(false, true, false, 1.0, None)
                    } else {
                        Space::new().into()
                    },
                ))
                .style(|_: &Theme| container::Style::default())
                .into(),
            ));
        }
        page = page.push(
            column![
                row![
                    label(self.say(w("ÉCLAIRAGE NOCTURNE", "NIGHT LIGHT"))),
                    container(later(lang)).padding(Padding::default().left(8)),
                ]
                .height(28)
                .align_y(Alignment::Center),
                container(card_box(night.into())).style(|_: &Theme| container::Style::default()),
            ]
            .spacing(8),
        );
        container(page).id("page:screens".to_owned()).into()
    }

    /// Nothing to draw yet, or nothing will come: the drawing says so.
    fn screens_empty(&self) -> Option<String> {
        if self.screens.unavailable {
            return Some(
                self.say(w(
                    "Le bureau ne dit pas ses écrans ici : les Réglages ne tournent pas sous Hocuspanda.",
                    "The desktop does not tell its screens here: Settings is not running under Hocuspanda.",
                ))
                .to_owned(),
            );
        }
        self.screens
            .known
            .is_none()
            .then(|| self.say(w("Les écrans…", "The screens…")).to_owned())
    }

    /// The screens as drawn.
    fn drawn(&self) -> Vec<Drawn> {
        let lang = self.lang;
        let (Some(known), Some(view)) = (self.screens_list(), self.view_of()) else {
            return Vec::new();
        };
        let chosen = self.chosen().map(|s| s.name.clone());
        let on = known.on().count();
        let now = self.now;
        let mut out = Vec::new();
        for s in &known.list {
            let mut rect = view.rect_of(s.rect());
            let mut lifted = false;
            if let Some(held) = &self.screens.held
                && held.dragging
                && held.name == s.name
            {
                rect.x += held.at.x - held.from.x;
                rect.y += held.at.y - held.from.y;
                lifted = true;
            } else if let Some(sliding) = &self.screens.sliding
                && sliding.name == s.name
                && !self.reduced_motion
            {
                let k = super::view::ease_out(now.saturating_duration_since(sliding.since), SLIDE);
                rect.x = sliding.from.x + (rect.x - sliding.from.x) * k;
                rect.y = sliding.from.y + (rect.y - sliding.from.y) * k;
            }
            let dashed = if !s.connected {
                Some(
                    self.say(w(
                        "Débranché · sa place est gardée",
                        "Unplugged · its place is kept",
                    ))
                    .to_owned(),
                )
            } else if s.lid_closed && on > 0 {
                Some(self.say(w("Couvercle fermé", "Lid closed")).to_owned())
            } else if !s.enabled {
                Some(self.say(w("Éteint", "Off")).to_owned())
            } else {
                None
            };
            let title = if rect.width < 150.0 {
                s.short(lang)
            } else {
                s.title(lang)
            };
            // A narrow one says its size alone.
            let mut line = match s.mode {
                Some(m) if rect.width < 190.0 => screens::say_size((m.0, m.1)),
                Some(m) => format!(
                    "{} · {}",
                    screens::say_size((m.0, m.1)),
                    screens::say_rate(m.2, lang)
                ),
                None => String::new(),
            };
            if !s.scale_auto && s.connected {
                line.push_str(&format!(" · {}", screens::say_scale(s.scale)));
            }
            let bar = (s.primary && dashed.is_none()).then(|| {
                let width = if s.size.0 >= FULL_FROM { 196.0 } else { 56.0 };
                let f = view.f;
                Rectangle {
                    x: rect.x + rect.width - (8.0 + width) * f,
                    y: rect.y + 8.0 * f,
                    width: width * f,
                    height: (rect.height - 16.0 * f).max(0.0),
                }
            });
            out.push(Drawn {
                name: s.name.clone(),
                number: s.number,
                rect,
                selected: chosen.as_deref() == Some(s.name.as_str()) && s.connected,
                dashed,
                title,
                line,
                bar,
                lifted,
            });
        }
        out
    }

    /// The notes for a screen missing.
    fn notes(&self) -> Vec<El<'_>> {
        let lang = self.lang;
        let Some(known) = self.screens_list() else {
            return Vec::new();
        };
        let laptop = known
            .plugged()
            .find(|s| s.built_in)
            .or_else(|| known.plugged().next());
        let other_name = |s: &Screen| {
            known
                .plugged()
                .find(|o| o.name != s.name && o.enabled)
                .map(|o| screens::lower_first(&o.title(lang)))
                .unwrap_or_default()
        };
        let mut notes = Vec::new();
        for s in &known.list {
            let title = s.title(lang);
            let (head, body) = if !s.connected {
                let at = s.unplugged_at.and_then(local_time).unwrap_or_default();
                let place = laptop.map(|l| {
                    let side = screens::side_of(s.rect(), l.rect());
                    let align = screens::align_of(s.rect(), l.rect(), side);
                    screens::say_place(side, align, &l.title(lang), lang)
                });
                let other = laptop
                    .map(|l| screens::lower_first(&l.title(lang)))
                    .unwrap_or_default();
                match lang {
                    Lang::French => (
                        format!("{title} débranché à {at}"),
                        format!(
                            "Sa place ({}) et ses réglages sont gardés : il les reprend dès qu’il revient. Ses fenêtres sont passées sur l’{other}.",
                            place.unwrap_or_default()
                        ),
                    ),
                    Lang::English => (
                        format!("{title} unplugged at {at}"),
                        format!(
                            "Its place ({}) and settings are kept: it takes them back as soon as it returns. Its windows went to the {other}.",
                            place.unwrap_or_default()
                        ),
                    ),
                }
            } else if s.lid_closed && known.on().count() > 0 {
                let other = other_name(s);
                match lang {
                    Lang::French => (
                        "Couvercle fermé".to_owned(),
                        format!(
                            "L’écran du portable est éteint ; la session continue sur l’{other}, et la barre y est passée. Ses réglages reviennent quand tu rouvres le couvercle."
                        ),
                    ),
                    Lang::English => (
                        "Lid closed".to_owned(),
                        format!(
                            "The laptop screen is off; the session goes on on the {other}, and the bar moved there. Its settings come back when you open the lid."
                        ),
                    ),
                }
            } else if !s.enabled {
                let other = other_name(s);
                match lang {
                    Lang::French => (
                        format!("{title} éteint"),
                        format!(
                            "Il reste branché et garde sa place ; ses fenêtres sont passées sur l’{other}."
                        ),
                    ),
                    Lang::English => (
                        format!("{title} off"),
                        format!(
                            "It stays plugged in and keeps its place; its windows went to the {other}."
                        ),
                    ),
                }
            } else {
                continue;
            };
            notes.push(note(head, body, format!("note:{}", s.name)));
        }
        if known.places_refused {
            notes.push(note(
                self.say(w("Places du fichier refusées", "The file's places refused")).to_owned(),
                self.say(w(
                    "Dans config.toml, les écrans se chevauchent ou l’un est à l’écart : ils sont rangés de gauche à droite. Glisse-les pour les placer.",
                    "In config.toml the screens overlap or one stands apart: they are put left to right. Drag them into place.",
                ))
                .to_owned(),
                "note:places".to_owned(),
            ));
        }
        notes
    }

    /// The chosen screen's group: its head and its rows.
    fn screen_group(&self, screen: &Screen) -> El<'_> {
        let lang = self.lang;
        let mut head = row![
            container(
                text(screen.number.to_string())
                    .size(10.5)
                    .font(BOLD)
                    .color(DEEP)
            )
            .width(20)
            .height(20)
            .center(20)
            .style(boxed(ACCENT.get(), 10.0, None)),
            label_owned(screen.title(lang).to_uppercase()),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(28);
        if screen.primary {
            head = head.push(
                container(t(self.say(w("Principal", "Primary")), 11.0, BLUE.get()).font(BOLD))
                    .padding([2, 8])
                    .style(boxed(
                        crate::theme::ink(rgba(143, 179, 255, 0.12)),
                        10.0,
                        None,
                    )),
            );
        }
        let mut facts = vec![screen.name.clone()];
        if let Some(i) = screen.inches() {
            facts.push(match lang {
                Lang::French => format!("{i} pouces"),
                Lang::English => format!("{i} in"),
            });
        }
        if let Some(d) = screen.density() {
            facts.push(match lang {
                Lang::French => format!("{d} ppp"),
                Lang::English => format!("{d} ppi"),
            });
        }
        head = head.push(Space::new().width(Length::Fill));
        head = head.push(text(facts.join(" · ")).size(11).font(MONO).color(FAINT));
        let only = self.screens_list().is_some_and(|k| k.on().count() <= 1) && screen.enabled;
        let lid = screen.lid_closed && self.screens_list().is_some_and(|k| k.on().count() > 0);
        let use_line = if lid {
            self.say(w("Le couvercle est fermé.", "The lid is closed."))
        } else if only {
            self.say(w(
                "C’est le seul écran allumé.",
                "It is the only screen on.",
            ))
        } else {
            self.say(w(
                "Éteint, il garde sa place et ses réglages.",
                "Off, it keeps its place and its settings.",
            ))
        };
        let native = screen.native();
        let size = screen.mode.map(|m| (m.0, m.1));
        let resolution_line = match (native, size) {
            (Some(n), Some(s)) if n == s => self
                .say(w("Sa définition native.", "Its native resolution."))
                .to_owned(),
            (Some(n), _) => match lang {
                Lang::French => format!(
                    "Hors de sa définition native ({}), l’image est moins nette.",
                    screens::say_size(n)
                ),
                Lang::English => format!(
                    "Not its native resolution ({}): the picture is less sharp.",
                    screens::say_size(n)
                ),
            },
            _ => String::new(),
        };
        let resolution_warn = screen
            .file_mode
            .as_ref()
            .filter(|_| !screen.mode_offered)
            .map(|m| {
                let applies = screen
                    .mode
                    .map(|m| say_mode3(m, lang))
                    .unwrap_or_default();
                match lang {
                    Lang::French => format!("Dans le fichier : « {m} », que l’écran ne propose pas ; {applies} s’applique."),
                    Lang::English => format!("In the file: “{m}”, which the screen does not offer; {applies} applies."),
                }
            });
        let rates = size.map(|s| screen.rates(s)).unwrap_or_default();
        let rate_line = (rates.len() <= 1).then(|| {
            self.say(w(
                "Une seule à cette résolution.",
                "Only one at this resolution.",
            ))
            .to_owned()
        });
        let logical_size = screen.size;
        let mut scale_line = match lang {
            Lang::French => format!("Comme {}", screens::say_size(logical_size)),
            Lang::English => format!("Like {}", screens::say_size(logical_size)),
        };
        if screen.scale_auto
            && let Some(d) = screen.density()
        {
            scale_line.push_str(&match lang {
                Lang::French => format!(", choisie par sa densité ({d} ppp)"),
                Lang::English => format!(", chosen by its density ({d} ppi)"),
            });
        }
        if logical_size.0 < FULL_FROM {
            scale_line.push_str(self.say(w(
                " ; la barre y prend sa forme mini.",
                "; the bar takes its mini form there.",
            )));
        }
        let vrr_line = if screen.vrr_capable {
            self.say(w(
                "L’image suit le rythme des jeux et des vidéos, sans saccades.",
                "The picture follows games and videos, without stutter.",
            ))
        } else {
            self.say(w(
                "Cet écran ne la propose pas.",
                "This screen does not offer it.",
            ))
        };
        let mut rows = column![];
        rows = rows.push(self.screen_row(
            USE,
            self.say(w("Utiliser cet écran", "Use this screen")),
            Some(use_line.to_owned()),
            None,
            None,
            self.switch_control(USE, screen.enabled),
        ));
        rows = rows.push(rule());
        let rows = rows
            .push(self.screen_row(
                RESOLUTION,
                self.say(w("Résolution", "Resolution")),
                Some(resolution_line),
                resolution_warn,
                None,
                self.menu_control(RESOLUTION),
            ))
            .push(rule())
            .push(self.screen_row(
                RATE,
                self.say(w("Fréquence", "Refresh rate")),
                rate_line,
                None,
                None,
                self.menu_control(RATE),
            ))
            .push(rule())
            .push(self.screen_row(
                SCALE,
                self.say(w("Échelle", "Scale")),
                Some(scale_line),
                None,
                None,
                self.menu_control(SCALE),
            ))
            .push(rule())
            .push(self.screen_row(
                VRR,
                self.say(w("Fréquence variable (VRR)", "Variable refresh rate (VRR)")),
                Some(vrr_line.to_owned()),
                None,
                None,
                self.switch_control(VRR, screen.vrr),
            ))
            .push(rule())
            .push(
                self.screen_row(
                    ROTATION,
                    self.say(w("Rotation", "Rotation")),
                    Some(
                        self.say(w(
                            "Prévue après la v1 (choix du 29 octobre) : à confirmer.",
                            "Planned after v1 (choice of 29 October): to be confirmed.",
                        ))
                        .to_owned(),
                    ),
                    None,
                    Some(later(lang)),
                    menu_button(
                        self.say(w("Normale", "Normal")).to_owned(),
                        true,
                        false,
                        false,
                        None,
                    ),
                ),
            );
        column![head, card_box(rows.into())].spacing(8).into()
    }

    /// The line under "Écran principal", and its warning.
    fn primary_line(&self) -> (String, Option<String>) {
        let line = self
            .say(w(
                "La barre et les nouvelles fenêtres y vont ; automatique : l’écran du portable s’il est allumé, sinon l’autre. La forme de la barre, écran par écran, se règle dans La barre.",
                "The bar and new windows go there; automatic: the laptop screen when it is on, else the other. The bar's form, screen by screen, is set in The bar.",
            ))
            .to_owned();
        let warn = self.primary_named().and_then(|name| {
            let known = self.screens_list()?;
            if known.get(&name).is_some_and(|s| s.connected) {
                return None;
            }
            let there = known.plugged().find(|s| s.primary)?;
            let named = self.say_primary(Some(&name));
            let there = screens::lower_first(&there.title(self.lang));
            Some(match self.lang {
                Lang::French => {
                    format!("{named} n’est pas là : la barre est sur l’{there} en attendant.")
                }
                Lang::English => {
                    format!("{named} is not there: the bar is on the {there} meanwhile.")
                }
            })
        });
        (line, warn)
    }

    fn switch_control(&self, row: &'static str, on: bool) -> El<'_> {
        let inert = self.screens_row_inert(row);
        let moved = self.knobs.get(row).map_or(1.0, |since| {
            super::view::ease_out(self.now.saturating_duration_since(*since), super::KNOB)
        });
        let focused = self.focused(&Control::Row(row));
        container(switch(
            on,
            inert,
            focused,
            moved,
            Some(Message::Screens(Msg::Toggle(row))),
        ))
        .id(format!("control:{row}"))
        .into()
    }

    fn menu_control(&self, row: &'static str) -> El<'_> {
        let inert = self.screens_row_inert(row);
        let current = self.screens_current(row);
        let said = self
            .screens_choices(row)
            .into_iter()
            .find(|(c, _)| Some(c) == current.as_ref())
            .map(|(_, l)| l)
            .unwrap_or_default();
        let open = self.screens.menu.is_some_and(|(r, _)| r == row);
        let focused = self.focused(&Control::Row(row));
        container(menu_button(
            said,
            inert,
            focused,
            open,
            Some(Message::Screens(Msg::Menu(row))),
        ))
        .id(format!("control:{row}"))
        .into()
    }

    /// A row: its name, its line, its warning, a chip, its control.
    fn screen_row<'a>(
        &self,
        id: &'static str,
        name: &'static str,
        line: Option<String>,
        warn: Option<String>,
        chip: Option<El<'a>>,
        control: El<'a>,
    ) -> El<'a> {
        let inert = self.screens_row_inert(id);
        let alpha = if inert && !matches!(id, PRIMARY) {
            INERT
        } else {
            1.0
        };
        let mut title = row![
            text(name)
                .size(14)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), alpha))
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(chip) = chip {
            title = title.push(chip);
        }
        let mut words = column![title].spacing(2).width(Length::Fill);
        if let Some(line) = line.filter(|l| !l.is_empty()) {
            words = words.push(t(line, 12.5, fade(DIM.get(), alpha)));
        }
        if let Some(warn) = warn {
            words = words.push(t(warn, 12.5, PEACH.get()));
        }
        let lit = self.lit(id);
        container(
            row![
                words,
                row![Space::new().height(36), control].align_y(Alignment::Center)
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
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

    // --- Over the page ------------------------------------------------------------------

    /// The page's open menu, under its button.
    pub(super) fn screens_menu_list(&self) -> Option<El<'_>> {
        let (row, since) = self.screens.menu?;
        let at = *self.places.at.get(&format!("control:{row}"))?;
        let current = self.screens_current(row);
        let opened = if self.reduced_motion {
            1.0
        } else {
            super::view::ease_out(self.now.saturating_duration_since(since), super::MENU_IN)
        };
        let width = at.width.max(200.0);
        let mut list = column![];
        for (i, (choice, label)) in self.screens_choices(row).into_iter().enumerate() {
            let chosen = current.as_ref() == Some(&choice);
            let check: El<'_> = if chosen {
                icon(Icon::Check, 13.0, fade(ACCENT.get(), opened))
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(label, 13.5, fade(TEXT.get(), opened)).font(if chosen {
                                SEMIBOLD
                            } else {
                                MANROPE
                            }),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 8])
                    .on_press(Message::Screens(Msg::Choose(row, choice)))
                    .style(flat(
                        (i == self.screens.menu_index)
                            .then_some(crate::theme::ink(rgba(255, 255, 255, 0.07))),
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        let panel = container(list)
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
        let x = (at.x + at.width - width).max(0.0);
        // Under its button, or over it when the window's bottom is near.
        let height = 10.0 + 32.0 * self.screens_choices(row).len() as f32;
        let bottom = self
            .places
            .at
            .get(&format!(
                "sidebar:{}",
                if self.lang.fr() { "fr" } else { "en" }
            ))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + 38.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0) + 4.0 * (1.0 - opened)
        } else {
            under - 4.0 * (1.0 - opened)
        };
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Screens(Msg::CloseMenu)),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    /// "Garder ?", over the window, while a mode is tried.
    pub(super) fn screens_dialog(&self) -> Option<El<'_>> {
        let trial = self.screens.trial.as_ref()?;
        let lang = self.lang;
        let screen = self.screens_list().and_then(|k| k.get(&trial.output));
        let title = screen
            .map(|s| screens::lower_first(&s.title(lang)))
            .unwrap_or_default();
        let now = say_mode_text(&trial.mode, lang);
        let then = say_mode_text(&trial.before, lang);
        let left = trial.left;
        let question = match (trial.what, lang) {
            (RATE, Lang::French) => "Garder cette fréquence ?",
            (RATE, Lang::English) => "Keep this refresh rate?",
            (_, Lang::French) => "Garder cette résolution ?",
            (_, Lang::English) => "Keep this resolution?",
        };
        let body = match lang {
            Lang::French => {
                let mut s = format!(
                    "L’{title} affiche maintenant {now}. Sans réponse, il revient à {then} dans {left} s."
                );
                if let Some(first) = s.get(..1) {
                    s = first.to_uppercase() + &s[1..];
                }
                s
            }
            Lang::English => format!(
                "The {title} now shows {now}. Without an answer it goes back to {then} in {left} s."
            ),
        };
        // Sizes and rates kept on one line.
        let body = body
            .replace(" × ", "\u{a0}×\u{a0}")
            .replace(" Hz", "\u{a0}Hz")
            .replace(" s.", "\u{a0}s.");
        let on_keep = trial.on_keep;
        let ring = canvas(Countdown {
            left,
            of: TRIAL_SECONDS,
        })
        .width(44)
        .height(44);
        let buttons = row![
            Space::new().width(Length::Fill),
            container(
                button(t(self.say(w("Revenir", "Revert")), 13.0, TEXT.get()).font(SEMIBOLD))
                    .height(32)
                    .padding([7, 14])
                    .on_press(Message::Screens(Msg::Revert))
                    .style(move |_, status| {
                        let mut style = solid(
                            crate::theme::ink(rgba(255, 255, 255, 0.08)),
                            TEXT.get(),
                            status,
                        );
                        if !on_keep {
                            style.border = ring_border(8.0);
                        }
                        style
                    }),
            )
            .id("dialog:revert".to_owned()),
            container(
                button(t(self.say(w("Garder", "Keep")), 13.0, DEEP.get()).font(BOLD))
                    .height(32)
                    .padding([7, 16])
                    .on_press(Message::Screens(Msg::Keep))
                    .style(move |_, status| {
                        let mut style = solid(ACCENT.get(), DEEP.get(), status);
                        if on_keep {
                            style.border =
                                border(8.0, 2.0, crate::theme::ink(rgba(255, 255, 255, 0.85)));
                        }
                        style
                    }),
            )
            .id("dialog:keep".to_owned()),
        ]
        .spacing(8);
        let dialog = container(
            column![
                row![
                    ring,
                    column![
                        text(question).size(17).font(BOLD).color(TEXT),
                        t(body, 13.0, SOFT.get()).line_height(1.5),
                        t(
                            self.say(w(
                                "Échap revient aussitôt. Si la fenêtre se ferme, l’écran revient aussi.",
                                "Esc goes back at once. If the window closes, the screen goes back too.",
                            )),
                            12.0,
                            FAINT.get(),
                        ),
                    ]
                    .spacing(6)
                    .width(Length::Fill),
                ]
                .spacing(16),
                buttons,
            ]
            .spacing(18),
        )
        .width(440)
        .padding(20)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(MENU.get())),
            border: border(14.0, 1.0, RIM_12.get()),
            shadow: shadow(),
            ..container::Style::default()
        })
        .id("dialog".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.58)),
                        0.0,
                        None
                    ))
                )
                .on_press(Message::Nothing),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(super) fn screens_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let drawn: Vec<serde_json::Value> = self
            .drawn()
            .into_iter()
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "number": d.number,
                    "at": [d.rect.x.round(), d.rect.y.round(), d.rect.width.round(), d.rect.height.round()],
                    "selected": d.selected,
                    "state": d.dashed,
                    "title": d.title,
                    "line": d.line,
                    "bar": d.bar.is_some(),
                })
            })
            .collect();
        let rows: Vec<serde_json::Value> = [USE, RESOLUTION, RATE, SCALE, VRR, PRIMARY]
            .into_iter()
            .map(|row| {
                let value = match row {
                    USE => self.chosen().map(|s| s.enabled.to_string()),
                    VRR => self.chosen().map(|s| s.vrr.to_string()),
                    _ => {
                        let current = self.screens_current(row);
                        self.screens_choices(row)
                            .into_iter()
                            .find(|(c, _)| Some(c) == current.as_ref())
                            .map(|(_, l)| l)
                    }
                };
                serde_json::json!({ "id": row, "value": value, "inert": self.screens_row_inert(row) })
            })
            .collect();
        let choices: Option<Vec<String>> = self.screens.menu.map(|(row, _)| {
            self.screens_choices(row)
                .into_iter()
                .map(|(_, l)| l)
                .collect()
        });
        let chosen = self.chosen();
        serde_json::json!({
            "known": self.screens.known.is_some(),
            "unavailable": self.screens.unavailable,
            "drawn": drawn,
            "chosen": chosen.map(|s| s.name.clone()),
            "chosen_title": chosen.map(|s| s.title(lang)),
            "rows": rows,
            "menu": self.screens.menu.map(|(row, _)| row),
            "choices": choices,
            "dialog": self.screens.trial.as_ref().map(|t| serde_json::json!({
                "id": t.id, "output": t.output, "mode": t.mode, "left": t.left, "keep_focused": t.on_keep,
            })),
            "dragging": self.screens.held.as_ref().is_some_and(|h| h.dragging),
            "identify": self.screens.identify.is_some(),
            "notes": self.screens_list().map_or(0, |_| self.notes().len()),
        })
    }
}

/// The trial's length, as the compositor counts it.
const TRIAL_SECONDS: u64 = 15;

/// `[outputs.NAME] what`.
fn key(name: &str, what: &str) -> Vec<String> {
    vec!["outputs".to_owned(), name.to_owned(), what.to_owned()]
}

/// Every place, as edits.
fn position_edits(places: &[(String, (i32, i32))]) -> Vec<KeyEdit> {
    places
        .iter()
        .map(|(name, (x, y))| KeyEdit {
            key: key(name, "position"),
            value: Some(format!("[{x}, {y}]")),
        })
        .collect()
}

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

/// The drawing's frame.
#[derive(Debug, Clone, Copy)]
struct View {
    /// Drawing pixels per logical pixel.
    f: f32,
    /// Where the logical box's corner is on the drawing.
    origin: (f32, f32),
    /// The logical box's corner.
    corner: (i32, i32),
}

impl View {
    fn of(known: &Screens, width: f32) -> Option<Self> {
        let rects: Vec<Rect> = known.list.iter().map(Screen::rect).collect();
        let x0 = rects.iter().map(|r| r.x).min()?;
        let y0 = rects.iter().map(|r| r.y).min()?;
        let x1 = rects.iter().map(|r| r.x + r.w).max()?;
        let y1 = rects.iter().map(|r| r.y + r.h).max()?;
        let (bw, bh) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
        let f = MOST_F
            .min((width - 2.0 * MARGIN_X) / bw)
            .min((DRAWING_HEIGHT - 2.0 * MARGIN_Y) / bh);
        Some(Self {
            f,
            origin: ((width - bw * f) / 2.0, (DRAWING_HEIGHT - bh * f) / 2.0),
            corner: (x0, y0),
        })
    }

    fn rect_of(&self, r: Rect) -> Rectangle {
        Rectangle {
            x: self.origin.0 + (r.x - self.corner.0) as f32 * self.f,
            y: self.origin.1 + (r.y - self.corner.1) as f32 * self.f,
            width: r.w as f32 * self.f,
            height: r.h as f32 * self.f,
        }
    }

    /// A point of the drawing, logical.
    fn logical(&self, p: Point) -> (i32, i32) {
        (
            ((p.x - self.origin.0) / self.f).round() as i32 + self.corner.0,
            ((p.y - self.origin.1) / self.f).round() as i32 + self.corner.1,
        )
    }
}

/// The drawing: dots, the screens, the bar on the primary one, where a
/// dragged one lands.
struct Drawing {
    drawn: Vec<Drawn>,
    landing: Option<Rectangle>,
    holding: Option<String>,
    big: bool,
    empty: Option<String>,
}

impl canvas::Program<Message> for Drawing {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // A dot every 14 px.
        let dot = crate::theme::ink(rgba(255, 255, 255, 0.07));
        let mut y = 7.0;
        while y < bounds.height {
            let mut x = 7.0;
            while x < bounds.width {
                frame.fill_rectangle(Point::new(x, y), Size::new(1.0, 1.0), dot);
                x += 14.0;
            }
            y += 14.0;
        }
        if let Some(empty) = &self.empty {
            frame.fill_text(canvas::Text {
                content: empty.clone(),
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0),
                color: FAINT.get(),
                size: 12.5.into(),
                font: MANROPE,
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        if let Some(landing) = self.landing {
            let path =
                canvas::Path::rounded_rectangle(landing.position(), landing.size(), 6.0.into());
            frame.stroke(
                &path,
                canvas::Stroke {
                    line_dash: canvas::LineDash {
                        segments: &[5.0, 4.0],
                        offset: 0,
                    },
                    ..canvas::Stroke::default()
                        .with_color(ACCENT.get())
                        .with_width(2.0)
                },
            );
        }
        // The one held last, over the other.
        let mut order: Vec<&Drawn> = self.drawn.iter().collect();
        order.sort_by_key(|d| d.lifted);
        for d in order {
            let r = d.rect;
            let path = canvas::Path::rounded_rectangle(r.position(), r.size(), 6.0.into());
            if d.lifted {
                let shadow = canvas::Path::rounded_rectangle(
                    Point::new(r.x + 2.0, r.y + 6.0),
                    r.size(),
                    6.0.into(),
                );
                frame.fill(&shadow, crate::theme::ink(rgba(0, 0, 0, 0.35)));
            }
            let alpha = if d.lifted { 0.88 } else { 1.0 };
            match (&d.dashed, d.selected) {
                (Some(_), selected) => {
                    frame.stroke(
                        &path,
                        canvas::Stroke {
                            line_dash: canvas::LineDash {
                                segments: &[5.0, 4.0],
                                offset: 0,
                            },
                            ..canvas::Stroke::default()
                                .with_color(if selected {
                                    ACCENT.get()
                                } else {
                                    crate::theme::ink(rgba(169, 178, 194, 0.55))
                                })
                                .with_width(1.5)
                        },
                    );
                }
                (None, true) => {
                    frame.fill(&path, fade(crate::theme::Ink::Accent(0.13).get(), alpha));
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_color(fade(ACCENT.get(), alpha))
                            .with_width(2.0),
                    );
                }
                (None, false) => {
                    frame.fill(
                        &path,
                        fade(crate::theme::ink(rgba(143, 179, 255, 0.10)), alpha),
                    );
                    frame.stroke(
                        &path,
                        canvas::Stroke::default()
                            .with_color(fade(crate::theme::ink(rgba(143, 179, 255, 0.50)), alpha))
                            .with_width(1.5),
                    );
                }
            }
            if let Some(bar) = d.bar {
                let strip = canvas::Path::rounded_rectangle(bar.position(), bar.size(), 2.0.into());
                frame.fill(
                    &strip,
                    fade(crate::theme::ink(rgba(238, 241, 246, 0.24)), alpha),
                );
            }
            // The number's disc, the name, the mode.
            let disc = Point::new(r.x + 8.0 + 9.0, r.y + 8.0 + 9.0);
            let (fill, ink) = if d.selected {
                (ACCENT.get(), DEEP)
            } else {
                (crate::theme::ink(rgba(143, 179, 255, 0.85)), DEEP)
            };
            let (fill, ink) = if d.dashed.is_some() && !d.selected {
                (crate::theme::ink(rgba(169, 178, 194, 0.55)), DEEP)
            } else {
                (fill, ink)
            };
            frame.fill(&canvas::Path::circle(disc, 9.0), fade(fill, alpha));
            frame.fill_text(canvas::Text {
                content: d.number.to_string(),
                position: disc,
                color: ink.get(),
                size: 10.5.into(),
                font: BOLD,
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            frame.fill_text(canvas::Text {
                content: d.title.clone(),
                position: Point::new(r.x + 33.0, r.y + 10.0),
                color: fade(TEXT.get(), alpha),
                size: 12.5.into(),
                font: BOLD,
                max_width: (r.width - 41.0).max(0.0),
                ..canvas::Text::default()
            });
            let under = d.dashed.clone().unwrap_or_else(|| d.line.clone());
            frame.fill_text(canvas::Text {
                content: under,
                position: Point::new(r.x + 10.0, r.y + 32.0),
                color: fade(DIM.get(), alpha),
                size: if d.dashed.is_some() {
                    11.5.into()
                } else {
                    10.5.into()
                },
                font: if d.dashed.is_some() { MANROPE } else { MONO },
                max_width: (r.width - 16.0).max(0.0),
                ..canvas::Text::default()
            });
            if self.big {
                frame.fill_text(canvas::Text {
                    content: d.number.to_string(),
                    position: Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0 + 6.0),
                    color: TEXT.get(),
                    size: 40.0.into(),
                    font: BOLD,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.holding.is_some() {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_in(bounds) {
            Some(at) if self.drawn.iter().any(|d| d.rect.contains(at)) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }
}

/// The trial's ring: the seconds left, and the share of them.
struct Countdown {
    left: u64,
    of: u64,
}

impl canvas::Program<Message> for Countdown {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let centre = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0 - 2.5;
        frame.stroke(
            &canvas::Path::circle(centre, radius),
            canvas::Stroke::default()
                .with_color(crate::theme::ink(rgba(255, 255, 255, 0.10)))
                .with_width(4.0),
        );
        let share = self.left as f32 / self.of.max(1) as f32;
        if share > 0.0 {
            let start = -std::f32::consts::FRAC_PI_2;
            let arc = canvas::Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: centre,
                    radius,
                    start_angle: iced::Radians(start),
                    end_angle: iced::Radians(start + share * std::f32::consts::TAU),
                });
            });
            frame.stroke(
                &arc,
                canvas::Stroke::default()
                    .with_color(ACCENT.get())
                    .with_width(4.0)
                    .with_line_cap(canvas::LineCap::Round),
            );
        }
        frame.fill_text(canvas::Text {
            content: self.left.to_string(),
            position: centre,
            color: TEXT.get(),
            size: 14.0.into(),
            font: MONO,
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
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
