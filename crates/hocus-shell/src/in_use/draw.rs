//! The sign of what is in use, under the clock (5.11's sharing item): 48
//! wide, radius 12, `rgba(91,141,239,0.18)` in a 1 px
//! `rgba(91,141,239,0.6)` ring, a 20 px screen-share icon over "Partage"
//! (10 px bold), both `#B9CEFF`; lit under the pointer and while its
//! popover shows. The same in both forms, centred in the bar: in the mini
//! form where the agents' item was, in the full one over the agents'
//! group, which both move under it.
//!
//! Over it, while recording (5.10): the recording's item, 48 wide, radius
//! 12, `rgba(242,84,91,0.16)` in a 1 px `rgba(242,84,91,0.55)` ring, a red
//! dot beating over 1.4 s and the time (Martian Mono 12 px, `#FFD9DB`);
//! paused, the dot amber and still, the time "pause". Hovered in the mini
//! form, and always in the full one, pause (or go on) and stop under it.

use hocus_link::{Form, IN_USE_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
        text::Alignment,
    },
};

use super::Words;
use crate::{
    Message,
    levels::draw::{MANROPE_BOLD, faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The sign: 48 wide, 6 px in above and under its 20 px icon, 3 px over
/// its word's 10 px line.
pub const SIGN: Size = Size::new(48.0, 6.0 + 20.0 + 3.0 + 10.0 * 1.366 + 6.0);
/// Between it and what comes under it, as between the bar's items.
pub const AFTER: f32 = 14.0;
/// The recording's item: 6 px in, its 10 px dot 4 px over its time's
/// line; opened, 2 px and 4 px more, then its two 36 × 30 buttons 4 px
/// apart (radius 8).
const REC_PAD: f32 = 6.0;
const REC_DOT: f32 = 10.0;
const REC_GAP: f32 = 4.0;
const REC_TIME: f32 = 12.0;
const REC_LINE: f32 = 16.0;
const REC_BUTTON: Size = Size::new(36.0, 30.0);
const REC_BUTTON_RADIUS: f32 = 8.0;
pub const REC_CLOSED: f32 = REC_PAD + REC_DOT + REC_GAP + REC_LINE + REC_PAD;
pub const REC_OPEN: f32 = REC_CLOSED + REC_GAP + 2.0 + 2.0 * REC_BUTTON.height + REC_GAP;
/// Between the recording's item and the sign of what is shared.
const BETWEEN: f32 = 10.0;
/// The dot's beat: from 1 to 0.35 of its opacity and back.
pub const BEAT: std::time::Duration = std::time::Duration::from_millis(1400);
const BEAT_LOW: f32 = 0.35;
/// It draws again this often while it beats.
const BEAT_STEP: std::time::Duration = std::time::Duration::from_millis(50);
const REC_FILL: Color = Color::from_rgba8(242, 84, 91, 0.16);
const REC_RING: Color = Color::from_rgba8(242, 84, 91, 0.55);
const REC_INK: Color = rgb(0xFFD9DB);
const REC_RED: Color = rgb(0xF2545B);
const REC_AMBER: Color = rgb(0xF5B44A);
const REC_PAUSE: Color = Color::from_rgba8(255, 255, 255, 0.06);
const REC_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const REC_STOP: Color = Color::from_rgba8(242, 84, 91, 0.85);
const REC_ICON: Color = rgb(0xEEF1F6);
const MONO: iced::Font = iced::Font::with_name("Martian Mono");

/// The items under the clock in `form`, from the first's top: each one's
/// kind, its top and its height.
pub fn items(form: Form) -> Vec<(super::Use, f32, f32)> {
    let uses = super::snapshot();
    let mut y = 0.0;
    let mut out = Vec::new();
    for kind in uses.kinds() {
        let height = match kind {
            super::Use::Recording if super::recording_open(form) => REC_OPEN,
            super::Use::Recording => REC_CLOSED,
            super::Use::Screen => SIGN.height,
        };
        out.push((kind, y, height));
        y += height + BETWEEN;
    }
    out
}

/// How tall the items are in `form`, together.
pub fn height(form: Form) -> f32 {
    items(form).last().map_or(SIGN.height, |(_, y, h)| y + h)
}
const RADIUS: f32 = 12.0;
const ICON: f32 = 20.0;
const PAD: f32 = 6.0;
const ICON_GAP: f32 = 3.0;
const WORD: f32 = 10.0;
const FILL: Color = Color::from_rgba8(91, 141, 239, 0.18);
const LIT: Color = Color::from_rgba8(91, 141, 239, 0.28);
const EDGE_RING: Color = Color::from_rgba8(91, 141, 239, 0.6);
const INK: Color = rgb(0xB9CEFF);
/// Around the sign on its surface: the keyboard's ring.
const ROOM: f32 = 5.0;
/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;

/// The sign's top under the bar's in `form`: where the agents' group (the
/// full form) or the agents' item (the mini) was.
fn top(form: Form) -> f32 {
    match form {
        Form::Full => crate::agents::draw::TOP,
        Form::Mini => crate::agents::MINI_TOP,
    }
}

/// Its surface's size and its top under the bar's, in `form` (both forms'
/// places while the bar switches).
pub fn placement(form: Form, switching: bool) -> ((u32, u32), f32) {
    let tall = |h: f32| (h.max(1.0) + 2.0 * ROOM).ceil() as u32;
    match (switching, form) {
        (true, _) => {
            let (a, b) = (top(Form::Full), top(Form::Mini));
            let most = height(Form::Full).max(height(Form::Mini));
            (
                (FULL_WIDTH, tall(a.max(b) - a.min(b) + most)),
                a.min(b) - ROOM,
            )
        }
        (false, Form::Full) => ((FULL_WIDTH, tall(height(form))), top(form) - ROOM),
        (false, Form::Mini) => ((MINI_WIDTH, tall(height(form))), top(form) - ROOM),
    }
}

/// Where the recording's item's buttons are, in an item at `rect`
/// opened: pause (or go on), then stop.
pub fn rec_buttons(rect: Rectangle) -> [Rectangle; 2] {
    let x = rect.center_x() - REC_BUTTON.width / 2.0;
    let first = rect.y + REC_CLOSED - REC_PAD + REC_GAP + 2.0;
    [
        Rectangle::new(Point::new(x, first), REC_BUTTON),
        Rectangle::new(
            Point::new(x, first + REC_BUTTON.height + REC_GAP),
            REC_BUTTON,
        ),
    ]
}

/// "0:12", "12:04", "1:02:03": how long it has recorded.
pub fn elapsed(time: std::time::Duration) -> String {
    let s = time.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The dot's opacity at `t` into its beat (ease-in-out, 1 to 0.35 and
/// back).
pub fn beat(t: std::time::Duration) -> f32 {
    let phase = (t.as_secs_f32() / BEAT.as_secs_f32()).fract();
    let k = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    1.0 - (1.0 - BEAT_LOW) * k
}

/// The sign of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct Sign {
    pub bar: String,
    pub forms: Vec<(Form, f32)>,
    pub bar_width: f32,
    pub words: Words,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
    /// The dot beats (no reduced motion).
    pub motion: bool,
}

/// What the pointer is over on the recording's item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnRec {
    #[default]
    Nothing,
    Item,
    Pause,
    Stop,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    hover: bool,
    rec: OnRec,
    pressed: Option<OnRec>,
    drawn: Option<std::time::Instant>,
}

impl Sign {
    /// The surface's top under the bar's, as placed for its forms now.
    fn surface_top(&self) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false).1,
            _ => placement(Form::Full, true).1,
        }
    }

    /// Item `kind` in `form` on a surface `size` large, centred in the bar
    /// as wide as it is drawn now; none if it is not shown.
    fn rect_of(&self, kind: super::Use, form: Form, size: Size) -> Option<Rectangle> {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        let (_, y, height) = items(form).into_iter().find(|(k, _, _)| *k == kind)?;
        Some(Rectangle::new(
            Point::new(
                centre - SIGN.width / 2.0,
                top(form) - self.surface_top() + y,
            ),
            Size::new(SIGN.width, height),
        ))
    }

    /// The sharing sign alone (not while the bar switches), for the
    /// pointer.
    fn item(&self, size: Size) -> Option<Rectangle> {
        match self.forms.as_slice() {
            [(form, _)] => self.rect_of(super::Use::Screen, *form, size),
            _ => None,
        }
    }

    /// The recording's item alone, and its form.
    fn rec_item(&self, size: Size) -> Option<(Rectangle, Form)> {
        match self.forms.as_slice() {
            [(form, _)] => Some((self.rect_of(super::Use::Recording, *form, size)?, *form)),
            _ => None,
        }
    }

    /// What the pointer at `at` is over on the recording's item.
    fn on_rec(&self, at: Option<Point>, size: Size) -> OnRec {
        let (Some(at), Some((rect, form))) = (at, self.rec_item(size)) else {
            return OnRec::Nothing;
        };
        if !rect.contains(at) {
            return OnRec::Nothing;
        }
        if super::recording_open(form) {
            let [pause, stop] = rec_buttons(rect);
            if pause.contains(at) {
                return OnRec::Pause;
            }
            if stop.contains(at) {
                return OnRec::Stop;
            }
        }
        OnRec::Item
    }

    /// `rect` (the surface's) in the bar's frame: from its right edge
    /// (negative inside it), from its top.
    fn in_bar(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(
            Point::new(rect.x - size.width - 1.0, self.surface_top() + rect.y),
            rect.size(),
        )
    }
}

