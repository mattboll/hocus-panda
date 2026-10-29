//! The sound's popover (bar.md §9, the 3.4 mockup): its title and the
//! default output's name; the master volume, its mute button and its
//! value; then the outputs, the default one checked. A click on an output
//! makes it the default, which keeps its own volume (PipeWire's, per sink):
//! the slider then shows that one's.
//!
//! One canvas draws it all, and takes the pointer and the keys itself:
//! what changes draws this surface alone, without a message to iced. The
//! slider's volumes go to the sound's service as they come; it sends
//! PipeWire only the last one each time `wpctl` is ready. The slider shows
//! the volume asked until PipeWire says it, a second at most.
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the mute button, the
//! slider and the outputs; `Enter` or `Space` acts; the arrows move the
//! slider by 5 % (and go through the outputs); `Esc` closes.

use std::time::{Duration, Instant};

use hocus_link::{LevelKey, POPOVER_WIDTH, RADIUS};
use iced::{
    Color, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Event as Seen, event};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, HOVER, LINE, MANROPE, MANROPE_BOLD, MUTED_FILL, SOUND, TEXT, TRACK,
        VALUE, cell, ring, stroke,
    },
    model::{Sound, Words},
    sound::{Ask, Kind, Sink},
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const GAP: f32 = 6.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
const HEADER: f32 = 32.0;
const TITLE: f32 = 16.0;
const NAME: f32 = 13.0;
const MASTER: f32 = 36.0;
const BUTTON: f32 = 32.0;
const BUTTON_RADIUS: f32 = 8.0;
const ICON: f32 = 18.0;
const SLIDER: f32 = 24.0;
const TRACK_HEIGHT: f32 = 8.0;
const KNOB: f32 = 16.0;
const VALUE_BOX: f32 = 36.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const LABEL: f32 = 11.5;
/// CSS's `letter-spacing: 0.08em`.
const LABEL_SPACING: f32 = 0.08 * LABEL;
const LABEL_PAD: f32 = 2.0;
const ROW: f32 = 36.0;
const ROW_GAP: f32 = 2.0;
/// The rows reach 6 px past the content's sides.
const ROW_OUT: f32 = 6.0;
const ROW_TEXT: f32 = 14.0;
const ROW_ICON_GAP: f32 = 10.0;
const CHECK: f32 = 16.0;

const HEADER_TOP: f32 = BORDER + PAD_TOP;
const MASTER_TOP: f32 = HEADER_TOP + HEADER + GAP;
const RULE_TOP: f32 = MASTER_TOP + MASTER + GAP + RULE;
const LABEL_TOP: f32 = RULE_TOP + 1.0 + RULE + GAP;
const ROWS_TOP: f32 = LABEL_TOP + LABEL_PAD * 2.0 + LABEL * LINE + GAP;

/// The tint over the compositor's blur, a little denser than the bar's.
const GLASS_TOP: Color = Color::from_rgba8(22, 27, 42, 0.72);
const GLASS_BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.82);
const OPAQUE_TOP: Color = Color::from_rgba8(0x16, 0x1B, 0x2A, 1.0);
const OPAQUE_BOTTOM: Color = Color::from_rgba8(0x0E, 0x11, 0x1C, 1.0);
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BUTTON_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const BUTTON_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const RULE_COLOUR: Color = Color::from_rgba8(255, 255, 255, 0.08);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.05);
const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.45);

/// The slider shows the volume asked this long at most, until PipeWire
/// says it.
const ASKED_FOR: Duration = Duration::from_secs(1);

/// The popover's height with `outputs` outputs listed.
pub fn height(outputs: usize) -> u32 {
    let rows = outputs as f32 * (ROW + ROW_GAP) - if outputs > 0 { ROW_GAP } else { 0.0 };
    (ROWS_TOP + rows + PAD_BOTTOM + BORDER).round() as u32
}

