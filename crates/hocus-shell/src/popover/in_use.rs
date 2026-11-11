//! The popover of the sign of what is in use (5.12; 5.11's and 5.10's
//! lines in it): "En ce moment", then a line per use, the microphone's,
//! the camera's, then the screen's (the recording, then each share): the
//! application's 28 px icon ringed 2 px in its kind's colour, its name
//! (13 px, semibold) and "Micro · meet.google.com · depuis 12 min" (11.5
//! px, in the kind's colour; the detail is the application's window's
//! title, else its stream's media name, when known). Then its buttons, 30
//! tall (radius 9): **Couper le micro** (or **Rétablir le micro**) while
//! the microphone is used, which mutes the default source as the sound's
//! button does; **Arrêter le partage** for each share (5.11's blue),
//! which ends its session; the recording's pause (or go on) and
//! **Arrêter l'enregistrement** (5.10's colours). Under the lines, while
//! sharing, how many notifications wait for its end. 290 wide, 12 px in,
//! its parts 8 px apart. A use that ended keeps its line while it
//! lingers, without its button.
//!
//! Built with the shell's widget kit ([`crate::ui`]).

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::{RecordAsk, ShareAsk, ToCompositor};
use iced::{
    Border, Color, Element, Length, Point, Rectangle, Size,
    alignment::Vertical,
    keyboard,
    widget::{
        Space, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, row,
    },
    window,
};

use super::{
    Effect, Pressed,
    kit::{self, MANROPE_SEMIBOLD, app_tile},
};
use crate::theme::Ink;
use crate::{
    in_use::{Kind, Media, Uses, Words, draw::colour},
    levels::{
        self,
        draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
        icons::Icon,
        sound::{Ask, Set},
    },
    ui::{
        self, Around, control,
        parts::{drawn, drawn_out, gap},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 290;
const BORDER: f32 = 1.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const TITLE: f32 = 14.0;
const ICON: f32 = 28.0;
const ICON_RADIUS: f32 = 8.0;
const ICON_GAP: f32 = 10.0;
const RING: f32 = 2.0;
const APP: f32 = 13.0;
const WHAT: f32 = 11.5;
const HELD: f32 = 11.5;
const BUTTON: f32 = 30.0;
const BUTTON_RADIUS: f32 = 9.0;
const BUTTON_TEXT: f32 = 12.5;
/// The recording's pause, beside its stop (5.10's).
const PAUSE: f32 = 36.0;
const PAUSE_GAP: f32 = 6.0;
const BLUE: Ink = Ink::Dark(rgb(0x5B8DEF));
const BLUE_HOVER: Ink = Ink::Dark(rgb(0x6E9BF2));
const MUTE_FILL: Ink = Ink::Dark(Color::from_rgba8(245, 163, 91, 0.14));
const MUTE_RING: Ink = Ink::Dark(Color::from_rgba8(245, 163, 91, 0.6));
const UNMUTE_RING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const REC_FILL: Ink = Ink::Dark(Color::from_rgba8(242, 84, 91, 0.16));
const REC_RED: Ink = Ink::Dark(rgb(0xF2545B));
const REC_AMBER: Ink = Ink::Dark(rgb(0xF5B44A));
const REC_PAUSE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const REC_STOP: Ink = Ink::Dark(Color::from_rgba8(242, 84, 91, 0.85));
const REC_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const REC_ICON: Ink = Ink::Dark(rgb(0xEEF1F6));

/// Its times go on while it shows: built again each second.
const TICK: Duration = Duration::from_secs(1);

/// A use's line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Microphone(usize),
    Camera(usize),
    Recording,
    Share(u64),
}

impl Line {
    fn kind(self) -> Kind {
        match self {
            Self::Microphone(_) => Kind::Microphone,
            Self::Camera(_) => Kind::Camera,
            Self::Recording | Self::Share(_) => Kind::Screen,
        }
    }
}

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Mute,
    Stop(u64),
    Pause,
    StopRecording,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("in_use:{self:?}")
    }
}

