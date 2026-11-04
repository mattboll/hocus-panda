//! The dialog on its surface, the 7.5 mockup's sizes (logical pixels): the
//! asking application's tile with the system's padlock, its name over
//! "Autorisation nécessaire"; polkit's message; the account (and the
//! others to choose from); the password's field, the line under it; the
//! details, folded; the requests waiting, Annuler and Autoriser. Its glass
//! is drawn here; the compositor draws its shadow, the dim and its motion.

use std::sync::{Mutex, PoisonError};

use hocus_link::{AUTH_RADIUS, AUTH_WIDTH};
use iced::{
    Color, Font, Point, Radians, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, gradient, path::arc::Elliptical},
        text::Alignment,
    },
    window,
};

use super::{Dialog, Phase, Pointing, Target, Words};
use crate::{
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, stroke},
    popover::kit::{self, MANROPE_SEMIBOLD},
};

const WIDTH: f32 = AUTH_WIDTH as f32;
const RADIUS: f32 = AUTH_RADIUS as f32;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 20.0;
const PAD_SIDE: f32 = 20.0;
const PAD_BOTTOM: f32 = 18.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = WIDTH - BORDER - PAD_SIDE;
/// The header: the 44 px tile (radius 12) and its 20 px badge 5 px past
/// it, ringed 2 px; 12 px after it the application's name (13 px 600)
/// over the title (17 px 700), 2 px apart.
const TILE: f32 = 44.0;
pub const ICON_PX: u32 = 32;
const TILE_RADIUS: f32 = 12.0;
const BADGE: f32 = 20.0;
const BADGE_OUT: f32 = 5.0;
const BADGE_RING: f32 = 2.0;
const TILE_GAP: f32 = 12.0;
const APP: f32 = 13.0;
const TITLE: f32 = 17.0;
const TITLE_GAP: f32 = 2.0;
/// polkit's message, 14 px under: 14.5 px on 1.5 lines.
const MESSAGE_GAP: f32 = 14.0;
const MESSAGE: f32 = 14.5;
const MESSAGE_LINE: f32 = 1.5;
/// The account, 16 px under: a 32 px avatar, 10 px before the name
/// (14.5 px 700) over "administrateur" (12.5 px).
const ACCOUNT_GAP: f32 = 16.0;
const AVATAR: f32 = 32.0;
const AVATAR_GAP: f32 = 10.0;
const NAME: f32 = 14.5;
const ROLE: f32 = 12.5;
/// "Changer": 30 px, 10 px in, radius 8, 12.5 px 600, a 12 px chevron 6 px
/// after it.
const CHANGE: f32 = 30.0;
const CHANGE_PAD: f32 = 10.0;
const CHANGE_RADIUS: f32 = 8.0;
const CHANGE_TEXT: f32 = 12.5;
const CHEVRON: f32 = 12.0;
const CHEVRON_GAP: f32 = 6.0;
/// The accounts' list, 8 px under: 4 px in, radius 10, a 1 px rim; rows
/// 34 px, radius 7, 10 px in, 13.5 px 600, a 14 px check.
const LIST_GAP: f32 = 8.0;
const LIST_PAD: f32 = 4.0;
const LIST_RADIUS: f32 = 10.0;
const PERSON: f32 = 34.0;
const PERSON_RADIUS: f32 = 7.0;
const PERSON_PAD: f32 = 10.0;
const PERSON_TEXT: f32 = 13.5;
const CHECK: f32 = 14.0;
/// The field, 14 px under: 44 px, radius 12, 14 px in on the left, 10 on
/// the right; a dot per character; Caps Lock's 22 px sign.
const FIELD_GAP: f32 = 14.0;
const FIELD: f32 = 44.0;
const FIELD_RADIUS: f32 = 12.0;
const FIELD_LEFT: f32 = 14.0;
const FIELD_RIGHT: f32 = 10.0;
const FIELD_TEXT: f32 = 16.0;
const DOT: f32 = 2.8;
const DOT_ADVANCE: f32 = 10.6;
const CAPS: f32 = 22.0;
const CAPS_RADIUS: f32 = 6.0;
/// The line under it, 6 px under: 13 px on 1.45 lines, 19 px kept.
const NOTE_GAP: f32 = 6.0;
const NOTE: f32 = 13.0;
const NOTE_LINE: f32 = 1.45;
const NOTE_ROOM: f32 = 19.0;
/// "Détails", 2 px under, 6 px to the left: 28 px, 4 px in on the left and
/// 8 on the right, its 14 px chevron 4 px before its word (13 px 600).
const DETAILS_GAP: f32 = 2.0;
const DETAILS: f32 = 28.0;
const DETAILS_OUT: f32 = 6.0;
const DETAILS_RADIUS: f32 = 7.0;
const DETAILS_TEXT: f32 = 13.0;
const DETAILS_CHEVRON: f32 = 14.0;
/// The facts, 6 px under: 10 / 12 px in, radius 8, 6 px between them; the
/// label 12 px in a 74 px column, 10 px before the value, Martian Mono
/// 11 px on 1.5 lines.
const FACTS_GAP: f32 = 6.0;
const FACTS_PAD_Y: f32 = 10.0;
const FACTS_PAD_X: f32 = 12.0;
const FACTS_RADIUS: f32 = 8.0;
const FACT_GAP: f32 = 6.0;
const LABEL: f32 = 12.0;
const LABEL_COLUMN: f32 = 74.0;
const LABEL_GAP: f32 = 10.0;
const VALUE: f32 = 11.0;
const VALUE_LINE: f32 = 1.5;
/// The footer, 16 px under: buttons 38 px, radius 10, 16 px in, 8 apart.
const FOOTER_GAP: f32 = 16.0;
const BUTTON: f32 = 38.0;
const BUTTON_RADIUS: f32 = 10.0;
const BUTTON_PAD: f32 = 16.0;
const BUTTON_GAP: f32 = 8.0;
const BUTTON_TEXT: f32 = 14.0;
const WAITING: f32 = 12.5;
const SPINNER: f32 = 14.0;
/// The field's shake: 360 ms, 10, 9, 6, 4 px from side to side.
const SHAKE_MS: f32 = 360.0;
const TURN_MS: f32 = 150.0;