/// The sound's popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What can be pointed at, or hold the keyboard's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Mute,
    Slider,
    Output(usize),
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// The outputs and the default one's sound, as last drawn.
    sinks: Vec<Sink>,
    sound: Option<Sound>,
    /// Their names as they fit, the title's output name, and the label's
    /// letters' places.
    names: Vec<String>,
    header: String,
    label: Vec<(char, f32)>,
    /// What the names were made for: the outputs', and the default one.
    made_for: (Vec<String>, Option<usize>),
    /// The `0`'s advance of the value's font, for tabular figures.
    zero: Option<f32>,
    hover: Option<Target>,
    pressed: Option<Target>,
    dragging: bool,
    /// The slider's volume while dragged, or asked and not yet said by
    /// PipeWire, and since when.
    asked: Option<(f32, Instant)>,
    focus: Option<Target>,
    pointer: Option<Point>,
    inside: bool,
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
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => self.refresh(state, *now),
            _ if !super::with(|c| c.is_open(self.id)) => None,
            iced::Event::Mouse(event) => {
                let at = crate::levels::draw::pointer(event, bounds, cursor, state.pointer);
                state.pointer = at;
                self.mouse(state, event, at)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                self.key(state, key, modifiers.shift())
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
        self.background(&mut frame, bounds.size());
        self.header(&mut frame, state);
        self.master(&mut frame, state);
        frame.fill_rectangle(
            Point::new(LEFT, RULE_TOP),
            Size::new(RIGHT - LEFT, 1.0),
            RULE_COLOUR,
        );
        for (ch, x) in &state.label {
            let mut letter = text(
                ch.to_string(),
                Point::new(*x, LABEL_TOP + LABEL_PAD + LABEL * LINE / 2.0),
                LABEL,
                DIM,
                Alignment::Left,
            );
            letter.font = MANROPE_BOLD;
            frame.fill_text(letter);
        }
        for (i, sink) in state.sinks.iter().enumerate() {
            self.output(&mut frame, state, i, sink);
        }
        if let Some(target) = state.focus {
            let (rect, radius) = target.shape();
            ring(&mut frame, rect, radius);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl Target {
    /// Where it is, and its corners' radius.
    fn shape(self) -> (Rectangle, f32) {
        match self {
            Self::Mute => (
                Rectangle::new(
                    Point::new(LEFT, MASTER_TOP + (MASTER - BUTTON) / 2.0),
                    Size::new(BUTTON, BUTTON),
                ),
                BUTTON_RADIUS,
            ),
            Self::Slider => (
                Rectangle::new(
                    Point::new(slider_left(), MASTER_TOP + (MASTER - SLIDER) / 2.0),
                    Size::new(slider_right() - slider_left(), SLIDER),
                ),
                SLIDER / 2.0,
            ),
            Self::Output(i) => (output_rect(i), BUTTON_RADIUS),
        }
    }
}

fn slider_left() -> f32 {
    LEFT + BUTTON + 8.0
}

fn slider_right() -> f32 {
    RIGHT - VALUE_BOX - 8.0
}

fn output_rect(i: usize) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - ROW_OUT, ROWS_TOP + i as f32 * (ROW + ROW_GAP)),
        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, ROW),
    )
}

/// The slider's volume at `x`, in percent: the knob's centre under the
/// pointer, its travel the track less the knob.
fn volume_at(x: f32) -> f32 {
    let travel = slider_right() - slider_left() - KNOB;
    ((x - slider_left() - KNOB / 2.0) / travel * 100.0)
        .round()
        .clamp(0.0, 100.0)
}

