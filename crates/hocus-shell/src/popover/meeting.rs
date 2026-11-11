//! The mini form's next meeting popover (Day.2, bar.md §5 and §9): opened
//! by a 150 ms hover or a click on the meeting's mark, its top on the
//! mark's, 280 wide.
//!
//! "PROCHAINE RÉUNION" / "NEXT MEETING" (or "EN COURS" / "NOW"), 11 px,
//! 700, spaced, `text.faint`; the calendar's stripe beside the title (15
//! px, 700, two lines at most), "14:00 – 14:30 · dans 25 min" (12.5 px,
//! `text.dim`, the times in Martian Mono), and the link's address or the
//! place with a 13 px camera or pin (12.5 px, `text.faint`). Then
//! **Rejoindre** (34 px, radius 9, `#5B8DEF`) with a link, **Voir dans le
//! calendrier** (30 px, `rgba(255,255,255,0.06)`), which opens the
//! calendar on today with the meeting chosen (Day.5), a divider,
//! and "ENSUITE" with the next event's time and title on one line. 14 px
//! in, its parts 10 px apart.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{Space, button, column, container, row},
    window,
};

use super::{
    Event as Seen, event,
    kit::{self, MANROPE_SEMIBOLD},
};
use crate::theme::Ink;
use crate::{
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    meeting::{
        Card, Words,
        draw::{JOIN_BLUE, MONO, camera, pin, relative_style},
    },
    ui::{
        Around, control,
        parts::{drawn, gap, spaced_by},
        style::{self, boxed, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 280;
const BORDER: f32 = 1.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
const LABEL: f32 = 11.0;
const LABEL_SPACING: f32 = 0.1;
const STRIPE: f32 = 6.0;
const STRIPE_GAP: f32 = 10.0;
const TITLE: f32 = 15.0;
const TITLE_LINES: usize = 2;
const DETAIL: f32 = 12.5;
const TIMES: f32 = 11.5;
const DETAILS_GAP: f32 = 3.0;
const ICON: f32 = 13.0;
const ICON_GAP: f32 = 5.0;
const JOIN: f32 = 34.0;
const JOIN_RADIUS: f32 = 9.0;
const JOIN_TEXT: f32 = 13.5;
const JOIN_ICON: f32 = 16.0;
const JOIN_ICON_GAP: f32 = 7.0;
const CALENDAR: f32 = 30.0;
const CALENDAR_RADIUS: f32 = 8.0;
const CALENDAR_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const CALENDAR_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const JOIN_HOVER: Ink = Ink::Dark(Color::from_rgb8(0x6E, 0x9B, 0xF2));
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const THEN_GAP: f32 = 7.0;
const THEN_TIME: f32 = 11.0;

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Join,
    Calendar,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("meeting:{self:?}")
    }
}

/// Where everything is, for the card now.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    /// The details' block (the title's lines, the times' line and the
    /// place's): its top and height; the title's lines.
    block: (f32, f32),
    title: Vec<String>,
    buttons: Vec<(Target, Rectangle)>,
    /// "ENSUITE"'s line: the divider's top.
    then: Option<f32>,
    height: f32,
}

fn layout(card: &Card) -> Layout {
    let left = BORDER + PAD;
    let room = WIDTH as f32 - 2.0 * left;
    let mut y = left;
    y += LABEL * LINE + GAP;
    let block_top = y;
    let title_room = room - STRIPE - STRIPE_GAP;
    let mut lines = kit::wrap(&card.title, title_room, |t| {
        kit::measure(t, TITLE, MANROPE_BOLD)
    });
    if lines.len() > TITLE_LINES {
        let rest = lines[TITLE_LINES - 1..].join(" ");
        lines.truncate(TITLE_LINES - 1);
        lines.push(kit::elide(&rest, title_room, |t| {
            kit::measure(t, TITLE, MANROPE_BOLD)
        }));
    }
    y += lines.len() as f32 * TITLE * LINE + DETAILS_GAP + DETAIL * LINE;
    if where_of(card).is_some() {
        y += DETAILS_GAP + DETAIL * LINE;
    }
    let block = (block_top, y - block_top);
    let mut buttons = Vec::new();
    if card.video.is_some() {
        y += GAP;
        buttons.push((
            Target::Join,
            Rectangle::new(Point::new(left, y), Size::new(room, JOIN)),
        ));
        y += JOIN;
    }
    // The calendar (Day.5) on today, the meeting chosen.
    y += GAP;
    buttons.push((
        Target::Calendar,
        Rectangle::new(Point::new(left, y), Size::new(room, CALENDAR)),
    ));
    y += CALENDAR;
    let then = card.then.as_ref().map(|_| {
        y += GAP;
        let rule = y;
        y += 1.0 + GAP + DETAIL * LINE;
        rule
    });
    Layout {
        block,
        title: lines,
        buttons,
        then,
        height: y + PAD + BORDER,
    }
}