const GLASS: (Color, Color) = (
    Color::from_rgba8(22, 27, 42, 0.96),
    Color::from_rgba8(14, 17, 28, 0.97),
);
const OPAQUE: (Color, Color) = (rgb(0x161B2A), rgb(0x0E111C));
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BADGE_FILL: Color = rgb(0x0B1020);
const BADGE_RING_COLOUR: Color = rgb(0x161B2A);
const FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const LIST_FILL: Color = Color::from_rgba8(255, 255, 255, 0.04);
const LIST_RIM: Color = Color::from_rgba8(255, 255, 255, 0.08);
const FIELD_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
const FIELD_ON: Color = Color::from_rgba8(108, 194, 135, 0.60);
const FIELD_WRONG: Color = rgb(0xF2945B);
const FIELD_CHECKING: Color = Color::from_rgba8(255, 255, 255, 0.16);
const AMBER: Color = rgb(0xF5C46E);
const AMBER_FILL: Color = Color::from_rgba8(245, 196, 110, 0.16);
const PEACH: Color = rgb(0xF7B98A);
const VALUE_INK: Color = rgb(0xC9D0DC);
const INK: Color = rgb(0x0B1020);
const READY_OFF: Color = Color::from_rgba8(108, 194, 135, 0.18);
const CHECKING_FILL: Color = Color::from_rgba8(255, 255, 255, 0.10);
const FOCUS: Color = ACCENT;
const AVATAR_FROM: Color = rgb(0x6CC287);
const AVATAR_TO: Color = rgb(0x3E8FBF);
const OTHER_FROM: Color = rgb(0xC792EA);
const OTHER_TO: Color = rgb(0x6F9BFF);
const GEAR_FROM: Color = rgb(0x5C6576);
const GEAR_TO: Color = rgb(0x9AA3B4);
const PANDA_DARK: Color = rgb(0x1D2230);
const PANDA_LIGHT: Color = rgb(0xF2F4F7);

const MARTIAN: Font = Font::with_name("Martian Mono");

/// Who asks: the name and icon of the window's application, else of the
/// program's desktop entry, else the program's name (and no icon).
pub fn who(dialog: &Dialog) -> (String, Option<String>) {
    let window = dialog
        .at
        .as_ref()
        .map(|a| a.app.clone())
        .unwrap_or_default();
    if let Some(entry) = crate::share::entry_of(&window) {
        return (entry.name, entry.icon);
    }
    let program = dialog
        .request
        .program
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    if let Some(entry) = crate::share::entry_of(&program) {
        return (entry.name, entry.icon);
    }
    let name = crate::share::app_name(&window)
        .or_else(|| (!program.is_empty()).then_some(program))
        .unwrap_or_else(|| "polkit".to_owned());
    (name, None)
}

/// A fact of the details: its label, its value's lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub label: &'static str,
    pub lines: Vec<String>,
    pub top: f32,
}

/// Where the dialog's parts are, and its size.
#[derive(Debug, Clone)]
pub struct Layout {
    pub height: f32,
    pub tile: Rectangle,
    pub app_at: Point,
    pub title_at: Point,
    pub app: String,
    pub message: Vec<String>,
    pub message_top: f32,
    pub account: f32,
    pub change: Option<Rectangle>,
    pub people: Vec<Rectangle>,
    pub list: Option<Rectangle>,
    pub field: Rectangle,
    pub note: Vec<String>,
    pub note_colour: Color,
    pub note_top: f32,
    pub details: Rectangle,
    pub facts_box: Option<Rectangle>,
    pub facts: Vec<Fact>,
    pub footer: f32,
    pub waiting: String,
    pub cancel: Rectangle,
    pub authorize: Rectangle,
}

fn width(text: &str, size: f32, font: Font) -> f32 {
    kit::measure(text, size, font)
}

/// `text` broken anywhere into lines that fit `room` in Martian Mono.
fn broken(text: &str, room: f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for c in text.chars() {
        let line = lines
            .last_mut()
            .map(|l| format!("{l}{c}"))
            .unwrap_or_default();
        if !line.is_empty()
            && width(&line, VALUE, MARTIAN) > room
            && lines.last().is_some_and(|l| !l.is_empty())
        {
            lines.push(c.to_string());
        } else if let Some(last) = lines.last_mut() {
            *last = line;
        }
    }
    lines
}

