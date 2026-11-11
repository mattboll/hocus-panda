//! The Énergie page (6.10, variant A, docs/design/settings.md "Énergie"):
//! the battery's card; the dim, the screen off and the sleep in 7.2's
//! order with their timeline; locking; the lid and the power button; the
//! power mode and the automatic power saver; what comes later.
//!
//! The rows of one key each are the window's ([`crate::power::settings`]);
//! the battery, the power mode and whether logind is there come from the
//! system bus, on a thread of its own ([`crate::power_bus`]), as changes.
//! The power mode is power-profiles-daemon's: chosen here, it toasts with
//! "Annuler" and joins `Ctrl+Z` as a file's change does.

use hocus_words::settings::app_power_page as words;
use std::time::Instant;

use iced::{
    Alignment, Background, Color, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    keyboard::key::Named,
    mouse,
    widget::{Space, button, canvas, column, container, row},
};

use super::{
    App, Control, Message, Page, TOAST, ToastAction, UNDONE, Undo,
    style::*,
    view::{El, card_box, divided, label, t},
};
use crate::{
    lang::{Lang, w},
    model::{SectionId, Setting, Source, Value},
    power::{self as model, Step, StepKind},
    power_bus::{self, Battery, Charge, Now},
    store::Slot,
};

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Now),
    /// A power mode chosen.
    Mode(&'static str),
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The power mode: `←`/`→`.
    Mode,
}

/// A power mode changed, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeChoice {
    pub before: String,
    pub after: String,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// What the system bus said last; `None` until it said.
    pub(super) now: Option<Now>,
    bus: Option<power_bus::Bus>,
    /// The kernel's supplies say the machine is on battery.
    pub(super) kernel_on_battery: bool,
}

impl State {
    /// Follows the system bus on a thread of its own; what it says comes
    /// to the window through `to`, as changes only.
    pub fn start(to: &iced::futures::channel::mpsc::UnboundedSender<Message>) -> Self {
        let to = to.clone();
        let bus =
            power_bus::start(move |now| to.unbounded_send(Message::Power(Msg::Heard(now))).is_ok());
        Self {
            now: None,
            bus: Some(bus),
            kernel_on_battery: false,
        }
    }
}

const GREEN: Color = rgb(0x6CC287);
const AMBER: Color = rgb(0xF5C46E);
const NAME: Color = rgb(0xD5DBE5);
const GREY: Color = rgb(0xA9B2C2);

impl App {
    // --- What it knows -----------------------------------------------------------------

    /// The battery: UPower's, else the kernel's level alone; `None`
    /// without one.
    pub(super) fn power_battery(&self) -> Option<Battery> {
        match &self.power.now {
            Some(now) if now.upower => now.battery,
            _ => self.battery.map(|percent| Battery {
                percent,
                charge: if self.on_battery_now() {
                    Charge::Discharging
                } else {
                    Charge::Unknown
                },
                left: None,
                health: None,
            }),
        }
    }

    fn on_battery_now(&self) -> bool {
        match &self.power.now {
            Some(now) if now.upower => now.on_battery,
            _ => self.battery.is_some() && self.power.kernel_on_battery,
        }
    }

    /// Whether the computer has a lid: UPower's word, else a battery
    /// (a laptop).
    fn power_lid(&self) -> bool {
        match &self.power.now {
            Some(now) if now.upower => now.lid,
            _ => self.battery.is_some(),
        }
    }

    /// logind is there (unknown yet: as if it were).
    fn power_logind(&self) -> bool {
        self.power.now.as_ref().is_none_or(|n| !n.bus || n.logind)
    }

    /// power-profiles-daemon's modes; `None` without it.
    fn power_modes(&self) -> Option<&power_bus::Modes> {
        self.power.now.as_ref().and_then(|n| n.modes.as_ref())
    }

    /// The system bus answered and power-profiles-daemon is not on it.
    fn power_no_modes(&self) -> bool {
        self.power
            .now
            .as_ref()
            .is_some_and(|n| n.bus && n.modes.is_none())
    }

    fn power_seconds(&self, id: &str) -> Option<u64> {
        let setting = crate::model::setting(id)?;
        match self.value(Slot::of(setting, self.source)) {
            Some(Value::Seconds(s)) => s,
            _ => None,
        }
    }