impl Panel {
    /// What the popover shows, read afresh; the names made to fit when
    /// they changed.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        // Keys that reached another of the shell's surfaces (see
        // `popover::key`).
        let keys = super::take_keys();
        let keyed = !keys.is_empty() && super::with(|c| c.is_open(self.id));
        if keyed {
            for (key, shift) in keys {
                let _: Option<canvas::Action<Message>> = self.key(state, &key, shift);
            }
        }
        state.sinks = levels::sound::sinks();
        state.sound = levels::snapshot().sound;
        let names: Vec<String> = state.sinks.iter().map(|s| s.description.clone()).collect();
        let default = state.sinks.iter().position(|s| s.default);
        if state.made_for != (names.clone(), default) || state.label.is_empty() {
            self.fit(state, &names);
            state.made_for = (names, default);
        }
        if let Some(focus) = state.focus
            && let Target::Output(i) = focus
            && i >= state.sinks.len()
        {
            state.focus = None;
        }
        // The volume asked stays shown until PipeWire says it, or a while.
        match state.asked {
            Some((asked, since)) if !state.dragging => {
                let said = state
                    .sound
                    .is_some_and(|s| (s.volume.round() - asked).abs() < 0.5);
                if said || now.saturating_duration_since(since) >= ASKED_FOR {
                    state.asked = None;
                    None
                } else {
                    Some(canvas::Action::request_redraw_at(since + ASKED_FOR))
                }
            }
            _ => None,
        }
    }

    /// The names elided to fit, the title's output name too, and the
    /// label's letters spaced.
    fn fit(&self, state: &mut State, names: &[String]) {
        let Ok(mut system) = iced::advanced::graphics::text::font_system().write() else {
            state.names = names.to_vec();
            return;
        };
        let system = system.raw();
        // Between the icon and the check, each with its gap.
        let row_room = RIGHT - CHECK - ROW_ICON_GAP - (LEFT + ICON + ROW_ICON_GAP);
        state.names = names
            .iter()
            .map(|n| elide(n, row_room, |t| measure(system, t, MANROPE, ROW_TEXT)))
            .collect();
        let title = measure(system, self.words.sound(), MANROPE_BOLD, TITLE);
        let default = state
            .sinks
            .iter()
            .find(|s| s.default)
            .map(|s| s.description.clone())
            .unwrap_or_default();
        state.header = elide(&default, RIGHT - LEFT - title - 12.0, |t| {
            measure(system, t, MANROPE, NAME)
        });
        let mut x = LEFT;
        state.label = self
            .words
            .output()
            .chars()
            .map(|ch| {
                let at = x;
                x += measure(system, &ch.to_string(), MANROPE_BOLD, LABEL) + LABEL_SPACING;
                (ch, at)
            })
            .collect();
        state.zero = crate::clock::measure(system, MANROPE, VALUE, VALUE * LINE).map(|m| m.zero);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| target_at(state, at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                if !state.inside && at.is_some() {
                    state.inside = true;
                    event(Seen::PopoverEnter);
                }
                if state.dragging
                    && let Some(at) = at
                {
                    self.slide(state, at.x);
                    return redraw;
                }
                if state.hover != target {
                    state.hover = target;
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.inside = false;
                event(Seen::PopoverLeave);
                state.hover = None;
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pressed = target;
                if target == Some(Target::Slider)
                    && let Some(at) = at
                {
                    state.dragging = true;
                    self.slide(state, at.x);
                }
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let pressed = state.pressed.take();
                if std::mem::take(&mut state.dragging) {
                    if let Some((v, _)) = state.asked {
                        state.asked = Some((v, Instant::now()));
                    }
                } else if pressed.is_some() && pressed == target {
                    self.act(state, pressed);
                }
                redraw
            }
            _ => None,
        }
    }

    /// The slider moved to `x`: that volume, asked at once.
    fn slide(&self, state: &mut State, x: f32) {
        let volume = volume_at(x);
        if state.asked.map(|(v, _)| v) != Some(volume) {
            levels::ask(Ask::Volume(volume));
        }
        state.asked = Some((volume, Instant::now()));
    }

    fn act(&self, state: &State, target: Option<Target>) {
        match target {
            Some(Target::Mute) => levels::key(LevelKey::Mute),
            Some(Target::Output(i)) => {
                if let Some(sink) = state.sinks.get(i).filter(|s| !s.default) {
                    levels::ask(Ask::Default(sink.id));
                }
            }
            Some(Target::Slider) | None => {}
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        shift: bool,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order: Vec<Target> = [Target::Mute, Target::Slider]
            .into_iter()
            .chain((0..state.sinks.len()).map(Target::Output))
            .collect();
        let at = state.focus.and_then(|f| order.iter().position(|t| *t == f));
        let step = |by: isize| {
            let n = order.len() as isize;
            let next = match at {
                Some(i) => (i as isize + by).rem_euclid(n),
                None if by > 0 => 0,
                None => n - 1,
            };
            order.get(next as usize).copied()
        };
        match key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => state.focus = step(if shift { -1 } else { 1 }),
            Key::Named(Named::Enter | Named::Space) => self.act(state, state.focus),
            Key::Named(
                arrow @ (Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown),
            ) => {
                let up = matches!(arrow, Named::ArrowRight | Named::ArrowUp);
                match state.focus {
                    Some(Target::Slider) => {
                        state.asked = None;
                        levels::key(if up {
                            LevelKey::VolumeUp
                        } else {
                            LevelKey::VolumeDown
                        });
                    }
                    Some(Target::Output(i))
                        if matches!(arrow, Named::ArrowUp | Named::ArrowDown) =>
                    {
                        let n = state.sinks.len();
                        let next = if up { i + n - 1 } else { i + 1 } % n.max(1);
                        state.focus = Some(Target::Output(next));
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let radius = RADIUS as f32;
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, radius.into());
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(&shape, canvas::Fill::from(tint));
        let rim = Path::rounded_rectangle(
            Point::new(BORDER / 2.0, BORDER / 2.0),
            Size::new(size.width - BORDER, size.height - BORDER),
            (radius - BORDER / 2.0).into(),
        );
        f.stroke(
            &rim,
            canvas::Stroke {
                style: canvas::Style::Solid(RIM),
                width: BORDER,
                ..canvas::Stroke::default()
            },
        );
    }

    fn header(&self, f: &mut Frame, state: &State) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words.sound().to_owned(),
            Point::new(LEFT, middle),
            TITLE,
            TEXT,
            Alignment::Left,
        );
        title.font = MANROPE_BOLD;
        f.fill_text(title);
        f.fill_text(text(
            state.header.clone(),
            Point::new(RIGHT, middle),
            NAME,
            FAINT,
            Alignment::Right,
        ));
    }

    fn master(&self, f: &mut Frame, state: &State) {
        let middle = MASTER_TOP + MASTER / 2.0;
        let muted = state.sound.is_some_and(|s| s.muted);
        // The mute button.
        let (button, radius) = Target::Mute.shape();
        let fill = if state.hover == Some(Target::Mute) {
            BUTTON_HOVER
        } else {
            BUTTON_FILL
        };
        f.fill(
            &Path::rounded_rectangle(button.position(), button.size(), radius.into()),
            fill,
        );
        let icon = Point::new(button.center_x() - ICON / 2.0, middle - ICON / 2.0);
        speaker(f, icon, muted);
        // The slider: its track, its fill to the volume, its knob.
        let volume = state
            .asked
            .map(|(v, _)| v)
            .or(state.sound.map(|s| s.volume))
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);
        let (left, right) = (slider_left(), slider_right());
        let travel = right - left - KNOB;
        let track_top = middle - TRACK_HEIGHT / 2.0;
        let radius = TRACK_HEIGHT / 2.0;
        f.fill(
            &Path::rounded_rectangle(
                Point::new(left, track_top),
                Size::new(right - left, TRACK_HEIGHT),
                radius.into(),
            ),
            TRACK,
        );
        let knob = left + travel * volume / 100.0;
        let filled = Path::rounded_rectangle(
            Point::new(left, track_top),
            Size::new(knob + KNOB / 2.0 - left, TRACK_HEIGHT),
            radius.into(),
        );
        if muted {
            f.fill(&filled, MUTED_FILL);
        } else {
            let fill = gradient::Linear::new(
                Point::new(left, middle),
                Point::new(knob + KNOB / 2.0, middle),
            )
            .add_stop(0.0, SOUND.0)
            .add_stop(1.0, SOUND.1);
            f.fill(&filled, canvas::Fill::from(fill));
        }
        let centre = Point::new(knob + KNOB / 2.0, middle);
        // `box-shadow: 0 1px 4px`: rings fading out under the knob, 1 px
        // down.
        for (grow, share) in [(2.5_f32, 0.12_f32), (1.6, 0.2), (0.8, 0.3)] {
            f.fill(
                &Path::circle(Point::new(centre.x, centre.y + 1.0), KNOB / 2.0 + grow),
                KNOB_SHADOW.scale_alpha(share),
            );
        }
        f.fill(&Path::circle(centre, KNOB / 2.0), TEXT);
        // The value.
        let at = Point::new(RIGHT, middle);
        match state.sound {
            Some(sound) if sound.muted => {
                f.fill_text(text(
                    self.words.muted().to_owned(),
                    at,
                    VALUE,
                    FAINT,
                    Alignment::Right,
                ));
            }
            Some(_) => {
                let zero = state.zero.unwrap_or(VALUE * 0.6);
                let digits = (volume.round() as u32).to_string();
                let mut x = RIGHT - zero * digits.len() as f32;
                for ch in digits.chars() {
                    cell(f, ch, x + zero / 2.0, middle, VALUE, MANROPE, DIM);
                    x += zero;
                }
            }
            None => {}
        }
    }

    fn output(&self, f: &mut Frame, state: &State, i: usize, sink: &Sink) {
        let rect = output_rect(i);
        let fill = if sink.default {
            Some(HOVER)
        } else if state.hover == Some(Target::Output(i)) {
            Some(ROW_HOVER)
        } else {
            None
        };
        if let Some(fill) = fill {
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), BUTTON_RADIUS.into()),
                fill,
            );
        }
        let middle = rect.center_y();
        device(f, Point::new(LEFT, middle - ICON / 2.0), sink.kind);
        let name = state.names.get(i).unwrap_or(&sink.description);
        f.fill_text(text(
            name.clone(),
            Point::new(LEFT + ICON + ROW_ICON_GAP, middle),
            ROW_TEXT,
            TEXT,
            Alignment::Left,
        ));
        if sink.default {
            check(f, Point::new(RIGHT - CHECK, middle - CHECK / 2.0));
        }
    }
}