impl Sign {
    /// The pointer on the recording's item: hovered, it opens (the mini
    /// form) and its buttons light; a click on one pauses, goes on or
    /// stops.
    fn rec_mouse(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
        size: Size,
    ) -> Option<canvas::Action<Message>> {
        self.rec_item(size)?;
        let on = self.on_rec(at, size);
        let mut redraw = false;
        if on != state.rec {
            if (on == OnRec::Nothing) != (state.rec == OnRec::Nothing) {
                super::open_recording(on != OnRec::Nothing);
            }
            state.rec = on;
            redraw = true;
        }
        match seen {
            mouse::Event::ButtonPressed(mouse::Button::Left) if on != OnRec::Nothing => {
                state.pressed = Some(on);
                return Some(canvas::Action::request_redraw().and_capture());
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if state.pressed.is_some() => {
                let pressed = state.pressed.take();
                if pressed == Some(on) {
                    let paused = super::snapshot().recording.is_some_and(|r| r.shown.paused);
                    let ask = match on {
                        OnRec::Pause if paused => Some(hocus_link::RecordAsk::Resume),
                        OnRec::Pause => Some(hocus_link::RecordAsk::Pause),
                        OnRec::Stop => Some(hocus_link::RecordAsk::Stop),
                        _ => None,
                    };
                    if let Some(ask) = ask {
                        tracing::debug!(?ask, "the recording's item is clicked");
                        popover::tell(hocus_link::ToCompositor::Record(ask));
                    }
                }
                return Some(canvas::Action::request_redraw().and_capture());
            }
            _ => {}
        }
        (redraw || on != OnRec::Nothing).then(|| {
            if on == OnRec::Nothing {
                canvas::Action::request_redraw()
            } else {
                canvas::Action::request_redraw().and_capture()
            }
        })
    }
}

impl canvas::Program<Message> for Sign {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.drawn = Some(*now);
                // The dot beats while it records, and the time goes on.
                let rec = super::snapshot().recording?;
                if rec.shown.paused {
                    return None;
                }
                let step = if self.motion {
                    BEAT_STEP
                } else {
                    std::time::Duration::from_millis(250)
                };
                Some(canvas::Action::request_redraw_at(*now + step))
            }
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.at);
                state.at = at;
                if let Some(action) = self.rec_mouse(state, seen, at, size) {
                    return Some(action);
                }
                let rect = self.item(size)?;
                let over = at.is_some_and(|at| rect.contains(at));
                match seen {
                    mouse::Event::CursorEntered
                    | mouse::Event::CursorMoved { .. }
                    | mouse::Event::CursorLeft => {
                        if over == state.hover {
                            return None;
                        }
                        state.hover = over;
                        if over {
                            popover::event(popover::Event::ItemEnter {
                                bar: self.bar.clone(),
                                item: Item::Sharing,
                                rect: self.in_bar(rect, size),
                            });
                        } else {
                            let at = at.unwrap_or(Point::new(size.width / 2.0, size.height));
                            popover::event(popover::Event::ItemLeave {
                                at: self.in_bar(Rectangle::new(at, Size::ZERO), size).position(),
                            });
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item: Item::Sharing,
                            rect: self.in_bar(rect, size),
                        });
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
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
        let size = bounds.size();
        if let Some(backdrop) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, size, backdrop);
        }
        if let Some(rect) = self.item(size) {
            let in_bar = self.in_bar(rect, size);
            popover::with(|c| {
                c.set_items(&self.bar, IN_USE_NAMESPACE, vec![(Item::Sharing, in_bar)])
            });
        }
        let uses = super::snapshot();
        if !uses.any() {
            return vec![frame.into_geometry()];
        }
        if let Some(rec) = &uses.recording {
            let now = state.drawn.unwrap_or_else(std::time::Instant::now);
            for &(form, alpha) in &self.forms {
                if let Some(rect) = self.rect_of(super::Use::Recording, form, size) {
                    let on = if self.forms.len() == 1 {
                        state.rec
                    } else {
                        OnRec::Nothing
                    };
                    draw_rec(
                        &mut frame,
                        rect,
                        rec,
                        now,
                        RecLook {
                            open: super::recording_open(form),
                            on,
                            motion: self.motion,
                            alpha,
                            words: self.words,
                        },
                    );
                }
            }
        }
        if uses.shares.is_empty() {
            return vec![frame.into_geometry()];
        }
        let still = self.forms.len() == 1;
        let (open, focused) = if still {
            popover::with(|c| {
                (
                    c.shows(&self.bar, Item::Sharing),
                    c.focused(&self.bar, Item::Sharing),
                )
            })
        } else {
            (false, false)
        };
        for &(form, alpha) in &self.forms {
            let Some(rect) = self.rect_of(super::Use::Screen, form, size) else {
                continue;
            };
            let lit = still && (state.hover || open);
            draw_sign(&mut frame, rect, self.words, lit, alpha);
            if focused {
                ring(&mut frame, rect, RADIUS);
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
        if state.hover || matches!(state.rec, OnRec::Pause | OnRec::Stop) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The sign at `rect`: its tint and ring, the icon over the word.
pub fn draw_sign(f: &mut Frame, rect: Rectangle, words: Words, lit: bool, alpha: f32) {
    let shape = kit::rounded(rect, RADIUS);
    f.fill(&shape, faded(if lit { LIT } else { FILL }, alpha));
    f.stroke(
        &kit::rounded(
            Rectangle::new(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
            ),
            RADIUS - 0.5,
        ),
        Stroke::default()
            .with_color(faded(EDGE_RING, alpha))
            .with_width(1.0),
    );
    let icon = Rectangle::new(
        Point::new(rect.center_x() - ICON / 2.0, rect.y + PAD),
        Size::new(ICON, ICON),
    );
    screen_icon(f, icon, faded(INK, alpha));
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            words.sharing().to_owned(),
            Point::new(
                rect.center_x(),
                icon.y + ICON + ICON_GAP + WORD * 1.366 / 2.0,
            ),
            WORD,
            faded(INK, alpha),
            Alignment::Center,
        ),
    ));
}

