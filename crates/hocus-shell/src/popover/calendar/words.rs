//! What the calendar says, in French and in English (Day.5): the month,
//! the days' letters, the day's line, the counts, the notes, the spans of
//! days and the times, as the system's clock writes them.

use hocus_words::shell::popover_calendar_words as txt;
use jiff::civil::{Date, Time, Weekday};

use crate::clock::time::Lang;

/// The calendar's words in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

const MONTHS_FR: [&str; 12] = [
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
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// The months as a span of days writes them ("7 – 8 nov.").
const SHORT_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];
const SHORT_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS_FR: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const DAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

fn month_index(month: i8) -> usize {
    usize::from(month.unsigned_abs().clamp(1, 12)) - 1
}

fn day_index(day: Weekday) -> usize {
    usize::from(day.to_monday_zero_offset().unsigned_abs())
}

/// The first letter of `word` in capitals.
fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    fn month(self, month: i8) -> &'static str {
        if self.lang == Lang::French {
            MONTHS_FR[month_index(month)]
        } else {
            MONTHS_EN[month_index(month)]
        }
    }

    fn short_month(self, month: i8) -> &'static str {
        if self.lang == Lang::French {
            SHORT_FR[month_index(month)]
        } else {
            SHORT_EN[month_index(month)]
        }
    }

    fn weekday(self, day: Weekday) -> &'static str {
        if self.lang == Lang::French {
            DAYS_FR[day_index(day)]
        } else {
            DAYS_EN[day_index(day)]
        }
    }

    /// The header's month: "Novembre 2026", "November 2026".
    pub fn month_title(self, month: Date) -> String {
        format!(
            "{} {}",
            capitalised(self.month(month.month())),
            month.year()
        )
    }

    /// The columns' letters, Monday first.
    pub fn letters(self) -> [&'static str; 7] {
        if self.lang == Lang::French {
            ["L", "M", "M", "J", "V", "S", "D"]
        } else {
            ["M", "T", "W", "T", "F", "S", "S"]
        }
    }

    /// Over the week numbers.
    pub fn week(self) -> &'static str {
        self.pick(txt::WK)
    }

    pub fn today_button(self) -> &'static str {
        self.pick(txt::TODAY)
    }

    pub fn previous(self) -> &'static str {
        self.pick(txt::PREVIOUS_MONTH)
    }

    pub fn next(self) -> &'static str {
        self.pick(txt::NEXT_MONTH)
    }

    /// The day's line: "Mercredi 4 novembre", "Wednesday 4 November", with
    /// its year when it is not `this_year`.
    pub fn day_title(self, day: Date, this_year: i16) -> String {
        let mut title = format!(
            "{} {} {}",
            capitalised(self.weekday(day.weekday())),
            day.day(),
            self.month(day.month())
        );
        if day.year() != this_year {
            title.push_str(&format!(" {}", day.year()));
        }
        title
    }

    /// After the day's name, when it is near `today`.
    pub fn relative(self, day: Date, today: Date) -> Option<&'static str> {
        if day == today {
            Some(self.pick(txt::TODAY_2))
        } else if today.tomorrow().ok() == Some(day) {
            Some(self.pick(txt::TOMORROW))
        } else if today.yesterday().ok() == Some(day) {
            Some(self.pick(txt::YESTERDAY))
        } else {
            None
        }
    }

    /// "6 événements", "1 événement"; nothing on an empty day.
    pub fn count(self, n: usize) -> Option<String> {
        match n {
            0 => None,
            1 => Some(self.pick(txt::N_1_EVENT).to_owned()),
            n => Some(format!("{n} {}", self.pick(txt::EVENTS))),
        }
    }

    pub fn all_day(self) -> &'static str {
        self.pick(txt::ALL_DAY)
    }

    /// An event of several days: "7 – 8 nov.", "30 nov. – 2 déc.";
    /// "7 – 8 Nov", "30 Nov – 2 Dec".
    pub fn span(self, first: Date, last: Date) -> String {
        if first.year() == last.year() && first.month() == last.month() {
            format!(
                "{} – {} {}",
                first.day(),
                last.day(),
                self.short_month(last.month())
            )
        } else {
            format!(
                "{} {} – {} {}",
                first.day(),
                self.short_month(first.month()),
                last.day(),
                self.short_month(last.month())
            )
        }
    }

    pub fn empty(self) -> &'static str {
        self.pick(txt::NOTHING_PLANNED_THAT_DAY)
    }

    /// No calendar account in the configuration (Day.1 not set up).
    pub fn no_source(self) -> &'static str {
        self.pick(txt::NO_CALENDAR_CONNECTED)
    }

    /// The button under it: Day.1's popover.
    pub fn choose_calendars(self) -> &'static str {
        self.pick(txt::CHOOSE_CALENDARS)
    }

    /// A day outside the window the shell keeps (2 months back to 12
    /// ahead), its month asked of the accounts, not answered yet.
    pub fn loading(self) -> &'static str {
        self.pick(txt::LOADING)
    }

    /// A day outside the window the shell keeps, its month not given by
    /// an account (offline, say).
    pub fn not_loaded(self) -> &'static str {
        self.pick(txt::THIS_DAY_IS_NOT_LOADED)
    }

    /// A day's name for screen readers: "mercredi 4 novembre, 6
    /// événements, aujourd'hui", "Wednesday 4 November, 6 events, today".
    pub fn day_name(self, day: Date, events: usize, today: Date) -> String {
        let mut name = format!(
            "{} {} {}",
            self.weekday(day.weekday()),
            day.day(),
            self.month(day.month())
        );
        match self.count(events) {
            Some(count) => name.push_str(&format!(", {count}")),
            None => name.push_str(self.pick(txt::NOTHING_PLANNED)),
        }
        if day == today {
            name.push_str(self.pick(txt::TODAY_3));
        }
        name
    }
}

