//! The "Souris et pavé tactile" page (6.12, variant A): the touchpad's
//! group, the mouse's, the gestures', each named by its device on its
//! label line, a dashed line in place of a group whose device is not
//! plugged; then the test area, where scrolling says which device scrolled
//! (Wayland's axis source: a wheel's notches or a finger's pixels) and a
//! press says which button.
//!
//! The gestures' group (6.13) has a finger diagram on each row: a
//! touchpad's outline, its dots moving along a dashed trail in a 1.8 s
//! loop. They move only while the group shows, the window has the focus
//! and motion is not reduced; otherwise they stand at the trail's middle
//! and nothing draws.
//!
//! The devices come from the compositor ([`crate::devices`], off this
//! thread). The settings are the window's as on any page
//! ([`crate::mouse`]); this module only lays them out and keeps the test
//! area's state.

use crate::lang::French as _;
use hocus_words::settings::app_mouse as words;
use std::time::{Duration, Instant};

use hocus_link::input::{DeviceKind, InputDevice};
use iced::{
    Alignment, Background, Color, Length, Theme,
    mouse::ScrollDelta,
    widget::{
        Space, button, column, container, mouse_area, pin, row, scrollable, slider, stack, text,
    },
};

use super::{
    App, Message,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::theme::Ink;
use crate::{
    lang::{Lang, w},
    model::{Icon, SectionId, Setting, Value, settings_of},
    mouse as model,
    store::Slot,
};

/// How long the target's dot takes to light, and stays lit.
pub const LIGHT_IN: Duration = Duration::from_millis(160);
const LIGHT_HELD: Duration = Duration::from_millis(1400);

/// The bipolar slider's width, and its knob's radius.
const SPEED_WIDTH: f32 = 172.0;
const KNOB: f32 = 8.0;

/// What the test area last recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrolled {
    /// A wheel's notches (Wayland's `axis_source` wheel).
    Wheel,
    /// A finger's pixels: the touchpad.
    Finger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Left,
    Right,
    Middle,
    Double,
}

/// The page's own state: the devices, and the test area's.
#[derive(Debug, Clone, Default)]
pub struct Mouse {
    /// `None` while the compositor has not said them (or cannot).
    pub devices: Option<Vec<InputDevice>>,
    hovered: bool,
    scrolled: Option<Scrolled>,
    press: Option<Press>,
    /// The target's dot lit, since then; and the serial of its end.
    lit: Option<(Instant, u64)>,
    /// The window has the keyboard's focus: the diagrams may move.
    pub focused: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Devices(Vec<InputDevice>),
    /// A wheel or a touchpad scrolled somewhere in the window.
    Wheel(ScrollDelta),
    /// The pointer over the test list, or off it.
    Over(bool),
    Press(Press),
    Unlit(u64),
    /// The Appearance page's cursor row (6.2's).
    Cursor,
    /// The window gained or lost the focus.
    Focused(bool),
}

impl Mouse {
    pub fn moving(&self, now: Instant) -> bool {
        self.lit
            .is_some_and(|(since, _)| now.saturating_duration_since(since) < LIGHT_IN)
    }

    fn of(&self, kind: DeviceKind) -> Vec<&InputDevice> {
        self.devices
            .iter()
            .flatten()
            .filter(|d| d.kind == kind)
            .collect()
    }

    /// The devices are known and none of `kind` is plugged.
    fn none(&self, kind: DeviceKind) -> bool {
        self.devices.is_some() && self.of(kind).is_empty()
    }
}