    fn power_on(&self, id: &str) -> bool {
        crate::model::setting(id)
            .and_then(|s| self.value(Slot::of(s, self.source)))
            .is_none_or(|v| v == Value::Bool(true))
    }

    /// The rows the page shows, in its order.
    pub(super) fn power_visible(&self) -> Vec<&'static Setting> {
        let battery = self.power_battery().is_some();
        let lid = self.power_lid();
        crate::model::settings_of(SectionId::Power)
            .iter()
            .filter(|s| match s.id {
                model::LID_CLOSED | model::DOCKED => lid,
                model::AUTO_SAVER | model::SAVER_BELOW => battery,
                _ => true,
            })
            .collect()
    }

    /// The keyboard's stops: the segmented control (with a battery), the
    /// rows, the power mode before the automatic saver.
    pub(super) fn power_controls(&self) -> Vec<Control> {
        let mut out = Vec::new();
        if self.power_battery().is_some() {
            out.push(Control::Segmented);
        }
        for setting in self.power_visible() {
            if setting.id == model::AUTO_SAVER {
                out.push(Control::Power(Stop::Mode));
            }
            out.push(Control::Setting(Slot::of(setting, self.source)));
        }
        if !out.contains(&Control::Power(Stop::Mode)) {
            out.push(Control::Power(Stop::Mode));
        }
        out
    }

    // --- What it does ------------------------------------------------------------------

    pub(super) fn power_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Heard(now) => {
                if now.upower {
                    let first = self.power.now.as_ref().is_none_or(|n| !n.upower);
                    // Without a battery, one set of delays: AC's; else
                    // the segmented control starts on the source in use.
                    if now.battery.is_none() {
                        self.source = Source::Ac;
                    } else if first {
                        self.source = if now.on_battery {
                            Source::Battery
                        } else {
                            Source::Ac
                        };
                    }
                }
                self.power.now = Some(now);
                iced::Task::none()
            }
            Msg::Mode(mode) => self.power_choose(mode),
        }
    }

    /// A mode the machine has, chosen: asked of power-profiles-daemon,
    /// toasted with "Annuler".
    fn power_choose(&mut self, mode: &'static str) -> iced::Task<Message> {
        let Some(modes) = self.power_modes() else {
            return iced::Task::none();
        };
        if modes.active == mode || !modes.available.iter().any(|m| m == mode) {
            return iced::Task::none();
        }
        let choice = ModeChoice {
            before: modes.active.clone(),
            after: mode.to_owned(),
        };
        self.power_ask(mode);
        self.undo.push(Undo::Power(choice));
        self.redo.clear();
        let text = self.power_mode_said(mode);
        self.toast(text, ToastAction::Undo, TOAST)
    }

    fn power_ask(&mut self, mode: &str) {
        if let Some(bus) = &self.power.bus {
            bus.ask(power_bus::Ask::Mode(mode.to_owned()));
        }
        // What it shows until power-profiles-daemon says it back.
        if let Some(modes) = self.power.now.as_mut().and_then(|n| n.modes.as_mut()) {
            modes.active = mode.to_owned();
        }
    }

    /// "Mode d'énergie : Économie".
    fn power_mode_said(&self, mode: &str) -> String {
        let name = model::mode_name(mode, self.lang);
        match self.lang {
            Lang::French => format!("Mode d’énergie : {name}"),
            Lang::English => format!("Power mode: {name}"),
        }
    }

    /// `choice` undone (or redone): its other side asked, the row lit.
    pub(super) fn power_undo(&mut self, choice: ModeChoice, redo: bool) -> iced::Task<Message> {
        let mode = if redo {
            choice.after.clone()
        } else {
            choice.before.clone()
        };
        self.power_ask(&mode);
        if redo {
            self.undo.push(Undo::Power(choice));
        } else {
            self.redo.push(Undo::Power(choice));
        }
        self.go(Page::Section(SectionId::Power));
        self.light = Some((model::MODE, Instant::now()));
        let said = self.power_mode_said(&mode);
        let text = match (self.lang, redo) {
            (Lang::French, false) => format!("Annulé : {said}."),
            (Lang::English, false) => format!("Undone: {said}."),
            (Lang::French, true) => format!("Rétabli : {said}."),
            (Lang::English, true) => format!("Redone: {said}."),
        };
        self.toast(text, ToastAction::None, UNDONE)
    }

    pub(super) fn power_key(&mut self, stop: Stop, named: Named) -> iced::Task<Message> {
        let Stop::Mode = stop;
        let Some(modes) = self.power_modes() else {
            return iced::Task::none();
        };
        let have: Vec<&'static str> = model::MODES
            .into_iter()
            .filter(|m| modes.available.iter().any(|a| a == m))
            .collect();
        let at = have.iter().position(|m| *m == modes.active);
        let next = match (named, at) {
            (Named::ArrowLeft, Some(i)) if i > 0 => have[i - 1],
            (Named::ArrowRight, Some(i)) if i + 1 < have.len() => have[i + 1],
            _ => return iced::Task::none(),
        };
        self.power_choose(next)
    }

    // --- The rows' words ---------------------------------------------------------------

    /// A row's line in place of its own, and whether it warns (amber).
    pub(super) fn power_line(&self, id: &str) -> Option<(String, bool)> {
        let lang = self.lang;
        match id {
            model::SCREEN_OFF if !self.power_on(model::LOCK_ON_IDLE) => Some((
                w(words::AFTER_THE_DIMMING_IF_NOTHING_HAPPENS)
                    .get(lang)
                    .to_owned(),
                false,
            )),
            model::LID_CLOSED | model::BUTTON if !self.power_logind() => Some((
                w(words::THE_SYSTEM_DECIDES_LOGIND_UNAVAILABLE)
                    .get(lang)
                    .to_owned(),
                true,
            )),
            model::AUTO_SAVER if self.power_no_modes() => Some((
                w(words::IT_WORKS_THROUGH_THE_MODES_WITHOUT)
                    .get(lang)
                    .to_owned(),
                false,
            )),
            _ => None,
        }
    }

    /// A row that does not apply now: the screen off while the dim is
    /// never; the automatic saver without power-profiles-daemon.
    pub(super) fn power_inert(&self, setting: &Setting) -> bool {
        match setting.id {
            model::SCREEN_OFF => self.power_seconds(model::IDLE).is_none(),
            model::AUTO_SAVER | model::SAVER_BELOW => self.power_no_modes(),
            _ => false,
        }
    }

    /// The timeline's steps for the source shown.
    fn power_steps(&self) -> Vec<Step> {
        model::timeline(
            self.power_seconds(model::IDLE),
            self.power_on(model::LOCK_ON_IDLE),
            self.power_seconds(model::SCREEN_OFF),
            self.power_seconds(model::SUSPEND),
            self.lang,
        )
    }

    // --- The page ----------------------------------------------------------------------

    pub(super) fn power_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let controls = self.controls();
        let visible = self.power_visible();
        let shown = |id: &str| visible.iter().any(|s| s.id == id);
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        let head = |words: &'static str, right: Option<El<'a>>| -> El<'a> {
            let mut head = row![label(words), Space::new().width(Length::Fill)]
                .align_y(Alignment::Center)
                .height(28);
            if let Some(right) = right {
                head = head.push(right);
            }
            head.into()
        };
        let group = |head: El<'a>, rows: Vec<El<'a>>| -> El<'a> {
            let mut body = column![];
            for (i, r) in rows.into_iter().enumerate() {
                body = body.push(divided(i > 0, r));
            }
            column![head, card_box(body.into())].spacing(8).into()
        };
        let mut groups = column![].spacing(18).width(Length::Fill);
        groups = groups.push(card_box(self.battery_card()));
        // ÉCRAN ET VEILLE: the rows and their timeline.
        let segmented = self.power_battery().is_some().then(|| {
            let focused = matches!(self.focus, super::Focus::Control(i) if controls.get(i) == Some(&Control::Segmented));
            self.segmented(focused)
        });
        let timeline: El<'_> = container(
            canvas(Timeline {
                steps: self.power_steps(),
                lang,
            })
            .width(Length::Fill)
            .height(58),
        )
        .padding(Padding {
            top: 14.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        })
        .id("power:timeline".to_owned())
        .into();
        groups = groups.push(group(
            head(w(words::SCREEN_AND_SLEEP).get(lang), segmented),
            vec![
                model_row(model::IDLE),
                model_row(model::SCREEN_OFF),
                model_row(model::SUSPEND),
                timeline,
            ],
        ));
        // VERROUILLAGE.
        let chord = self
            .chords("shortcuts.lock")
            .first()
            .map(|c| crate::keys::shown(c, lang));
        let note = chord.map(|c| -> El<'a> {
            t(
                match lang {
                    Lang::French => format!("{c} verrouille à tout moment"),
                    Lang::English => format!("{c} locks at any time"),
                },
                12.0,
                FAINT.get(),
            )
            .into()
        });
        groups = groups.push(group(
            head(w(words::LOCKING).get(lang), note),
            vec![
                model_row(model::LOCK_ON_IDLE),
                model_row(model::LOCK_BEFORE_SLEEP),
            ],
        ));
        // COUVERCLE ET BOUTON, or BOUTON without a lid.
        let mut lid_rows = Vec::new();
        if shown(model::LID_CLOSED) {
            lid_rows.push(model_row(model::LID_CLOSED));
            lid_rows.push(model_row(model::DOCKED));
        }
        lid_rows.push(model_row(model::BUTTON));
        groups = groups.push(group(
            head(
                if self.power_lid() {
                    w(words::LID_AND_BUTTON).get(lang)
                } else {
                    w(words::BUTTON).get(lang)
                },
                None,
            ),
            lid_rows,
        ));
        // ÉCONOMIE D'ÉNERGIE.
        let mut saving = vec![self.mode_row()];
        if shown(model::AUTO_SAVER) {
            saving.push(model_row(model::AUTO_SAVER));
            saving.push(model_row(model::SAVER_BELOW));
        }
        groups = groups.push(group(head(w(words::POWER_SAVING).get(lang), None), saving));
        // PLUS TARD.
        groups = groups.push(group(
            head(w(words::LATER).get(lang), None),
            vec![self.charge_limit_row()],
        ));
        page = page.push(groups);
        container(page).id("page:power".to_owned()).into()
    }

    /// The battery's card, read only: its level, its line, its health; or
    /// "Sur secteur" without one.
    fn battery_card(&self) -> El<'_> {
        let lang = self.lang;
        let content: El<'_> = match self.power_battery() {
            Some(battery) => {
                let line = model::battery_line(battery.charge, battery.left, lang);
                let mut r = row![
                    canvas(BatteryIcon {
                        percent: battery.percent,
                        bolt: !matches!(battery.charge, Charge::Discharging),
                    })
                    .width(46)
                    .height(26),
                    column![
                        t(format!("{} %", battery.percent), 26.0, TEXT.get()).font(BOLD),
                        t(line, 13.0, DIM.get()),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(14)
                .align_y(Alignment::Center);
                if let Some(health) = battery.health {
                    let said = match lang {
                        Lang::French => {
                            format!("État de la batterie :\n{health} % de sa capacité d’origine")
                        }
                        Lang::English => {
                            format!("Battery health:\n{health} % of its original capacity")
                        }
                    };
                    r = r.push(
                        t(said, 12.5, FAINT.get())
                            .line_height(1.5)
                            .align_x(iced::alignment::Horizontal::Right),
                    );
                }
                r.into()
            }
            None => row![
                container(icon_bolt())
                    .width(46)
                    .height(46)
                    .center_x(46)
                    .center_y(46)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            108, 194, 135, 0.16
                        )))),
                        border: border(12.0, 0.0, Color::TRANSPARENT),
                        ..container::Style::default()
                    }),
                column![
                    t(w(words::ON_AC).get(lang), 22.0, TEXT.get()).font(BOLD),
                    t(
                        w(words::THIS_COMPUTER_HAS_NO_BATTERY_ONE).get(lang),
                        13.0,
                        DIM.get()
                    ),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(14)
            .align_y(Alignment::Center)
            .into(),
        };
        container(content)
            .padding([12, 16])
            .height(Length::Shrink)
            .center_y(Length::Shrink)
            .width(Length::Fill)
            .id(format!("row:{}", model::BATTERY))
            .into()
    }

    /// "Mode d'énergie", its chip, its line and its segmented control.
    fn mode_row(&self) -> El<'_> {
        let lang = self.lang;
        let controls = self.controls();
        let focused = matches!(self.focus, super::Focus::Control(i) if controls.get(i) == Some(&Control::Power(Stop::Mode)));
        let modes = self.power_modes();
        let known = self.power_no_modes();
        let ink = if known { INERT } else { 1.0 };
        let name = row![
            t(w(words::POWER_MODE).get(lang), 14.0, fade(TEXT.get(), ink)).font(SEMIBOLD),
            chip(
                w(words::NEW).get(lang),
                BLUE.get(),
                rgba(143, 179, 255, 0.16)
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut words = column![name].spacing(2).width(Length::Fill);
        match modes {
            None if known => {
                words = words.push(t(
                    w(words::POWER_MODES_NEED_THE_POWER_PROFILES).get(lang),
                    12.5,
                    PEACH.get(),
                ));
            }
            _ => {
                words = words.push(t(
                    w(words::THE_SAME_AS_IN_THE_BATTERY).get(lang),
                    12.5,
                    DIM.get(),
                ));
            }
        }
        if let Some(modes) = modes {
            if !modes.available.iter().any(|m| m == "performance") {
                words = words.push(t(
                    w(words::THIS_COMPUTER_HAS_NO_PERFORMANCE_MODE).get(lang),
                    12.5,
                    FAINT.get(),
                ));
            }
            if let Some(why) = model::degraded(&modes.degraded, lang) {
                words = words.push(t(why, 12.5, PEACH.get()));
            }
        }
        let mut options = row![];
        for mode in model::MODES {
            let active = modes.is_some_and(|m| m.active == mode);
            let there = modes.is_some_and(|m| m.available.iter().any(|a| a == mode));
            let colour = if !there {
                fade(TEXT.get(), 0.28)
            } else if active {
                TEXT.get()
            } else {
                DIM.get()
            };
            let mut b =
                button(t(model::mode_name(mode, lang), 12.5, fade(colour, ink)).font(SEMIBOLD))
                    .height(24)
                    .padding([3, 10])
                    .style(move |_, _| button::Style {
                        background: active.then_some(Background::Color(crate::theme::ink(rgba(
                            255, 255, 255, 0.13,
                        )))),
                        text_color: TEXT.get(),
                        border: border(7.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    });
            if there && !active && self.writable_mode() {
                b = b.on_press(Message::Power(Msg::Mode(mode)));
            }
            options = options.push(container(b).id(format!("mode:{mode}")));
        }
        let control = container(options)
            .padding(2)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.05,
                )))),
                border: if focused {
                    ring(9.0)
                } else {
                    border(9.0, 0.0, Color::TRANSPARENT)
                },
                ..container::Style::default()
            })
            .id("control:power.mode".to_owned());
        let lit = self.power_lit(model::MODE);
        container(
            row![
                words,
                row![Space::new().height(36), control].align_y(Alignment::Center)
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .style(move |_: &Theme| container::Style {
            background: (lit > 0.0).then_some(Background::Color(
                crate::theme::Ink::Accent(0.20 * lit).get(),
            )),
            border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
            ..container::Style::default()
        })
        .id(format!("row:{}", model::MODE))
        .into()
    }

    /// The modes take a choice once known.
    fn writable_mode(&self) -> bool {
        self.power_modes().is_some()
    }

    fn power_lit(&self, id: &str) -> f32 {
        self.lit(id)
    }

    /// "Limiter la charge à 80 %": later, inert.
    fn charge_limit_row(&self) -> El<'_> {
        let lang = self.lang;
        let name = row![
            t(
                w(words::LIMIT_CHARGING_TO_80).get(lang),
                14.0,
                fade(TEXT.get(), INERT)
            )
            .font(SEMIBOLD),
            chip(
                w(words::LATER_2).get(lang),
                AMBER,
                rgba(245, 196, 110, 0.16)
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let words = column![
            name,
            t(
                w(words::SPARES_THE_BATTERY_OF_A_LAPTOP).get(lang),
                12.5,
                fade(DIM.get(), INERT)
            ),
        ]
        .spacing(2)
        .width(Length::Fill);
        let off = container(
            container(Space::new().width(18).height(18)).style(|_: &Theme| container::Style {
                background: Some(Background::Color(fade(Color::WHITE, INERT))),
                border: border(9.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            }),
        )
        .padding(2)
        .width(40)
        .height(22)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(fade(
                crate::theme::Ink::Token(|t| t.switch_off).get(),
                INERT,
            ))),
            border: border(11.0, 0.0, Color::TRANSPARENT),
            ..container::Style::default()
        });
        container(
            row![
                words,
                row![Space::new().height(36), off].align_y(Alignment::Center)
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .id(format!("row:{}", model::CHARGE_LIMIT))
        .into()
    }

    /// What the page shows, for the log.
    pub(super) fn power_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let battery = self.power_battery().map(|b| {
            serde_json::json!({
                "percent": b.percent,
                "line": model::battery_line(b.charge, b.left, lang),
                "health": b.health,
            })
        });
        let steps: Vec<serde_json::Value> = self
            .power_steps()
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "kind": s.kind.name(lang),
                    "time": s.time,
                    "at": (s.at * 100.0).round(),
                })
            })
            .collect();
        let modes = self.power_modes().map(|m| {
            serde_json::json!({
                "active": m.active,
                "available": m.available,
                "degraded": m.degraded,
            })
        });
        serde_json::json!({
            "battery": battery,
            "segmented": self.power_battery().is_some(),
            "lid": self.power_lid(),
            "logind": self.power_logind(),
            "modes": modes,
            "timeline": steps,
            "lines": self.power_visible().iter().filter_map(|s| {
                self.power_line(s.id).map(|(l, warn)| serde_json::json!([s.id, l, warn]))
            }).collect::<Vec<_>>(),
        })
    }
}

