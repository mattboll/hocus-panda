//! A shortcut's corners and swipes on the Shortcuts sub-page (issue
//! #893, the approved mockup `s27-gestures`): its chips beside its keys,
//! blue for a swipe with its fingers drawn, amber for a corner, each with
//! its ✕; the row's "+" opens a picker of three tabs, Touches (the keys
//! recorded, 6.8), Coin (the four corners) and Geste (3 and 4 fingers ×
//! left, right, up, down). An item another action holds is greyed with
//! that action's name, and taking it asks first in the amber box, as keys
//! do; this action's own says "déjà ici"; the bar's corner is not offered;
//! a touchpad that senses three fingers greys the four-finger ones.
//!
//! The summaries of the Mouse and touchpad page and of the Desktops and
//! windows page say what the gestures and the corners do, with a link to
//! the shortcuts.

use hocus_words::settings::app_keyboard_page as words;
use hocus_words::settings::triggers as said;

use iced::{
    Alignment, Background, Color, Length, Theme,
    widget::{Space, button, column, container, row, text},
};

use super::{
    App, Message,
    keyboard_page::{Msg, Picker, Tab, Taking},
    style::*,
    view::{El, t},
};
use crate::triggers::{self, Corner, FINGERS, Toward, Trigger};

/// The swipes' blue and the corners' amber.
const SWIPE_FILL: Color = rgba(143, 179, 255, 0.12);
const SWIPE_RIM: Color = rgba(143, 179, 255, 0.25);
const AMBER: Color = rgb(0xF5B44A);
const CORNER_FILL: Color = rgba(245, 180, 74, 0.10);
const CORNER_RIM: Color = rgba(245, 180, 74, 0.25);
/// A greyed item's opacity.
const GREYED: f32 = 0.55;

/// What an item of the picker is to the action it was opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Free,
    /// Another action holds it: taking it asks first.
    Taken(&'static str),
    /// This action has it already.
    Here,
    /// The bar's own corner.
    Bar,
    /// Four fingers on a touchpad that senses three.
    NotOnThisPad,
}

impl Standing {
    pub fn word(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Taken(_) => "taken",
            Self::Here => "here",
            Self::Bar => "bar",
            Self::NotOnThisPad => "pad",
        }
    }
}

/// A touchpad `size` wide with `fingers` on it, their way dashed: the
/// mockup's diagram.
struct Fingers {
    fingers: u8,
    toward: Toward,
    alpha: f32,
}

impl<M> iced::widget::canvas::Program<M> for Fingers {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::{
            Point, Size,
            widget::canvas::{Frame, LineDash, Path, Stroke},
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let a = self.alpha;
        let pad = Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(w - 1.0, h - 1.0),
            (w / 8.0).into(),
        );
        frame.stroke(
            &pad,
            Stroke::default()
                .with_color(crate::theme::ink(rgba(255, 255, 255, 0.28 * a)))
                .with_width(1.0),
        );
        let gap = w / f32::from(self.fingers + 1);
        for i in 1..=self.fingers {
            frame.fill(
                &Path::circle(Point::new(gap * f32::from(i), h / 2.0), w / 14.0),
                crate::theme::ink(rgba(0xEE, 0xF1, 0xF6, a)),
            );
        }
        let reach = w * 0.3;
        let (ux, uy) = self.toward.unit();
        let (vx, vy) = (ux as f32 * reach, uy as f32 * reach * 0.6);
        let from = Point::new(w / 2.0, h / 2.0);
        frame.stroke(
            &Path::line(from, Point::new(from.x + vx, from.y + vy)),
            Stroke {
                line_dash: LineDash {
                    segments: &[2.0, 2.0],
                    offset: 0,
                },
                ..Stroke::default()
                    .with_color(crate::theme::ink(rgba(0x6C, 0xC2, 0x87, a)))
                    .with_width(1.6)
            },
        );
        vec![frame.into_geometry()]
    }
}

/// The fingers' diagram, `size` wide.
pub fn fingers<'a>(fingers: u8, toward: Toward, size: f32, alpha: f32) -> El<'a> {
    iced::widget::canvas(Fingers {
        fingers,
        toward,
        alpha,
    })
    .width(size)
    .height((size * 0.7).round())
    .into()
}

