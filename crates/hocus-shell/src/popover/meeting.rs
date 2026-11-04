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

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Pressed, event,
    kit::{self, Input, MANROPE_SEMIBOLD, Pointer, text, text_in},
};
use crate::{
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, ring},
    meeting::{
        Card, Words,
        draw::{JOIN_BLUE, MONO, camera, pin, relative_style},
    },
};

pub const WIDTH: u32 = 280;
const BORDER: f32 = 1.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
const LABEL: f32 = 11.0;
const LABEL_SPACING: f32 = 0.1 * LABEL;
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
const CALENDAR_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const CALENDAR_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const JOIN_HOVER: Color = Color::from_rgb8(0x6E, 0x9B, 0xF2);
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);
const THEN_GAP: f32 = 7.0;
const THEN_TIME: f32 = 11.0;

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Join,
    Calendar,
}

/// Where everything is, for the card now.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    label: f32,
    /// The details' block: its top and height; the title's lines, the
    /// times' line and the place's.
    block: (f32, f32),
    title: Vec<(String, f32)>,
    times: f32,
    place: Option<f32>,
    buttons: Vec<(Target, Rectangle)>,
    /// The divider and "ENSUITE"'s line.
    then: Option<(f32, f32)>,
    height: f32,
}

fn layout(card: &Card) -> Layout {
    let left = BORDER + PAD;
    let room = WIDTH as f32 - 2.0 * left;
    let mut y = left;
    let label = y + LABEL * LINE / 2.0;
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
    let mut title = Vec::new();
    for line in lines {
        title.push((line, y + TITLE * LINE / 2.0));
        y += TITLE * LINE;
    }
    y += DETAILS_GAP;
    let times = y + DETAIL * LINE / 2.0;
    y += DETAIL * LINE;
    let place = where_of(card).map(|_| {
        y += DETAILS_GAP;
        let middle = y + DETAIL * LINE / 2.0;
        y += DETAIL * LINE;
        middle
    });
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
        y += 1.0 + GAP;
        let middle = y + DETAIL * LINE / 2.0;
        y += DETAIL * LINE;
        (rule, middle)
    });
    Layout {
        label,
        block,
        title,
        times,
        place,
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
pub struct State {
    pointer: Pointer<Target>,
    focused: Option<Target>,
    said: Option<String>,
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
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = Self::key(state, &pressed);
                }
                self.say(state);
                None
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
        let Some(card) = crate::meeting::snapshot() else {
            return vec![frame.into_geometry()];
        };
        let f = &mut frame;
        let layout = layout(&card);
        let left = BORDER + PAD;
        let room = WIDTH as f32 - 2.0 * left;
        spaced(f, self.words.heading(card.phase), left, layout.label);
        let [r, g, b] = card.colour;
        let (top, height) = layout.block;
        f.fill(
            &kit::rounded(
                Rectangle::new(Point::new(left, top), Size::new(STRIPE, height)),
                STRIPE / 2.0,
            ),
            Color::from_rgb8(r, g, b),
        );
        let x = left + STRIPE + STRIPE_GAP;
        for (line, middle) in &layout.title {
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    line.clone(),
                    Point::new(x, *middle),
                    TITLE,
                    TEXT,
                    Alignment::Left,
                ),
            ));
        }
        // "14:00 – 14:30" then " · dans 25 min".
        let times = format!("{} – {}", card.start, card.end);
        f.fill_text(text_in(
            MONO,
            text(
                times.clone(),
                Point::new(x, layout.times),
                TIMES,
                DIM,
                Alignment::Left,
            ),
        ));
        let mut at = x + kit::measure(&times, TIMES, MONO);
        f.fill_text(text(
            " · ".to_owned(),
            Point::new(at, layout.times),
            DETAIL,
            DIM,
            Alignment::Left,
        ));
        at += kit::measure(" · ", DETAIL, MANROPE);
        let (colour, font) = relative_style(card.phase.look());
        f.fill_text(text_in(
            font,
            text(
                self.words.relative(card.phase),
                Point::new(at, layout.times),
                DETAIL,
                colour,
                Alignment::Left,
            ),
        ));
        if let (Some(middle), Some((said, call))) = (layout.place, where_of(&card)) {
            let icon = Rectangle::new(Point::new(x, middle - ICON / 2.0), Size::new(ICON, ICON));
            if call {
                camera(f, icon, FAINT);
            } else {
                pin(f, icon, FAINT);
            }
            let tx = x + ICON + ICON_GAP;
            let said = kit::elide(&said, left + room - tx, |t| {
                kit::measure(t, DETAIL, MANROPE)
            });
            f.fill_text(text(
                said,
                Point::new(tx, middle),
                DETAIL,
                FAINT,
                Alignment::Left,
            ));
        }
        for (target, rect) in &layout.buttons {
            let hover = state.pointer.hover == Some(*target);
            match target {
                Target::Join => {
                    let fill = if hover { JOIN_HOVER } else { JOIN_BLUE };
                    f.fill(&kit::rounded(*rect, JOIN_RADIUS), fill);
                    let word = self.words.join();
                    let wide =
                        JOIN_ICON + JOIN_ICON_GAP + kit::measure(word, JOIN_TEXT, MANROPE_BOLD);
                    let x0 = rect.center_x() - wide / 2.0;
                    camera(
                        f,
                        Rectangle::new(
                            Point::new(x0, rect.center_y() - JOIN_ICON / 2.0),
                            Size::new(JOIN_ICON, JOIN_ICON),
                        ),
                        Color::WHITE,
                    );
                    f.fill_text(text_in(
                        MANROPE_BOLD,
                        text(
                            word.to_owned(),
                            Point::new(x0 + JOIN_ICON + JOIN_ICON_GAP, rect.center_y()),
                            JOIN_TEXT,
                            Color::WHITE,
                            Alignment::Left,
                        ),
                    ));
                    if state.focused == Some(*target) {
                        ring(f, *rect, JOIN_RADIUS);
                    }
                }
                Target::Calendar => {
                    let fill = if hover { CALENDAR_HOVER } else { CALENDAR_FILL };
                    f.fill(&kit::rounded(*rect, CALENDAR_RADIUS), fill);
                    f.fill_text(text_in(
                        MANROPE_SEMIBOLD,
                        text(
                            self.words.show_in_calendar().to_owned(),
                            rect.center(),
                            DETAIL,
                            DIM,
                            Alignment::Center,
                        ),
                    ));
                    if state.focused == Some(*target) {
                        ring(f, *rect, CALENDAR_RADIUS);
                    }
                }
            }
        }
        if let (Some((rule, middle)), Some((time, title))) = (layout.then, &card.then) {
            f.fill_rectangle(Point::new(left, rule), Size::new(room, 1.0), RULE);
            let mut at = spaced(f, self.words.then(), left, middle) + THEN_GAP;
            f.fill_text(text_in(
                MONO,
                text(
                    time.clone(),
                    Point::new(at, middle),
                    THEN_TIME,
                    DIM,
                    Alignment::Left,
                ),
            ));
            at += kit::measure(time, THEN_TIME, MONO) + THEN_GAP;
            let title = kit::elide(title, (left + room - at).max(0.0), |t| {
                kit::measure(t, DETAIL, MANROPE)
            });
            f.fill_text(text(
                title,
                Point::new(at, middle),
                DETAIL,
                DIM,
                Alignment::Left,
            ));
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

/// `word` spaced 0.1 em in `text.faint` (11 px, 700) from `x`, its line
/// centred on `middle`: where it ends.
fn spaced(f: &mut Frame, word: &str, x: f32, middle: f32) -> f32 {
    let mut at = x;
    for ch in word.chars() {
        let ch = ch.to_string();
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                ch.clone(),
                Point::new(at, middle),
                LABEL,
                FAINT,
                Alignment::Left,
            ),
        ));
        at += kit::measure(&ch, LABEL, MANROPE_BOLD) + LABEL_SPACING;
    }
    at - LABEL_SPACING
}

