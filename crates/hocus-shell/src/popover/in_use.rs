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

use std::time::{Duration, Instant};

use hocus_link::{RecordAsk, ShareAsk, ToCompositor};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Pressed, event,
    kit::{self, Input, MANROPE_SEMIBOLD, Pointer, app_tile, text, text_in},
};
use crate::{
    in_use::{Kind, Media, Uses, Words, draw::colour},
    levels::{
        self,
        draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring},
        icons::Icon,
        sound::{Ask, Set},
    },
};

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
const BLUE: Color = rgb(0x5B8DEF);
const BLUE_HOVER: Color = rgb(0x6E9BF2);
const MUTE_FILL: Color = Color::from_rgba8(245, 163, 91, 0.14);
const MUTE_RING: Color = Color::from_rgba8(245, 163, 91, 0.6);
const UNMUTE_RING: Color = Color::from_rgba8(255, 255, 255, 0.14);
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const REC_FILL: Color = Color::from_rgba8(242, 84, 91, 0.16);
const REC_RED: Color = rgb(0xF2545B);
const REC_AMBER: Color = rgb(0xF5B44A);
const REC_PAUSE: Color = Color::from_rgba8(255, 255, 255, 0.06);
const REC_STOP: Color = Color::from_rgba8(242, 84, 91, 0.85);
const REC_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const REC_ICON: Color = rgb(0xEEF1F6);

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
pub struct State {
    pointer: Pointer<Target>,
    focused: Option<Target>,
    drawn: Option<Instant>,
    said: Option<Vec<String>>,
}

