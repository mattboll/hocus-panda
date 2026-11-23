//! The menu "POUR <APP>" (#899, docs/design/notifications.md "Per
//! application"): the "⋯" of a bubble, or a right click on it, and the
//! "⋯" of a line of the history, or a right click on it, open the four
//! levels of that notification's application, the current one ticked:
//! "Bulle et son", "Bulle, sans son", "Historique seulement", "Rien". A
//! choice applies at once and is written into the configuration file
//! (`[notifications.apps]`), then the menu closes. Under a rule, the check
//! box « Vues dès que je l'ouvre » (#923): unticked, opening the
//! application folds none of its notifications
//! (`[notifications.seen-on-focus]`); the menu stays open.
//!
//! Beside the bar as the other popovers, right-aligned with the bubbles:
//! under the bubble; from the history, in its place, its top where
//! the line was (as a window's menu from a popover's line, #726). It holds
//! the keyboard: the arrows go through the levels, `Enter` or `Space`
//! chooses, `Esc` and a click elsewhere close it.
//!
//! Built with the shell's widget kit ([`crate::ui`]), its parts pinned
//! where [`layout`] places them.

use hocus_words::shell::notify_levels as txt;
use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::NotifyLevel;
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::Vertical,
    keyboard,
    widget::{Space, button, canvas::Path, container, pin, row, space},
    window,
};

use super::{Event as Seen, Item, Pressed, event, kit};
use crate::{
    clock::time::Lang,
    levels::draw::{ACCENT, FAINT, HOVER, MANROPE, TEXT},
    popover::kit::MANROPE_SEMIBOLD,
    ui::{
        self, Around, control,
        parts::{self, drawn},
        style::{self, said as text_of},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 220;
const BORDER: f32 = 1.0;
const PAD: f32 = 4.0;
const SIDE: f32 = BORDER + PAD;
const INNER: f32 = WIDTH as f32 - 2.0 * SIDE;
/// The header, "POUR SLACK": 11 px bold capitals, 10 px in.
const HEADER: f32 = 26.0;
const HEADER_TEXT: f32 = 11.0;
const ROW_IN: f32 = 10.0;
/// A level: 30 px, its words 13 px, the check 13 px.
const ROW: f32 = 30.0;
/// The rule over the check box (#923): 1 px, 4 px from what is around it,
/// 6 px in; the box 16 px, 1.5 px rim, radius 4, 10 px before its words.
const RULE_AROUND: f32 = 4.0;
const RULE_IN: f32 = 6.0;
const BOX: f32 = 16.0;
const BOX_GAP: f32 = 10.0;
/// The rule's ink.
const RULE_INK: crate::theme::Ink = crate::theme::Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const ROW_RADIUS: f32 = 7.0;
const ROW_TEXT: f32 = 13.0;
const CHECK: f32 = 13.0;
/// The current level's ground.
const CURRENT: crate::theme::Ink = crate::theme::Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));

/// The application the menu is for: its id and its name; from the history,
/// where its line's top was on the history's popover.
#[derive(Debug, Clone, PartialEq, Eq)]
struct For {
    id: String,
    name: String,
}

/// What can be acted on in it: a level, or the check box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Level(NotifyLevel),
    SeenOnOpen,
}

/// The menu's choices, in their order.
const CHOICES: [Choice; 5] = [
    Choice::Level(NotifyLevel::All),
    Choice::Level(NotifyLevel::Silent),
    Choice::Level(NotifyLevel::History),
    Choice::Level(NotifyLevel::None),
    Choice::SeenOnOpen,
];

#[derive(Debug, Default)]
struct Menu {
    shown: Option<For>,
    /// From the history: its line's top on the popover.
    line_top: f32,
    focus: Option<Choice>,
    said: Option<String>,
}

static MENU: Mutex<Option<Menu>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Menu) -> T) -> T {
    let mut menu = MENU.lock().unwrap_or_else(PoisonError::into_inner);
    f(menu.get_or_insert_with(Menu::default))
}