impl App {
    pub(super) fn mouse_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Devices(devices) => {
                tracing::info!(devices = devices.len(), "the mice and touchpads");
                self.mouse.devices = Some(devices);
            }
            Msg::Wheel(delta) => {
                if self.mouse.hovered {
                    self.mouse.scrolled = Some(match delta {
                        ScrollDelta::Lines { .. } => Scrolled::Wheel,
                        ScrollDelta::Pixels { .. } => Scrolled::Finger,
                    });
                }
            }
            Msg::Over(over) => self.mouse.hovered = over,
            Msg::Press(press) => {
                self.mouse.press = Some(press);
                if press == Press::Double {
                    self.serial += 1;
                    let serial = self.serial;
                    self.mouse.lit = Some((Instant::now(), serial));
                    return iced::Task::perform(
                        tokio::time::sleep(LIGHT_IN + LIGHT_HELD),
                        move |()| Message::Mouse(Msg::Unlit(serial)),
                    );
                }
            }
            Msg::Unlit(serial) => {
                if self.mouse.lit.is_some_and(|(_, s)| s == serial) {
                    self.mouse.lit = None;
                }
            }
            Msg::Cursor => self.go(super::Page::Section(SectionId::Appearance)),
            Msg::Focused(focused) => self.mouse.focused = focused,
        }
        iced::Task::none()
    }

    /// The settings of the page shown: a group whose device is not plugged
    /// shows none.
    pub(super) fn mouse_visible(&self) -> Vec<&'static Setting> {
        let no_pad = self.mouse.none(DeviceKind::Touchpad);
        let no_mouse = self.mouse.none(DeviceKind::Pointer);
        settings_of(SectionId::Mouse)
            .iter()
            .filter(|s| match model::kind_of(s.id) {
                Some(DeviceKind::Touchpad) => !no_pad,
                Some(DeviceKind::Pointer) => !no_mouse,
                None => true,
            })
            .collect()
    }

    /// The setting is on this page and no device plugged can take it.
    pub(super) fn not_here(&self, setting: &Setting) -> Option<&'static str> {
        if setting.section != SectionId::Mouse
            || model::takes(setting.id, self.mouse.devices.as_deref())
        {
            return None;
        }
        if setting.group == model::GESTURES {
            return Some(self.say(match self.fingers() {
                Some(2) => w(words::NOT_ON_THIS_TOUCHPAD_IT_SENSES),
                Some(1) => w(words::NOT_ON_THIS_TOUCHPAD_IT_SENSES_2),
                _ => w(words::NOT_ON_THIS_TOUCHPAD_IT_SENSES_3),
            }));
        }
        Some(match model::kind_of(setting.id) {
            Some(DeviceKind::Pointer) => self.say(w(words::NOT_ON_THIS_MOUSE)),
            _ => self.say(w(words::NOT_ON_THIS_TOUCHPAD)),
        })
    }

    /// What applies when the file says nothing, when a device says it.
    pub(super) fn device_default(&self, slot: Slot) -> Option<Value> {
        model::device_default(slot.setting, self.mouse.devices.as_deref())
    }

    /// The touchpad is off: `off-with-mouse` on, and a mouse plugged.
    fn touchpad_off(&self) -> bool {
        let off = model_setting("touchpad.off-with-mouse")
            .and_then(|s| self.value(Slot::of(s, self.source)))
            == Some(Value::Bool(true));
        off && !self.mouse.of(DeviceKind::Pointer).is_empty()
    }

    fn switch_on(&self, id: &str) -> bool {
        model_setting(id).and_then(|s| self.value(Slot::of(s, self.source)))
            == Some(Value::Bool(true))
    }

    // --- The page -----------------------------------------------------------------

    pub(super) fn mouse_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let controls = self.controls();
        let link = button(
            row![
                icon(Icon::Arrow, 14.0, FAINT.get()),
                t(
                    w(words::THE_CURSOR_S_THEME_AND_SIZE).get(lang),
                    13.0,
                    FAINT.get()
                ),
                t(
                    w(words::APPEARANCE_AND_LANGUAGE).get(lang),
                    13.0,
                    MINT.get()
                )
                .font(SEMIBOLD),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding(0)
        .on_press(Message::Mouse(Msg::Cursor))
        .style(|_, _| button::Style::default());
        page = page.push(container(link).id("mouse:cursor-link".to_owned()));
        let visible = self.mouse_visible();
        let rows_of = |group: crate::model::RowGroup| -> Vec<&'static Setting> {
            visible
                .iter()
                .copied()
                .filter(|s| s.group == group)
                .collect()
        };
        // Pavé tactile.
        let pad = self.mouse.of(DeviceKind::Touchpad);
        let pad_head: Option<El<'_>> = if self.touchpad_off() {
            Some(device_tag(
                Icon::Mouse,
                self.say(w(words::OFF_A_MOUSE_IS_PLUGGED_IN)).to_owned(),
                AMBER.get(),
                AMBER.get(),
            ))
        } else if pad.is_empty() {
            self.mouse.devices.as_ref().map(|_| {
                device_tag(
                    Icon::Mouse,
                    self.say(w(words::NONE)).to_owned(),
                    FAINT.get(),
                    FAINT.get(),
                )
            })
        } else {
            self.mouse
                .devices
                .as_deref()
                .and_then(|d| model::named(DeviceKind::Touchpad, d, lang))
                .map(|name| device_tag(Icon::Mouse, name, SOFT.get(), ACCENT.get()))
        };
        page = page.push(
            self.mouse_group(
                "touchpad",
                model::TOUCHPAD.label.get(lang),
                pad_head,
                &rows_of(model::TOUCHPAD),
                &controls,
                self.mouse
                    .none(DeviceKind::Touchpad)
                    .then(|| (self.say(w(words::NO_TOUCHPAD)), None)),
            ),
        );
        // Souris.
        let mice = self.mouse.of(DeviceKind::Pointer);
        let mouse_head: Option<El<'_>> = if mice.is_empty() {
            self.mouse.devices.as_ref().map(|_| {
                device_tag(
                    Icon::Mouse,
                    self.say(w(words::NONE_2)).to_owned(),
                    FAINT.get(),
                    FAINT.get(),
                )
            })
        } else {
            self.mouse
                .devices
                .as_deref()
                .and_then(|d| model::named(DeviceKind::Pointer, d, lang))
                .map(|name| device_tag(Icon::Mouse, name, SOFT.get(), ACCENT.get()))
        };
        page = page.push(self.mouse_group(
            "mouse",
            model::MOUSE.label.get(lang),
            mouse_head,
            &rows_of(model::MOUSE),
            &controls,
            self.mouse.none(DeviceKind::Pointer).then(|| {
                (
                    self.say(w(words::NO_MOUSE_PLUGGED_IN)),
                    Some(self.say(w(words::ITS_SETTINGS_STAY_IN_THE_FILE))),
                )
            }),
        ));
        // Gestes: hidden with no touchpad.
        if !self.mouse.none(DeviceKind::Touchpad) {
            let head: Option<El<'_>> = self.fingers().map(|n| {
                let said = if lang.fr() {
                    format!("Sur ce pavé : jusqu’à {n} doigts")
                } else {
                    format!("On this touchpad: up to {n} fingers")
                };
                t(said, 12.0, FAINT.get()).into()
            });
            let group = self.mouse_group(
                "gestures",
                model::GESTURES.label.get(lang),
                head,
                &rows_of(model::GESTURES),
                &controls,
                None,
            );
            let note = t(
                w(words::THE_GESTURE_ALWAYS_FOLLOWS_YOUR_FINGERS).get(lang),
                12.0,
                FAINT.get(),
            );
            page = page.push(
                column![
                    group,
                    container(note)
                        .padding([0, 4])
                        .id("gestures:note".to_owned())
                ]
                .spacing(8),
            );
        }
        page = page.push(self.try_area(&controls));
        container(page).id("page:mouse".to_owned()).into()
    }

    fn mouse_group<'a>(
        &'a self,
        id: &str,
        name: &'static str,
        head: Option<El<'a>>,
        rows: &[&'static Setting],
        controls: &[super::Control],
        empty: Option<(&'static str, Option<&'static str>)>,
    ) -> El<'a> {
        let mut line = row![label(name), Space::new().width(Length::Fill)]
            .align_y(Alignment::Center)
            .height(28);
        if let Some(head) = head {
            line = line.push(container(head).id(format!("device:{id}")));
        }
        let body: El<'_> = match empty {
            Some((title, more)) => {
                let mut words = column![t(title, 14.0, TEXT.get()).font(SEMIBOLD)].spacing(2);
                if let Some(more) = more {
                    words = words.push(t(more, 12.5, DIM.get()));
                }
                container(
                    row![icon(Icon::Mouse, 20.0, FAINT.get()), words]
                        .spacing(12)
                        .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([9, 16])
                .height(Length::Shrink)
                .center_y(54)
                .style(boxed(
                    crate::theme::ink(rgba(255, 255, 255, 0.02)),
                    12.0,
                    Some((1.0, crate::theme::ink(rgba(255, 255, 255, 0.12)))),
                ))
                .id(format!("empty:{id}"))
                .into()
            }
            None => {
                let mut list = column![];
                for (j, setting) in rows.iter().enumerate() {
                    list = list.push(divided(j > 0, self.row(setting, controls)));
                }
                container(card_box(list.into()))
                    .id(format!("card:{id}"))
                    .into()
            }
        };
        column![line, body].spacing(8).into()
    }

    // --- The test area ------------------------------------------------------------

    /// What the test area recognised, on its label line.
    fn recognised(&self) -> &'static str {
        match self.mouse.scrolled {
            Some(Scrolled::Finger) => self.say(w(words::RECOGNISED_TOUCHPAD)),
            Some(Scrolled::Wheel) => self.say(w(words::RECOGNISED_THE_MOUSE_S_WHEEL)),
            None => self.say(w(words::NOTHING_DONE_HERE_IS_WRITTEN)),
        }
    }

    /// The direction in force for the device that scrolled (the
    /// touchpad's before any).
    fn direction(&self) -> &'static str {
        let finger = match self.mouse.scrolled {
            Some(Scrolled::Finger) => true,
            Some(Scrolled::Wheel) => false,
            None => !self.mouse.of(DeviceKind::Touchpad).is_empty() || self.mouse.devices.is_none(),
        };
        let natural = self.switch_on(if finger {
            "touchpad.natural-scroll"
        } else {
            "pointer.natural-scroll"
        });
        self.say(match (finger, natural) {
            (true, true) => w(words::TWO_FINGERS_NATURAL_CONTENT_FOLLOWS_YOUR),
            (true, false) => w(words::TWO_FINGERS_CLASSIC_CONTENT_GOES_UP),
            (false, true) => w(words::WHEEL_NATURAL_CONTENT_FOLLOWS_THE_WHEEL),
            (false, false) => w(words::WHEEL_CLASSIC_CONTENT_GOES_UP_AS),
        })
    }

    /// The last press, as the target says it.
    fn pressed(&self) -> String {
        let lang = self.lang;
        let swapped = self.switch_on("pointer.left-handed");
        let said = match self.mouse.press {
            None => {
                return w(words::CLICK_HERE_WITH_EACH_BUTTON).get(lang).to_owned();
            }
            Some(Press::Double) => {
                return w(words::DOUBLE_CLICK_THE_DOT_LIGHTS_UP)
                    .get(lang)
                    .to_owned();
            }
            Some(Press::Left) => w(words::LEFT_CLICK),
            Some(Press::Right) => w(words::RIGHT_CLICK),
            Some(Press::Middle) => w(words::MIDDLE_CLICK),
        };
        let said = said.get(lang);
        match (swapped, lang) {
            (false, _) => said.to_owned(),
            (true, Lang::French) => format!("{said} : boutons inversés pour un gaucher"),
            (true, Lang::English) => format!("{said}: buttons swapped for a left-hander"),
        }
    }

    /// How lit the target's dot is (0 to 1).
    fn dot_lit(&self) -> f32 {
        match self.mouse.lit {
            None => 0.0,
            Some(_) if self.reduced_motion => 1.0,
            Some((since, _)) => {
                super::view::ease(self.now.saturating_duration_since(since), LIGHT_IN)
            }
        }
    }

    fn try_area(&self, controls: &[super::Control]) -> El<'_> {
        let lang = self.lang;
        let focused = |part| matches!(self.focus, super::Focus::Control(i) if controls.get(i) == Some(&super::Control::Try(part)));
        let head = row![
            label(w(words::TRY_IT_HERE).get(lang)),
            Space::new().width(Length::Fill),
            container(t(self.recognised(), 12.0, FAINT.get())).id("try:recognised".to_owned()),
        ]
        .align_y(Alignment::Center)
        .height(28);
        let mut lines = column![];
        for n in 1..=16 {
            lines = lines.push(
                container(
                    row![
                        text(format!("{n:02}")).size(11).font(MONO).color(FAINT),
                        t(
                            match lang {
                                Lang::French => format!("Ligne {n} de la liste"),
                                Lang::English => format!("Line {n} of the list"),
                            },
                            13.0,
                            SOFT.get(),
                        ),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .height(30)
                .center_y(30)
                .padding([0, 12]),
            );
        }
        let list_ring = focused(super::TryPart::List);
        let list = container(
            scrollable(lines)
                .id(TRY_LIST)
                .height(144)
                .width(Length::Fill)
                .style(super::view::thin),
        )
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(0, 0, 0, 0.20)))),
            border: if list_ring {
                ring(9.0)
            } else {
                border(9.0, 0.0, Color::TRANSPARENT)
            },
            ..container::Style::default()
        });
        let list = mouse_area(list)
            .on_enter(Message::Mouse(Msg::Over(true)))
            .on_exit(Message::Mouse(Msg::Over(false)));
        let scroll = column![
            t(w(words::SCROLL).get(lang), 12.5, DIM.get()).font(SEMIBOLD),
            container(list).id("try:list".to_owned()),
            container(t(self.direction(), 12.0, FAINT.get())).id("try:direction".to_owned()),
        ]
        .spacing(8)
        .width(Length::FillPortion(1));
        let lit = self.dot_lit();
        let target_ring = focused(super::TryPart::Target);
        let dot =
            container(Space::new().width(42).height(42)).style(move |_: &Theme| container::Style {
                background: Some(Background::Color(mix(
                    crate::theme::ink(rgba(255, 255, 255, 0.10)),
                    ACCENT.get(),
                    lit,
                ))),
                border: border(
                    21.0 + 8.0 * lit,
                    8.0 * lit,
                    crate::theme::Ink::Accent(0.22 * lit).get(),
                ),
                ..container::Style::default()
            });
        let target = container(dot)
            .width(Length::Fill)
            .height(144)
            .center(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.02,
                )))),
                // iced draws no dashed rim: a faint solid one.
                border: if target_ring {
                    ring(9.0)
                } else {
                    border(9.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.14)))
                },
                ..container::Style::default()
            });
        let target = mouse_area(target)
            .on_press(Message::Mouse(Msg::Press(Press::Left)))
            .on_double_click(Message::Mouse(Msg::Press(Press::Double)))
            .on_right_press(Message::Mouse(Msg::Press(Press::Right)))
            .on_middle_press(Message::Mouse(Msg::Press(Press::Middle)));
        let click = column![
            t(w(words::CLICK).get(lang), 12.5, DIM.get()).font(SEMIBOLD),
            container(target).id("try:target".to_owned()),
            container(t(self.pressed(), 12.0, FAINT.get())).id("try:pressed".to_owned()),
        ]
        .spacing(8)
        .width(Length::FillPortion(1));
        let card = card_box(
            container(row![scroll, click].spacing(18))
                .padding([14, 16])
                .into(),
        );
        column![
            head,
            card,
            t(
                w(words::SPEED_AND_ACCELERATION_APPLY_TO_THE).get(lang),
                12.0,
                FAINT.get(),
            ),
        ]
        .spacing(8)
        .into()
    }

    /// The bipolar speed slider: "Lente", the track with its middle tick,
    /// the fill from the middle to the knob, "Rapide"; no number.
    pub(super) fn speed_control(
        &self,
        slot: Slot,
        min: i64,
        max: i64,
        inert: bool,
        focused: bool,
    ) -> El<'_> {
        let alpha = if inert { INERT } else { 1.0 };
        let now = match self.value(slot) {
            Some(Value::Int(n)) => n,
            _ => 0,
        };
        let narrow = |n: i64| i32::try_from(n).unwrap_or_default();
        let share = (now - min) as f32 / (max - min).max(1) as f32;
        let knob = KNOB + (SPEED_WIDTH - 2.0 * KNOB) * share;
        let middle = SPEED_WIDTH / 2.0;
        let (from, to) = if knob < middle {
            (knob, middle)
        } else {
            (middle, knob)
        };
        let track = container(Space::new().width(SPEED_WIDTH).height(6)).style(boxed(
            fade(crate::theme::ink(rgba(255, 255, 255, 0.12)), alpha),
            3.0,
            None,
        ));
        let fill = container(Space::new().width((to - from).max(0.0)).height(6)).style(boxed(
            fade(ACCENT.get(), alpha),
            3.0,
            None,
        ));
        let tick = container(Space::new().width(2).height(14)).style(boxed(
            fade(crate::theme::ink(rgba(255, 255, 255, 0.35)), alpha),
            1.0,
            None,
        ));
        let bar = slider(narrow(min)..=narrow(max), narrow(now), move |n| {
            if inert {
                Message::Nothing
            } else {
                Message::Slide(slot, i64::from(n))
            }
        })
        .step(1)
        .width(SPEED_WIDTH)
        .height(16)
        .style(move |_, _| slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(Color::TRANSPARENT),
                    Background::Color(Color::TRANSPARENT),
                ),
                width: 6.0,
                border: border(3.0, 0.0, Color::TRANSPARENT),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: KNOB },
                background: Background::Color(fade(Color::WHITE, alpha)),
                border_width: if focused { 3.0 } else { 0.0 },
                border_color: ACCENT.get(),
            },
        });
        let drawn = stack![
            pin(track).y(5),
            pin(fill).x(from).y(5),
            pin(tick).x(middle - 1.0).y(1),
            bar,
        ]
        .width(SPEED_WIDTH)
        .height(16);
        let lang = self.lang;
        row![
            t(w(words::SLOW).get(lang), 11.5, fade(FAINT.get(), alpha)),
            drawn,
            t(w(words::FAST).get(lang), 11.5, fade(FAINT.get(), alpha)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    /// What the page shows, for the log.
    pub(super) fn mouse_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let named = |kind| {
            self.mouse
                .devices
                .as_deref()
                .and_then(|d| model::named(kind, d, lang))
        };
        let group = |id: &str, group: crate::model::RowGroup, kind: Option<DeviceKind>| {
            let empty = kind.is_some_and(|k| self.mouse.none(k));
            let rows: Vec<&str> = self
                .mouse_visible()
                .into_iter()
                .filter(|s| s.group == group)
                .map(|s| s.id)
                .collect();
            serde_json::json!({ "id": id, "empty": empty, "rows": rows })
        };
        let mut groups = vec![
            group("touchpad", model::TOUCHPAD, Some(DeviceKind::Touchpad)),
            group("mouse", model::MOUSE, Some(DeviceKind::Pointer)),
        ];
        if !self.mouse.none(DeviceKind::Touchpad) {
            groups.push(group("gestures", model::GESTURES, None));
        }
        let gestures = serde_json::json!({
            "fingers": self.fingers(),
            "moving": self.gestures_moving(),
        });
        serde_json::json!({
            "touchpad": named(DeviceKind::Touchpad),
            "mouse": named(DeviceKind::Pointer),
            "touchpad_off": self.touchpad_off(),
            "groups": groups,
            "recognised": self.recognised(),
            "direction": self.direction(),
            "pressed": self.pressed(),
            "lit": self.mouse.lit.is_some(),
            "gestures": gestures,
        })
    }
}

