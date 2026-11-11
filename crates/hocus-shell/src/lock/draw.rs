//! The lock on one screen, variant A's sizes (logical pixels, the mockup's
//! divided by its ×1.5): the desktop's background with `rgba(5,7,12,0.45)`
//! over it; the clock (Martian Mono 112 px in fixed cells, the colon 0.55
//! of a digit) and its date 16 px under it (Manrope 22 px, `text.dim`),
//! its top 330 px above the middle (90 px on the other screens, where it
//! is alone); and on the form's screen:
//!
//! - the form, 320 px wide, its top 110 px above the middle: the avatar
//!   (88 px round, the accent's gradient, a 2 px ring and a soft shadow,
//!   the panda's face of 66 px on it, or the account's photo), the name
//!   14 px under it (Manrope 700, 20 px), the field 20 px under (320 × 44,
//!   radius 12, its border by state, a dot a character, the Caps Lock sign
//!   and the 34 px button), the line 12 px under it (13.5 px on 1.45, two
//!   lines kept), and the agents waiting, a count in an amber pill;
//! - the status row at the bottom right (the layout's button, the battery,
//!   the Wi-Fi), 32 px from the right, its centre 44 px over the bottom;
//! - two pandas asleep in the bottom left corner, 34 px in, 26 px up:
//!   the validated drawing, 132 and 108 px (#649), the second turned 14°
//!   back; as the lock shows they breathe twice and their "z" rise (8 s),
//!   then lie still: the lock draws once a minute again. The agents' pill
//!   holds the waiting panda (34 px), playing its mood meanwhile. Still
//!   with reduced motion, in power saver or on the CPU.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{LOCK_SHAKE, LockFocus, LockPhase};
use iced::{
    Color, Font, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{image::FilterMethod, svg},
    alignment, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Lock, Words};
use crate::pandas::{self, BREATH, Burst, Mood};
use crate::theme::Ink;
use crate::{
    clock::time::{Lang, Shown, Wall},
    levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::MANROPE_SEMIBOLD,
};

const MARTIAN: Font = Font::with_name("Martian Mono");

/// The background's veil.
const VEIL: Ink = Ink::Dark(Color::from_rgba8(5, 7, 12, 0.45));
/// The background without the gradient (`plain`): the compositor's base.
const PLAIN: Ink = Ink::Dark(Color::from_rgb(0.055, 0.059, 0.071));

/// The clock: 112 px, line height 1; the date 16 px under, 22 px.
const CLOCK: f32 = 112.0;
const COLON: f32 = 0.55;
const DATE: f32 = 22.0;
/// Over 12 hours, between the minutes and AM or PM.
const HALF_GAP: f32 = 10.0;
const DATE_GAP: f32 = 16.0;
const LINE: f32 = 1.366;
/// The clock's top above the middle: with the form, alone.
const CLOCK_ABOVE: f32 = 330.0;
const CLOCK_ALONE_ABOVE: f32 = 90.0;

/// The form.
const FORM_ABOVE: f32 = 110.0;
const FORM_WIDTH: f32 = 320.0;
const AVATAR: f32 = 88.0;
const FACE: f32 = 66.0;
const NAME: f32 = 20.0;
const NAME_GAP: f32 = 14.0;
const FIELD_GAP: f32 = 20.0;
const FIELD_HEIGHT: f32 = 44.0;
const FIELD_RADIUS: f32 = 12.0;
const FIELD_PAD: f32 = 14.0;
const BUTTON: f32 = 34.0;
const BUTTON_IN: f32 = 5.0;
const BUTTON_RADIUS: f32 = 9.0;
const CAPS: f32 = 22.0;
const GAP: f32 = 6.0;
const TEXT_SIZE: f32 = 16.0;
/// A dot a character: its radius and its advance (16 px, spaced 0.1 em).
const DOT: f32 = 3.5;
const DOT_ADVANCE: f32 = 10.6;
const MESSAGE_GAP: f32 = 12.0;
const MESSAGE: f32 = 13.5;
const MESSAGE_LINE: f32 = 1.45;
const MESSAGE_ROOM: f32 = 40.0;
/// The agents' pill (variant C's line, decided for A): 34 px high.
const AGENTS_GAP: f32 = 12.0;
const AGENTS_HEIGHT: f32 = 34.0;
const AGENTS_TEXT: f32 = 13.0;

/// The status row.
const ROW_RIGHT: f32 = 32.0;
const ROW_ABOVE: f32 = 44.0;
const ROW_GAP: f32 = 18.0;
const LAYOUT_SIZE: Size = Size::new(44.0, 32.0);
const LAYOUT_RADIUS: f32 = 8.0;
const ROW_TEXT: f32 = 14.0;

/// The pandas asleep: 34 px from the left, 26 px from the bottom; 132
/// and 108 px (#649).
const PANDAS_LEFT: f32 = 34.0;
const PANDAS_BOTTOM: f32 = 26.0;
const PANDA_NEAR: f32 = 132.0;
const PANDA_FAR: f32 = 108.0;
/// The agents' pill: its panda 34 px, 6 px in.
const AGENTS_PANDA: f32 = 34.0;
const AGENTS_IN: f32 = 6.0;

