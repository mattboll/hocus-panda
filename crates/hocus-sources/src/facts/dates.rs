//! Dates said in French, resolved in code (§2.4): "demain", "jeudi",
//! "jeudi prochain", "jeudi en huit", "avant vendredi", "d'ici lundi",
//! "la semaine prochaine", "ce week-end", "dans 3 jours", "le 12",
//! "le 12 novembre", "12/11", "fin du mois", "en mars".
//!
//! What French leaves open stays open: "jeudi prochain" said on a Monday
//! may be this week's Thursday or the next one, so both are given
//! ([`Resolved::Ambiguous`]) for her to ask once. Said on a Friday, it can
//! only be next week's.

use std::sync::LazyLock;

use jiff::{ToSpan, civil::Date, civil::Weekday};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

/// What a date said means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolved {
    Day(Date),
    /// Every day from `first` to `last`.
    Days {
        first: Date,
        last: Date,
    },
    /// A deadline: before that day ("avant vendredi"), the day excluded.
    Before(Date),
    /// A deadline: by the end of that day ("d'ici vendredi", "pour
    /// vendredi").
    By(Date),
    /// Two readings; she asks which.
    Ambiguous(Vec<Date>),
}

/// A date found in a text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    /// Its place in the text, in bytes.
    pub start: usize,
    pub end: usize,
    pub resolved: Resolved,
}

const MONTHS: &str = "janvier|février|fevrier|mars|avril|mai|juin|juillet|août|aout|septembre|octobre|novembre|décembre|decembre";

static DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?ix)
        (?:^|[^\p{{L}}\p{{N}}])
        (?:(?P<prep>avant(?:\s+le)?|d['’]ici(?:\s+(?:à|a|au))?|pour|au\s+plus\s+tard|jusqu['’](?:à|a|au))\s+)?
        (?P<expr>
          (?P<today>aujourd['’]hui|ce\s+soir|ce\s+matin|cet\s+après-midi|ce\s+midi)
        | (?P<dayafter>apr[eè]s-demain)
        | (?P<tomorrow>demain)
        | (?P<daybefore>avant-hier)
        | (?P<yesterday>hier)
        | (?P<nextweek>la\s+semaine\s+prochaine)
        | (?P<eow>la\s+fin\s+de\s+(?:la\s+)?semaine|fin\s+de\s+semaine)
        | (?P<thisweek>cette\s+semaine)
        | (?:ce|le)\s+week-?end(?:\s+(?P<wnext>prochain))?
        | (?P<eom>(?:la\s+)?fin\s+du\s+mois)
        | dans\s+(?P<n>\d{{1,3}}|un|une|deux|trois|quatre|cinq|six|sept|huit|dix|quinze)\s+(?P<unit>jours?|semaines?|mois)
        | (?:le\s+)?(?P<numd>\d{{1,2}})/(?P<numm>\d{{1,2}})(?:/(?P<numy>\d{{2}}|\d{{4}}))?
        | le\s+(?P<dom>\d{{1,2}}|1er)(?:\s+(?P<month>{MONTHS})(?:\s+(?P<year>\d{{4}}))?)?
        | (?P<dom2>\d{{1,2}}|1er)\s+(?P<month2>{MONTHS})(?:\s+(?P<year2>\d{{4}}))?
        | en\s+(?P<inmonth>{MONTHS})
        | (?:(?:ce|le)\s+)?(?P<weekday>lundi|mardi|mercredi|jeudi|vendredi|samedi|dimanche)(?:\s+(?P<which>prochain|en\s+huit|dernier|qui\s+vient))?
        )
        (?:$|[^\p{{L}}\p{{N}}])"
    ))
    .expect("a regex")
});

fn weekday(name: &str) -> Weekday {
    match name.to_lowercase().as_str() {
        "lundi" => Weekday::Monday,
        "mardi" => Weekday::Tuesday,
        "mercredi" => Weekday::Wednesday,
        "jeudi" => Weekday::Thursday,
        "vendredi" => Weekday::Friday,
        "samedi" => Weekday::Saturday,
        _ => Weekday::Sunday,
    }
}

fn month(name: &str) -> i8 {
    match name.to_lowercase().as_str() {
        "janvier" => 1,
        "février" | "fevrier" => 2,
        "mars" => 3,
        "avril" => 4,
        "mai" => 5,
        "juin" => 6,
        "juillet" => 7,
        "août" | "aout" => 8,
        "septembre" => 9,
        "octobre" => 10,
        "novembre" => 11,
        _ => 12,
    }
}

fn number(word: &str) -> Option<i64> {
    Some(match word.to_lowercase().as_str() {
        "un" | "une" => 1,
        "deux" => 2,
        "trois" => 3,
        "quatre" => 4,
        "cinq" => 5,
        "six" => 6,
        "sept" => 7,
        "huit" => 8,
        "dix" => 10,
        "quinze" => 15,
        digits => digits.parse().ok()?,
    })
}