/// Where everything is, for the uses now.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    title: f32,
    /// Each line and its top.
    lines: Vec<(Line, f32)>,
    held: Option<f32>,
    buttons: Vec<(Target, Rectangle)>,
    height: f32,
}

/// A line's height: its name over its use, or its icon, the taller.
fn line_height() -> f32 {
    (APP * LINE + 1.0 + WHAT * LINE).max(ICON)
}

fn layout(uses: &Uses) -> Layout {
    let left = BORDER + PAD;
    let room = WIDTH as f32 - 2.0 * left;
    let mut y = left;
    let title = y + TITLE * LINE / 2.0;
    y += TITLE * LINE;
    let mut lines = Vec::new();
    let all = (0..uses.microphone.len())
        .map(Line::Microphone)
        .chain((0..uses.camera.len()).map(Line::Camera))
        .chain(uses.recording.iter().map(|_| Line::Recording))
        .chain(uses.shares.iter().map(|s| Line::Share(s.what.id)));
    for line in all {
        y += GAP;
        lines.push((line, y));
        y += line_height();
    }
    let held = (uses.live_shares().count() > 0).then(|| {
        y += GAP;
        let middle = y + HELD * LINE / 2.0;
        y += HELD * LINE;
        middle
    });
    let mut buttons = Vec::new();
    let mut row = |y: &mut f32, targets: &[(Target, f32, f32)]| {
        *y += GAP;
        for (target, x, width) in targets {
            buttons.push((
                *target,
                Rectangle::new(Point::new(*x, *y), Size::new(*width, BUTTON)),
            ));
        }
        *y += BUTTON;
    };
    if !uses.microphone.is_empty() {
        row(&mut y, &[(Target::Mute, left, room)]);
    }
    for share in uses.live_shares() {
        row(&mut y, &[(Target::Stop(share.id), left, room)]);
    }
    if uses.live_recording().is_some() {
        let stop = left + PAUSE + PAUSE_GAP;
        row(
            &mut y,
            &[
                (Target::Pause, left, PAUSE),
                (Target::StopRecording, stop, room - PAUSE - PAUSE_GAP),
            ],
        );
    }
    Layout {
        title,
        lines,
        held,
        buttons,
        height: y + PAD + BORDER,
    }
}

/// Its height, for the uses now.
pub fn height() -> u32 {
    layout(&crate::in_use::snapshot()).height.round() as u32
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// When its content was built: its times are of then.
    built: Option<Instant>,
    said: Option<Vec<String>>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The popover opens: nothing holds the ring, nothing said yet.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A press anywhere on the popover.
    Pressed,
    /// What is used changed under the view, or a second passed.
    Stale,
}

/// A line's words and colour.
struct Said {
    app: String,
    what: String,
    colour: Color,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let words = panel.words;
    let order: Vec<Target> = layout(&crate::in_use::snapshot())
        .buttons
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let next = || ui(|ui| ui.built.map(|at| at + TICK));
    Around::new(panel.id, build(words), handle, Msg::Stale)
        .rebuild(move || build(words))
        .stale_when(move || next().is_some_and(|at| Instant::now() >= at))
        .wake_at(next)
        .keys(move |pressed| arrows(pressed, &order))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// The arrows with no button holding the ring: the first (down), or the
/// last (up); the kit takes them round after.
fn arrows(pressed: &Pressed, order: &[Target]) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let down = match &pressed.key {
        Key::Named(Named::ArrowDown) => true,
        Key::Named(Named::ArrowUp) => false,
        _ => return None,
    };
    if ui(|ui| ui.focus.is_some()) {
        return None;
    }
    let target = if down { order.first() } else { order.last() };
    target.map(|t| Msg::Focus(*t))
}

/// Its content, as what is used is now.
fn build<'a>(words: Words) -> El<'a> {
    let now = Instant::now();
    let uses = crate::in_use::snapshot();
    let layout = layout(&uses);
    ui(|ui| {
        ui.built = Some(now);
        say(words, &uses, &layout, now, ui);
        content(words, &uses, &layout, now, ui.focus)
    })
}