const ACCENT: Ink = Ink::Accent(1.0);
const ACCENT_TO: Ink = Ink::Dark(rgb(0x3E8FBF));
const FIELD_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const BORDER_KEYS: Ink = Ink::Accent(0.60);
const BORDER_WRONG: Ink = Ink::Dark(rgb(0xF2945B));
const BORDER_IDLE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.16));
const BORDER_WAITING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const AMBER: Ink = Ink::Dark(rgb(0xF5C46E));
const WRONG_TEXT: Ink = Ink::Dark(rgb(0xF7B98A));
const INK: Ink = Ink::Dark(rgb(0x0B1020));
const LIT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const RING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.18));
const PANDA_DARK: Ink = Ink::Fixed(rgb(0x1D2230));
const PANDA_LIGHT: Ink = Ink::Fixed(rgb(0xF2F4F7));

/// What a canvas of the lock draws: iced draws a canvas' images over its
/// shapes, so the background's picture is a canvas under the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The desktop's background.
    Back,
    /// The veil over it, and everything else.
    Front,
}

/// What the lock on screen `output` draws under its controls
/// ([`super::view`]).
#[derive(Debug, Clone)]
pub struct View {
    pub layer: Layer,
    pub output: String,
    pub wall: Wall,
    pub lang: Lang,
    pub words: Words,
}

/// The canvas' state: the clock's font measured.
#[derive(Debug, Default)]
pub struct State {
    zero: Option<f32>,
    /// Since when the pandas breathe, from the lock's first frame (#649).
    shown: Option<Instant>,
}

/// The corner's pandas breathe twice as the lock shows.
fn breaths(since: Instant) -> Burst {
    Burst::lasting(since, BREATH * 2)
}

/// Where everything is on a screen `size` large.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    clock_top: f32,
    avatar: Rectangle,
    name_top: f32,
    pub field: Rectangle,
    pub button: Rectangle,
    caps: Rectangle,
    pub message: Rectangle,
    pub agents_top: f32,
    pub row_centre: f32,
}

impl Layout {
    pub fn of(size: Size, form: bool) -> Self {
        let cx = (size.width / 2.0).round();
        let cy = (size.height / 2.0).round();
        let clock_top = cy - if form { CLOCK_ABOVE } else { CLOCK_ALONE_ABOVE };
        let top = cy - FORM_ABOVE;
        let left = cx - FORM_WIDTH / 2.0;
        let avatar = Rectangle::new(
            Point::new(cx - AVATAR / 2.0, top),
            Size::new(AVATAR, AVATAR),
        );
        let name_top = top + AVATAR + NAME_GAP;
        let field_top = name_top + NAME * LINE + FIELD_GAP;
        let field = Rectangle::new(
            Point::new(left, field_top),
            Size::new(FORM_WIDTH, FIELD_HEIGHT),
        );
        let button = Rectangle::new(
            Point::new(
                field.x + field.width - BUTTON_IN - BUTTON,
                field.y + (FIELD_HEIGHT - BUTTON) / 2.0,
            ),
            Size::new(BUTTON, BUTTON),
        );
        let caps = Rectangle::new(
            Point::new(button.x - GAP - CAPS, field.y + (FIELD_HEIGHT - CAPS) / 2.0),
            Size::new(CAPS, CAPS),
        );
        let message = Rectangle::new(
            Point::new(left, field.y + FIELD_HEIGHT + MESSAGE_GAP),
            Size::new(FORM_WIDTH, MESSAGE_ROOM),
        );
        let row_centre = size.height - ROW_ABOVE;
        Self {
            clock_top,
            avatar,
            name_top,
            field,
            button,
            caps,
            message,
            agents_top: message.y + MESSAGE_ROOM + AGENTS_GAP,
            row_centre,
        }
    }
}

impl Layout {
    /// The same, in a frame whose origin is `origin` (a control's): a
    /// canvas' clip does not follow its translation.
    pub fn from(&self, origin: Point) -> Self {
        let by = Vector::new(-origin.x, -origin.y);
        Self {
            clock_top: self.clock_top - origin.y,
            avatar: self.avatar + by,
            name_top: self.name_top - origin.y,
            field: self.field + by,
            button: self.button + by,
            caps: self.caps + by,
            message: self.message + by,
            agents_top: self.agents_top - origin.y,
            row_centre: self.row_centre - origin.y,
        }
    }
}

/// The status row's items, right to left: the Wi-Fi, the battery, the
/// layout's button.
pub struct Row {
    wifi: Option<(Rectangle, crate::network::model::Link)>,
    battery: Option<(Rectangle, crate::levels::model::Battery, String)>,
    pub layout: Option<(Rectangle, String)>,
}

pub fn row(size: Size, centre: f32) -> Row {
    use crate::network::model::Link;
    let mut right = size.width - ROW_RIGHT;
    let link = crate::network::snapshot().link();
    let wifi = (!matches!(link, Link::Hidden)).then(|| {
        let rect = Rectangle::new(
            Point::new(right - 20.0, centre - 10.0),
            Size::new(20.0, 20.0),
        );
        right = rect.x - ROW_GAP;
        (rect, link)
    });
    let battery = crate::levels::snapshot().battery.map(|battery| {
        let text = format!("{} %", battery.shown());
        let width = 24.0 + 6.0 + crate::popover::kit::measure(&text, ROW_TEXT, MANROPE_BOLD);
        let rect = Rectangle::new(
            Point::new(right - width, centre - 10.0),
            Size::new(width, 20.0),
        );
        right = rect.x - ROW_GAP;
        (rect, battery, text)
    });
    let layout = crate::layouts::snapshot().shown().map(|layout| {
        let width = LAYOUT_SIZE
            .width
            .max(20.0 + crate::popover::kit::measure(&layout.code, ROW_TEXT, MANROPE_BOLD));
        let rect = Rectangle::new(
            Point::new(right - width, centre - LAYOUT_SIZE.height / 2.0),
            Size::new(width, LAYOUT_SIZE.height),
        );
        (rect, layout.code.clone())
    });
    Row {
        wifi,
        battery,
        layout,
    }
}