/// How the system's clock writes the time: 24 hours, or 12 with AM and
/// PM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hours {
    TwentyFour,
    Twelve,
}

impl Hours {
    /// As the locale of the time (`LC_ALL`, `LC_TIME`, `LANG`) says: 24
    /// hours in French and in most of the world; 12 in the English of the
    /// countries that write it so.
    /// `[locale] time` when the file says it (#402), for the whole
    /// desktop: the bar's clock, the calendar, the reminders, the lock.
    pub fn from_env() -> Self {
        if let Some(hours) = crate::locale::chosen_hours() {
            return hours;
        }
        let locale = ["LC_ALL", "LC_TIME", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::of(&locale)
    }

    /// From a locale's name, `en_US.UTF-8`.
    pub fn of(locale: &str) -> Self {
        let name = locale.split(['.', '@']).next().unwrap_or("");
        match name {
            "en_US" | "en_CA" | "en_AU" | "en_NZ" | "en_PH" | "en_IN" => Self::Twelve,
            _ => Self::TwentyFour,
        }
    }

    /// `time` as the clock writes it: "09:30", "9:30 AM".
    pub fn write(self, time: Time) -> String {
        match self {
            Self::TwentyFour => format!("{:02}:{:02}", time.hour(), time.minute()),
            Self::Twelve => {
                let (hour, half) = match time.hour() {
                    0 => (12, "AM"),
                    h @ 1..=11 => (h, "AM"),
                    12 => (12, "PM"),
                    h => (h - 12, "PM"),
                };
                format!("{hour}:{:02} {half}", time.minute())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::{date, time};

    use super::*;

    #[test]
    fn the_words_in_french_and_in_english() {
        let (fr, en) = (Words::new(Lang::French), Words::new(Lang::English));
        let day = date(2026, 11, 4);
        assert_eq!(fr.month_title(day), "Novembre 2026");
        assert_eq!(en.month_title(day), "November 2026");
        assert_eq!(fr.day_title(day, 2026), "Mercredi 4 novembre");
        assert_eq!(en.day_title(day, 2026), "Wednesday 4 November");
        assert_eq!(
            fr.day_title(date(2027, 1, 6), 2026),
            "Mercredi 6 janvier 2027"
        );
        assert_eq!(fr.relative(day, day), Some("aujourd'hui"));
        assert_eq!(en.relative(date(2026, 11, 5), day), Some("tomorrow"));
        assert_eq!(fr.relative(date(2026, 11, 3), day), Some("hier"));
        assert_eq!(fr.relative(date(2026, 11, 6), day), None);
        assert_eq!(fr.count(6).as_deref(), Some("6 événements"));
        assert_eq!(fr.count(1).as_deref(), Some("1 événement"));
        assert_eq!(en.count(1).as_deref(), Some("1 event"));
        assert_eq!(en.count(0), None);
        assert_eq!(fr.loading(), "Chargement…");
        assert_eq!(en.loading(), "Loading…");
        assert_eq!(fr.letters().join(" "), "L M M J V S D");
        assert_eq!(en.letters().join(" "), "M T W T F S S");
        assert_eq!(fr.span(date(2026, 11, 7), date(2026, 11, 8)), "7 – 8 nov.");
        assert_eq!(en.span(date(2026, 11, 7), date(2026, 11, 8)), "7 – 8 Nov");
        assert_eq!(
            fr.span(date(2026, 11, 30), date(2026, 12, 2)),
            "30 nov. – 2 déc."
        );
        assert_eq!(
            fr.day_name(day, 6, day),
            "mercredi 4 novembre, 6 événements, aujourd'hui"
        );
        assert_eq!(
            en.day_name(day, 6, day),
            "Wednesday 4 November, 6 events, today"
        );
        assert_eq!(
            fr.day_name(date(2026, 11, 13), 0, day),
            "vendredi 13 novembre, rien de prévu"
        );
    }

    #[test]
    fn times_follow_the_locales_clock() {
        assert_eq!(Hours::of("fr_FR.UTF-8"), Hours::TwentyFour);
        assert_eq!(Hours::of("en_GB.UTF-8"), Hours::TwentyFour);
        assert_eq!(Hours::of("en_US.UTF-8"), Hours::Twelve);
        assert_eq!(Hours::of("C"), Hours::TwentyFour);
        assert_eq!(Hours::TwentyFour.write(time(9, 30, 0, 0)), "09:30");
        assert_eq!(Hours::Twelve.write(time(9, 30, 0, 0)), "9:30 AM");
        assert_eq!(Hours::Twelve.write(time(0, 5, 0, 0)), "12:05 AM");
        assert_eq!(Hours::Twelve.write(time(12, 0, 0, 0)), "12:00 PM");
        assert_eq!(Hours::Twelve.write(time(17, 45, 0, 0)), "5:45 PM");
    }
}