fn add(date: Date, days: i64) -> Option<Date> {
    date.checked_add(days.days()).ok()
}

/// The first `day` strictly after `today`.
fn coming(today: Date, day: Weekday) -> Option<Date> {
    let ahead = i64::from(today.weekday().until(day));
    add(today, if ahead == 0 { 7 } else { ahead })
}

/// Monday of `date`'s week.
fn monday(date: Date) -> Option<Date> {
    add(date, -i64::from(date.weekday().to_monday_zero_offset()))
}

/// The day-of-month `day` (of `month`, `year` if given) on or after
/// `today`.
fn on_or_after(today: Date, day: i8, month: Option<i8>, year: Option<i16>) -> Option<Date> {
    match (month, year) {
        (Some(m), Some(y)) => Date::new(y, m, day).ok(),
        (Some(m), None) => {
            let this = Date::new(today.year(), m, day).ok()?;
            if this >= today {
                Some(this)
            } else {
                Date::new(today.year() + 1, m, day).ok()
            }
        }
        _ => {
            // The next month that has that day.
            let mut first = today.first_of_month();
            for _ in 0..13 {
                if let Ok(date) = Date::new(first.year(), first.month(), day)
                    && date >= today
                {
                    return Some(date);
                }
                first = first.checked_add(1.month()).ok()?;
            }
            None
        }
    }
}

fn day_of(c: &Captures<'_>, name: &str) -> Option<i8> {
    let text = c.name(name)?.as_str();
    if text.eq_ignore_ascii_case("1er") {
        Some(1)
    } else {
        text.parse().ok()
    }
}

/// What `c` names, before its preposition.
fn expression(c: &Captures<'_>, today: Date) -> Option<Resolved> {
    use Resolved::{Ambiguous, Day, Days};
    let has = |name: &str| c.name(name).is_some();
    if has("today") {
        return Some(Day(today));
    }
    if has("dayafter") {
        return add(today, 2).map(Day);
    }
    if has("tomorrow") {
        return add(today, 1).map(Day);
    }
    if has("daybefore") {
        return add(today, -2).map(Day);
    }
    if has("yesterday") {
        return add(today, -1).map(Day);
    }
    if has("nextweek") {
        let first = add(monday(today)?, 7)?;
        return Some(Days {
            first,
            last: add(first, 6)?,
        });
    }
    if has("eow") {
        let friday = add(monday(today)?, 4)?;
        return Some(Day(friday.max(today)));
    }
    if has("thisweek") {
        return Some(Days {
            first: today,
            last: add(monday(today)?, 6)?,
        });
    }
    if let Some(expr) = c.name("expr")
        && expr.as_str().to_lowercase().contains("week")
    {
        let saturday = add(monday(today)?, 5)?;
        let saturday = if has("wnext") {
            add(saturday, 7)?
        } else {
            saturday
        };
        let first = saturday.max(today);
        return Some(Days {
            first,
            last: add(saturday, 1)?,
        });
    }
    if has("eom") {
        return Some(Day(today.last_of_month()));
    }
    if let (Some(n), Some(unit)) = (c.name("n"), c.name("unit")) {
        let n = number(n.as_str())?;
        let unit = unit.as_str().to_lowercase();
        return if unit.starts_with("jour") {
            add(today, n).map(Day)
        } else if unit.starts_with("semaine") {
            add(today, 7 * n).map(Day)
        } else {
            today.checked_add(n.months()).ok().map(Day)
        };
    }
    if let Some(day) = day_of(c, "dom") {
        let month = c.name("month").map(|m| month(m.as_str()));
        let year = c.name("year").and_then(|y| y.as_str().parse().ok());
        return on_or_after(today, day, month, year).map(Day);
    }
    if let Some(day) = day_of(c, "dom2") {
        let month = c.name("month2").map(|m| month(m.as_str()));
        let year = c.name("year2").and_then(|y| y.as_str().parse().ok());
        return on_or_after(today, day, month, year).map(Day);
    }
    if let (Some(d), Some(m)) = (c.name("numd"), c.name("numm")) {
        let (d, m): (i8, i8) = (d.as_str().parse().ok()?, m.as_str().parse().ok()?);
        let year = c.name("numy").and_then(|y| {
            let y: i16 = y.as_str().parse().ok()?;
            Some(if y < 100 { 2000 + y } else { y })
        });
        return on_or_after(today, d, Some(m), year).map(Day);
    }
    if let Some(m) = c.name("inmonth") {
        let m = month(m.as_str());
        let year = if m >= today.month() {
            today.year()
        } else {
            today.year() + 1
        };
        let first = Date::new(year, m, 1).ok()?;
        return Some(Days {
            first: first.max(today),
            last: first.last_of_month(),
        });
    }
    if let Some(name) = c.name("weekday") {
        let day = weekday(name.as_str());
        let next = coming(today, day)?;
        let which = c.name("which").map(|w| w.as_str().to_lowercase());
        return match which.as_deref() {
            Some("dernier") => add(next, -7).map(Day),
            Some(w) if w.starts_with("en") => add(next, 7).map(Day),
            Some("prochain") => {
                // This week's, or the next one's: open when this week's is
                // still to come.
                let same_week = monday(next)? == monday(today)?;
                if same_week {
                    Some(Ambiguous(vec![next, add(next, 7)?]))
                } else {
                    Some(Day(next))
                }
            }
            _ => Some(Day(next)),
        };
    }
    None
}