/// Whether screen `output` shows the form: the one the compositor names,
/// else the first screen when that one has no surface.
pub fn form(lock: &Lock, output: &str) -> bool {
    if output == lock.shown.output {
        return true;
    }
    if lock.surfaces.contains_key(&lock.shown.output) {
        return false;
    }
    lock.surfaces.keys().min().map(String::as_str) == Some(output)
}

/// The next minute's start, a little past it, from `now`.
fn until_next_minute(wall: &Wall) -> Duration {
    let now = wall.now();
    let into = Duration::from_secs(u64::from(now.second().unsigned_abs()))
        + crate::clock::time::subsec(&now);
    Duration::from_secs(60).saturating_sub(into) + Duration::from_millis(20)
}

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.layer == Layer::Back {
            return None;
        }
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                if state.zero.is_none()
                    && let Ok(mut system) = iced::advanced::graphics::text::font_system().write()
                {
                    state.zero =
                        crate::clock::measure(system.raw(), MARTIAN, CLOCK, CLOCK).map(|m| m.zero);
                }
                if state.shown.is_none() && pandas::lively() {
                    state.shown = Some(*now);
                }
                let breathing = state.shown.is_some_and(|t| breaths(t).plays(*now));
                let moving = super::with(|l| {
                    let l = l.as_ref()?;
                    let shaking = l.shake.is_some_and(|(_, since)| {
                        now.saturating_duration_since(since) < LOCK_SHAKE
                    });
                    let spinning = l.checking.is_some() && l.shown.motion;
                    Some(shaking || spinning || breathing)
                })?;
                Some(if moving {
                    canvas::Action::request_redraw()
                } else {
                    canvas::Action::request_redraw_at(
                        Instant::now() + until_next_minute(&self.wall),
                    )
                })
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let Some(lock) = super::with(|l| l.clone()) else {
            return vec![frame.into_geometry()];
        };
        let size = bounds.size();
        let now = Instant::now();
        if self.layer == Layer::Back {
            background(&mut frame, size, lock.shown.wallpaper, &self.output);
            return vec![frame.into_geometry()];
        }
        frame.fill_rectangle(Point::ORIGIN, size, VEIL.get());
        let form = form(&lock, &self.output);
        let layout = Layout::of(size, form);
        let shown = Shown::at(&self.wall.now(), self.lang);
        draw_clock(
            &mut frame,
            size.width / 2.0,
            layout.clock_top,
            &shown,
            state.zero,
        );
        if form {
            draw_form(&mut frame, &lock, &layout, self.words, (now, state.shown));
            draw_row(&mut frame, size, layout.row_centre);
            draw_pandas(&mut frame, size, now, state.shown);
        }
        let said = Said {
            output: self.output.clone(),
            form,
            phase: lock.shown.phase,
            chars: lock.shown.chars,
            caps: lock.shown.caps,
            hm: shown.time(),
            photo: super::avatar::photo().is_some(),
        };
        let mut last = SAID.lock().unwrap_or_else(PoisonError::into_inner);
        let last = last.get_or_insert_with(HashMap::new);
        if last.get(&self.output) != Some(&said) {
            tracing::debug!(?said, "the lock is drawn");
            last.insert(self.output.clone(), said);
        }
        vec![frame.into_geometry()]
    }
}

/// What a lock surface last showed, as its log line says it: the tests
/// read it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct Said {
    output: String,
    form: bool,
    phase: LockPhase,
    chars: u32,
    caps: bool,
    hm: String,
    photo: bool,
}

static SAID: Mutex<Option<HashMap<String, Said>>> = Mutex::new(None);

/// The screen's wallpaper, or the lock's own picture (6.3), drawn off
/// this thread ([`super::wallpaper`]); plain until it is.
fn background(f: &mut Frame, size: Size, wallpaper: bool, output: &str) {
    let all = Rectangle::new(Point::ORIGIN, size);
    let key = (size.width.round() as u32, size.height.round() as u32);
    match wallpaper
        .then(|| super::wallpaper::background(output, key))
        .flatten()
    {
        Some(handle) => f.draw_image(
            all,
            canvas::Image::new(handle).filter_method(FilterMethod::Linear),
        ),
        None => f.fill_rectangle(Point::ORIGIN, size, PLAIN.get()),
    }
}

fn text(
    content: String,
    at: Point,
    size: f32,
    font: Font,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position: at,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font,
        align_x: align,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
    }
}