fn content<'a>(
    words: Words,
    uses: &Uses,
    layout: &Layout,
    now: Instant,
    focus: Option<Target>,
) -> El<'a> {
    let mut all = column![
        container(said(words.right_now(), TITLE, MANROPE_BOLD, TEXT.get()))
            .height(TITLE * LINE)
            .align_y(Vertical::Center)
    ]
    .spacing(GAP);
    let room = WIDTH as f32 - 2.0 * (BORDER + PAD) - ICON - ICON_GAP;
    for (line, _) in &layout.lines {
        let Some(said_of) = said_of(words, uses, *line, now) else {
            continue;
        };
        let (app, what) = elided(&said_of, room);
        let ring = said_of.colour;
        let tile = icon(words, uses, *line);
        let mark = drawn_out(ICON, line_height(), RING, move |f, s| {
            let at = Rectangle::new(
                Point::new(0.0, (s.height - ICON) / 2.0),
                Size::new(ICON, ICON),
            );
            tile(f, at);
            // Ringed 2 px outside it.
            f.stroke(
                &kit::rounded(
                    Rectangle::new(
                        Point::new(at.x - RING / 2.0, at.y - RING / 2.0),
                        Size::new(ICON + RING, ICON + RING),
                    ),
                    ICON_RADIUS + RING / 2.0,
                ),
                Stroke::default().with_color(ring).with_width(RING),
            );
        });
        let texts = column![
            said(app, APP, MANROPE_SEMIBOLD, TEXT.get()),
            Space::new().height(1.0),
            said(what, WHAT, MANROPE, said_of.colour),
        ];
        let line = row![mark, gap(ICON_GAP), texts]
            .height(line_height())
            .align_y(Vertical::Center);
        all = all.push(ui::line(line, format!("{}, {}", said_of.app, said_of.what)));
    }
    if layout.held.is_some() {
        let held = words.held(crate::notifications::with(|c| c.stack.held_while_sharing()));
        let line = container(said(held.clone(), HELD, MANROPE, FAINT.get()))
            .height(HELD * LINE)
            .align_y(Vertical::Center);
        all = all.push(ui::line(line, held));
    }
    let mut buttons = layout.buttons.iter().map(|(t, _)| *t).peekable();
    while let Some(target) = buttons.next() {
        let shown = button_of(words, uses, target, focus);
        if target == Target::Pause
            && let Some(stop) = buttons.next_if_eq(&Target::StopRecording)
        {
            all = all.push(
                row![shown, gap(PAUSE_GAP), button_of(words, uses, stop, focus)]
                    .width(Length::Fill),
            );
        } else {
            all = all.push(shown);
        }
    }
    container(all).padding(BORDER + PAD).into()
}

/// Line `line`'s words and colour at `now`.
fn said_of(words: Words, uses: &Uses, line: Line, now: Instant) -> Option<Said> {
    let kind = line.kind();
    let muted = kind == Kind::Microphone && uses.muted;
    let colour = colour(kind, muted);
    let media = |seen: &crate::in_use::Seen<Media>| Said {
        app: seen.what.app.clone(),
        what: words.line(
            kind,
            muted,
            detail(&seen.what).as_deref(),
            now.saturating_duration_since(seen.since),
        ),
        colour,
    };
    match line {
        Line::Microphone(i) => uses.microphone.get(i).map(media),
        Line::Camera(i) => uses.camera.get(i).map(media),
        Line::Recording => {
            let rec = uses.recording.as_ref()?;
            Some(Said {
                app: words.recording().to_owned(),
                what: words.recording_line(rec.what.shown.paused, rec.what.elapsed(now)),
                colour,
            })
        }
        Line::Share(id) => {
            let share = uses.shares.iter().find(|s| s.what.id == id)?;
            Some(Said {
                app: app_name(words, &share.what.app),
                what: words.line(
                    kind,
                    false,
                    Some(&words.shares(&share.what.thing)),
                    now.saturating_duration_since(share.since),
                ),
                colour,
            })
        }
    }
}

