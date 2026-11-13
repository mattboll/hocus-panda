//! What a project holds (docs/design/projects.md §4, board 5), its card
//! while it is open, on the widget kit, the mockup's sizes.
//!
//! 600 px wide, 18 inside, its parts 14 apart, radius 16. Its monogram on
//! a 42 px badge (radius 12, the accent's gradient), its name (17 px,
//! 700) and the pill "ouvert · bureau 1" (11.5 px, 700, the accent's
//! tint), its folder under them (Martian Mono 11.5 px, `text.faint`),
//! "depuis 09:12" on the right (12 px). Then the desktop's sketch (160 ×
//! 100, as the bar's tiles draw it) beside "FENÊTRES · 3" and a 28 px row
//! per window: its icon, its title (13 px), where it is (11.5 px). Then,
//! on a faint ground, that private windows are never read nor kept.
//!
//! The terminals' folders, the tabs and the agents' conversations come
//! with the lots that read them (B, C, D); closing with B.

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    alignment::Vertical,
    mouse,
    widget::{
        Space,
        canvas::{self, Frame, Path, Stroke},
        column, container, row,
    },
    window,
};

use super::{Card, Projects, Words};
use crate::theme::Ink;
use crate::{
    levels::draw::{LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit,
    surfaces::Lent,
    ui::{
        self, Around,
        parts::{drawn, gap, spaced_by},
        style::said,
    },
};

type El<'a> = Element<'a, Msg>;

const WIDTH: f32 = hocus_link::PROJECT_CARD_WIDTH as f32;
const BORDER: f32 = 1.0;
const PAD: f32 = 18.0;
const INNER: f32 = WIDTH - 2.0 * (BORDER + PAD);
const GAP: f32 = 14.0;
const RADIUS: f32 = hocus_link::PROJECT_CARD_RADIUS as f32;
const BADGE: f32 = 42.0;
const BADGE_RADIUS: f32 = 12.0;
const BADGE_TEXT: f32 = 14.0;
const NAME: f32 = 17.0;
const PILL: f32 = 11.5;
const FOLDER: f32 = 11.5;
const WHEN: f32 = 12.0;
const SKETCH: Size = Size::new(160.0, 100.0);
const HEADING: f32 = 11.0;
const HEADING_BOTTOM: f32 = 4.0;
const ROW: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
const ROW_PAD: f32 = 6.0;
const ICON: f32 = 16.0;
const TITLE: f32 = 13.0;
const PLACE: f32 = 11.5;
const PRIVATE: f32 = 12.0;
const PRIVATE_PAD: (f32, f32) = (8.0, 10.0);
/// The windows listed at most; the sketch shows them all.
const MOST: usize = 8;

const MARTIAN: iced::Font = iced::Font::with_name("Martian Mono");
const MANROPE_EXTRABOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::ExtraBold,
    ..MANROPE
};

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.97));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.98));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const FAINT: Ink = Ink::Dark(rgb(0x8C96A8));
const PILL_FILL: Ink = Ink::Accent(0.18);
const PRIVATE_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.035));
const ON_ACCENT: Ink = Ink::Fixed(rgb(0x0B1020));
const SKETCH_TOP: Ink = Ink::Dark(rgb(0x121B32));
const SKETCH_BOTTOM: Ink = Ink::Dark(rgb(0x090D1A));
const SKETCH_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const WINDOW: Ink = Ink::Dark(rgba(236, 239, 244, 0.20));
const WINDOW_FOCUSED: Ink = Ink::Dark(rgba(236, 239, 244, 0.40));

/// What a widget asks: nothing but to be built again.
#[derive(Debug, Clone)]
pub enum Msg {
    Key(crate::popover::Pressed),
    Stale,
}

/// A window of the card: its id, icon's app, title and place.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: u64,
    app: String,
    title: String,
    place: hocus_link::WindowPlace,
}

/// The windows' rows, front first, as the bar's tiles have them.
fn rows(shown: &hocus_link::ProjectCard) -> Vec<Row> {
    let titles = crate::desktops::titles(&shown.output);
    let desktops = crate::desktops::snapshot(&shown.output);
    shown
        .windows
        .iter()
        .map(|(id, place)| {
            let app = desktops
                .as_ref()
                .and_then(|d| d.windows.iter().find(|w| w.id == *id))
                .map(|w| w.app.clone())
                .unwrap_or_default();
            let title = titles
                .as_ref()
                .and_then(|t| t.titles.iter().find(|(i, _)| i == id))
                .map(|(_, t)| t.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| app.clone());
            Row {
                id: *id,
                app,
                title,
                place: *place,
            }
        })
        .collect()
}

fn windows_height(count: usize) -> f32 {
    let n = count.min(MOST) as f32;
    let rows = if n > 0.0 {
        n * ROW + (n - 1.0) * ROW_GAP
    } else {
        0.0
    };
    (HEADING * LINE + HEADING_BOTTOM + rows).max(SKETCH.height)
}