/// The test list's scrollable.
pub const TRY_LIST: &str = "try-list";

/// The amber of a touchpad turned off.
const AMBER: Ink = Ink::Dark(rgb(0xF5B44A));

fn model_setting(id: &str) -> Option<&'static Setting> {
    crate::model::setting(id)
}

/// A device on its group's label line: its icon, its name, a dot.
fn device_tag<'a>(which: Icon, name: String, ink: Color, dot: Color) -> El<'a> {
    row![
        icon(which, 15.0, ink),
        t(name, 12.5, ink),
        container(Space::new().width(7).height(7)).style(boxed(dot, 3.5, None)),
    ]
    .spacing(7)
    .align_y(Alignment::Center)
    .into()
}

fn mix(from: Color, to: Color, by: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * by,
        g: from.g + (to.g - from.g) * by,
        b: from.b + (to.b - from.b) * by,
        a: from.a + (to.a - from.a) * by,
    }
}

// --- The gestures' diagrams (6.13) ----------------------------------------------

/// The diagrams' loop.
const LOOP: Duration = Duration::from_millis(1800);

/// What a row's diagram shows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    /// Three dots side by side, moving right to left.
    Sideways,
    /// Four dots in a row, moving up.
    Up,
    /// Two dots pinching.
    Pinch,
}