/// `HH:MM` in its cells, centred on `cx`, and the date under it.
fn draw_clock(f: &mut Frame, cx: f32, top: f32, shown: &Shown, zero: Option<f32>) {
    let zero = zero.unwrap_or(CLOCK * 0.6);
    let cells: Vec<f32> = shown
        .hm
        .iter()
        .map(|c| match c {
            ':' => zero * COLON,
            ' ' => 0.0,
            _ => zero,
        })
        .collect();
    // Over 12 hours, AM or PM after the minutes, at the date's size, on
    // their baseline's side.
    let half = shown
        .half
        .map(|h| (h, HALF_GAP + 2.0 * zero * DATE / CLOCK * 1.25));
    let width: f32 = cells.iter().sum::<f32>() + half.map_or(0.0, |(_, w)| w);
    let mut x = cx - width / 2.0;
    for (ch, w) in shown.hm.iter().zip(&cells) {
        let mut glyph = text(
            ch.to_string(),
            Point::new(x + w / 2.0, top),
            CLOCK,
            MARTIAN,
            crate::theme::digit_colour(*ch, TEXT.get()),
            Alignment::Center,
        );
        glyph.line_height = LineHeight::Absolute(Pixels(CLOCK));
        // The theme's glow behind it.
        crate::theme::glow(*ch, |offset, colour| {
            f.fill_text(canvas::Text {
                position: glyph.position + offset,
                color: colour,
                ..glyph.clone()
            });
        });
        f.fill_text(glyph);
        x += w;
    }
    if let Some((half, _)) = half {
        f.fill_text(text(
            half.to_owned(),
            Point::new(x + HALF_GAP, top + CLOCK - DATE * 1.6),
            DATE,
            MARTIAN,
            DIM.get(),
            Alignment::Left,
        ));
    }
    f.fill_text(text(
        shown.date.clone(),
        Point::new(cx, top + CLOCK + DATE_GAP),
        DATE,
        MANROPE,
        DIM.get(),
        Alignment::Center,
    ));
}

fn draw_form(
    f: &mut Frame,
    lock: &Lock,
    layout: &Layout,
    words: Words,
    (now, breathing): (Instant, Option<Instant>),
) {
    let shown = &lock.shown;
    if crate::theme::matrix() {
        draw_card(f, layout);
    }
    draw_avatar(f, layout.avatar);
    f.fill_text(text(
        crate::user::name(),
        Point::new(layout.avatar.center_x(), layout.name_top),
        NAME,
        MANROPE_BOLD,
        TEXT.get(),
        Alignment::Center,
    ));
    // The line under the field.
    let (message, colour) = message(lock, words);
    if !message.is_empty() {
        let mut line = text(
            message,
            Point::new(layout.message.center_x(), layout.message.y),
            MESSAGE,
            MANROPE,
            colour.get(),
            Alignment::Center,
        );
        line.line_height = LineHeight::Relative(MESSAGE_LINE);
        line.max_width = layout.message.width;
        f.fill_text(line);
    }
    if shown.agents > 0 {
        draw_agents(
            f,
            words.agents(shown.agents),
            (layout.message.center_x(), layout.agents_top),
            now,
            breathing.map(|t| Burst::of(Mood::Waiting, t)),
        );
    }
}

/// The line under the field, and its colour: wrong, checked, the wait,
/// Caps Lock.
pub fn message(lock: &Lock, words: Words) -> (String, Ink) {
    let shown = &lock.shown;
    match shown.phase {
        LockPhase::Wrong => (words.wrong(shown.caps), WRONG_TEXT),
        LockPhase::Checking => (words.checking().to_owned(), FAINT),
        LockPhase::Waiting(seconds) => (words.wait(seconds), AMBER),
        _ if caps_shown(lock) => (words.caps().to_owned(), AMBER),
        _ => (String::new(), DIM),
    }
}

/// Whether Caps Lock's sign shows in the field.
fn caps_shown(lock: &Lock) -> bool {
    lock.shown.caps
        && matches!(
            lock.shown.phase,
            LockPhase::Idle | LockPhase::Typing | LockPhase::Wrong
        )
}

/// The field shaken after a wrong password, `now`: its offset.
pub fn shaken(lock: &Lock, now: Instant) -> f32 {
    lock.shake
        .filter(|(shake, _)| *shake == lock.shown.shake && lock.shown.phase == LockPhase::Wrong)
        .map_or(0.0, |(_, since)| {
            shake_at(now.saturating_duration_since(since))
        })
}

/// While it waits, the field and its button are faded.
fn alpha(lock: &Lock) -> f32 {
    if matches!(lock.shown.phase, LockPhase::Waiting(_)) {
        0.55
    } else {
        1.0
    }
}

/// The field at `layout`'s, in the screen's frame: its box, its border by
/// state, a dot a character (or its placeholder) and Caps Lock's sign.
pub fn draw_field(f: &mut Frame, lock: &Lock, layout: &Layout, words: Words) {
    let shown = &lock.shown;
    let alpha = alpha(lock);
    let field = layout.field;
    let keys = shown.focus == LockFocus::Field && shown.phase.takes_keys();
    let border = match shown.phase {
        LockPhase::Wrong => BORDER_WRONG,
        LockPhase::Waiting(_) => BORDER_WAITING,
        _ if keys => BORDER_KEYS,
        _ => BORDER_IDLE,
    };
    let shape = Path::rounded_rectangle(field.position(), field.size(), FIELD_RADIUS.into());
    f.fill(&shape, faded(FIELD_FILL.get(), alpha));
    let rim = Path::rounded_rectangle(
        Point::new(field.x + 0.5, field.y + 0.5),
        Size::new(field.width - 1.0, field.height - 1.0),
        (FIELD_RADIUS - 0.5).into(),
    );
    f.stroke(
        &rim,
        Stroke::default()
            .with_color(faded(border.get(), alpha))
            .with_width(1.0),
    );
    let caps = caps_shown(lock);
    let room_right = if caps {
        layout.caps.x - GAP
    } else {
        layout.button.x - GAP
    };
    let inside = Rectangle::new(
        Point::new(field.x + FIELD_PAD, field.y),
        Size::new((room_right - field.x - FIELD_PAD).max(0.0), field.height),
    );
    if shown.chars == 0 {
        if !matches!(shown.phase, LockPhase::Checking) {
            f.with_clip(inside, |f| {
                f.fill_text(text(
                    words.password().to_owned(),
                    Point::new(inside.x, field.center_y() - TEXT_SIZE * LINE / 2.0),
                    TEXT_SIZE,
                    MANROPE,
                    faded(FAINT.get(), alpha),
                    Alignment::Left,
                ));
            });
        }
    } else {
        // The last dots when they do not all fit.
        let fit = ((inside.width - DOT) / DOT_ADVANCE).floor().max(0.0) as u32 + 1;
        let shown_dots = shown.chars.min(fit);
        for i in 0..shown_dots {
            let centre = Point::new(inside.x + DOT + i as f32 * DOT_ADVANCE, field.center_y());
            f.fill(&Path::circle(centre, DOT), faded(TEXT.get(), alpha));
        }
    }
    if caps {
        draw_caps(f, layout.caps);
    }
}

