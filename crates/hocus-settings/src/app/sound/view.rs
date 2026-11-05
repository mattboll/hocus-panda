//! What the Son page draws (the approved mockup, variant A): SORTIE and
//! ENTRÉE, each its devices' card (one row each, the one in use ticked)
//! and the chosen device's card; then APPLICATIONS.

use crate::theme::Ink;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_sound::{Kind, Sink, tester::Side};
use iced::{
    Alignment, Background, Color, Gradient, Length, Padding, Shadow, Theme, gradient,
    widget::{Space, button, column, container, pin, row, slider, stack, svg, text},
};

use super::{super::Focus, AppRow, Input, MenuId, Msg, Pick, SEGMENTS, Stop, Which};
use crate::{
    app::{
        App, Control, Message,
        style::{self, *},
        view::{El, card_box, divided, ease_out, label, t},
    },
    lang::{Lang, w},
    sound as model,
};

/// The page's colours (bar.md §13's sound gauge, the mockup's).
const GAUGE_FROM: Ink = Ink::Dark(rgb(0x5B8DEF));
const AMBER: Ink = Ink::Dark(rgb(0xF5B44A));
/// The microphone's, as the in-use sign's (5.12).
const MIC: Ink = Ink::Dark(rgb(0xF5A35B));
const CHECK: Ink = Ink::Accent(1.0);
const MUTED_FILL: Ink = Ink::Dark(rgba(169, 178, 194, 0.45));

/// The page's glyphs: the mockup's 24 px stroke paths.
const SPEAKER: &str = "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9 A4 4 0 0 1 15.5 15 M18 6.5 A7.5 7.5 0 0 1 18 17.5";
const SPEAKER_MUTED: &str =
    "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9.5 L20.5 14.5 M20.5 9.5 L15.5 14.5";
const HEADPHONES: &str = "M4.5 15 V12.5 A7.5 7.5 0 0 1 19.5 12.5 V15 M4.5 14 H7.5 V19.5 H5.5 A1 1 0 0 1 4.5 18.5 Z M19.5 14 H16.5 V19.5 H18.5 A1 1 0 0 0 19.5 18.5 Z";
const SCREEN: &str = "M3 4.5 H21 V16 H3 Z M8 20 H16 M12 16 V20";
const MICROPHONE: &str = "M12 3.5 A3 3 0 0 0 9 6.5 V11.5 A3 3 0 0 0 15 11.5 V6.5 A3 3 0 0 0 12 3.5 Z M5.5 11 A6.5 6.5 0 0 0 18.5 11 M12 17.5 V20.5";
const MICROPHONE_MUTED: &str = "M12 3.5 A3 3 0 0 0 9 6.5 V11.5 A3 3 0 0 0 15 11.5 V6.5 A3 3 0 0 0 12 3.5 Z M5.5 11 A6.5 6.5 0 0 0 18.5 11 M12 17.5 V20.5 M4 4 L20 20";
const HEADSET: &str = "M4.5 13.5 V12 A7.5 7.5 0 0 1 19.5 12 V13.5 M4.5 13 H7.5 V18 H4.5 Z M19.5 13 H16.5 V18 H19.5 Z M6 18 C6 20 7.5 20.5 10 20.5 H12";
const CHEVRON: &str = "M6 9 L12 15 L18 9";
const TICK: &str = "M5 12.5 L10 17.5 L19 7.5";

/// The letters' tiles' colours, by the application's name: the bar's
/// (3.4b), so that one keeps its colour from the popover to the page.
const TILES: [u32; 6] = [0xF5B34A, 0x8FB3FF, 0x7EE0A0, 0xF7B98A, 0xC9A7F5, 0x6FD3D8];

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

fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Speaker => SPEAKER,
        Kind::Headphones => HEADPHONES,
        Kind::Screen => SCREEN,
    }
}

/// The 32 px tile of a device: blue for the one in use.
fn tile<'a>(path: &'static str, on: bool) -> El<'a> {
    let (fill, ink) = if on {
        (crate::theme::ink(rgba(143, 179, 255, 0.16)), BLUE)
    } else {
        (crate::theme::ink(rgba(255, 255, 255, 0.06)), DIM)
    };
    container(draw(path, 18.0, ink.get(), 1.8))
        .width(32)
        .height(32)
        .center(32)
        .style(boxed(fill, 9.0, None))
        .into()
}

/// A row of the page: its name and line, its control on the right; 54 px
/// at least.
fn plain_row<'a>(
    name: String,
    line: Option<El<'a>>,
    control: El<'a>,
    id: Option<&str>,
    lit: f32,
) -> El<'a> {
    let mut words = column![text(name).size(14).font(SEMIBOLD).color(TEXT)]
        .spacing(2)
        .width(Length::Fill);
    if let Some(line) = line {
        words = words.push(line);
    }
    let body = container(
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
    .style(lit_style(lit));
    match id {
        Some(id) => body.id(format!("row:{id}")).into(),
        None => body.into(),
    }
}

