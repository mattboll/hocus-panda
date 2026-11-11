//! What the Bluetooth page draws (the approved mockup, variant A): the
//! switch's card, MES APPAREILS, AUTOUR and PLUS TARD; without an adapter,
//! the info box alone.

use hocus_words::settings::app_bluetooth_view as words;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_bluetooth::{Action, Device, Failure, Kind};
use iced::{
    Alignment, Background, Color, Length, Padding, Theme,
    widget::{
        Column, Space, button, column, container, mouse_area, pin, rich_text, row, span, svg, text,
        text_input,
    },
};

use super::{super::Focus, Msg, Stop};
use crate::{
    app::{
        App, Control, Message,
        style::*,
        view::{El, card_box, divided, label, t},
    },
    bluetooth as model,
    lang::Lang,
    theme::Ink,
};

/// The page's colours: a device connected (bar.md §13's green), the
/// "Plus tard" chip's amber, the info box's blue.
const GREEN: Ink = Ink::Dark(rgb(0x6CC287));
const AMBER: Ink = Ink::Dark(rgb(0xF5B44A));
const INFO_TEXT: Ink = Ink::Token(|t| t.text2);

/// The page's glyphs: the mockup's 24 px stroke paths.
const SIGN: &str = "M7 7.5 L17 16.5 L12 20.5 V3.5 L17 7.5 L7 16.5";
const HEADPHONES: &str =
    "M4 15 V12 A8 8 0 0 1 20 12 V15 M4 14 H7.5 V20 H4 Z M20 14 H16.5 V20 H20 Z";
const MOUSE: &str = "M12 3 A6 6 0 0 0 6 9 V15 A6 6 0 0 0 18 15 V9 A6 6 0 0 0 12 3 Z M12 6.5 V10.5";
const PHONE: &str = "M7.5 3 H16.5 V21 H7.5 Z M11 18 H13";
const KEYBOARD: &str =
    "M3 6.5 H21 V17.5 H3 Z M6.5 10 H7.5 M10 10 H11 M13 10 H14 M16.5 10 H17.5 M7.5 14 H16.5";
const COMPUTER: &str = "M5 5 H19 V15 H5 Z M2.5 18.5 H21.5";
const SPEAKER: &str =
    "M7 3.5 H17 V20.5 H7 Z M12 17 A2.8 2.8 0 1 0 12 11.4 A2.8 2.8 0 1 0 12 17 Z M12 7 V7.1";
const PENCIL: &str = "M4 20 L4.5 16.5 L15.5 5.5 L18.5 8.5 L7.5 19.5 Z M13.5 7.5 L16.5 10.5";
const FILE: &str = "M6 3.5 H14 L18.5 8 V20.5 H6 Z M14 3.5 V8 H18.5";
const INFO: &str = "M12 21 A9 9 0 1 0 12 3 A9 9 0 1 0 12 21 Z M12 11 V16.5 M12 7.8 V7.9";
const SPINNER: &str = "M12 3.5 A8.5 8.5 0 1 1 3.5 12";
const DOTS: &str = "M6 12 V12.1 M12 12 V12.1 M18 12 V12.1";
const TICK: &str = "M5 12.5 L10 17.5 L19 7.5";

/// The rename field's and the PIN field's ids.
pub(super) const RENAME: &str = "bt-rename";
pub(super) const PIN: &str = "bt-pin";
/// The forget button's fill.
const FORGET: Ink = Ink::Dark(rgb(0xF2945B));

/// `path` stroked in `color`, `width` wide, drawn once.
fn glyph(path: &'static str, color: Color, width: f32) -> svg::Handle {
    type Drawn = HashMap<(&'static str, [u8; 4], u32), svg::Handle>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let rgba = color.into_rgba8();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry((path, rgba, width.to_bits()))
        .or_insert_with(|| {
            let [r, g, b, a] = rgba;
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                     height=\"24\"><path d=\"{path}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" \
                     stroke-opacity=\"{}\" stroke-width=\"{width}\" stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\"/></svg>",
                    f32::from(a) / 255.0
                )
                .into_bytes(),
            )
        })
        .clone()
}

fn draw<'a>(path: &'static str, size: f32, color: Color, width: f32) -> El<'a> {
    svg(glyph(path, color, width))
        .width(size)
        .height(size)
        .into()
}

/// A device's glyph, by its kind: the Bluetooth sign for any other.
fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Headphones | Kind::Earbuds => HEADPHONES,
        Kind::Speaker => SPEAKER,
        Kind::Mouse => MOUSE,
        Kind::Keyboard => KEYBOARD,
        Kind::Phone | Kind::Tablet => PHONE,
        Kind::Computer => COMPUTER,
        Kind::Gamepad | Kind::Other => SIGN,
    }
}

/// The 32 px tile of a device: green for one connected (or Bluetooth on).
fn tile<'a>(path: &'static str, on: bool, alpha: f32) -> El<'a> {
    let (fill, ink) = if on {
        (GREEN.get().scale_alpha(0.16), GREEN.get())
    } else {
        (crate::theme::ink(rgba(255, 255, 255, 0.06)), DIM.get())
    };
    container(draw(path, 17.0, fade(ink, alpha), 1.8))
        .width(32)
        .height(32)
        .center(32)
        .style(boxed(fade(fill, alpha), 9.0, None))
        .into()
}

