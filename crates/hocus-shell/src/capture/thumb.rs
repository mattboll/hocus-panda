//! The capture's thumbnail (5.9, variant A of its mockup): at the bottom
//! right of the captured screen, against the bar, 8 px from it and from
//! the screen's bottom. The image scaled to fit, "Copiée · enregistrée",
//! "Annoter" and ✕; a 2 px line counting its 6 s down, which the pointer
//! on it pauses.
//!
//! - A click on the image or on "Annoter" opens the editor; ✕ deletes the
//!   capture; the image dragged drops its file into an application (the
//!   compositor drags it).
//! - Its time up, it goes, and the capture goes to the notifications'
//!   history with its image and its actions.
//!
//! A layer surface of its own ([`hocus_link::CAPTURE_THUMB_NAMESPACE`]), on the
//! overlay layer, taking the pointer, never the keyboard. The compositor
//! slides it in and draws its shadow; the shell draws its card, the
//! mockup's sizes × the launcher's zoom.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{CAPTURE_THUMB_RADIUS, CaptureAsk, EDGE, LAUNCHER_ZOOM};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::image::Handle,
    mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{Shot, Words, marks};
use crate::{
    levels::{
        draw::{MANROPE_BOLD, TEXT, rgb},
        icons::Icon,
    },
    popover::{Effect, kit},
};

const Z: f32 = LAUNCHER_ZOOM as f32;
/// The card: 220 wide, 8 in, its image 128 tall, 8 over the row of 26.
pub const WIDTH: f32 = 220.0 * Z;
const PAD: f32 = 8.0 * Z;
const IMAGE: f32 = 128.0 * Z;
const ROW_GAP: f32 = 8.0 * Z;
const ROW: f32 = 26.0 * Z;
pub const HEIGHT: f32 = PAD + IMAGE + ROW_GAP + ROW + PAD;
const BORDER: f32 = 1.0;
const IMAGE_RADIUS: f32 = 10.0 * Z;
/// "Copiée · enregistrée", 11 px.
const STATUS: f32 = 11.0 * Z;
/// "Annoter": 9 px in, radius 7, 11.5 px bold; ✕ 26 square, 12 px; 4 apart.
const BUTTON_PAD: f32 = 9.0 * Z;
const BUTTON_RADIUS: f32 = 7.0 * Z;
const BUTTON_TEXT: f32 = 11.5 * Z;
const CLOSE_SIZE: f32 = 12.0 * Z;
const BUTTON_GAP: f32 = 4.0 * Z;
/// The countdown: 2 px, 10 px from the sides, on the card's bottom.
const COUNTDOWN: f32 = 2.0 * Z;
const COUNTDOWN_IN: f32 = 10.0 * Z;
/// It draws again a pixel at a time, at 30 frames a second at most.
const COUNTDOWN_STEP: Duration = Duration::from_millis(33);
/// How long it stays.
pub const LIFE: Duration = Duration::from_secs(6);
/// A press moved this far on the image drags it.
const DRAG: f32 = 6.0;
/// Its gap to the bar, and to the screen's bottom.
const GAP: i32 = 8;

const TOP: Color = Color::from_rgba8(22, 27, 42, 0.96);
const BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.98);
const OPAQUE: (Color, Color) = (rgb(0x161B2A), rgb(0x0E111C));
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.12);
const IMAGE_RIM: Color = Color::from_rgba8(255, 255, 255, 0.18);
const IMAGE_BACK: Color = Color::from_rgba8(0, 0, 0, 0.25);
const STATUS_COLOUR: Color = rgb(0xA9B2C2);
const GREEN_TINT: Color = Color::from_rgba8(108, 194, 135, 0.22);
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const CLOSE_COLOUR: Color = rgb(0xC9D0DC);
const RUNNING: Color = Color::from_rgba8(108, 194, 135, 0.7);
const PAUSED: Color = Color::from_rgba8(255, 255, 255, 0.35);

/// What its card shows of the capture, made on the thread that read it:
/// the image at the size it shows (the screen's pixels), and the
/// history's icon.
#[derive(Debug, Clone)]
pub struct Made {
    pub image: Handle,
    /// The image's size, pixels, and the screen's scale it was made for:
    /// it shows at most a pixel a pixel.
    pub size: (u32, u32),
    pub scale: f32,
    pub icon: Icon,
}