/// Its size, logical pixels.
pub fn size(shown: &hocus_link::ProjectCard) -> (u32, u32) {
    let private = PRIVATE * LINE + 2.0 * PRIVATE_PAD.0;
    let height =
        2.0 * (BORDER + PAD) + BADGE + GAP + windows_height(shown.windows.len()) + GAP + private;
    (WIDTH as u32, height.ceil() as u32)
}

/// The card on its surface `id`, on the glass (else opaque).
pub fn view<'a>(
    projects: &'a Lent<Projects>,
    id: window::Id,
    words: Words,
    glass: bool,
) -> Element<'a, crate::Message> {
    Around::new(
        id,
        build(projects, id, words),
        move |m| handle(projects, m),
        Msg::Stale,
    )
    .own(hocus_link::PROJECT_CARD_NAMESPACE, move || {
        projects.read(|p| p.card_on(id).is_some()).unwrap_or(false)
    })
    .rebuild(move || build(projects, id, words))
    .keys(move |pressed| Some(Msg::Key(pressed.clone())))
    .on(iced::widget::canvas(Tint {
        radius: RADIUS,
        top: if glass { GLASS_TOP } else { OPAQUE_TOP },
        bottom: if glass { GLASS_BOTTOM } else { OPAQUE_BOTTOM },
        rim: RIM,
    })
    .width(Length::Fill)
    .height(Length::Fill))
}

fn handle(projects: &Lent<Projects>, message: Msg) {
    let Msg::Key(pressed) = message else {
        return;
    };
    let now = std::time::Instant::now();
    let done = projects.change(|p| {
        let mut effects = Vec::new();
        if matches!(
            pressed.key,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
        ) {
            p.close(now, &mut effects);
        }
        (effects, p.due().is_some())
    });
    let Some((effects, due)) = done else {
        return;
    };
    if !effects.is_empty() {
        crate::popover::send(effects);
    }
    if due {
        crate::surfaces::ask(crate::surfaces::Asked::Project(super::Come::Tick));
    }
}

fn build<'a>(projects: &'a Lent<Projects>, id: window::Id, words: Words) -> El<'a> {
    projects
        .change(|p| {
            let card = p.card.as_deref_mut().filter(|c| c.surface == id)?;
            let rows = rows(&card.shown);
            let now = shows(card, &rows, words);
            if now != card.logged {
                tracing::debug!(said = %now, "the project's card shows it");
                card.logged = now;
            }
            Some(content(card, &rows, words))
        })
        .flatten()
        .unwrap_or_else(|| Space::new().into())
}