/// The spinner, 12 px, at `turn` of a turn: three quarters of a circle,
/// drawn at one of [`STEPS`] angles (iced's rotated SVG does not draw on
/// the CPU).
fn spinner<'a>(turn: f32) -> El<'a> {
    static PATHS: OnceLock<Vec<String>> = OnceLock::new();
    let paths = PATHS.get_or_init(|| {
        (0..STEPS)
            .map(|i| {
                let at = |turns: f32| {
                    let a = turns * std::f32::consts::TAU;
                    (12.0 + 8.5 * a.sin(), 12.0 - 8.5 * a.cos())
                };
                let start = i as f32 / STEPS as f32;
                let (x0, y0) = at(start);
                let (x1, y1) = at(start + 0.75);
                format!("M{x0:.2} {y0:.2} A8.5 8.5 0 1 1 {x1:.2} {y1:.2}")
            })
            .collect()
    });
    let step = ((turn.rem_euclid(1.0) * STEPS as f32) as usize).min(STEPS - 1);
    let path: &'static str = paths.get(step).map_or(SPINNER, String::as_str);
    draw(path, 12.0, DIM.get(), 3.0)
}

/// The spinner's angles.
const STEPS: usize = 12;

/// A 28 px secondary button.
fn small_button<'a>(said: String, press: Option<Message>, alpha: f32, id: String) -> El<'a> {
    let mut button = button(
        text(said)
            .size(12.5)
            .font(SEMIBOLD)
            .color(fade(TEXT.get(), alpha)),
    )
    .height(28)
    .padding([5, 11])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(fade(
                crate::theme::ink(rgba(255, 255, 255, if hovered { 0.10 } else { 0.06 })),
                alpha,
            ))),
            text_color: TEXT.get(),
            border: border(7.0, 1.0, fade(RIM_12.get(), alpha)),
            ..button::Style::default()
        }
    });
    if let Some(press) = press {
        button = button.on_press(press);
    }
    container(button).id(id).into()
}

/// A paired device's 30 px "⋯" button, lit while its menu is open.
fn more_button<'a>(open: bool, press: Message, id: String) -> El<'a> {
    container(
        button(
            container(draw(
                DOTS,
                16.0,
                if open { TEXT.get() } else { DIM.get() },
                3.0,
            ))
            .center(28),
        )
        .width(30)
        .height(30)
        .padding(0)
        .on_press(press)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255,
                    255,
                    255,
                    if open || hovered { 0.10 } else { 0.04 },
                )))),
                text_color: TEXT.get(),
                border: border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.10))),
                ..button::Style::default()
            }
        }),
    )
    .id(id)
    .into()
}

/// 6.6's input: 34 px, its rim green while it holds the keyboard.
fn input_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.06))),
        border: border(
            9.0,
            1.0,
            if focused {
                Ink::Accent(0.65).get()
            } else {
                crate::theme::ink(rgba(255, 255, 255, 0.12))
            },
        ),
        icon: DIM.get(),
        placeholder: FAINT.get(),
        value: TEXT.get(),
        selection: Ink::Accent(0.35).get(),
    }
}

/// A device's name as a 240 px field, while it is renamed.
fn rename_field<'a>(typed: &str) -> El<'a> {
    container(
        text_input("", typed)
            .id(RENAME)
            .on_input(|t| Message::Bluetooth(Msg::RenameText(t)))
            .on_submit(Message::Bluetooth(Msg::RenameSave))
            .size(14.0)
            .font(SEMIBOLD)
            .padding([6, 10])
            .width(240)
            .style(input_style),
    )
    .id("bt:rename".to_owned())
    .into()
}

/// A sheet's button: plain, green (accepts) or orange (forgets).
fn sheet_button<'a>(said: &'static str, fill: Option<Color>, press: Message, id: &str) -> El<'a> {
    let ink = if fill.is_some() {
        DEEP.get()
    } else {
        TEXT.get()
    };
    container(
        button(
            text(said)
                .size(13)
                .font(if fill.is_some() { BOLD } else { SEMIBOLD })
                .color(ink),
        )
        .height(32)
        .padding([7, 14])
        .on_press(press)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let base = fill.unwrap_or_else(|| crate::theme::ink(rgba(255, 255, 255, 0.06)));
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color {
                        a: (base.a + 0.04).min(1.0),
                        ..base
                    }
                } else {
                    base
                })),
                text_color: ink,
                border: border(8.0, 1.0, fill.unwrap_or_else(|| RIM_12.get())),
                ..button::Style::default()
            }
        }),
    )
    .id(id.to_owned())
    .into()
}

/// "482 913".
fn grouped(code: &str) -> String {
    if code.len() == 6 && code.is_ascii() {
        format!("{} {}", &code[..3], &code[3..])
    } else {
        code.to_owned()
    }
}