impl Made {
    /// What `capture` shows at the screen's `scale`.
    pub fn of(capture: &marks::Image, scale: f32) -> Self {
        let room = (WIDTH - 2.0 * PAD) * scale;
        let (w, h) = fit(capture.dimensions(), (room, IMAGE * scale));
        let small = Handle::from_rgba(w, h, resize(capture, w, h).into_raw());
        Self {
            image: small,
            size: (w, h),
            scale: scale.max(0.1),
            icon: icon(capture, (ICON * scale.max(1.0)).round() as u32),
        }
    }
}

/// The history's icon side, logical pixels (5.4's).
const ICON: f32 = 28.0;

/// `size` scaled to fit in `room`, never larger than it is.
pub fn fit(size: (u32, u32), room: (f32, f32)) -> (u32, u32) {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let k = (room.0 / w).min(room.1 / h).min(1.0);
    (
        ((w * k).round() as u32).max(1),
        ((h * k).round() as u32).max(1),
    )
}

/// `capture` at `w` × `h`.
pub fn resize(capture: &marks::Image, w: u32, h: u32) -> marks::Image {
    if capture.dimensions() == (w, h) {
        return capture.clone();
    }
    image::imageops::resize(capture, w, h, image::imageops::FilterType::Triangle)
}

/// The capture as a square `side` pixels, its middle, its corners rounded
/// as the history's tiles are.
fn icon(capture: &marks::Image, side: u32) -> Icon {
    let (w, h) = capture.dimensions();
    let square = w.min(h).max(1);
    let cut =
        image::imageops::crop_imm(capture, (w - square) / 2, (h - square) / 2, square, square)
            .to_image();
    let mut small = resize(&cut, side, side);
    let r = side as f32 * 0.28;
    let s = side as f32;
    for (x, y, pixel) in small.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let cx = px.clamp(r, s - r);
        let cy = py.clamp(r, s - r);
        let d = (px - cx).hypot(py - cy);
        let keep = (r + 0.5 - d).clamp(0.0, 1.0);
        pixel.0[3] = (f32::from(pixel.0[3]) * keep).round() as u8;
    }
    Icon::Raster(Handle::from_rgba(side, side, small.into_raw()))
}

/// Where a press on the card goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Image,
    Annotate,
    Close,
    Card,
}

/// Its countdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Countdown {
    Running { until: Instant },
    Paused { left: Duration },
}