/// A screen `size` wide with its `corner` filled.
pub fn corner<'a>(corner: Corner, size: f32, ink: Color) -> El<'a> {
    let height = (size * 0.72).round();
    let mark = container(Space::new().width(6).height(5)).style(boxed(ink, 1.0, None));
    let placed = container(mark)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(if corner.left() {
            Alignment::Start
        } else {
            Alignment::End
        })
        .align_y(if corner.top() {
            Alignment::Start
        } else {
            Alignment::End
        });
    container(placed)
        .width(size)
        .height(height)
        .padding(1)
        .style(boxed(Color::TRANSPARENT, 2.0, Some((1.0, fade(ink, 0.6)))))
        .into()
}

/// A trigger's chip: blue with its fingers for a swipe, amber with its
/// screen for a corner; its ✕ sends `remove` (none: a chip that only
/// says).
pub fn chip<'a>(trigger: Trigger, said: String, remove: Option<Message>, id: String) -> El<'a> {
    let (ink, fill, rim, drawn) = match trigger {
        Trigger::Swipe { fingers: n, toward } => (
            BLUE.get(),
            crate::theme::ink(SWIPE_FILL),
            crate::theme::ink(SWIPE_RIM),
            fingers(n, toward, 22.0, 1.0),
        ),
        Trigger::Corner(c) => {
            let amber = crate::theme::ink(AMBER);
            (
                amber,
                crate::theme::ink(CORNER_FILL),
                crate::theme::ink(CORNER_RIM),
                corner(c, 18.0, amber),
            )
        }
    };
    let mut line = row![drawn, text(said).size(12).font(SEMIBOLD).color(ink)]
        .spacing(6)
        .align_y(Alignment::Center);
    if let Some(remove) = remove {
        let x = button(text("✕").size(11).color(fade(ink, 0.65)))
            .padding([0, 2])
            .on_press(remove)
            .style(|_, _| button::Style::default());
        line = line.push(container(x).id(format!("un{id}")));
    }
    container(line)
        .padding(iced::Padding {
            top: 3.0,
            right: 9.0,
            bottom: 3.0,
            left: 6.0,
        })
        .height(26)
        .center_y(26)
        .style(boxed(fill, 13.0, Some((1.0, rim))))
        .id(id)
        .into()
}

impl App {
    /// What each item of the picker opened for `id` is to it.
    pub(super) fn standing(&self, id: &str, trigger: Trigger) -> Standing {
        if let Trigger::Corner(c) = trigger
            && c.bars(self.bar_on_the_left())
        {
            return Standing::Bar;
        }
        if self.triggers_of(id).contains(&trigger) {
            return Standing::Here;
        }
        if let Trigger::Swipe { fingers, .. } = trigger
            && crate::mouse::most_fingers(self.mouse_devices())
                .is_some_and(|most| most < u32::from(fingers))
        {
            return Standing::NotOnThisPad;
        }
        match self.trigger_holder(trigger, id) {
            Some(other) => Standing::Taken(other),
            None => Standing::Free,
        }
    }

