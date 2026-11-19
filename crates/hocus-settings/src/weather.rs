//! Météo / Weather (#814): the weather in the bar. Whether it shows, its
//! place (a town found by its name with Open-Meteo's geocoding, its page's
//! own field, which writes `[weather] place`, `latitude` and `longitude`
//! together) and its units; then what the network is asked, said plainly.

use hocus_words::settings::weather as words;
use std::sync::OnceLock;

use crate::model::{Choice, Codec, Kind, RowGroup, SectionId, Setting, Value, one};

pub const WEATHER: RowGroup = RowGroup {
    label: words::THE_WEATHER,
    per_source: false,
};

pub const ENABLED: &str = "weather.enabled";
pub const PLACE: &str = "weather.place";
pub const UNITS: &str = "weather.units";

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![
            Setting {
                id: ENABLED,
                section: SectionId::Weather,
                group: WEATHER,
                name: words::SHOW,
                line: Some(words::SHOW_LINE),
                keywords: (
                    &["météo", "temps", "pluie", "température"],
                    &["weather", "rain", "temperature", "forecast"],
                ),
                kind: Kind::Switch,
                places: one(&["weather", "enabled"], Codec::Bool),
                needs: None,
            },
            // Drawn by the page: a field that finds a town by its name.
            Setting {
                id: PLACE,
                section: SectionId::Weather,
                group: WEATHER,
                name: words::PLACE,
                line: Some(words::PLACE_LINE),
                keywords: (
                    &["lieu", "ville", "village", "météo"],
                    &["place", "town", "city", "location", "weather"],
                ),
                kind: Kind::Text,
                places: one(&["weather", "place"], Codec::Text),
                needs: Some(ENABLED),
            },
            Setting {
                id: UNITS,
                section: SectionId::Weather,
                group: WEATHER,
                name: words::UNITS,
                line: None,
                keywords: (
                    &["unités", "celsius", "fahrenheit", "degrés"],
                    &["units", "celsius", "fahrenheit", "degrees"],
                ),
                kind: Kind::Segmented(vec![
                    Choice {
                        label: words::METRIC,
                        value: Value::Word("metric"),
                    },
                    Choice {
                        label: words::IMPERIAL,
                        value: Value::Word("imperial"),
                    },
                ]),
                places: one(&["weather", "units"], Codec::Word(&["metric", "imperial"])),
                needs: Some(ENABLED),
            },
        ]
    })
}

/// The `[weather]` keys a place chosen writes: its name, where it is.
pub fn place_keys(place: &hocus_weather::geocode::Place) -> Vec<crate::store::KeyEdit> {
    let key = |name: &str| vec!["weather".to_owned(), name.to_owned()];
    vec![
        crate::store::KeyEdit {
            key: key("place"),
            value: Some(toml_edit::Value::from(place.name.as_str()).to_string()),
        },
        crate::store::KeyEdit {
            key: key("latitude"),
            value: Some(format!("{:.4}", place.latitude)),
        },
        crate::store::KeyEdit {
            key: key("longitude"),
            value: Some(format!("{:.4}", place.longitude)),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_chosen_writes_its_name_and_where_it_is() {
        let place = hocus_weather::geocode::Place {
            name: "Saint-Étienne".to_owned(),
            admin1: Some("Auvergne-Rhône-Alpes".to_owned()),
            country: Some("France".to_owned()),
            latitude: 45.43389,
            longitude: 4.39,
        };
        let keys = place_keys(&place);
        let said: Vec<(String, String)> = keys
            .iter()
            .map(|k| (k.key.join("."), k.value.clone().unwrap_or_default()))
            .collect();
        assert_eq!(
            said,
            [
                ("weather.place".to_owned(), "\"Saint-Étienne\"".to_owned()),
                ("weather.latitude".to_owned(), "45.4339".to_owned()),
                ("weather.longitude".to_owned(), "4.3900".to_owned()),
            ]
        );
        // Each value reads as TOML.
        for k in &keys {
            assert!(k.value.as_deref().unwrap().parse::<toml_edit::Value>().is_ok());
        }
    }

    #[test]
    fn its_rows_take_their_defaults_from_the_shipped_file() {
        let all = settings();
        assert_eq!(all.len(), 3);
        let default = |id| {
            crate::model::setting(id)
                .map(|s| s.places.of(crate::model::Source::Ac).default.clone())
        };
        assert_eq!(default(ENABLED), Some(Value::Bool(true)));
        assert_eq!(default(PLACE), Some(Value::Text(String::new())));
        assert_eq!(default(UNITS), Some(Value::Word("metric")));
    }
}
