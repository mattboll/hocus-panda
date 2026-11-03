//! The capture's toolbar on its surface: the four modes, a separator, the
//! delay, the pointer and the green "Capturer", the 5.8 mockup's sizes ×
//! the launcher's zoom. The compositor blurs behind it, draws its shadow
//! and fades it in; the shell draws its tint, its rim and its buttons.

use hocus_link::{CAPTURE_DELAYS, CAPTURE_TOOLBAR_RADIUS, CaptureMode, LAUNCHER_ZOOM};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{Button, Words};
use crate::{
    levels::draw::{MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD},
};

const Z: f32 = LAUNCHER_ZOOM as f32;
/// 6 px inside its 1 px rim, its buttons 2 px apart, 36 px tall.
const PAD: f32 = 6.0 * Z;
const GAP: f32 = 2.0 * Z;
const BUTTON: f32 = 36.0 * Z;
const BORDER: f32 = 1.0;
/// A mode's and the delay's: 10 px in, the icon 6 px from the label.
const BUTTON_PAD: f32 = 10.0 * Z;
const MODE_ICON: f32 = 18.0 * Z;
const DELAY_ICON: f32 = 16.0 * Z;
const ICON_GAP: f32 = 6.0 * Z;
const LABEL: f32 = 12.5 * Z;
const BUTTON_RADIUS: f32 = 10.0 * Z;
/// The separator: 1 px, 26 px tall, 4 px from each side.
const SEPARATOR: (f32, f32) = (1.0, 26.0 * Z);
const SEPARATOR_MARGIN: f32 = 4.0 * Z;
/// "Capturer": 16 px in, 13 px 700, 4 px after the pointer's.
const SHOOT_PAD: f32 = 16.0 * Z;
const SHOOT_TEXT: f32 = 13.0 * Z;
const SHOOT_MARGIN: f32 = 4.0 * Z;
/// The icons' strokes: 1.8 of their 24.
const STROKE: f32 = 1.8;

/// The popovers' glass (bar.md §9) and the mockup's rim; opaque without
/// glass.
const GLASS: (Color, Color) = (
    Color::from_rgba8(22, 27, 42, 0.72),
    Color::from_rgba8(14, 17, 28, 0.82),
);
const OPAQUE: (Color, Color) = (rgb(0x161B2A), rgb(0x0E111C));
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.12);
const HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const CHOSEN: Color = Color::from_rgba8(108, 194, 135, 0.22);
const CHOSEN_RING: Color = Color::from_rgba8(108, 194, 135, 0.6);
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.12);
const GREEN: Color = rgb(0x6CC287);
const ON_GREEN: Color = rgb(0x0B1020);

/// Where the toolbar's buttons are, and its size.
#[derive(Debug, Clone)]
pub struct Layout {
    pub buttons: Vec<(Button, Rectangle)>,
    pub separator: Rectangle,
    pub size: Size,
}

impl Layout {
    /// The toolbar in `words`: its delay's button as wide as its widest
    /// label, so that it does not change size as the delay does.
    pub fn of(words: Words) -> Self {
        let label = |t: &str| kit::measure(t, LABEL, MANROPE_SEMIBOLD);
        let top = BORDER + PAD;
        let mut x = BORDER + PAD;
        let mut buttons = Vec::new();
        let mut push = |button: Button, width: f32, x: &mut f32| {
            buttons.push((
                button,
                Rectangle::new(Point::new(*x, top), Size::new(width, BUTTON)),
            ));
            *x += width + GAP;
        };
        for mode in CaptureMode::ALL {
            let width = BUTTON_PAD + MODE_ICON + ICON_GAP + label(words.mode(mode)) + BUTTON_PAD;
            push(Button::Mode(mode), width.ceil(), &mut x);
        }
        x += SEPARATOR_MARGIN;
        let separator = Rectangle::new(
            Point::new(x, top + (BUTTON - SEPARATOR.1) / 2.0),
            Size::new(SEPARATOR.0, SEPARATOR.1),
        );
        x += SEPARATOR.0 + SEPARATOR_MARGIN + GAP;
        let widest = CAPTURE_DELAYS
            .iter()
            .map(|d| label(&words.delay(*d)))
            .fold(0.0, f32::max);
        push(
            Button::Delay,
            (BUTTON_PAD + DELAY_ICON + ICON_GAP + widest + BUTTON_PAD).ceil(),
            &mut x,
        );
        push(Button::Pointer, BUTTON, &mut x);
        x += SHOOT_MARGIN;
        let shoot = kit::measure(words.shoot(), SHOOT_TEXT, MANROPE_BOLD);
        push(Button::Shoot, (2.0 * SHOOT_PAD + shoot).ceil(), &mut x);
        let width = x - GAP + PAD + BORDER;
        Self {
            buttons,
            separator,
            size: Size::new(width.ceil(), (2.0 * (BORDER + PAD) + BUTTON).ceil()),
        }
    }