/// Whether the button's spinner turns.
pub fn spinning(lock: &Lock) -> bool {
    lock.checking.is_some() && lock.shown.motion
}

/// The layout's button at `rect`, its code, lit under the pointer.
pub fn draw_layout(f: &mut Frame, rect: Rectangle, code: &str, lit: bool) {
    f.fill(
        &Path::rounded_rectangle(rect.position(), rect.size(), LAYOUT_RADIUS.into()),
        if lit { LIT } else { FIELD_FILL },
    );
    f.fill_text(text(
        code.to_owned(),
        Point::new(rect.center_x(), rect.center_y() - ROW_TEXT * LINE / 2.0),
        ROW_TEXT,
        MANROPE_BOLD,
        TEXT.get(),
        Alignment::Center,
    ));
}

/// The field's shake at `t`: 10, 9, 6, 4 px side to side over
/// [`LOCK_SHAKE`], each step `cubic-bezier(0.36,0.07,0.19,0.97)`.
/// Matrix's card under the form (#431): the popovers' glass tint and rim,
/// its green hairline atop and its code edge down its left side.
fn draw_card(f: &mut Frame, layout: &Layout) {
    const AROUND: f32 = 26.0;
    const RADIUS: f32 = 18.0;
    let top = layout.avatar.y - 18.0;
    let rect = Rectangle::new(
        Point::new(layout.field.x - AROUND, top),
        Size::new(
            layout.field.width + 2.0 * AROUND,
            layout.field.y + layout.field.height + 20.0 - top,
        ),
    );
    let shape = Path::rounded_rectangle(rect.position(), rect.size(), RADIUS.into());
    let tint = gradient::Linear::new(rect.position(), Point::new(rect.x, rect.y + rect.height))
        .add_stop(0.0, Ink::Token(|t| t.popover_top).get())
        .add_stop(1.0, Ink::Token(|t| t.popover_bottom).get());
    f.fill(&shape, tint);
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(rect.x + 0.5, rect.y + 0.5),
            Size::new(rect.width - 1.0, rect.height - 1.0),
            (RADIUS - 0.5).into(),
        ),
        Stroke::default()
            .with_color(Ink::Token(|t| t.bar_border).get())
            .with_width(1.0),
    );
    crate::theme::hairline(f, rect.x + 14.0, rect.x + rect.width - 14.0, rect.y);
    crate::theme::code_edge(
        f,
        Rectangle::new(
            Point::new(rect.x + 1.0, rect.y + 16.0),
            Size::new(8.0, rect.height - 32.0),
        ),
        9.0,
    );
}

pub fn shake_at(t: Duration) -> f32 {
    const STEPS: [(f32, f32); 6] = [
        (0.0, 0.0),
        (0.15, -10.0),
        (0.35, 9.0),
        (0.55, -6.0),
        (0.75, 4.0),
        (1.0, 0.0),
    ];
    let p = t.as_secs_f32() / LOCK_SHAKE.as_secs_f32();
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    for pair in STEPS.windows(2) {
        let ((a, x0), (b, x1)) = (pair[0], pair[1]);
        if p <= b {
            let u = ((p - a) / (b - a)).clamp(0.0, 1.0);
            let e = bezier((0.36, 0.07, 0.19, 0.97), u);
            return x0 + (x1 - x0) * e;
        }
    }
    0.0
}

