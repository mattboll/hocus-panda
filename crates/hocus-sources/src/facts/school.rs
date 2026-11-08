//! School holidays by zone (§14, AC42), from the Ministry of Education's
//! open data calendar (`fr-en-calendrier-scolaire` on
//! data.education.gouv.fr, Licence Ouverte v2.0 / Etalab: reuse allowed
//! with the source named). Fetched on a worker, at most once a month, and
//! cached; read from the cache meanwhile and offline.
//!
//! The user's zone is taught once ("on est en zone C", or an académie:
//! "Lyon") and kept in her memory as a rule; this module only resolves it
//! and answers which days are holidays there.
//!
//! The dataset gives each holiday as two instants in UTC: its start, the
//! midnight in Paris of its first day off, and its end, the midnight in
//! Paris of the day classes start again. A bridge of one day ("Pont de
//! l'Ascension") has both the same.

use std::{path::Path, time::Duration};

use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use serde::{Deserialize, Serialize};

use crate::{Result, SourceError};

/// The dataset's export, as JSON.
pub const API: &str = "https://data.education.gouv.fr/api/explore/v2.1/catalog/datasets/fr-en-calendrier-scolaire/exports/json";
/// For tests only: the dataset at an address on this machine.
pub const API_ENV: &str = "HOCUS_TEST_SCHOOL_CALENDAR";
/// How old a cache may get before it is fetched again.
pub const FRESH: Duration = Duration::from_secs(30 * 24 * 3600);
/// What is said of where the data comes from, as the licence asks.
pub const ATTRIBUTION: &str = "Calendrier scolaire : ministère de l'Éducation nationale, data.education.gouv.fr (Licence Ouverte v2.0)";

/// A record of the dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub description: String,
    /// "-" (everyone), "Élèves" or "Enseignants".
    #[serde(default)]
    pub population: String,
    pub start_date: String,
    pub end_date: String,
    /// The académie (or the territory: "Corse", "Réunion").
    pub location: String,
    /// "Zone A", "Zone B", "Zone C", or a territory.
    pub zones: String,
    #[serde(default)]
    pub annee_scolaire: String,
}

/// A school holiday of a zone, its days off from `first` to `last`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchoolHoliday {
    pub name: String,
    pub zone: String,
    pub first: Date,
    pub last: Date,
}

/// The school calendar, every zone's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchoolCalendar {
    records: Vec<Record>,
    /// When it was fetched.
    pub fetched: Option<Timestamp>,
}

/// Lower case, accents off, dashes and apostrophes as spaces.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'À' | 'Â' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => 'e',
            'î' | 'ï' | 'Î' => 'i',
            'ô' | 'ö' | 'Ô' => 'o',
            'ù' | 'û' | 'ü' | 'Û' => 'u',
            'ç' | 'Ç' => 'c',
            '-' | '\'' | '’' => ' ',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl SchoolCalendar {
    pub fn new(records: Vec<Record>, fetched: Option<Timestamp>) -> Self {
        Self { records, fetched }
    }

    /// The zone a user names: "zone C", "C", an académie ("Lyon",
    /// "Aix-Marseille") or a territory ("Corse", "La Réunion"). `None`
    /// when the calendar knows none such.
    pub fn zone(&self, named: &str) -> Option<String> {
        let named = fold(named);
        let named = named.strip_prefix("la ").unwrap_or(&named);
        let named = named.strip_prefix("academie de ").unwrap_or(named);
        let letter = named.strip_prefix("zone ").unwrap_or(named);
        if letter.len() == 1 {
            let zone = format!("zone {letter}");
            return self
                .records
                .iter()
                .find(|r| fold(&r.zones) == zone)
                .map(|r| r.zones.clone());
        }
        self.records
            .iter()
            .find(|r| fold(&r.zones) == named || fold(&r.location) == named)
            .map(|r| r.zones.clone())
    }

    /// `zone`'s holidays (pupils'), in order, each once.
    pub fn holidays(&self, zone: &str) -> Vec<SchoolHoliday> {
        let paris = TimeZone::get("Europe/Paris").unwrap_or(TimeZone::UTC);
        let day = |instant: &str| -> Option<Date> {
            let at: Timestamp = instant.parse().ok()?;
            Some(at.to_zoned(paris.clone()).date())
        };
        let mut out: Vec<SchoolHoliday> = self
            .records
            .iter()
            .filter(|r| r.zones == zone && r.population != "Enseignants")
            .filter_map(|r| {
                let first = day(&r.start_date)?;
                let back = day(&r.end_date)?;
                let last = back.checked_sub(1.day()).ok()?.max(first);
                Some(SchoolHoliday {
                    name: r.description.clone(),
                    zone: r.zones.clone(),
                    first,
                    last,
                })
            })
            .collect();
        out.sort_by(|a, b| (a.first, a.last, &a.name).cmp(&(b.first, b.last, &b.name)));
        out.dedup();
        out
    }

    /// The holiday `date` falls in, in `zone`.
    pub fn on(&self, zone: &str, date: Date) -> Option<SchoolHoliday> {
        self.holidays(zone)
            .into_iter()
            .find(|h| h.first <= date && date <= h.last)
    }

    /// The next holiday of `zone` starting after `date`.
    pub fn next(&self, zone: &str, date: Date) -> Option<SchoolHoliday> {
        self.holidays(zone).into_iter().find(|h| h.first > date)
    }

    /// The dataset's JSON export read.
    pub fn from_json(json: &str, fetched: Option<Timestamp>) -> Result<Self> {
        let records: Vec<Record> = serde_json::from_str(json).map_err(|err| {
            SourceError::Invalid(format!("the school calendar does not read: {err}"))
        })?;
        Ok(Self::new(records, fetched))
    }

    /// The cache at `path`, if it reads.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Written to `path`, atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::consent::write_atomically(path, &serde_json::to_vec(self)?)
    }

    /// Whether it should be fetched again at `now`.
    pub fn stale(&self, now: Timestamp) -> bool {
        self.fetched
            .is_none_or(|at| now.duration_since(at).unsigned_abs().ge(&FRESH))
    }
}

