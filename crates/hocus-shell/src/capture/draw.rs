//! The capture's toolbar on its surface: the Photo / Vidéo switch (5.10),
//! the modes, a separator, then in Photo the delay, the pointer and the
//! green "Capturer", in Vidéo Son, Micro and the red "Démarrer"; the 5.8
//! and 5.10 mockups' sizes × the launcher's zoom. The compositor blurs
//! behind it, draws its shadow and fades it in; the shell draws its tint,
//! its rim and its buttons.

use hocus_link::{CAPTURE_DELAYS, CAPTURE_TOOLBAR_RADIUS, CaptureBar, CaptureMode, LAUNCHER_ZOOM};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{Button, Words};
use crate::theme::Ink;
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
/// The switch (5.10): 3 px in its own tint, radius 10, 4 px before the
/// modes; its tabs 30 tall, 12 px in, 12.5 px 700, radius 8.
const SWITCH_PAD: f32 = 3.0 * Z;
const SWITCH_RADIUS: f32 = 10.0 * Z;
const SWITCH_MARGIN: f32 = 4.0 * Z;
const TAB: f32 = 30.0 * Z;
const TAB_PAD: f32 = 12.0 * Z;
const TAB_RADIUS: f32 = 8.0 * Z;
/// Son and Micro: 10 px in, their 16 px icon 6 px from the label; off, at
/// 0.6 of their opacity.
const TOGGLE_ICON: f32 = 16.0 * Z;
const OFF: f32 = 0.6;
/// "Démarrer": 14 px in, its 10 px white dot 7 px from the label.
const START_PAD: f32 = 14.0 * Z;
const START_DOT: f32 = 10.0 * Z;
const START_GAP: f32 = 7.0 * Z;

/// The popovers' glass (bar.md §9) and the mockup's rim; opaque without
/// glass.
const GLASS: (Ink, Ink) = (
    Ink::Dark(Color::from_rgba8(22, 27, 42, 0.72)),
    Ink::Dark(Color::from_rgba8(14, 17, 28, 0.82)),
);
const OPAQUE: (Ink, Ink) = (Ink::Dark(rgb(0x161B2A)), Ink::Dark(rgb(0x0E111C)));
const RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const CHOSEN: Ink = Ink::Accent(0.22);
const CHOSEN_RING: Ink = Ink::Accent(0.6);
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const GREEN: Ink = Ink::Accent(1.0);
const ON_GREEN: Ink = Ink::Dark(rgb(0x0B1020));
const SWITCH: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const TAB_TEXT: Ink = Ink::Dark(rgb(0xC9D0DC));
const PHOTO_ON: Ink = Ink::Dark(rgb(0xEEF1F6));
const RED: Ink = Ink::Dark(rgb(0xF2545B));
const WHITE: Color = Color::WHITE;

/// Where the toolbar's buttons are, and its size.
#[derive(Debug, Clone)]
pub struct Layout {
    pub buttons: Vec<(Button, Rectangle)>,
    pub separator: Rectangle,
    /// The Photo / Vidéo switch's tint, its tabs in `buttons`.
    pub switch: Rectangle,
    pub size: Size,
}

impl Layout {
    /// The toolbar in `words`, in Photo or in Vidéo (`video`): its delay's
    /// button as wide as its widest label, so that it does not change
    /// size as the delay does.
    pub fn of(words: Words, video: bool) -> Self {
        let label = |t: &str| kit::measure(t, LABEL, MANROPE_SEMIBOLD);
        let tab_label = |t: &str| kit::measure(t, LABEL, MANROPE_BOLD);
        let top = BORDER + PAD;
        let mut x = BORDER + PAD;
        let mut buttons = Vec::new();
        // The switch, at its left (5.10).
        let switch_x = x;
        x += SWITCH_PAD;
        for kind in [false, true] {
            let width = (2.0 * TAB_PAD + tab_label(words.kind(kind))).ceil();
            buttons.push((
                Button::Kind(kind),
                Rectangle::new(
                    Point::new(x, top + (BUTTON - TAB) / 2.0),
                    Size::new(width, TAB),
                ),
            ));
            x += width;
        }
        x += SWITCH_PAD;
        let switch = Rectangle::new(
            Point::new(switch_x, top + (BUTTON - TAB) / 2.0 - SWITCH_PAD),
            Size::new(x - switch_x, TAB + 2.0 * SWITCH_PAD),
        );
        x += SWITCH_MARGIN + GAP;
        let mut push = |button: Button, width: f32, x: &mut f32| {
            buttons.push((
                button,
                Rectangle::new(Point::new(*x, top), Size::new(width, BUTTON)),
            ));
            *x += width + GAP;
        };
        // No "all screens" for a video.
        for mode in CaptureMode::ALL
            .into_iter()
            .filter(|m| !video || *m != CaptureMode::All)
        {
            let width = BUTTON_PAD + MODE_ICON + ICON_GAP + label(words.mode(mode)) + BUTTON_PAD;
            push(Button::Mode(mode), width.ceil(), &mut x);
        }
        x += SEPARATOR_MARGIN;
        let separator = Rectangle::new(
            Point::new(x, top + (BUTTON - SEPARATOR.1) / 2.0),
            Size::new(SEPARATOR.0, SEPARATOR.1),
        );
        x += SEPARATOR.0 + SEPARATOR_MARGIN + GAP;
        if video {
            for (button, said) in [
                (Button::Sound, words.sound()),
                (Button::Micro, words.micro()),
            ] {
                let width = BUTTON_PAD + TOGGLE_ICON + ICON_GAP + label(said) + BUTTON_PAD;
                push(button, width.ceil(), &mut x);
            }
            x += SHOOT_MARGIN;
            let start = kit::measure(words.start(), SHOOT_TEXT, MANROPE_BOLD);
            push(
                Button::Shoot,
                (2.0 * START_PAD + START_DOT + START_GAP + start).ceil(),
                &mut x,
            );
        } else {
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
        }
        let width = x - GAP + PAD + BORDER;
        Self {
            buttons,
            separator,
            switch,
            size: Size::new(width.ceil(), (2.0 * (BORDER + PAD) + BUTTON).ceil()),
        }
    }
}