/// A CSS `cubic-bezier` at progress `x`.
fn bezier((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    let axis = |p1: f32, p2: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..30 {
        t = (lo + hi) / 2.0;
        if axis(x1, x2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    axis(y1, y2, t)
}

fn faded(colour: Color, alpha: f32) -> Color {
    Color {
        a: colour.a * alpha,
        ..colour
    }
}

/// The button at `rect`, at `now`: the arrow, or the spinner while the
/// password is checked.
pub fn draw_button(f: &mut Frame, lock: &Lock, rect: Rectangle, now: Instant) {
    let alpha = alpha(lock);
    let shown = &lock.shown;
    let has = shown.chars > 0;
    let checking = shown.phase == LockPhase::Checking;
    let waiting = matches!(shown.phase, LockPhase::Waiting(_));
    let fill = if checking {
        crate::theme::ink(Color::from_rgba8(255, 255, 255, 0.10))
    } else if has && !waiting {
        ACCENT.get()
    } else {
        FIELD_FILL.get()
    };
    f.fill(
        &Path::rounded_rectangle(rect.position(), rect.size(), BUTTON_RADIUS.into()),
        faded(fill, alpha),
    );
    let centre = rect.center();
    if checking {
        // A 14 px spinner, a quarter lit, turning every 800 ms.
        let turn = lock.checking.filter(|_| shown.motion).map_or(0.0, |since| {
            let t = now.saturating_duration_since(since).as_secs_f32() / 0.8;
            t.fract() * std::f32::consts::TAU
        });
        let r = 6.0;
        f.stroke(
            &Path::circle(centre, r),
            Stroke::default()
                .with_color(crate::theme::ink(Color::from_rgba8(238, 241, 246, 0.22)))
                .with_width(2.0),
        );
        let start = -std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_4 + turn;
        let arc = Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: centre,
                radius: r,
                start_angle: Radians(start),
                end_angle: Radians(start + std::f32::consts::FRAC_PI_2),
            });
        });
        f.stroke(
            &arc,
            Stroke::default().with_color(TEXT.get()).with_width(2.0),
        );
        return;
    }
    // The arrow: `M5 12 H19 M13 6 L19 12 L13 18` in 24, 16 px, 2.4 px.
    let colour = if has && !waiting { INK } else { FAINT };
    let s = 16.0 / 24.0;
    let at = Point::new(centre.x - 8.0, centre.y - 8.0);
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let arrow = Path::new(|b| {
        b.move_to(p(5.0, 12.0));
        b.line_to(p(19.0, 12.0));
        b.move_to(p(13.0, 6.0));
        b.line_to(p(19.0, 12.0));
        b.line_to(p(13.0, 18.0));
    });
    f.stroke(
        &arrow,
        Stroke {
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
                .with_color(faded(colour.get(), alpha))
                .with_width(2.4 * s)
        },
    );
}