/// A small coloured label beside a row's name: "Nouveau", "Plus tard".
fn chip<'a>(words: &'static str, ink: Color, back: Color) -> El<'a> {
    container(t(words, 10.5, ink).font(BOLD))
        .padding([1, 6])
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::ink(back))),
            border: border(5.0, 0.0, Color::TRANSPARENT),
            ..container::Style::default()
        })
        .into()
}

fn icon_bolt<'a>() -> El<'a> {
    canvas(Bolt).width(20).height(24).into()
}

/// A bolt, green.
#[derive(Debug)]
struct Bolt;

impl canvas::Program<Message> for Bolt {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let s = bounds.width / 20.0;
        let bolt = canvas::Path::new(|p| {
            p.move_to(Point::new(12.0 * s, 1.0 * s));
            p.line_to(Point::new(3.0 * s, 14.0 * s));
            p.line_to(Point::new(9.5 * s, 14.0 * s));
            p.line_to(Point::new(7.5 * s, 23.0 * s));
            p.line_to(Point::new(17.0 * s, 9.5 * s));
            p.line_to(Point::new(10.5 * s, 9.5 * s));
            p.close();
        });
        f.fill(&bolt, GREEN);
        vec![f.into_geometry()]
    }
}

/// The battery drawn: 46 × 26, filled to its level in its band's colour;
/// a bolt on AC.
#[derive(Debug)]
struct BatteryIcon {
    percent: u8,
    bolt: bool,
}

