//! The weather (#814, docs/design/bar.md "Weather"): Open-Meteo's
//! forecast for one place, read from its JSON ([`Forecast::parse`]), kept
//! on disk ([`cache`]), and what the bar says of it at a given time: the
//! weather now ([`Forecast::now`]), the rain coming later today
//! ([`Forecast::rain_today`]), the next quarter hours, hours and days.
//! Places are found by name with Open-Meteo's geocoding ([`geocode`]).
//!
//! Everything is held in metric units (°C, mm, km/h), as asked of
//! Open-Meteo; [`Units`] converts what is shown. Nothing here blocks but
//! [`fetch::forecast`] and [`geocode::search`], which their callers run
//! on threads of their own.

pub mod cache;
pub mod fetch;
pub mod geocode;
mod open_meteo;

use jiff::{SignedDuration, Timestamp, Zoned, civil};
use serde::{Deserialize, Serialize};

/// `HOCUS_WEATHER_FIXTURE=FILE`: the shell reads the forecast from this
/// saved Open-Meteo answer instead of the network (tests, screenshots).
pub const FIXTURE_ENV: &str = "HOCUS_WEATHER_FIXTURE";

/// Rain from this much in a slot (mm).
pub const WET: f32 = 0.1;

/// From this hour the bar shows tomorrow first.
pub const EVENING: i8 = 18;

/// The weather's look, one of the bar's icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Icon {
    Sun,
    PartlyCloudy,
    Cloudy,
    Rain,
    Showers,
    Snow,
    Fog,
    Thunder,
    /// A clear night.
    Moon,
}

impl Icon {
    /// The icon of WMO weather code `code`, by day or night.
    pub fn of(code: u8, day: bool) -> Self {
        match Condition::of(code) {
            Condition::Clear if day => Self::Sun,
            Condition::Clear => Self::Moon,
            Condition::Partly if day => Self::PartlyCloudy,
            Condition::Partly | Condition::Cloudy => Self::Cloudy,
            Condition::Fog => Self::Fog,
            Condition::Drizzle | Condition::Rain => Self::Rain,
            Condition::Showers => Self::Showers,
            Condition::Snow => Self::Snow,
            Condition::Thunder => Self::Thunder,
        }
    }

    /// Whether it says it rains (the rain on the glass, "only when it
    /// rains").
    pub fn wet(self) -> bool {
        matches!(self, Self::Rain | Self::Showers | Self::Thunder)
    }
}

/// What the weather is, as the popover says it ("Couvert").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    Clear,
    Partly,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Showers,
    Snow,
    Thunder,
}

impl Condition {
    /// WMO weather code `code`'s (Open-Meteo's `weather_code`); one it does
    /// not list reads as cloudy.
    pub fn of(code: u8) -> Self {
        match code {
            0 | 1 => Self::Clear,
            2 => Self::Partly,
            45 | 48 => Self::Fog,
            51..=57 => Self::Drizzle,
            61..=67 => Self::Rain,
            71..=77 | 85 | 86 => Self::Snow,
            80..=82 => Self::Showers,
            95..=99 => Self::Thunder,
            _ => Self::Cloudy,
        }
    }

    pub fn snow(self) -> bool {
        self == Self::Snow
    }
}

/// How values are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Units {
    /// °C, mm, km/h.
    #[default]
    Metric,
    /// °F, in, mph.
    Imperial,
}

impl Units {
    /// `metric` or `imperial`, as the configuration writes them.
    pub fn of(word: &str) -> Self {
        match word {
            "imperial" => Self::Imperial,
            _ => Self::Metric,
        }
    }

    /// A temperature (°C), whole, in these units.
    pub fn degrees(self, celsius: f32) -> i32 {
        let t = match self {
            Self::Metric => celsius,
            Self::Imperial => celsius * 9.0 / 5.0 + 32.0,
        };
        t.round() as i32
    }

