//! What the Date et heure page draws (the approved mockup, variant A):
//! the clock's card, the automatic time and the time set by hand, the
//! zone and its picker, the format's link, what comes later.

use hocus_words::settings::app_time_view as words;
use iced::{
    Alignment, Background, Color, Length, Padding, Shadow, Theme, Vector,
    widget::{
        Column, Space, button, column, container, mouse_area, rich_text, row, span, stack, text,
        text_input,
    },
};

use super::{DATE_FIELD, Msg, Stop, TIME_FIELD, ZONE_SEARCH};
use crate::{
    app::{
        App, Message,
        style::*,
        view::{El, card_box, divided, icon, label, t},
    },
    lang::{Lang, pair, w},
    model::{Icon, SectionId},
    theme::Ink,
    time as model,
};

/// The amber of a time not right yet.
const AMBER: Ink = Ink::Dark(rgb(0xF5C46E));

impl App {
    /// The page under its title.
    pub(in crate::app) fn time_page<'a>(&'a self, mut page: Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        page = page.push(self.time_clock());
        page = page.push(group(
            w(words::TIME).get(lang),
            column![self.time_auto_row(), divided(true, self.time_set_row()),].into(),
        ));
        page = page.push(group(w(words::TIME_ZONE).get(lang), self.time_zone_row()));
        page = page.push(group(w(words::FORMAT).get(lang), self.time_format_row()));
        page = page.push(group(
            w(words::LATER).get(lang),
            column![
                later_row(
                    w(words::AUTOMATIC_TIME_ZONE),
                    w(words::FOLLOW_WHERE_THE_COMPUTER_IS_WHEN),
                    true,
                    lang,
                ),
                divided(
                    true,
                    later_row(
                        w(words::REGIONAL_FORMATS),
                        w(words::NUMBERS_CURRENCY_AND_THE_FIRST_DAY),
                        false,
                        lang,
                    ),
                ),
            ]
            .into(),
        ));
        container(page)
            .id(format!("page:{}", SectionId::DateTime.section().slug))
            .into()
    }

    /// The clock's card: the time, the date in words, the zone and its
    /// next change.
    fn time_clock(&self) -> El<'_> {
        let lang = self.lang;
        let now = self.time_now();
        let clock = model::say_time(now.time(), self.time_twelve());
        let line = self.time_zone().map_or_else(
            || w(words::TIME_ZONE_NOT_KNOWN).get(lang).to_owned(),
            |zone| model::clock_line(zone, now.timestamp(), lang),
        );
        card_box(
            container(
                row![
                    text(clock).size(36).font(MONO).color(TEXT),
                    column![
                        text(model::say_date(now.date(), lang))
                            .size(15)
                            .font(BOLD)
                            .color(TEXT),
                        t(line, 12.5, DIM.get()),
                    ]
                    .spacing(4),
                ]
                .spacing(20)
                .align_y(Alignment::Center),
            )
            .height(88)
            .center_y(88)
            .padding([0, 18])
            .id("time-clock".to_owned())
            .into(),
        )
    }

    /// "Mettre à l'heure automatiquement", its state, its switch.
    fn time_auto_row(&self) -> El<'_> {
        let lang = self.lang;
        let td = self.time.td();
        let on = self.time_ntp();
        let (line, ink) = self.time_auto_line();
        let inert = !td.is_some_and(|t| t.can_ntp) || self.time.ntp_asked.is_some();
        let moved = self.knobs.get(model::AUTO).map_or(1.0, |since| {
            crate::app::view::ease_out(self.now.saturating_duration_since(*since), crate::app::KNOB)
        });
        let switch = container(switch(
            on && td.is_some_and(|t| t.can_ntp),
            inert,
            self.time_focused(Stop::Ntp),
            moved,
            Message::Time(Msg::Ntp),
        ))
        .id(format!("control:{}", model::AUTO));
        self.time_row(
            model::AUTO,
            w(words::SET_THE_TIME_AUTOMATICALLY).get(lang),
            t(line, 12.5, ink).into(),
            switch.into(),
        )
    }

    /// The automatic time's line, and its colour.
    pub(super) fn time_auto_line(&self) -> (String, Color) {
        let lang = self.lang;
        let said = |fr: &'static str, en: &'static str| pair(fr, en).get(lang).to_owned();
        match self.time.heard.as_ref() {
            None => (String::new(), DIM.get()),
            Some(None) => (
                said(
                    "Le service de l’heure du système (timedated) ne répond pas.",
                    "The system’s time service (timedated) does not answer.",
                ),
                FAINT.get(),
            ),
            Some(Some(td)) if !td.can_ntp => (
                said(
                    "Aucun service de mise à l’heure sur cet ordinateur.",
                    "No time service on this computer.",
                ),
                FAINT.get(),
            ),
            Some(Some(_)) if !self.time_ntp() => (
                said(
                    "Coupé : l’heure ne se corrige plus seule.",
                    "Off: the time no longer corrects itself.",
                ),
                DIM.get(),
            ),
            Some(Some(td)) if td.synced => (
                said(
                    "Par Internet (NTP) · à l’heure",
                    "Over the Internet (NTP) · on time",
                ),
                MINT.get(),
            ),
            Some(Some(_)) => (
                said(
                    "Par Internet (NTP) · pas encore à l’heure",
                    "Over the Internet (NTP) · not on time yet",
                ),
                AMBER.get(),
            ),
        }
    }

    /// "Date et heure": the two fields, inert while the time is
    /// automatic; "Régler" when it is not.
    fn time_set_row(&self) -> El<'_> {
        let lang = self.lang;
        let manual = self.time_manual();
        let (date, time) = self.time_fields();
        let line = if manual {
            w(words::TYPE_THE_DATE_AND_THE_TIME)
        } else {
            w(words::SET_BY_HAND_WHEN_AUTOMATIC_TIME)
        };
        let busy = self.time.time_asked.is_some();
        let typed = self.time_typed();
        let date_ok = model::parse_date(&date).is_some();
        let time_ok = model::parse_time(&time).is_some();
        let mut controls = row![
            field(DATE_FIELD, date, 118.0, manual && !busy, date_ok, |s| {
                Message::Time(Msg::Date(s))
            }),
            field(TIME_FIELD, time, 72.0, manual && !busy, time_ok, |s| {
                Message::Time(Msg::Time(s))
            }),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if manual {
            let can = typed.is_some() && !busy;
            let focused = self.time_focused(Stop::Set);
            let mut set = button(
                text(w(words::SET).get(lang))
                    .size(12.5)
                    .font(BOLD)
                    .color(fade(DEEP.get(), if can { 1.0 } else { INERT })),
            )
            .height(28)
            .padding([5, 12])
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(fade(
                        ACCENT.get(),
                        match (can, hovered) {
                            (false, _) => INERT,
                            (true, true) => 0.9,
                            (true, false) => 1.0,
                        },
                    ))),
                    text_color: DEEP.get(),
                    border: if focused {
                        ring(8.0)
                    } else {
                        border(8.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }
            });
            if can {
                set = set.on_press(Message::Time(Msg::Set));
            }
            controls = controls.push(container(set).id("time-set".to_owned()));
        }
        self.time_row(
            model::SET,
            w(words::DATE_AND_TIME).get(lang),
            t(line.get(lang), 12.5, DIM.get()).into(),
            controls.into(),
        )
    }

    /// "Fuseau horaire": the city, its country, its offset; the zone's
    /// button opening the picker.
    fn time_zone_row(&self) -> El<'_> {
        let lang = self.lang;
        let zone = self.time_zone().unwrap_or("—").to_owned();
        let line = self
            .time_zone()
            .map(|z| model::zone_line(z, jiff::Timestamp::now(), lang))
            .unwrap_or_default();
        let inert = self.time.td().is_none() || self.time.picker.is_some();
        let busy = self.time.zone_asked.is_some();
        let alpha = if inert || busy { INERT } else { 1.0 };
        let focused = self.time_focused(Stop::Zone);
        let mut open = button(
            row![
                text(zone)
                    .size(13.5)
                    .font(SEMIBOLD)
                    .color(fade(TEXT.get(), alpha)),
                Space::new().width(Length::Fill),
                text("›")
                    .size(16)
                    .font(MANROPE)
                    .color(fade(DIM.get(), alpha)),
            ]
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .width(200)
        .height(32)
        .padding([0, 12])
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(fade(
                    crate::theme::ink(rgba(255, 255, 255, if hovered { 0.09 } else { 0.06 })),
                    alpha,
                ))),
                text_color: TEXT.get(),
                border: if focused {
                    ring(8.0)
                } else {
                    border(8.0, 1.0, fade(RIM_12.get(), alpha))
                },
                ..button::Style::default()
            }
        });
        if !inert && !busy {
            open = open.on_press(Message::Time(Msg::Picker(true)));
        }
        self.time_row(
            model::ZONE,
            w(words::TIME_ZONE_2).get(lang),
            t(line, 12.5, DIM.get()).into(),
            container(open)
                .id(format!("control:{}", model::ZONE))
                .into(),
        )
    }

    /// "Format de l'heure et de la date": the desktop's, and its link.
    fn time_format_row(&self) -> El<'_> {
        let lang = self.lang;
        let focused = self.time_focused(Stop::Format);
        let said = format!("{} ›", SectionId::Appearance.name(lang));
        let link = button(t(said, 13.5, MINT.get()).font(SEMIBOLD))
            .padding([4, 6])
            .on_press(Message::Time(Msg::Format))
            .style(flat(
                None,
                crate::theme::ink(rgba(255, 255, 255, 0.05)),
                7.0,
                focused,
            ));
        self.time_row(
            model::FORMAT,
            w(words::TIME_AND_DATE_FORMAT).get(lang),
            t(self.time_format_line(), 12.5, DIM.get()).into(),
            container(link).id(format!("link:{}", model::FORMAT)).into(),
        )
    }

    /// "24 h · « jeudi 5 novembre ». Une fois pour tout le bureau : …".
    pub(super) fn time_format_line(&self) -> String {
        let hours = if self.time_twelve() { "12 h" } else { "24 h" };
        let today =
            crate::appearance::today_as(self.time_date_format(), self.time_now().date(), self.lang);
        words::once_for_the_whole_desktop_the(
            self.lang,
            words::OnceForTheWholeDesktopThe {
                hours,
                today: &today,
            },
        )
    }

    /// A row of the page: its name, its line, its control, lit when found.
    fn time_row<'a>(
        &'a self,
        id: &'static str,
        name: &'a str,
        line: El<'a>,
        control: El<'a>,
    ) -> El<'a> {
        let lit = self.lit(id);
        container(
            row![
                column![text(name).size(14).font(SEMIBOLD).color(TEXT), line]
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

    /// The time zone picker, over the window while it is open.
    pub(in crate::app) fn time_picker(&self) -> Option<El<'_>> {
        let picker = self.time.picker.as_ref()?;
        let lang = self.lang;
        let found = self.time_found();
        let now = jiff::Timestamp::now();
        let twelve = self.time_twelve();
        let field = text_input(w(words::A_CITY_A_COUNTRY_A_ZONE).get(lang), &picker.query)
            .id(ZONE_SEARCH)
            .on_input(|s| Message::Time(Msg::Query(s)))
            .on_submit(Message::Time(Msg::Choose))
            .size(13.5)
            .font(MANROPE)
            .padding([7, 0])
            .style(|_: &Theme, _| text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: border(0.0, 0.0, Color::TRANSPARENT),
                icon: DIM.get(),
                placeholder: FAINT.get(),
                value: TEXT.get(),
                selection: crate::theme::Ink::Accent(0.35).get(),
            });
        let search = container(
            row![icon(Icon::Search, 15.0, DIM.get()), field]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([0, 10])
        .height(36)
        .center_y(36)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255, 255, 255, 0.04,
            )))),
            border: border(9.0, 1.0, ACCENT.get()),
            ..container::Style::default()
        });
        let mut rows = column![].spacing(2);
        for (i, found) in found.iter().enumerate() {
            let name = found.city.said(lang);
            let city: El<'_> = match found.marked {
                Some((start, end)) if end <= name.len() && start < end => {
                    let (pre, rest) = name.split_at(start);
                    let (hit, post) = rest.split_at(end - start);
                    let spans: [text::Span<'_, (), iced::Font>; 3] = [
                        span(pre.to_owned()),
                        span(hit.to_owned())
                            .background(crate::theme::Ink::Accent(0.30).get())
                            .border(border(3.0, 0.0, Color::TRANSPARENT)),
                        span(post.to_owned()),
                    ];
                    rich_text(spans)
                        .size(13.5)
                        .font(SEMIBOLD)
                        .color(TEXT)
                        .into()
                }
                _ => text(name).size(13.5).font(SEMIBOLD).color(TEXT).into(),
            };
            let (line, time) = zone_said(found.city.zone, now, twelve);
            let selected = i == picker.selected;
            rows = rows.push(
                container(
                    button(
                        row![
                            column![city, t(line, 11.5, FAINT.get())]
                                .spacing(1)
                                .width(Length::Fill),
                            text(time).size(12).font(MONO).color(SOFT),
                        ]
                        .align_y(Alignment::Center)
                        .height(Length::Fill),
                    )
                    .width(Length::Fill)
                    .height(44)
                    .padding([0, 10])
                    .on_press(Message::Time(Msg::Pick(i)))
                    .style(flat(
                        selected.then(|| crate::theme::ink(rgba(255, 255, 255, 0.07))),
                        HOVER.get(),
                        8.0,
                        false,
                    )),
                )
                .id(format!("zone:{i}")),
            );
        }
        if found.is_empty() {
            rows = rows.push(
                container(t(
                    words::no_time_zone_matches(lang, picker.query.trim()),
                    13.0,
                    DIM.get(),
                ))
                .padding([12, 10]),
            );
        }
        let chosen = found.get(picker.selected);
        let choose_said = chosen.map_or_else(
            || w(words::CHOOSE).get(lang).to_owned(),
            |f| words::choose_2(lang, f.city.name(lang)),
        );
        let mut choose = button(text(choose_said).size(13).font(BOLD).color(DEEP))
            .height(32)
            .padding([7, 16])
            .style(|_: &Theme, status| solid(ACCENT.get(), DEEP.get(), status));
        if chosen.is_some() {
            choose = choose.on_press(Message::Time(Msg::Choose));
        }
        let buttons = row![
            Space::new().width(Length::Fill),
            container(
                button(t(w(words::CANCEL).get(lang), 13.0, TEXT.get()).font(SEMIBOLD))
                    .height(32)
                    .padding([7, 14])
                    .on_press(Message::Time(Msg::Picker(false)))
                    .style(|_: &Theme, status| {
                        solid(
                            crate::theme::ink(rgba(255, 255, 255, 0.08)),
                            TEXT.get(),
                            status,
                        )
                    }),
            )
            .id("picker:cancel".to_owned()),
            container(choose).id("picker:choose".to_owned()),
        ]
        .spacing(8);
        let sheet = container(
            column![
                row![
                    icon(Icon::Appearance, 18.0, TEXT.get()),
                    text(w(words::TIME_ZONE_2).get(lang))
                        .size(17)
                        .font(BOLD)
                        .color(TEXT),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                search,
                rows,
                t(w(words::A_CITY_A_COUNTRY_OR_A).get(lang), 12.0, FAINT.get(),),
                buttons,
            ]
            .spacing(12),
        )
        .width(440)
        .padding(20)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(MENU.get())),
            border: border(14.0, 1.0, RIM_12.get()),
            shadow: shadow(),
            ..container::Style::default()
        })
        .id("picker".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.58)),
                        0.0,
                        None
                    ))
                )
                .on_press(Message::Time(Msg::Picker(false))),
                container(sheet)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(in crate::app) fn time_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let now = self.time_now();
        let (date, time) = self.time_fields();
        let (auto_line, _) = self.time_auto_line();
        let twelve = self.time_twelve();
        let picker = self.time.picker.as_ref().map(|p| {
            let found = self.time_found();
            let stamp = jiff::Timestamp::now();
            serde_json::json!({
                "query": p.query,
                "selected": p.selected,
                "rows": found.iter().map(|f| {
                    let (line, time) = zone_said(f.city.zone, stamp, twelve);
                    serde_json::json!({
                        "city": f.city.said(lang),
                        "zone": f.city.zone,
                        "line": line,
                        "time": time,
                        "marked": f.marked.map(|(s, e)| f.city.said(lang).get(s..e).unwrap_or("").to_owned()),
                    })
                }).collect::<Vec<_>>(),
            })
        });
        serde_json::json!({
            "heard": self.time.heard.is_some(),
            "service": self.time.td().is_some(),
            "zone": self.time_zone(),
            "clock": model::say_time(now.time(), twelve),
            "date": model::say_date(now.date(), lang),
            "clock_line": self.time_zone().map(|z| model::clock_line(z, now.timestamp(), lang)),
            "ntp": self.time_ntp(),
            "can_ntp": self.time.td().is_some_and(|t| t.can_ntp),
            "ntp_line": auto_line,
            "fields": {
                "date": date,
                "time": time,
                "editable": self.time_manual(),
                "typed": self.time.draft.is_some(),
            },
            "zone_line": self.time_zone().map(|z| model::zone_line(z, now.timestamp(), lang)),
            "format_line": self.time_format_line(),
            "asked": {
                "ntp": self.time.ntp_asked,
                "zone": self.time.zone_asked.as_ref().map(|(c, _)| c.after.clone()),
                "time": self.time.time_asked,
            },
            "picker": picker,
        })
    }
}

