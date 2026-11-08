//! French public holidays, computed (§2.4): the fixed ones, the ones
//! Easter moves (Easter Monday, Ascension, Whit Monday), Alsace-Moselle's
//! two more, and each overseas department's abolition of slavery.

use jiff::{ToSpan, civil::Date};
use serde::{Deserialize, Serialize};

/// Where the user lives, for its holidays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Region {
    #[default]
    Metropole,
    /// Bas-Rhin, Haut-Rhin, Moselle: Good Friday and 26 December too.
    AlsaceMoselle,
    Guadeloupe,
    Martinique,
    Guyane,
    Reunion,
    Mayotte,
}

/// A public holiday.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Holiday {
    pub date: Date,
    /// Its name, in French, as the user reads it.
    pub name: &'static str,
}

/// Easter Sunday of `year` (the Gregorian computus, Meeus/Jones/Butcher).
pub fn easter(year: i16) -> Date {
    let y = i32::from(year);
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    // Both within range for any year jiff takes.
    jiff::civil::date(year, month as i8, day as i8)
}

/// The public holidays of `year` in `region`, in order.
pub fn holidays(year: i16, region: Region) -> Vec<Holiday> {
    let on = |month: i8, day: i8, name: &'static str| Holiday {
        date: jiff::civil::date(year, month, day),
        name,
    };
    let easter = easter(year);
    let after = |days: i64, name: &'static str| Holiday {
        date: easter.checked_add(days.days()).unwrap_or(easter),
        name,
    };
    let mut days = vec![
        on(1, 1, "Jour de l'an"),
        after(1, "Lundi de Pâques"),
        on(5, 1, "Fête du Travail"),
        on(5, 8, "Victoire 1945"),
        after(39, "Ascension"),
        after(50, "Lundi de Pentecôte"),
        on(7, 14, "Fête nationale"),
        on(8, 15, "Assomption"),
        on(11, 1, "Toussaint"),
        on(11, 11, "Armistice 1918"),
        on(12, 25, "Noël"),
    ];
    match region {
        Region::Metropole => {}
        Region::AlsaceMoselle => {
            days.push(after(-2, "Vendredi saint"));
            days.push(on(12, 26, "Saint-Étienne"));
        }
        Region::Guadeloupe => days.push(on(5, 27, "Abolition de l'esclavage")),
        Region::Martinique => days.push(on(5, 22, "Abolition de l'esclavage")),
        Region::Guyane => days.push(on(6, 10, "Abolition de l'esclavage")),
        Region::Reunion => days.push(on(12, 20, "Abolition de l'esclavage")),
        Region::Mayotte => days.push(on(4, 27, "Abolition de l'esclavage")),
    }
    days.sort_by_key(|h| h.date);
    days
}

/// The holiday on `date`, if it is one.
pub fn holiday(date: Date, region: Region) -> Option<Holiday> {
    holidays(date.year(), region)
        .into_iter()
        .find(|h| h.date == date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn easter_falls_where_the_calendar_says() {
        assert_eq!(easter(2024), date(2024, 3, 31));
        assert_eq!(easter(2025), date(2025, 4, 20));
        assert_eq!(easter(2026), date(2026, 4, 5));
        assert_eq!(easter(2027), date(2027, 3, 28));
        assert_eq!(easter(2038), date(2038, 4, 25));
    }

    #[test]
    fn the_holidays_easter_moves_follow_it() {
        let names = |d: Date| holiday(d, Region::Metropole).map(|h| h.name);
        assert_eq!(names(date(2026, 4, 6)), Some("Lundi de Pâques"));
        assert_eq!(names(date(2026, 5, 14)), Some("Ascension"));
        assert_eq!(names(date(2026, 5, 25)), Some("Lundi de Pentecôte"));
        assert_eq!(names(date(2026, 11, 11)), Some("Armistice 1918"));
        assert_eq!(names(date(2026, 4, 3)), None);
        assert_eq!(
            holiday(date(2026, 4, 3), Region::AlsaceMoselle).map(|h| h.name),
            Some("Vendredi saint")
        );
        assert_eq!(holidays(2026, Region::Metropole).len(), 11);
    }
}