impl Panel {
    /// What it shows, for the log, when it changed.
    fn say(&self, state: &mut State) {
        let Some(card) = crate::meeting::snapshot() else {
            return;
        };
        let layout = layout(&card);
        let mut said = vec![
            self.words.heading(card.phase).to_owned(),
            layout
                .title
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>()
                .join(" / "),
            format!(
                "{} – {} · {}",
                card.start,
                card.end,
                self.words.relative(card.phase)
            ),
        ];
        if let Some((place, _)) = where_of(&card) {
            said.push(place);
        }
        for (target, _) in &layout.buttons {
            said.push(match target {
                Target::Join => self.words.join().to_owned(),
                Target::Calendar => self.words.show_in_calendar().to_owned(),
            });
        }
        if let Some((time, title)) = &card.then {
            said.push(format!("{} {time} {title}", self.words.then()));
        }
        let said = said.join(" | ");
        if state.said.as_deref() != Some(said.as_str()) {
            tracing::debug!(%said, "the meeting's popover shows it");
            state.said = Some(said);
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

    fn targets() -> Vec<Target> {
        crate::meeting::snapshot()
            .map(|card| layout(&card).buttons.into_iter().map(|(t, _)| t).collect())
            .unwrap_or_default()
    }

    fn mouse<Message>(
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let buttons = crate::meeting::snapshot()
            .map(|card| layout(&card).buttons)
            .unwrap_or_default();
        let target = at.and_then(|at| {
            buttons
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