    fn button_at(&self, at: Point) -> Option<Button> {
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(at))
            .map(|(b, _)| *b)
    }
}

/// The toolbar on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

impl<Message> canvas::Program<Message> for View {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(seen) = event else {
            return None;
        };
        let layout = Layout::of(self.words);
        let target = cursor
            .position_in(bounds)
            .and_then(|at| layout.button_at(at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        let clicked = super::with(|toolbar| {
            let toolbar = toolbar.as_mut().filter(|t| t.id == self.id)?;
            match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    (toolbar.hover != target).then(|| {
                        toolbar.hover = target;
                        None
                    })
                }
                mouse::Event::CursorLeft => {
                    toolbar.hover = None;
                    toolbar.pressed = None;
                    Some(None)
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    toolbar.pressed = target;
                    Some(None)
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    let pressed = toolbar.pressed.take();
                    Some(pressed.filter(|p| Some(*p) == target))
                }
                _ => None,
            }
        });
        let clicked = clicked?;
        if let Some(button) = clicked {
            super::clicked(self.id, button);
        }
        redraw
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let shown = super::with(|t| t.as_ref().filter(|t| t.id == self.id).cloned());
        if let Some(toolbar) = shown {
            self.draw_toolbar(&mut frame, &toolbar, bounds.size());
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let on = cursor
            .position_in(bounds)
            .and_then(|at| Layout::of(self.words).button_at(at));
        if on.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl View {
    fn draw_toolbar(&self, f: &mut Frame, toolbar: &super::Toolbar, size: Size) {
        let layout = Layout::of(self.words);
        let radius = CAPTURE_TOOLBAR_RADIUS as f32;
        let (top, bottom) = if self.glass { GLASS } else { OPAQUE };
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
        f.fill_rectangle(layout.separator.position(), layout.separator.size(), RULE);
        let bar = &toolbar.bar;
        for (button, rect) in &layout.buttons {
            let hovered = toolbar.hover == Some(*button);
            let shape = kit::rounded(*rect, BUTTON_RADIUS);
            match button {
                Button::Shoot => {
                    f.fill(&shape, GREEN);
                    f.fill_text(kit::text_in(
                        MANROPE_BOLD,
                        kit::text(
                            self.words.shoot().to_owned(),
                            rect.center(),
                            SHOOT_TEXT,
                            ON_GREEN,
                            Alignment::Center,
                        ),
                    ));
                    continue;
                }
                Button::Mode(mode) if bar.mode == *mode => {
                    f.fill(&shape, CHOSEN);
                    let ring = Rectangle::new(
                        Point::new(rect.x + 0.5, rect.y + 0.5),
                        Size::new(rect.width - 1.0, rect.height - 1.0),
                    );
                    f.stroke(
                        &kit::rounded(ring, BUTTON_RADIUS - 0.5),
                        Stroke::default().with_color(CHOSEN_RING).with_width(1.0),
                    );
                }
                Button::Pointer if bar.pointer => f.fill(&shape, CHOSEN),
                _ if hovered => f.fill(&shape, HOVER),
                _ => {}
            }
            let middle = rect.center_y();
            let (icon, label) = match button {
                Button::Mode(mode) => (MODE_ICON, Some(self.words.mode(*mode).to_owned())),
                Button::Delay => (DELAY_ICON, Some(self.words.delay(bar.delay))),
                Button::Pointer => (DELAY_ICON, None),
                Button::Shoot => continue,
            };
            let icon_x = match label {
                Some(_) => rect.x + BUTTON_PAD,
                None => rect.center_x() - icon / 2.0,
            };
            draw_icon(
                f,
                *button,
                Rectangle::new(
                    Point::new(icon_x, middle - icon / 2.0),
                    Size::new(icon, icon),
                ),
            );
            if let Some(label) = label {
                f.fill_text(kit::text_in(
                    MANROPE_SEMIBOLD,
                    kit::text(
                        label,
                        Point::new(icon_x + icon + ICON_GAP, middle),
                        LABEL,
                        TEXT,
                        Alignment::Left,
                    ),
                ));
            }
        }
    }
}

/// The mockup's icon of `button` (its 24 px drawing) in `rect`.
fn draw_icon(f: &mut Frame, button: Button, rect: Rectangle) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let stroke = Stroke::default()
        .with_color(TEXT)
        .with_width(STROKE * k)
        .with_line_join(LineJoin::Round)
        .with_line_cap(LineCap::Round);
    // Polylines in the drawing's coordinates; `true` closes one.
    let lines: Vec<(Vec<(f32, f32)>, bool)> = match button {
        Button::Mode(CaptureMode::Area) => vec![
            (vec![(5.0, 9.0), (5.0, 5.0), (9.0, 5.0)], false),
            (vec![(15.0, 5.0), (19.0, 5.0), (19.0, 9.0)], false),
            (vec![(19.0, 15.0), (19.0, 19.0), (15.0, 19.0)], false),
            (vec![(9.0, 19.0), (5.0, 19.0), (5.0, 15.0)], false),
        ],
        Button::Mode(CaptureMode::Window) => vec![
            (
                vec![(4.0, 6.0), (20.0, 6.0), (20.0, 18.0), (4.0, 18.0)],
                true,
            ),
            (vec![(4.0, 9.0), (20.0, 9.0)], false),
        ],
        Button::Mode(CaptureMode::Screen) => vec![
            (
                vec![(3.5, 5.0), (20.5, 5.0), (20.5, 16.0), (3.5, 16.0)],
                true,
            ),
            (vec![(9.0, 20.0), (15.0, 20.0)], false),
            (vec![(12.0, 16.0), (12.0, 20.0)], false),
        ],
        Button::Mode(CaptureMode::All) => vec![
            (
                vec![(2.5, 6.0), (11.0, 6.0), (11.0, 13.0), (2.5, 13.0)],
                true,
            ),
            (
                vec![(13.0, 6.0), (21.5, 6.0), (21.5, 13.0), (13.0, 13.0)],
                true,
            ),
            (vec![(6.0, 17.0), (18.0, 17.0)], false),
        ],
        Button::Delay => vec![
            (vec![(12.0, 9.0), (12.0, 13.0), (14.5, 15.0)], false),
            (vec![(10.0, 3.5), (14.0, 3.5)], false),
        ],
        Button::Pointer => vec![(
            vec![
                (6.0, 3.0),
                (18.0, 13.0),
                (12.5, 13.5),
                (15.5, 20.0),
                (13.0, 21.0),
                (10.0, 14.5),
                (6.0, 18.0),
            ],
            true,
        )],
        Button::Shoot => Vec::new(),
    };
    let path = Path::new(|b| {
        for (points, closed) in &lines {
            let mut points = points.iter();
            if let Some(&(x, y)) = points.next() {
                b.move_to(p(x, y));
            }
            for &(x, y) in points {
                b.line_to(p(x, y));
            }
            if *closed {
                b.close();
            }
        }
        if button == Button::Delay {
            b.circle(p(12.0, 13.0), 7.5 * k);
        }
    });
    f.stroke(&path, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    #[test]
    fn its_buttons_sit_in_a_row_and_the_delay_does_not_change_its_size() {
        let layout = Layout::of(Words::new(Lang::French));
        assert_eq!(layout.buttons.len(), 7);
        let mut last = 0.0;
        for (_, rect) in &layout.buttons {
            assert!(rect.x >= last, "{rect:?}");
            last = rect.x + rect.width;
            assert_eq!(rect.height, BUTTON);
        }
        assert!(last <= layout.size.width);
        assert_eq!(layout.size.height, (2.0 * (BORDER + PAD) + BUTTON).ceil());
        let pointer = layout
            .buttons
            .iter()
            .find(|(b, _)| *b == Button::Pointer)
            .map(|(_, r)| r.width);
        assert_eq!(pointer, Some(BUTTON));
    }
}
