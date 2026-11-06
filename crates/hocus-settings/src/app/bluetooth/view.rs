//! What the Bluetooth page draws (the approved mockup, variant A): the
//! switch's card, MES APPAREILS, AUTOUR and PLUS TARD; without an adapter,
//! the info box alone.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_bluetooth::{Action, Device, Failure, Kind};
use iced::{
    Alignment, Background, Color, Length, Padding, Theme,
    widget::{
        Column, Space, button, column, container, mouse_area, rich_text, row, span, svg, text,
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
    lang::{Lang, w},
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
    lines: Vec<El<'a>>,
    controls: Option<El<'a>>,
    compact: bool,
    alpha: f32,
    focused: bool,
}

impl<'a> Line<'a> {
    fn draw(self) -> El<'a> {
        let mut words = column![
            text(self.name)
                .size(14)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), self.alpha))
        ]
        .spacing(2)
        .width(Length::Fill);
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
                        span(
                            w("Aucun adaptateur Bluetooth.", "No Bluetooth adapter.").get(lang),
                        )
                        .font(BOLD)
                        .color(INFO_TEXT.get()),
                        span(
                            w(
                                " Cet ordinateur n’en a pas, ou le service Bluetooth (BlueZ) ne tourne pas. Une clé USB Bluetooth branchée apparaît ici aussitôt.",
                                " This computer has none, or the Bluetooth service (BlueZ) is not running. A USB Bluetooth dongle plugged in shows here at once.",
                            )
                            .get(lang),
                        )
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
            let line = match lang {
                Lang::French => format!(
                    "Visible sous le nom « {} » tant que cette page est ouverte.",
                    bt.alias
                ),
                Lang::English => {
                    format!("Visible as “{}” while this page is open.", bt.alias)
                }
            };
            (line, DIM.get())
        } else if bt.blocked {
            (
                w(
                    "Bloqué par l’interrupteur sans fil ou le mode avion.",
                    "Blocked by the wireless switch or airplane mode.",
                )
                .get(lang)
                .to_owned(),
                PEACH.get(),
            )
        } else {
            (
                w(
                    "Coupé. Tes appareils restent associés : ils reviennent quand tu le rallumes.",
                    "Off. Your devices stay paired: they come back when you turn it on.",
                )
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
                    w("Aucun appareil associé.", "No paired device.").get(lang),
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
                (
                    w("Déconnecter", "Disconnect"),
                    Msg::Disconnect(device.path.clone()),
                )
            } else {
                (w("Connecter", "Connect"), Msg::Connect(device.path.clone()))
            };
            let enabled = bt.powered && running.is_none();
            let controls = asked.is_none().then(|| {
                small_button(
                    act.0.get(lang).to_owned(),
                    enabled.then_some(Message::Bluetooth(act.1)),
                    if enabled || !bt.powered { alpha } else { INERT },
                    format!("bt:act:{}", device.name),
                )
            });
            let line = Line {
                tile: tile(kind_glyph(device.kind), device.connected, alpha),
                name: device.name.clone(),
                lines,
                controls,
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
        container(
            column![
                group_head(w("MES APPAREILS", "MY DEVICES").get(lang), count),
                card
            ]
            .spacing(8),
        )
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
                t(w("Connexion…", "Connecting…").get(lang), 12.5, DIM.get())
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
                container(t(
                    w("Aucun nouvel appareil trouvé.", "No new device found.").get(lang),
                    13.5,
                    DIM.get(),
                ))
                .padding([14, 16]),
            );
        }
        for (i, device) in nearby.iter().enumerate() {
            let pairing = running.is_some_and(|(p, _)| p == device.path);
            let inert = running.is_some() && !pairing;
            let alpha = if inert { INERT } else { 1.0 };
            let mut lines: Vec<El<'_>> = vec![if pairing {
                row![
                    spinner(turn),
                    t(w("Association…", "Pairing…").get(lang), 12.5, DIM.get())
                ]
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
                    w("Associer", "Pair").get(lang).to_owned(),
                    (!inert).then(|| Message::Bluetooth(Msg::Pair(device.path.clone()))),
                    alpha,
                    format!("bt:pair:{}", device.name),
                )
            });
            let line = Line {
                tile: tile(kind_glyph(device.kind), false, alpha),
                name: device.name.clone(),
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
                t(w("Recherche…", "Searching…").get(lang), 12.0, FAINT.get())
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
                group_head(w("AUTOUR", "NEARBY").get(lang), searching),
                card,
                note(
                    w(
                        "Un appareil à associer doit être en mode association : souvent un appui long sur son bouton. Les plus proches d’abord.",
                        "A device to pair must be in pairing mode: often a long press on its button. Nearest first.",
                    )
                    .get(lang)
                ),
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
                text(w("Plus tard", "Later").get(lang))
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
            w("Nom de l’ordinateur", "Computer name").get(lang),
            w(
                "Celui que voient les autres appareils ; aujourd’hui le nom de la machine.",
                "The one other devices see; today the machine's name.",
            )
            .get(lang),
            t(alias, 13.5, fade(DIM.get(), INERT)).into(),
        );
        let files = later(
            FILE,
            w("Recevoir des fichiers", "Receive files").get(lang),
            w(
                "Accepter une photo envoyée par un téléphone.",
                "Accept a photo sent by a phone.",
            )
            .get(lang),
            super::super::screens::switch(false, true, false, 1.0, None),
        );
        container(
            column![
                group_head(w("PLUS TARD", "LATER").get(lang), None),
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
                        w("Connexion…", "Connecting…").get(lang).to_owned()
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
                    "button": (!asked).then(|| if d.connected {
                        w("Déconnecter", "Disconnect").get(lang)
                    } else {
                        w("Connecter", "Connect").get(lang)
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
                        w("Association…", "Pairing…").get(lang).to_owned()
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
        })
    }
}

/// What a failure says under its row.
fn failure_words(failure: Failure, lang: Lang) -> &'static str {
    match failure {
        Failure::NoAnswer => w(
            "Il ne répond pas. Vérifie qu’il est allumé et à portée.",
            "It does not answer. Check that it is on and in range.",
        ),
        Failure::Pairing | Failure::Code => w(
            "L’association a échoué. Mets l’appareil en mode association et réessaie.",
            "Pairing failed. Put the device in pairing mode and try again.",
        ),
    }
    .get(lang)
}

fn failure_line<'a>(failure: Failure, lang: Lang) -> El<'a> {
    t(failure_words(failure, lang), 12.5, PEACH.get())
        .line_height(1.4)
        .into()
}