impl Layout {
    pub fn of(dialog: &Dialog, words: Words, waiting: usize, caps: bool) -> Self {
        let room = RIGHT - LEFT;
        let mut y = BORDER + PAD_TOP;
        let tile = Rectangle::new(Point::new(LEFT, y), Size::new(TILE, TILE));
        let words_x = LEFT + TILE + TILE_GAP;
        let block = APP * LINE + TITLE_GAP + TITLE * LINE;
        let block_top = y + (TILE - block) / 2.0;
        let app_at = Point::new(words_x, block_top + APP * LINE / 2.0);
        let title_at = Point::new(
            words_x,
            block_top + APP * LINE + TITLE_GAP + TITLE * LINE / 2.0,
        );
        let app = kit::elide(&who(dialog).0, RIGHT - words_x, |t| {
            width(t, APP, MANROPE_SEMIBOLD)
        });
        y += TILE + MESSAGE_GAP;
        let message = kit::wrap(dialog.request.message.trim(), room, |t| {
            width(t, MESSAGE, MANROPE)
        });
        let message_top = y;
        y += message.len() as f32 * MESSAGE * MESSAGE_LINE + ACCOUNT_GAP;
        let account = y;
        let change = dialog.several().then(|| {
            let w = CHANGE_PAD
                + width(words.change(), CHANGE_TEXT, MANROPE_SEMIBOLD)
                + CHEVRON_GAP
                + CHEVRON
                + CHANGE_PAD;
            Rectangle::new(
                Point::new(RIGHT - w, y + (AVATAR - CHANGE) / 2.0),
                Size::new(w, CHANGE),
            )
        });
        y += AVATAR;
        let mut people = Vec::new();
        let mut list = None;
        if dialog.several() && dialog.chooser {
            y += LIST_GAP;
            let n = dialog.request.accounts.len() as f32;
            let rect = Rectangle::new(
                Point::new(LEFT, y),
                Size::new(room, 2.0 * BORDER + 2.0 * LIST_PAD + n * PERSON),
            );
            for i in 0..dialog.request.accounts.len() {
                people.push(Rectangle::new(
                    Point::new(
                        LEFT + BORDER + LIST_PAD,
                        y + BORDER + LIST_PAD + i as f32 * PERSON,
                    ),
                    Size::new(room - 2.0 * (BORDER + LIST_PAD), PERSON),
                ));
            }
            y += rect.height;
            list = Some(rect);
        }
        y += FIELD_GAP;
        let field = Rectangle::new(Point::new(LEFT, y), Size::new(room, FIELD));
        y += FIELD + NOTE_GAP;
        let (note, note_colour) = note(dialog, words, caps);
        let note = if note.is_empty() {
            Vec::new()
        } else {
            kit::wrap(&note, room, |t| width(t, NOTE, MANROPE))
        };
        let note_top = y;
        y += (note.len() as f32 * NOTE * NOTE_LINE).max(NOTE_ROOM) + DETAILS_GAP;
        let details_w = 4.0
            + DETAILS_CHEVRON
            + 4.0
            + width(words.details(), DETAILS_TEXT, MANROPE_SEMIBOLD)
            + 8.0;
        let details = Rectangle::new(
            Point::new(LEFT - DETAILS_OUT, y),
            Size::new(details_w, DETAILS),
        );
        y += DETAILS;
        let mut facts = Vec::new();
        let mut facts_box = None;
        if dialog.details {
            y += FACTS_GAP;
            let top = y;
            let value_room = room - 2.0 * FACTS_PAD_X - LABEL_COLUMN - LABEL_GAP;
            let mut at = top + FACTS_PAD_Y;
            for (label, value) in self::facts(dialog, words) {
                let lines = broken(&value, value_room);
                let h = (lines.len() as f32 * VALUE * VALUE_LINE).max(LABEL * LINE);
                facts.push(Fact {
                    label,
                    lines,
                    top: at,
                });
                at += h + FACT_GAP;
            }
            let bottom = at - FACT_GAP + FACTS_PAD_Y;
            facts_box = Some(Rectangle::new(
                Point::new(LEFT, top),
                Size::new(room, bottom - top),
            ));
            y = bottom;
        }
        y += FOOTER_GAP;
        let footer = y;
        let authorize_w = BUTTON_PAD
            + width(words.authorize(), BUTTON_TEXT, MANROPE_BOLD)
            + BUTTON_PAD
            + if dialog.phase == Phase::Checking {
                SPINNER + 8.0
            } else {
                0.0
            };
        let authorize = Rectangle::new(
            Point::new(RIGHT - authorize_w, y),
            Size::new(authorize_w, BUTTON),
        );
        let cancel_w =
            BUTTON_PAD + width(words.cancel(), BUTTON_TEXT, MANROPE_SEMIBOLD) + BUTTON_PAD;
        let cancel = Rectangle::new(
            Point::new(authorize.x - BUTTON_GAP - cancel_w, y),
            Size::new(cancel_w, BUTTON),
        );
        y += BUTTON + PAD_BOTTOM + BORDER;
        Self {
            height: y,
            tile,
            app_at,
            title_at,
            app,
            message,
            message_top,
            account,
            change,
            people,
            list,
            field,
            note,
            note_colour,
            note_top,
            details,
            facts_box,
            facts,
            footer,
            waiting: words.waiting(waiting),
            cancel,
            authorize,
        }
    }

    /// The surface's size.
    pub fn surface(&self) -> (u32, u32) {
        (WIDTH as u32, self.height.ceil() as u32)
    }

    /// What is at `at`.
    pub fn target(&self, at: Point) -> Option<Target> {
        if let Some(i) = self.people.iter().position(|r| r.contains(at)) {
            return Some(Target::Account(i));
        }
        [
            (self.change, Target::Change),
            (Some(self.field), Target::Field),
            (Some(self.details), Target::Details),
            (Some(self.cancel), Target::Cancel),
            (Some(self.authorize), Target::Authorize),
        ]
        .into_iter()
        .find_map(|(rect, target)| rect.filter(|r| r.contains(at)).map(|_| target))
    }
}