/// The name of the application of id `app` (a share's).
fn app_name(words: Words, app: &str) -> String {
    crate::share::app_name(app).unwrap_or_else(|| words.an_app().to_owned())
}

/// What draws an icon at a place.
type Draw = Box<dyn Fn(&mut Frame, Rectangle)>;

/// What draws line `line`'s application's icon at a place.
fn icon(words: Words, uses: &Uses, line: Line) -> Draw {
    let tile = |icon: Option<Icon>, name: String| -> Draw {
        Box::new(move |f, rect| {
            app_tile(f, rect, (icon.as_ref(), &name), (ICON_RADIUS, 12.0), 1.0);
        })
    };
    match line {
        Line::Microphone(i) | Line::Camera(i) => {
            let list = if matches!(line, Line::Microphone(_)) {
                &uses.microphone
            } else {
                &uses.camera
            };
            match list.get(i) {
                Some(seen) => tile(seen.what.icon.clone(), seen.what.app.clone()),
                None => Box::new(|_, _| {}),
            }
        }
        Line::Recording => {
            // 5.10's item: its tint, its dot (amber while paused).
            let paused = uses.recording.as_ref().is_some_and(|r| r.what.shown.paused);
            Box::new(move |f, rect| {
                f.fill(&kit::rounded(rect, ICON_RADIUS), REC_FILL.get());
                let dot = if paused { REC_AMBER } else { REC_RED };
                f.fill(&Path::circle(rect.center(), 5.0), dot.get());
            })
        }
        Line::Share(id) => {
            let Some(share) = uses.shares.iter().find(|s| s.what.id == id) else {
                return Box::new(|_, _| {});
            };
            let scale = kit::popover_scale();
            let px = crate::launcher::icons::pixels(ICON as u32, scale);
            let handle = crate::share::entry_of(&share.what.app)
                .and_then(|e| e.icon)
                .and_then(|icon| match crate::launcher::icons::of(&icon, px) {
                    Some(found) => found,
                    None => {
                        crate::launcher::icons::want([icon.as_str()], ICON as u32, scale);
                        None
                    }
                });
            match handle {
                Some(handle) => Box::new(move |f, rect| {
                    f.draw_image(rect, canvas::Image::new(handle.clone()));
                }),
                None => tile(None, app_name(words, &share.what.app)),
            }
        }
    }
}

