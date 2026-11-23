//! The night light's group on the Screens page (#436, #929): the model's
//! rows, and with the sun its hours today at the weather's place in place
//! of the fixed ones; without a place, where to choose one and the fixed
//! hours meanwhile.

use hocus_link::sun::{self, SunDay};
use hocus_words::settings::screens as words;
// The widget macros by name: through the glob they would be ambiguous.
use iced::widget::{column, row};

use super::super::wallpaper_changing::clock_hhmm;
use super::*;
use crate::{
    model::{SectionId, Value},
    store::Slot,
    wallpaper::WeatherPlace,
};

/// The sun's row's id, for the log and the tests.
const SUN_ROW: &str = "screens.night-light.sun";

impl App {
    /// Whether the night light follows the sun (`[night-light] schedule`,
    /// "sun" by default).
    fn night_follows_sun(&self) -> bool {
        crate::model::setting(screens::NIGHT_SCHEDULE)
            .and_then(|s| self.value(Slot::of(s, self.source)))
            .is_none_or(|v| v == Value::Word("sun"))
    }

    /// `[weather]`'s place, whose sun it follows.
    fn night_place(&self) -> Option<WeatherPlace> {
        self.snapshot.as_ref()?.wallpaper.place.clone()
    }

    /// The group's model rows, in order: the fixed hours only while they
    /// apply (chosen, or the sun without a place).
    pub(in crate::app) fn night_rows(&self) -> Vec<&'static str> {
        let hours = !self.night_follows_sun() || self.night_place().is_none();
        screens::NIGHT_ROWS
            .into_iter()
            .filter(|id| hours || !matches!(*id, screens::NIGHT_FROM | screens::NIGHT_TO))
            .collect()
    }

    /// The sun's row: its name and line; whether it links to Weather.
    fn night_sun_said(&self) -> (String, String, bool) {
        let lang = self.lang;
        match self.night_place() {
            Some(WeatherPlace(place, lat, lon)) => {
                let name = match sun::on(jiff::Zoned::now().date(), lat, lon) {
                    SunDay::Rises { rise, set } => words::sunset_and_sunrise(
                        lang,
                        words::SunsetAndSunrise {
                            set: &clock_hhmm(set),
                            rise: &clock_hhmm(rise),
                        },
                    ),
                    SunDay::AlwaysUp => words::NO_SUNSET_TODAY.get(lang).to_owned(),
                    SunDay::AlwaysDown => words::NO_SUNRISE_TODAY.get(lang).to_owned(),
                };
                (name, words::at_the_weather_s_place(lang, &place), false)
            }
            None => (
                words::CHOOSE_A_PLACE_IN_WEATHER.get(lang).to_owned(),
                words::UNTIL_THEN_THE_HOURS_BELOW.get(lang).to_owned(),
                true,
            ),
        }
    }

    /// The group: its label and its card.
    pub(in crate::app) fn night_group(&self) -> El<'_> {
        let controls = self.controls();
        let mut night = column![];
        for (i, id) in self.night_rows().into_iter().enumerate() {
            if let Some(setting) = crate::model::setting(id) {
                night = night.push(divided(i > 0, self.row(setting, &controls)));
            }
            if id == screens::NIGHT_SCHEDULE && self.night_follows_sun() {
                night = night.push(divided(true, self.night_sun_row()));
            }
        }
        column![
            row![label(
                self.say(hocus_words::settings::app_screens_view::NIGHT_LIGHT)
            )]
            .height(28)
            .align_y(Alignment::Center),
            card_box(night.into()),
        ]
        .spacing(8)
        .into()
    }

    /// "Coucher 17:12 · lever 07:58", "À Saint-Étienne, le lieu de la
    /// météo"; without a place, "Choisis un lieu dans Météo" and its link.
    fn night_sun_row(&self) -> El<'_> {
        let ink = crate::model::setting(screens::NIGHT_SCHEDULE)
            .map_or(1.0, |s| if self.inert(s) { INERT } else { 1.0 });
        let (name, line, link) = self.night_sun_said();
        let words = column![
            t(name, 14.0, fade(TEXT.get(), ink)).font(SEMIBOLD),
            t(line, 12.5, fade(DIM.get(), ink)),
        ]
        .spacing(2)
        .width(Length::Fill);
        let mut line = row![words, Space::new().height(36)];
        if link {
            line = line.push(
                container(self.link_control(SectionId::Weather)).id(format!("control:{SUN_ROW}")),
            );
        }
        container(line.spacing(16).align_y(Alignment::Center))
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .id(format!("row:{SUN_ROW}"))
            .into()
    }

    /// What the group shows, for the log (tests).
    pub(in crate::app) fn night_shown(&self) -> serde_json::Value {
        let sun = self.night_follows_sun().then(|| {
            let (name, line, link) = self.night_sun_said();
            serde_json::json!({ "name": name, "line": line, "link": link })
        });
        serde_json::json!({ "rows": self.night_rows(), "sun": sun })
    }
}
