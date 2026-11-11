//! The bar's preview on the Bar page, drawn on a canvas.

use super::*;

/// A screen in the preview's row.
#[derive(Debug, Clone)]
pub(super) struct Mini {
    pub(super) rect: Rectangle,
    /// Its bar's width there, if it has one.
    pub(super) bar: Option<f32>,
    pub(super) plugged: bool,
    pub(super) caption: String,
}

/// The preview, as drawn.
pub(super) struct Preview {
    pub(super) label: String,
    pub(super) caption: String,
    pub(super) screens: Vec<Mini>,
    pub(super) screens_high: f32,
    /// The laptop's bar's width now, logical pixels.
    pub(super) width: f32,
    pub(super) form: Form,
    pub(super) meeting: bool,
    pub(super) windows: bool,
    pub(super) groups: Vec<BarGroup>,
    pub(super) hh: String,
    pub(super) mm: String,
    pub(super) ss: Option<String>,
    pub(super) half: Option<&'static str>,
    pub(super) date: String,
    pub(super) font: Font,
    pub(super) colour: Color,
    pub(super) played: Option<(&'static str, f32)>,
    pub(super) lang: Lang,
    /// The bars stand on the screens' left.
    pub(super) left: bool,
    /// Under it: what it is, and that a click lights a row.
    pub(super) hint: String,
}

/// The sample content's heights, logical pixels (the mockup's).
pub(super) const CARD: f32 = 44.0;
pub(super) const HEAD: f32 = 18.0;
pub(super) const TODAY: f32 = HEAD + 2.0 * 20.0;
pub(super) const TASKS: f32 = HEAD + 3.0 * 22.0;
pub(super) const AGENTS: f32 = HEAD + 3.0 * 34.0;
pub(super) const WINDOWS: f32 = 3.0 * 30.0;
pub(super) const GAP: f32 = 14.0;
/// The full form's lower groups, from the bar's bottom up: its 12 px of
/// padding, the footer, the levels, the connections, the desktops.
pub(super) const FOOTER_TOP: f32 = BAR_HIGH - 12.0 - 34.0;
pub(super) const LEVELS_TOP: f32 = FOOTER_TOP - GAP - 74.0;
pub(super) const CONNS_TOP: f32 = LEVELS_TOP - GAP - 94.0;
pub(super) const DESKTOPS_TOP: f32 = CONNS_TOP - GAP - 30.0;
/// The mini form's, from its bottom up (10 px of padding): the
/// notifications' bell and the avatar, the battery and the layout, the
/// Wi-Fi, the volume and Bluetooth, the desktops.
pub(super) const MINI_FOOTER_TOP: f32 = BAR_HIGH - 10.0 - 70.0;
pub(super) const MINI_LEVELS_TOP: f32 = MINI_FOOTER_TOP - GAP - 82.0;
pub(super) const MINI_CONNS_TOP: f32 = MINI_LEVELS_TOP - GAP - 152.0;
pub(super) const MINI_DESKTOPS_TOP: f32 = MINI_CONNS_TOP - GAP - 30.0;

/// The mockup's colours.
pub(super) const INK: Color = Color::from_rgb(
    0xEE as f32 / 255.0,
    0xF1 as f32 / 255.0,
    0xF6 as f32 / 255.0,
);
pub(super) const INK_DIM: Color = Color::from_rgb(
    0xA9 as f32 / 255.0,
    0xB2 as f32 / 255.0,
    0xC2 as f32 / 255.0,
);
pub(super) const INK_FAINT: Color = Color::from_rgb(
    0x8C as f32 / 255.0,
    0x96 as f32 / 255.0,
    0xA8 as f32 / 255.0,
);
pub(super) const BLUE_SOFT: Color = Color::from_rgb(
    0x8F as f32 / 255.0,
    0xB3 as f32 / 255.0,
    0xFF as f32 / 255.0,
);
pub(super) const AMBER: Color = Color::from_rgb(
    0xF5 as f32 / 255.0,
    0xB4 as f32 / 255.0,
    0x4A as f32 / 255.0,
);
pub(super) const GREEN: Color = Color::from_rgb(
    0x6C as f32 / 255.0,
    0xC2 as f32 / 255.0,
    0x87 as f32 / 255.0,
);

impl Preview {
    /// Where the bar starts in the canvas: under the label and the
    /// screens.
    pub(super) fn bar_top(&self) -> f32 {
        22.0 + self.screens_high.min(70.0) + 24.0
    }