/// `top` over `under`, as the canvas laid one fill over another.
fn over(top: Color, under: Color) -> Color {
    let a = top.a + under.a * (1.0 - top.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    let mix = |t: f32, u: f32| (t * top.a + u * under.a * (1.0 - top.a)) / a;
    Color {
        r: mix(top.r, under.r),
        g: mix(top.g, under.g),
        b: mix(top.b, under.b),
        a,
    }
}

/// Button `target`: its fill (lit under the pointer), its edge, its word
/// or its icon.
fn button_of<'a>(words: Words, uses: &Uses, target: Target, focus: Option<Target>) -> El<'a> {
    let muted = uses.muted;
    let paused = uses.live_recording().is_some_and(|r| r.shown.paused);
    let word = |said_as: String, ink: Color| -> El<'a> {
        container(said(said_as, BUTTON_TEXT, MANROPE_BOLD, ink))
            .center(Length::Fill)
            .into()
    };
    let (face, name, role, width): (El<'a>, String, Role, Length) = match target {
        Target::Mute => (
            word(words.mute(muted).to_owned(), TEXT.get()),
            words.mute(muted).to_owned(),
            Role::ToggleButton,
            Length::Fill,
        ),
        Target::Stop(id) => {
            let said_as = stop_word(words, uses, id);
            (
                word(said_as.clone(), Color::WHITE),
                said_as,
                Role::Button,
                Length::Fill,
            )
        }
        Target::Pause => (
            drawn(PAUSE, BUTTON, move |f, s| {
                pause_icon(f, Rectangle::new(Point::ORIGIN, s), paused);
            }),
            words.pause_recording(paused).to_owned(),
            Role::Button,
            Length::Fixed(PAUSE),
        ),
        Target::StopRecording => (
            word(words.stop_recording().to_owned(), Color::WHITE),
            words.stop_recording().to_owned(),
            Role::Button,
            Length::Fill,
        ),
    };
    let pressed = button(face)
        .padding(0)
        .width(width)
        .height(BUTTON)
        .style(move |_, status| {
            let lit = style::lit(status);
            let contrast = crate::theme::contrast();
            let (fill, hover, edge) = match target {
                Target::Mute => (
                    (!muted).then(|| MUTE_FILL.get()),
                    HOVER,
                    Some(if muted { UNMUTE_RING } else { MUTE_RING }),
                ),
                Target::Stop(_) => {
                    let blue = if lit { BLUE_HOVER } else { BLUE };
                    (Some(blue.get()), HOVER, None)
                }
                Target::Pause => (Some(REC_PAUSE.get()), REC_HOVER, None),
                Target::StopRecording => (Some(REC_STOP.get()), REC_HOVER, None),
            };
            let lit_fill = !matches!(target, Target::Stop(_)) && lit;
            let background = match (fill, lit_fill && !contrast) {
                (Some(fill), true) => Some(over(hover.get(), fill)),
                (None, true) => Some(hover.get()),
                (fill, false) => fill,
            };
            let border = match edge {
                Some(edge) => Border {
                    color: edge.get(),
                    width: 1.0,
                    radius: BUTTON_RADIUS.into(),
                },
                // Under high contrast a hover is a 1 px border in
                // `text.dim` (bar.md §13).
                None if lit_fill && contrast => {
                    style::hovered(Color::TRANSPARENT, BUTTON_RADIUS).border
                }
                None => style::round(BUTTON_RADIUS),
            };
            button::Style {
                background: background.map(Into::into),
                border,
                ..button::Style::default()
            }
        })
        .on_press(Msg::Act(target));
    control(pressed, target.id(), role, name)
        .node(|n| {
            if target == Target::Mute {
                n.checked(muted)
            } else {
                n
            }
        })
        .focus(focus == Some(target), Msg::Focus(target), BUTTON_RADIUS)
        .activate(Msg::Act(target))
        .rows(0)
        .into()
}

/// Share `id`'s button: with several shares, each names its application.
fn stop_word(words: Words, uses: &Uses, id: u64) -> String {
    if uses.live_shares().count() < 2 {
        return words.stop(None);
    }
    let app = uses
        .live_shares()
        .find(|s| s.id == id)
        .map(|s| app_name(words, &s.app));
    words.stop(app.as_deref())
}

/// What it shows, for the log: when something other than its times
/// changed.
fn say(words: Words, uses: &Uses, layout: &Layout, now: Instant, ui: &mut Ui) {
    let mut said: Vec<String> = vec![words.right_now().to_owned()];
    for (line, _) in &layout.lines {
        if let Some(line) = said_of(words, uses, *line, now) {
            said.push(line.app);
            said.push(line.what);
        }
    }
    if layout.held.is_some() {
        let held = crate::notifications::with(|c| c.stack.held_while_sharing());
        said.push(words.held(held));
    }
    for (target, _) in &layout.buttons {
        said.push(match target {
            Target::Mute => words.mute(uses.muted).to_owned(),
            Target::Stop(id) => stop_word(words, uses, *id),
            Target::Pause => "⏸".to_owned(),
            Target::StopRecording => words.stop_recording().to_owned(),
        });
    }
    let changed = ui.said.as_ref().is_none_or(|before| {
        before.len() != said.len()
            || before
                .iter()
                .zip(&said)
                .any(|(a, b)| untimed(a) != untimed(b))
    });
    if changed {
        tracing::debug!(?said, "the in-use popover shows it");
    }
    ui.said = Some(said);
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(target) => act(target),
    }
}