/// A 64 × 44 touchpad with its fingers at `phase` of their loop (`None`:
/// still, at the trail's middle), drawn `alpha` opaque.
struct Diagram {
    gesture: Gesture,
    phase: Option<f32>,
    alpha: f32,
}

impl App {
    /// The most fingers a touchpad plugged senses, when the compositor says.
    fn fingers(&self) -> Option<u32> {
        self.mouse
            .of(DeviceKind::Touchpad)
            .iter()
            .map(|d| d.can.fingers)
            .max()
            .filter(|n| *n > 0)
    }

    /// Whether the diagrams move: the gestures' card shows in the page,
    /// the window has the focus, and motion is not reduced.
    pub(super) fn gestures_moving(&self) -> bool {
        if self.reduced_motion
            || !self.mouse.focused
            || self.page != super::Page::Section(SectionId::Mouse)
            || self.mouse.none(DeviceKind::Touchpad)
        {
            return false;
        }
        let (Some(card), Some(page)) = (self.places.at.get("card:gestures"), self.places.page)
        else {
            return false;
        };
        card.intersects(&page.viewport)
    }

    /// A gesture's row's diagram, `ink` its row's.
    pub(super) fn gesture_diagram(&self, id: &str, ink: f32) -> Option<El<'_>> {
        let gesture = match id {
            model::SWIPE_DESKTOPS => Gesture::Sideways,
            model::SWIPE_LAUNCHER => Gesture::Up,
            model::LEFT_TO_APPS => Gesture::Pinch,
            _ => return None,
        };
        let phase = self.gestures_moving().then(|| {
            let since = self.now.saturating_duration_since(*epoch());
            (since.as_secs_f32() % LOOP.as_secs_f32()) / LOOP.as_secs_f32()
        });
        Some(
            container(
                iced::widget::canvas(Diagram {
                    gesture,
                    phase,
                    alpha: ink,
                })
                .width(64)
                .height(44),
            )
            .id(format!("diagram:{id}"))
            .into(),
        )
    }

    /// "Nouveau" beside the four-finger row's name.
    pub(super) fn mouse_chip(&self, setting: &Setting) -> Option<El<'_>> {
        (setting.id == model::SWIPE_LAUNCHER).then(|| self.new_chip())
    }
}

