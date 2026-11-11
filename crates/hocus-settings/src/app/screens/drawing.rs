//! The screens' drawing, where they are dragged into place, and the
//! trial's countdown.

use super::*;

/// The drawing's frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct View {
    /// Drawing pixels per logical pixel.
    pub(super) f: f32,
    /// Where the logical box's corner is on the drawing.
    pub(super) origin: (f32, f32),
    /// The logical box's corner.
    pub(super) corner: (i32, i32),
}

impl View {
    pub(super) fn of(known: &Screens, width: f32) -> Option<Self> {
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

    pub(super) fn rect_of(&self, r: Rect) -> Rectangle {
        Rectangle {
            x: self.origin.0 + (r.x - self.corner.0) as f32 * self.f,
            y: self.origin.1 + (r.y - self.corner.1) as f32 * self.f,
            width: r.w as f32 * self.f,
            height: r.h as f32 * self.f,
        }
    }

    /// A point of the drawing, logical.
    pub(super) fn logical(&self, p: Point) -> (i32, i32) {
        (
            ((p.x - self.origin.0) / self.f).round() as i32 + self.corner.0,
            ((p.y - self.origin.1) / self.f).round() as i32 + self.corner.1,
        )
    }
}

/// The drawing: dots, the screens, the bar on the primary one, where a
/// dragged one lands.
pub(super) struct Drawing {
    pub(super) drawn: Vec<Drawn>,
    pub(super) landing: Option<Rectangle>,
    pub(super) holding: Option<String>,
    pub(super) big: bool,
    pub(super) empty: Option<String>,
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
pub(super) struct Countdown {
    pub(super) left: u64,
    pub(super) of: u64,
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