/// What nothing holds on the toolbar, under its buttons: its tint, its
/// rim, the separator and the switch's tint.
#[derive(Debug, Clone)]
pub struct Back {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

impl<Message> canvas::Program<Message> for Back {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let video = super::with(|t| t.as_ref().filter(|t| t.id == self.id).map(|t| t.bar.video));
        if let Some(video) = video {
            self.draw_back(&mut f, video, bounds.size());
        }
        vec![f.into_geometry()]
    }
}

impl Back {
    fn draw_back(&self, f: &mut Frame, video: bool, size: Size) {
        let layout = Layout::of(self.words, video);
        let radius = CAPTURE_TOOLBAR_RADIUS as f32;
        let (top, bottom) = if self.glass { GLASS } else { OPAQUE };
        let (top, bottom) = (top.get(), bottom.get());
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
            Stroke::default().with_color(RIM.get()).with_width(BORDER),
        );
        f.fill_rectangle(
            layout.separator.position(),
            layout.separator.size(),
            RULE.get(),
        );
        f.fill(&kit::rounded(layout.switch, SWITCH_RADIUS), SWITCH.get());
    }
}

/// Button `button` at `rect` (the surface's frame) as the toolbar `bar`
/// shows it, lit (`hovered`) under the pointer.
pub fn draw_button(
    f: &mut Frame,
    button: &Button,
    rect: &Rectangle,
    bar: &CaptureBar,
    words: Words,
    hovered: bool,
) {
    let shape = kit::rounded(*rect, BUTTON_RADIUS);
    match button {
        Button::Kind(kind) => {
            let on = bar.video == *kind;
            let (back, ink) = match (on, kind) {
                (true, true) => (Some(RED.get()), WHITE),
                (true, false) => (Some(PHOTO_ON.get()), ON_GREEN.get()),
                (false, _) => (hovered.then_some(HOVER.get()), TAB_TEXT.get()),
            };
            if let Some(back) = back {
                f.fill(&kit::rounded(*rect, TAB_RADIUS), back);
            }
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    words.kind(*kind).to_owned(),
                    rect.center(),
                    LABEL,
                    ink,
                    Alignment::Center,
                ),
            ));
            return;
        }
        Button::Shoot if bar.video => {
            f.fill(&shape, RED.get());
            let text = kit::measure(words.start(), SHOOT_TEXT, MANROPE_BOLD);
            let left = rect.center_x() - (START_DOT + START_GAP + text) / 2.0;
            f.fill(
                &Path::circle(
                    Point::new(left + START_DOT / 2.0, rect.center_y()),
                    START_DOT / 2.0,
                ),
                WHITE,
            );
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    words.start().to_owned(),
                    Point::new(left + START_DOT + START_GAP, rect.center_y()),
                    SHOOT_TEXT,
                    WHITE,
                    Alignment::Left,
                ),
            ));
            return;
        }
        Button::Sound | Button::Micro => {
            let on = if *button == Button::Sound {
                bar.sound
            } else {
                bar.micro
            };
            if on {
                f.fill(&shape, CHOSEN.get());
                let ring = Rectangle::new(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                );
                f.stroke(
                    &kit::rounded(ring, BUTTON_RADIUS - 0.5),
                    Stroke::default()
                        .with_color(CHOSEN_RING.get())
                        .with_width(1.0),
                );
            } else if hovered {
                crate::theme::hover(f, &shape, HOVER.get());
            }
            let alpha = if on { 1.0 } else { OFF };
            let middle = rect.center_y();
            let icon = Rectangle::new(
                Point::new(rect.x + BUTTON_PAD, middle - TOGGLE_ICON / 2.0),
                Size::new(TOGGLE_ICON, TOGGLE_ICON),
            );
            toggle_icon(
                f,
                *button,
                icon,
                Color {
                    a: alpha,
                    ..TEXT.get()
                },
            );
            let said = if *button == Button::Sound {
                words.sound()
            } else {
                words.micro()
            };
            f.fill_text(kit::text_in(
                MANROPE_SEMIBOLD,
                kit::text(
                    said.to_owned(),
                    Point::new(icon.x + TOGGLE_ICON + ICON_GAP, middle),
                    LABEL,
                    Color {
                        a: alpha,
                        ..TEXT.get()
                    },
                    Alignment::Left,
                ),
            ));
            return;
        }
        Button::Shoot => {
            f.fill(&shape, GREEN.get());
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    words.shoot().to_owned(),
                    rect.center(),
                    SHOOT_TEXT,
                    ON_GREEN.get(),
                    Alignment::Center,
                ),
            ));
            return;
        }
        Button::Mode(mode) if bar.mode == *mode => {
            f.fill(&shape, CHOSEN.get());
            let ring = Rectangle::new(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
            );
            f.stroke(
                &kit::rounded(ring, BUTTON_RADIUS - 0.5),
                Stroke::default()
                    .with_color(CHOSEN_RING.get())
                    .with_width(1.0),
            );
        }
        Button::Pointer if bar.pointer => f.fill(&shape, CHOSEN.get()),
        _ if hovered => crate::theme::hover(f, &shape, HOVER.get()),
        _ => {}
    }
    let middle = rect.center_y();
    let (icon, label) = match button {
        Button::Mode(mode) => (MODE_ICON, Some(words.mode(*mode).to_owned())),
        Button::Delay => (DELAY_ICON, Some(words.delay(bar.delay))),
        Button::Pointer => (DELAY_ICON, None),
        Button::Shoot | Button::Kind(_) | Button::Sound | Button::Micro => return,
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
                TEXT.get(),
                Alignment::Left,
            ),
        ));
    }
}