impl canvas::Program<Message> for BatteryIcon {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let rim = fade(TEXT.get(), 0.45);
        f.stroke(
            &canvas::Path::rounded_rectangle(
                Point::new(1.0, 1.0),
                Size::new(40.0, 24.0),
                5.0.into(),
            ),
            canvas::Stroke::default().with_color(rim).with_width(1.6),
        );
        f.fill(
            &canvas::Path::rounded_rectangle(
                Point::new(43.0, 9.0),
                Size::new(2.4, 8.0),
                1.2.into(),
            ),
            rim,
        );
        let band = match self.percent {
            0..=10 => PEACH.get(),
            11..=20 => AMBER,
            _ => GREEN,
        };
        let width = 36.0 * f32::from(self.percent.min(100)) / 100.0;
        if width > 0.5 {
            f.fill(
                &canvas::Path::rounded_rectangle(
                    Point::new(3.0, 3.0),
                    Size::new(width, 20.0),
                    3.0.into(),
                ),
                band,
            );
        }
        if self.bolt {
            let bolt = canvas::Path::new(|p| {
                p.move_to(Point::new(23.0, 4.0));
                p.line_to(Point::new(15.0, 14.5));
                p.line_to(Point::new(20.5, 14.5));
                p.line_to(Point::new(18.5, 22.0));
                p.line_to(Point::new(27.0, 11.0));
                p.line_to(Point::new(21.5, 11.0));
                p.close();
            });
            f.fill(&bolt, Color::WHITE);
            f.stroke(
                &bolt,
                canvas::Stroke::default()
                    .with_color(rgba(20, 24, 34, 0.6))
                    .with_width(1.0),
            );
        }
        vec![f.into_geometry()]
    }
}