/// The "⋯" of a bubble on `screen` (or a right click on it), the menu's
/// top at `from`'s (the bar's frame), its application `id` named `name`.
pub fn from_bubble(screen: &str, from: Rectangle, id: &str, name: &str) {
    tracing::debug!(app = id, screen, "a bubble's menu of levels");
    with(|m| {
        m.shown = Some(For {
            id: id.to_owned(),
            name: name.to_owned(),
        });
    });
    event(Seen::Open {
        bar: screen.to_owned(),
        item: Item::NotifyLevels,
        rect: from,
    });
}

/// The "⋯" of a line of the history (or a right click on it), its top
/// `top` on the history's popover, its application `id` named `name`: the
/// menu takes the history's place.
pub fn from_history(top: f32, id: &str, name: &str) {
    tracing::debug!(app = id, "a history's line's menu of levels");
    with(|m| {
        m.shown = Some(For {
            id: id.to_owned(),
            name: name.to_owned(),
        });
        m.line_top = top;
    });
    event(Seen::Replace(Item::NotifyLevels));
}

/// Where the line it was opened from is on the history's popover.
pub fn line_top() -> f32 {
    with(|m| m.line_top)
}

/// The menu opens: nothing holds the ring.
pub fn opening() {
    with(|m| {
        m.focus = None;
        m.said = None;
    });
}

/// Where its parts are: the levels' rows, the rule, the check box's row,
/// and its height.
fn layout() -> (Vec<(NotifyLevel, Rectangle)>, Rectangle, Rectangle, f32) {
    let mut y = BORDER + PAD + HEADER;
    let rows = NotifyLevel::ALL
        .into_iter()
        .map(|level| {
            let rect = Rectangle::new(Point::new(SIDE, y), Size::new(INNER, ROW));
            y += ROW;
            (level, rect)
        })
        .collect();
    let rule = Rectangle::new(
        Point::new(SIDE + RULE_IN, y + RULE_AROUND),
        Size::new(INNER - 2.0 * RULE_IN, 1.0),
    );
    y += 2.0 * RULE_AROUND + 1.0;
    let check = Rectangle::new(Point::new(SIDE, y), Size::new(INNER, ROW));
    (rows, rule, check, y + ROW + PAD + BORDER)
}

/// Its height.
pub fn height() -> u32 {
    layout().3.round() as u32
}

/// A level's name.
pub fn name(level: NotifyLevel, lang: Lang) -> &'static str {
    match level {
        NotifyLevel::All => txt::ALL.get(lang),
        NotifyLevel::Silent => txt::SILENT.get(lang),
        NotifyLevel::History => txt::HISTORY.get(lang),
        NotifyLevel::None => txt::NONE.get(lang),
    }
}

/// Its control's id: what tests, and screen readers, know it by.
fn id_of(choice: Choice) -> String {
    match choice {
        Choice::Level(level) => format!("notify_levels:{level:?}"),
        Choice::SeenOnOpen => "notify_levels:SeenOnOpen".to_owned(),
    }
}

/// The menu on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A level pressed, or `Enter` on it.
    Choose(NotifyLevel),
    /// The check box pressed: opening the application folds, or not.
    SeenOnOpen,
    /// The ring comes to it.
    Focus(Choice),
    /// A press anywhere on the menu.
    Pressed,
    /// What it shows changed under the view.
    Stale,
}

/// What it was built from: the application, its level, whether opening
/// it folds.
fn built_from() -> Option<(For, NotifyLevel, bool)> {
    let shown = with(|m| m.shown.clone())?;
    let level = crate::notifications::apps::level_of(&shown.id);
    let folds = crate::notifications::apps::folds_on_focus(&shown.id);
    Some((shown, level, folds))
}