/// Caps Lock's sign: 22 × 22, radius 6, its arrow in amber.
fn draw_caps(f: &mut Frame, rect: Rectangle) {
    f.fill(
        &Path::rounded_rectangle(rect.position(), rect.size(), 6.0.into()),
        crate::theme::ink(Color::from_rgba8(245, 196, 110, 0.16)),
    );
    // `M12 4 L20 12.5 H15.5 V17 H8.5 V12.5 H4 Z M8.5 20 H15.5`, 14 px.
    let s = 14.0 / 24.0;
    let at = Point::new(rect.center_x() - 7.0, rect.center_y() - 7.0);
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let sign = Path::new(|b| {
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
    f.stroke(
        &sign,
        Stroke {
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
                .with_color(AMBER.get())
                .with_width(2.0 * s)
        },
    );
}

/// The avatar: the account's photo, else the panda on the accent's
/// gradient; a 2 px ring and a soft shadow.
fn draw_avatar(f: &mut Frame, rect: Rectangle) {
    let centre = rect.center();
    let r = rect.width / 2.0;
    // `0 10px 30px rgba(0,0,0,0.35)`: rings fading out, 10 px down.
    for i in (1..=8).rev() {
        let k = i as f32 / 8.0;
        f.fill(
            &Path::circle(Point::new(centre.x, centre.y + 10.0), r + 15.0 * k),
            Color::from_rgba8(0, 0, 0, 0.35 / 8.0 * (1.0 - k * 0.7)),
        );
    }
    f.fill(&Path::circle(centre, r + 2.0), RING.get());
    match super::avatar::photo() {
        Some(photo) => {
            let mut image = canvas::Image::new(photo).filter_method(FilterMethod::Linear);
            image.border_radius = r.into();
            f.draw_image(rect, image);
        }
        None => {
            let fill = gradient::Linear::new(
                rect.position(),
                Point::new(rect.x + rect.width, rect.y + rect.height),
            )
            .add_stop(0.0, ACCENT.get())
            .add_stop(1.0, ACCENT_TO.get());
            f.fill(&Path::circle(centre, r), canvas::Fill::from(fill));
            panda(
                f,
                Point::new(centre.x - FACE / 2.0, centre.y - FACE / 2.0),
                FACE,
                false,
                1.0,
            );
        }
    }
}

/// The panda's eyes: open (lit), half closed, closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Eyes {
    Open,
    Half,
    Closed,
}

/// A small avatar, round in `rect` (the footer's, the session menu's,
/// 7.4): the account's photo, else the panda's face on the accent's
/// gradient, as the lock's but without its ring and shadow; `alpha`
/// opaque.
pub(crate) fn avatar_round(f: &mut Frame, rect: Rectangle, alpha: f32) {
    let centre = rect.center();
    let r = rect.width / 2.0;
    match super::avatar::photo() {
        Some(photo) => {
            let mut image = canvas::Image::new(photo)
                .filter_method(FilterMethod::Linear)
                .opacity(alpha);
            image.border_radius = r.into();
            f.draw_image(rect, image);
        }
        None => {
            let fill = gradient::Linear::new(
                rect.position(),
                Point::new(rect.x + rect.width, rect.y + rect.height),
            )
            .add_stop(0.0, faded(ACCENT.get(), alpha))
            .add_stop(1.0, faded(ACCENT_TO.get(), alpha));
            f.fill(&Path::circle(centre, r), canvas::Fill::from(fill));
            let face = rect.width * FACE / AVATAR;
            panda(
                f,
                Point::new(centre.x - face / 2.0, centre.y - face / 2.0),
                face,
                false,
                alpha,
            );
        }
    }
}

/// The panda's face, `size` px at `at` (the mockup's 30-unit box): awake
/// (its eyes lit) or asleep (its eyes closed).
pub(crate) fn panda(f: &mut Frame, at: Point, size: f32, asleep: bool, alpha: f32) {
    let eyes = if asleep { Eyes::Closed } else { Eyes::Open };
    panda_eyes(f, at, size, eyes, alpha);
}

/// The panda's face with its eyes `eyes` (the curtain's panda falling
/// asleep, 7.2).
pub(crate) fn panda_eyes(f: &mut Frame, at: Point, size: f32, eyes: Eyes, alpha: f32) {
    let asleep = eyes == Eyes::Closed;
    let s = size / 30.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let dark = faded(PANDA_DARK.get(), alpha);
    let light = faded(PANDA_LIGHT.get(), alpha);
    f.fill(&Path::circle(p(8.5, 8.5), 4.2 * s), dark);
    f.fill(&Path::circle(p(21.5, 8.5), 4.2 * s), dark);
    f.fill(&Path::circle(p(15.0, 16.5), 10.5 * s), light);
    let (ry, eye_y) = if asleep { (3.2, 16.4) } else { (3.6, 16.0) };
    for (x, angle) in [(10.8, -20.0f32), (19.2, 20.0)] {
        let centre = p(x, eye_y);
        f.with_save(|f| {
            f.translate(Vector::new(centre.x, centre.y));
            f.rotate(angle.to_radians());
            let patch = Path::new(|b| {
                b.ellipse(canvas::path::arc::Elliptical {
                    center: Point::ORIGIN,
                    radii: Vector::new(2.8 * s, ry * s),
                    rotation: Radians(0.0),
                    start_angle: Radians(0.0),
                    end_angle: Radians(std::f32::consts::TAU),
                });
            });
            f.fill(&patch, dark);
        });
    }
    if asleep {
        // `M9.6 16.6 Q11 17.6 12.4 16.6 M17.6 16.6 Q19 17.6 20.4 16.6`.
        let lids = Path::new(|b| {
            b.move_to(p(9.6, 16.6));
            b.quadratic_curve_to(p(11.0, 17.6), p(12.4, 16.6));
            b.move_to(p(17.6, 16.6));
            b.quadratic_curve_to(p(19.0, 17.6), p(20.4, 16.6));
        });
        f.stroke(
            &lids,
            Stroke {
                line_cap: LineCap::Round,
                ..Stroke::default().with_color(light).with_width(0.9 * s)
            },
        );
    } else if eyes == Eyes::Half {
        // The lids half down: lower, smaller lights under a line.
        let lids = Path::new(|b| {
            b.move_to(p(9.4, 15.6));
            b.quadratic_curve_to(p(11.0, 15.0), p(12.6, 15.6));
            b.move_to(p(17.4, 15.6));
            b.quadratic_curve_to(p(19.0, 15.0), p(20.6, 15.6));
        });
        f.stroke(
            &lids,
            Stroke {
                line_cap: LineCap::Round,
                ..Stroke::default().with_color(light).with_width(0.8 * s)
            },
        );
        f.fill(
            &Path::circle(p(11.1, 16.6), 0.7 * s),
            faded(Color::WHITE, alpha),
        );
        f.fill(
            &Path::circle(p(18.9, 16.6), 0.7 * s),
            faded(Color::WHITE, alpha),
        );
    } else {
        f.fill(
            &Path::circle(p(11.3, 15.4), 1.0 * s),
            faded(Color::WHITE, alpha),
        );
        f.fill(
            &Path::circle(p(18.7, 15.4), 1.0 * s),
            faded(Color::WHITE, alpha),
        );
    }
    let nose = Path::new(|b| {
        b.ellipse(canvas::path::arc::Elliptical {
            center: p(15.0, 21.0),
            radii: Vector::new(1.6 * s, 1.1 * s),
            rotation: Radians(0.0),
            start_angle: Radians(0.0),
            end_angle: Radians(std::f32::consts::TAU),
        });
    });
    f.fill(&nose, dark);
}

/// The two pandas asleep in the bottom left corner, breathing twice from
/// `shown` (#649).
fn draw_pandas(f: &mut Frame, size: Size, now: Instant, shown: Option<Instant>) {
    let bottom = size.height - PANDAS_BOTTOM;
    let burst = shown.map(breaths);
    let drawing = pandas::drawing(Mood::Resting, burst, now);
    let box_of = |side: f32| Size::new(side, side * pandas::VIEW_BOX.1 / pandas::VIEW_BOX.0);
    let first = box_of(PANDA_NEAR);
    f.draw_svg(
        Rectangle::new(Point::new(PANDAS_LEFT, bottom - first.height), first),
        svg::Svg::new(drawing.clone()),
    );
    // The second, smaller, 12 px into the first's box, its feet 2 px
    // lower, turned 14° back.
    let second = box_of(PANDA_FAR);
    let centre = Point::new(
        PANDAS_LEFT + PANDA_NEAR - 12.0 + second.width / 2.0,
        bottom + 2.0 - second.height / 2.0,
    );
    f.with_save(|f| {
        f.translate(Vector::new(centre.x, centre.y));
        f.rotate(-14f32.to_radians());
        f.draw_svg(
            Rectangle::new(
                Point::new(-second.width / 2.0, -second.height / 2.0),
                second,
            ),
            svg::Svg::new(drawing),
        );
    });
}

/// The agents waiting: the waiting panda (34 px, #649) and their count,
/// in a pill centred on `cx`, its top at `top`.
fn draw_agents(
    f: &mut Frame,
    words: String,
    (cx, top): (f32, f32),
    now: Instant,
    burst: Option<Burst>,
) {
    let width = crate::popover::kit::measure(&words, AGENTS_TEXT, MANROPE_SEMIBOLD)
        + AGENTS_IN
        + AGENTS_PANDA
        + 8.0
        + 12.0;
    let rect = Rectangle::new(
        Point::new(cx - width / 2.0, top),
        Size::new(width, AGENTS_HEIGHT),
    );
    f.fill(
        &Path::rounded_rectangle(rect.position(), rect.size(), 10.0.into()),
        crate::theme::ink(Color::from_rgba8(245, 180, 74, 0.10)),
    );
    let tall = AGENTS_PANDA * pandas::VIEW_BOX.1 / pandas::VIEW_BOX.0;
    f.draw_svg(
        Rectangle::new(
            Point::new(rect.x + AGENTS_IN, rect.center_y() - tall / 2.0),
            Size::new(AGENTS_PANDA, tall),
        ),
        svg::Svg::new(pandas::drawing(Mood::Waiting, burst, now)),
    );
    f.fill_text(text(
        words,
        Point::new(
            rect.x + AGENTS_IN + AGENTS_PANDA + 8.0,
            rect.center_y() - AGENTS_TEXT * LINE / 2.0,
        ),
        AGENTS_TEXT,
        MANROPE_SEMIBOLD,
        AMBER.get(),
        Alignment::Left,
    ));
}

/// The status row: the layout's button, the battery, the Wi-Fi.
fn draw_row(f: &mut Frame, size: Size, centre: f32) {
    use crate::levels::model::Band;
    let items = row(size, centre);
    if let Some((rect, link)) = &items.wifi {
        use crate::network::model::Link;
        let (bars, slash) = match link {
            Link::Connected { bars, .. } => (*bars, false),
            Link::Wired | Link::Connecting => (4, false),
            Link::Off | Link::Offline | Link::Airplane => (0, true),
            Link::Hidden => (0, true),
        };
        crate::network::draw::wifi(f, rect.position(), 20.0, bars, TEXT.get(), slash, 1.0);
    }
    if let Some((rect, battery, label)) = &items.battery {
        let (text_colour, fill) = match battery.band() {
            Band::Ok => (crate::theme::ink(rgb(0x7EE0A0)), rgb(0x3FA66B)),
            Band::Low => (AMBER.get(), rgb(0xE8A13A)),
            Band::Critical => (WRONG_TEXT.get(), rgb(0xE5793F)),
        };
        // `rect 0.75 0.75 21.5 13.5 rx3`, the fill in `3 3 … 9 rx1.5`, the
        // nub at 23.5: 24 × 14 (26 × 15 drawn smaller).
        let k = 24.0 / 26.0;
        let at = Point::new(rect.x, centre - 7.0);
        let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
        f.stroke(
            &Path::rounded_rectangle(
                p(0.75, 0.75),
                Size::new(21.5 * k, 13.5 * k),
                (3.0 * k).into(),
            ),
            Stroke::default().with_color(TEXT.get()).with_width(1.5 * k),
        );
        let full = 17.5 * k;
        let width = full * (battery.shown() as f32 / 100.0).clamp(0.05, 1.0);
        f.fill(
            &Path::rounded_rectangle(p(3.0, 3.0), Size::new(width, 9.0 * k), (1.5 * k).into()),
            fill,
        );
        f.fill(
            &Path::rounded_rectangle(p(23.5, 4.5), Size::new(2.0 * k, 6.0 * k), (1.0 * k).into()),
            TEXT.get(),
        );
        f.fill_text(text(
            label.clone(),
            Point::new(rect.x + 24.0 + 6.0, centre - ROW_TEXT * LINE / 2.0),
            ROW_TEXT,
            MANROPE_BOLD,
            text_colour,
            Alignment::Left,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shake_goes_10_9_6_4_px_and_back() {
        let ms = Duration::from_millis;
        assert_eq!(shake_at(ms(0)), 0.0);
        assert!(
            (shake_at(ms(54)) + 10.0).abs() < 0.01,
            "{}",
            shake_at(ms(54))
        );
        assert!((shake_at(ms(126)) - 9.0).abs() < 0.01);
        assert!((shake_at(ms(198)) + 6.0).abs() < 0.01);
        assert!((shake_at(ms(270)) - 4.0).abs() < 0.01);
        assert_eq!(shake_at(ms(360)), 0.0);
        assert_eq!(shake_at(ms(1000)), 0.0);
    }

    #[test]
    fn the_form_sits_as_the_mockup_says() {
        let layout = Layout::of(Size::new(2560.0, 1600.0), true);
        assert_eq!(layout.clock_top, 800.0 - 330.0);
        assert_eq!(layout.avatar.y, 800.0 - 110.0);
        assert_eq!(layout.avatar.x, 1280.0 - 44.0);
        assert_eq!(layout.field.width, 320.0);
        assert_eq!(layout.field.x, 1280.0 - 160.0);
        // The button 5 px in from the field's right.
        assert_eq!(
            layout.button.x + layout.button.width,
            layout.field.x + 315.0
        );
        assert_eq!(layout.message.y, layout.field.y + 44.0 + 12.0);
        let alone = Layout::of(Size::new(1920.0, 1080.0), false);
        assert_eq!(alone.clock_top, 540.0 - 90.0);
    }
}