/// The timeline under the delays: a 2 px track, a 9 px dot a step, its
/// time above and its name under; in order, not to scale.
#[derive(Debug)]
struct Timeline {
    steps: Vec<Step>,
    lang: Lang,
}

fn step_colour(kind: StepKind) -> Color {
    match kind {
        StepKind::Start | StepKind::NoSleep => FAINT.get(),
        StepKind::Dim | StepKind::Lock => BLUE.get(),
        StepKind::Off => GREY,
        StepKind::Sleep => GREEN,
    }
}

impl Timeline {
    /// Each label's left: the first at the start, the last ending at the
    /// end, the others centred on their dots; then each kept clear of the
    /// next (from the end back), and of the one before.
    fn label_places(&self, texts: &[String], size: f32, font: iced::Font, width: f32) -> Vec<f32> {
        const ROOM: f32 = 8.0;
        let widths: Vec<f32> = texts.iter().map(|t| measure(t, size, font)).collect();
        let last = texts.len().saturating_sub(1);
        let mut left: Vec<f32> = self
            .steps
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(i, (step, w))| {
                let x = step.at * width;
                if i == 0 {
                    0.0
                } else if i == last {
                    width - w
                } else {
                    (x - w / 2.0).clamp(0.0, (width - w).max(0.0))
                }
            })
            .collect();
        for i in (0..last).rev() {
            left[i] = left[i].min(left[i + 1] - ROOM - widths[i]);
        }
        for i in 1..=last {
            left[i] = left[i].max(left[i - 1] + widths[i - 1] + ROOM);
        }
        left
    }
}