/// The line under the field, and its colour.
fn note(dialog: &Dialog, words: Words, caps: bool) -> (String, Color) {
    match (&dialog.said, dialog.phase) {
        (Some((text, error)), _) => (text.clone(), if *error { PEACH } else { DIM }),
        (None, Phase::Checking) => (words.checking().to_owned(), FAINT),
        (None, Phase::Typing) if dialog.wrong_note => (words.wrong(caps), PEACH),
        (None, Phase::Typing) if caps => (words.caps().to_owned(), AMBER),
        _ => (String::new(), FAINT),
    }
}

/// The details' facts, in order.
fn facts(dialog: &Dialog, words: Words) -> Vec<(&'static str, String)> {
    let window = dialog
        .at
        .as_ref()
        .filter(|a| !a.app.is_empty())
        .map(|a| {
            if a.title.is_empty() {
                a.app.clone()
            } else {
                a.title.clone()
            }
        })
        .unwrap_or_else(|| words.none().to_owned());
    vec![
        (words.action(), dialog.request.action.clone()),
        (
            words.program(),
            if dialog.request.program.is_empty() {
                "?".to_owned()
            } else {
                dialog.request.program.clone()
            },
        ),
        (
            words.process(),
            words.started(dialog.request.pid, dialog.request.started_by.as_deref()),
        ),
        (words.window(), window),
    ]
}

/// The dialog's surface, as iced draws it.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
    pub motion: bool,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
}

