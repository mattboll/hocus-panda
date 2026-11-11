//! The Date et heure page's model (6.11, settings.md "Date et heure"):
//! what the search finds on it, the time zones the picker offers, and how
//! the page says a zone, its offset, its next clock change and the time.
//! Nothing of it is in the file: the time, the zone and the automatic time
//! are systemd's timedated's ([`crate::timedate`]); their format is 6.2's
//! `[locale] time` and `[locale] date`, only read here. The window's side
//! is `app::time`.
//!
//! The zones are tzdata's `zone.tab`, each with its city's and its
//! country's names in French and English from CLDR
//! (`data/zones/zones.tsv`, made by `scripts/zones/generate.py`); their
//! offsets and changes are computed with jiff from the system's tzdata.

use crate::lang::French as _;
use hocus_words::settings::time as words;
use std::sync::OnceLock;

use jiff::{
    Timestamp, Zoned,
    civil::{Date, Time},
    tz::{Dst, Offset, TimeZone},
};

use crate::{
    lang::{Lang, Words, w},
    model::{Outside, SectionId},
};

const TIME: Words = w(words::TIME);
const ZONE_GROUP: Words = w(words::TIME_ZONE);
const FORMAT_GROUP: Words = w(words::FORMAT);

/// The rows, by their search ids.
pub const AUTO: &str = "time.auto";
pub const SET: &str = "time.set";
pub const ZONE: &str = "time.zone";
pub const FORMAT: &str = "time.format";

/// How many zones the picker lists.
pub const LISTED: usize = 8;

/// The page's settings, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const fn o(
        id: &'static str,
        group: Words,
        name: Words,
        fr: &'static [&'static str],
        en: &'static [&'static str],
    ) -> Outside {
        Outside {
            id,
            section: SectionId::DateTime,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 4] = [
        o(
            AUTO,
            TIME,
            w(words::SET_THE_TIME_AUTOMATICALLY),
            &["ntp", "synchroniser", "internet"],
            &["automatic time", "sync", "ntp"],
        ),
        o(
            SET,
            TIME,
            w(words::DATE_AND_TIME),
            &["régler", "changer l’heure"],
            &["set time", "clock"],
        ),
        o(
            ZONE,
            ZONE_GROUP,
            w(words::TIME_ZONE_2),
            &["pays", "ville", "utc", "gmt", "décalage"],
            &["time zone", "timezone", "offset"],
        ),
        o(
            FORMAT,
            FORMAT_GROUP,
            w(words::TIME_FORMAT),
            &["12 h", "24 h", "am", "pm", "format de la date"],
            &["time format", "date format"],
        ),
    ];
    &ALL
}

// --- The zones -----------------------------------------------------------------------

static TABLE: &str = include_str!("../data/zones/zones.tsv");

/// A city the picker offers: its zone, its country, its names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct City {
    pub zone: &'static str,
    pub country: &'static str,
    pub fr: &'static str,
    pub en: &'static str,
}

impl City {
    pub fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::French => self.fr,
            Lang::English => self.en,
        }
    }

    /// "Montréal, Canada"; "Monaco" alone when the city is its country.
    pub fn said(&self, lang: Lang) -> String {
        let city = self.name(lang);
        match country_name(self.country, lang) {
            Some(country) if country != city => format!("{city}, {country}"),
            _ => city.to_owned(),
        }
    }
}

struct Table {
    cities: Vec<City>,
    /// (code, French, English).
    countries: Vec<(&'static str, &'static str, &'static str)>,
}

fn table() -> &'static Table {
    static PARSED: OnceLock<Table> = OnceLock::new();
    PARSED.get_or_init(|| {
        let mut cities = Vec::new();
        let mut countries = Vec::new();
        for line in TABLE.lines() {
            let fields: Vec<&'static str> = line.split('\t').collect();
            match fields.as_slice() {
                ["Z", zone, country, fr, en] => cities.push(City {
                    zone,
                    country,
                    fr,
                    en,
                }),
                ["C", code, fr, en] => countries.push((*code, *fr, *en)),
                _ => {}
            }
        }
        Table { cities, countries }
    })
}