/// The dates said in `text`, resolved from `today`.
pub fn resolve(text: &str, today: Date) -> Vec<Said> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(c) = DATE.captures_at(text, at) {
        let (Some(whole), Some(expr)) = (c.get(0), c.name("expr")) else {
            break;
        };
        at = expr.end().max(at + 1);
        let start = c.name("prep").map_or(expr.start(), |p| p.start());
        let Some(resolved) = expression(&c, today) else {
            continue;
        };
        let resolved = match (c.name("prep").map(|p| p.as_str().to_lowercase()), resolved) {
            (None, resolved) => resolved,
            (Some(prep), Resolved::Day(day)) if prep.starts_with("avant") => Resolved::Before(day),
            (Some(prep), Resolved::Days { first, .. }) if prep.starts_with("avant") => {
                Resolved::Before(first)
            }
            (Some(_), Resolved::Day(day)) => Resolved::By(day),
            (Some(_), Resolved::Days { last, .. }) => Resolved::By(last),
            (Some(_), other) => other,
        };
        out.push(Said {
            start,
            end: expr.end(),
            resolved,
        });
        if whole.end() <= at {
            continue;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn one(text: &str, today: Date) -> Resolved {
        let said = resolve(text, today);
        assert_eq!(said.len(), 1, "{text}: {said:?}");
        said[0].resolved.clone()
    }

    // Monday 9 November 2026.
    const MONDAY: Date = date(2026, 11, 9);

    #[test]
    fn days_said_relative_to_today() {
        assert_eq!(one("demain", MONDAY), Resolved::Day(date(2026, 11, 10)));
        assert_eq!(
            one("après-demain", MONDAY),
            Resolved::Day(date(2026, 11, 11))
        );
        assert_eq!(
            one("on se voit jeudi ?", MONDAY),
            Resolved::Day(date(2026, 11, 12))
        );
        assert_eq!(
            one("dans 3 jours", MONDAY),
            Resolved::Day(date(2026, 11, 12))
        );
        assert_eq!(
            one("dans deux semaines", MONDAY),
            Resolved::Day(date(2026, 11, 23))
        );
    }

    #[test]
    fn jeudi_prochain_is_open_early_in_the_week_and_not_after() {
        assert_eq!(
            one("jeudi prochain", MONDAY),
            Resolved::Ambiguous(vec![date(2026, 11, 12), date(2026, 11, 19)])
        );
        // Said on Friday 13: only next week's.
        assert_eq!(
            one("jeudi prochain", date(2026, 11, 13)),
            Resolved::Day(date(2026, 11, 19))
        );
        assert_eq!(
            one("jeudi en huit", MONDAY),
            Resolved::Day(date(2026, 11, 19))
        );
    }

    #[test]
    fn deadlines() {
        assert_eq!(
            one("avant vendredi", MONDAY),
            Resolved::Before(date(2026, 11, 13))
        );
        assert_eq!(
            one("d'ici vendredi", MONDAY),
            Resolved::By(date(2026, 11, 13))
        );
        assert_eq!(
            one("pour la semaine prochaine", MONDAY),
            Resolved::By(date(2026, 11, 22))
        );
        assert_eq!(
            one("avant la fin du mois", MONDAY),
            Resolved::Before(date(2026, 11, 30))
        );
    }

    #[test]
    fn dates_of_the_calendar() {
        assert_eq!(one("le 12", MONDAY), Resolved::Day(date(2026, 11, 12)));
        assert_eq!(one("le 3", MONDAY), Resolved::Day(date(2026, 12, 3)));
        assert_eq!(one("le 2 mars", MONDAY), Resolved::Day(date(2027, 3, 2)));
        assert_eq!(
            one("rendez-vous 1er décembre", MONDAY),
            Resolved::Day(date(2026, 12, 1))
        );
        assert_eq!(one("le 24/12", MONDAY), Resolved::Day(date(2026, 12, 24)));
        assert_eq!(
            one("en mars", MONDAY),
            Resolved::Days {
                first: date(2027, 3, 1),
                last: date(2027, 3, 31)
            }
        );
        assert_eq!(
            one("ce week-end", MONDAY),
            Resolved::Days {
                first: date(2026, 11, 14),
                last: date(2026, 11, 15)
            }
        );
    }

    #[test]
    fn numbers_that_are_not_dates_stay_so() {
        assert!(resolve("la v2 coûte 12 euros", MONDAY).is_empty());
    }
}