/// What the dialog last said it shows, for the log.
static SAID: Mutex<Option<Vec<String>>> = Mutex::new(None);

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let now = std::time::Instant::now();
        let redraw = |draw: bool| draw.then(|| canvas::Action::request_redraw().and_capture());
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                let moving = self.motion
                    && super::with_dialog(self.id, |d, _, _| {
                        let busy = |since: Option<std::time::Instant>, ms: f32| {
                            since.is_some_and(|s| {
                                now.saturating_duration_since(s).as_secs_f32() * 1000.0 < ms
                            })
                        };
                        busy(d.shake, SHAKE_MS) || busy(d.turned, TURN_MS)
                    })
                    .unwrap_or(false);
                moving.then(canvas::Action::request_redraw)
            }
            iced::Event::Mouse(seen) => {
                let at = crate::levels::draw::pointer(seen, bounds, cursor, state.at);
                state.at = at;
                let target = super::with_dialog(self.id, |d, caps, waiting| {
                    let layout = Layout::of(d, self.words, waiting, caps);
                    at.and_then(|at| layout.target(at))
                })
                .flatten();
                let pointing = match seen {
                    mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                        Pointing::Over(target)
                    }
                    mouse::Event::CursorLeft => Pointing::Over(None),
                    mouse::Event::ButtonPressed(mouse::Button::Left) => Pointing::Press(target),
                    mouse::Event::ButtonReleased(mouse::Button::Left) => Pointing::Release(target),
                    _ => return None,
                };
                redraw(super::pointer(self.id, pointing, now))
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => redraw(super::key_on(
                self.id,
                key,
                *modifiers,
                text.as_deref(),
                now,
            )),
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let said = super::with_dialog(self.id, |dialog, caps, waiting| {
            self.draw_dialog(&mut frame, dialog, caps, waiting)
        });
        if let Some(said) = said {
            let mut last = SAID.lock().unwrap_or_else(PoisonError::into_inner);
            if last.as_ref() != Some(&said) {
                tracing::debug!(?said, "the authentication dialog shows it");
                *last = Some(said);
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let over = super::with_dialog(self.id, |d, caps, waiting| {
            state
                .at
                .and_then(|at| Layout::of(d, self.words, waiting, caps).target(at))
        })
        .flatten();
        match over {
            Some(Target::Field) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

/// The shake's offset `ms` into it: 10, 9, 6, 4 px from side to side, the
/// mockup's keyframes, eased as `cubic-bezier(0.36,0.07,0.19,0.97)` is
/// near enough (linear between them).
fn shake_at(ms: f32) -> f32 {
    const KEYS: [(f32, f32); 6] = [
        (0.0, 0.0),
        (0.15, -10.0),
        (0.35, 9.0),
        (0.55, -6.0),
        (0.75, 4.0),
        (1.0, 0.0),
    ];
    let t = (ms / SHAKE_MS).clamp(0.0, 1.0);
    KEYS.windows(2).find(|w| t <= w[1].0).map_or(0.0, |w| {
        let k = (t - w[0].0) / (w[1].0 - w[0].0);
        w[0].1 + (w[1].1 - w[0].1) * k
    })
}

fn fill_rounded(f: &mut Frame, rect: Rectangle, radius: f32, colour: Color) {
    f.fill(&kit::rounded(rect, radius), colour);
}

fn rim(f: &mut Frame, rect: Rectangle, radius: f32, colour: Color, width: f32) {
    let inset = Rectangle::new(
        Point::new(rect.x + width / 2.0, rect.y + width / 2.0),
        Size::new(rect.width - width, rect.height - width),
    );
    f.stroke(
        &kit::rounded(inset, radius - width / 2.0),
        Stroke::default().with_color(colour).with_width(width),
    );
}

fn text(f: &mut Frame, content: &str, at: Point, size: f32, colour: Color, font: Font) {
    f.fill_text(kit::text_in(
        font,
        kit::text(content.to_owned(), at, size, colour, Alignment::Left),
    ));
}

fn centred(f: &mut Frame, content: &str, at: Point, size: f32, colour: Color, font: Font) {
    f.fill_text(kit::text_in(
        font,
        kit::text(content.to_owned(), at, size, colour, Alignment::Center),
    ));
}

/// A chevron in a `size` box at `at`, pointing right, turned by `turn`
/// radians: `M9 6 L15 12 L9 18`.
fn chevron(f: &mut Frame, at: Point, size: f32, turn: f32, colour: Color, width: f32) {
    let k = size / 24.0;
    let centre = Point::new(at.x + 12.0 * k, at.y + 12.0 * k);
    let point = |x: f32, y: f32| {
        let (dx, dy) = ((x - 12.0) * k, (y - 12.0) * k);
        let (s, c) = turn.sin_cos();
        Point::new(centre.x + dx * c - dy * s, centre.y + dx * s + dy * c)
    };
    let path = Path::new(|b| {
        b.move_to(point(9.0, 6.0));
        b.line_to(point(15.0, 12.0));
        b.line_to(point(9.0, 18.0));
    });
    f.stroke(&path, stroke(width, colour));
}

/// The system's padlock in its badge: `M8 11 V8 A4 4 0 0 1 16 8 V11` over
/// a 14 × 10 body, 12 px in a 20 px circle.
fn badge(f: &mut Frame, tile: Rectangle) {
    let centre = Point::new(
        tile.x + tile.width + BADGE_OUT - BADGE / 2.0,
        tile.y + tile.height + BADGE_OUT - BADGE / 2.0,
    );
    f.fill(
        &Path::circle(centre, BADGE / 2.0 + BADGE_RING),
        BADGE_RING_COLOUR,
    );
    f.fill(&Path::circle(centre, BADGE / 2.0), BADGE_FILL);
    let k = 12.0 / 24.0;
    let o = Point::new(centre.x - 6.0, centre.y - 6.0);
    let p = |x: f32, y: f32| Point::new(o.x + x * k, o.y + y * k);
    f.fill(
        &kit::rounded(
            Rectangle::new(p(5.0, 11.0), Size::new(14.0 * k, 10.0 * k)),
            2.5 * k,
        ),
        ACCENT,
    );
    let shackle = Path::new(|b| {
        b.move_to(p(8.0, 11.0));
        b.line_to(p(8.0, 8.0));
        b.arc(canvas::path::Arc {
            center: p(12.0, 8.0),
            radius: 4.0 * k,
            start_angle: Radians(std::f32::consts::PI),
            end_angle: Radians(std::f32::consts::TAU),
        });
        b.line_to(p(16.0, 11.0));
    });
    f.stroke(&shackle, stroke(2.6 * k, ACCENT));
}

/// A generic gear: the tile's neutral gradient and a white cog.
fn gear(f: &mut Frame, tile: Rectangle) {
    let fill = gradient::Linear::new(
        tile.position(),
        Point::new(tile.x + tile.width, tile.y + tile.height),
    )
    .add_stop(0.0, GEAR_FROM)
    .add_stop(1.0, GEAR_TO);
    f.fill(&kit::rounded(tile, TILE_RADIUS), canvas::Fill::from(fill));
    let c = tile.center();
    let teeth = 8;
    let cog = Path::new(|b| {
        for i in 0..teeth * 2 {
            let a = i as f32 * std::f32::consts::PI / teeth as f32;
            let r = if i % 2 == 0 { 11.0 } else { 8.5 };
            let at = |a: f32| Point::new(c.x + r * a.cos(), c.y + r * a.sin());
            let half = std::f32::consts::PI / teeth as f32 / 2.0;
            if i == 0 {
                b.move_to(at(a - half));
            } else {
                b.line_to(at(a - half));
            }
            b.line_to(at(a + half));
        }
        b.close();
    });
    f.stroke(&cog, stroke(2.0, Color::WHITE));
    f.stroke(&Path::circle(c, 3.6), stroke(2.0, Color::WHITE));
}

/// The avatar: the user's panda on the accent's gradient; another account
/// its initial on its own.
fn avatar(f: &mut Frame, centre: Point, panda: bool, name: &str) {
    let r = AVATAR / 2.0;
    let (from, to) = if panda {
        (AVATAR_FROM, AVATAR_TO)
    } else {
        (OTHER_FROM, OTHER_TO)
    };
    let fill = gradient::Linear::new(
        Point::new(centre.x - r, centre.y - r),
        Point::new(centre.x + r, centre.y + r),
    )
    .add_stop(0.0, from)
    .add_stop(1.0, to);
    f.fill(&Path::circle(centre, r), canvas::Fill::from(fill));
    if !panda {
        centred(
            f,
            &crate::user::initial(name),
            centre,
            14.0,
            INK,
            MANROPE_BOLD,
        );
        return;
    }
    // The mockup's 24 px panda, of a 30 px box.
    let k = 24.0 / 30.0;
    let o = Point::new(centre.x - 12.0, centre.y - 12.0);
    let p = |x: f32, y: f32| Point::new(o.x + x * k, o.y + y * k);
    f.fill(&Path::circle(p(8.5, 8.5), 4.2 * k), PANDA_DARK);
    f.fill(&Path::circle(p(21.5, 8.5), 4.2 * k), PANDA_DARK);
    f.fill(&Path::circle(p(15.0, 16.5), 10.5 * k), PANDA_LIGHT);
    for (x, turn) in [(10.8, -20.0_f32), (19.2, 20.0)] {
        let eye = Path::new(|b| {
            b.ellipse(Elliptical {
                center: p(x, 16.0),
                radii: iced::Vector::new(2.8 * k, 3.6 * k),
                rotation: Radians(turn.to_radians()),
                start_angle: Radians(0.0),
                end_angle: Radians(std::f32::consts::TAU),
            });
        });
        f.fill(&eye, PANDA_DARK);
    }
    f.fill(&Path::circle(p(11.3, 15.4), k), Color::WHITE);
    f.fill(&Path::circle(p(18.7, 15.4), k), Color::WHITE);
    let nose = Path::new(|b| {
        b.ellipse(Elliptical {
            center: p(15.0, 21.0),
            radii: iced::Vector::new(1.6 * k, 1.1 * k),
            rotation: Radians(0.0),
            start_angle: Radians(0.0),
            end_angle: Radians(std::f32::consts::TAU),
        });
    });
    f.fill(&nose, PANDA_DARK);
}

/// Caps Lock's arrow: `M12 4 L20 12.5 H15.5 V17 H8.5 V12.5 H4 Z M8.5 20
/// H15.5`, 14 px.
fn caps_arrow(f: &mut Frame, at: Point) {
    let k = 14.0 / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let arrow = Path::new(|b| {
        b.move_to(p(12.0, 4.0));
        b.line_to(p(20.0, 12.5));
        b.line_to(p(15.5, 12.5));
        b.line_to(p(15.5, 17.0));
        b.line_to(p(8.5, 17.0));
        b.line_to(p(8.5, 12.5));
        b.line_to(p(4.0, 12.5));
        b.close();
        b.move_to(p(8.5, 20.0));
        b.line_to(p(15.5, 20.0));
    });
    f.stroke(&arrow, stroke(2.0, AMBER));
}

/// A check: `M5 12.5 L10 17.5 L19 7.5`.
fn check(f: &mut Frame, at: Point) {
    let k = CHECK / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(5.0, 12.5));
        b.line_to(p(10.0, 17.5));
        b.line_to(p(19.0, 7.5));
    });
    f.stroke(&path, stroke(2.6, ACCENT));
}