/// What is under `at`.
fn target_at(state: &State, at: Point) -> Option<Target> {
    [Target::Mute, Target::Slider]
        .into_iter()
        .chain((0..state.sinks.len()).map(Target::Output))
        .find(|t| t.shape().0.contains(at))
}

fn text(
    content: String,
    position: Point,
    size: f32,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font: MANROPE,
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// `text` cut with an ellipsis to fit `room` px as `width` measures it.
pub fn elide(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let cut: String = chars[..keep]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…";
        if width(&cut) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

/// `text`'s width in `font` at `size`.
fn measure(
    system: &mut iced::advanced::graphics::text::cosmic_text::FontSystem,
    text: &str,
    font: iced::Font,
    size: f32,
) -> f32 {
    use iced::advanced::graphics::text::cosmic_text;
    let mut buffer = cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(size, size * LINE));
    let attrs = iced::advanced::graphics::text::to_attributes(font);
    buffer.set_text(system, text, &attrs, cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
}

// --- Icons, from the mockup's SVGs (24 units, drawn 18 px) --------------------

/// Lines through `points` (SVG units from `at`, `scale` px a unit).
fn lines(f: &mut Frame, at: Point, scale: f32, width: f32, colour: Color, points: &[(f32, f32)]) {
    let p = |(x, y): (f32, f32)| Point::new(at.x + x * scale, at.y + y * scale);
    let path = Path::new(|b| {
        if let Some((first, rest)) = points.split_first() {
            b.move_to(p(*first));
            for point in rest {
                b.line_to(p(*point));
            }
        }
    });
    f.stroke(&path, stroke(width * scale, colour));
}

/// An arc of an SVG `a r r 0 0 1` from `(x, y)` down by `chord`, bulging
/// right: the speaker's waves.
fn wave(
    f: &mut Frame,
    at: Point,
    scale: f32,
    (x, y): (f32, f32),
    r: f32,
    chord: f32,
    colour: Color,
) {
    let half = chord / 2.0;
    let centre_x = x - (r * r - half * half).sqrt();
    let angle = (half / r).asin();
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: Point::new(at.x + centre_x * scale, at.y + (y + half) * scale),
            radius: r * scale,
            start_angle: Radians(-angle),
            end_angle: Radians(angle),
        });
    });
    f.stroke(&arc, stroke(2.0 * scale, colour));
}