    pub(super) fn full(&self) -> bool {
        self.form == Form::Full && self.width > 120.0
    }

    /// The groups of the bar, by the row each lights, at their logical
    /// places in the bar: the clock, then the column in its order, the
    /// windows over the desktops.
    pub(super) fn layout(&self) -> Vec<(&'static str, Rectangle)> {
        let full = self.full();
        let (x, width) = if full { (12.0, 172.0) } else { (6.0, 44.0) };
        let clock = if full {
            42.0 * 1.05 + 6.0 + 21.0
        } else {
            19.0 * 1.1 * 2.0
        };
        let mut y = if full { 14.0 } else { 12.0 };
        let mut out = vec![(
            model::SECONDS,
            Rectangle::new(Point::new(x, y), Size::new(width, clock)),
        )];
        y += clock + GAP;
        let mut put = |id: &'static str, high: f32, out: &mut Vec<(&'static str, Rectangle)>| {
            out.push((id, Rectangle::new(Point::new(x, y), Size::new(width, high))));
            y += high + GAP;
        };
        if self.meeting {
            put(model::MEETING, if full { CARD } else { 4.0 }, &mut out);
        }
        for g in &self.groups {
            let high = match (g, full) {
                (BarGroup::Today, true) => TODAY,
                (BarGroup::Today, false) => continue,
                (BarGroup::Tasks, true) => TASKS,
                (BarGroup::Tasks, false) => 34.0,
                (BarGroup::Agents, true) => AGENTS,
                (BarGroup::Agents, false) => 68.0,
            };
            put(model::group_setting(*g), high, &mut out);
        }
        if self.windows {
            let (top, high) = if full {
                (DESKTOPS_TOP - GAP - WINDOWS, WINDOWS)
            } else {
                (MINI_DESKTOPS_TOP - GAP - 34.0, 34.0)
            };
            out.push((
                model::WINDOWS,
                Rectangle::new(Point::new(x, top), Size::new(width, high)),
            ));
        }
        out
    }

    /// The bar's top left in the canvas, and the screen's edge's x: the
    /// edge on the bar's outer side, 8 px (at 55 %) from it.
    pub(super) fn bar_origin(&self, bounds: Rectangle) -> (Point, f32) {
        if self.left {
            let edge_x = 0.0;
            (
                Point::new(edge_x + 2.0 + 8.0 * BAR_SCALE, self.bar_top()),
                edge_x,
            )
        } else {
            let edge_x = bounds.width - 2.0;
            let x = edge_x - 8.0 * BAR_SCALE - self.width * BAR_SCALE;
            (Point::new(x, self.bar_top()), edge_x)
        }
    }
}

/// `s` at `at` (its top left; centred on `at.x` when `centred`).
pub(super) fn write(
    f: &mut canvas::Frame,
    s: &str,
    at: Point,
    size: f32,
    font: Font,
    color: Color,
    centred: bool,
) {
    f.fill_text(canvas::Text {
        content: s.to_owned(),
        position: at,
        color,
        size: size.into(),
        font,
        align_x: if centred {
            iced::alignment::Horizontal::Center.into()
        } else {
            iced::alignment::Horizontal::Left.into()
        },
        ..canvas::Text::default()
    });
}

/// `s` with its right at `right`.
pub(super) fn write_right(
    f: &mut canvas::Frame,
    s: &str,
    right: f32,
    y: f32,
    size: f32,
    font: Font,
    color: Color,
) {
    let w = crate::app::style::measure(s, size, font);
    write(f, s, Point::new(right - w, y), size, font, color, false);
}

pub(super) fn rounded(f: &mut canvas::Frame, at: Point, size: Size, radius: f32, fill: Color) {
    f.fill(
        &canvas::Path::rounded_rectangle(at, size, radius.into()),
        fill,
    );
}

/// An agent's panda's head: white, its ring in its state's colour.
pub(super) fn head(f: &mut canvas::Frame, centre: Point, radius: f32, ring: Color) {
    f.fill(
        &canvas::Path::circle(centre, radius),
        Color::from_rgb8(0x1c, 0x21, 0x30),
    );
    f.fill(
        &canvas::Path::circle(Point::new(centre.x, centre.y + radius * 0.1), radius * 0.48),
        Color::from_rgb8(0xF4, 0xF6, 0xFA),
    );
    f.stroke(
        &canvas::Path::circle(centre, radius),
        canvas::Stroke::default().with_color(ring).with_width(1.5),
    );
}