/// The focus ring around `rect`.
fn focus_ring(f: &mut Frame, rect: Rectangle, radius: f32) {
    let out = Rectangle::new(
        Point::new(rect.x - 3.0, rect.y - 3.0),
        Size::new(rect.width + 6.0, rect.height + 6.0),
    );
    rim(f, out, radius + 3.0, FOCUS, 2.0);
}

impl View {
    /// The dialog drawn; what it says, for the log (never the password:
    /// its length only).
    fn draw_dialog(
        &self,
        f: &mut Frame,
        dialog: &Dialog,
        caps: bool,
        waiting: usize,
    ) -> Vec<String> {
        let w = self.words;
        let layout = Layout::of(dialog, w, waiting, caps);
        let size = f.size();
        let now = std::time::Instant::now();
        let mut said = Vec::new();
        let (top, bottom) = if self.glass { GLASS } else { OPAQUE };
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
            canvas::Fill::from(tint),
        );
        rim(f, Rectangle::new(Point::ORIGIN, size), RADIUS, RIM, BORDER);
        // The header.
        let (_, icon) = who(dialog);
        let scale = dialog.at.as_ref().map_or(1.0, |a| a.scale as f32 / 120.0);
        let px = crate::launcher::icons::pixels(ICON_PX, scale);
        match icon.and_then(|name| crate::launcher::icons::of(&name, px).flatten()) {
            Some(handle) => {
                fill_rounded(f, layout.tile, TILE_RADIUS, FILL);
                let inset = (TILE - ICON_PX as f32) / 2.0;
                let rect = Rectangle::new(
                    Point::new(layout.tile.x + inset, layout.tile.y + inset),
                    Size::new(ICON_PX as f32, ICON_PX as f32),
                );
                f.draw_image(rect, canvas::Image::new(handle));
            }
            None => gear(f, layout.tile),
        }
        badge(f, layout.tile);
        text(f, &layout.app, layout.app_at, APP, DIM, MANROPE_SEMIBOLD);
        text(f, w.title(), layout.title_at, TITLE, TEXT, MANROPE_BOLD);
        said.push(w.name(&layout.app));
        // The message.
        for (i, line) in layout.message.iter().enumerate() {
            let middle = layout.message_top + (i as f32 + 0.5) * MESSAGE * MESSAGE_LINE;
            f.fill_text(canvas::Text {
                line_height: iced::widget::text::LineHeight::Relative(MESSAGE_LINE),
                ..kit::text(
                    line.clone(),
                    Point::new(LEFT, middle),
                    MESSAGE,
                    TEXT,
                    Alignment::Left,
                )
            });
        }
        said.push(layout.message.join(" "));
        // The account.
        let checking = dialog.phase == Phase::Checking;
        let me = rustix::process::getuid().as_raw();
        if let Some(account) = dialog.request.account() {
            let centre = Point::new(LEFT + AVATAR / 2.0, layout.account + AVATAR / 2.0);
            avatar(f, centre, account.uid == me, &account.name);
            let x = LEFT + AVATAR + AVATAR_GAP;
            let room = layout.change.map_or(RIGHT, |c| c.x - 8.0) - x;
            let name = kit::elide(&account.name, room, |t| width(t, NAME, MANROPE_BOLD));
            let block = NAME * LINE + ROLE * LINE;
            let top = layout.account + (AVATAR - block) / 2.0;
            text(
                f,
                &name,
                Point::new(x, top + NAME * LINE / 2.0),
                NAME,
                TEXT,
                MANROPE_BOLD,
            );
            text(
                f,
                w.administrator(),
                Point::new(x, top + NAME * LINE + ROLE * LINE / 2.0),
                ROLE,
                FAINT,
                MANROPE,
            );
            said.push(name);
            said.push(w.administrator().to_owned());
        }
        if let Some(change) = layout.change {
            let lit = dialog.hover == Some(Target::Change) && !checking;
            fill_rounded(f, change, CHANGE_RADIUS, if lit { HOVER } else { FILL });
            text(
                f,
                w.change(),
                Point::new(change.x + CHANGE_PAD, change.center_y()),
                CHANGE_TEXT,
                DIM,
                MANROPE_SEMIBOLD,
            );
            chevron(
                f,
                Point::new(
                    change.x + change.width - CHANGE_PAD - CHEVRON,
                    change.center_y() - CHEVRON / 2.0,
                ),
                CHEVRON,
                if dialog.chooser {
                    -std::f32::consts::FRAC_PI_2
                } else {
                    std::f32::consts::FRAC_PI_2
                },
                DIM,
                2.2,
            );
            if dialog.focus == Target::Change {
                focus_ring(f, change, CHANGE_RADIUS);
            }
            said.push(format!("[{}]", w.change()));
        }
        if let Some(list) = layout.list {
            fill_rounded(f, list, LIST_RADIUS, LIST_FILL);
            rim(f, list, LIST_RADIUS, LIST_RIM, BORDER);
            for (i, (row, account)) in layout
                .people
                .iter()
                .zip(&dialog.request.accounts)
                .enumerate()
            {
                let on = i == dialog.request.chosen;
                let lit = dialog.hover == Some(Target::Account(i));
                if on || lit {
                    fill_rounded(f, *row, PERSON_RADIUS, if lit { HOVER } else { FILL });
                }
                text(
                    f,
                    &account.name,
                    Point::new(row.x + PERSON_PAD, row.center_y()),
                    PERSON_TEXT,
                    TEXT,
                    MANROPE_SEMIBOLD,
                );
                if on {
                    check(
                        f,
                        Point::new(
                            row.x + row.width - PERSON_PAD - CHECK,
                            row.center_y() - CHECK / 2.0,
                        ),
                    );
                }
                if dialog.focus == Target::Account(i) {
                    focus_ring(f, *row, PERSON_RADIUS);
                }
                said.push(format!(
                    "{}{}",
                    if on { "(x) " } else { "( ) " },
                    account.name
                ));
            }
        }
        // The field.
        let shake = if self.motion {
            dialog.shake.map_or(0.0, |s| {
                shake_at(now.saturating_duration_since(s).as_secs_f32() * 1000.0)
            })
        } else {
            0.0
        };
        let field = Rectangle::new(
            Point::new(layout.field.x + shake, layout.field.y),
            layout.field.size(),
        );
        fill_rounded(f, field, FIELD_RADIUS, FIELD_FILL);
        let wrong = dialog.wrong_note;
        let border = match dialog.phase {
            Phase::Checking => FIELD_CHECKING,
            Phase::Typing if wrong => FIELD_WRONG,
            Phase::Typing => FIELD_ON,
        };
        rim(f, field, FIELD_RADIUS, border, BORDER);
        let middle = field.center_y();
        let left = field.x + FIELD_LEFT;
        let caps_shown = caps && dialog.phase == Phase::Typing;
        let room =
            field.width - FIELD_LEFT - FIELD_RIGHT - if caps_shown { CAPS + 6.0 } else { 0.0 };
        let typed = dialog.field.chars();
        if typed == 0 {
            // After the caret, never under it.
            text(
                f,
                w.password(),
                Point::new(left + 3.0, middle),
                FIELD_TEXT,
                FAINT,
                MANROPE,
            );
        } else {
            let width = typed as f32 * DOT_ADVANCE;
            let x = left - (width + 2.0 - room).max(0.0);
            for i in 0..typed {
                let centre = Point::new(x + DOT_ADVANCE * (i as f32 + 0.5), middle);
                if centre.x > left - DOT {
                    f.fill(&Path::circle(centre, DOT), TEXT);
                }
            }
        }
        if dialog.focus == Target::Field && !checking {
            let x = left + (typed as f32 * DOT_ADVANCE).min(room - 2.0);
            f.fill_rectangle(Point::new(x, middle - 9.0), Size::new(1.0, 18.0), TEXT);
        }
        if caps_shown {
            let sign = Rectangle::new(
                Point::new(
                    field.x + field.width - FIELD_RIGHT - CAPS,
                    middle - CAPS / 2.0,
                ),
                Size::new(CAPS, CAPS),
            );
            fill_rounded(f, sign, CAPS_RADIUS, AMBER_FILL);
            caps_arrow(f, Point::new(sign.x + 4.0, sign.y + 4.0));
            said.push("[caps]".to_owned());
        }
        let name = dialog
            .request
            .account()
            .map(|a| a.name.clone())
            .unwrap_or_default();
        said.push(format!("{}: {typed}", w.password_of(&name)));
        // The line under it.
        for (i, line) in layout.note.iter().enumerate() {
            let middle = layout.note_top + (i as f32 + 0.5) * NOTE * NOTE_LINE;
            f.fill_text(canvas::Text {
                line_height: iced::widget::text::LineHeight::Relative(NOTE_LINE),
                ..kit::text(
                    line.clone(),
                    Point::new(LEFT, middle),
                    NOTE,
                    layout.note_colour,
                    Alignment::Left,
                )
            });
        }
        if !layout.note.is_empty() {
            said.push(layout.note.join(" "));
        }
        // Details.
        let lit = dialog.hover == Some(Target::Details);
        if lit {
            fill_rounded(f, layout.details, DETAILS_RADIUS, FILL);
        }
        let turned = dialog.turned.map_or(1.0, |t| {
            if self.motion {
                (now.saturating_duration_since(t).as_secs_f32() * 1000.0 / TURN_MS).clamp(0.0, 1.0)
            } else {
                1.0
            }
        });
        let turn = if dialog.details { turned } else { 1.0 - turned };
        chevron(
            f,
            Point::new(
                layout.details.x + 4.0,
                layout.details.center_y() - DETAILS_CHEVRON / 2.0,
            ),
            DETAILS_CHEVRON,
            turn * std::f32::consts::FRAC_PI_2,
            DIM,
            2.4,
        );
        text(
            f,
            w.details(),
            Point::new(
                layout.details.x + 4.0 + DETAILS_CHEVRON + 4.0,
                layout.details.center_y(),
            ),
            DETAILS_TEXT,
            DIM,
            MANROPE_SEMIBOLD,
        );
        if dialog.focus == Target::Details {
            focus_ring(f, layout.details, DETAILS_RADIUS);
        }
        said.push(format!(
            "[{}{}]",
            if dialog.details { "v " } else { "> " },
            w.details()
        ));
        if let Some(rect) = layout.facts_box {
            fill_rounded(f, rect, FACTS_RADIUS, LIST_FILL);
            for fact in &layout.facts {
                text(
                    f,
                    fact.label,
                    Point::new(LEFT + FACTS_PAD_X, fact.top + VALUE * VALUE_LINE / 2.0),
                    LABEL,
                    FAINT,
                    MANROPE,
                );
                for (i, line) in fact.lines.iter().enumerate() {
                    let middle = fact.top + (i as f32 + 0.5) * VALUE * VALUE_LINE;
                    f.fill_text(canvas::Text {
                        line_height: iced::widget::text::LineHeight::Relative(VALUE_LINE),
                        ..kit::text_in(
                            MARTIAN,
                            kit::text(
                                line.clone(),
                                Point::new(LEFT + FACTS_PAD_X + LABEL_COLUMN + LABEL_GAP, middle),
                                VALUE,
                                VALUE_INK,
                                Alignment::Left,
                            ),
                        )
                    });
                }
                said.push(format!("{}: {}", fact.label, fact.lines.concat()));
            }
        }
        // The footer.
        let footer_middle = layout.footer + BUTTON / 2.0;
        if !layout.waiting.is_empty() {
            text(
                f,
                &layout.waiting,
                Point::new(LEFT, footer_middle),
                WAITING,
                FAINT,
                MANROPE,
            );
            said.push(layout.waiting.clone());
        }
        let lit = dialog.hover == Some(Target::Cancel);
        fill_rounded(
            f,
            layout.cancel,
            BUTTON_RADIUS,
            if lit { HOVER } else { FILL },
        );
        centred(
            f,
            w.cancel(),
            layout.cancel.center(),
            BUTTON_TEXT,
            TEXT,
            MANROPE_SEMIBOLD,
        );
        if dialog.focus == Target::Cancel {
            focus_ring(f, layout.cancel, BUTTON_RADIUS);
        }
        let ready = !dialog.field.is_empty() && !checking;
        let (fill, ink) = if checking {
            (CHECKING_FILL, TEXT)
        } else if ready {
            (ACCENT, INK)
        } else {
            (READY_OFF, FAINT)
        };
        fill_rounded(f, layout.authorize, BUTTON_RADIUS, fill);
        let mut word_x = layout.authorize.x + BUTTON_PAD;
        if checking {
            let c = Point::new(word_x + SPINNER / 2.0, footer_middle);
            f.stroke(
                &Path::circle(c, SPINNER / 2.0 - 1.0),
                stroke(2.0, Color { a: 0.22, ..TEXT }),
            );
            let arc = Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: c,
                    radius: SPINNER / 2.0 - 1.0,
                    start_angle: Radians(-std::f32::consts::FRAC_PI_2),
                    end_angle: Radians(0.0),
                });
            });
            f.stroke(&arc, stroke(2.0, TEXT));
            word_x += SPINNER + 8.0;
        }
        text(
            f,
            w.authorize(),
            Point::new(word_x, footer_middle),
            BUTTON_TEXT,
            ink,
            MANROPE_BOLD,
        );
        if dialog.focus == Target::Authorize {
            focus_ring(f, layout.authorize, BUTTON_RADIUS);
        }
        said.push(format!("[{}]", w.cancel()));
        said.push(format!(
            "[{}{}]",
            w.authorize(),
            if ready { "" } else { " (inert)" }
        ));
        said
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shake_goes_side_to_side_and_back() {
        assert_eq!(shake_at(0.0), 0.0);
        assert!((shake_at(0.15 * SHAKE_MS) + 10.0).abs() < 1e-3);
        assert!((shake_at(0.35 * SHAKE_MS) - 9.0).abs() < 1e-3);
        assert_eq!(shake_at(SHAKE_MS), 0.0);
        assert_eq!(shake_at(2.0 * SHAKE_MS), 0.0);
    }
}