impl canvas::Program<Message> for Timeline {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let track = 22.0;
        f.fill_rectangle(
            Point::new(0.0, track),
            Size::new(width, 2.0),
            fade(TEXT.get(), 0.10),
        );
        for pair in self.steps.windows(2) {
            let (a, b) = (pair[0].at * width, pair[1].at * width);
            f.fill_rectangle(
                Point::new(a, track),
                Size::new((b - a).max(0.0), 2.0),
                step_colour(pair[1].kind),
            );
        }
        let card = CARD.get();
        for step in &self.steps {
            let x = (step.at * width).clamp(4.5, width - 4.5);
            let colour = step_colour(step.kind);
            f.fill(&canvas::Path::circle(Point::new(x, track + 1.0), 7.5), card);
            f.fill(
                &canvas::Path::circle(Point::new(x, track + 1.0), 4.5),
                colour,
            );
        }
        let times: Vec<String> = self.steps.iter().map(|s| s.time.clone()).collect();
        let names: Vec<String> = self
            .steps
            .iter()
            .map(|s| s.kind.name(self.lang).to_owned())
            .collect();
        for (texts, size, font, y, colour) in [
            (&times, 10.5, MONO, 2.0, FAINT.get()),
            (&names, 11.5, MANROPE, 32.0, NAME),
        ] {
            for (text, x) in texts
                .iter()
                .zip(self.label_places(texts, size, font, width))
            {
                f.fill_text(canvas::Text {
                    content: text.clone(),
                    position: Point::new(x, y),
                    color: colour,
                    size: size.into(),
                    font,
                    ..canvas::Text::default()
                });
            }
        }
        vec![f.into_geometry()]
    }
}