/// The menu on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let built = std::rc::Rc::new(std::cell::RefCell::new(built_from()));
    let (seen, stale) = (built.clone(), built);
    Around::new(panel.id, build(), handle, Msg::Stale)
        .rebuild(move || {
            *seen.borrow_mut() = built_from();
            build()
        })
        .stale_when(move || *stale.borrow() != built_from())
        .keys(arrows)
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// The arrows go through the levels, round.
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let forward = match &pressed.key {
        Key::Named(Named::ArrowDown) => true,
        Key::Named(Named::ArrowUp) => false,
        _ => return None,
    };
    let next = match with(|m| m.focus) {
        Some(focus) => kit::cycle(&CHOICES, focus, forward),
        None => kit::step(&CHOICES, None, !forward),
    };
    next.map(Msg::Focus)
}

/// `content` with its top left corner at `point` in the menu.
fn at<'a>(content: impl Into<El<'a>>, point: Point) -> El<'a> {
    pin(content).x(point.x).y(point.y).into()
}

/// Its content: the header, then the levels.
fn build<'a>() -> El<'a> {
    let lang = Lang::of_messages();
    let Some((shown, current, folds)) = built_from() else {
        return Space::new().into();
    };
    let focus = with(|m| {
        let said_now = format!(
            "app={} for={:?} level={} seen_on_open={folds}",
            shown.id,
            shown.name,
            current.word()
        );
        if m.said.as_ref() != Some(&said_now) {
            tracing::debug!("the menu of levels shows it: {said_now}");
            m.said = Some(said_now);
        }
        m.focus
    });
    let (rows, rule, check_row, height) = layout();
    let mut layers: Vec<El<'a>> = vec![Space::new().width(Length::Fill).height(height).into()];
    let header = txt::for_app(lang, shown.name.to_uppercase());
    layers.push(at(
        container(ui::line(
            parts::spaced(&header, HEADER_TEXT, FAINT.get(), INNER - 2.0 * ROW_IN),
            header.clone(),
        ))
        .height(HEADER)
        .align_y(Vertical::Center),
        Point::new(SIDE + ROW_IN, BORDER + PAD),
    ));
    for (level, rect) in rows {
        let chosen = level == current;
        let focused = focus == Some(Choice::Level(level));
        let check = drawn(CHECK, CHECK, move |f, s| {
            if !chosen {
                return;
            }
            let k = s.width / 13.0;
            let path = Path::new(|p| {
                p.move_to(Point::new(2.0 * k, 7.0 * k));
                p.line_to(Point::new(5.2 * k, 10.0 * k));
                p.line_to(Point::new(11.0 * k, 3.2 * k));
            });
            f.stroke(
                &path,
                iced::widget::canvas::Stroke::default()
                    .with_color(ACCENT.get())
                    .with_width(1.8 * k)
                    .with_line_cap(iced::widget::canvas::LineCap::Round)
                    .with_line_join(iced::widget::canvas::LineJoin::Round),
            );
        });
        let words = name(level, lang);
        let line = row![
            text_of(
                words,
                ROW_TEXT,
                if chosen { MANROPE_SEMIBOLD } else { MANROPE },
                TEXT.get()
            ),
            space::horizontal(),
            check,
        ]
        .align_y(Vertical::Center);
        let pressed = button(container(line).center_y(Length::Fill))
            .padding(Padding {
                left: ROW_IN,
                right: ROW_IN,
                ..Padding::ZERO
            })
            .width(rect.width)
            .height(rect.height)
            .style(move |_, status| {
                if style::lit(status) {
                    style::hovered(HOVER.get(), ROW_RADIUS)
                } else if chosen {
                    style::hovered(CURRENT.get(), ROW_RADIUS)
                } else {
                    style::clear()
                }
            })
            .on_press(Msg::Choose(level));
        layers.push(at(
            control(
                pressed,
                id_of(Choice::Level(level)),
                Role::RadioButton,
                words,
            )
            .node(|n| n.checked(chosen).set(level as usize + 1, 4))
            .focus(focused, Msg::Focus(Choice::Level(level)), ROW_RADIUS)
            .activate(Msg::Choose(level)),
            rect.position(),
        ));
    }
    // The rule, then « Vues dès que je l'ouvre » (#923).
    layers.push(at(
        container(Space::new().width(rule.width).height(rule.height))
            .style(|_| style::boxed(RULE_INK.get(), 0.0)),
        rule.position(),
    ));
    let tick = drawn(BOX, BOX, move |f, s| {
        let rect = Rectangle::new(
            Point::new(0.75, 0.75),
            Size::new(s.width - 1.5, s.height - 1.5),
        );
        if folds {
            f.fill(&kit::rounded(rect, 4.0), ACCENT.get());
            let k = s.width / 16.0;
            let path = Path::new(|p| {
                p.move_to(Point::new(4.5 * k, 8.0 * k));
                p.line_to(Point::new(7.0 * k, 10.5 * k));
                p.line_to(Point::new(11.5 * k, 5.5 * k));
            });
            f.stroke(
                &path,
                iced::widget::canvas::Stroke::default()
                    .with_color(Color::from_rgb8(0x0B, 0x10, 0x20))
                    .with_width(2.0 * k)
                    .with_line_cap(iced::widget::canvas::LineCap::Round)
                    .with_line_join(iced::widget::canvas::LineJoin::Round),
            );
        } else {
            f.stroke(
                &kit::rounded(rect, 4.0),
                iced::widget::canvas::Stroke::default()
                    .with_color(FAINT.get())
                    .with_width(1.5),
            );
        }
    });
    let words = txt::SEEN_ON_OPEN.get(lang);
    let pressed = button(
        container(
            row![
                tick,
                Space::new().width(BOX_GAP),
                text_of(words, ROW_TEXT, MANROPE, TEXT.get())
            ]
            .align_y(Vertical::Center),
        )
        .center_y(Length::Fill),
    )
    .padding(Padding {
        left: ROW_IN,
        right: ROW_IN,
        ..Padding::ZERO
    })
    .width(check_row.width)
    .height(check_row.height)
    .style(move |_, status| {
        if style::lit(status) {
            style::hovered(HOVER.get(), ROW_RADIUS)
        } else {
            style::clear()
        }
    })
    .on_press(Msg::SeenOnOpen);
    layers.push(at(
        control(pressed, id_of(Choice::SeenOnOpen), Role::CheckBox, words)
            .node(|n| n.checked(folds))
            .focus(
                focus == Some(Choice::SeenOnOpen),
                Msg::Focus(Choice::SeenOnOpen),
                ROW_RADIUS,
            )
            .activate(Msg::SeenOnOpen),
        check_row.position(),
    ));
    iced::widget::Stack::with_children(layers).into()
}

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => with(|m| m.focus = None),
        Msg::Focus(choice) => with(|m| m.focus = Some(choice)),
        Msg::Choose(level) => {
            if let Some(shown) = with(|m| m.shown.clone()) {
                crate::notifications::apps::choose(&shown.id, &shown.name, level);
            }
            event(Seen::Chosen);
        }
        // A check box: the menu stays, its box ticked or not.
        Msg::SeenOnOpen => {
            if let Some((shown, _, folds)) = built_from() {
                crate::notifications::apps::seen_on_focus(&shown.id, &shown.name, !folds);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_four_levels_under_its_header_then_its_check_box() {
        let (rows, rule, check, height) = layout();
        let levels: Vec<NotifyLevel> = rows.iter().map(|(l, _)| *l).collect();
        assert_eq!(levels, NotifyLevel::ALL);
        assert_eq!(rows[0].1.y, BORDER + PAD + HEADER);
        assert!(rows.windows(2).all(|w| w[1].1.y == w[0].1.y + ROW));
        let last = rows[3].1;
        assert_eq!(rule.y, last.y + ROW + 4.0);
        assert_eq!(check.y, rule.y + 1.0 + 4.0);
        assert_eq!(height, check.y + ROW + PAD + BORDER);
        assert_eq!(
            txt::SEEN_ON_OPEN.get(Lang::French),
            "Vues dès que je l’ouvre"
        );
        assert_eq!(name(NotifyLevel::Silent, Lang::French), "Bulle, sans son");
        assert_eq!(name(NotifyLevel::History, Lang::English), "History only");
    }
}