fn act(target: Target) {
    match target {
        Target::Mute => {
            // The default source, as the sound's button mutes it.
            let mixer = levels::sound::mixer();
            if let Some(mic) = mixer.microphone() {
                let muted = !mic.sound.is_some_and(|s| s.muted);
                tracing::info!(id = mic.id, muted, "the microphone from the bar");
                levels::ask(Ask::Node(mic.id, Set::Mute(muted)));
            }
        }
        Target::Stop(id) => {
            tracing::info!(id, "sharing stopped from the bar");
            super::send(vec![Effect::Tell(ToCompositor::Share(ShareAsk::Stop {
                id,
            }))]);
        }
        Target::Pause => {
            let paused = crate::in_use::snapshot()
                .live_recording()
                .is_some_and(|r| r.shown.paused);
            let ask = if paused {
                RecordAsk::Resume
            } else {
                RecordAsk::Pause
            };
            tracing::debug!(?ask, "the recording from the bar");
            super::tell(ToCompositor::Record(ask));
        }
        Target::StopRecording => {
            tracing::debug!(ask = ?RecordAsk::Stop, "the recording from the bar");
            super::tell(ToCompositor::Record(RecordAsk::Stop));
        }
    }
}

/// A line's two parts, cut to `room`: a use's detail is cut first, so
/// that what is used and since when stay.
fn elided(said: &Said, room: f32) -> (String, String) {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => {
            let mut what = |t: &str| kit::measure_in(system.raw(), t, WHAT, MANROPE);
            let line = fit_detail(&said.what, room, &mut what);
            let app = kit::elide(&said.app, room, |t| {
                kit::measure_in(system.raw(), t, APP, MANROPE_SEMIBOLD)
            });
            (app, line)
        }
        Err(_) => (said.app.clone(), said.what.clone()),
    }
}

/// `line` ("Micro · detail · depuis 12 min") in `room`, its detail cut
/// as much as needed; the whole cut when that is not enough.
fn fit_detail(line: &str, room: f32, width: &mut impl FnMut(&str) -> f32) -> String {
    if width(line) <= room {
        return line.to_owned();
    }
    let parts: Vec<&str> = line.split(" · ").collect();
    if let [first, middle @ .., last] = parts.as_slice()
        && !middle.is_empty()
    {
        let ends = width(&format!("{first} ·  · {last}"));
        if ends < room {
            let detail = kit::elide(&middle.join(" · "), room - ends, &mut *width);
            return format!("{first} · {detail} · {last}");
        }
    }
    kit::elide(line, room, width)
}

/// The detail of `media`'s line: its window's title, else its stream's
/// media name.
fn detail(media: &Media) -> Option<String> {
    crate::desktops::title_of_app(&media.ids).or_else(|| media.media.clone())
}

/// A line without its time, to log what changes and not the seconds.
fn untimed(said: &str) -> &str {
    said.rsplit_once(" · ").map_or(said, |(rest, _)| rest)
}