/// A line's words and colour.
struct Said {
    app: String,
    what: String,
    colour: Color,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => {
                state.drawn = Some(now);
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = Self::key(state, &pressed);
                }
                self.say(state, now);
                // The times go on while it shows: drawn again each second.
                super::with(|c| c.is_open(self.id))
                    .then(|| canvas::Action::request_redraw_at(now + Duration::from_secs(1)))
            }
            Input::Mouse(seen, at) => Self::mouse(state, seen, at),
            Input::Key(pressed) => Self::key(state, &pressed),
            Input::Nothing => None,
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
        kit::background(&mut frame, bounds.size(), self.glass);
        let f = &mut frame;
        let now = state.drawn.unwrap_or_else(Instant::now);
        let uses = crate::in_use::snapshot();
        let layout = layout(&uses);
        let left = BORDER + PAD;
        let room = WIDTH as f32 - 2.0 * left - ICON - ICON_GAP;
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                self.words.right_now().to_owned(),
                Point::new(left, layout.title),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        for (line, top) in &layout.lines {
            let Some(said) = self.said(&uses, *line, now) else {
                continue;
            };
            let icon = Rectangle::new(
                Point::new(left, top + (line_height() - ICON) / 2.0),
                Size::new(ICON, ICON),
            );
            self.icon(f, &uses, *line, icon);
            // Ringed 2 px outside it.
            f.stroke(
                &kit::rounded(
                    Rectangle::new(
                        Point::new(icon.x - RING / 2.0, icon.y - RING / 2.0),
                        Size::new(ICON + RING, ICON + RING),
                    ),
                    ICON_RADIUS + RING / 2.0,
                ),
                Stroke::default().with_color(said.colour).with_width(RING),
            );
            let x = left + ICON + ICON_GAP;
            let middle = top + line_height() / 2.0;
            let app_middle = middle - (1.0 + WHAT * LINE) / 2.0;
            let what_middle = middle + (APP * LINE + 1.0) / 2.0;
            let (app, what) = elided(&said, room);
            f.fill_text(text_in(
                MANROPE_SEMIBOLD,
                text(app, Point::new(x, app_middle), APP, TEXT, Alignment::Left),
            ));
            f.fill_text(text(
                what,
                Point::new(x, what_middle),
                WHAT,
                said.colour,
                Alignment::Left,
            ));
        }
        if let Some(middle) = layout.held {
            let held = crate::notifications::with(|c| c.stack.held_while_sharing());
            f.fill_text(text(
                self.words.held(held),
                Point::new(left, middle),
                HELD,
                FAINT,
                Alignment::Left,
            ));
        }
        for (target, rect) in &layout.buttons {
            let hover = state.pointer.hover == Some(*target);
            self.button(f, &uses, *target, *rect, hover);
            if state.focused == Some(*target) {
                ring(f, *rect, BUTTON_RADIUS);
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
        state.pointer.interaction(false)
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

impl Panel {
    /// Line `line`'s words and colour at `now`.
    fn said(&self, uses: &Uses, line: Line, now: Instant) -> Option<Said> {
        let words = self.words;
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
                    app: self.app_name(&share.what.app),
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
    fn app_name(&self, app: &str) -> String {
        crate::share::app_name(app).unwrap_or_else(|| self.words.an_app().to_owned())
    }

    /// Line `line`'s application's icon at `rect`.
    fn icon(&self, f: &mut Frame, uses: &Uses, line: Line, rect: Rectangle) {
        let tile = |f: &mut Frame, icon: Option<&Icon>, name: &str| {
            app_tile(f, rect, (icon, name), (ICON_RADIUS, 12.0), 1.0);
        };
        match line {
            Line::Microphone(i) | Line::Camera(i) => {
                let list = if matches!(line, Line::Microphone(_)) {
                    &uses.microphone
                } else {
                    &uses.camera
                };
                if let Some(seen) = list.get(i) {
                    tile(f, seen.what.icon.as_ref(), &seen.what.app);
                }
            }
            Line::Recording => {
                // 5.10's item: its tint, its dot (amber while paused).
                f.fill(&kit::rounded(rect, ICON_RADIUS), REC_FILL);
                let paused = uses.recording.as_ref().is_some_and(|r| r.what.shown.paused);
                let dot = if paused { REC_AMBER } else { REC_RED };
                f.fill(&Path::circle(rect.center(), 5.0), dot);
            }
            Line::Share(id) => {
                let Some(share) = uses.shares.iter().find(|s| s.what.id == id) else {
                    return;
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
                    Some(handle) => f.draw_image(rect, canvas::Image::new(handle)),
                    None => tile(f, None, &self.app_name(&share.what.app)),
                }
            }
        }
    }

    /// Button `target` at `rect`.
    fn button(&self, f: &mut Frame, uses: &Uses, target: Target, rect: Rectangle, hover: bool) {
        let shape = kit::rounded(rect, BUTTON_RADIUS);
        let word = |f: &mut Frame, said: String, ink: Color| {
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(said, rect.center(), BUTTON_TEXT, ink, Alignment::Center),
            ));
        };
        match target {
            Target::Mute => {
                if !uses.muted {
                    f.fill(&shape, MUTE_FILL);
                }
                if hover {
                    f.fill(&shape, HOVER);
                }
                let edge = if uses.muted { UNMUTE_RING } else { MUTE_RING };
                f.stroke(
                    &kit::rounded(
                        Rectangle::new(
                            Point::new(rect.x + 0.5, rect.y + 0.5),
                            Size::new(rect.width - 1.0, rect.height - 1.0),
                        ),
                        BUTTON_RADIUS - 0.5,
                    ),
                    Stroke::default().with_color(edge).with_width(1.0),
                );
                word(f, self.words.mute(uses.muted).to_owned(), TEXT);
            }
            Target::Stop(id) => {
                f.fill(&shape, if hover { BLUE_HOVER } else { BLUE });
                word(f, self.stop_word(uses, id), Color::WHITE);
            }
            Target::Pause => {
                f.fill(&shape, REC_PAUSE);
                if hover {
                    f.fill(&shape, REC_HOVER);
                }
                let paused = uses.live_recording().is_some_and(|r| r.shown.paused);
                pause_icon(f, rect, paused);
            }
            Target::StopRecording => {
                f.fill(&shape, REC_STOP);
                if hover {
                    f.fill(&shape, REC_HOVER);
                }
                word(f, self.words.stop_recording().to_owned(), Color::WHITE);
            }
        }
    }

    /// Share `id`'s button: with several shares, each names its
    /// application.
    fn stop_word(&self, uses: &Uses, id: u64) -> String {
        if uses.live_shares().count() < 2 {
            return self.words.stop(None);
        }
        let app = uses
            .live_shares()
            .find(|s| s.id == id)
            .map(|s| self.app_name(&s.app));
        self.words.stop(app.as_deref())
    }

    /// What it shows, for the log: when something other than its times
    /// changed.
    fn say(&self, state: &mut State, now: Instant) {
        let uses = crate::in_use::snapshot();
        let layout = layout(&uses);
        let mut said: Vec<String> = vec![self.words.right_now().to_owned()];
        for (line, _) in &layout.lines {
            if let Some(line) = self.said(&uses, *line, now) {
                said.push(line.app);
                said.push(line.what);
            }
        }
        if layout.held.is_some() {
            let held = crate::notifications::with(|c| c.stack.held_while_sharing());
            said.push(self.words.held(held));
        }
        for (target, _) in &layout.buttons {
            said.push(match target {
                Target::Mute => self.words.mute(uses.muted).to_owned(),
                Target::Stop(id) => self.stop_word(&uses, *id),
                Target::Pause => "⏸".to_owned(),
                Target::StopRecording => self.words.stop_recording().to_owned(),
            });
        }
        let changed = state.said.as_ref().is_none_or(|before| {
            before.len() != said.len()
                || before
                    .iter()
                    .zip(&said)
                    .any(|(a, b)| untimed(a) != untimed(b))
        });
        if changed {
            tracing::debug!(?said, "the in-use popover shows it");
        }
        state.said = Some(said);
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

    fn targets() -> Vec<Target> {
        layout(&crate::in_use::snapshot())
            .buttons
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    fn mouse<Message>(
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let layout = layout(&crate::in_use::snapshot());
        let target = at.and_then(|at| {
            layout
                .buttons
                .iter()
                .find(|(_, r)| r.contains(at))
                .map(|(t, _)| *t)
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focused = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    Self::act(target);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(state: &mut State, pressed: &Pressed) -> Option<canvas::Action<Message>> {
        use iced::keyboard::{Key, key::Named};
        let targets = Self::targets();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab | Named::ArrowDown) => {
                state.focused = kit::step(&targets, state.focused, pressed.modifiers.shift());
            }
            Key::Named(Named::ArrowUp) => {
                state.focused = kit::step(&targets, state.focused, true);
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(target) = state.focused.filter(|t| targets.contains(t)) {
                    Self::act(target);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
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
    f.fill(&icon, REC_ICON);
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
