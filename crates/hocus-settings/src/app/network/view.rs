//! What the Réseau page draws (the approved mockup, variant A): airplane
//! mode's card, WI-FI (the switch, the network in use, the saved
//! networks' row), RÉSEAUX DISPONIBLES, PLUS TARD; the saved networks'
//! sub-page; the sheet over the page.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
    time::Instant,
};

use hocus_network::{
    model::{Saved, Security, Wifi, bars},
    rfkill::Block,
};
use iced::{
    Alignment, Background, Color, Gradient, Length, Padding, Radians, Rotation, Shadow, Theme,
    Vector, gradient,
    widget::{Space, button, column, container, mouse_area, row, svg, text, text_input},
};

use super::{Msg, PASSWORD_FIELD, SHEET_FIELD, Stop, now};
use crate::{
    app::{
        App, Message,
        style::{self, *},
        view::{El, card_box, divided, ease_out, label, t},
    },
    lang::{Lang, w},
    network as model,
    theme::Ink,
};

/// The page's colours: on and connected (the mockup's green), airplane
/// mode's line, its tile.
const GREEN: Ink = Ink::Dark(rgb(0x6CC287));
const AMBER: Ink = Ink::Dark(rgb(0xF5C46E));
const LATER: Ink = Ink::Dark(rgb(0xF5B44A));

/// The page's glyphs: the mockup's 24 px stroke paths.
const AIRPLANE: &str = "M10.6 4 C10.6 2.6 13.4 2.6 13.4 4 V9.6 L21 14.2 V16.2 L13.4 13.8 V18.4 L15.8 20.2 V21.4 L12 20.4 L8.2 21.4 V20.2 L10.6 18.4 V13.8 L3 16.2 V14.2 L10.6 9.6 Z";
const KEY: &str = "M8 15.5 A3.5 3.5 0 1 0 8 8.5 A3.5 3.5 0 1 0 8 15.5 Z M11.5 12 H20.5 M17.5 12 V15 M20.5 12 V14.5";
const LOCK: &str = "M7.5 11 V8 A4.5 4.5 0 0 1 16.5 8 V11 M5.5 11 H18.5 V20.5 H5.5 Z";
const CHEVRON_RIGHT: &str = "M9 6 L15 12 L9 18";
const CHEVRON_LEFT: &str = "M15 6 L9 12 L15 18";
const CHEVRON_DOWN: &str = "M6 9 L12 15 L18 9";
const CHEVRON_UP: &str = "M6 15 L12 9 L18 15";
const EYE: &str = "M2.5 12 C5 7 8.5 5 12 5 C15.5 5 19 7 21.5 12 C19 17 15.5 19 12 19 C8.5 19 5 17 2.5 12 Z M12 15 A3 3 0 1 0 12 9 A3 3 0 1 0 12 15 Z";
const EYE_SHUT: &str = "M2.5 12 C5 7 8.5 5 12 5 C15.5 5 19 7 21.5 12 C19 17 15.5 19 12 19 C8.5 19 5 17 2.5 12 Z M12 15 A3 3 0 1 0 12 9 A3 3 0 1 0 12 15 Z M4 4 L20 20";
const SPINNER: &str = "M12 3.5 A8.5 8.5 0 1 1 3.5 12";
const WARNING: &str = "M12 4 L21 19.5 H3 Z M12 10 V14 M12 16.8 V17";
/// The Wi-Fi's dot and arcs, from the smallest.
const WIFI_PARTS: [&str; 4] = [
    "M12 19.5 V19.6",
    "M8.5 16 A5 5 0 0 1 15.5 16",
    "M5.5 12.5 A9.5 9.5 0 0 1 18.5 12.5",
    "M2.5 9 A14 14 0 0 1 21.5 9",
];

/// A tile's tone: neutral, green (on, connected), blue (airplane mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Plain,
    Green,
    Blue,
}

/// `paths` stroked in `color`, `width` wide, drawn once.
fn glyph(paths: &[(&'static str, f32)], color: Color, width: f32) -> svg::Handle {
    type Drawn = HashMap<(Vec<(&'static str, u32)>, [u8; 4], u32), svg::Handle>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let rgba = color.into_rgba8();
    let key: Vec<(&'static str, u32)> = paths.iter().map(|(p, a)| (*p, a.to_bits())).collect();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry((key, rgba, width.to_bits()))
        .or_insert_with(|| {
            let [r, g, b, a] = rgba;
            let strokes: String = paths
                .iter()
                .map(|(path, alpha)| {
                    format!(
                        "<path d=\"{path}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" \
                         stroke-opacity=\"{}\" stroke-width=\"{width}\" stroke-linecap=\"round\" \
                         stroke-linejoin=\"round\"/>",
                        f32::from(a) / 255.0 * alpha
                    )
                })
                .collect();
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                     height=\"24\">{strokes}</svg>"
                )
                .into_bytes(),
            )
        })
        .clone()
}

fn draw<'a>(path: &'static str, size: f32, color: Color) -> El<'a> {
    svg(glyph(&[(path, 1.0)], color, 1.8))
        .width(size)
        .height(size)
        .into()
}

/// The Wi-Fi's glyph, `lit` of its four parts lit, the others faint.
fn wifi_glyph<'a>(lit: u8, size: f32, color: Color) -> El<'a> {
    let parts: Vec<(&'static str, f32)> = WIFI_PARTS
        .iter()
        .enumerate()
        .map(|(i, p)| (*p, if i < usize::from(lit) { 1.0 } else { 0.3 }))
        .collect();
    svg(glyph(&parts, color, 1.9))
        .width(size)
        .height(size)
        .into()
}

/// A 32 px tile around `inner`.
fn tile<'a>(inner: impl FnOnce(Color) -> El<'a>, tone: Tone, alpha: f32) -> El<'a> {
    let (fill, ink) = match tone {
        Tone::Plain => (rgba(255, 255, 255, 0.06), DIM.get()),
        Tone::Green => (rgba(108, 194, 135, 0.16), GREEN.get()),
        Tone::Blue => (rgba(143, 179, 255, 0.16), BLUE.get()),
    };
    container(inner(fade(ink, alpha)))
        .width(32)
        .height(32)
        .center(32)
        .style(boxed(fade(crate::theme::ink(fill), alpha), 9.0, None))
        .into()
}