/// Since when it is open, as the clock says it.
fn since(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// What it shows, in words, for the log and the tests.
fn shows(card: &Card, rows: &[Row], words: Words) -> String {
    let windows: Vec<String> = rows
        .iter()
        .map(|r| format!("{} ({})", r.title, words.place(r.place)))
        .collect();
    format!(
        "name={:?} desktop={} folder={:?} since={:?} windows={windows:?}",
        card.shown.name,
        card.shown.desktop + 1,
        card.shown.folder,
        since(card.shown.since),
    )
}

fn content<'a>(card: &Card, rows: &[Row], words: Words) -> El<'a> {
    let shown = &card.shown;
    let mono = hocus_link::projects::monogram(&shown.name);
    let badge = drawn(BADGE, BADGE, move |f, s| {
        let rect = Rectangle::new(Point::ORIGIN, s);
        let accent = crate::theme::ACCENT.get();
        let light = crate::theme::ACCENT_TEXT.get();
        let fill = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(s.width, s.height))
            .add_stop(0.0, accent)
            .add_stop(1.0, light);
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), BADGE_RADIUS.into()),
            canvas::Fill::from(fill),
        );
        f.fill_text(canvas::Text {
            content: mono.clone(),
            position: rect.center(),
            max_width: f32::INFINITY,
            color: ON_ACCENT.get(),
            size: iced::Pixels(BADGE_TEXT),
            line_height: iced::widget::text::LineHeight::Relative(1.0),
            font: MANROPE_EXTRABOLD,
            align_x: iced::widget::text::Alignment::Center,
            align_y: Vertical::Center,
            shaping: iced::widget::text::Shaping::Basic,
        });
    });
    let pill_text = words.open_on(usize::from(shown.desktop) + 1);
    let pill = container(said(
        pill_text.clone(),
        PILL,
        MANROPE_BOLD,
        crate::theme::ACCENT_TEXT.get(),
    ))
    .padding(Padding {
        top: 2.0,
        right: 8.0,
        bottom: 2.0,
        left: 8.0,
    })
    .style(|_| container::Style {
        background: Some(PILL_FILL.get().into()),
        border: iced::Border {
            radius: 8.0.into(),
            ..iced::Border::default()
        },
        ..container::Style::default()
    });
    let when = words.since(&since(shown.since));
    let when_w = kit::measure(&when, WHEN, MANROPE);
    let room = INNER - BADGE - 12.0 - when_w - 12.0;
    let pill_w = kit::measure(&pill_text, PILL, MANROPE_BOLD) + 16.0;
    let name = kit::elide(&shown.name, room - pill_w - 10.0, |t| {
        kit::measure(t, NAME, MANROPE_BOLD)
    });
    let folder = kit::elide_start(&crate::desktops::folder_shown(&shown.folder), room, |t| {
        kit::measure(t, FOLDER, MARTIAN)
    });
    let titles = column![
        row![
            ui::line(
                said(name, NAME, MANROPE_BOLD, TEXT.get()),
                shown.name.clone()
            ),
            gap(10.0),
            ui::line(pill, pill_text),
        ]
        .align_y(Vertical::Center),
        ui::line(said(folder.clone(), FOLDER, MARTIAN, FAINT.get()), folder),
    ]
    .spacing(2.0);
    let head = row![
        badge,
        gap(12.0),
        titles,
        iced::widget::space::horizontal(),
        ui::line(said(when.clone(), WHEN, MANROPE, FAINT.get()), when),
    ]
    .height(BADGE)
    .align_y(Vertical::Center);
    // The desktop's sketch, and its windows.
    let desktops = crate::desktops::snapshot(&shown.output);
    let desktop = usize::from(shown.desktop);
    let sketch = drawn(SKETCH.width, SKETCH.height, move |f, s| {
        sketch_of(
            f,
            Rectangle::new(Point::ORIGIN, s),
            desktops.as_ref(),
            desktop,
        );
    });
    let heading = words.windows_heading(rows.len());
    let mut list = column![ui::line(
        container(spaced_by(
            &heading,
            HEADING,
            MANROPE_BOLD,
            FAINT.get(),
            0.08
        ))
        .padding(Padding {
            left: ROW_PAD,
            bottom: HEADING_BOTTOM,
            ..Padding::ZERO
        })
        .height(HEADING * LINE + HEADING_BOTTOM),
        heading.clone(),
    )]
    .spacing(0.0);
    let list_width = INNER - SKETCH.width - 14.0;
    let mut windows = column![].spacing(ROW_GAP);
    for row in rows.iter().take(MOST) {
        windows = windows.push(window_row(row, words, list_width));
    }
    list = list.push(windows);
    let middle = row![sketch, gap(14.0), list].height(windows_height(rows.len()));
    // That private windows are never read.
    let private = container(
        row![
            drawn(14.0, 14.0, |f, s| lock(f, s, FAINT.get())),
            gap(8.0),
            said(words.private().to_owned(), PRIVATE, MANROPE, FAINT.get()),
        ]
        .align_y(Vertical::Center),
    )
    .width(INNER)
    .padding(Padding {
        top: PRIVATE_PAD.0,
        right: PRIVATE_PAD.1,
        bottom: PRIVATE_PAD.0,
        left: PRIVATE_PAD.1,
    })
    .style(|_| container::Style {
        background: Some(PRIVATE_FILL.get().into()),
        border: iced::Border {
            radius: 10.0.into(),
            ..iced::Border::default()
        },
        ..container::Style::default()
    });
    let all = column![
        head,
        Space::new().height(GAP),
        middle,
        Space::new().height(GAP),
        ui::line(private, words.private()),
    ];
    container(all)
        .padding(Padding {
            top: BORDER + PAD,
            right: BORDER + PAD,
            bottom: 0.0,
            left: BORDER + PAD,
        })
        .into()
}

/// A window's row: its icon, its title, where it is.
fn window_row<'a>(row: &Row, words: Words, width: f32) -> El<'a> {
    let place = words.place(row.place).to_owned();
    let place_w = kit::measure(&place, PLACE, MANROPE);
    let room = width - 2.0 * ROW_PAD - ICON - 10.0 - place_w - 8.0;
    let title = kit::elide(&row.title, room, |t| kit::measure(t, TITLE, MANROPE));
    let found = crate::desktops::icons::of(&row.app);
    let letter = row
        .app
        .rsplit('.')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(&row.app)
        .to_owned();
    let icon = drawn(ICON, ICON, move |f, s| {
        kit::app_tile(
            f,
            Rectangle::new(Point::ORIGIN, s),
            (found.as_ref(), &letter),
            (4.0, 9.0),
            1.0,
        );
    });
    let said_as = format!("{}, {place}", row.title);
    let line = row![
        icon,
        gap(10.0),
        said(title, TITLE, MANROPE, TEXT.get()),
        iced::widget::space::horizontal(),
        said(place, PLACE, MANROPE, FAINT.get()),
    ]
    .align_y(Vertical::Center);
    let _ = row.id;
    ui::control(
        container(line)
            .padding(Padding {
                left: ROW_PAD,
                right: ROW_PAD,
                ..Padding::ZERO
            })
            .width(width)
            .height(ROW)
            .align_y(Vertical::Center),
        format!("project:Window({})", row.id),
        Role::ListItem,
        said_as,
    )
    .into()
}