/// The dataset's address `test` names when it is on this machine, else
/// the ministry's.
pub fn api(test: Option<&str>) -> String {
    match test {
        Some(test) if crate::http::local(test) => test.to_owned(),
        _ => API.to_owned(),
    }
}

/// The school years around `today` ("2026-2027" and the next).
fn years(today: Date) -> [String; 2] {
    let start = if today.month() >= 8 {
        today.year()
    } else {
        today.year() - 1
    };
    [
        format!("{}-{}", start, start + 1),
        format!("{}-{}", start + 1, start + 2),
    ]
}

/// The calendar of this school year and the next, fetched from `api`.
/// Blocking: on a worker.
pub fn fetch(api: &str, today: Date, now: Timestamp) -> Result<SchoolCalendar> {
    let [this, next] = years(today);
    let filter = format!("annee_scolaire in (\"{this}\",\"{next}\")");
    let agent = crate::http::agent(Duration::from_secs(30));
    let answer = agent
        .get(api)
        .query("where", &filter)
        .query(
            "select",
            "description,population,start_date,end_date,location,zones,annee_scolaire",
        )
        .call();
    let (status, body) = crate::http::read(answer, "the school calendar", 8 * 1024 * 1024)?;
    if status != 200 {
        return Err(SourceError::Offline(format!(
            "the school calendar answers {status}"
        )));
    }
    SchoolCalendar::from_json(&body, Some(now))
}

/// The calendar: the cache at `cache` while fresh, else fetched and
/// cached; the stale cache when the fetch fails.
pub fn cached(api: &str, cache: &Path, today: Date, now: Timestamp) -> Result<SchoolCalendar> {
    let kept = SchoolCalendar::load(cache);
    if let Some(kept) = &kept
        && !kept.stale(now)
    {
        return Ok(kept.clone());
    }
    match fetch(api, today, now) {
        Ok(fetched) => {
            if let Err(err) = fetched.save(cache) {
                tracing::warn!(%err, path = %cache.display(), "the school calendar is not cached");
            }
            Ok(fetched)
        }
        Err(err) => match kept {
            Some(kept) => {
                tracing::info!(%err, "the school calendar is not fetched: the cache as it was");
                Ok(kept)
            }
            None => Err(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn calendar() -> SchoolCalendar {
        let json = include_str!("../../tests/fixtures/calendrier-scolaire.json");
        SchoolCalendar::from_json(json, None).expect("the fixture reads")
    }

    #[test]
    fn a_zone_is_named_by_its_letter_or_an_academie() {
        let calendar = calendar();
        assert_eq!(calendar.zone("zone C").as_deref(), Some("Zone C"));
        assert_eq!(calendar.zone("c").as_deref(), Some("Zone C"));
        assert_eq!(calendar.zone("Lyon").as_deref(), Some("Zone A"));
        assert_eq!(
            calendar.zone("académie de Rennes").as_deref(),
            Some("Zone B")
        );
        assert_eq!(calendar.zone("Corse").as_deref(), Some("Corse"));
        assert_eq!(calendar.zone("Atlantide"), None);
    }

    #[test]
    fn a_holiday_runs_from_its_first_day_off_to_the_day_before_school() {
        let calendar = calendar();
        let christmas = calendar.on("Zone C", date(2026, 12, 25)).expect("Noël");
        assert_eq!(christmas.first, date(2026, 12, 19));
        assert_eq!(christmas.last, date(2027, 1, 3));
        assert_eq!(calendar.on("Zone C", date(2027, 1, 4)), None);
        // Winter holidays differ by zone.
        assert!(calendar.on("Zone C", date(2027, 2, 8)).is_some());
        assert!(calendar.on("Zone A", date(2027, 2, 8)).is_none());
        // A bridge of one day.
        let bridge = calendar.on("Zone A", date(2027, 5, 7)).expect("the bridge");
        assert_eq!(
            (bridge.first, bridge.last),
            (date(2027, 5, 7), date(2027, 5, 7))
        );
    }

    #[test]
    fn the_school_years_follow_the_rentree() {
        assert_eq!(years(date(2026, 11, 8)), ["2026-2027", "2027-2028"]);
        assert_eq!(years(date(2027, 3, 1)), ["2026-2027", "2027-2028"]);
    }
}