/// A row lit after a search, as 6.1's.
fn lit_style(lit: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: (lit > 0.0).then_some(Background::Color(
            crate::theme::Ink::Accent(0.20 * lit).get(),
        )),
        border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
        ..container::Style::default()
    }
}

/// A line under a name, in `text.dim`, its end in `MIC` when it says a
/// recording.
fn sub_line<'a>(said: String, recording: Option<String>) -> El<'a> {
    let mut line = row![t(said, 12.5, DIM.get())];
    if let Some(recording) = recording {
        line = line.push(t(recording, 12.5, MIC.get()));
    }
    line.into()
}

impl App {
    /// The page's groups, under its title.
    pub(in crate::app) fn sound_groups(&self) -> El<'_> {
        let lang = self.lang;
        if self.sound.graph.is_none() {
            return card_box(
                container(
                    column![
                        t(
                            w("PipeWire ne répond pas", "PipeWire does not answer").get(lang),
                            14.0,
                            TEXT.get()
                        )
                        .font(SEMIBOLD),
                        t(
                            w(
                                "Le son reviendra ici dès que PipeWire tournera.",
                                "The sound shows here as soon as PipeWire runs.",
                            )
                            .get(lang),
                            12.5,
                            DIM.get(),
                        ),
                    ]
                    .spacing(4),
                )
                .padding([14, 16])
                .into(),
            );
        }
        let controls = self.controls();
        let focused = |stop: Stop| matches!(self.focus, Focus::Control(i) if controls.get(i) == Some(&Control::Sound(stop)));
        let mut page = column![].spacing(18);
        // SORTIE.
        let mut output = column![group_head(w("SORTIE", "OUTPUT").get(lang), None)].spacing(8);
        output = output.push(self.output_devices(focused(Stop::Outputs)));
        if let Some(sink) = self.sound.output() {
            output = output.push(self.output_card(sink, &focused));
        }
        page = page.push(container(output).id("group:sound.output".to_owned()));
        // ENTRÉE.
        let mut input = column![group_head(w("ENTRÉE", "INPUT").get(lang), None)].spacing(8);
        input = input.push(self.input_devices(focused(Stop::Inputs)));
        if let Some(source) = self.sound.input() {
            input = input.push(self.input_card(source, &focused));
        }
        input = input.push(note(
            w(
                "La jauge écoute le micro tant que la page est ouverte ; la barre le montre, comme toute écoute.",
                "The meter listens to the microphone while the page is open; the bar shows it, as for any listening.",
            )
            .get(lang),
        ));
        page = page.push(container(input).id("group:sound.input".to_owned()));
        // APPLICATIONS.
        let apps = self.sound.apps();
        let count = (!apps.is_empty()).then(|| match lang {
            Lang::French => format!("{} en cours", apps.len()),
            Lang::English => format!("{} active", apps.len()),
        });
        let mut group = column![group_head(
            w("APPLICATIONS", "APPLICATIONS").get(lang),
            count
        )]
        .spacing(8);
        group = group.push(
            container(card_box(self.app_rows(&apps, &focused)))
                .style(lit_style(self.lit("sound.apps")))
                .id("row:sound.apps".to_owned()),
        );
        group = group.push(note(
            w(
                "Celles qui jouent ou enregistrent maintenant. PipeWire se souvient du volume et de la sortie de chacune.",
                "Those playing or recording now. PipeWire remembers each one's volume and output.",
            )
            .get(lang),
        ));
        page = page.push(container(group).id("group:sound.apps".to_owned()));
        page.into()
    }

    /// The outputs' card: one row each, the one in use ticked.
    fn output_devices(&self, focused: bool) -> El<'_> {
        let lang = self.lang;
        let graph = self.sound.graph.as_deref();
        let mut rows = column![];
        let outputs = self.sound.outputs();
        if outputs.is_empty() {
            rows = rows.push(plain_row(
                w("Aucune sortie", "No output").get(lang).to_owned(),
                None,
                Space::new().into(),
                None,
                0.0,
            ));
        }
        for (i, sink) in outputs.iter().enumerate() {
            let line = graph
                .map(|g| model::line(sink, g, false, lang))
                .unwrap_or_default();
            let on = sink.default;
            let row = device_row(
                kind_glyph(sink.kind),
                sink.description.clone(),
                sub_line(line, None),
                model::say_volume(sink, false, lang),
                on,
                focused && self.sound.cursor.0 == i,
                Message::Sound(Msg::Output(sink.name.clone())),
                format!("out:{}", sink.name),
                if on { self.lit("sound.output") } else { 0.0 },
                on.then_some("sound.output"),
            );
            rows = rows.push(divided(i > 0, row));
        }
        card_box(rows.into())
    }

    /// The inputs' card, the same.
    fn input_devices(&self, focused: bool) -> El<'_> {
        let lang = self.lang;
        let graph = self.sound.graph.as_deref();
        let inputs = self.sound.inputs(lang);
        let mut rows = column![];
        if inputs.is_empty() {
            rows = rows.push(plain_row(
                w("Aucune entrée", "No input").get(lang).to_owned(),
                None,
                Space::new().into(),
                None,
                0.0,
            ));
        }
        let users: Vec<String> = graph
            .map(|g| {
                let input = g.microphone().map(|s| s.id);
                g.recorders
                    .iter()
                    .filter(|r| !r.ours && input.is_some_and(|i| r.sources.contains(&i)))
                    .map(|r| r.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        for (i, Input { pick, sink, name }) in inputs.into_iter().enumerate() {
            let on = sink.as_ref().is_some_and(|s| s.default);
            let (line, volume, path, id) = match (&pick, &sink) {
                (Pick::Source(source), Some(sink)) => (
                    graph
                        .map(|g| model::line(sink, g, true, lang))
                        .unwrap_or_default(),
                    model::say_volume(sink, true, lang),
                    if sink.bus.as_deref() == Some("bluetooth") {
                        HEADSET
                    } else {
                        MICROPHONE
                    },
                    format!("in:{source}"),
                ),
                (Pick::Headset { card, .. }, _) => {
                    let device = graph
                        .and_then(|g| g.card(*card))
                        .map(|c| c.description.clone())
                        .unwrap_or_default();
                    let line = match lang {
                        Lang::French => format!("{device} · le passe en mode appel (HFP)"),
                        Lang::English => format!("{device} · switches it to call mode (HFP)"),
                    };
                    (line, String::new(), HEADSET, format!("in:card:{card}"))
                }
                _ => (String::new(), String::new(), MICROPHONE, format!("in:{i}")),
            };
            let using = (on && !users.is_empty()).then(|| match lang {
                Lang::French => format!(" · {} l’utilise", users.join(", ")),
                Lang::English => format!(" · {} is using it", users.join(", ")),
            });
            let row = device_row(
                path,
                name,
                sub_line(line, using),
                volume,
                on,
                focused && self.sound.cursor.1 == i,
                Message::Sound(Msg::Input(pick)),
                id,
                if on { self.lit("sound.input") } else { 0.0 },
                on.then_some("sound.input"),
            );
            rows = rows.push(divided(i > 0, row));
        }
        card_box(rows.into())
    }

    /// The output in use: its volume, its profile, its balance and its
    /// test.
    fn output_card<'a>(&'a self, sink: &Sink, focused: &dyn Fn(Stop) -> bool) -> El<'a> {
        let lang = self.lang;
        let mut rows = column![caption(kind_glyph(sink.kind), sink.description.clone())];
        rows = rows.push(plain_row(
            w("Volume", "Volume").get(lang).to_owned(),
            None,
            self.volume_control(
                Which::Output,
                sink.sound,
                false,
                220.0,
                focused(Stop::OutMute),
                focused(Stop::OutVolume),
            ),
            Some("sound.output-volume"),
            self.lit("sound.output-volume"),
        ));
        // The profile: a menu when the device has more than one.
        let graph = self.sound.graph.as_deref();
        let card = sink.device.and_then(|d| graph.and_then(|g| g.card(d)));
        if let Some(card) = card {
            let profiles = self.sound_profiles();
            let current = card
                .profile
                .and_then(|p| card.profiles.iter().find(|q| q.index == p));
            let name = current.map(|p| p.description.clone()).unwrap_or_default();
            let only = profiles.len() <= 1;
            let line = current
                .and_then(|p| model::profile_line(&p.name, only))
                .map(|l| t(l.get(lang), 12.5, DIM.get()).into());
            let control: El<'_> = if only {
                t(name, 13.5, DIM.get()).into()
            } else {
                self.menu_button(
                    name,
                    200.0,
                    MenuId::Profile,
                    focused(Stop::Profile),
                    "menu:sound.profile".to_owned(),
                )
            };
            rows = rows.push(divided(
                true,
                plain_row(
                    w("Profil", "Profile").get(lang).to_owned(),
                    line,
                    control,
                    Some("sound.profile"),
                    self.lit("sound.profile"),
                ),
            ));
        }
        // The balance and the test.
        let mono = sink.mono();
        let line = if mono {
            w(
                "Cette sortie est mono ; « Tester » joue au milieu.",
                "This output is mono; “Test” plays in the middle.",
            )
        } else {
            w(
                "« Tester » joue un son à gauche, puis à droite.",
                "“Test” plays a sound on the left, then on the right.",
            )
        };
        let control = row![
            self.balance_control(mono, focused(Stop::Balance)),
            self.test_button(focused(Stop::Test)),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        let lit = self.lit("sound.balance").max(self.lit("sound.test"));
        rows = rows.push(divided(
            true,
            plain_row(
                w("Balance", "Balance").get(lang).to_owned(),
                Some(t(line.get(lang), 12.5, DIM.get()).into()),
                control.into(),
                Some("sound.balance"),
                lit,
            ),
        ));
        card_box(rows.into())
    }

    /// The input in use: its volume and its level.
    fn input_card<'a>(&'a self, source: &Sink, focused: &dyn Fn(Stop) -> bool) -> El<'a> {
        let lang = self.lang;
        let path = if source.bus.as_deref() == Some("bluetooth") {
            HEADSET
        } else {
            MICROPHONE
        };
        let mut rows = column![caption(path, source.description.clone())];
        rows = rows.push(plain_row(
            w("Volume", "Volume").get(lang).to_owned(),
            None,
            self.volume_control(
                Which::Input,
                source.sound,
                true,
                220.0,
                focused(Stop::InMute),
                focused(Stop::InVolume),
            ),
            Some("sound.input-volume"),
            self.lit("sound.input-volume"),
        ));
        rows = rows.push(divided(
            true,
            plain_row(
                w("Niveau", "Level").get(lang).to_owned(),
                Some(
                    t(
                        w(
                            "Parle : la jauge suit ta voix.",
                            "Speak: the meter follows your voice.",
                        )
                        .get(lang),
                        12.5,
                        DIM.get(),
                    )
                    .into(),
                ),
                self.meter(source),
                Some("sound.meter"),
                self.lit("sound.meter"),
            ),
        ));
        card_box(rows.into())
    }

    /// The meter: 24 segments of 7 px, 2 px apart, lit from the left,
    /// the top tenth amber; "coupé" while the microphone is muted.
    fn meter(&self, source: &Sink) -> El<'_> {
        let muted = source.sound.is_some_and(|s| s.muted);
        let lit = self.sound.lit();
        let mut bar = row![].spacing(2);
        for i in 0..SEGMENTS {
            let at = i as f32 / (SEGMENTS - 1) as f32;
            let color = if i >= SEGMENTS * 9 / 10 {
                AMBER.get()
            } else {
                mix(GAUGE_FROM.get(), BLUE.get(), (at / 0.72).min(1.0))
            };
            let fill = if i < lit {
                color
            } else {
                crate::theme::ink(rgba(255, 255, 255, 0.12))
            };
            bar =
                bar.push(container(Space::new().width(7).height(10)).style(boxed(fill, 1.0, None)));
        }
        let said = if muted {
            w("coupé", "off").get(self.lang)
        } else {
            ""
        };
        row![
            container(bar).width(220).id("meter".to_owned()),
            container(text(said).size(12.5).font(MONO).color(FAINT))
                .width(44)
                .align_right(44),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    /// A mute button, a slider and its value: an output's, an input's or
    /// an application's.
    fn volume_control(
        &self,
        which: Which,
        sound: Option<hocus_sound::Sound>,
        input: bool,
        width: f32,
        mute_focused: bool,
        slider_focused: bool,
    ) -> El<'_> {
        let lang = self.lang;
        let muted = sound.is_some_and(|s| s.muted);
        let volume = self.sound.volume(&which, sound).unwrap_or(0.0);
        let path = match (input, muted) {
            (false, false) => SPEAKER,
            (false, true) => SPEAKER_MUTED,
            (true, false) => MICROPHONE,
            (true, true) => MICROPHONE_MUTED,
        };
        let id = match &which {
            Which::Output => "output".to_owned(),
            Which::Input => "input".to_owned(),
            Which::App(key) => format!("app:{key}"),
        };
        let mute = container(
            button(draw(
                path,
                16.0,
                if muted { PEACH.get() } else { DIM.get() },
                1.9,
            ))
            .width(32)
            .height(32)
            .padding(8)
            .on_press(Message::Sound(Msg::Mute(which.clone())))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let fill = match (muted, hovered) {
                    (true, _) => crate::theme::ink(rgba(229, 121, 63, 0.18)),
                    (false, true) => crate::theme::ink(rgba(255, 255, 255, 0.10)),
                    (false, false) => crate::theme::ink(rgba(255, 255, 255, 0.06)),
                };
                button::Style {
                    background: Some(Background::Color(fill)),
                    text_color: TEXT.get(),
                    border: if mute_focused {
                        ring(8.0)
                    } else {
                        border(8.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }
            }),
        )
        .id(format!("mute:{id}"));
        let on_change = which.clone();
        let bar = slider(0.0..=100.0, volume.clamp(0.0, 100.0), move |v: f32| {
            Message::Sound(Msg::Volume(on_change.clone(), v))
        })
        .step(1.0_f32)
        .width(width)
        .height(16)
        .style(move |_, _| slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    if muted {
                        Background::Color(MUTED_FILL.get())
                    } else {
                        Background::Gradient(Gradient::Linear(
                            gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                                .add_stop(0.0, GAUGE_FROM.get())
                                .add_stop(1.0, BLUE.get()),
                        ))
                    },
                    Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.12))),
                ),
                width: 6.0,
                border: border(3.0, 0.0, Color::TRANSPARENT),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 8.0 },
                background: Background::Color(if muted {
                    crate::theme::ink(rgb(0xC9D0DC))
                } else {
                    Color::WHITE
                }),
                border_width: if slider_focused { 3.0 } else { 0.0 },
                border_color: ACCENT.get(),
            },
        });
        let said = if muted {
            if input {
                w("coupé", "off")
            } else {
                w("muet", "muted")
            }
            .get(lang)
            .to_owned()
        } else {
            format!("{} %", volume.round() as i32)
        };
        row![
            mute,
            container(bar).id(format!("slider:{id}")),
            container(
                text(said)
                    .size(12.5)
                    .font(MONO)
                    .color(if muted { FAINT } else { TEXT })
            )
            .width(44)
            .align_right(44),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    /// The balance: "G" … "D" around a 160 px slider by tens, its fill from
    /// the centre to the knob.
    fn balance_control(&self, mono: bool, focused: bool) -> El<'_> {
        let lang = self.lang;
        let alpha = if mono { INERT } else { 1.0 };
        let balance = self.sound.balance();
        let centre = 80.0;
        let knob = (balance + 100) as f32 / 200.0 * 144.0 + 8.0;
        let (from, width) = (knob.min(centre), (knob - centre).abs());
        let track = container(stack![
            container(Space::new().width(160).height(6)).style(boxed(
                fade(crate::theme::ink(rgba(255, 255, 255, 0.12)), alpha),
                3.0,
                None
            )),
            container(container(Space::new().width(width).height(6)).style(boxed(
                fade(BLUE.get(), alpha),
                3.0,
                None
            )))
            .padding(Padding::default().left(from)),
            container(container(Space::new().width(2).height(14)).style(boxed(
                fade(crate::theme::ink(rgba(255, 255, 255, 0.28)), alpha),
                1.0,
                None
            )))
            .padding(Padding::default().left(centre - 1.0).top(-4.0)),
        ])
        .height(24)
        .center_y(24);
        let bar = slider(-100..=100, balance, move |b: i32| {
            if mono {
                Message::Nothing
            } else {
                Message::Sound(Msg::Balance(b))
            }
        })
        .step(10)
        .width(160)
        .height(24)
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
                shape: slider::HandleShape::Circle { radius: 8.0 },
                background: Background::Color(fade(Color::WHITE, alpha)),
                border_width: if focused { 3.0 } else { 0.0 },
                border_color: ACCENT.get(),
            },
        });
        let letter = |l: &'static str| {
            text(l)
                .size(11.5)
                .font(BOLD)
                .color(fade(FAINT.get(), alpha))
        };
        let (left, right) = match lang {
            Lang::French => ("G", "D"),
            Lang::English => ("L", "R"),
        };
        row![
            letter(left),
            container(stack![track, bar])
                .width(160)
                .id("slider:balance".to_owned()),
            letter(right),
            container(
                text(model::say_balance(balance, lang))
                    .size(12.5)
                    .font(MONO)
                    .color(fade(TEXT.get(), alpha))
            )
            .width(54)
            .align_right(54),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    /// "Tester": two dots lit in turn, its words saying the side.
    fn test_button(&self, focused: bool) -> El<'_> {
        let lang = self.lang;
        let testing = self.sound.testing;
        let words = match testing {
            Some(Side::Left) => w("À gauche…", "On the left…"),
            Some(Side::Right) => w("À droite…", "On the right…"),
            Some(Side::Middle) => w("Au milieu…", "In the middle…"),
            None => w("Tester", "Test"),
        };
        let dot = |on: bool| {
            container(Space::new().width(7).height(7)).style(boxed(
                if on {
                    BLUE.get()
                } else {
                    crate::theme::ink(rgba(255, 255, 255, 0.20))
                },
                3.5,
                None,
            ))
        };
        let left = matches!(testing, Some(Side::Left | Side::Middle));
        let right = matches!(testing, Some(Side::Right | Side::Middle));
        container(
            button(
                row![
                    row![dot(left), dot(right)].spacing(3),
                    text(words.get(lang)).size(13.5).font(SEMIBOLD).color(TEXT)
                ]
                .spacing(9)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(32)
            .width(Length::Shrink)
            .padding(Padding {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 10.0,
            })
            .on_press(Message::Sound(Msg::Test))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if hovered { 0.10 } else { 0.06 },
                    )))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(8.0)
                    } else {
                        border(8.0, 1.0, RIM_12.get())
                    },
                    ..button::Style::default()
                }
            }),
        )
        .width(Length::Shrink)
        .id("row:sound.test".to_owned())
        .into()
    }

    /// A menu's button: its value and its chevron.
    fn menu_button(
        &self,
        said: String,
        least: f32,
        id: MenuId,
        focused: bool,
        at: String,
    ) -> El<'_> {
        let width = (style::measure(&said, 13.5, SEMIBOLD).ceil() + 12.0 + 10.0 + 22.0 + 8.0)
            .clamp(least, 260.0);
        let open = self.sound.menu.as_ref().is_some_and(|(m, _)| *m == id);
        container(
            button(
                row![
                    text(said)
                        .size(13.5)
                        .font(SEMIBOLD)
                        .color(TEXT)
                        .wrapping(text::Wrapping::None),
                    Space::new().width(Length::Fill),
                    draw(CHEVRON, 12.0, DIM.get(), 2.4),
                ]
                .spacing(10)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(32)
            .width(width)
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 12.0,
            })
            .on_press(Message::Sound(Msg::Menu(id)))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if hovered || open { 0.09 } else { 0.06 },
                    )))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(8.0)
                    } else {
                        border(8.0, 1.0, RIM_12.get())
                    },
                    ..button::Style::default()
                }
            }),
        )
        .id(at)
        .into()
    }

    /// The applications' rows, or the line that none plays.
    fn app_rows(&self, apps: &[AppRow], focused: &dyn Fn(Stop) -> bool) -> El<'_> {
        let lang = self.lang;
        if apps.is_empty() {
            return container(
                column![
                    text(
                        w(
                            "Aucune application ne joue de son.",
                            "No application is playing sound."
                        )
                        .get(lang)
                    )
                    .size(14)
                    .font(SEMIBOLD)
                    .color(DIM),
                    t(
                        w(
                            "Elles apparaissent ici dès qu’elles jouent ou enregistrent.",
                            "They show here as soon as they play or record.",
                        )
                        .get(lang),
                        12.5,
                        DIM.get(),
                    ),
                ]
                .spacing(2),
            )
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .height(Length::Shrink)
            .id("apps:none".to_owned())
            .into();
        }
        let graph = self.sound.graph.as_deref();
        let mut rows = column![];
        for (i, app) in apps.iter().enumerate() {
            let on = app
                .sink
                .and_then(|id| graph.and_then(|g| g.node(id)))
                .or_else(|| self.sound.output());
            let mut line = String::new();
            if app.plays {
                line = match (lang, on) {
                    (Lang::French, Some(s)) => format!("Joue sur {}", s.description),
                    (Lang::English, Some(s)) => format!("Playing on {}", s.description),
                    (Lang::French, None) => "Joue".to_owned(),
                    (Lang::English, None) => "Playing".to_owned(),
                };
            }
            let records = app
                .records
                .and_then(|id| graph.and_then(|g| g.node(id)))
                .map(|s| match (lang, app.plays) {
                    (Lang::French, true) => format!(" · enregistre {}", s.description),
                    (Lang::English, true) => format!(" · recording {}", s.description),
                    (Lang::French, false) => format!("Enregistre {}", s.description),
                    (Lang::English, false) => format!("Recording {}", s.description),
                });
            let muted = app.sound.is_some_and(|s| s.muted);
            let tile = app_tile(&app.name, muted);
            let words = column![
                text(app.name.clone()).size(14).font(SEMIBOLD).color(TEXT),
                sub_line(line, records),
            ]
            .spacing(2)
            .width(Length::Fill);
            let mut controls = row![].spacing(12).align_y(Alignment::Center);
            if app.plays {
                let chosen = self.sound_target_name(app);
                let said = match chosen
                    .and_then(|name| graph.and_then(|g| g.sinks.iter().find(|s| s.name == name)))
                {
                    Some(sink) => sink.description.clone(),
                    None => w("Suit la sortie", "Follows the output")
                        .get(lang)
                        .to_owned(),
                };
                controls = controls.push(self.menu_button(
                    said,
                    150.0,
                    MenuId::App(app.key.clone()),
                    focused(Stop::AppMenu(i)),
                    format!("menu:app:{}", app.key),
                ));
            }
            controls = controls.push(self.volume_control(
                Which::App(app.key.clone()),
                app.sound,
                !app.plays,
                140.0,
                focused(Stop::AppMute(i)),
                focused(Stop::AppVolume(i)),
            ));
            let body = container(
                row![
                    row![tile, words]
                        .spacing(14)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    row![Space::new().height(36), controls].align_y(Alignment::Center),
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
            .id(format!("app:{}", app.key));
            rows = rows.push(divided(i > 0, body.into()));
        }
        rows.into()
    }

    /// The open menu's list, 38 px under its button's top, its right on
    /// the button's.
    pub(in crate::app) fn sound_menu_list(&self) -> Option<El<'_>> {
        let (id, since) = self.sound.menu.as_ref()?;
        let at_id = match id {
            MenuId::Profile => "menu:sound.profile".to_owned(),
            MenuId::App(key) => format!("menu:app:{key}"),
        };
        let at = *self.places.at.get(&at_id)?;
        let opened = if self.reduced_motion {
            1.0
        } else {
            ease_out(
                self.now.saturating_duration_since(*since),
                super::super::MENU_IN,
            )
        };
        let options = self.sound_menu_options(id);
        let height = options.len() as f32 * 32.0 + 10.0;
        let mut list = column![];
        let mut widest = at.width;
        for (i, (label, sub, on, msg)) in options.into_iter().enumerate() {
            let check: El<'_> = if on {
                draw(TICK, 13.0, fade(CHECK.get(), opened), 3.0)
            } else {
                Space::new().width(13).into()
            };
            let mut line = row![
                container(check).width(18).center_x(18),
                text(label.clone())
                    .size(13.5)
                    .font(if on { BOLD } else { MANROPE })
                    .color(fade(TEXT.get(), opened))
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .height(Length::Fill)
            .align_y(Alignment::Center);
            let mut width = style::measure(&label, 13.5, BOLD) + 18.0 + 6.0 + 18.0 + 10.0;
            if let Some(sub) = sub {
                width += style::measure(&sub, 12.5, MANROPE) + 12.0;
                line = line.push(Space::new().width(Length::Fill));
                line = line.push(
                    text(sub)
                        .size(12.5)
                        .color(fade(FAINT.get(), opened))
                        .wrapping(text::Wrapping::None),
                );
            }
            widest = widest.max(width);
            let keyboard = i == self.sound.menu_index;
            list = list.push(
                container(
                    button(line)
                        .width(Length::Fill)
                        .height(32)
                        .padding(Padding {
                            top: 0.0,
                            right: 12.0,
                            bottom: 0.0,
                            left: 6.0,
                        })
                        .on_press(Message::Sound(msg))
                        .style(flat(
                            keyboard.then_some(crate::theme::ink(rgba(255, 255, 255, 0.07))),
                            crate::theme::ink(rgba(255, 255, 255, 0.07)),
                            7.0,
                            false,
                        )),
                )
                .id(format!("option:{i}")),
            );
        }
        let width = widest + 10.0;
        let panel = container(list)
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..style::shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let x = (at.x + at.width - width).max(0.0);
        // Under its button, or over it when the window ends first (an
        // application's, at the page's bottom).
        let bottom = self
            .places
            .page
            .map_or(f32::MAX, |p| p.viewport.y + p.viewport.height);
        let y = if at.y + 38.0 + height > bottom - 8.0 {
            at.y - height - 6.0 + 4.0 * (1.0 - opened)
        } else {
            at.y + 38.0 - 4.0 * (1.0 - opened)
        };
        Some(pin(panel).x(x).y(y.max(0.0)).into())
    }

    /// What the page shows, for the log.
    pub(in crate::app) fn sound_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let listening = self.sound.listening();
        let Some(graph) = self.sound.graph.as_deref() else {
            return serde_json::json!({ "pipewire": false, "listening": listening });
        };
        let device = |s: &Sink, input: bool| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "line": model::line(s, graph, input, lang),
                "value": model::say_volume(s, input, lang),
                "default": s.default,
            })
        };
        let inputs: Vec<serde_json::Value> = self
            .sound
            .inputs(lang)
            .into_iter()
            .map(|i| match i.sink {
                Some(s) => device(&s, true),
                None => serde_json::json!({ "name": i.name, "headset": true }),
            })
            .collect();
        let apps: Vec<serde_json::Value> = self
            .sound
            .apps()
            .into_iter()
            .map(|a| {
                let volume = self.sound.volume(&Which::App(a.key.clone()), a.sound);
                serde_json::json!({
                    "key": a.key,
                    "name": a.name,
                    "volume": volume.map(|v| v.round() as i32),
                    "muted": a.sound.is_some_and(|s| s.muted),
                    "follows": self.sound_target_name(&a).is_none(),
                    "target": self.sound_target_name(&a),
                    "plays": a.plays,
                    "records": a.records.is_some(),
                })
            })
            .collect();
        let output = graph.output().map(|s| {
            serde_json::json!({
                "name": s.name,
                "volume": self.sound.volume(&Which::Output, s.sound).map(|v| v.round() as i32),
                "muted": s.sound.is_some_and(|s| s.muted),
                "balance": self.sound.balance(),
                "mono": s.mono(),
                "profiles": self.sound_profiles().iter().map(|p| p.description.clone()).collect::<Vec<_>>(),
            })
        });
        let input = graph.microphone().map(|s| {
            serde_json::json!({
                "name": s.name,
                "volume": self.sound.volume(&Which::Input, s.sound).map(|v| v.round() as i32),
                "muted": s.sound.is_some_and(|s| s.muted),
            })
        });
        serde_json::json!({
            "pipewire": true,
            "outputs": graph.sinks.iter().map(|s| device(s, false)).collect::<Vec<_>>(),
            "inputs": inputs,
            "output": output,
            "input": input,
            "apps": apps,
            "listening": listening,
            "level": self.sound.lit(),
            "testing": self.sound.testing.map(|s| format!("{s:?}")),
            "menu": self.sound.menu.as_ref().map(|(m, _)| format!("{m:?}")),
        })
    }
}