/// A group's label, and what it says on its right.
fn group_head<'a>(words: &'static str, right: Option<El<'a>>) -> El<'a> {
    let mut head = row![label(words), Space::new().width(Length::Fill)]
        .align_y(Alignment::Center)
        .height(28);
    if let Some(right) = right {
        head = head.push(right);
    }
    head.into()
}

/// The note under a group.
fn note<'a>(words: &'static str) -> El<'a> {
    container(t(words, 12.5, FAINT.get()).line_height(1.5))
        .padding([0, 4])
        .into()
}

/// A row lit after a search, as 6.1's.
fn lit_style(lit: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: (lit > 0.0).then_some(Background::Color(Ink::Accent(0.20 * lit).get())),
        border: border(11.0, 1.5, Ink::Accent(0.85 * lit).get()),
        ..container::Style::default()
    }
}

/// A row of the page: its tile, its name and its lines, its controls on
/// the right; `compact` for a device around (46 px at least, else 54).
struct Line<'a> {
    tile: El<'a>,
    name: String,
    /// Its name as a field, while it is renamed.
    field: Option<El<'a>>,
    lines: Vec<El<'a>>,
    controls: Option<El<'a>>,
    compact: bool,
    alpha: f32,
    focused: bool,
}

impl<'a> Line<'a> {
    fn draw(self) -> El<'a> {
        let name: El<'a> = match self.field {
            Some(field) => field,
            None => text(self.name)
                .size(14)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), self.alpha))
                .into(),
        };
        let mut words = column![name].spacing(2).width(Length::Fill);
        for line in self.lines {
            words = words.push(line);
        }
        let least = if self.compact { 34 } else { 36 };
        let mut inside = row![self.tile, words]
            .spacing(16)
            .align_y(Alignment::Center);
        if let Some(controls) = self.controls {
            inside = inside.push(
                row![Space::new().height(least), controls]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        } else {
            inside = inside.push(Space::new().height(least));
        }
        let vertical = if self.compact { 6.0 } else { 9.0 };
        let focused = self.focused;
        container(inside)
            .padding(Padding {
                top: vertical,
                right: 14.0,
                bottom: vertical,
                left: 16.0,
            })
            .style(move |_: &Theme| container::Style {
                border: if focused {
                    ring(11.0)
                } else {
                    border(11.0, 0.0, Color::TRANSPARENT)
                },
                ..container::Style::default()
            })
            .into()
    }
}