impl canvas::Program<Message> for Preview {
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let at = cursor.position_in(bounds)?;
        let (origin, _) = self.bar_origin(bounds);
        let local = Point::new((at.x - origin.x) / BAR_SCALE, (at.y - origin.y) / BAR_SCALE);
        let (id, _) = self
            .layout()
            .into_iter()
            .rev()
            .find(|(_, r)| r.contains(local))?;
        Some(canvas::Action::publish(Message::Bar(Msg::Lit(id))).and_capture())
    }

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The label line.
        write(
            &mut frame,
            &self.label,
            Point::new(0.0, 2.0),
            11.5,
            BOLD,
            DIM.get(),
            false,
        );
        write_right(
            &mut frame,
            &self.caption,
            bounds.width,
            2.0,
            11.5,
            MANROPE,
            FAINT.get(),
        );
        // The screens, as Écrans places them, each its caption under it.
        for s in &self.screens {
            let rect = Rectangle::new(Point::new(s.rect.x, 22.0 + s.rect.y), s.rect.size());
            let path = canvas::Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into());
            if s.plugged {
                frame.fill(
                    &path,
                    canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            rect.position(),
                            Point::new(rect.x + rect.width * 0.35, rect.y + rect.height),
                        )
                        .add_stop(0.0, Color::from_rgb8(0x1d, 0x2a, 0x44))
                        .add_stop(1.0, Color::from_rgb8(0x12, 0x1a, 0x2b)),
                    ),
                );
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(rgba(255, 255, 255, 0.18))
                        .with_width(1.0),
                );
                if let Some(bar) = s.bar {
                    let w = bar.max(2.0);
                    rounded(
                        &mut frame,
                        Point::new(
                            if self.left {
                                rect.x + 1.0
                            } else {
                                rect.x + rect.width - 1.0 - w
                            },
                            rect.y + 1.0,
                        ),
                        Size::new(w, (rect.height - 2.0).max(1.0)),
                        2.0,
                        rgba(238, 241, 246, 0.55),
                    );
                }
            } else {
                frame.stroke(
                    &path,
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[3.0, 3.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(rgba(255, 255, 255, 0.24))
                            .with_width(1.0)
                    },
                );
            }
            write(
                &mut frame,
                &s.caption,
                Point::new(rect.x + rect.width / 2.0, rect.y + rect.height + 4.0),
                10.5,
                MANROPE,
                INK_DIM,
                true,
            );
        }
        // The laptop's bar at 55 %, 8 px (at 55 %) from its screen's edge.
        let (origin, edge_x) = self.bar_origin(bounds);
        let top = self.bar_top();
        rounded(
            &mut frame,
            Point::new(edge_x, top - 6.0),
            Size::new(2.0, BAR_HIGH * BAR_SCALE + 12.0),
            1.0,
            rgba(255, 255, 255, 0.22),
        );
        frame.with_save(|f| {
            f.translate(iced::Vector::new(origin.x, origin.y));
            f.scale(BAR_SCALE);
            self.draw_bar(f);
        });
        frame.fill_text(canvas::Text {
            content: self.hint.clone(),
            position: Point::new(0.0, top + BAR_HIGH * BAR_SCALE + 14.0),
            color: FAINT.get(),
            size: 12.0.into(),
            font: MANROPE,
            max_width: bounds.width,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

impl Preview {
    /// The bar at its logical sizes, `self.width` wide, 960 high, with the
    /// mockup's sample content.
    pub(super) fn draw_bar(&self, f: &mut canvas::Frame) {
        let bw = self.width;
        let full = self.full();
        let body =
            canvas::Path::rounded_rectangle(Point::ORIGIN, Size::new(bw, BAR_HIGH), 14.0.into());
        f.fill(
            &body,
            canvas::Gradient::Linear(
                canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, BAR_HIGH))
                    .add_stop(0.0, Color::from_rgba8(22, 27, 42, 0.95))
                    .add_stop(1.0, Color::from_rgba8(14, 17, 28, 0.98)),
            ),
        );
        f.stroke(
            &body,
            canvas::Stroke::default()
                .with_color(rgba(255, 255, 255, 0.10))
                .with_width(1.0),
        );
        let soft = |a: f32| rgba(255, 255, 255, a);
        let lang = self.lang;
        // The minutes' transition, played once.
        let (lift, alpha) = match self.played {
            Some(("odometer", p)) => ((1.0 - ease(p)) * 30.0, ease(p)),
            Some(("roll", p)) => ((1.0 - ease(p)) * 18.0, p),
            Some(("flip", p)) => (0.0, ease(p)),
            Some(("neon", p)) => (
                0.0,
                if (0.12..0.22).contains(&p) || (0.36..0.5).contains(&p) {
                    0.2
                } else {
                    1.0
                },
            ),
            _ => (0.0, 1.0),
        };
        let minutes = Color {
            a: alpha,
            ..self.colour
        };
        let measure = crate::app::style::measure;
        if full {
            // HH:MM at 42 px, the colon pulled in, the seconds (or AM/PM)
            // after on the same baseline; the date under them.
            let hh = measure(&self.hh, 42.0, self.font);
            let colon = measure(":", 42.0, self.font) - 2.0 * 0.12 * 42.0;
            let mm = measure(&self.mm, 42.0, self.font);
            let aside = self.half.map(str::to_owned).or_else(|| self.ss.clone());
            let (aside_font, aside_size) = match self.half {
                Some(_) => (MANROPE, 13.0),
                None => (MONO, 18.0),
            };
            let aside_w = aside
                .as_ref()
                .map_or(0.0, |a| 4.0 + measure(a, aside_size, aside_font));
            let mut x = (bw - (hh + colon + mm + aside_w)) / 2.0;
            write(
                f,
                &self.hh,
                Point::new(x, 14.0),
                42.0,
                self.font,
                self.colour,
                false,
            );
            x += hh;
            write(
                f,
                ":",
                Point::new(x - 0.12 * 42.0, 14.0),
                42.0,
                self.font,
                self.colour,
                false,
            );
            x += colon;
            write(
                f,
                &self.mm,
                Point::new(x, 14.0 + lift),
                42.0,
                self.font,
                minutes,
                false,
            );
            x += mm;
            if let Some(aside) = aside {
                write(
                    f,
                    &aside,
                    Point::new(x + 4.0, 14.0 + 42.0 - aside_size - 4.0),
                    aside_size,
                    aside_font,
                    INK_FAINT,
                    false,
                );
            }
            write(
                f,
                &self.date,
                Point::new(bw / 2.0, 14.0 + 42.0 * 1.05 + 6.0),
                15.5,
                MANROPE,
                INK_DIM,
                true,
            );
        } else {
            let centre = bw / 2.0;
            write(
                f,
                &self.hh,
                Point::new(centre, 12.0),
                19.0,
                self.font,
                self.colour,
                true,
            );
            write(
                f,
                &self.mm,
                Point::new(centre, 12.0 + 20.9 + lift * 0.5),
                19.0,
                self.font,
                minutes,
                true,
            );
        }
        for (id, r) in self.layout() {
            if !full {
                draw_mini_item(f, bw, id, r, lang);
                continue;
            }
            match id {
                model::MEETING => {
                    rounded(
                        f,
                        r.position(),
                        r.size(),
                        10.0,
                        Color::from_rgba8(143, 179, 255, 0.10),
                    );
                    f.with_save(|g| {
                        g.fill_rectangle(
                            Point::new(r.x, r.y + 2.0),
                            Size::new(6.0, r.height - 4.0),
                            BLUE_SOFT,
                        );
                    });
                    write(
                        f,
                        w("Point d’équipe", "Team check-in").get(lang),
                        Point::new(r.x + 14.0, r.y + 5.0),
                        13.5,
                        BOLD,
                        INK,
                        false,
                    );
                    write(
                        f,
                        w("14:30 · dans 48 min", "14:30 · in 48 min").get(lang),
                        Point::new(r.x + 14.0, r.y + 24.0),
                        12.0,
                        MANROPE,
                        INK_DIM,
                        false,
                    );
                    f.stroke(
                        &canvas::Path::new(|b| {
                            let (x, y) = (r.x + r.width - 24.0, r.y + 14.0);
                            b.move_to(Point::new(x + 2.3, y + 4.7));
                            b.line_to(Point::new(x + 10.0, y + 4.7));
                            b.line_to(Point::new(x + 10.0, y + 11.3));
                            b.line_to(Point::new(x + 2.3, y + 11.3));
                            b.close();
                            b.move_to(Point::new(x + 10.0, y + 7.0));
                            b.line_to(Point::new(x + 13.7, y + 5.0));
                            b.line_to(Point::new(x + 13.7, y + 11.0));
                            b.line_to(Point::new(x + 10.0, y + 9.0));
                        }),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb8(0xA9, 0xC4, 0xFF))
                            .with_width(1.2),
                    );
                }
                model::TODAY => {
                    write(
                        f,
                        w("AUJOURD’HUI", "TODAY").get(lang),
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    for (i, (at, what, dot)) in [
                        ("16:00", w("Revue du budget", "Budget review"), GREEN),
                        ("18:30", w("Cours de yoga", "Yoga class"), AMBER),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 20.0;
                        write(f, at, Point::new(r.x, y + 3.0), 12.0, MONO, INK_DIM, false);
                        f.fill(
                            &canvas::Path::circle(Point::new(r.x + 52.0, y + 10.0), 3.5),
                            *dot,
                        );
                        write(
                            f,
                            what.get(lang),
                            Point::new(r.x + 62.0, y + 2.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                    }
                }
                model::TASKS => {
                    write(
                        f,
                        w("À FAIRE", "TO DO").get(lang),
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    write_right(f, "+", r.x + r.width, r.y, 14.0, MANROPE, INK_DIM);
                    for (i, what) in [
                        w("Relire la page de la barre", "Review the bar's page"),
                        w("Rappeler le garage", "Call the garage back"),
                        w("Acheter du café", "Buy coffee"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 22.0;
                        f.stroke(
                            &canvas::Path::rounded_rectangle(
                                Point::new(r.x + 4.0, y + 4.0),
                                Size::new(14.0, 14.0),
                                4.0.into(),
                            ),
                            canvas::Stroke::default()
                                .with_color(soft(0.38))
                                .with_width(1.5),
                        );
                        write(
                            f,
                            what.get(lang),
                            Point::new(r.x + 28.0, y + 3.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                    }
                }
                model::AGENTS => {
                    write(
                        f,
                        "AGENTS",
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    write_right(f, "3", r.x + r.width, r.y + 2.0, 11.0, MANROPE, INK_DIM);
                    for (i, (name, says, ring, lit)) in [
                        ("hocus-panda", w("t’attend", "waiting"), AMBER, true),
                        ("site-web", w("12 min", "12 min"), GREEN, false),
                        ("carnet", w("repos", "resting"), soft(0.20), false),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 34.0;
                        if *lit {
                            rounded(
                                f,
                                Point::new(r.x - 6.0, y),
                                Size::new(r.width + 12.0, 34.0),
                                8.0,
                                Color::from_rgba8(245, 180, 74, 0.10),
                            );
                        }
                        head(f, Point::new(r.x + 13.0, y + 17.0), 13.0, *ring);
                        write(
                            f,
                            name,
                            Point::new(r.x + 34.0, y + 8.0),
                            13.5,
                            SEMIBOLD,
                            INK,
                            false,
                        );
                        let (colour, font) = match i {
                            0 => (AMBER, BOLD),
                            1 => (INK_DIM, MANROPE),
                            _ => (INK_FAINT, MANROPE),
                        };
                        write_right(
                            f,
                            says.get(lang),
                            r.x + r.width,
                            y + 10.0,
                            12.0,
                            font,
                            colour,
                        );
                    }
                }
                model::WINDOWS => {
                    for (i, (name, colour)) in [
                        (
                            w("Terminal — hocus-panda", "Terminal — hocus-panda"),
                            Color::from_rgb8(0x5B, 0x8D, 0xEF),
                        ),
                        (
                            w("Navigateur — Agenda", "Browser — Calendar"),
                            Color::from_rgb8(0xF2, 0x94, 0x5B),
                        ),
                        (w("Fichiers — Images", "Files — Pictures"), GREEN),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + i as f32 * 30.0;
                        if i == 0 {
                            rounded(
                                f,
                                Point::new(r.x - 6.0, y),
                                Size::new(r.width + 12.0, 30.0),
                                8.0,
                                soft(0.08),
                            );
                        }
                        rounded(
                            f,
                            Point::new(r.x, y + 7.0),
                            Size::new(16.0, 16.0),
                            4.0,
                            *colour,
                        );
                        let name = elided(name.get(lang), 13.0, MANROPE, r.width - 24.0 - 18.0);
                        write(
                            f,
                            &name,
                            Point::new(r.x + 24.0, y + 7.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                        write_right(f, "✕", r.x + r.width, y + 8.0, 11.0, MANROPE, INK_FAINT);
                    }
                }
                _ => {}
            }
        }
        if full {
            // The desktops, the connections, the levels, the footer.
            let tile = (172.0 - 3.0 * 6.0) / 4.0;
            for i in 0..4 {
                let at = Point::new(12.0 + i as f32 * (tile + 6.0), DESKTOPS_TOP);
                rounded(f, at, Size::new(tile, 30.0), 6.0, soft(0.05));
                f.stroke(
                    &canvas::Path::rounded_rectangle(at, Size::new(tile, 30.0), 6.0.into()),
                    canvas::Stroke::default()
                        .with_color(if i == 0 { GREEN } else { soft(0.10) })
                        .with_width(if i == 0 { 2.0 } else { 1.0 }),
                );
                if i < 2 {
                    rounded(
                        f,
                        Point::new(at.x + tile * 0.12, at.y + 7.5),
                        Size::new(tile * 0.6, 15.0),
                        2.0,
                        Color::from_rgba8(236, 239, 244, 0.30),
                    );
                } else {
                    write(
                        f,
                        &(i + 1).to_string(),
                        Point::new(at.x + tile / 2.0, at.y + 8.0),
                        11.0,
                        SEMIBOLD,
                        INK_FAINT,
                        true,
                    );
                }
            }
            for (i, (name, right)) in [
                ("Maison", ""),
                (w("Casque", "Headset").get(lang), "82 %"),
                (w("Français", "French").get(lang), "FR"),
            ]
            .iter()
            .enumerate()
            {
                let y = CONNS_TOP + i as f32 * 32.0;
                f.stroke(
                    &canvas::Path::circle(Point::new(21.0, y + 15.0), 7.0),
                    canvas::Stroke::default().with_color(INK).with_width(1.6),
                );
                write(
                    f,
                    name,
                    Point::new(38.0, y + 6.0),
                    14.5,
                    MANROPE,
                    INK,
                    false,
                );
                if i == 2 {
                    rounded(
                        f,
                        Point::new(184.0 - 26.0, y + 6.0),
                        Size::new(26.0, 18.0),
                        6.0,
                        soft(0.08),
                    );
                    write(
                        f,
                        right,
                        Point::new(184.0 - 13.0, y + 7.0),
                        12.0,
                        BOLD,
                        INK_DIM,
                        true,
                    );
                } else if !right.is_empty() {
                    write_right(f, right, 184.0, y + 8.0, 13.0, MANROPE, INK_DIM);
                }
            }
            for (i, (pc, a, b)) in [
                (0.60, (0x5B, 0x8D, 0xEF), (0x8F, 0xB3, 0xFF)),
                (0.70, (0xE8, 0xB4, 0x4E), (0xFF, 0xE0, 0x8A)),
                (0.82, (0x3F, 0xA6, 0x6B), (0x7E, 0xE0, 0xA0)),
            ]
            .iter()
            .enumerate()
            {
                let y = LEVELS_TOP + i as f32 * 28.0;
                f.stroke(
                    &canvas::Path::circle(Point::new(21.0, y + 9.0), 6.0),
                    canvas::Stroke::default().with_color(INK).with_width(1.6),
                );
                let (x, w) = (38.0, 108.0);
                rounded(
                    f,
                    Point::new(x, y + 5.0),
                    Size::new(w, 8.0),
                    4.0,
                    soft(0.10),
                );
                f.fill(
                    &canvas::Path::rounded_rectangle(
                        Point::new(x, y + 5.0),
                        Size::new(w * pc, 8.0),
                        4.0.into(),
                    ),
                    canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            Point::new(x, 0.0),
                            Point::new(x + w * pc, 0.0),
                        )
                        .add_stop(0.0, Color::from_rgb8(a.0, a.1, a.2))
                        .add_stop(1.0, Color::from_rgb8(b.0, b.1, b.2)),
                    ),
                );
                write_right(
                    f,
                    &format!("{}", (pc * 100.0).round()),
                    184.0,
                    y + 1.0,
                    14.5,
                    MANROPE,
                    INK_DIM,
                );
            }
            for i in 0..3 {
                rounded(
                    f,
                    Point::new(12.0 + i as f32 * 44.0, FOOTER_TOP),
                    Size::new(40.0, 34.0),
                    10.0,
                    soft(0.05),
                );
            }
            f.fill(
                &canvas::Path::circle(Point::new(184.0 - 20.0, FOOTER_TOP + 17.0), 12.0),
                GREEN,
            );
            write(
                f,
                "A",
                Point::new(184.0 - 20.0, FOOTER_TOP + 9.0),
                11.0,
                BOLD,
                Color::from_rgb8(0x0b, 0x10, 0x20),
                true,
            );
        } else {
            draw_mini_lower(f, bw);
        }
    }
}

/// A 44 × `high` button of the mini form, centred, its top at `y`.
pub(super) fn mini_button(f: &mut canvas::Frame, centre: f32, y: f32, high: f32) {
    rounded(
        f,
        Point::new(centre - 22.0, y),
        Size::new(44.0, high),
        10.0,
        rgba(255, 255, 255, 0.05),
    );
}

/// A line through `points` (an icon's strokes), in a 24 px box at `at`
/// drawn `size` px.
pub(super) fn icon_lines(
    f: &mut canvas::Frame,
    at: Point,
    size: f32,
    points: &[(f32, f32)],
    colour: Color,
) {
    let k = size / 24.0;
    let path = canvas::Path::new(|b| {
        for (i, (x, y)) in points.iter().enumerate() {
            let p = Point::new(at.x + x * k, at.y + y * k);
            if i == 0 {
                b.move_to(p);
            } else {
                b.line_to(p);
            }
        }
    });
    f.stroke(
        &path,
        canvas::Stroke::default()
            .with_color(colour)
            .with_width(1.8 * k)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round),
    );
}

/// The mini form's item of group `id`, in `r` (the mockup's §5 items).
pub(super) fn draw_mini_item(f: &mut canvas::Frame, bw: f32, id: &str, r: Rectangle, lang: Lang) {
    let c = bw / 2.0;
    match id {
        model::MEETING => rounded(
            f,
            Point::new(c - 15.0, r.y),
            Size::new(30.0, 4.0),
            2.0,
            BLUE_SOFT,
        ),
        model::TASKS => {
            mini_button(f, c, r.y, 34.0);
            f.stroke(
                &canvas::Path::rounded_rectangle(
                    Point::new(c - 14.0, r.y + 10.0),
                    Size::new(13.0, 13.0),
                    3.5.into(),
                ),
                canvas::Stroke::default()
                    .with_color(INK_DIM)
                    .with_width(1.5),
            );
            write(
                f,
                "3",
                Point::new(c + 2.0, r.y + 9.0),
                11.0,
                MONO,
                INK_DIM,
                false,
            );
        }
        model::AGENTS => {
            head(f, Point::new(c, r.y + 20.0), 20.0, AMBER);
            f.fill(
                &canvas::Path::circle(Point::new(c + 14.0, r.y + 34.0), 10.0),
                Color::from_rgb8(0x12, 0x17, 0x26),
            );
            f.fill(
                &canvas::Path::circle(Point::new(c + 14.0, r.y + 34.0), 8.0),
                AMBER,
            );
            write(
                f,
                "1",
                Point::new(c + 14.0, r.y + 28.0),
                10.0,
                BOLD,
                Color::from_rgb8(0x1C, 0x14, 0x06),
                true,
            );
            write(
                f,
                "1",
                Point::new(c, r.y + 44.0),
                11.0,
                SEMIBOLD,
                INK_DIM,
                true,
            );
            write(
                f,
                w("au travail", "working").get(lang),
                Point::new(c, r.y + 55.0),
                11.0,
                SEMIBOLD,
                Color::from_rgb8(0x7C, 0x86, 0x98),
                true,
            );
        }
        model::WINDOWS => {
            mini_button(f, c, r.y, 34.0);
            // A window: its frame and its title bar.
            let at = Point::new(c - 9.0, r.y + 8.0);
            icon_lines(
                f,
                at,
                18.0,
                &[
                    (3.5, 5.0),
                    (20.5, 5.0),
                    (20.5, 19.0),
                    (3.5, 19.0),
                    (3.5, 5.0),
                ],
                BLUE_SOFT,
            );
            icon_lines(f, at, 18.0, &[(3.5, 9.0), (20.5, 9.0)], BLUE_SOFT);
        }
        _ => {}
    }
}

/// The mini form's lower groups: the desktops, the connections, the
/// levels, the footer.
pub(super) fn draw_mini_lower(f: &mut canvas::Frame, bw: f32) {
    let c = bw / 2.0;
    let soft = |a: f32| rgba(255, 255, 255, a);
    // The desktops: two by two, the current one green.
    for i in 0..4 {
        let at = Point::new(
            c - 20.0 + (i % 2) as f32 * 22.0,
            MINI_DESKTOPS_TOP + (i / 2) as f32 * 17.0,
        );
        let fill = match i {
            0 => GREEN,
            1 => soft(0.30),
            _ => soft(0.10),
        };
        rounded(f, at, Size::new(18.0, 13.0), 4.0, fill);
    }
    // The Wi-Fi, the volume's gauge, Bluetooth.
    let y = MINI_CONNS_TOP;
    mini_button(f, c, y, 38.0);
    let wifi = Point::new(c, y + 26.0);
    for (radius, alpha) in [(4.0_f32, 1.0), (8.5, 1.0), (13.0, 1.0)] {
        f.stroke(
            &canvas::Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: wifi,
                    radius,
                    start_angle: iced::Radians(-2.4),
                    end_angle: iced::Radians(-0.74),
                });
            }),
            canvas::Stroke::default()
                .with_color(Color { a: alpha, ..INK })
                .with_width(1.8)
                .with_line_cap(canvas::LineCap::Round),
        );
    }
    let gauge = Point::new(c - 4.0, y + 38.0 + 2.0 + 12.0);
    rounded(f, gauge, Size::new(8.0, 48.0), 4.0, soft(0.10));
    f.fill(
        &canvas::Path::rounded_rectangle(
            Point::new(gauge.x, gauge.y + 48.0 * 0.4),
            Size::new(8.0, 48.0 * 0.6),
            4.0.into(),
        ),
        canvas::Gradient::Linear(
            canvas::gradient::Linear::new(
                Point::new(0.0, gauge.y + 48.0),
                Point::new(0.0, gauge.y + 48.0 * 0.4),
            )
            .add_stop(0.0, Color::from_rgb8(0x5B, 0x8D, 0xEF))
            .add_stop(1.0, BLUE_SOFT),
        ),
    );
    let bt = y + 38.0 + 2.0 + 72.0 + 2.0;
    mini_button(f, c, bt, 38.0);
    icon_lines(
        f,
        Point::new(c - 10.0, bt + 9.0),
        20.0,
        &[
            (7.0, 7.5),
            (17.0, 16.5),
            (12.0, 20.5),
            (12.0, 3.5),
            (17.0, 7.5),
            (7.0, 16.5),
        ],
        INK,
    );
    // The battery at 82 %, its words, the layout.
    let y = MINI_LEVELS_TOP;
    let battery = Point::new(c - 13.0, y);
    f.stroke(
        &canvas::Path::rounded_rectangle(battery, Size::new(26.0, 15.0), 3.0.into()),
        canvas::Stroke::default().with_color(INK).with_width(1.5),
    );
    rounded(
        f,
        Point::new(battery.x + 3.0, y + 3.0),
        Size::new(20.0 * 0.82, 9.0),
        1.5,
        Color::from_rgb8(0x7E, 0xE0, 0xA0),
    );
    write(
        f,
        "82 %",
        Point::new(c, y + 19.0),
        14.0,
        BOLD,
        Color::from_rgb8(0x7E, 0xE0, 0xA0),
        true,
    );
    mini_button(f, c, y + 52.0, 30.0);
    write(f, "FR", Point::new(c, y + 59.0), 13.0, BOLD, INK, true);
    // The bell, the avatar.
    let y = MINI_FOOTER_TOP;
    mini_button(f, c, y, 34.0);
    icon_lines(
        f,
        Point::new(c - 9.0, y + 8.0),
        18.0,
        &[
            (6.0, 16.5),
            (6.0, 11.0),
            (8.0, 7.0),
            (12.0, 5.5),
            (16.0, 7.0),
            (18.0, 11.0),
            (18.0, 16.5),
            (19.5, 18.0),
            (4.5, 18.0),
            (6.0, 16.5),
        ],
        INK,
    );
    icon_lines(
        f,
        Point::new(c - 9.0, y + 8.0),
        18.0,
        &[(10.0, 20.5), (14.0, 20.5)],
        INK,
    );
    f.fill(
        &canvas::Path::circle(Point::new(c, y + 36.0 + 5.0 + 12.0), 12.0),
        canvas::Gradient::Linear(
            canvas::gradient::Linear::new(
                Point::new(c - 10.0, y + 41.0),
                Point::new(c + 10.0, y + 65.0),
            )
            .add_stop(0.0, GREEN)
            .add_stop(1.0, Color::from_rgb8(0x5B, 0x8D, 0xEF)),
        ),
    );
    write(
        f,
        "A",
        Point::new(c, y + 46.0),
        10.0,
        BOLD,
        Color::from_rgb8(0x0b, 0x10, 0x20),
        true,
    );
}

/// Ease-out, cubic.
pub(super) fn ease(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}