/// A row's line and the time there: "America/Toronto · UTC−05:00",
/// "09:42".
fn zone_said(zone: &str, now: jiff::Timestamp, twelve: bool) -> (String, String) {
    match model::zone(zone) {
        Some(tz) => {
            let there = now.to_zoned(tz.clone());
            (
                format!("{zone} · {}", model::say_offset(tz.to_offset(now))),
                model::say_time(there.time(), twelve),
            )
        }
        None => (zone.to_owned(), String::new()),
    }
}

/// A group: its label over its card.
fn group<'a>(name: &'static str, body: El<'a>) -> El<'a> {
    let card = card_box(body);
    column![container(label(name)).height(28).center_y(28), card]
        .spacing(8)
        .into()
}

/// A field of the time set by hand: typed in while it is editable, its
/// text at 45 % otherwise; its rim orange while it does not read.
fn field<'a>(
    id: &'static str,
    value: String,
    width: f32,
    editable: bool,
    reads: bool,
    on_input: fn(String) -> Message,
) -> El<'a> {
    if !editable {
        return container(
            text(value)
                .size(12.5)
                .font(MONO)
                .color(fade(TEXT.get(), INERT)),
        )
        .width(width)
        .height(34)
        .padding([0, 10])
        .center_y(34)
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.03)),
            8.0,
            Some((1.0, fade(RIM_12.get(), INERT))),
        ))
        .id(id.to_owned())
        .into();
    }
    container(
        text_input("", &value)
            .id(id)
            .on_input(on_input)
            .on_submit(Message::Time(Msg::Set))
            .size(12.5)
            .font(MONO)
            .padding([8, 10])
            .width(width)
            .style(move |_: &Theme, status| {
                let focused = matches!(status, text_input::Status::Focused { .. });
                text_input::Style {
                    background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05))),
                    border: border(
                        8.0,
                        1.0,
                        if !reads {
                            ORANGE.get()
                        } else if focused {
                            ACCENT.get()
                        } else {
                            RIM_12.get()
                        },
                    ),
                    icon: DIM.get(),
                    placeholder: FAINT.get(),
                    value: TEXT.get(),
                    selection: crate::theme::Ink::Accent(0.35).get(),
                }
            }),
    )
    .id(format!("box:{id}"))
    .into()
}