    /// A precipitation (mm): "6 mm", "0.2 in"; none under 0.5 mm.
    pub fn amount(self, mm: f32) -> Option<String> {
        if mm < 0.5 {
            return None;
        }
        Some(match self {
            Self::Metric => format!("{} mm", mm.round() as i32),
            Self::Imperial => format!("{:.1} in", mm / 25.4),
        })
    }

    /// A wind's speed (km/h): "18 km/h", "11 mph".
    pub fn wind(self, kmh: f32) -> String {
        match self {
            Self::Metric => format!("{} km/h", kmh.round() as i32),
            Self::Imperial => format!("{} mph", (kmh / 1.609_344).round() as i32),
        }
    }
}

/// The weather at a time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Moment {
    pub at: Timestamp,
    pub temperature: f32,
    /// Feels like.
    pub feels: f32,
    /// mm, over the slot it ends.
    pub precipitation: f32,
    /// WMO weather code.
    pub code: u8,
    /// km/h.
    pub wind: f32,
    pub day: bool,
}

impl Moment {
    pub fn icon(&self) -> Icon {
        Icon::of(self.code, self.day)
    }
}

/// An hour of the forecast, at its start; its rain is the hour's before it
/// (Open-Meteo's "preceding hour").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    pub moment: Moment,
    /// The chance of rain, 0 to 100.
    pub probability: u8,
}

/// A quarter hour's rain: from `start` for 15 minutes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quarter {
    pub start: Timestamp,
    pub mm: f32,
}

/// A day of the forecast.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Day {
    pub date: civil::Date,
    pub code: u8,
    pub min: f32,
    pub max: f32,
    pub mm: f32,
}

impl Day {
    pub fn icon(&self) -> Icon {
        Icon::of(self.code, true)
    }
}

/// A forecast for one place, as Open-Meteo answered it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    /// When it was asked (the fixture's: its `current` time).
    pub fetched: Timestamp,
    /// Where it was asked for.
    pub latitude: f64,
    pub longitude: f64,
    pub current: Moment,
    pub quarters: Vec<Quarter>,
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
}

/// Rain coming later today, or falling now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rain {
    /// When it starts; `None` while it rains.
    pub start: Option<Timestamp>,
    /// When it stops; `None` past what the forecast says.
    pub end: Option<Timestamp>,
    /// It snows.
    pub snow: bool,
}

/// A slot of rain: from, to, mm.
type Slot = (Timestamp, Timestamp, f32);

const QUARTER: SignedDuration = SignedDuration::from_mins(15);
const HOUR: SignedDuration = SignedDuration::from_hours(1);
/// The current block is the weather now for this long after it.
const CURRENT_FOR: SignedDuration = SignedDuration::from_mins(30);

impl Forecast {
    /// Whether it is for the place at `latitude`, `longitude`.
    pub fn is_for(&self, latitude: f64, longitude: f64) -> bool {
        (self.latitude - latitude).abs() < 0.05 && (self.longitude - longitude).abs() < 0.05
    }

    /// The weather at `at`: the answer's current block while it is
    /// recent, else the hour nearest `at` (a forecast kept offline); none
    /// past the hours it holds.
    pub fn now(&self, at: Timestamp) -> Option<Moment> {
        let since = at.duration_since(self.current.at);
        if since.abs() <= CURRENT_FOR {
            return Some(self.current);
        }
        self.hours
            .iter()
            .map(|h| h.moment)
            .filter(|m| at.duration_since(m.at).abs() <= SignedDuration::from_mins(90))
            .min_by_key(|m| at.duration_since(m.at).abs())
    }

    /// Whether it rains at `at`: its quarter hour holds rain, or the
    /// weather now has some or says rain, showers or thunder.
    pub fn raining(&self, at: Timestamp) -> bool {
        let quarter = self
            .quarters
            .iter()
            .find(|q| q.start <= at && at < q.start + QUARTER)
            .is_some_and(|q| q.mm >= WET);
        quarter
            || self
                .now(at)
                .is_some_and(|m| m.precipitation > 0.0 || m.icon().wet())
    }