impl App {
    /// The page, under its title.
    pub(in crate::app) fn bluetooth_page<'a>(&'a self, page: Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let id = format!("page:{}", crate::model::SectionId::Bluetooth.section().slug);
        let Some(bt) = self.bluetooth.bt.as_ref() else {
            return container(page).id(id).into();
        };
        if !bt.adapter {
            let info = container(
                row![
                    draw(INFO, 18.0, BLUE.get(), 2.0),
                    rich_text::<'_, (), _, _, _>([
                        span(words::NO_BLUETOOTH_ADAPTER.get(lang),)
                            .font(BOLD)
                            .color(INFO_TEXT.get()),
                        span(words::THIS_COMPUTER_HAS_NONE_OR_THE.get(lang),)
                            .color(INFO_TEXT.get()),
                    ])
                    .size(12.5)
                    .line_height(1.5)
                    .font(MANROPE)
                    .width(Length::Fill),
                ]
                .spacing(10),
            )
            .padding([10, 14])
            .style(boxed(
                crate::theme::ink(rgba(143, 179, 255, 0.08)),
                10.0,
                Some((1.0, crate::theme::ink(rgba(143, 179, 255, 0.25)))),
            ))
            .id("bt:no-adapter".to_owned());
            return container(page.push(info)).id(id).into();
        }
        let page = page
            .push(self.bluetooth_switch_card())
            .push(self.bluetooth_mine());
        let page = if bt.powered {
            page.push(self.bluetooth_nearby())
        } else {
            page
        };
        container(page.push(self.bluetooth_later())).id(id).into()
    }

    fn bluetooth_focused(&self, stop: Stop) -> bool {
        let controls = self.controls();
        matches!(self.focus, Focus::Control(i) if controls.get(i) == Some(&Control::Bluetooth(stop)))
    }

    /// The switch's card: on, off, or blocked.
    fn bluetooth_switch_card(&self) -> El<'_> {
        let lang = self.lang;
        let Some(bt) = self.bluetooth.bt.as_ref() else {
            return Space::new().into();
        };
        let (line, colour) = if bt.powered {
            let line = words::visible_as_while_this_page_is(lang, &bt.alias);
            (line, DIM.get())
        } else if bt.blocked {
            (
                words::BLOCKED_BY_THE_WIRELESS_SWITCH_OR
                    .get(lang)
                    .to_owned(),
                PEACH.get(),
            )
        } else {
            (
                words::OFF_YOUR_DEVICES_STAY_PAIRED_THEY
                    .get(lang)
                    .to_owned(),
                DIM.get(),
            )
        };
        let moved = self.knobs.get(model::SWITCH).map_or(1.0, |since| {
            super::super::view::ease_out(
                self.now.saturating_duration_since(*since),
                super::super::KNOB,
            )
        });
        let switch = container(super::super::screens::switch(
            bt.powered,
            false,
            self.bluetooth_focused(Stop::Switch),
            moved,
            Some(Message::Bluetooth(Msg::Power)),
        ))
        .id("bt:switch".to_owned());
        let row = Line {
            tile: tile(SIGN, bt.powered, 1.0),
            name: "Bluetooth".to_owned(),
            field: None,
            lines: vec![t(line, 12.5, colour).line_height(1.4).into()],
            controls: Some(switch.into()),
            compact: false,
            alpha: 1.0,
            focused: false,
        }
        .draw();
        container(card_box(row))
            .style(lit_style(
                self.lit(model::SWITCH).max(self.lit("bluetooth.visible")),
            ))
            .id(format!("row:{}", model::SWITCH))
            .into()
    }

    /// MES APPAREILS: the paired devices, those connected first; at 45 %
    /// and inert while Bluetooth is off.
    fn bluetooth_mine(&self) -> El<'_> {
        let lang = self.lang;
        let Some(bt) = self.bluetooth.bt.as_ref() else {
            return Space::new().into();
        };
        let mine = self.bluetooth.mine();
        let alpha = if bt.powered { 1.0 } else { INERT };
        let running = self.bluetooth.running();
        let focused = self.bluetooth_focused(Stop::Mine);
        let turn = self.bluetooth_turn();
        let mut rows = column![];
        if mine.is_empty() {
            rows = rows.push(
                container(t(
                    words::NO_PAIRED_DEVICE.get(lang),
                    13.5,
                    fade(DIM.get(), alpha),
                ))
                .padding([17, 16]),
            );
        }
        for (i, device) in mine.iter().enumerate() {
            let asked = running.filter(|(p, _)| *p == device.path).map(|(_, a)| a);
            let mut lines = vec![self.bluetooth_mine_line(device, asked, alpha, turn)];
            if let Some(failure) = self.bluetooth.failure(&device.path) {
                lines.push(failure_line(failure, lang));
            }
            let act = if device.connected {
                (words::DISCONNECT, Msg::Disconnect(device.path.clone()))
            } else {
                (words::CONNECT, Msg::Connect(device.path.clone()))
            };
            let enabled = bt.powered && running.is_none();
            let mut controls = row![].spacing(10).align_y(Alignment::Center);
            if asked.is_none() {
                controls = controls.push(small_button(
                    act.0.get(lang).to_owned(),
                    enabled.then_some(Message::Bluetooth(act.1)),
                    if enabled || !bt.powered { alpha } else { INERT },
                    format!("bt:act:{}", device.name),
                ));
            }
            // Its menu, while Bluetooth is on.
            if bt.powered {
                let open = self
                    .bluetooth
                    .menu
                    .as_ref()
                    .is_some_and(|(p, _)| *p == device.path);
                controls = controls.push(more_button(
                    open,
                    Message::Bluetooth(Msg::Menu(device.path.clone())),
                    format!("bt:more:{}", device.name),
                ));
            }
            let field = self
                .bluetooth
                .renaming
                .as_ref()
                .filter(|(p, ..)| *p == device.path)
                .map(|(_, typed, _)| rename_field(typed));
            let line = Line {
                tile: tile(kind_glyph(device.kind), device.connected, alpha),
                name: device.name.clone(),
                field,
                lines,
                controls: Some(controls.into()),
                compact: false,
                alpha,
                focused: focused && self.bluetooth.cursor.0 == i,
            }
            .draw();
            rows = rows.push(divided(
                i > 0,
                container(line)
                    .id(format!("bt:mine:{}", device.name))
                    .into(),
            ));
        }
        let count = (!mine.is_empty()).then(|| {
            t(
                model::paired_count(mine.len(), lang),
                12.0,
                fade(FAINT.get(), alpha),
            )
            .into()
        });
        let lit = [
            model::MY_DEVICES,
            "bluetooth.forget",
            "bluetooth.rename",
            "bluetooth.trust",
        ]
        .into_iter()
        .map(|id| self.lit(id))
        .fold(0.0, f32::max);
        // Each search id the card answers to, for the found row's scroll.
        let card = ["bluetooth.forget", "bluetooth.rename", "bluetooth.trust"]
            .into_iter()
            .fold(
                container(card_box(rows.into()))
                    .style(lit_style(lit))
                    .id(format!("row:{}", model::MY_DEVICES)),
                |inner, id| container(inner).id(format!("row:{id}")),
            );
        container(column![group_head(words::MY_DEVICES.get(lang), count), card].spacing(8))
            .id("group:bluetooth.mine".to_owned())
            .into()
    }

    /// A paired device's line: connected (and its battery), not
    /// connected, or connecting.
    fn bluetooth_mine_line<'a>(
        &self,
        device: &Device,
        asked: Option<Action>,
        alpha: f32,
        turn: f32,
    ) -> El<'a> {
        let lang = self.lang;
        if asked.is_some() {
            return row![
                spinner(turn),
                t(words::CONNECTING.get(lang), 12.5, DIM.get())
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into();
        }
        if device.connected {
            let said = model::connected(device.kind, lang);
            return match device.battery {
                Some(b) if device.low() => row![
                    t(said, 12.5, fade(GREEN.get(), alpha)),
                    t(" · ", 12.5, fade(DIM.get(), alpha)),
                    t(
                        model::battery(b, lang).trim_start_matches(" · ").to_owned(),
                        12.5,
                        fade(PEACH.get(), alpha)
                    ),
                ]
                .into(),
                Some(b) => t(
                    format!("{said}{}", model::battery(b, lang)),
                    12.5,
                    fade(GREEN.get(), alpha),
                )
                .into(),
                None => t(said, 12.5, fade(GREEN.get(), alpha)).into(),
            };
        }
        t(
            model::not_connected(device, lang),
            12.5,
            fade(DIM.get(), alpha),
        )
        .line_height(1.4)
        .into()
    }

    /// AUTOUR: the devices around to pair, while Bluetooth is on.
    fn bluetooth_nearby(&self) -> El<'_> {
        let lang = self.lang;
        let Some(bt) = self.bluetooth.bt.as_ref() else {
            return Space::new().into();
        };
        let nearby = self.bluetooth.nearby();
        let running = self.bluetooth.running();
        let focused = self.bluetooth_focused(Stop::Nearby);
        let turn = self.bluetooth_turn();
        let mut rows = column![];
        if nearby.is_empty() {
            rows = rows.push(
                container(t(words::NO_NEW_DEVICE_FOUND.get(lang), 13.5, DIM.get()))
                    .padding([14, 16]),
            );
        }
        for (i, device) in nearby.iter().enumerate() {
            let pairing = running.is_some_and(|(p, _)| p == device.path);
            let inert = running.is_some() && !pairing;
            let alpha = if inert { INERT } else { 1.0 };
            let mut lines: Vec<El<'_>> = vec![if pairing {
                row![spinner(turn), t(words::PAIRING.get(lang), 12.5, DIM.get())]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                t(
                    model::nearby_line(device, lang),
                    12.5,
                    fade(DIM.get(), alpha),
                )
                .into()
            }];
            if let Some(failure) = self.bluetooth.failure(&device.path) {
                lines.push(failure_line(failure, lang));
            }
            let controls = (!pairing).then(|| {
                small_button(
                    words::PAIR.get(lang).to_owned(),
                    (!inert).then(|| Message::Bluetooth(Msg::Pair(device.path.clone()))),
                    alpha,
                    format!("bt:pair:{}", device.name),
                )
            });
            let line = Line {
                tile: tile(kind_glyph(device.kind), false, alpha),
                name: device.name.clone(),
                field: None,
                lines,
                controls,
                compact: true,
                alpha,
                focused: focused && self.bluetooth.cursor.1 == i,
            }
            .draw();
            rows = rows.push(divided(
                i > 0,
                container(line)
                    .id(format!("bt:near:{}", device.name))
                    .into(),
            ));
        }
        let searching = bt.searching.then(|| {
            row![
                spinner(turn),
                t(words::SEARCHING.get(lang), 12.0, FAINT.get())
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .into()
        });
        let card = mouse_area(
            container(card_box(rows.into()))
                .style(lit_style(self.lit(model::PAIR)))
                .id(format!("row:{}", model::PAIR)),
        )
        .on_enter(Message::Bluetooth(Msg::Over(true)))
        .on_exit(Message::Bluetooth(Msg::Over(false)));
        container(
            column![
                group_head(words::NEARBY.get(lang), searching),
                card,
                note(words::A_DEVICE_TO_PAIR_MUST_BE.get(lang)),
            ]
            .spacing(8),
        )
        .id("group:bluetooth.nearby".to_owned())
        .into()
    }

    /// PLUS TARD: the computer's name and receiving files, inert.
    fn bluetooth_later(&self) -> El<'_> {
        let lang = self.lang;
        let alias = self
            .bluetooth
            .bt
            .as_ref()
            .map(|b| b.alias.clone())
            .unwrap_or_default();
        let chip = || -> El<'_> {
            container(
                text(words::LATER.get(lang))
                    .size(11.5)
                    .font(BOLD)
                    .color(fade(AMBER.get(), INERT)),
            )
            .padding([2, 7])
            .style(boxed(
                fade(AMBER.get().scale_alpha(0.14), INERT),
                6.0,
                Some((1.0, fade(AMBER.get().scale_alpha(0.35), INERT))),
            ))
            .into()
        };
        let later = |path, name: &'static str, line: &'static str, control: El<'static>| {
            let named = row![
                text(name)
                    .size(14)
                    .font(SEMIBOLD)
                    .color(fade(TEXT.get(), INERT)),
                chip()
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            container(
                row![
                    tile(path, false, INERT),
                    column![
                        named,
                        t(line, 12.5, fade(DIM.get(), INERT)).line_height(1.4)
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    row![Space::new().height(36), control].align_y(Alignment::Center),
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
        };
        let name = later(
            PENCIL,
            words::COMPUTER_NAME.get(lang),
            words::THE_ONE_OTHER_DEVICES_SEE_TODAY.get(lang),
            t(alias, 13.5, fade(DIM.get(), INERT)).into(),
        );
        let files = later(
            FILE,
            words::RECEIVE_FILES.get(lang),
            words::ACCEPT_A_PHOTO_SENT_BY_A.get(lang),
            super::super::screens::switch(false, true, false, 1.0, None),
        );
        container(
            column![
                group_head(words::LATER_2.get(lang), None),
                card_box(column![name, divided(true, files.into())].into()),
            ]
            .spacing(8),
        )
        .id("group:bluetooth.later".to_owned())
        .into()
    }

    /// What the page shows, for the log.
    pub(in crate::app) fn bluetooth_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let Some(bt) = self.bluetooth.bt.as_ref() else {
            return serde_json::json!({ "heard": false });
        };
        let running = self.bluetooth.running();
        let mine: Vec<serde_json::Value> = self
            .bluetooth
            .mine()
            .iter()
            .map(|d| {
                let asked = running.is_some_and(|(p, _)| p == d.path);
                serde_json::json!({
                    "name": d.name,
                    "connected": d.connected,
                    "line": if asked {
                        words::CONNECTING.get(lang).to_owned()
                    } else if d.connected {
                        format!(
                            "{}{}",
                            model::connected(d.kind, lang),
                            d.battery.map(|b| model::battery(b, lang)).unwrap_or_default()
                        )
                    } else {
                        model::not_connected(d, lang)
                    },
                    "low": d.low(),
                    "trusted": d.trusted,
                    "button": (!asked).then(|| if d.connected {
                        words::DISCONNECT.get(lang)
                    } else {
                        words::CONNECT.get(lang)
                    }),
                    "failure": self.bluetooth.failure(&d.path).map(|f| failure_words(f, lang)),
                })
            })
            .collect();
        let nearby: Vec<serde_json::Value> = self
            .bluetooth
            .nearby()
            .iter()
            .map(|d| {
                let pairing = running.is_some_and(|(p, _)| p == d.path);
                serde_json::json!({
                    "name": d.name,
                    "line": if pairing {
                        words::PAIRING.get(lang).to_owned()
                    } else {
                        model::nearby_line(d, lang)
                    },
                    "inert": running.is_some() && !pairing,
                    "failure": self.bluetooth.failure(&d.path).map(|f| failure_words(f, lang)),
                })
            })
            .collect();
        serde_json::json!({
            "heard": true,
            "adapter": bt.adapter,
            "powered": bt.powered,
            "blocked": bt.blocked,
            "visible": bt.discoverable,
            "searching": bt.searching,
            "mine": mine,
            "nearby": if bt.powered { serde_json::json!(nearby) } else { serde_json::Value::Null },
            "cursor": [self.bluetooth.cursor.0, self.bluetooth.cursor.1],
            "menu": self.bluetooth.menu.as_ref().and_then(|(p, _)| self.bluetooth.device(p)).map(|d| d.name.clone()),
            "renaming": self.bluetooth.renaming.as_ref().map(|(_, typed, _)| typed.clone()),
            "forgetting": self.bluetooth.forgetting.as_ref().map(|(_, name)| name.clone()),
            "sheet": self.bluetooth_sheet_request().map(|r| {
                let left = r.until.map(|u| u.saturating_duration_since(std::time::Instant::now()).as_secs_f32().ceil() as u64);
                let (kind, code, typed) = match &r.asked {
                    hocus_bluetooth::Asked::Confirm(key) => ("confirm", Some(grouped(&format!("{key:06}"))), None),
                    hocus_bluetooth::Asked::Display { code, typed } => ("display", Some(code.clone()), *typed),
                    hocus_bluetooth::Asked::Pin => ("pin", None, None),
                    hocus_bluetooth::Asked::Passkey => ("passkey", None, None),
                    hocus_bluetooth::Asked::Authorize => ("authorize", None, None),
                };
                serde_json::json!({
                    "kind": kind,
                    "name": r.name,
                    "code": code,
                    "typed": typed,
                    "left": left,
                    "ours": r.ours,
                })
            }),
        })
    }
}

