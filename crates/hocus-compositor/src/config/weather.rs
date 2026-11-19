//! The weather, `[weather]` (#814, docs/design/bar.md "Weather"): the
//! place the shell asks Open-Meteo's forecast for, and its units. The
//! compositor only passes it on to the shell, which fetches and shows it;
//! followed live.

use serde::Deserialize;

/// `[weather]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Weather {
    /// Shown once a place is set.
    pub enabled: bool,
    /// The place's name, as the bar says it; empty: none.
    pub place: String,
    pub latitude: f64,
    pub longitude: f64,
    pub units: WeatherUnits,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            enabled: true,
            place: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            units: WeatherUnits::Metric,
        }
    }
}

/// How the weather's values are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WeatherUnits {
    /// °C, mm, km/h.
    #[default]
    Metric,
    /// °F, in, mph.
    Imperial,
}

impl Weather {
    /// What the shell is told of it.
    pub fn place(&self) -> hocus_link::WeatherPlace {
        hocus_link::WeatherPlace {
            enabled: self.enabled,
            place: self.place.trim().to_owned(),
            latitude: self.latitude,
            longitude: self.longitude,
            imperial: self.units == WeatherUnits::Imperial,
        }
    }

    /// Whether a place is set, whose weather the rain on the glass may
    /// follow (`[effects] rain-when = "raining"`).
    pub fn has_place(&self) -> bool {
        !self.place.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_reads_its_place_and_units_and_refuses_what_it_does_not_know() {
        let config: crate::Config = toml::from_str("").unwrap();
        assert_eq!(config.weather, Weather::default());
        assert!(!config.weather.place().shown());
        let config: crate::Config = toml::from_str(
            "[weather]\nplace = \"Saint-Étienne\"\nlatitude = 45.43\nlongitude = 4.39\nunits = \"imperial\"\n",
        )
        .unwrap();
        let place = config.weather.place();
        assert!(place.shown() && place.imperial);
        assert_eq!((place.latitude, place.longitude), (45.43, 4.39));
        assert!(toml::from_str::<crate::Config>("[weather]\nunits = \"kelvin\"\n").is_err());
        assert!(toml::from_str::<crate::Config>("[weather]\ncity = \"Lyon\"\n").is_err());
        // Its rain on the glass: always, or only while it rains.
        let config: crate::Config =
            toml::from_str("[effects]\nrain-on-glass = true\nrain-when = \"raining\"\n").unwrap();
        assert_eq!(config.effects.rain_when, crate::config::RainWhen::Raining);
    }
}