/// The mockup's icon of `button` (its 24 px drawing) in `rect`.
fn draw_icon(f: &mut Frame, button: Button, rect: Rectangle) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let stroke = Stroke::default()
        .with_color(TEXT.get())
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
        Button::Shoot | Button::Kind(_) | Button::Sound | Button::Micro => Vec::new(),
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

/// The mockup's sound and microphone icons (their 24 px drawings) in
/// `rect`.
fn toggle_icon(f: &mut Frame, button: Button, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        if button == Button::Sound {
            // A speaker, and two waves.
            b.move_to(p(4.0, 9.5));
            b.line_to(p(8.0, 9.5));
            b.line_to(p(13.0, 5.0));
            b.line_to(p(13.0, 19.0));
            b.line_to(p(8.0, 14.5));
            b.line_to(p(4.0, 14.5));
            b.close();
            b.move_to(p(16.5, 9.0));
            b.quadratic_curve_to(p(18.5, 12.0), p(16.5, 15.0));
            b.move_to(p(19.0, 6.5));
            b.quadratic_curve_to(p(23.0, 12.0), p(19.0, 17.5));
        } else {
            // A microphone on its stand.
            b.move_to(p(9.0, 6.5));
            b.arc_to(p(9.0, 3.5), p(12.0, 3.5), 3.0 * k);
            b.arc_to(p(15.0, 3.5), p(15.0, 6.5), 3.0 * k);
            b.line_to(p(15.0, 12.0));
            b.arc_to(p(15.0, 15.0), p(12.0, 15.0), 3.0 * k);
            b.arc_to(p(9.0, 15.0), p(9.0, 12.0), 3.0 * k);
            b.close();
            b.move_to(p(6.0, 11.5));
            b.quadratic_curve_to(p(6.0, 17.5), p(12.0, 17.5));
            b.quadratic_curve_to(p(18.0, 17.5), p(18.0, 11.5));
            b.move_to(p(12.0, 17.5));
            b.line_to(p(12.0, 20.5));
        }
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(STROKE * k)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    #[test]
    fn its_buttons_sit_in_a_row_and_the_delay_does_not_change_its_size() {
        let layout = Layout::of(Words::new(Lang::French), false);
        assert_eq!(layout.buttons.len(), 9);
        let mut last = 0.0;
        for (button, rect) in &layout.buttons {
            assert!(rect.x >= last, "{rect:?}");
            last = rect.x + rect.width;
            let tall = if matches!(button, Button::Kind(_)) {
                TAB
            } else {
                BUTTON
            };
            assert_eq!(rect.height, tall);
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

    #[test]
    fn in_video_the_switch_three_modes_son_micro_and_demarrer() {
        let layout = Layout::of(Words::new(Lang::French), true);
        let buttons: Vec<Button> = layout.buttons.iter().map(|(b, _)| *b).collect();
        assert_eq!(
            buttons,
            [
                Button::Kind(false),
                Button::Kind(true),
                Button::Mode(CaptureMode::Area),
                Button::Mode(CaptureMode::Window),
                Button::Mode(CaptureMode::Screen),
                Button::Sound,
                Button::Micro,
                Button::Shoot,
            ]
        );
        // The switch first, holding both tabs.
        let (_, photo) = layout.buttons[0];
        assert!(layout.switch.x < photo.x && layout.switch.contains(photo.center()));
        let mut last = 0.0;
        for (_, rect) in &layout.buttons {
            assert!(rect.x >= last, "{rect:?}");
            last = rect.x + rect.width;
        }
        assert!(last <= layout.size.width);
    }
}