/// A group's label, and what it says on its right.
fn group_head<'a>(words: &'static str, right: Option<String>) -> El<'a> {
    let mut head = row![label(words), Space::new().width(Length::Fill)]
        .align_y(Alignment::Center)
        .height(28);
    if let Some(right) = right {
        head = head.push(t(right, 12.5, FAINT.get()));
    }
    head.into()
}

/// The note under a group.
fn note<'a>(words: &'static str) -> El<'a> {
    container(t(words, 12.5, FAINT.get()).line_height(1.5))
        .padding([0, 4])
        .into()
}

/// The chosen device's caption: its icon, its name, a divider under it.
fn caption<'a>(path: &'static str, name: String) -> El<'a> {
    column![
        container(
            row![
                draw(path, 15.0, BLUE.get(), 1.9),
                text(name).size(12.5).font(BOLD).color(DIM)
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        )
        .height(34)
        .center_y(34)
        .padding([0, 16]),
        container(Space::new().width(Length::Fill).height(1)).style(boxed(LINE.get(), 0.0, None)),
    ]
    .into()
}

/// A device's row: one button, its tile, its name and line, its own
/// volume, the tick on the one in use.
#[allow(clippy::too_many_arguments)]
fn device_row<'a>(
    path: &'static str,
    name: String,
    line: El<'a>,
    volume: String,
    on: bool,
    focused: bool,
    press: Message,
    id: String,
    lit: f32,
    row_id: Option<&str>,
) -> El<'a> {
    let tick: El<'a> = if on {
        draw(TICK, 16.0, CHECK.get(), 3.0)
    } else {
        Space::new().width(16).into()
    };
    let inner = button(
        row![
            tile(path, on),
            column![
                text(name)
                    .size(14)
                    .font(if on { BOLD } else { SEMIBOLD })
                    .color(TEXT),
                line
            ]
            .spacing(2)
            .width(Length::Fill),
            text(volume).size(12).font(MONO).color(FAINT),
            container(tick).width(22).center_x(22),
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: 9.0,
        right: 12.0,
        bottom: 9.0,
        left: 16.0,
    })
    .on_press(press)
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (hovered && !on).then_some(Background::Color(HOVER.get())),
            text_color: TEXT.get(),
            border: if focused {
                ring(11.0)
            } else {
                border(11.0, 0.0, Color::TRANSPARENT)
            },
            ..button::Style::default()
        }
    });
    let inner = container(container(inner).height(Length::Shrink).id(id)).style(lit_style(lit));
    match row_id {
        Some(row_id) => inner.id(format!("row:{row_id}")).into(),
        None => inner.into(),
    }
}

/// An application's tile: its letter on its colour.
fn app_tile<'a>(name: &str, muted: bool) -> El<'a> {
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let sum = name.bytes().fold(0_usize, |sum, b| {
        sum.wrapping_mul(31).wrapping_add(usize::from(b))
    });
    let color = rgb(TILES[sum % TILES.len()]);
    let alpha = if muted { INERT } else { 1.0 };
    container(
        text(letter)
            .size(14)
            .font(BOLD)
            .color(fade(DEEP.get(), alpha)),
    )
    .width(32)
    .height(32)
    .center(32)
    .style(boxed(fade(color, alpha), 9.0, None))
    .into()
}

/// `a` to `b` by `by`.
fn mix(a: Color, b: Color, by: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * by,
        g: a.g + (b.g - a.g) * by,
        b: a.b + (b.b - a.b) * by,
        a: a.a + (b.a - a.a) * by,
    }
}