fn speaker(f: &mut Frame, at: Point, muted: bool) {
    let s = ICON / 24.0;
    let colour = if muted { FAINT } else { TEXT };
    let cone = [
        (11.0, 5.0),
        (6.0, 9.0),
        (2.0, 9.0),
        (2.0, 15.0),
        (6.0, 15.0),
        (11.0, 19.0),
        (11.0, 5.0),
        (6.0, 9.0),
    ];
    lines(f, at, s, 2.0, colour, &cone);
    if muted {
        lines(f, at, s, 2.0, colour, &[(16.0, 9.0), (21.0, 15.0)]);
        lines(f, at, s, 2.0, colour, &[(21.0, 9.0), (16.0, 15.0)]);
    } else {
        // `M15.5 8.5a5 5 0 0 1 0 7` and `M18.5 6a9 9 0 0 1 0 12`.
        wave(f, at, s, (15.5, 8.5), 5.0, 7.0, colour);
        wave(f, at, s, (18.5, 6.0), 9.0, 12.0, colour);
    }
}

fn device(f: &mut Frame, at: Point, kind: Kind) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, DIM);
    let rect = |f: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32| {
        f.stroke(
            &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (r * s).into()),
            line,
        );
    };
    match kind {
        Kind::Speaker => {
            rect(f, 3.0, 7.0, 18.0, 11.0, 2.0);
            lines(f, at, s, 2.0, DIM, &[(7.0, 21.0), (17.0, 21.0)]);
        }
        Kind::Headphones => {
            // `M4 15v-3a8 8 0 0 1 16 0v3`.
            let band = Path::new(|b| {
                b.move_to(p(4.0, 15.0));
                b.line_to(p(4.0, 12.0));
                b.arc(canvas::path::Arc {
                    center: p(12.0, 12.0),
                    radius: 8.0 * s,
                    start_angle: Radians(std::f32::consts::PI),
                    end_angle: Radians(2.0 * std::f32::consts::PI),
                });
                b.line_to(p(20.0, 15.0));
            });
            f.stroke(&band, line);
            rect(f, 3.0, 14.0, 4.0, 6.0, 1.5);
            rect(f, 17.0, 14.0, 4.0, 6.0, 1.5);
        }
        Kind::Screen => {
            rect(f, 2.0, 4.0, 20.0, 13.0, 2.0);
            lines(f, at, s, 2.0, DIM, &[(8.0, 21.0), (16.0, 21.0)]);
            lines(f, at, s, 2.0, DIM, &[(12.0, 17.0), (12.0, 21.0)]);
        }
    }
}

