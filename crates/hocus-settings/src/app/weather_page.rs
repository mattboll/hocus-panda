//! The Météo page (#814): "Montrer la météo", the place, the units, and
//! what the network is asked, said plainly. The place is a field that
//! finds a town by its name: typed, it asks Open-Meteo's geocoding (300 ms
//! after the last key, on a blocking task, never the window's thread) and
//! lists what it found, name over region and country; one chosen writes
//! `[weather] place`, `latitude` and `longitude` together, with its toast
//! and its undo. Only the name typed leaves the machine.
//!
//! The rain on the glass's "Quand" (Settings › Appearance › Effects) is
//! inert without a place, and says where to choose one.

use hocus_weather::geocode::Place;
use hocus_words::settings::weather as words;
use std::time::Duration;

use iced::{
    Alignment, Length, Padding,
    widget::{Space, button, column, container, row, text_input},
};

use super::{
    App, Doing, KeysSaid, Message,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    lang::Lang,
    model::{Icon, Setting, Value},
    store::Slot,
    weather as model,
};

/// How long after the last key the place is looked for.
const TYPED: Duration = Duration::from_millis(300);
/// The field's id.
pub const FIELD: &str = "weather:place";

/// What the field found.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Found {
    #[default]
    Nothing,
    Searching,
    Places(Vec<Place>),
    /// No answer (offline): why, for the log.
    Failed,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// What is typed in the field, while it is edited.
    query: Option<String>,
    found: Found,
    /// The last search asked, so that an older answer is let go.
    serial: u64,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Typed(String),
    /// The pause after the last key: search, if nothing was typed since.
    Search(u64),
    Answer(u64, Result<Vec<Place>, String>),
    Pick(usize),
}

impl App {
    /// The place the file names (empty: none).
    pub(super) fn weather_place(&self) -> String {
        match crate::model::setting(model::PLACE)
            .and_then(|s| self.value(Slot::of(s, self.source)))
        {
            Some(Value::Text(place)) => place.trim().to_owned(),
            _ => String::new(),
        }
    }

    /// Whether `setting` waits for a place: the rain's "Quand".
    pub(super) fn weather_inert(&self, setting: &Setting) -> bool {
        setting.id == crate::effects::RAIN_WHEN && self.weather_place().is_empty()
    }