    /// The rain slots from `at` on: the quarter hours', then the hours'
    /// past them.
    fn slots(&self, at: Timestamp) -> Vec<Slot> {
        let mut slots: Vec<Slot> = self
            .quarters
            .iter()
            .map(|q| (q.start, q.start + QUARTER, q.mm))
            .filter(|(_, end, _)| *end > at)
            .collect();
        let after = slots.last().map(|(_, end, _)| *end);
        slots.extend(
            self.hours
                .iter()
                .map(|h| (h.moment.at - HOUR, h.moment.at, h.moment.precipitation))
                .filter(|(start, end, _)| *end > at && after.is_none_or(|after| *start >= after)),
        );
        slots
    }

    /// The rain coming later today at `now` (in the desktop's zone): the
    /// first slot from now to midnight holding 0.1 mm or more, and where
    /// its run of wet slots ends; `start` none while it rains now.
    pub fn rain_today(&self, now: &Zoned) -> Option<Rain> {
        let at = now.timestamp();
        let midnight = now
            .date()
            .tomorrow()
            .ok()?
            .to_zoned(now.time_zone().clone())
            .ok()?
            .timestamp();
        let slots = self.slots(at);
        let first = slots
            .iter()
            .position(|(start, _, mm)| *start < midnight && *mm >= WET)?;
        let (start, _, _) = *slots.get(first)?;
        // The run ends at the first dry slot; past the last slot, the
        // forecast does not say.
        let end = slots
            .iter()
            .skip(first + 1)
            .find(|(_, _, mm)| *mm < WET)
            .map(|(dry, _, _)| *dry);
        let snow = self
            .hour_at(start.max(at))
            .is_some_and(|h| Condition::of(h.moment.code).snow());
        Some(Rain {
            start: (start > at).then_some(start),
            end,
            snow,
        })
    }

    /// The hour slot holding `at` (its hour starting at or before it).
    fn hour_at(&self, at: Timestamp) -> Option<&Hour> {
        self.hours
            .iter()
            .filter(|h| h.moment.at <= at)
            .max_by_key(|h| h.moment.at)
    }

    /// The rain of the `n` quarter hours from the one holding `at` (mm),
    /// 0 where the forecast says nothing.
    pub fn next_quarters(&self, at: Timestamp, n: usize) -> Vec<f32> {
        let first = floor(at, 15);
        (0..n)
            .map(|i| {
                let start = first + QUARTER * i32::try_from(i).unwrap_or(0);
                self.quarters
                    .iter()
                    .find(|q| q.start == start)
                    .map_or(0.0, |q| q.mm)
            })
            .collect()
    }

    /// Whether rain comes (or falls) within the 12 quarter hours from
    /// `at`: the mini form's drops.
    pub fn rain_soon(&self, at: Timestamp) -> bool {
        self.next_quarters(at, 12).iter().any(|mm| *mm >= WET)
    }

    /// The `n` hours after `at`.
    pub fn next_hours(&self, at: Timestamp, n: usize) -> Vec<Hour> {
        self.hours
            .iter()
            .filter(|h| h.moment.at > at)
            .take(n)
            .copied()
            .collect()
    }

    /// The forecast's day `date`.
    pub fn day(&self, date: civil::Date) -> Option<Day> {
        self.days.iter().find(|d| d.date == date).copied()
    }

    /// The days from `date` on, `n` at most.
    pub fn week(&self, date: civil::Date, n: usize) -> Vec<Day> {
        self.days
            .iter()
            .filter(|d| d.date >= date)
            .take(n)
            .copied()
            .collect()
    }
}

/// `at` down to its last multiple of `minutes` (UTC: a quarter hour and an
/// hour are the same in every zone the bar is used in).
fn floor(at: Timestamp, minutes: i64) -> Timestamp {
    let step = minutes * 60;
    let s = at.as_second();
    Timestamp::from_second(s - s.rem_euclid(step)).unwrap_or(at)
}

/// Whether `now` is the evening, when the bar shows tomorrow first.
pub fn evening(now: &Zoned) -> bool {
    now.hour() >= EVENING
}

#[cfg(test)]
mod tests;
