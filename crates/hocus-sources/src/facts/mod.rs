//! Facts computed in code (§2.4, §14): the model reasons on them, it is
//! never asked to find them. [`day`] gathers a day's (public holiday,
//! school holiday of the taught zone, birthdays, free slots) and
//! [`DayFacts::lines`] says them as the context's "the day" part does
//! (§7.3).

pub mod dates;
pub mod holidays;
pub mod school;
pub mod slots;

use jiff::{SignedDuration, civil::Date, tz::TimeZone};
use serde::{Deserialize, Serialize};

use crate::item::{Anniversary, Content, Item, Notebook};
pub use holidays::{Holiday, Region};
pub use school::{SchoolCalendar, SchoolHoliday};
pub use slots::{Hours, Span};

/// What the user taught that facts depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub region: Region,
    /// The school zone, once taught ("Zone C"); `None`: not asked yet.
    pub school_zone: Option<String>,
    pub hours: Option<Hours>,
}

/// A birthday on a day, with the age when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BirthdayFact {
    pub name: String,
    pub kind: Anniversary,
    pub age: Option<i16>,
    pub notebook: Notebook,
}

/// A day's facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayFacts {
    pub date: Date,
    pub holiday: Option<Holiday>,
    pub school_holiday: Option<SchoolHoliday>,
    pub birthdays: Vec<BirthdayFact>,
    /// Free slots of 30 minutes or more in the working hours.
    pub free: Vec<Span>,
}

/// `date`'s facts, from what was read (`items`: events and birthdays of
/// every consented source) and what was taught (`place`).
pub fn day(
    date: Date,
    place: &Place,
    school: Option<&SchoolCalendar>,
    items: &[Item],
    zone: &TimeZone,
) -> DayFacts {
    let holiday = holidays::holiday(date, place.region);
    let school_holiday = match (school, &place.school_zone) {
        (Some(calendar), Some(zone)) => calendar.on(zone, date),
        _ => None,
    };
    let birthdays = items
        .iter()
        .filter_map(|item| match &item.content {
            Content::Birthday(b) => {
                let (on, age) = b.next(date)?;
                (on == date).then(|| BirthdayFact {
                    name: b.name.clone(),
                    kind: b.kind,
                    age,
                    notebook: item.notebook,
                })
            }
            _ => None,
        })
        .collect();
    let free = slots::free(
        &[slots::busy(items)],
        date,
        date,
        place.hours.unwrap_or_default(),
        zone,
        place.region,
        SignedDuration::from_mins(30),
    );
    DayFacts {
        date,
        holiday,
        school_holiday,
        birthdays,
        free,
    }
}

const DAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// "mercredi 11 novembre".
pub fn french_date(date: Date) -> String {
    let weekday = DAYS[usize::try_from(date.weekday().to_monday_zero_offset()).unwrap_or(0)];
    let month = MONTHS[usize::try_from(date.month() - 1).unwrap_or(0)];
    let day = if date.day() == 1 {
        "1er".to_owned()
    } else {
        date.day().to_string()
    };
    format!("{weekday} {day} {month}")
}

impl DayFacts {
    /// The facts as lines of the context, in French, each marked as
    /// computed. `public` leaves perso birthdays out (a meeting, a shared
    /// screen: §7.4).
    pub fn lines(&self, zone: &TimeZone, public: bool) -> Vec<String> {
        let date = french_date(self.date);
        let mut lines = Vec::new();
        match &self.holiday {
            Some(holiday) => lines.push(format!(
                "[fait calculé] {date} : jour férié ({})",
                holiday.name
            )),
            None => lines.push(format!("[fait calculé] {date} : pas un jour férié")),
        }
        if let Some(school) = &self.school_holiday {
            lines.push(format!(
                "[fait calculé] {date} : {} ({}), du {} au {}",
                school.name,
                school.zone,
                french_date(school.first),
                french_date(school.last)
            ));
        }
        for b in self
            .birthdays
            .iter()
            .filter(|b| !public || b.notebook == Notebook::Pro)
        {
            let what = match b.kind {
                Anniversary::Birthday => "anniversaire",
                Anniversary::Anniversary => "anniversaire de mariage",
            };
            match b.age {
                Some(age) => lines.push(format!(
                    "[fait calculé] {date} : {what} de {} ({age} ans)",
                    b.name
                )),
                None => lines.push(format!("[fait calculé] {date} : {what} de {}", b.name)),
            }
        }
        if !self.free.is_empty() {
            let hm = |t: jiff::Timestamp| {
                let z = t.to_zoned(zone.clone());
                format!("{}:{:02}", z.hour(), z.minute())
            };
            let slots: Vec<String> = self
                .free
                .iter()
                .map(|s| format!("{}–{}", hm(s.start), hm(s.end)))
                .collect();
            lines.push(format!(
                "[fait calculé] {date} : créneaux libres {}",
                slots.join(", ")
            ));
        }
        lines
    }
}