impl App {
    /// The open "⋯" menu: its list 36 px under its button's top, its right
    /// on the button's.
    pub(in crate::app) fn bluetooth_menu_list(&self) -> Option<El<'_>> {
        let (path, since) = self.bluetooth.menu.as_ref()?;
        let device = self.bluetooth.device(path)?;
        let at = *self.places.at.get(&format!("bt:more:{}", device.name))?;
        let lang = self.lang;
        let opened = if self.reduced_motion {
            1.0
        } else {
            super::super::view::ease_out(
                self.now.saturating_duration_since(*since),
                super::super::MENU_IN,
            )
        };
        let mut list = column![].spacing(0);
        for (i, item) in super::Item::ALL.into_iter().enumerate() {
            let (said, colour, checked) = match item {
                super::Item::Rename => (words::RENAME, TEXT.get(), false),
                super::Item::Trust => (words::CONNECT_AUTOMATICALLY, TEXT.get(), device.trusted),
                super::Item::Forget => (words::FORGET_THIS_DEVICE, PEACH.get(), false),
            };
            if item == super::Item::Forget {
                list =
                    list.push(
                        container(container(Space::new().width(Length::Fill).height(1)).style(
                            boxed(crate::theme::ink(rgba(255, 255, 255, 0.08)), 0.0, None),
                        ))
                        .padding([4, 6]),
                    );
            }
            let check: El<'_> = if checked {
                draw(TICK, 13.0, fade(GREEN.get(), opened), 3.0)
            } else {
                Space::new().width(13).into()
            };
            let keyboard = i == self.bluetooth.menu_index;
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(said.get(lang), 13.5, fade(colour, opened)).font(if checked {
                                BOLD
                            } else {
                                MANROPE
                            }),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding(Padding {
                        top: 0.0,
                        right: 14.0,
                        bottom: 0.0,
                        left: 6.0,
                    })
                    .on_press(Message::Bluetooth(Msg::Item(item)))
                    .style(flat(
                        keyboard.then_some(crate::theme::ink(rgba(255, 255, 255, 0.07))),
                        crate::theme::ink(rgba(255, 255, 255, 0.06)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("bt:item:{i}")),
            );
        }
        let width = 210.0;
        let panel = container(list)
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: iced::Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..crate::app::style::shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let x = (at.x + at.width - width).max(0.0);
        let height = 3.0 * 32.0 + 9.0 + 10.0;
        let bottom = self
            .places
            .page
            .map_or(f32::INFINITY, |p| p.viewport.y + p.viewport.height);
        let y = if at.y + 36.0 + height > bottom {
            at.y - 6.0 - height + 4.0 * (1.0 - opened)
        } else {
            at.y + 36.0 - 4.0 * (1.0 - opened)
        };
        Some(pin(panel).x(x).y(y).into())
    }

    /// The sheet over the page: forgetting a device, or what the agent
    /// asks (a code to compare, to type, a PIN, a device pairing itself).
    pub(in crate::app) fn bluetooth_sheet(&self) -> Option<El<'_>> {
        let lang = self.lang;
        let (width, sheet, close) = if let Some((_, name)) = &self.bluetooth.forgetting {
            let title = words::forget_2(lang, name);
            let body = column![
                text(title).size(17).font(BOLD).color(TEXT),
                t(
                    words::THE_COMPUTER_ERASES_THE_KEY_THEY.get(lang),
                    13.0,
                    DIM.get(),
                )
                .line_height(1.5),
                row![
                    Space::new().width(Length::Fill),
                    sheet_button(
                        words::CANCEL.get(lang),
                        None,
                        Message::Bluetooth(Msg::ForgetCancel),
                        "sheet:cancel"
                    ),
                    sheet_button(
                        words::FORGET.get(lang),
                        Some(FORGET.get()),
                        Message::Bluetooth(Msg::ForgetConfirm),
                        "sheet:ok"
                    ),
                ]
                .spacing(8),
            ]
            .spacing(14);
            (400.0, body, Message::Bluetooth(Msg::ForgetCancel))
        } else {
            let request = self.bluetooth_sheet_request()?;
            self.bluetooth_code_sheet(&request)?
        };
        let sheet = container(sheet)
            .width(width)
            .padding(Padding {
                top: 20.0,
                right: 22.0,
                bottom: 18.0,
                left: 22.0,
            })
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(14.0, 1.0, RIM_12.get()),
                shadow: crate::app::style::shadow(),
                ..container::Style::default()
            })
            .id("sheet".to_owned());
        Some(
            iced::widget::stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(7, 9, 15, 0.62)),
                        0.0,
                        None
                    ))
                )
                .on_press(close),
                container(sheet).center(Length::Fill),
            ]
            .into(),
        )
    }

    /// A pairing's sheet, by what the agent asks: its width, its body,
    /// what a click beside it does.
    fn bluetooth_code_sheet(
        &self,
        request: &hocus_bluetooth::Request,
    ) -> Option<(f32, Column<'_, Message>, Message)> {
        use hocus_bluetooth::{Answer, Asked};
        let lang = self.lang;
        let name = &request.name;
        let cancel = if request.ours {
            Message::Bluetooth(Msg::CancelPairing)
        } else {
            Message::Bluetooth(Msg::Answer(Answer::No))
        };
        let title_words = match &request.asked {
            Asked::Authorize => words::wants_to_pair_with_this_computer(lang, name),
            _ => words::pair_2(lang, name),
        };
        let title = row![
            container(draw(kind_glyph(request.kind), 17.0, BLUE.get(), 1.8))
                .width(32)
                .height(32)
                .center(32)
                .style(boxed(
                    crate::theme::ink(rgba(143, 179, 255, 0.16)),
                    9.0,
                    None
                )),
            text(title_words)
                .size(17)
                .font(BOLD)
                .color(TEXT)
                .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let say = |words: String| t(words, 13.0, DIM.get()).line_height(1.5);
        let cancel_button = |words| sheet_button(words, None, cancel.clone(), "sheet:cancel");
        let ok = |words, msg| sheet_button(words, Some(GREEN.get()), msg, "sheet:ok");
        let buttons = |second: Option<El<'static>>| {
            let mut buttons = row![Space::new().width(Length::Fill)].spacing(8);
            let first = match request.asked {
                Asked::Authorize => words::REFUSE,
                _ => words::CANCEL,
            };
            buttons = buttons.push(cancel_button(first.get(lang)));
            if let Some(second) = second {
                buttons = buttons.push(second);
            }
            buttons
        };
        let the = |kind: Kind| match kind {
            Kind::Phone => words::THE_PHONE.get(lang),
            Kind::Tablet => words::THE_TABLET.get(lang),
            Kind::Computer => words::THE_COMPUTER.get(lang),
            _ => words::THE_DEVICE.get(lang),
        };
        let body = match &request.asked {
            Asked::Confirm(passkey) => {
                let left = request.until.map_or(0, |until| {
                    until
                        .saturating_duration_since(std::time::Instant::now())
                        .as_secs_f32()
                        .ceil() as u64
                });
                column![
                    title,
                    say(words::check_that_shows_the_same_code(
                        lang,
                        the(request.kind)
                    )),
                    container(
                        text(grouped(&format!("{passkey:06}")))
                            .size(34)
                            .font(MONO)
                            .color(TEXT)
                    )
                    .id("sheet:code".to_owned())
                    .center_x(Length::Fill)
                    .padding([4, 0]),
                    container(t(words::s_left(lang, left), 12.0, FAINT.get()))
                        .center_x(Length::Fill),
                    buttons(Some(ok(
                        words::THE_CODE_MATCHES.get(lang),
                        Message::Bluetooth(Msg::Answer(Answer::Yes)),
                    ))),
                ]
            }
            Asked::Display { code, typed } => {
                let on_keyboard = request.kind == Kind::Keyboard;
                let words = match on_keyboard {
                    true => words::TYPE_THIS_CODE_ON_THE_KEYBOARD.get(lang),
                    false => words::TYPE_THIS_CODE_ON_THE_DEVICE.get(lang),
                };
                let lit = usize::from(typed.unwrap_or(0));
                let mut cells = row![].spacing(6);
                for (i, digit) in code.chars().enumerate() {
                    let on = i < lit;
                    cells = cells.push(
                        container(text(digit.to_string()).size(24).font(MONO).color(if on {
                            TEXT.get()
                        } else {
                            DIM.get()
                        }))
                        .width(36)
                        .height(46)
                        .center(46)
                        .style(boxed(
                            if on {
                                GREEN.get().scale_alpha(0.16)
                            } else {
                                crate::theme::ink(rgba(255, 255, 255, 0.05))
                            },
                            9.0,
                            Some((
                                1.0,
                                if on {
                                    GREEN.get().scale_alpha(0.55)
                                } else {
                                    crate::theme::ink(rgba(255, 255, 255, 0.10))
                                },
                            )),
                        )),
                    );
                }
                let mut body = column![
                    title,
                    say(words.to_owned()),
                    container(cells)
                        .id("sheet:cells".to_owned())
                        .center_x(Length::Fill)
                        .padding([4, 0]),
                ];
                if let Some(typed) = typed {
                    body = body.push(
                        container(t(
                            words::of_digits_typed(
                                lang,
                                words::OfDigitsTyped {
                                    code_len: code.len(),
                                    typed,
                                },
                            ),
                            12.0,
                            FAINT.get(),
                        ))
                        .center_x(Length::Fill),
                    );
                }
                body.push(buttons(None))
            }
            Asked::Pin | Asked::Passkey => {
                let typed = if self.bluetooth.pin.0 == request.id {
                    self.bluetooth.pin.1.as_str()
                } else {
                    ""
                };
                column![
                    title,
                    column![
                        text(words::PIN_CODE.get(lang))
                            .size(12)
                            .font(BOLD)
                            .color(DIM),
                        text_input("", typed)
                            .id(PIN)
                            .on_input(|t| Message::Bluetooth(Msg::PinText(t)))
                            .on_submit(Message::Bluetooth(Msg::PinGive))
                            .size(13.5)
                            .font(MONO)
                            .padding([8, 10])
                            .style(input_style),
                        t(
                            words::OFTEN_0000_OR_1234_OR_THE.get(lang),
                            12.0,
                            FAINT.get()
                        ),
                    ]
                    .spacing(5),
                    buttons(Some(ok(
                        words::PAIR.get(lang),
                        Message::Bluetooth(Msg::PinGive),
                    ))),
                ]
            }
            Asked::Authorize => column![
                title,
                buttons(Some(ok(
                    words::PAIR.get(lang),
                    Message::Bluetooth(Msg::Answer(Answer::Yes)),
                ))),
            ],
        };
        Some((440.0, body.spacing(14), cancel))
    }
}

/// What a failure says under its row.
fn failure_words(failure: Failure, lang: Lang) -> &'static str {
    match failure {
        Failure::NoAnswer => words::IT_DOES_NOT_ANSWER_CHECK_THAT,
        Failure::Pairing | Failure::Code | Failure::Stopped => {
            words::PAIRING_FAILED_PUT_THE_DEVICE_IN
        }
        Failure::Cancelled => words::THE_DEVICE_CANCELLED_THE_PAIRING,
    }
    .get(lang)
}

fn failure_line<'a>(failure: Failure, lang: Lang) -> El<'a> {
    t(failure_words(failure, lang), 12.5, PEACH.get())
        .line_height(1.4)
        .into()
}