/// The desktop's sketch, as the bar's tiles draw it, larger: its ground,
/// its windows at their places, each with its icon.
fn sketch_of(
    f: &mut Frame,
    rect: Rectangle,
    desktops: Option<&hocus_link::Desktops>,
    desktop: usize,
) {
    let ground =
        canvas::gradient::Linear::new(rect.position(), Point::new(rect.width, rect.height))
            .add_stop(0.0, SKETCH_TOP.get())
            .add_stop(1.0, SKETCH_BOTTOM.get());
    let path = Path::rounded_rectangle(rect.position(), rect.size(), 6.0.into());
    f.fill(&path, canvas::Fill::from(ground));
    if let Some(desktops) = desktops {
        for window in desktops
            .windows
            .iter()
            .filter(|w| usize::from(w.desktop) == desktop && w.look != hocus_link::Look::Minimized)
        {
            let at = crate::desktops::draw::sketch(rect, window.rect);
            let ink = if window.look == hocus_link::Look::Focused {
                WINDOW_FOCUSED
            } else {
                WINDOW
            };
            f.fill(
                &Path::rounded_rectangle(at.position(), at.size(), 2.0.into()),
                ink.get(),
            );
            if let Some(place) = crate::desktops::draw::icon_in(at)
                && let Some(icon) = crate::desktops::icons::of(&window.app)
            {
                let big = Rectangle::new(
                    Point::new(place.center_x() - 8.0, place.center_y() - 8.0),
                    Size::new(16.0, 16.0),
                );
                kit::app_tile(f, big, (Some(&icon), ""), (4.0, 0.0), 1.0);
            }
        }
    }
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(rect.width - 1.0, rect.height - 1.0),
            5.5.into(),
        ),
        Stroke::default()
            .with_color(SKETCH_RIM.get())
            .with_width(1.0),
    );
}

/// The padlock (the mockup's 24 px drawing), `size` large.
fn lock(f: &mut Frame, size: Size, ink: Color) {
    let k = size.width / 24.0;
    let p = |x: f32, y: f32| Point::new(x * k, y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(ink).with_width(1.8 * k)
    };
    f.stroke(
        &Path::rounded_rectangle(p(5.0, 11.0), Size::new(14.0 * k, 9.0 * k), (2.0 * k).into()),
        stroke,
    );
    f.stroke(
        &Path::new(|b| {
            b.move_to(p(8.0, 11.0));
            b.line_to(p(8.0, 8.0));
            b.arc(canvas::path::Arc {
                center: p(12.0, 8.0),
                radius: 4.0 * k,
                start_angle: iced::Radians(std::f32::consts::PI),
                end_angle: iced::Radians(2.0 * std::f32::consts::PI),
            });
            b.line_to(p(16.0, 11.0));
        }),
        stroke,
    );
}

/// The form's and the card's background: their tint (opaque without
/// glass) and their rim.
#[derive(Debug, Clone, Copy)]
pub struct Tint {
    pub radius: f32,
    pub top: Ink,
    pub bottom: Ink,
    pub rim: Ink,
}

impl<M> canvas::Program<M> for Tint {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut f = Frame::new(renderer, size);
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, self.top.get())
            .add_stop(1.0, self.bottom.get());
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, self.radius.into()),
            canvas::Fill::from(tint),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (self.radius - BORDER / 2.0).into(),
            ),
            Stroke::default()
                .with_color(self.rim.get())
                .with_width(BORDER),
        );
        vec![f.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_grows_with_its_windows_past_the_sketch() {
        let card = |n: usize| hocus_link::ProjectCard {
            output: "eDP-1".into(),
            desktop: 0,
            at: hocus_link::Rect {
                x: 0,
                y: 0,
                width: 160,
                height: 100,
            },
            screen: (1440, 900),
            name: "hocus-panda".into(),
            folder: "/home/alex/github/hocus-panda".into(),
            since: 0,
            windows: (0..n as u64)
                .map(|i| (i, hocus_link::WindowPlace::Floating))
                .collect(),
        };
        // Up to two windows the sketch is the tallest.
        assert_eq!(size(&card(0)), size(&card(2)));
        assert!(size(&card(5)).1 > size(&card(3)).1);
        assert_eq!(size(&card(20)), size(&card(MOST)));
        assert_eq!(size(&card(1)).0, 600);
    }
}