/// The default output's check, 16 px: `M5 12.5 10 17 19 7`.
fn check(f: &mut Frame, at: Point) {
    let s = CHECK / 24.0;
    lines(
        f,
        at,
        s,
        2.6,
        ACCENT,
        &[(5.0, 12.5), (10.0, 17.0), (19.0, 7.0)],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_height_follows_its_outputs() {
        // The mockup's, at ×1: 261 px with three outputs, 185 with one.
        assert_eq!(height(3), 261);
        assert_eq!(height(1), 185);
        assert_eq!(height(2), 223);
    }

    #[test]
    fn the_slider_covers_0_to_100() {
        let (left, right) = (slider_left(), slider_right());
        assert_eq!(right - left, 166.0);
        assert_eq!(volume_at(left), 0.0);
        assert_eq!(volume_at(left + KNOB / 2.0), 0.0);
        assert_eq!(volume_at(right - KNOB / 2.0), 100.0);
        assert_eq!(volume_at(right + 50.0), 100.0);
        assert_eq!(volume_at((left + right) / 2.0), 50.0);
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        let width = |t: &str| t.chars().count() as f32 * 7.0;
        assert_eq!(elide("Casque", 100.0, width), "Casque");
        assert_eq!(
            elide("Haut-parleurs du portable", 100.0, width),
            "Haut-parleurs…"
        );
        assert_eq!(elide("abc", 1.0, width), "…");
    }

    #[test]
    fn its_rows_and_controls_lie_inside_it() {
        let height = height(3) as f32;
        for target in [
            Target::Mute,
            Target::Slider,
            Target::Output(0),
            Target::Output(2),
        ] {
            let (rect, _) = target.shape();
            // With the keyboard's ring.
            let out = crate::levels::draw::RING_OUT + crate::levels::draw::RING;
            assert!(rect.x - out >= 0.0, "{target:?}");
            assert!(
                rect.x + rect.width + out <= POPOVER_WIDTH as f32,
                "{target:?}"
            );
            assert!(rect.y + rect.height <= height, "{target:?}");
        }
    }
}
