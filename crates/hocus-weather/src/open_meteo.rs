//! Open-Meteo's forecast answer (`/v1/forecast`), read into a
//! [`Forecast`]: its local times (`timezone=auto`) made instants with its
//! `utc_offset_seconds`, its missing values (`null`) left out.

use jiff::{Timestamp, civil, tz::Offset};
use serde::Deserialize;

use crate::{Day, Forecast, Hour, Moment, Quarter};

#[derive(Debug, Deserialize)]
struct Answer {
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    utc_offset_seconds: i32,
    current: Current,
    #[serde(default)]
    minutely_15: Option<Minutely>,
    hourly: Hourly,
    daily: Daily,
}

#[derive(Debug, Deserialize)]
struct Current {
    time: civil::DateTime,
    temperature_2m: f32,
    #[serde(default)]
    apparent_temperature: Option<f32>,
    #[serde(default)]
    precipitation: Option<f32>,
    weather_code: u8,
    #[serde(default)]
    wind_speed_10m: Option<f32>,
    #[serde(default)]
    is_day: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct Minutely {
    time: Vec<civil::DateTime>,
    precipitation: Vec<Option<f32>>,
}

#[derive(Debug, Deserialize)]
struct Hourly {
    time: Vec<civil::DateTime>,
    temperature_2m: Vec<Option<f32>>,
    #[serde(default)]
    apparent_temperature: Vec<Option<f32>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f32>>,
    #[serde(default)]
    precipitation: Vec<Option<f32>>,
    weather_code: Vec<Option<u8>>,
    #[serde(default)]
    wind_speed_10m: Vec<Option<f32>>,
    #[serde(default)]
    is_day: Vec<Option<u8>>,
}

#[derive(Debug, Deserialize)]
struct Daily {
    time: Vec<civil::Date>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_max: Vec<Option<f32>>,
    temperature_2m_min: Vec<Option<f32>>,
    #[serde(default)]
    precipitation_sum: Vec<Option<f32>>,
}

/// Why an answer does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread(pub String);

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unread {}

/// `list`'s `i`th value, when there is one.
fn at<T: Copy>(list: &[Option<T>], i: usize) -> Option<T> {
    list.get(i).copied().flatten()
}

impl Forecast {
    /// Open-Meteo's answer `json`, asked at `fetched` (none: its current
    /// time, a fixture's).
    pub fn parse(json: &str, fetched: Option<Timestamp>) -> Result<Self, Unread> {
        let answer: Answer =
            serde_json::from_str(json).map_err(|err| Unread(format!("forecast: {err}")))?;
        let offset = Offset::from_seconds(answer.utc_offset_seconds)
            .map_err(|err| Unread(format!("forecast's offset: {err}")))?;
        let instant = |t: civil::DateTime| -> Result<Timestamp, Unread> {
            offset
                .to_timestamp(t)
                .map_err(|err| Unread(format!("forecast's time {t}: {err}")))
        };
        let c = &answer.current;
        let current = Moment {
            at: instant(c.time)?,
            temperature: c.temperature_2m,
            feels: c.apparent_temperature.unwrap_or(c.temperature_2m),
            precipitation: c.precipitation.unwrap_or(0.0),
            code: c.weather_code,
            wind: c.wind_speed_10m.unwrap_or(0.0),
            day: c.is_day.is_none_or(|d| d != 0),
        };
        let mut quarters = Vec::new();
        if let Some(m) = &answer.minutely_15 {
            for (i, time) in m.time.iter().enumerate() {
                // Each is the quarter hour's before its time.
                if let Some(mm) = at(&m.precipitation, i) {
                    let end = instant(*time)?;
                    quarters.push(Quarter {
                        start: end - jiff::SignedDuration::from_mins(15),
                        mm,
                    });
                }
            }
        }
        let h = &answer.hourly;
        let mut hours = Vec::new();
        for (i, time) in h.time.iter().enumerate() {
            let (Some(temperature), Some(code)) =
                (at(&h.temperature_2m, i), at(&h.weather_code, i))
            else {
                continue;
            };
            hours.push(Hour {
                moment: Moment {
                    at: instant(*time)?,
                    temperature,
                    feels: at(&h.apparent_temperature, i).unwrap_or(temperature),
                    precipitation: at(&h.precipitation, i).unwrap_or(0.0),
                    code,
                    wind: at(&h.wind_speed_10m, i).unwrap_or(0.0),
                    day: at(&h.is_day, i).is_none_or(|d| d != 0),
                },
                probability: at(&h.precipitation_probability, i)
                    .map_or(0, |p| p.clamp(0.0, 100.0).round() as u8),
            });
        }
        let d = &answer.daily;
        let mut days = Vec::new();
        for (i, date) in d.time.iter().enumerate() {
            let (Some(code), Some(min), Some(max)) = (
                at(&d.weather_code, i),
                at(&d.temperature_2m_min, i),
                at(&d.temperature_2m_max, i),
            ) else {
                continue;
            };
            days.push(Day {
                date: *date,
                code,
                min,
                max,
                mm: at(&d.precipitation_sum, i).unwrap_or(0.0),
            });
        }
        Ok(Self {
            fetched: fetched.unwrap_or(current.at),
            latitude: answer.latitude,
            longitude: answer.longitude,
            current,
            quarters,
            hours,
            days,
        })
    }
}