    /// The row's picker, under it.
    pub(super) fn picker_box(&self, picker: &Picker) -> El<'_> {
        let lang = self.lang;
        let tab = |which: Tab, said: &'static str| -> El<'_> {
            let on = picker.tab == which;
            container(
                button(
                    container(t(said, 12.5, if on { TEXT.get() } else { DIM.get() }).font(BOLD))
                        .center_x(Length::Fill),
                )
                .width(Length::Fill)
                .padding([6, 6])
                .on_press(Message::Keys(Msg::Tab(which)))
                .style(flat(
                    on.then(|| crate::theme::ink(rgba(255, 255, 255, 0.13))),
                    HOVER.get(),
                    6.0,
                    false,
                )),
            )
            .width(Length::Fill)
            .id(format!("tab:{}", which.word()))
            .into()
        };
        let tabs = container(
            row![
                tab(Tab::Keys, words::TAB_KEYS.get(lang)),
                tab(Tab::Corner, words::TAB_CORNER.get(lang)),
                tab(Tab::Gesture, words::TAB_GESTURE.get(lang)),
            ]
            .spacing(2),
        )
        .padding(2)
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.05)),
            8.0,
            None,
        ));
        let body: El<'_> = match (&picker.taking, picker.tab) {
            (Some(taking), _) => self.give_box(picker.setting, taking),
            (None, Tab::Keys) => {
                container(t(words::PRESS_THE_COMBINATION.get(lang), 13.0, DIM.get()))
                    .center_x(Length::Fill)
                    .padding(14)
                    .style(boxed(
                        Color::TRANSPARENT,
                        10.0,
                        Some((1.0, crate::theme::ink(rgba(255, 255, 255, 0.18)))),
                    ))
                    .into()
            }
            (None, Tab::Corner) => {
                let item = |c: Corner| self.item(picker.setting, Trigger::Corner(c));
                column![
                    row![item(Corner::TopLeft), item(Corner::TopRight)].spacing(6),
                    row![item(Corner::BottomLeft), item(Corner::BottomRight)].spacing(6),
                ]
                .spacing(6)
                .into()
            }
            (None, Tab::Gesture) => {
                let mut grid = column![].spacing(6);
                for n in FINGERS {
                    grid = grid.push(t(words::fingers_row(lang, n), 11.0, FAINT.get()).font(BOLD));
                    let mut line = row![].spacing(6);
                    for toward in Toward::ALL {
                        line = line
                            .push(self.item(picker.setting, Trigger::Swipe { fingers: n, toward }));
                    }
                    grid = grid.push(line);
                }
                grid.into()
            }
        };
        container(
            container(column![tabs, body].spacing(10))
                .width(420)
                .padding(10)
                .style(move |_: &Theme| container::Style {
                    shadow: shadow(),
                    ..boxed(MENU.get(), 12.0, Some((1.0, RIM_12.get())))(&Theme::Dark)
                })
                .id("picker".to_owned()),
        )
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding(iced::Padding {
            top: 0.0,
            right: 14.0,
            bottom: 8.0,
            left: 14.0,
        })
        .into()
    }

    /// One corner or swipe of the picker.
    fn item(&self, id: &'static str, trigger: Trigger) -> El<'_> {
        let lang = self.lang;
        let standing = self.standing(id, trigger);
        let alpha = if standing == Standing::Free {
            1.0
        } else {
            GREYED
        };
        let under = match standing {
            Standing::Free => words::FREE.get(lang),
            Standing::Taken(other) => self.name_of(other),
            Standing::Here => words::ALREADY_HERE.get(lang),
            Standing::Bar => words::THE_BAR.get(lang),
            Standing::NotOnThisPad => words::NOT_ON_THIS_TOUCHPAD.get(lang),
        };
        let (drawn, name): (El<'_>, &str) = match trigger {
            Trigger::Swipe { fingers: n, toward } => {
                (fingers(n, toward, 34.0, alpha), triggers::way(toward, lang))
            }
            Trigger::Corner(c) => (
                corner(c, 30.0, fade(crate::theme::ink(AMBER), alpha)),
                triggers::place(c, lang),
            ),
        };
        let inside = column![
            drawn,
            t(name, 11.5, fade(SOFT.get(), alpha)),
            t(under, 10.5, fade(FAINT.get(), alpha.max(0.8)))
                .wrapping(text::Wrapping::WordOrGlyph)
                .align_x(Alignment::Center),
        ]
        .spacing(3)
        .align_x(Alignment::Center)
        .width(Length::Fill);
        let mut b = button(inside)
            .width(Length::Fill)
            .padding([8, 4])
            .style(move |_, status| button::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255,
                    255,
                    255,
                    if matches!(status, button::Status::Hovered) && alpha == 1.0 {
                        0.09
                    } else {
                        0.04
                    },
                )))),
                text_color: TEXT.get(),
                border: border(9.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.08))),
                ..button::Style::default()
            });
        if matches!(standing, Standing::Free | Standing::Taken(_)) && self.writable() {
            b = b.on_press(Message::Keys(Msg::Take(trigger)));
        }
        container(b)
            .width(Length::Fill)
            .id(format!("take:{}", trigger.written()))
            .into()
    }

    /// The amber box: "3 doigts ↑ sert déjà à « X ». Le donner à « Y » le
    /// lui enlève.", Remplacer and Annuler.
    fn give_box(&self, id: &'static str, taking: &Taking) -> El<'_> {
        let lang = self.lang;
        let said = words::gives_it_away(
            lang,
            words::GivesItAway {
                what: triggers::long(taking.trigger, lang),
                other: self.name_of(taking.from),
                mine: self.name_of(id),
            },
        );
        let amber = crate::theme::ink(AMBER);
        container(
            column![
                container(t(said, 12.5, SOFT.get())).id("give:text".to_owned()),
                row![
                    container(
                        button(t(words::REPLACE.get(lang), 12.0, DEEP.get()).font(BOLD))
                            .padding([5, 10])
                            .on_press(Message::Keys(Msg::GiveIt))
                            .style(move |_, _| button::Style {
                                background: Some(Background::Color(amber)),
                                text_color: DEEP.get(),
                                border: border(7.0, 0.0, Color::TRANSPARENT),
                                ..button::Style::default()
                            }),
                    )
                    .id("give:replace".to_owned()),
                    container(
                        button(t(words::CANCEL.get(lang), 12.0, TEXT.get()).font(BOLD))
                            .padding([5, 10])
                            .on_press(Message::Keys(Msg::KeepIt))
                            .style(flat(
                                Some(crate::theme::ink(rgba(255, 255, 255, 0.09))),
                                HOVER.get(),
                                7.0,
                                false
                            )),
                    )
                    .id("give:cancel".to_owned()),
                ]
                .spacing(6),
            ]
            .spacing(8),
        )
        .padding([9, 11])
        .width(Length::Fill)
        .style(boxed(
            crate::theme::ink(CORNER_FILL),
            9.0,
            Some((1.0, crate::theme::ink(rgba(245, 180, 74, 0.3)))),
        ))
        .id("give".to_owned())
        .into()
    }

    /// The gestures the shortcuts bind, a line a finger count and an
    /// axis: "3 doigts ← → : Bureau de droite, Bureau de gauche", then
    /// "3 doigts ↓ : Tous les bureaux" (#944).
    pub(super) fn gestures_said(&self) -> Vec<(u8, Vec<Toward>, String)> {
        let lang = self.lang;
        let mut out = Vec::new();
        let axes = [[Toward::Left, Toward::Right], [Toward::Up, Toward::Down]];
        for (n, axis) in FINGERS
            .into_iter()
            .flat_map(|n| axes.into_iter().map(move |axis| (n, axis)))
        {
            let mut ways = Vec::new();
            let mut names = Vec::new();
            for toward in axis {
                let swipe = Trigger::Swipe { fingers: n, toward };
                if let Some(owner) = self.trigger_holder(swipe, "") {
                    ways.push(toward);
                    names.push(self.name_of(owner));
                }
            }
            if !ways.is_empty() {
                let said = said::summary(
                    lang,
                    said::Summary {
                        what: triggers::fingers(n, &ways, lang),
                        actions: names.join(", "),
                    },
                );
                out.push((n, ways, said));
            }
        }
        out
    }

    /// The corners the shortcuts bind: "Coin ◤ : Vue d’ensemble".
    pub(super) fn corners_said(&self) -> Vec<(Corner, String)> {
        let lang = self.lang;
        Corner::ALL
            .into_iter()
            .filter_map(|c| {
                let owner = self.trigger_holder(Trigger::Corner(c), "")?;
                Some((
                    c,
                    said::summary(
                        lang,
                        said::Summary {
                            what: triggers::short(Trigger::Corner(c), lang),
                            actions: self.name_of(owner),
                        },
                    ),
                ))
            })
            .collect()
    }

    /// A summary's card: its lines, and the link to the shortcuts.
    pub(super) fn summary_card<'a>(
        &'a self,
        id: &'static str,
        lines: Vec<(El<'a>, String)>,
        none: &'static str,
    ) -> El<'a> {
        let lang = self.lang;
        let mut list = column![].spacing(8);
        if lines.is_empty() {
            list = list.push(t(none, 13.0, DIM.get()));
        }
        for (drawn, said) in lines {
            list = list.push(
                row![drawn, t(said, 13.5, TEXT.get()).font(SEMIBOLD)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        }
        let link = container(
            button(t(said::CHANGE_IN_THE_SHORTCUTS.get(lang), 12.5, MINT.get()).font(SEMIBOLD))
                .padding([1, 2])
                .on_press(Message::Keys(Msg::Sub(true)))
                .style(flat(None, HOVER.get(), 5.0, false)),
        )
        .id(format!("link:{id}"));
        let lit = self.lit(id);
        container(column![list, link].spacing(10))
            .width(Length::Fill)
            .padding([12, 16])
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(format!("row:{id}"))
            .into()
    }

    /// The gestures' summary, for the Mouse and touchpad page.
    pub(super) fn gestures_summary(&self) -> El<'_> {
        let lines = self
            .gestures_said()
            .into_iter()
            .map(|(n, ways, said)| (fingers(n, ways[0], 30.0, 1.0), said))
            .collect();
        self.summary_card(
            crate::mouse::GESTURES_SUMMARY,
            lines,
            said::NO_GESTURE.get(self.lang),
        )
    }

    /// The corners' summary, for the Desktops and windows page.
    pub(super) fn corners_summary(&self) -> El<'_> {
        let amber = crate::theme::ink(AMBER);
        let lines = self
            .corners_said()
            .into_iter()
            .map(|(c, said)| (corner(c, 22.0, amber), said))
            .collect();
        self.summary_card(
            crate::desktops::CORNERS,
            lines,
            said::NO_CORNER.get(self.lang),
        )
    }
}