/// The recording's pause (two bars), or go on (a triangle) when `paused`,
/// 15 px in the middle of `rect` (5.10's).
fn pause_icon(f: &mut Frame, rect: Rectangle, paused: bool) {
    let k = 15.0 / 24.0;
    let o = Point::new(rect.center_x() - 7.5, rect.center_y() - 7.5);
    let p = |x: f32, y: f32| Point::new(o.x + x * k, o.y + y * k);
    let icon = if paused {
        Path::new(|b| {
            b.move_to(p(8.0, 5.0));
            b.line_to(p(19.0, 12.0));
            b.line_to(p(8.0, 19.0));
            b.close();
        })
    } else {
        Path::new(|b| {
            for (x0, x1) in [(7.0, 10.5), (13.5, 17.0)] {
                b.move_to(p(x0, 5.0));
                b.line_to(p(x1, 5.0));
                b.line_to(p(x1, 19.0));
                b.line_to(p(x0, 19.0));
                b.close();
            }
        })
    };
    f.fill(&icon, REC_ICON.get());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::in_use::{Rec, Seen};
    use hocus_link::{RecordingShown, ShareShown, SharedThing};

    fn seen<T>(what: T) -> Seen<T> {
        Seen {
            what,
            since: Instant::now(),
            ended: None,
        }
    }

    fn media(app: &str) -> Media {
        Media {
            key: app.to_owned(),
            app: app.to_owned(),
            icon: None,
            ids: Vec::new(),
            media: None,
        }
    }

    #[test]
    fn a_line_per_use_then_the_buttons_of_what_is_on() {
        let mut uses = Uses {
            microphone: vec![seen(media("Firefox")), seen(media("Discord"))],
            camera: vec![seen(media("Firefox"))],
            ..Uses::default()
        };
        let one = layout(&uses);
        assert_eq!(
            one.lines.iter().map(|l| l.0).collect::<Vec<_>>(),
            [Line::Microphone(0), Line::Microphone(1), Line::Camera(0)]
        );
        // The microphone used: its button, the whole width.
        assert_eq!(one.buttons.len(), 1);
        assert_eq!(one.buttons[0].0, Target::Mute);
        assert_eq!(one.buttons[0].1.width, WIDTH as f32 - 2.0 * (BORDER + PAD));
        assert_eq!(one.held, None);
        // A share and the recording: their lines last, a stop for the
        // share, the recording's pause and stop side by side.
        uses.shares = vec![seen(ShareShown {
            id: 7,
            app: "org.mozilla.firefox".into(),
            thing: SharedThing::Screen(String::new()),
        })];
        uses.recording = Some(seen(Rec {
            shown: RecordingShown {
                id: 1,
                paused: false,
                elapsed: 0,
            },
            at: Instant::now(),
        }));
        let all = layout(&uses);
        assert_eq!(all.lines[3].0, Line::Recording);
        assert_eq!(all.lines[4].0, Line::Share(7));
        assert!(all.held.is_some());
        let targets: Vec<Target> = all.buttons.iter().map(|b| b.0).collect();
        assert_eq!(
            targets,
            [
                Target::Mute,
                Target::Stop(7),
                Target::Pause,
                Target::StopRecording
            ]
        );
        let (pause, stop) = (all.buttons[2].1, all.buttons[3].1);
        assert_eq!(pause.y, stop.y);
        assert_eq!(pause.width, PAUSE);
        assert_eq!(stop.x + stop.width, WIDTH as f32 - BORDER - PAD);
        assert!(all.height > one.height);
        // A share that ended lingers, its line kept, its button gone.
        uses.shares[0].ended = Some(Instant::now());
        uses.recording = None;
        let lingering = layout(&uses);
        assert_eq!(lingering.lines.last().map(|l| l.0), Some(Line::Share(7)));
        assert_eq!(lingering.held, None);
        assert_eq!(lingering.buttons.len(), 1);
    }

    #[test]
    fn a_long_detail_is_cut_and_the_time_kept() {
        // One unit a character.
        let mut width = |t: &str| t.chars().count() as f32;
        let line = "Micro · Réunion d'équipe — Meet · depuis 12 min";
        assert_eq!(fit_detail(line, 100.0, &mut width), line);
        let cut = fit_detail(line, 36.0, &mut width);
        assert!(cut.starts_with("Micro · Réu"), "{cut}");
        assert!(cut.ends_with(" · depuis 12 min"), "{cut}");
        assert!(width(&cut) <= 36.0, "{cut}");
    }

    #[test]
    fn the_log_says_what_changes_not_the_seconds() {
        assert_eq!(
            untimed("Micro · meet.google.com · depuis 12 s"),
            "Micro · meet.google.com"
        );
        assert_eq!(untimed("Firefox"), "Firefox");
    }
}