impl Countdown {
    /// What is left at `now`, from 1 to 0.
    pub fn left(self, now: Instant) -> f32 {
        let left = match self {
            Self::Running { until } => until.saturating_duration_since(now),
            Self::Paused { left } => left,
        };
        (left.as_secs_f32() / LIFE.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// The thumbnail shown.
#[derive(Debug, Clone)]
pub struct Thumb {
    pub id: window::Id,
    pub shot: Shot,
    pub made: Made,
    pub countdown: Countdown,
    pub hover: Option<Spot>,
    /// Pressed where, and from where; whether it was dragged since.
    pressed: Option<(Spot, Point)>,
    dragged: bool,
}

static THUMB: Mutex<Option<Thumb>> = Mutex::new(None);

fn with<T>(with: impl FnOnce(&mut Option<Thumb>) -> T) -> T {
    with(&mut THUMB.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether surface `id` is the thumbnail's.
pub fn is_surface(id: window::Id) -> bool {
    with(|t| t.as_ref().is_some_and(|t| t.id == id))
}

/// Its margins from the screen's bottom right corner: past the bar and
/// its gap, or the gap alone on a screen without a bar.
fn margin(output: &str) -> (i32, i32, i32, i32) {
    let bars = crate::popover::with(|c| c.bar_forms());
    let bar = bars
        .iter()
        .find(|(b, _)| b == output)
        .map_or(0, |(_, form)| form.width() + EDGE);
    (0, bar + GAP, GAP, 0)
}

/// Capture `shot`'s thumbnail shows from `now`, in place of one shown
/// (which goes to the history).
pub fn show(shot: Shot, made: Made, now: Instant) {
    let id = window::Id::unique();
    let mut effects = Vec::new();
    let before = with(|thumb| {
        let before = thumb.take();
        tracing::debug!(capture = shot.id, file = %shot.file.display(), "the capture's thumbnail shows");
        effects.push(Effect::CaptureThumb {
            id,
            output: shot.output.clone(),
            size: (WIDTH.ceil() as u32, HEIGHT.ceil() as u32),
            margin: margin(&shot.output),
        });
        *thumb = Some(Thumb {
            id,
            shot,
            made,
            countdown: Countdown::Running { until: now + LIFE },
            hover: None,
            pressed: None,
            dragged: false,
        });
        before
    });
    if let Some(old) = before {
        effects.insert(0, Effect::Remove(old.id));
        to_history(&old);
    }
    crate::popover::send(effects);
}

/// When its time is up.
pub fn deadline() -> Option<Instant> {
    with(|t| match t.as_ref()?.countdown {
        Countdown::Running { until } => Some(until),
        Countdown::Paused { .. } => None,
    })
}

/// What is due at `now`: its time up, it goes, to the history.
pub fn tick(now: Instant) -> Vec<Effect> {
    let gone = with(|thumb| match thumb.take() {
        Some(t) if matches!(t.countdown, Countdown::Running { until } if until <= now) => Some(t),
        kept => {
            *thumb = kept;
            None
        }
    });
    let Some(gone) = gone else {
        return Vec::new();
    };
    tracing::debug!(
        capture = gone.shot.id,
        "the capture's thumbnail's time is up"
    );
    to_history(&gone);
    vec![Effect::Remove(gone.id)]
}

/// The capture goes to the notifications' history.
fn to_history(thumb: &Thumb) {
    crate::notifications::keep_capture(&thumb.shot, Some(thumb.made.icon.clone()));
}

/// The thumbnail goes without going to the history: the editor opens, or
/// the capture was deleted.
fn take(id: window::Id) -> Option<Thumb> {
    with(|thumb| {
        if thumb.as_ref().is_some_and(|t| t.id == id) {
            thumb.take()
        } else {
            None
        }
    })
}

/// Where the card's pieces are, in its frame.
#[derive(Debug, Clone, Copy)]
struct Layout {
    image: Rectangle,
    annotate: Rectangle,
    close: Rectangle,
    row_middle: f32,
}

impl Layout {
    fn of(words: Words) -> Self {
        let image = Rectangle::new(Point::new(PAD, PAD), Size::new(WIDTH - 2.0 * PAD, IMAGE));
        let top = PAD + IMAGE + ROW_GAP;
        let close = Rectangle::new(Point::new(WIDTH - PAD - ROW, top), Size::new(ROW, ROW));
        let label = kit::measure(words.annotate(), BUTTON_TEXT, MANROPE_BOLD);
        let width = (label + 2.0 * BUTTON_PAD).ceil();
        let annotate = Rectangle::new(
            Point::new(close.x - BUTTON_GAP - width, top),
            Size::new(width, ROW),
        );
        Self {
            image,
            annotate,
            close,
            row_middle: top + ROW / 2.0,
        }
    }

    fn spot(&self, at: Point) -> Spot {
        if self.image.contains(at) {
            Spot::Image
        } else if self.annotate.contains(at) {
            Spot::Annotate
        } else if self.close.contains(at) {
            Spot::Close
        } else {
            Spot::Card
        }
    }
}

/// The thumbnail on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    /// On the CPU, whose surfaces have no transparency: opaque, the
    /// compositor rounds its corners.
    pub software: bool,
}

/// The canvas's own: when it last drew.
#[derive(Debug, Default)]
pub struct State {
    drawn: Option<Instant>,
}

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                state.drawn = Some(*now);
                let running = with(|t| {
                    t.as_ref().is_some_and(|t| {
                        t.id == self.id && matches!(t.countdown, Countdown::Running { .. })
                    })
                });
                // The line shortens a pixel at a time, while it runs.
                running.then(|| {
                    let pixel = LIFE.div_f32(WIDTH - 2.0 * COUNTDOWN_IN);
                    canvas::Action::request_redraw_at(*now + pixel.max(COUNTDOWN_STEP))
                })
            }
            iced::Event::Mouse(seen) => self.mouse(seen, bounds, cursor),
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
        let shown = with(|t| t.as_ref().filter(|t| t.id == self.id).cloned());
        if let Some(thumb) = shown {
            let now = state.drawn.unwrap_or_else(Instant::now);
            self.draw_card(&mut frame, &thumb, now);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let layout = Layout::of(self.words);
        match cursor.position_in(bounds).map(|at| layout.spot(at)) {
            Some(Spot::Image | Spot::Annotate | Spot::Close) => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}

/// What a click on the thumbnail does.
enum Act {
    Edit(Thumb),
    Delete(Thumb),
    Drag(u64),
}

impl View {
    fn mouse<Message>(
        &self,
        seen: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let layout = Layout::of(self.words);
        let at = cursor.position_in(bounds);
        let spot = at.map(|p| layout.spot(p));
        let now = Instant::now();
        let mut act = None;
        let mut resumed = false;
        let redraw = with(|thumb| {
            let thumb = thumb.as_mut().filter(|t| t.id == self.id)?;
            match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    let mut changed = thumb.hover != spot;
                    thumb.hover = spot;
                    // The pointer on it holds it.
                    if spot.is_some()
                        && let Countdown::Running { until } = thumb.countdown
                    {
                        thumb.countdown = Countdown::Paused {
                            left: until.saturating_duration_since(now),
                        };
                        tracing::debug!(capture = thumb.shot.id, "the capture's thumbnail is held");
                        changed = true;
                    }
                    if let (Some((Spot::Image, from)), Some(p)) = (thumb.pressed, at)
                        && !thumb.dragged
                        && (p.x - from.x).hypot(p.y - from.y) >= DRAG
                    {
                        thumb.dragged = true;
                        act = Some(Act::Drag(thumb.shot.id));
                    }
                    Some(changed)
                }
                mouse::Event::CursorLeft => {
                    thumb.hover = None;
                    thumb.pressed = None;
                    thumb.dragged = false;
                    if let Countdown::Paused { left } = thumb.countdown {
                        thumb.countdown = Countdown::Running { until: now + left };
                        resumed = true;
                        tracing::debug!(capture = thumb.shot.id, "the capture's thumbnail goes on");
                    }
                    Some(true)
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    thumb.pressed = spot.zip(at);
                    thumb.dragged = false;
                    Some(false)
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    let pressed = thumb.pressed.take().map(|(s, _)| s);
                    let dragged = std::mem::take(&mut thumb.dragged);
                    if !dragged && pressed.is_some() && pressed == spot {
                        act = match pressed {
                            Some(Spot::Image | Spot::Annotate) => Some(Act::Edit(thumb.clone())),
                            Some(Spot::Close) => Some(Act::Delete(thumb.clone())),
                            _ => None,
                        };
                    }
                    Some(false)
                }
                _ => None,
            }
        });
        if resumed {
            // The effects' loop waits for its new deadline.
            crate::popover::send(Vec::new());
        }
        match act {
            Some(Act::Drag(capture)) => {
                tracing::debug!(capture, "the capture's thumbnail is dragged");
                super::tell(CaptureAsk::Drag(capture));
            }
            Some(Act::Edit(shown)) => {
                if take(shown.id).is_some() {
                    tracing::debug!(
                        capture = shown.shot.id,
                        "the capture's thumbnail opens the editor"
                    );
                    crate::popover::send(vec![Effect::Remove(shown.id)]);
                    super::editor::open(shown.shot);
                }
                return Some(canvas::Action::capture());
            }
            Some(Act::Delete(shown)) => {
                if take(shown.id).is_some() {
                    crate::popover::send(vec![Effect::Remove(shown.id)]);
                    super::delete(shown.shot.id);
                }
                return Some(canvas::Action::capture());
            }
            None => {}
        }
        match redraw? {
            true => Some(canvas::Action::request_redraw()),
            false => None,
        }
    }

    fn draw_card(&self, f: &mut Frame, thumb: &Thumb, now: Instant) {
        let size = Size::new(WIDTH, HEIGHT);
        let radius = CAPTURE_THUMB_RADIUS as f32;
        let (top, bottom) = if self.software { OPAQUE } else { (TOP, BOTTOM) };
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, radius.into()),
            canvas::Fill::from(tint),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (radius - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM).with_width(BORDER),
        );
        let layout = Layout::of(self.words);
        // The image, scaled to fit, in the middle of its place.
        let place = layout.image;
        f.fill(&kit::rounded(place, IMAGE_RADIUS), IMAGE_BACK);
        let (w, h) = thumb.made.size;
        let scale = (place.width / w as f32)
            .min(place.height / h as f32)
            .min(1.0 / thumb.made.scale);
        let shown = fit_in(place, (w, h), scale);
        // Its corners rounded: the radius cuts the image's clip.
        let mut picture = canvas::Image::new(thumb.made.image.clone());
        picture.border_radius = IMAGE_RADIUS.into();
        f.with_clip(shown, |f| f.draw_image(shown, picture));
        f.stroke(
            &kit::rounded(
                Rectangle::new(
                    Point::new(place.x + 0.5, place.y + 0.5),
                    Size::new(place.width - 1.0, place.height - 1.0),
                ),
                IMAGE_RADIUS - 0.5,
            ),
            Stroke::default().with_color(IMAGE_RIM).with_width(1.0),
        );
        // "Copiée · enregistrée", "Annoter", ✕.
        f.fill_text(kit::text(
            self.words.kept().to_owned(),
            Point::new(PAD, layout.row_middle),
            STATUS,
            STATUS_COLOUR,
            Alignment::Left,
        ));
        let annotate = layout.annotate;
        f.fill(&kit::rounded(annotate, BUTTON_RADIUS), GREEN_TINT);
        if thumb.hover == Some(Spot::Annotate) {
            f.fill(&kit::rounded(annotate, BUTTON_RADIUS), HOVER);
        }
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                self.words.annotate().to_owned(),
                annotate.center(),
                BUTTON_TEXT,
                TEXT,
                Alignment::Center,
            ),
        ));
        let close = layout.close;
        if thumb.hover == Some(Spot::Close) {
            f.fill(&kit::rounded(close, BUTTON_RADIUS), HOVER);
        }
        cross(f, close.center(), CLOSE_SIZE * 0.6, CLOSE_COLOUR);
        // The countdown.
        let left = thumb.countdown.left(now);
        let full = WIDTH - 2.0 * COUNTDOWN_IN;
        let colour = match thumb.countdown {
            Countdown::Running { .. } => RUNNING,
            Countdown::Paused { .. } => PAUSED,
        };
        if left > 0.0 {
            f.fill_rectangle(
                Point::new(COUNTDOWN_IN, HEIGHT - COUNTDOWN),
                Size::new((full * left).round(), COUNTDOWN),
                colour,
            );
        }
    }
}