/// How the recording's item is drawn.
#[derive(Debug, Clone, Copy)]
pub struct RecLook {
    pub open: bool,
    pub on: OnRec,
    pub motion: bool,
    pub alpha: f32,
    pub words: Words,
}

/// The recording's item at `rect`: its tint and ring, its dot and its
/// time; opened, pause and stop under them.
pub fn draw_rec(
    f: &mut Frame,
    rect: Rectangle,
    rec: &super::Rec,
    now: std::time::Instant,
    look: RecLook,
) {
    let alpha = look.alpha;
    let shape = kit::rounded(rect, RADIUS);
    f.fill(&shape, faded(REC_FILL, alpha));
    f.stroke(
        &kit::rounded(
            Rectangle::new(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
            ),
            RADIUS - 0.5,
        ),
        Stroke::default()
            .with_color(faded(REC_RING, alpha))
            .with_width(1.0),
    );
    let paused = rec.shown.paused;
    let elapsed = rec.elapsed(now);
    let dot_alpha = if paused || !look.motion {
        1.0
    } else {
        beat(elapsed)
    };
    let dot = Point::new(rect.center_x(), rect.y + REC_PAD + REC_DOT / 2.0);
    let colour = if paused { REC_AMBER } else { REC_RED };
    f.fill(
        &Path::circle(dot, REC_DOT / 2.0),
        faded(colour, alpha * dot_alpha),
    );
    let said = if paused {
        look.words.paused().to_owned()
    } else {
        elapsed_text(elapsed)
    };
    f.fill_text(kit::text_in(
        MONO,
        kit::text(
            said,
            Point::new(
                rect.center_x(),
                rect.y + REC_PAD + REC_DOT + REC_GAP + REC_LINE / 2.0,
            ),
            REC_TIME,
            faded(REC_INK, alpha),
            Alignment::Center,
        ),
    ));
    if !look.open {
        return;
    }
    let [pause, stop] = rec_buttons(rect);
    f.fill(
        &kit::rounded(pause, REC_BUTTON_RADIUS),
        faded(REC_PAUSE, alpha),
    );
    if look.on == OnRec::Pause {
        f.fill(
            &kit::rounded(pause, REC_BUTTON_RADIUS),
            faded(REC_HOVER, alpha),
        );
    }
    let k = 15.0 / 24.0;
    let o = Point::new(pause.center_x() - 7.5, pause.center_y() - 7.5);
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
    f.fill(&icon, faded(REC_ICON, alpha));
    f.fill(
        &kit::rounded(stop, REC_BUTTON_RADIUS),
        faded(REC_STOP, alpha),
    );
    if look.on == OnRec::Stop {
        f.fill(
            &kit::rounded(stop, REC_BUTTON_RADIUS),
            faded(REC_HOVER, alpha),
        );
    }
    let side = 13.0 * 16.0 / 24.0;
    let square = Rectangle::new(
        Point::new(stop.center_x() - side / 2.0, stop.center_y() - side / 2.0),
        Size::new(side, side),
    );
    f.fill(
        &kit::rounded(square, 3.0 * 13.0 / 24.0),
        faded(Color::WHITE, alpha),
    );
}