/// The spinner, turning once in 0.9 s (still with reduced motion).
fn spinner<'a>(size: f32, color: Color, turning: bool) -> El<'a> {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    let angle = if turning {
        (start.elapsed().as_secs_f32() / 0.9).fract() * std::f32::consts::TAU
    } else {
        0.0
    };
    svg(glyph(&[(SPINNER, 1.0)], color, 2.2))
        .width(size)
        .height(size)
        .rotation(Rotation::Floating(Radians(angle)))
        .into()
}

/// A row of the page: its tile, its name and lines, its control on the
/// right; 54 px at least (46 for a compact one).
#[allow(clippy::too_many_arguments)]
fn page_row<'a>(
    tile: El<'a>,
    name: El<'a>,
    lines: Vec<El<'a>>,
    control: El<'a>,
    id: Option<String>,
    lit: f32,
    compact: bool,
    focused: bool,
) -> El<'a> {
    let mut words = column![name].spacing(2).width(Length::Fill);
    for line in lines {
        words = words.push(line);
    }
    let pad = if compact { 6.0 } else { 9.0 };
    let least = if compact { 34.0 } else { 36.0 };
    let body = container(
        row![
            tile,
            words,
            row![Space::new().height(least), control]
                .spacing(10)
                .align_y(Alignment::Center)
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: pad,
        right: 14.0,
        bottom: pad,
        left: 16.0,
    })
    .style(move |_: &Theme| container::Style {
        background: (lit > 0.0).then_some(Background::Color(Ink::Accent(0.20 * lit).get())),
        border: if focused && lit <= 0.0 {
            ring(11.0)
        } else {
            border(11.0, 1.5, Ink::Accent(0.85 * lit).get())
        },
        ..container::Style::default()
    });
    match id {
        Some(id) => body.id(id).into(),
        None => body.into(),
    }
}

/// A row's name, at `alpha`, and what follows it (a lock, a chip).
fn name<'a>(said: String, alpha: f32, after: Option<El<'a>>) -> El<'a> {
    let said = text(said)
        .size(14)
        .font(SEMIBOLD)
        .color(fade(TEXT.get(), alpha))
        .wrapping(text::Wrapping::None);
    match after {
        Some(after) => row![said, after]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        None => said.into(),
    }
}

fn line<'a>(said: impl Into<String>, color: Color) -> El<'a> {
    t(said.into(), 12.5, color).into()
}

/// A secondary button: 28 px (`small`) or 32.
fn secondary<'a>(said: String, press: Option<Message>, focused: bool, small: bool) -> El<'a> {
    let alpha = if press.is_some() { 1.0 } else { INERT };
    let (height, size, radius, pad) = if small {
        (28.0, 12.5, 7.0, 11.0)
    } else {
        (32.0, 13.0, 8.0, 14.0)
    };
    let mut b = button(
        container(t(said, size, fade(TEXT.get(), alpha)).font(BOLD))
            .height(Length::Fill)
            .center_y(Length::Fill),
    )
    .height(height)
    .padding([0.0, pad])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(fade(
                crate::theme::ink(rgba(255, 255, 255, if hovered { 0.09 } else { 0.06 })),
                alpha,
            ))),
            text_color: TEXT.get(),
            border: if focused {
                ring(radius)
            } else {
                border(radius, 1.0, fade(RIM_12.get(), alpha))
            },
            ..button::Style::default()
        }
    });
    if let Some(press) = press {
        b = b.on_press(press);
    }
    b.into()
}

/// A ghost button: its words only, dim.
fn ghost<'a>(said: String, press: Message, focused: bool) -> El<'a> {
    button(
        container(t(said, 12.5, DIM.get()).font(BOLD))
            .height(Length::Fill)
            .center_y(Length::Fill),
    )
    .height(28)
    .padding([0, 11])
    .on_press(press)
    .style(flat(None, HOVER.get(), 7.0, focused))
    .into()
}

/// The amber "Plus tard".
fn later<'a>(lang: Lang) -> El<'a> {
    container(
        text(w("Plus tard", "Later").get(lang))
            .size(11.5)
            .font(BOLD)
            .color(LATER),
    )
    .padding([1, 7])
    .style(boxed(
        crate::theme::ink(rgba(245, 180, 74, 0.14)),
        6.0,
        Some((1.0, crate::theme::ink(rgba(245, 180, 74, 0.35)))),
    ))
    .into()
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

fn note<'a>(words: &'static str) -> El<'a> {
    container(t(words, 12.5, FAINT.get()).line_height(1.5))
        .padding([0, 4])
        .into()
}