/// An image `size` pixels drawn `scale` logical pixels a pixel in the
/// middle of `place`.
pub fn fit_in(place: Rectangle, size: (u32, u32), scale: f32) -> Rectangle {
    let (w, h) = (size.0 as f32 * scale, size.1 as f32 * scale);
    Rectangle::new(
        Point::new(
            place.x + (place.width - w) / 2.0,
            place.y + (place.height - h) / 2.0,
        ),
        Size::new(w, h),
    )
}

/// A ✕ `size` across, centred on `at`.
pub fn cross(f: &mut Frame, at: Point, size: f32, colour: Color) {
    let h = size / 2.0;
    let path = Path::new(|b| {
        b.move_to(Point::new(at.x - h, at.y - h));
        b.line_to(Point::new(at.x + h, at.y + h));
        b.move_to(Point::new(at.x + h, at.y - h));
        b.line_to(Point::new(at.x - h, at.y + h));
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(1.8 * Z)
            .with_line_cap(canvas::LineCap::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    #[test]
    fn an_image_is_scaled_to_fit_never_larger() {
        assert_eq!(fit((2560, 1600), (306.0, 192.0)), (306, 191));
        assert_eq!(fit((4480, 1440), (306.0, 192.0)), (306, 98));
        assert_eq!(fit((100, 50), (306.0, 192.0)), (100, 50));
        assert_eq!(fit((600, 2000), (306.0, 192.0)), (58, 192));
    }

    #[test]
    fn its_card_is_the_mockups_zoomed_its_buttons_on_its_row() {
        assert_eq!(WIDTH, 330.0);
        assert_eq!(HEIGHT, 267.0);
        let layout = Layout::of(Words::new(Lang::French));
        assert_eq!(
            layout.image,
            Rectangle::new(Point::new(12.0, 12.0), Size::new(306.0, 192.0))
        );
        assert_eq!(layout.close.x + layout.close.width, WIDTH - PAD);
        assert!(layout.annotate.x + layout.annotate.width <= layout.close.x);
        assert_eq!(layout.spot(Point::new(100.0, 100.0)), Spot::Image);
        assert_eq!(layout.spot(layout.close.center()), Spot::Close);
        assert_eq!(layout.spot(layout.annotate.center()), Spot::Annotate);
        assert_eq!(layout.spot(Point::new(20.0, 230.0)), Spot::Card);
    }

    #[test]
    fn the_countdown_pauses_and_goes_on_from_where_it_was() {
        let t = Instant::now();
        let running = Countdown::Running { until: t + LIFE };
        assert_eq!(running.left(t), 1.0);
        assert_eq!(running.left(t + Duration::from_secs(3)), 0.5);
        let paused = Countdown::Paused {
            left: Duration::from_secs(3),
        };
        assert_eq!(paused.left(t + Duration::from_secs(100)), 0.5);
    }
}