/// Every city the picker offers, the zones' own first.
pub fn cities() -> &'static [City] {
    &table().cities
}

/// A country's name, by its ISO code.
pub fn country_name(code: &str, lang: Lang) -> Option<&'static str> {
    table()
        .countries
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, fr, en)| if lang.fr() { *fr } else { *en })
}

/// The zone's own city (`Europe/Paris`: Paris).
pub fn city_of(zone: &str) -> Option<&'static City> {
    cities().iter().find(|c| c.zone == zone)
}

/// The zone named `id`, from the system's tzdata.
pub fn zone(id: &str) -> Option<TimeZone> {
    TimeZone::get(id).ok()
}

// --- Saying them ---------------------------------------------------------------------

/// "UTC+01:00", "UTC−05:00" (a true minus), "UTC+05:30".
pub fn say_offset(offset: Offset) -> String {
    let seconds = offset.seconds();
    let sign = if seconds < 0 { '−' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// "jeudi 5 novembre 2026", "Thursday 5 November 2026".
pub fn say_date(date: Date, lang: Lang) -> String {
    format!(
        "{} {}",
        crate::appearance::today_as("long", date, lang),
        date.year()
    )
}

/// "28 mars", "28 March": a day of the coming year.
fn say_day(date: Date, lang: Lang) -> String {
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
    let month = usize::from(date.month().unsigned_abs().clamp(1, 12)) - 1;
    let name = if lang.fr() {
        MONTHS_FR[month]
    } else {
        MONTHS_EN[month]
    };
    if lang.fr() && date.day() == 1 {
        format!("1er {name}")
    } else {
        format!("{} {name}", date.day())
    }
}

/// When the zone's offset next changes, as the clock's card says it:
/// "heure d'hiver jusqu'au 28 mars", or "pas de changement d'heure".
pub fn next_change(tz: &TimeZone, now: Timestamp, lang: Lang) -> String {
    let offset = tz.to_offset(now);
    let next = tz
        .following(now)
        .take(8)
        .find(|t| t.offset() != offset)
        .map(|t| t.timestamp());
    let Some(at) = next else {
        return w(words::NO_CLOCK_CHANGE).get(lang).to_owned();
    };
    let dst = tz.to_offset_info(now).dst() == Dst::Yes;
    // The day the change happens there; the year said only when it is
    // more than a year away.
    let day = at.to_zoned(tz.clone()).date();
    let mut said = say_day(day, lang);
    if at.duration_since(now).as_secs() > 365 * 86_400 {
        said = format!("{said} {}", day.year());
    }
    match (lang, dst) {
        (Lang::French, false) => format!("heure d’hiver jusqu’au {said}"),
        (Lang::French, true) => format!("heure d’été jusqu’au {said}"),
        (Lang::English, false) => format!("standard time until {said}"),
        (Lang::English, true) => format!("summer time until {said}"),
    }
}

/// The clock card's last line: "Europe/Paris · UTC+01:00, heure d'hiver
/// jusqu'au 28 mars".
pub fn clock_line(id: &str, now: Timestamp, lang: Lang) -> String {
    let Some(tz) = zone(id) else {
        return id.to_owned();
    };
    format!(
        "{id} · {}, {}",
        say_offset(tz.to_offset(now)),
        next_change(&tz, now, lang)
    )
}

/// The zone's row's line: "Paris, France · UTC+01:00".
pub fn zone_line(id: &str, now: Timestamp, lang: Lang) -> String {
    let offset = zone(id).map(|tz| say_offset(tz.to_offset(now)));
    let place = city_of(id).map(|c| c.said(lang));
    match (place, offset) {
        (Some(place), Some(offset)) => format!("{place} · {offset}"),
        (None, Some(offset)) => offset,
        (Some(place), None) => place,
        (None, None) => String::new(),
    }
}

/// "14:42", or "2:42 PM" over 12 hours.
pub fn say_time(time: Time, twelve: bool) -> String {
    let (hour, minute) = (time.hour(), time.minute());
    if twelve {
        let h = match hour % 12 {
            0 => 12,
            h => h,
        };
        format!("{h}:{minute:02} {}", if hour < 12 { "AM" } else { "PM" })
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// The date in the desktop's numeric order: "05/11/2026", or
/// "2026-11-05" when `[locale] date` is the year first.
pub fn say_numeric(date: Date, iso: bool) -> String {
    let (year, month, day) = (date.year(), date.month(), date.day());
    if iso {
        format!("{year}-{month:02}-{day:02}")
    } else {
        format!("{day:02}/{month:02}/{year}")
    }
}

/// A date typed in either order: "05/11/2026", "5/11/2026", "2026-11-05".
pub fn parse_date(text: &str) -> Option<Date> {
    let parts: Vec<i16> = text
        .trim()
        .split(['/', '-', '.'])
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    let (year, month, day) = if *a > 31 { (*a, *b, *c) } else { (*c, *b, *a) };
    if !(1970..=9999).contains(&year) {
        return None;
    }
    Date::new(year, i8::try_from(month).ok()?, i8::try_from(day).ok()?).ok()
}

/// A time typed: "14:45", "2:45 PM", "2:45pm", "14h45".
pub fn parse_time(text: &str) -> Option<Time> {
    let lower = text.trim().to_lowercase();
    let (clock, half) = if let Some(rest) = lower.strip_suffix("pm") {
        (rest.trim(), Some(true))
    } else if let Some(rest) = lower.strip_suffix("am") {
        (rest.trim(), Some(false))
    } else {
        (lower.as_str(), None)
    };
    let (h, m) = clock.split_once([':', 'h'])?;
    let mut hour: i8 = h.trim().parse().ok()?;
    let minute: i8 = m.trim().parse().ok()?;
    if let Some(pm) = half {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = match (hour, pm) {
            (12, false) => 0,
            (12, true) => 12,
            (h, true) => h + 12,
            (h, false) => h,
        };
    }
    Time::new(hour, minute, 0, 0).ok()
}

/// `date` at `time` in `tz`, in microseconds since the epoch, as
/// timedated's `SetTime` takes it.
pub fn usec_of(date: Date, time: Time, tz: &TimeZone) -> Option<i64> {
    let zoned = date.to_datetime(time).to_zoned(tz.clone()).ok()?;
    Some(zoned.timestamp().as_microsecond())
}

/// `now` in the zone `id` (UTC if the system does not know it).
pub fn now_in(id: Option<&str>, now: Timestamp) -> Zoned {
    let tz = id.and_then(zone).unwrap_or(TimeZone::UTC);
    now.to_zoned(tz)
}

// --- The picker's search -------------------------------------------------------------

/// The largest or most asked cities, first when nothing tells them apart.
const KNOWN: [&str; 48] = [
    "Paris",
    "London",
    "New York",
    "Montreal",
    "Tokyo",
    "Los Angeles",
    "Berlin",
    "Madrid",
    "Rome",
    "Brussels",
    "Geneva",
    "Zurich",
    "Amsterdam",
    "Lisbon",
    "Toronto",
    "Chicago",
    "Mexico City",
    "Sao Paulo",
    "Buenos Aires",
    "Montevideo",
    "Monterrey",
    "Shanghai",
    "Beijing",
    "Hong Kong",
    "Singapore",
    "Seoul",
    "Mumbai",
    "New Delhi",
    "Kolkata",
    "Dubai",
    "Istanbul",
    "Moscow",
    "Cairo",
    "Lagos",
    "Johannesburg",
    "Casablanca",
    "Algiers",
    "Tunis",
    "Dakar",
    "Abidjan",
    "Sydney",
    "Auckland",
    "Bangkok",
    "Jakarta",
    "Manila",
    "Karachi",
    "Dhaka",
    "Tehran",
];

/// A city found, and where the typed part is in its name (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub city: &'static City,
    pub marked: Option<(usize, usize)>,
}

/// A text without its accents or capitals, and for each of its
/// characters the byte it came from.
fn folded(text: &str) -> (Vec<char>, Vec<usize>) {
    use unicode_normalization::UnicodeNormalization as _;
    let mut out = (Vec::new(), Vec::new());
    for (at, c) in text.char_indices() {
        let c = if c == '’' { '\'' } else { c };
        for d in c.nfd() {
            if unicode_normalization::char::is_combining_mark(d) {
                continue;
            }
            for l in d.to_lowercase() {
                out.0.push(l);
                out.1.push(at);
            }
        }
    }
    out
}

/// Where `word` starts a word of `text` (or `text` itself), in `text`'s
/// bytes, and where it ends.
fn word_start(text: &str, word: &[char]) -> Option<(usize, usize, bool)> {
    let (chars, from) = folded(text);
    if word.is_empty() || word.len() > chars.len() {
        return None;
    }
    (0..=chars.len() - word.len())
        .find(|&i| (i == 0 || !chars[i - 1].is_alphanumeric()) && chars[i..i + word.len()] == *word)
        .map(|i| {
            let start = from[i];
            let end = from
                .get(i + word.len())
                .copied()
                .unwrap_or(text.len())
                .max(start);
            (start, end, i == 0)
        })
}

/// An offset typed: "UTC+1", "gmt-5", "+05:30", "UTC−3"; bare "UTC" or
/// "GMT" is UTC itself. In seconds.
fn typed_offset(query: &str) -> Option<i32> {
    let q = query.trim().to_lowercase().replace('−', "-");
    let rest = q
        .strip_prefix("utc")
        .or_else(|| q.strip_prefix("gmt"))
        .map(str::trim);
    let rest = match rest {
        Some("") => return Some(0),
        Some(r) => r,
        None => q.as_str(),
    };
    let (sign, digits) = match rest.chars().next()? {
        '+' => (1, &rest[1..]),
        '-' => (-1, &rest[1..]),
        _ => return None,
    };
    let (h, m) = match digits.split_once([':', 'h']) {
        Some((h, m)) => (h, m),
        None if digits.len() == 4 => (&digits[..2], &digits[2..]),
        None => (digits, "0"),
    };
    let h: i32 = h.trim().parse().ok()?;
    let m: i32 = if m.trim().is_empty() {
        0
    } else {
        m.trim().parse().ok()?
    };
    if h > 14 || m >= 60 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

/// How well `city` answers `query`, the lower the better; `None`: not
/// at all.
fn rank(
    city: &City,
    query: &str,
    word: &[char],
    offset: Option<i32>,
    now: Timestamp,
) -> Option<u8> {
    let starts = |text: &str| word_start(text, word).map(|(_, _, first)| first);
    match (starts(city.fr), starts(city.en)) {
        (Some(true), _) | (_, Some(true)) => return Some(0),
        (Some(false), _) | (_, Some(false)) => return Some(1),
        _ => {}
    }
    let country = |lang| country_name(city.country, lang).and_then(starts);
    if country(Lang::French).is_some() || country(Lang::English).is_some() {
        return Some(2);
    }
    let id: String = folded(city.zone).0.into_iter().collect();
    let typed: String = word.iter().collect();
    if id.contains(&typed) || id.replace('_', " ").contains(&typed) {
        return Some(3);
    }
    let tz = zone(city.zone)?;
    if let Some(seconds) = offset {
        return (tz.to_offset(now).seconds() == seconds).then_some(4);
    }
    // An abbreviation, now or in six months ("CET" in summer too).
    let upper = query.trim().to_uppercase();
    if (2..=5).contains(&upper.len()) && upper.chars().all(|c| c.is_ascii_alphabetic()) {
        let later = now
            .checked_add(jiff::SignedDuration::from_hours(183 * 24))
            .unwrap_or(now);
        let said = [now, later]
            .into_iter()
            .any(|at| tz.to_offset_info(at).abbreviation() == upper);
        if said {
            return Some(4);
        }
    }
    None
}

/// Where a city comes when nothing else tells them apart: the most known
/// first, then by name.
fn known(city: &City) -> usize {
    KNOWN
        .iter()
        .position(|k| *k == city.en)
        .unwrap_or(KNOWN.len())
}

/// The picker's rows for `query`: with nothing typed, the zone `current`
/// first, then its country's, then the most known; otherwise the cities
/// whose name, country, zone, abbreviation or offset answer it, those
/// whose name starts with it first. At most [`LISTED`].
pub fn find(query: &str, current: Option<&str>, lang: Lang, now: Timestamp) -> Vec<Found> {
    let all = cities();
    let usable = |c: &&City| zone(c.zone).is_some();
    let typed = query.trim();
    if typed.is_empty() {
        let mut out: Vec<&City> = Vec::new();
        let push = |c: &'static City, out: &mut Vec<&'static City>| {
            if out.len() < LISTED && !out.iter().any(|o| o.zone == c.zone) {
                out.push(c);
            }
        };
        let here = current.and_then(city_of);
        if let Some(here) = here {
            push(here, &mut out);
            let mut same: Vec<&City> = all
                .iter()
                .filter(|c| c.country == here.country)
                .filter(usable)
                .collect();
            same.sort_by_key(|c| (known(c), c.name(lang)));
            for c in same {
                push(c, &mut out);
            }
        }
        let mut most: Vec<&City> = all.iter().filter(|c| known(c) < KNOWN.len()).collect();
        most.sort_by_key(|c| known(c));
        for c in most {
            push(c, &mut out);
        }
        return out
            .into_iter()
            .map(|city| Found { city, marked: None })
            .collect();
    }
    let (word, _) = folded(typed);
    let offset = typed_offset(typed);
    let mut found: Vec<(u8, usize, String, &City)> = all
        .iter()
        .filter_map(|c| {
            let r = rank(c, typed, &word, offset, now)?;
            Some((r, known(c), folded(c.name(lang)).0.into_iter().collect(), c))
        })
        .filter(|(_, _, _, c)| usable(c))
        .collect();
    found.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    found
        .into_iter()
        .take(LISTED)
        .map(|(_, _, _, city)| Found {
            city,
            marked: word_start(city.name(lang), &word).map(|(s, e, _)| (s, e)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn a_zone_says_its_offset_and_its_next_change() {
        let now = at("2026-11-05T13:42:00Z");
        assert_eq!(
            clock_line("Europe/Paris", now, Lang::French),
            "Europe/Paris · UTC+01:00, heure d’hiver jusqu’au 28 mars"
        );
        assert_eq!(
            clock_line("Europe/Paris", at("2026-07-01T12:00:00Z"), Lang::English),
            "Europe/Paris · UTC+02:00, summer time until 25 October"
        );
        assert_eq!(
            clock_line("Asia/Tokyo", now, Lang::French),
            "Asia/Tokyo · UTC+09:00, pas de changement d’heure"
        );
        assert_eq!(
            zone_line("America/Toronto", now, Lang::French),
            "Toronto, Canada · UTC−05:00"
        );
        assert_eq!(
            zone_line("Asia/Kolkata", now, Lang::English),
            "Kolkata, India · UTC+05:30"
        );
        assert_eq!(
            zone_line("Europe/Monaco", now, Lang::French),
            "Monaco · UTC+01:00"
        );
    }

    #[test]
    fn dates_and_times_read_and_say_as_typed() {
        let date = Date::new(2026, 11, 5).expect("a date");
        assert_eq!(say_numeric(date, false), "05/11/2026");
        assert_eq!(say_numeric(date, true), "2026-11-05");
        assert_eq!(say_date(date, Lang::French), "jeudi 5 novembre 2026");
        assert_eq!(parse_date("05/11/2026"), Some(date));
        assert_eq!(parse_date("2026-11-05"), Some(date));
        assert_eq!(parse_date("31/02/2026"), None);
        let time = Time::new(14, 45, 0, 0).expect("a time");
        assert_eq!(parse_time("14:45"), Some(time));
        assert_eq!(parse_time("2:45 PM"), Some(time));
        assert_eq!(parse_time("14h45"), Some(time));
        assert_eq!(parse_time("25:00"), None);
        assert_eq!(say_time(time, true), "2:45 PM");
        assert_eq!(say_time(time, false), "14:45");
        let paris = zone("Europe/Paris").expect("tzdata");
        assert_eq!(
            usec_of(date, time, &paris),
            Some(at("2026-11-05T13:45:00Z").as_microsecond())
        );
    }

    #[test]
    fn the_picker_finds_cities_by_name_country_zone_abbreviation_and_offset() {
        let now = at("2026-11-05T13:42:00Z");
        let names = |q: &str, lang| -> Vec<String> {
            find(q, Some("Europe/Paris"), lang, now)
                .iter()
                .map(|f| f.city.said(lang))
                .collect()
        };
        let mon = names("mon", Lang::French);
        assert_eq!(mon.first().map(String::as_str), Some("Montréal, Canada"));
        assert!(mon.contains(&"Monaco".to_owned()), "{mon:?}");
        assert!(mon.contains(&"Montevideo, Uruguay".to_owned()), "{mon:?}");
        let found = find("mon", None, Lang::French, now);
        assert_eq!(found[0].city.zone, "America/Toronto");
        assert_eq!(found[0].marked, Some((0, 3)));
        // Accents and case ignored, both languages' names.
        assert_eq!(names("MONTREAL", Lang::English)[0], "Montreal, Canada");
        assert_eq!(names("pekin", Lang::French)[0], "Pékin, Chine");
        assert_eq!(names("londres", Lang::French)[0], "Londres, Royaume-Uni");
        assert!(names("japon", Lang::French).contains(&"Tokyo, Japon".to_owned()));
        assert!(names("america/toronto", Lang::French).contains(&"Toronto, Canada".to_owned()));
        assert!(names("PST", Lang::English).contains(&"Los Angeles, United States".to_owned()));
        assert!(names("CET", Lang::French).contains(&"Paris, France".to_owned()));
        assert!(names("+05:30", Lang::English).contains(&"Kolkata, India".to_owned()));
        assert!(names("UTC−5", Lang::English).contains(&"Toronto, Canada".to_owned()));
        // Nothing typed: the zone, its country's, then the most known.
        let rest = names("", Lang::French);
        assert_eq!(rest[0], "Paris, France");
        assert!(
            rest.contains(&"Londres, Royaume-Uni".to_owned()),
            "{rest:?}"
        );
        assert_eq!(rest.len(), LISTED);
    }

    #[test]
    fn every_setting_of_the_page_is_found_by_its_words() {
        let found = |q: &str, lang| {
            crate::search::find(q, lang)
                .iter()
                .map(|f| f.target.id())
                .collect::<Vec<_>>()
        };
        assert!(found("ntp", Lang::French).contains(&AUTO.to_owned()));
        assert!(found("fuseau", Lang::French).contains(&ZONE.to_owned()));
        assert!(found("timezone", Lang::English).contains(&ZONE.to_owned()));
        assert!(found("régler", Lang::French).contains(&SET.to_owned()));
    }
}