/// The loops all start from the window's first diagram.
fn epoch() -> &'static Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now)
}

/// CSS's `cubic-bezier(x1, y1, x2, y2)` at `t`.
fn bezier((x1, y1, x2, y2): (f32, f32, f32, f32), t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    // The curve's parameter whose x is `t`, by bisection.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

/// The mockup's swipes' curve, applied between keyframes as CSS does.
const SWIPE_CURVE: (f32, f32, f32, f32) = (0.3, 0.6, 0.3, 1.0);
const EASE_IN_OUT: (f32, f32, f32, f32) = (0.42, 0.0, 0.58, 1.0);

/// A swipe's dots at `phase`: their travel along the trail from `start`
/// through `-start` (at 70 %) to `end`, and their opacity (in by 15 %,
/// out over the last 30 %), the mockup's `side3` and `up4`.
fn swipe_at(phase: f32, start: f32, end: f32) -> (f32, f32) {
    let ease = |t: f32| bezier(SWIPE_CURVE, t);
    let travel = if phase < 0.7 {
        start + (-start - start) * ease(phase / 0.7)
    } else {
        -start + (end + start) * ease((phase - 0.7) / 0.3)
    };
    let opacity = if phase < 0.15 {
        ease(phase / 0.15)
    } else if phase < 0.7 {
        1.0
    } else {
        1.0 - ease((phase - 0.7) / 0.3)
    };
    (travel, opacity)
}

impl<M> iced::widget::canvas::Program<M> for Diagram {
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
            widget::canvas::{Frame, LineCap, LineDash, Path, Stroke},
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let a = self.alpha;
        let pad = Path::rounded_rectangle(Point::new(0.5, 0.5), Size::new(63.0, 43.0), 7.0.into());
        frame.fill(&pad, crate::theme::ink(rgba(255, 255, 255, 0.04 * a)));
        frame.stroke(
            &pad,
            Stroke::default()
                .with_color(crate::theme::ink(rgba(255, 255, 255, 0.22 * a)))
                .with_width(1.0),
        );
        let trail = |frame: &mut Frame, from: Point, to: Point| {
            frame.stroke(
                &Path::line(from, to),
                Stroke {
                    line_cap: LineCap::Round,
                    line_dash: LineDash {
                        segments: &[2.0, 4.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_color(crate::theme::ink(rgba(126, 224, 160, 0.35 * a)))
                        .with_width(2.0)
                },
            );
        };
        let dot = |frame: &mut Frame, x: f32, y: f32, opacity: f32| {
            frame.fill(
                &Path::circle(Point::new(x, y), 4.0),
                crate::theme::ink(rgba(0xEE, 0xF1, 0xF6, opacity * a)),
            );
        };
        match self.gesture {
            Gesture::Sideways => {
                trail(&mut frame, Point::new(48.0, 22.0), Point::new(16.0, 22.0));
                let (dx, o) = self.phase.map_or((0.0, 1.0), |p| swipe_at(p, 11.0, -13.0));
                for x in [23.0, 32.0, 41.0] {
                    dot(&mut frame, x + dx, 22.0, o);
                }
            }
            Gesture::Up => {
                trail(&mut frame, Point::new(32.0, 36.0), Point::new(32.0, 8.0));
                let (dy, o) = self.phase.map_or((0.0, 1.0), |p| swipe_at(p, 9.0, -10.0));
                for x in [18.5, 27.5, 36.5, 45.5] {
                    dot(&mut frame, x, 22.0 + dy, o);
                }
            }
            Gesture::Pinch => {
                // Together at the loop's ends, apart at its middle; still,
                // where the mockup draws them.
                let k = self.phase.map_or(4.0 / 7.0, |p| {
                    if p < 0.5 {
                        bezier(EASE_IN_OUT, p * 2.0)
                    } else {
                        1.0 - bezier(EASE_IN_OUT, (p - 0.5) * 2.0)
                    }
                });
                let (ax, ay) = (4.0 - 7.0 * k, 3.0 - 5.0 * k);
                dot(&mut frame, 26.0 + ax, 18.0 + ay, 1.0);
                dot(&mut frame, 38.0 - ax, 26.0 - ay, 1.0);
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dots_come_in_travel_and_fade_out() {
        let (x, o) = swipe_at(0.0, 11.0, -13.0);
        assert!((x - 11.0).abs() < 1e-3 && o.abs() < 1e-3);
        let (_, o) = swipe_at(0.15, 11.0, -13.0);
        assert!((o - 1.0).abs() < 1e-3);
        let (x, o) = swipe_at(0.7, 11.0, -13.0);
        assert!((x + 11.0).abs() < 1e-3 && (o - 1.0).abs() < 1e-3);
        let (x, o) = swipe_at(1.0, 11.0, -13.0);
        assert!((x + 13.0).abs() < 1e-3 && o.abs() < 1e-3);
        assert!((bezier(EASE_IN_OUT, 0.5) - 0.5).abs() < 1e-3);
    }
}