    /// The line under the rain's "Quand" without a place: where to choose
    /// one.
    pub(super) fn weather_line(&self, id: &str) -> Option<&'static str> {
        (id == crate::effects::RAIN_WHEN && self.weather_place().is_empty())
            .then(|| hocus_words::settings::effects::RAIN_NEEDS_PLACE.get(self.lang))
    }

    pub(super) fn weather_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Typed(text) => {
                self.weather.serial += 1;
                let serial = self.weather.serial;
                self.weather.found = Found::Nothing;
                self.weather.query = Some(text);
                iced::Task::perform(tokio::time::sleep(TYPED), move |()| {
                    Message::Weather(Msg::Search(serial))
                })
            }
            Msg::Search(serial) => {
                let Some(query) = self.weather.query.clone() else {
                    return iced::Task::none();
                };
                if serial != self.weather.serial || query.trim().chars().count() < 2 {
                    return iced::Task::none();
                }
                self.weather.found = Found::Searching;
                let language = match self.lang {
                    Lang::French => "fr",
                    _ => "en",
                };
                tracing::info!("the weather's place is looked for");
                iced::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            hocus_weather::geocode::search(&query, language)
                        })
                        .await
                        .unwrap_or_else(|err| Err(err.to_string()))
                    },
                    move |answer| Message::Weather(Msg::Answer(serial, answer)),
                )
            }
            Msg::Answer(serial, answer) => {
                if serial == self.weather.serial && self.weather.query.is_some() {
                    self.weather.found = match answer {
                        Ok(places) => {
                            tracing::info!(found = places.len(), "the weather's places");
                            Found::Places(places)
                        }
                        Err(err) => {
                            tracing::warn!(err, "the weather's places: no answer");
                            Found::Failed
                        }
                    };
                }
                iced::Task::none()
            }
            Msg::Pick(i) => {
                let Found::Places(places) = &self.weather.found else {
                    return iced::Task::none();
                };
                let Some(place) = places.get(i).cloned() else {
                    return iced::Task::none();
                };
                let lang = self.lang;
                let before = self.weather_place();
                let then = if before.is_empty() {
                    words::NO_PLACE.get(lang).to_owned()
                } else {
                    before
                };
                let said = KeysSaid {
                    done: words::place_done(lang, &place.name),
                    undone: words::place_undone(lang, &then),
                    redone: words::place_redone(lang, &place.name),
                    row: model::PLACE,
                };
                tracing::info!(region = place.region(), "a place chosen for the weather");
                self.weather = State::default();
                self.ask_keys(model::place_keys(&place), said, Doing::New, None);
                iced::Task::none()
            }
        }
    }

    pub(super) fn weather_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let controls = self.controls();
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        let rows = column![
            model_row(model::ENABLED),
            divided(true, self.place_row()),
            divided(true, model_row(model::UNITS)),
        ];
        page = page.push(
            column![
                container(label(model::WEATHER.label.get(lang)))
                    .height(28)
                    .center_y(28),
                card_box(rows.into()),
            ]
            .spacing(8),
        );
        page = page.push(
            container(
                row![
                    icon(Icon::Info, 17.0, DIM.get()),
                    column![
                        t(words::WHERE_FROM.get(lang), 12.5, TEXT.get()).font(BOLD),
                        t(words::WHERE_FROM_LINE.get(lang), 12.5, DIM.get()),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .padding([12, 14])
            .width(Length::Fill)
            .style(boxed(
                iced::Color::TRANSPARENT,
                10.0,
                Some((1.0, RIM_12.get())),
            ))
            .id("weather:where-from".to_owned()),
        );
        container(page).id("page:weather".to_owned()).into()
    }

    /// "Lieu": the field, and under it what it found.
    fn place_row(&self) -> El<'_> {
        let lang = self.lang;
        let place = self.weather_place();
        let enabled = crate::model::setting(model::ENABLED)
            .and_then(|s| self.value(Slot::of(s, self.source)))
            != Some(Value::Bool(false));
        let ink = if enabled { 1.0 } else { INERT };
        let shown = self.weather.query.clone().unwrap_or_else(|| place.clone());
        let mut field = text_input(words::SEARCH.get(lang), &shown)
            .id(FIELD)
            .size(14.0)
            .font(MANROPE)
            .padding([8, 10])
            .style(search);
        if enabled && self.writable() {
            field = field
                .on_input(|s| Message::Weather(Msg::Typed(s)))
                .on_submit(match &self.weather.found {
                    Found::Places(places) if !places.is_empty() => Message::Weather(Msg::Pick(0)),
                    _ => Message::Nothing,
                });
        }
        let editing = self.weather.query.is_some();
        let mut under = column![
            container(field)
                .width(Length::Fill)
                .style(boxed(
                    crate::theme::ink(rgba(255, 255, 255, 0.06)),
                    8.0,
                    Some((1.0, if editing { ACCENT.get() } else { RIM_12.get() })),
                ))
                .id("weather:field".to_owned()),
        ]
        .spacing(4);
        let note = |said: &'static str| -> El<'_> {
            container(t(said, 12.0, FAINT.get()))
                .padding(Padding::default().left(10).top(4))
                .into()
        };
        match &self.weather.found {
            Found::Nothing => {}
            Found::Searching => under = under.push(note(words::SEARCHING.get(lang))),
            Found::Failed => under = under.push(note(words::NO_ANSWER.get(lang))),
            Found::Places(places) if places.is_empty() => {
                under = under.push(note(words::NOTHING_FOUND.get(lang)));
            }
            Found::Places(places) => {
                let mut list = column![];
                for (i, found) in places.iter().enumerate() {
                    let line = column![
                        t(found.name.clone(), 13.5, TEXT.get()).font(SEMIBOLD),
                        t(found.region(), 12.0, DIM.get()),
                    ]
                    .spacing(1);
                    let lit = i == 0;
                    list = list.push(
                        container(
                            button(line)
                                .width(Length::Fill)
                                .padding([6, 10])
                                .on_press(Message::Weather(Msg::Pick(i)))
                                .style(flat(
                                    lit.then(|| crate::theme::ink(rgba(255, 255, 255, 0.08))),
                                    HOVER.get(),
                                    7.0,
                                    false,
                                )),
                        )
                        .id(format!("weather:pick:{i}")),
                    );
                }
                under = under.push(
                    container(list)
                        .padding(4)
                        .width(Length::Fill)
                        .style(boxed(MENU.get(), 10.0, Some((1.0, RIM_12.get()))))
                        .id("weather:found".to_owned()),
                );
            }
        }
        let words_col = column![
            t(words::PLACE.get(lang), 14.0, fade(TEXT.get(), ink)).font(SEMIBOLD),
            t(words::PLACE_LINE.get(lang), 12.5, fade(DIM.get(), ink)),
        ]
        .spacing(2)
        .width(Length::Fill);
        container(
            row![words_col, container(under).width(300)]
                .spacing(16)
                .align_y(Alignment::Start),
        )
        .padding(Padding {
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
            left: 16.0,
        })
        .id(format!("row:{}", model::PLACE))
        .into()
    }

    /// What the page shows, for the log (tests).
    pub(super) fn weather_shown(&self) -> serde_json::Value {
        let found: Vec<String> = match &self.weather.found {
            Found::Places(places) => places
                .iter()
                .map(|p| format!("{} ({})", p.name, p.region()))
                .collect(),
            _ => Vec::new(),
        };
        serde_json::json!({
            "place": self.weather_place(),
            "typed": self.weather.query,
            "found": found,
        })
    }
}