/// The link's address (without its scheme), else the place: what the
/// details' last line says, and whether it is a call's.
fn where_of(card: &Card) -> Option<(String, bool)> {
    match (&card.video, &card.place) {
        (Some(video), _) => {
            let bare = video
                .strip_prefix("https://")
                .or_else(|| video.strip_prefix("http://"))
                .unwrap_or(video);
            Some((bare.trim_end_matches('/').to_owned(), true))
        }
        (None, Some(place)) => Some((place.clone(), false)),
        (None, None) => None,
    }
}

/// Its height, for the card now (none: as if it had nothing).
pub fn height() -> u32 {
    crate::meeting::snapshot().map_or(120, |card| layout(&card).height.round() as u32)
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
    said: Option<String>,
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
    /// The card changed under the view.
    Stale,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let words = panel.words;
    Around::new(panel.id, build(words), handle, Msg::Stale)
        .rebuild(move || build(words))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// Its content, as the card is now.
fn build<'a>(words: Words) -> El<'a> {
    let Some(card) = crate::meeting::snapshot() else {
        return Space::new().into();
    };
    let layout = layout(&card);
    ui(|ui| {
        say(words, &card, &layout, ui);
        content(words, &card, &layout, ui.focus)
    })
}

fn content<'a>(words: Words, card: &Card, layout: &Layout, focus: Option<Target>) -> El<'a> {
    let room = WIDTH as f32 - 2.0 * (BORDER + PAD);
    let label = container(spaced_by(
        words.heading(card.phase),
        LABEL,
        MANROPE_BOLD,
        FAINT.get(),
        LABEL_SPACING,
    ))
    .height(LABEL * LINE)
    .align_y(Vertical::Center);
    let mut all = column![label].spacing(GAP);
    // The details: the title's lines, the times, the place; the stripe
    // beside them.
    let mut details = column![];
    for line in &layout.title {
        details = details.push(said(line.clone(), TITLE, MANROPE_BOLD, TEXT.get()));
    }
    let times = format!("{} – {}", card.start, card.end);
    let (colour, font) = relative_style(card.phase.look());
    details = details.push(Space::new().height(DETAILS_GAP)).push(
        row![
            said(times, TIMES, MONO, DIM.get()),
            said(" · ", DETAIL, MANROPE, DIM.get()),
            said(words.relative(card.phase), DETAIL, font, colour),
        ]
        .height(DETAIL * LINE)
        .align_y(Vertical::Center),
    );
    let x = STRIPE + STRIPE_GAP;
    if let Some((place, call)) = where_of(card) {
        let icon = drawn(ICON, ICON, move |f, s| {
            let at = Rectangle::new(Point::ORIGIN, s);
            if call {
                camera(f, at, FAINT.get());
            } else {
                pin(f, at, FAINT.get());
            }
        });
        let place = kit::elide(&place, room - x - ICON - ICON_GAP, |t| {
            kit::measure(t, DETAIL, MANROPE)
        });
        details = details.push(Space::new().height(DETAILS_GAP)).push(
            row![
                icon,
                gap(ICON_GAP),
                said(place, DETAIL, MANROPE, FAINT.get())
            ]
            .height(DETAIL * LINE)
            .align_y(Vertical::Center),
        );
    }
    let [r, g, b] = card.colour;
    let stripe = container(Space::new().width(STRIPE).height(layout.block.1))
        .style(move |_| boxed(Color::from_rgb8(r, g, b), STRIPE / 2.0));
    all = all.push(crate::ui::line(
        row![stripe, gap(STRIPE_GAP), details],
        words.name(card),
    ));
    for (target, _) in &layout.buttons {
        all = all.push(button_of(words, *target, focus));
    }
    if let Some((time, title)) = &card.then {
        all = all.push(
            container(Space::new().width(Length::Fill).height(1.0))
                .style(|_| boxed(RULE.get(), 0.0)),
        );
        let label = spaced_by(
            words.then(),
            LABEL,
            MANROPE_BOLD,
            FAINT.get(),
            LABEL_SPACING,
        );
        let used = kit::measure(words.then(), LABEL, MANROPE_BOLD)
            + LABEL_SPACING * LABEL * (words.then().chars().count() as f32 - 1.0)
            + THEN_GAP
            + kit::measure(time, THEN_TIME, MONO)
            + THEN_GAP;
        let shown = kit::elide(title, (room - used).max(0.0), |t| {
            kit::measure(t, DETAIL, MANROPE)
        });
        let then = if words.fr() { "Ensuite" } else { "Next" };
        all = all.push(crate::ui::line(
            row![
                label,
                gap(THEN_GAP),
                said(time.clone(), THEN_TIME, MONO, DIM.get()),
                gap(THEN_GAP),
                said(shown, DETAIL, MANROPE, DIM.get()),
            ]
            .height(DETAIL * LINE)
            .align_y(Vertical::Center),
            format!("{then} : {time}, {title}"),
        ));
    }
    container(all).padding(BORDER + PAD).into()
}

/// Button `target`: "Rejoindre", blue, with its camera; "Voir dans le
/// calendrier", quiet.
fn button_of<'a>(words: Words, target: Target, focus: Option<Target>) -> El<'a> {
    let (face, height, radius, name): (El<'a>, f32, f32, &str) = match target {
        Target::Join => {
            let camera = drawn(JOIN_ICON, JOIN_ICON, |f, s| {
                camera(f, Rectangle::new(Point::ORIGIN, s), Color::WHITE);
            });
            (
                container(
                    row![
                        camera,
                        gap(JOIN_ICON_GAP),
                        said(words.join(), JOIN_TEXT, MANROPE_BOLD, Color::WHITE)
                    ]
                    .align_y(Vertical::Center),
                )
                .center(Length::Fill)
                .into(),
                JOIN,
                JOIN_RADIUS,
                words.join_call(),
            )
        }
        Target::Calendar => (
            container(said(
                words.show_in_calendar(),
                DETAIL,
                MANROPE_SEMIBOLD,
                DIM.get(),
            ))
            .center(Length::Fill)
            .into(),
            CALENDAR,
            CALENDAR_RADIUS,
            words.show_in_calendar(),
        ),
    };
    let pressed = button(face)
        .padding(0)
        .width(Length::Fill)
        .height(height)
        .style(move |_, status| {
            let lit = style::lit(status);
            let fill = match (target, lit) {
                (Target::Join, false) => JOIN_BLUE,
                (Target::Join, true) => JOIN_HOVER,
                (Target::Calendar, false) => CALENDAR_FILL,
                (Target::Calendar, true) => CALENDAR_HOVER,
            };
            button::Style {
                background: Some(fill.get().into()),
                border: style::round(radius),
                ..button::Style::default()
            }
        })
        .on_press(Msg::Act(target));
    control(pressed, target.id(), Role::Button, name)
        .focus(focus == Some(target), Msg::Focus(target), radius)
        .activate(Msg::Act(target))
        .rows(0)
        .into()
}

/// What it shows, for the log, when it changed.
fn say(words: Words, card: &Card, layout: &Layout, ui: &mut Ui) {
    let mut said = vec![
        words.heading(card.phase).to_owned(),
        layout.title.join(" / "),
        format!(
            "{} – {} · {}",
            card.start,
            card.end,
            words.relative(card.phase)
        ),
    ];
    if let Some((place, _)) = where_of(card) {
        said.push(place);
    }
    for (target, _) in &layout.buttons {
        said.push(match target {
            Target::Join => words.join().to_owned(),
            Target::Calendar => words.show_in_calendar().to_owned(),
        });
    }
    if let Some((time, title)) = &card.then {
        said.push(format!("{} {time} {title}", words.then()));
    }
    let said = said.join(" | ");
    if ui.said.as_deref() != Some(said.as_str()) {
        tracing::debug!(%said, "the meeting's popover shows it");
        ui.said = Some(said);
    }
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
        Target::Join => {
            super::send(crate::meeting::join().into_iter().collect());
            event(Seen::Chosen);
        }
        // The calendar takes this popover's place (it closes it).
        Target::Calendar => {
            let bar = super::with(|c| c.open_on().map(|(bar, _)| bar.to_owned()));
            if let Some(bar) = bar {
                super::send(crate::meeting::open_in_calendar(&bar).into_iter().collect());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::Phase;

    fn card(video: bool, page: bool, then: bool) -> Card {
        Card {
            id: "p".into(),
            title: "Point produit".into(),
            colour: [0x8F, 0xB3, 0xFF],
            start: "14:00".into(),
            end: "14:30".into(),
            phase: Phase::Ahead { minutes: 25 },
            video: video.then(|| "https://meet.example.org/abc-defg".into()),
            page: page.then(|| "https://calendar.example.org/event/1".into()),
            place: Some("Salle Jupiter".into()),
            then: then.then(|| ("15:00".into(), "Entretien".into())),
        }
    }

    #[test]
    fn its_parts_ten_px_apart_the_buttons_as_the_card_has_them() {
        let all = layout(&card(true, true, true));
        let targets: Vec<Target> = all.buttons.iter().map(|(t, _)| *t).collect();
        assert_eq!(targets, [Target::Join, Target::Calendar]);
        let (join, calendar) = (all.buttons[0].1, all.buttons[1].1);
        assert_eq!((join.width, join.height), (250.0, 34.0));
        assert_eq!(calendar.height, 30.0);
        assert_eq!(calendar.y - (join.y + join.height), GAP);
        assert!(all.then.is_some());
        // Without a link or a page: the calendar's button alone (Day.5).
        let bare = layout(&card(false, false, false));
        let targets: Vec<Target> = bare.buttons.iter().map(|(t, _)| *t).collect();
        assert_eq!(targets, [Target::Calendar]);
        assert!(bare.then.is_none());
        assert!(bare.height < all.height);
        assert_eq!(
            where_of(&card(true, false, false)),
            Some(("meet.example.org/abc-defg".to_owned(), true))
        );
        assert_eq!(
            where_of(&card(false, false, false)),
            Some(("Salle Jupiter".to_owned(), false))
        );
    }
}