impl App {
    /// A switch of the page, its knob sliding over 160 ms.
    fn network_switch(
        &self,
        key: &'static str,
        on: bool,
        press: Option<Message>,
        focused: bool,
    ) -> El<'_> {
        let alpha = if press.is_some() { 1.0 } else { INERT };
        let moved = self.knobs.get(key).map_or(1.0, |since| {
            ease_out(
                self.now.saturating_duration_since(*since),
                super::super::KNOB,
            )
        });
        let at = if on { moved } else { 1.0 - moved };
        let track = if on {
            GREEN.get()
        } else {
            Ink::Token(|t| t.switch_off).get()
        };
        let knob =
            container(Space::new().width(18).height(18)).style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(Color::WHITE, alpha))),
                border: border(9.0, 0.0, Color::TRANSPARENT),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.3 * alpha),
                    offset: Vector::new(0.0, 1.0),
                    blur_radius: 2.0,
                },
                ..container::Style::default()
            });
        let mut switch =
            button(container(knob).padding(Padding::default().left(2.0 + 18.0 * at).top(2)))
                .width(40)
                .height(22)
                .padding(0)
                .style(move |_, _| button::Style {
                    background: Some(Background::Color(fade(track, alpha))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(11.0)
                    } else {
                        border(11.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                });
        if let Some(press) = press {
            switch = switch.on_press(press);
        }
        container(switch).id(format!("switch:{key}")).into()
    }

    /// The page under its title, or the saved networks' sub-page.
    pub(in crate::app) fn network_page<'a>(
        &'a self,
        page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        if self.network.sub {
            return self.network_saved_page();
        }
        let lang = self.lang;
        let mut page = page;
        let manager = self.network.manager();
        if !manager {
            page = page.push(self.network_banner());
        }
        if !manager {
            // Read-only: what the rows would say, unknown.
            let rows = column![
                self.network_airplane_row(),
                divided(true, self.network_wifi_row())
            ];
            page =
                page.push(container(card_box(rows.into())).id("group:network.airplane".to_owned()));
            return container(page).id("page:network".to_owned()).into();
        }
        page = page.push(
            container(card_box(self.network_airplane_row()))
                .id("group:network.airplane".to_owned()),
        );
        // WI-FI.
        let mut rows = column![self.network_wifi_row()];
        if let Some(current) = self.network_current_row() {
            rows = rows.push(divided(true, current));
        }
        rows = rows.push(divided(true, self.network_saved_row()));
        page = page.push(
            container(
                column![
                    group_head(WIFI_LABEL.get(lang), None),
                    card_box(rows.into())
                ]
                .spacing(8),
            )
            .id("group:network.wifi".to_owned()),
        );
        // RÉSEAUX DISPONIBLES, while the Wi-Fi is on.
        if self.network.wifi_on() {
            page = page.push(self.network_around_group());
        }
        // PLUS TARD.
        let hotspot = page_row(
            tile(|c| wifi_glyph(4, 18.0, c), Tone::Plain, INERT),
            name(
                w("Partage de connexion Wi-Fi", "Wi-Fi hotspot")
                    .get(lang)
                    .to_owned(),
                INERT,
                Some(later(lang)),
            ),
            vec![line(
                w(
                    "Faire de l’ordinateur un point d’accès pour un téléphone ou une tablette.",
                    "Make the computer an access point for a phone or a tablet.",
                )
                .get(lang),
                fade(DIM.get(), INERT),
            )],
            self.network_switch("network.hotspot", false, None, false),
            Some("row:network.hotspot".to_owned()),
            0.0,
            false,
            false,
        );
        page = page.push(
            column![
                group_head(w("PLUS TARD", "LATER").get(lang), None),
                card_box(hotspot)
            ]
            .spacing(8),
        );
        container(page).id("page:network".to_owned()).into()
    }

    /// 6.1's banner, amber: NetworkManager is not there.
    fn network_banner(&self) -> El<'_> {
        let lang = self.lang;
        container(
            row![
                container(draw(WARNING, 18.0, PEACH.get())).padding(Padding::default().top(1)),
                column![
                    t(
                        w("NetworkManager ne répond pas", "NetworkManager does not answer").get(lang),
                        13.5,
                        PEACH.get()
                    )
                    .font(BOLD),
                    t(
                        w(
                            "Le réseau de cet ordinateur est géré par un autre outil, ou ce service est arrêté. Cette page se remplit dès qu’il revient.",
                            "This computer's network is managed by another tool, or this service is stopped. This page fills in as soon as it is back.",
                        )
                        .get(lang),
                        12.5,
                        SOFT.get(),
                    )
                    .line_height(1.45),
                ]
                .spacing(4),
            ]
            .spacing(12),
        )
        .width(Length::Fill)
        .padding([11, 14])
        .style(boxed(
            crate::theme::ink(rgba(242, 148, 91, 0.12)),
            10.0,
            Some((1.0, crate::theme::ink(rgba(242, 148, 91, 0.40)))),
        ))
        .id("banner:network".to_owned())
        .into()
    }

    fn network_airplane_row(&self) -> El<'_> {
        let lang = self.lang;
        let manager = self.network.manager();
        let readable = self.network.radios.is_some();
        let on = self.network.airplane();
        let alive = manager && readable;
        let alpha = if alive { 1.0 } else { INERT };
        let said = if !readable && manager {
            line(
                w(
                    "Les radios ne se lisent pas (/dev/rfkill).",
                    "The radios do not read (/dev/rfkill).",
                )
                .get(lang),
                FAINT.get(),
            )
        } else if on && manager {
            line(
                w(
                    "Activé : le Wi-Fi et le Bluetooth sont coupés.",
                    "On: Wi-Fi and Bluetooth are off.",
                )
                .get(lang),
                AMBER.get(),
            )
        } else {
            line(
                w(
                    "Coupe le Wi-Fi et le Bluetooth ; un câble reste branché.",
                    "Turns Wi-Fi and Bluetooth off; a cable stays connected.",
                )
                .get(lang),
                fade(DIM.get(), alpha),
            )
        };
        page_row(
            tile(
                |c| draw(AIRPLANE, 18.0, c),
                if on && manager {
                    Tone::Blue
                } else {
                    Tone::Plain
                },
                alpha,
            ),
            name(
                w("Mode avion", "Airplane mode").get(lang).to_owned(),
                alpha,
                None,
            ),
            vec![said],
            self.network_switch(
                "network.airplane",
                on && manager,
                alive.then_some(Message::Network(Msg::Airplane)),
                self.network_focused(Stop::Airplane),
            ),
            Some("row:network.airplane".to_owned()),
            self.lit("network.airplane"),
            false,
            false,
        )
    }

    fn network_wifi_row(&self) -> El<'_> {
        let lang = self.lang;
        let details = self.network.details.as_deref();
        let manager = self.network.manager();
        let hardware = details.is_some_and(|d| d.network.hardware);
        let block = self.network.wifi_block();
        let on = self.network.wifi_on();
        let (said, color) = if !manager {
            (w("Inconnu", "Unknown"), fade(DIM.get(), INERT))
        } else if !hardware {
            (w("Aucune carte Wi-Fi.", "No Wi-Fi adapter."), DIM.get())
        } else if block == Block::Hard {
            (
                w(
                    "Bloqué par l’interrupteur sans fil.",
                    "Blocked by the wireless switch.",
                ),
                PEACH.get(),
            )
        } else if self.network.airplane() {
            (
                w(
                    "Coupé par le mode avion. L’allumer ici le rallume seul.",
                    "Off by airplane mode. Turning it on here turns it on alone.",
                ),
                DIM.get(),
            )
        } else {
            (
                w(
                    "Carte Wi-Fi intégrée · cherche et rejoint seule les réseaux enregistrés.",
                    "Built-in Wi-Fi adapter · finds and joins saved networks by itself.",
                ),
                DIM.get(),
            )
        };
        let alpha = if manager { 1.0 } else { INERT };
        let switchable = manager && hardware && block != Block::Hard;
        let on = on && manager;
        let control: El<'_> = if manager && !hardware {
            Space::new().into()
        } else {
            self.network_switch(
                "network.wifi",
                on,
                switchable.then_some(Message::Network(Msg::Wifi)),
                self.network_focused(Stop::Wifi),
            )
        };
        let lit_parts = if on { 4 } else { 0 };
        page_row(
            tile(
                |c| wifi_glyph(lit_parts.max(1), 18.0, c),
                if on { Tone::Green } else { Tone::Plain },
                alpha,
            ),
            name("Wi-Fi".to_owned(), alpha, None),
            vec![line(said.get(lang), color)],
            control,
            Some("row:network.wifi".to_owned()),
            self.lit("network.wifi"),
            false,
            false,
        )
    }

    /// The network in use, while the Wi-Fi is on one.
    fn network_current_row(&self) -> Option<El<'_>> {
        if !self.network.wifi_on() {
            return None;
        }
        let lang = self.lang;
        let details = self.network.details.as_deref()?;
        let wifi = details.network.current.as_ref()?;
        let current = details.current.as_ref();
        let mut lines = vec![line(model::connected(wifi, current, lang), GREEN.get())];
        if let Some(address) = current.and_then(|c| model::address(c, lang)) {
            lines.push(line(address, DIM.get()));
        }
        if details.network.portal {
            lines.push(line(
                w("Page de connexion à ouvrir", "Sign-in page to open").get(lang),
                AMBER.get(),
            ));
        }
        let controls = row![
            container(secondary(
                w("Déconnecter", "Disconnect").get(lang).to_owned(),
                Some(Message::Network(Msg::Disconnect)),
                self.network_focused(Stop::Disconnect),
                true,
            ))
            .id("network:disconnect".to_owned()),
            container(ghost(
                w("Détails", "Details").get(lang).to_owned(),
                Message::Network(Msg::Details),
                self.network_focused(Stop::Details),
            ))
            .id("network:details".to_owned()),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        Some(page_row(
            tile(|c| wifi_glyph(wifi.bars(), 18.0, c), Tone::Green, 1.0),
            name(wifi.name.clone(), 1.0, None),
            lines,
            controls.into(),
            Some("row:network.address".to_owned()),
            self.lit("network.address"),
            false,
            false,
        ))
    }

    fn network_saved_row(&self) -> El<'_> {
        let lang = self.lang;
        let count = self.network.details.as_deref().map_or(0, |d| d.saved.len());
        let said = if self.network.airplane() {
            w("Ils restent enregistrés.", "They stay saved.")
        } else {
            w(
                "Ceux que l’ordinateur rejoint seul : les oublier, changer leur mot de passe.",
                "Those the computer joins by itself: forget them, change their password.",
            )
        };
        let focused = self.network_focused(Stop::Saved);
        let chip = container(t(count.to_string(), 12.0, DIM.get()).font(SEMIBOLD))
            .padding([2, 7])
            .style(boxed(
                crate::theme::ink(rgba(255, 255, 255, 0.08)),
                6.0,
                None,
            ));
        let inner = page_row(
            tile(|c| draw(KEY, 18.0, c), Tone::Plain, 1.0),
            name(
                w("Réseaux enregistrés", "Saved networks")
                    .get(lang)
                    .to_owned(),
                1.0,
                None,
            ),
            vec![line(said.get(lang), DIM.get())],
            row![chip, draw(CHEVRON_RIGHT, 16.0, DIM.get())]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            None,
            self.lit("network.saved"),
            false,
            focused,
        );
        container(
            button(inner)
                .padding(0)
                .width(Length::Fill)
                .on_press(Message::Network(Msg::Saved))
                .style(|_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: hovered.then_some(Background::Color(HOVER.get())),
                        text_color: TEXT.get(),
                        border: border(11.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    }
                }),
        )
        .id("row:network.saved".to_owned())
        .into()
    }

    /// RÉSEAUX DISPONIBLES: one row per network around but the one in
    /// use, strongest first, held under the pointer.
    fn network_around_group(&self) -> El<'_> {
        let lang = self.lang;
        let details = self.network.details.as_deref();
        let scanning = details.is_some_and(|d| d.network.scanning);
        let right: Option<El<'_>> = scanning.then(|| {
            row![
                spinner(12.0, FAINT.get(), !self.reduced_motion),
                t(w("Recherche…", "Searching…").get(lang), 12.0, FAINT.get())
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .into()
        });
        let around = self.network.around();
        let connecting = details.and_then(|d| d.network.connecting_to());
        let focused = self.network_focused(Stop::Around);
        let mut rows = column![];
        if around.is_empty() {
            rows = rows.push(
                container(t(
                    w("Aucun réseau trouvé.", "No network found.").get(lang),
                    13.5,
                    DIM.get(),
                ))
                .padding([16, 16]),
            );
        }
        for (i, wifi) in around.iter().enumerate() {
            let row =
                self.network_around_row(wifi, connecting, focused && self.network.cursor.0 == i);
            rows = rows.push(divided(i > 0, row));
        }
        let list = mouse_area(card_box(rows.into()))
            .on_enter(Message::Network(Msg::Hover(true)))
            .on_exit(Message::Network(Msg::Hover(false)));
        container(
            column![
                group_head(
                    w("RÉSEAUX DISPONIBLES", "AVAILABLE NETWORKS").get(lang),
                    right
                ),
                list,
                note(
                    w(
                        "Les plus forts d’abord. La liste reste en place sous la souris.",
                        "Strongest first. The list stays in place under the pointer.",
                    )
                    .get(lang)
                ),
            ]
            .spacing(8),
        )
        .id("group:network.around".to_owned())
        .into()
    }

    fn network_around_row(&self, wifi: &Wifi, connecting: Option<&[u8]>, focused: bool) -> El<'_> {
        let lang = self.lang;
        let (said, warns) = model::around(wifi, lang);
        let busy = connecting.is_some();
        let this = connecting == Some(wifi.ssid.as_slice());
        let alpha = if busy && !this { INERT } else { 1.0 };
        let lock = wifi
            .security
            .locked()
            .then(|| draw(LOCK, 13.0, fade(DIM.get(), alpha)));
        let at_once = model::joins_at_once(wifi);
        let control: El<'_> = if this {
            row![
                spinner(14.0, DIM.get(), !self.reduced_motion),
                t(w("Connexion…", "Connecting…").get(lang), 12.5, DIM.get())
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .into()
        } else {
            let label = if at_once {
                w("Se connecter", "Connect")
            } else {
                w("Se connecter…", "Connect…")
            };
            // 802.1X and WEP join from their sheet, which comes with the
            // page's second part.
            let can = !busy && (at_once || wifi.security.joinable());
            secondary(
                label.get(lang).to_owned(),
                can.then(|| Message::Network(Msg::Connect(wifi.ssid.clone()))),
                false,
                true,
            )
        };
        let ink = if warns { PEACH.get() } else { DIM.get() };
        page_row(
            tile(|c| wifi_glyph(wifi.bars(), 18.0, c), Tone::Plain, alpha),
            name(wifi.name.clone(), alpha, lock),
            vec![line(said, fade(ink, alpha))],
            container(control)
                .id(format!("connect:{}", wifi.name))
                .into(),
            Some(format!("around:{}", wifi.name)),
            0.0,
            true,
            focused,
        )
    }

    // --- The saved networks ---------------------------------------------------------

    fn network_saved_page(&self) -> El<'_> {
        let lang = self.lang;
        let back = container(
            button(
                row![
                    draw(CHEVRON_LEFT, 14.0, MINT.get()),
                    t(w("Réseau", "Network").get(lang), 13.0, MINT.get()).font(SEMIBOLD)
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([3, 4])
            .on_press(Message::Network(Msg::Back))
            .style(flat(
                None,
                HOVER.get(),
                7.0,
                self.network_focused(Stop::Back),
            )),
        )
        .id("network:back".to_owned());
        let title = column![
            back,
            text(w("Réseaux enregistrés", "Saved networks").get(lang))
                .size(22)
                .font(BOLD)
                .color(TEXT),
            t(
                w(
                    "Les plus récents d’abord. Un réseau oublié perd son mot de passe.",
                    "Most recent first. A forgotten network loses its password.",
                )
                .get(lang),
                13.5,
                DIM.get()
            )
            .line_height(1.5),
        ]
        .spacing(5);
        let saved = self.network.saved();
        let mut rows = column![];
        if saved.is_empty() {
            rows = rows.push(
                container(t(
                    w("Aucun réseau enregistré.", "No saved network.").get(lang),
                    13.5,
                    DIM.get(),
                ))
                .padding([16, 16]),
            );
        }
        let focused = self.network_focused(Stop::SavedList);
        let now = now();
        for (i, network) in saved.iter().enumerate() {
            let ends = (i == 0, i + 1 == saved.len());
            let row =
                self.network_saved_entry(network, now, focused && self.network.cursor.1 == i, ends);
            rows = rows.push(divided(i > 0, row));
        }
        container(
            column![title, card_box(rows.into())]
                .spacing(18)
                .padding(Padding::default().top(14)),
        )
        .id("page:network.saved".to_owned())
        .into()
    }

    /// A saved network's row, unfolded or not; `ends`: it is the card's
    /// first, its last (their corners rounded as the card's).
    fn network_saved_entry(
        &self,
        saved: &Saved,
        now: u64,
        focused: bool,
        ends: (bool, bool),
    ) -> El<'_> {
        let lang = self.lang;
        let open = self.network.unfolded.as_deref() == Some(saved.uuid.as_str());
        let color = if saved.connected {
            GREEN.get()
        } else {
            DIM.get()
        };
        let head = page_row(
            tile(
                |c| draw(KEY, 18.0, c),
                if saved.connected {
                    Tone::Green
                } else {
                    Tone::Plain
                },
                1.0,
            ),
            name(saved.name.clone(), 1.0, None),
            vec![line(model::saved_line(saved, now, lang), color)],
            draw(
                if open { CHEVRON_UP } else { CHEVRON_DOWN },
                16.0,
                DIM.get(),
            ),
            None,
            0.0,
            false,
            focused,
        );
        let head = container(
            button(head)
                .padding(0)
                .width(Length::Fill)
                .on_press(Message::Network(Msg::Unfold(saved.uuid.clone())))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: (hovered && !open).then_some(Background::Color(HOVER.get())),
                        text_color: TEXT.get(),
                        border: border(11.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    }
                }),
        )
        .id(format!("saved:{}", saved.name));
        if !open {
            return head.into();
        }
        let mut body = column![head].spacing(0);
        let mut inner = column![].spacing(10);
        // Se connecter seul.
        inner = inner.push(
            row![
                t(
                    w("Se connecter seul", "Connect automatically").get(lang),
                    13.5,
                    TEXT.get()
                )
                .width(Length::Fill),
                self.network_switch(
                    "network.autoconnect",
                    saved.autoconnect,
                    Some(Message::Network(Msg::Autoconnect(saved.uuid.clone()))),
                    self.network_focused(Stop::Autoconnect),
                ),
            ]
            .align_y(Alignment::Center)
            .height(32),
        );
        // Its password, or its identity.
        match saved.security {
            Security::Psk { .. } | Security::Wep => {
                inner = inner.push(
                    row![
                        t(w("Mot de passe", "Password").get(lang), 13.5, TEXT.get())
                            .width(Length::Fill),
                        self.network_password_field(&saved.uuid),
                        container(secondary(
                            w("Changer…", "Change…").get(lang).to_owned(),
                            Some(Message::Network(Msg::Change(saved.uuid.clone()))),
                            self.network_focused(Stop::Change),
                            false,
                        ))
                        .id("network:change".to_owned()),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                );
            }
            Security::Enterprise => {
                inner = inner.push(
                    row![
                        t(w("Identifiant", "Identity").get(lang), 13.5, TEXT.get())
                            .width(Length::Fill),
                        t(saved.identity.clone().unwrap_or_default(), 13.5, DIM.get()),
                    ]
                    .align_y(Alignment::Center)
                    .height(32),
                );
            }
            Security::Open => {}
        }
        // The one in use: its details.
        if saved.connected
            && let Some(current) = self
                .network
                .details
                .as_deref()
                .and_then(|d| d.current.as_ref())
        {
            let mut said: Vec<String> = Vec::new();
            let (fr, en) = (lang == Lang::French, lang == Lang::English);
            if let Some(address) = &current.address {
                said.push(format!(
                    "{} {address}",
                    if fr { "Adresse" } else { "Address" }
                ));
            }
            if let Some(gateway) = &current.gateway {
                said.push(format!(
                    "{} {gateway}",
                    if fr { "Passerelle" } else { "Gateway" }
                ));
            }
            if !current.dns.is_empty() {
                said.push(format!("DNS {}", current.dns.join(", ")));
            }
            if let Some(mac) = &current.mac {
                said.push(format!(
                    "{} {mac}",
                    if en { "MAC address" } else { "Adresse MAC" }
                ));
            }
            let band = current.frequency.map(|f| model::band(f, lang).to_owned());
            let speed = current.speed.map(|s| format!("{s} Mb/s"));
            let radio: Vec<String> = band.into_iter().chain(speed).collect();
            if !radio.is_empty() {
                said.push(radio.join(" · "));
            }
            let mut lines = column![].spacing(2);
            for each in said {
                lines = lines.push(t(each, 12.5, DIM.get()));
            }
            inner = inner.push(container(lines).id("network:details-lines".to_owned()));
        }
        // Oublier ce réseau.
        let forget_focused = self.network_focused(Stop::Forget);
        inner = inner.push(row![
            Space::new().width(Length::Fill),
            container(
                button(
                    container(
                        t(
                            w("Oublier ce réseau", "Forget this network").get(lang),
                            12.5,
                            PEACH.get()
                        )
                        .font(BOLD)
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill)
                )
                .height(28)
                .padding([0, 11])
                .on_press(Message::Network(Msg::Forget(saved.uuid.clone())))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            242,
                            148,
                            91,
                            if hovered { 0.14 } else { 0.08 },
                        )))),
                        text_color: PEACH.get(),
                        border: if forget_focused {
                            ring(7.0)
                        } else {
                            border(7.0, 1.0, crate::theme::ink(rgba(242, 148, 91, 0.40)))
                        },
                        ..button::Style::default()
                    }
                })
            )
            .id("network:forget".to_owned()),
        ]);
        body = body.push(container(inner).padding(Padding {
            top: 2.0,
            right: 14.0,
            bottom: 14.0,
            left: 62.0,
        }));
        let (top, bottom) = (
            if ends.0 { 11.0 } else { 0.0 },
            if ends.1 { 11.0 } else { 0.0 },
        );
        container(body)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.03,
                )))),
                border: iced::Border {
                    radius: iced::border::Radius {
                        top_left: top,
                        top_right: top,
                        bottom_right: bottom,
                        bottom_left: bottom,
                    },
                    ..iced::Border::default()
                },
                ..container::Style::default()
            })
            .into()
    }

    /// A saved network's password: dots, or itself once read with the
    /// eye; a field while it is typed anew.
    fn network_password_field(&self, uuid: &str) -> El<'_> {
        let password = self.network.password.as_ref().filter(|p| p.uuid == uuid);
        let editing = password.is_some_and(|p| p.editing);
        let shown = password.is_some_and(|p| p.shown);
        let inside: El<'_> = match password {
            Some(p) if p.editing => text_input("", &p.text)
                .id(PASSWORD_FIELD)
                .on_input(|s| Message::Network(Msg::Password(s)))
                .on_submit(Message::Network(Msg::SavePassword))
                .secure(!p.shown)
                .size(13.5)
                .font(MANROPE)
                .padding(0)
                .style(style::search)
                .into(),
            Some(p) if p.shown => t(p.text.clone(), 13.5, TEXT.get())
                .wrapping(text::Wrapping::None)
                .into(),
            _ => t("•".repeat(10), 13.5, TEXT.get()).into(),
        };
        let eye = container(
            button(draw(if shown { EYE_SHUT } else { EYE }, 15.0, DIM.get()))
                .width(26)
                .height(26)
                .padding(5.5)
                .on_press(Message::Network(Msg::Eye(uuid.to_owned())))
                .style(flat(
                    None,
                    HOVER.get(),
                    6.0,
                    self.network_focused(Stop::Eye),
                )),
        )
        .id("network:eye".to_owned());
        container(
            row![container(inside).width(Length::Fill).clip(true), eye]
                .spacing(4)
                .align_y(Alignment::Center),
        )
        .width(200)
        .height(34)
        .padding(Padding {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: 10.0,
        })
        .center_y(34)
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.06)),
            9.0,
            Some((
                1.0,
                if editing {
                    crate::theme::ink(rgba(108, 194, 135, 0.65))
                } else {
                    RIM_12.get()
                },
            )),
        ))
        .id("network:password".to_owned())
        .into()
    }

    // --- The sheet ------------------------------------------------------------------

    /// The sheet over the page, under its scrim.
    pub(in crate::app) fn network_sheet(&self) -> Option<El<'_>> {
        let sheet = self.network.sheet.as_ref()?;
        let lang = self.lang;
        let title = match lang {
            Lang::French => format!("Se connecter à « {} »", sheet.name),
            Lang::English => format!("Connect to \u{201c}{}\u{201d}", sheet.name),
        };
        let waiting = sheet.waiting();
        let wrong = sheet.failed == Some(hocus_network::model::Failure::Secret);
        let field_focused = self.network_focused(Stop::SheetField);
        let mut input = text_input("", &sheet.text)
            .id(SHEET_FIELD)
            .secure(!sheet.shown)
            .size(13.5)
            .font(MANROPE)
            .padding(0)
            .style(style::search);
        if !waiting {
            input = input
                .on_input(|s| Message::Network(Msg::SheetInput(s)))
                .on_submit(Message::Network(Msg::SheetConnect));
        }
        let rim = if wrong {
            ORANGE.get()
        } else if field_focused {
            crate::theme::ink(rgba(108, 194, 135, 0.65))
        } else {
            RIM_12.get()
        };
        let field = container(
            row![
                container(input).width(Length::Fill),
                button(draw(
                    if sheet.shown { EYE_SHUT } else { EYE },
                    15.0,
                    DIM.get()
                ))
                .width(26)
                .height(26)
                .padding(5.5)
                .on_press(Message::Network(Msg::SheetEye))
                .style(flat(None, HOVER.get(), 6.0, false)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .height(34)
        .center_y(34)
        .padding(Padding {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: 10.0,
        })
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.06)),
            9.0,
            Some((1.0, rim)),
        ))
        .id("sheet:field".to_owned());
        let mut fields = column![
            t(w("Mot de passe", "Password").get(lang), 12.0, DIM.get()).font(BOLD),
            field
        ]
        .spacing(5);
        match sheet.failed {
            Some(hocus_network::model::Failure::Secret) => {
                fields = fields.push(t(
                    w(
                        "Mot de passe incorrect. Vérifie-le et réessaie.",
                        "Wrong password. Check it and try again.",
                    )
                    .get(lang),
                    12.0,
                    PEACH.get(),
                ));
            }
            Some(hocus_network::model::Failure::Other) => {
                fields = fields.push(t(
                    w(
                        "La connexion a échoué. Réessaie.",
                        "Could not connect. Try again.",
                    )
                    .get(lang),
                    12.0,
                    PEACH.get(),
                ));
            }
            None => {}
        }
        let filled = sheet.filled();
        let connect_focused = self.network_focused(Stop::SheetConnect);
        let connect: El<'_> = if waiting {
            container(
                row![
                    spinner(14.0, DEEP.get(), !self.reduced_motion),
                    t(w("Connexion…", "Connecting…").get(lang), 13.0, DEEP.get()).font(BOLD)
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([0, 14])
            .center_y(32)
            .style(boxed(fade(GREEN.get(), INERT), 8.0, None))
            .into()
        } else {
            let mut b = button(
                container(
                    t(
                        w("Se connecter", "Connect").get(lang),
                        13.0,
                        if filled {
                            DEEP.get()
                        } else {
                            fade(TEXT.get(), INERT)
                        },
                    )
                    .font(BOLD),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .height(32)
            .padding([0, 14])
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if filled {
                    GREEN.get()
                } else {
                    crate::theme::ink(rgba(255, 255, 255, 0.04))
                })),
                text_color: TEXT.get(),
                border: if connect_focused {
                    ring(8.0)
                } else {
                    border(8.0, 1.0, if filled { GREEN.get() } else { RIM_12.get() })
                },
                ..button::Style::default()
            });
            if filled {
                b = b.on_press(Message::Network(Msg::SheetConnect));
            }
            b.into()
        };
        let actions = row![
            Space::new().width(Length::Fill),
            container(secondary(
                w("Annuler", "Cancel").get(lang).to_owned(),
                Some(Message::Network(Msg::SheetCancel)),
                self.network_focused(Stop::SheetCancel),
                false,
            ))
            .id("sheet:cancel".to_owned()),
            container(connect).id("sheet:connect".to_owned()),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let panel = container(
            column![
                row![
                    wifi_glyph(4, 18.0, TEXT.get()),
                    text(title).size(17).font(BOLD).color(TEXT)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                fields,
                container(actions).padding(Padding::default().top(4)),
            ]
            .spacing(14),
        )
        .width(460)
        .padding(Padding {
            top: 20.0,
            right: 22.0,
            bottom: 18.0,
            left: 22.0,
        })
        .style(|_: &Theme| container::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                gradient::Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, crate::theme::ink(rgb(0x1d2333)))
                    .add_stop(1.0, crate::theme::ink(rgb(0x171c29))),
            ))),
            border: border(14.0, 1.0, RIM_12.get()),
            shadow: Shadow {
                color: rgba(0, 0, 0, 0.55),
                offset: Vector::new(0.0, 24.0),
                blur_radius: 60.0,
            },
            ..container::Style::default()
        })
        .id("sheet".to_owned());
        Some(
            mouse_area(
                container(panel)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(rgba(7, 9, 15, 0.62))),
                        ..container::Style::default()
                    }),
            )
            .on_press(Message::Nothing)
            .into(),
        )
    }

    // --- What the page shows, for the log -------------------------------------------

    pub(in crate::app) fn network_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let details = self.network.details.as_deref();
        let around: Vec<serde_json::Value> = self
            .network
            .around()
            .iter()
            .map(|n| {
                let (line, warn) = model::around(n, lang);
                let connecting =
                    details.and_then(|d| d.network.connecting_to()) == Some(n.ssid.as_slice());
                serde_json::json!({
                    "name": n.name,
                    "line": line,
                    "warn": warn,
                    "bars": bars(n.strength),
                    "action": if connecting {
                        "connecting"
                    } else if model::joins_at_once(n) {
                        "connect"
                    } else if n.security.joinable() {
                        "sheet"
                    } else {
                        "none"
                    },
                })
            })
            .collect();
        let now = now();
        let saved: Vec<serde_json::Value> = self
            .network
            .saved()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "uuid": s.uuid,
                    "name": s.name,
                    "line": model::saved_line(s, now, lang),
                    "autoconnect": s.autoconnect,
                    "connected": s.connected,
                })
            })
            .collect();
        let current = details.filter(|_| self.network.wifi_on()).and_then(|d| {
            let wifi = d.network.current.as_ref()?;
            Some(serde_json::json!({
                "name": wifi.name,
                "line": model::connected(wifi, d.current.as_ref(), lang),
                "address": d.current.as_ref().and_then(|c| model::address(c, lang)),
            }))
        });
        let block = match self.network.wifi_block() {
            Block::No => "no",
            Block::Soft => "soft",
            Block::Hard => "hard",
        };
        serde_json::json!({
            "manager": self.network.manager(),
            "radios": self.network.radios.is_some(),
            "airplane": self.network.airplane(),
            "wifi": {
                "on": self.network.wifi_on(),
                "hardware": details.is_some_and(|d| d.network.hardware),
                "block": block,
            },
            "current": current,
            "saved_count": details.map_or(0, |d| d.saved.len()),
            "around": if self.network.wifi_on() { serde_json::json!(around) } else { serde_json::Value::Null },
            "scanning": details.is_some_and(|d| d.network.scanning),
            "sub": self.network.sub,
            "saved": if self.network.sub { serde_json::json!(saved) } else { serde_json::Value::Null },
            "unfolded": self.network.unfolded,
            "password": self.network.password.as_ref().map(|p| serde_json::json!({
                "uuid": p.uuid,
                "shown": p.shown,
                "editing": p.editing,
                "read": !p.text.is_empty(),
            })),
            "sheet": self.network.sheet.as_ref().map(|s| serde_json::json!({
                "name": s.name,
                "filled": s.filled(),
                "waiting": s.waiting(),
                "error": s.failed.map(|f| format!("{f:?}")),
            })),
            "card": self.network_card(),
        })
    }
}

const WIFI_LABEL: crate::lang::Words = w("WI-FI", "WI-FI");