/// The time as the item says it.
fn elapsed_text(time: std::time::Duration) -> String {
    elapsed(time)
}

/// The mockup's screen-share icon (its 24 px drawing) in `rect`: a screen,
/// its foot, and an arrow up in it.
pub fn screen_icon(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(3.5, 5.0));
        b.line_to(p(20.5, 5.0));
        b.line_to(p(20.5, 16.0));
        b.line_to(p(3.5, 16.0));
        b.close();
        b.move_to(p(9.0, 20.0));
        b.line_to(p(15.0, 20.0));
        b.move_to(p(12.0, 16.0));
        b.line_to(p(12.0, 20.0));
        b.move_to(p(12.0, 8.0));
        b.line_to(p(12.0, 13.0));
        b.move_to(p(9.5, 10.5));
        b.line_to(p(12.0, 8.0));
        b.line_to(p(14.5, 10.5));
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(1.8 * k)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recordings_time_reads_and_its_dot_beats() {
        use std::time::Duration;
        assert_eq!(elapsed(Duration::from_secs(12)), "0:12");
        assert_eq!(elapsed(Duration::from_secs(724)), "12:04");
        assert_eq!(elapsed(Duration::from_secs(3723)), "1:02:03");
        assert!((beat(Duration::ZERO) - 1.0).abs() < 1e-4);
        assert!((beat(Duration::from_millis(700)) - BEAT_LOW).abs() < 1e-4);
        assert!((beat(BEAT) - 1.0).abs() < 1e-3);
        let buttons = rec_buttons(Rectangle::new(Point::ORIGIN, Size::new(48.0, REC_OPEN)));
        assert_eq!(buttons[0].y, 42.0);
        assert_eq!(buttons[1].y + buttons[1].height + REC_PAD, REC_OPEN);
    }

    #[test]
    fn the_sign_sits_where_the_agents_started_and_they_move_under_it() {
        let ((w, h), top) = placement(Form::Mini, false);
        assert_eq!(w, 54);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert!((h as f32 - (SIGN.height + 2.0 * ROOM)).abs() < 1.0);
        let ((w, _), top) = placement(Form::Full, false);
        assert_eq!((w, top), (194, crate::agents::draw::TOP - ROOM));
        // While switching, both forms' places on one surface.
        let ((_, h), top) = placement(Form::Full, true);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert!(top + h as f32 >= crate::agents::draw::TOP + SIGN.height);
    }
}