/// A row of what comes later: inert, its amber chip; a switch, off, for
/// the automatic zone.
fn later_row<'a>(
    name: crate::lang::Words,
    line: crate::lang::Words,
    switched: bool,
    lang: Lang,
) -> El<'a> {
    let chip = container(
        text(w(words::LATER_2).get(lang))
            .size(10.5)
            .font(BOLD)
            .color(rgb(0xF5B44A)),
    )
    .padding([1, 7])
    .style(boxed(
        crate::theme::ink(rgba(245, 180, 74, 0.12)),
        8.0,
        Some((1.0, crate::theme::ink(rgba(245, 180, 74, 0.25)))),
    ));
    let mut out = row![
        column![
            row![
                text(name.get(lang))
                    .size(14)
                    .font(SEMIBOLD)
                    .color(fade(TEXT.get(), INERT)),
                chip,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            t(line.get(lang), 12.5, fade(DIM.get(), INERT)),
        ]
        .spacing(2)
        .width(Length::Fill),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    if switched {
        out = out.push(switch(false, true, false, 1.0, Message::Nothing));
    }
    container(out)
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .into()
}

/// A switch, 40 × 22, as the other pages draw it.
fn switch<'a>(on: bool, inert: bool, focused: bool, moved: f32, press: Message) -> El<'a> {
    let alpha = if inert { INERT } else { 1.0 };
    let at = if on { moved } else { 1.0 - moved };
    let track = if on {
        ACCENT.get()
    } else {
        crate::theme::Ink::Token(|t| t.switch_off).get()
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
    if !inert {
        switch = switch.on_press(press);
    }
    switch.into()
}

/// A filled button.
fn solid(fill: Color, ink: Color, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            Color {
                a: (fill.a + 0.04).min(1.0),
                ..fill
            }
        } else {
            fill
        })),
        text_color: ink,
        border: border(8.0, 0.0, Color::TRANSPARENT),
        ..button::Style::default()
    }
}
