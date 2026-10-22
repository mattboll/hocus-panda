//! iCalendar (RFC 5545), what an agenda needs of it: events, their times in
//! any time zone, all-day ones, recurrences (daily, weekly on given days,
//! monthly on a day or the nth weekday, yearly) with their exceptions and
//! moved occurrences, and the meeting link to join.

use jiff::{
    Timestamp,
    civil::{Date, DateTime, Weekday},
    tz::TimeZone,
};

/// One event of a calendar, as written.
#[derive(Debug, Clone, PartialEq)]
pub struct VEvent {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub url: String,
    pub start: When,
    /// The end, or the start plus the duration.
    pub end: Option<When>,
    pub duration: Option<jiff::SignedDuration>,
    pub rule: Option<Rule>,
    /// Occurrences taken out (EXDATE).
    pub except: Vec<When>,
    /// This one replaces the occurrence starting then (RECURRENCE-ID).
    pub recurrence_id: Option<When>,
    pub cancelled: bool,
}

/// A time as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// All day.
    Date(Date),
    /// In this time zone (`TZID`), or floating: the user's.
    Local(DateTime, Option<String>),
    Utc(Timestamp),
}

impl When {
    /// The moment, `tz` for floating times and dates.
    pub fn timestamp(&self, tz: &TimeZone) -> Option<Timestamp> {
        match self {
            Self::Utc(t) => Some(*t),
            Self::Date(d) => d.to_zoned(tz.clone()).ok().map(|z| z.timestamp()),
            Self::Local(dt, zone) => {
                let zone = zone
                    .as_deref()
                    .and_then(|name| TimeZone::get(name).ok())
                    .unwrap_or_else(|| tz.clone());
                dt.to_zoned(zone).ok().map(|z| z.timestamp())
            }
        }
    }

    pub fn is_date(&self) -> bool {
        matches!(self, Self::Date(_))
    }
}

/// A recurrence (RRULE), as far as agendas go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: i64,
    pub count: Option<u32>,
    pub until: Option<When>,
    /// Weekdays, each with its rank in the month for monthly rules (`2TU`,
    /// `-1FR`), 0 for every one.
    pub by_day: Vec<(i8, Weekday)>,
    pub by_month_day: Vec<i8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// One occurrence in an agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub start: Timestamp,
    pub end: Timestamp,
    pub all_day: bool,
    /// Where to join it: a video call's link.
    pub link: Option<String>,
}

/// Lines unfolded (a line that starts with a space or a tab goes on the
/// one before).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            Some(more) if !lines.is_empty() => {
                if let Some(last) = lines.last_mut() {
                    last.push_str(more);
                }
            }
            _ => lines.push(raw.to_owned()),
        }
    }
    lines
}

/// A property's parameters (`TZID=…`, `VALUE=DATE`), names in capitals.
type Params = Vec<(String, String)>;

/// `NAME;PARAM=V;…:VALUE` into its name, its parameters and its value.
fn property(line: &str) -> Option<(String, Params, String)> {
    // The value starts at the first colon outside quotes.
    let mut quoted = false;
    let at = line.char_indices().find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(i),
        _ => None,
    })?;
    let (head, value) = (&line[..at], &line[at + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.to_ascii_uppercase(), v.trim_matches('"').to_owned()))
        })
        .collect();
    Some((name, params, value.to_owned()))
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn parse_when(value: &str, params: &[(String, String)]) -> Option<When> {
    let param = |k: &str| params.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str());
    let value = value.trim();
    if param("VALUE") == Some("DATE") || value.len() == 8 {
        let d = Date::strptime("%Y%m%d", value).ok()?;
        return Some(When::Date(d));
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let dt = DateTime::strptime("%Y%m%dT%H%M%S", utc).ok()?;
        return dt
            .to_zoned(TimeZone::UTC)
            .ok()
            .map(|z| When::Utc(z.timestamp()));
    }
    let dt = DateTime::strptime("%Y%m%dT%H%M%S", value).ok()?;
    Some(When::Local(dt, param("TZID").map(str::to_owned)))
}

/// `P1DT2H30M`, `PT45M`, `-PT15M`; a day is 24 hours.
fn parse_duration(value: &str) -> Option<jiff::SignedDuration> {
    let span = value.trim().parse::<jiff::Span>().ok()?;
    span.to_duration(jiff::SpanRelativeTo::days_are_24_hours())
        .ok()
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

fn parse_rule(value: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
    };
    let mut freq = None;
    for part in value.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    // Hourly and finer: not an agenda's matter.
                    _ => return None,
                });
            }
            "INTERVAL" => rule.interval = v.parse::<i64>().ok()?.clamp(1, 1000),
            "COUNT" => rule.count = v.parse().ok(),
            "UNTIL" => rule.until = parse_when(v, &[]),
            "BYDAY" => {
                for day in v.split(',') {
                    let split = day.len().checked_sub(2)?;
                    // Not on a character's edge: not a weekday either.
                    let (rank, name) = (day.get(..split)?, day.get(split..)?);
                    let rank = if rank.is_empty() {
                        0
                    } else {
                        rank.trim_start_matches('+').parse().ok()?
                    };
                    rule.by_day.push((rank, parse_weekday(name)?));
                }
            }
            "BYMONTHDAY" => {
                for d in v.split(',') {
                    rule.by_month_day.push(d.parse().ok()?);
                }
            }
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

/// Every VEVENT of `text`.
pub fn parse(text: &str) -> Vec<VEvent> {
    let mut events = Vec::new();
    let mut current: Option<VEvent> = None;
    // VALARM and the like have properties of their own.
    let mut depth = 0;
    for line in unfold(text) {
        let Some((name, params, value)) = property(&line) else {
            continue;
        };
        match (name.as_str(), value.as_str()) {
            ("BEGIN", "VEVENT") => {
                current = Some(VEvent {
                    uid: String::new(),
                    summary: String::new(),
                    location: String::new(),
                    description: String::new(),
                    url: String::new(),
                    start: When::Date(Date::constant(1970, 1, 1)),
                    end: None,
                    duration: None,
                    rule: None,
                    except: Vec::new(),
                    recurrence_id: None,
                    cancelled: false,
                });
                depth = 0;
            }
            ("END", "VEVENT") => {
                if let Some(e) = current.take()
                    && !e.uid.is_empty()
                {
                    events.push(e);
                }
            }
            ("BEGIN", _) if current.is_some() => depth += 1,
            ("END", _) if current.is_some() => depth -= 1,
            _ => {}
        }
        let Some(e) = current.as_mut().filter(|_| depth == 0) else {
            continue;
        };
        match name.as_str() {
            "UID" => e.uid = value,
            "SUMMARY" => e.summary = unescape(&value),
            "LOCATION" => e.location = unescape(&value),
            "DESCRIPTION" => e.description = unescape(&value),
            "URL" => e.url = value,
            "DTSTART" => {
                if let Some(w) = parse_when(&value, &params) {
                    e.start = w;
                }
            }
            "DTEND" => e.end = parse_when(&value, &params),
            "DURATION" => e.duration = parse_duration(&value),
            "RRULE" => e.rule = parse_rule(&value),
            "EXDATE" => {
                for v in value.split(',') {
                    e.except.extend(parse_when(v, &params));
                }
            }
            "RECURRENCE-ID" => e.recurrence_id = parse_when(&value, &params),
            "STATUS" => e.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    events
}

/// A video call's link: the event's URL, or one found in its location or
/// description (Jitsi, Meet, Zoom, Teams, Webex, BigBlueButton…).
pub fn meeting_link(e: &VEvent) -> Option<String> {
    const CALLS: [&str; 8] = [
        "meet.google.com/",
        "zoom.us/j/",
        "teams.microsoft.com/",
        "meet.jit.si/",
        "jitsi",
        "webex.com/",
        "whereby.com/",
        "/b/",
    ];
    let urls = |text: &str| -> Vec<String> {
        text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
            .filter(|w| w.starts_with("https://"))
            .map(|w| w.trim_end_matches([',', ';', '.', ')']).to_owned())
            .collect()
    };
    let mut all = Vec::new();
    if e.url.starts_with("https://") {
        all.push(e.url.clone());
    }
    all.extend(urls(&e.location));
    all.extend(urls(&e.description));
    // A known call first; else a link in the location is where it is.
    all.iter()
        .find(|u| CALLS.iter().any(|c| u.contains(c)))
        .cloned()
        .or_else(|| urls(&e.location).into_iter().next())
}

/// Every occurrence of `events` that overlaps `from`..`to`, floating times
/// in `tz`, sorted by start.
pub fn expand(events: &[VEvent], from: Timestamp, to: Timestamp, tz: &TimeZone) -> Vec<Occurrence> {
    let mut out = Vec::new();
    // Moved or cancelled occurrences, by uid and original start.
    let overrides: Vec<(&str, Timestamp)> = events
        .iter()
        .filter_map(|e| Some((e.uid.as_str(), e.recurrence_id.as_ref()?.timestamp(tz)?)))
        .collect();
    for e in events {
        let Some(start) = e.start.timestamp(tz) else {
            continue;
        };
        let length = match (&e.end, &e.duration) {
            (Some(end), _) => end
                .timestamp(tz)
                .map(|end| end.duration_since(start))
                .unwrap_or_default(),
            (None, Some(d)) => *d,
            // A date alone lasts the day, a time alone no time.
            (None, None) if e.start.is_date() => jiff::SignedDuration::from_hours(24),
            (None, None) => jiff::SignedDuration::ZERO,
        };
        let starts: Vec<Timestamp> = match (&e.rule, &e.recurrence_id) {
            (Some(rule), None) => occurrences(
                &e.start,
                rule,
                from.checked_sub(length).unwrap_or(from),
                to,
                tz,
            )
            .into_iter()
            .filter(|s| {
                !e.except.iter().any(|x| x.timestamp(tz) == Some(*s))
                    && !overrides.iter().any(|(uid, at)| *uid == e.uid && at == s)
            })
            .collect(),
            _ => vec![start],
        };
        if e.cancelled {
            continue;
        }
        let link = meeting_link(e);
        for s in starts {
            let end = s.checked_add(length).unwrap_or(s);
            if end > from && s < to {
                out.push(Occurrence {
                    uid: e.uid.clone(),
                    summary: e.summary.clone(),
                    location: e.location.clone(),
                    start: s,
                    end,
                    all_day: e.start.is_date(),
                    link: link.clone(),
                });
            }
        }
    }
    out.sort_by_key(|o| (o.start, o.uid.clone()));
    out
}

/// Where a recurring event starts, up to `to`; at most a few thousand.
/// Where a recurring event starts from `from` (less its length, so that
/// one going on is kept) up to `to`; at most a few thousand.
fn occurrences(
    start: &When,
    rule: &Rule,
    from: Timestamp,
    to: Timestamp,
    tz: &TimeZone,
) -> Vec<Timestamp> {
    const MAX: usize = 4000;
    // Recurrences go by the wall clock of the event's own zone.
    let zone = match start {
        When::Local(_, Some(name)) => TimeZone::get(name).unwrap_or_else(|_| tz.clone()),
        When::Utc(_) => TimeZone::UTC,
        _ => tz.clone(),
    };
    let Some(first) = start.timestamp(tz).map(|t| t.to_zoned(zone.clone())) else {
        return Vec::new();
    };
    // A date alone: the whole day.
    let until = match &rule.until {
        Some(When::Date(d)) => d
            .tomorrow()
            .ok()
            .and_then(|d| When::Date(d).timestamp(tz))
            .and_then(|t| t.checked_sub(jiff::SignedDuration::from_secs(1)).ok()),
        Some(u) => u.timestamp(tz),
        None => None,
    };
    let time = first.datetime().time();
    let at = |date: Date| -> Option<Timestamp> {
        date.to_datetime(time)
            .to_zoned(zone.clone())
            .ok()
            .map(|z| z.timestamp())
    };
    let mut out: Vec<Timestamp> = Vec::new();
    // Occurrences since the first, for COUNT.
    let mut seen = 0usize;
    let done = |seen: usize, t: Timestamp| {
        t > to || until.is_some_and(|u| t > u) || rule.count.is_some_and(|c| seen >= c as usize)
    };
    let date = first.date();
    // Without COUNT, the years before `from` need no walking through: a
    // daily meeting begun long ago starts near the window, a period early.
    let mut period = match rule.count {
        Some(_) => 0,
        None => {
            let from_date = from.to_zoned(zone.clone()).date();
            let days = date.until(from_date).map_or(0, |s| i64::from(s.get_days()));
            let per_period = match rule.freq {
                Freq::Daily => 1,
                Freq::Weekly => 7,
                Freq::Monthly => 31,
                Freq::Yearly => 366,
            } * rule.interval;
            (days / per_period - 1).max(0)
        }
    };
    let last = period + 100_000;
    while out.len() < MAX {
        // Out of range, or of what a date can be: the rule ends there.
        let Some(step) = period.checked_mul(rule.interval) else {
            break;
        };
        let offset = |d: Date| i64::from(d.weekday().to_monday_zero_offset());
        let mut candidates: Vec<Date> = match rule.freq {
            Freq::Daily => match add_days(date, step) {
                Some(d) => vec![d],
                None => break,
            },
            Freq::Weekly => {
                let week = add_days(date, -offset(date))
                    .zip(step.checked_mul(7))
                    .and_then(|(monday, days)| add_days(monday, days));
                let Some(week) = week else {
                    break;
                };
                if rule.by_day.is_empty() {
                    add_days(week, offset(date)).into_iter().collect()
                } else {
                    rule.by_day
                        .iter()
                        .filter_map(|(_, wd)| add_days(week, i64::from(wd.to_monday_zero_offset())))
                        .collect()
                }
            }
            Freq::Monthly => match add_months(date.first_of_month(), step) {
                Some(month) => month_days(month, date, rule),
                None => break,
            },
            Freq::Yearly => match add_years(date, step) {
                Some(d) => vec![d],
                None => break,
            },
        };
        candidates.sort();
        let mut stop = false;
        for d in candidates {
            if d < date {
                continue;
            }
            let Some(t) = at(d) else {
                continue;
            };
            if done(seen, t) {
                stop = true;
                break;
            }
            seen += 1;
            if t >= from {
                out.push(t);
            }
        }
        if stop {
            break;
        }
        period += 1;
        if period > last {
            break;
        }
    }
    out
}

fn add_days(d: Date, n: i64) -> Option<Date> {
    d.checked_add(jiff::Span::new().try_days(n).ok()?).ok()
}

fn add_months(d: Date, n: i64) -> Option<Date> {
    d.checked_add(jiff::Span::new().try_months(n).ok()?).ok()
}

fn add_years(d: Date, n: i64) -> Option<Date> {
    d.checked_add(jiff::Span::new().try_years(n).ok()?).ok()
}

/// The days of `month` a monthly rule picks: its days of the month, its
/// ranked weekdays, else the first start's day.
fn month_days(month: Date, first: Date, rule: &Rule) -> Vec<Date> {
    let days = month.days_in_month();
    let mut out = Vec::new();
    for d in &rule.by_month_day {
        let day = if *d < 0 { days + 1 + *d } else { *d };
        if (1..=days).contains(&day) {
            out.extend(month.with().day(day).build());
        }
    }
    for (rank, wd) in &rule.by_day {
        let all: Vec<Date> = (1..=days)
            .filter_map(|day| month.with().day(day).build().ok())
            .filter(|d| d.weekday() == *wd)
            .collect();
        match *rank {
            0 => out.extend(all),
            r if r > 0 => out.extend(all.get(r as usize - 1)),
            r => out.extend(
                all.len()
                    .checked_sub(usize::from(r.unsigned_abs()))
                    .and_then(|i| all.get(i)),
            ),
        }
    }
    if rule.by_day.is_empty() && rule.by_month_day.is_empty() {
        out.extend(month.with().day(first.day()).build());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().expect("a timestamp")
    }

    const CAL: &str = "BEGIN:VCALENDAR\r
BEGIN:VEVENT\r
UID:weekly@test\r
SUMMARY:Weekly review\r
DTSTART;TZID=Europe/Paris:20261005T100000\r
DTEND;TZID=Europe/Paris:20261005T103000\r
RRULE:FREQ=WEEKLY;BYDAY=MO,WE\r
EXDATE;TZID=Europe/Paris:20261007T100000\r
LOCATION:https://meet.jit.si/hocus-weekly\r
BEGIN:VALARM\r
DESCRIPTION:not the event's\r
END:VALARM\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:weekly@test\r
RECURRENCE-ID;TZID=Europe/Paris:20261012T100000\r
SUMMARY:Weekly review (moved)\r
DTSTART;TZID=Europe/Paris:20261012T140000\r
DTEND;TZID=Europe/Paris:20261012T143000\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday@test\r
SUMMARY:Holiday\\, all day\r
DTSTART;VALUE=DATE:20261009\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn a_week_of_a_recurring_meeting_with_its_exceptions() {
        let events = parse(CAL);
        assert_eq!(events.len(), 3);
        let utc = TimeZone::UTC;
        let o = expand(
            &events,
            ts("2026-10-05T00:00:00Z"),
            ts("2026-10-15T00:00:00Z"),
            &utc,
        );
        let seen: Vec<(String, String)> = o
            .iter()
            .map(|o| (o.summary.clone(), o.start.to_string()))
            .collect();
        assert_eq!(
            seen,
            [
                ("Weekly review".into(), "2026-10-05T08:00:00Z".into()),
                ("Holiday, all day".into(), "2026-10-09T00:00:00Z".into()),
                (
                    "Weekly review (moved)".into(),
                    "2026-10-12T12:00:00Z".into()
                ),
                ("Weekly review".into(), "2026-10-14T08:00:00Z".into()),
            ]
        );
        assert_eq!(
            o[0].link.as_deref(),
            Some("https://meet.jit.si/hocus-weekly")
        );
        assert!(o[1].all_day);
        assert_eq!(o[1].end.duration_since(o[1].start).as_hours(), 24);
    }

    #[test]
    fn monthly_on_the_last_friday_and_counted() {
        let text = "BEGIN:VEVENT\nUID:a\nSUMMARY:Demo\nDTSTART:20260130T150000Z\nDURATION:PT1H\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3\nEND:VEVENT\n";
        let o = expand(
            &parse(text),
            ts("2026-01-01T00:00:00Z"),
            ts("2027-01-01T00:00:00Z"),
            &TimeZone::UTC,
        );
        let days: Vec<String> = o.iter().map(|o| o.start.to_string()).collect();
        assert_eq!(
            days,
            [
                "2026-01-30T15:00:00Z",
                "2026-02-27T15:00:00Z",
                "2026-03-27T15:00:00Z"
            ]
        );
    }

    #[test]
    fn until_a_date_takes_the_whole_day() {
        let text = "BEGIN:VEVENT\nUID:c\nSUMMARY:Standup\nDTSTART:20261019T091500Z\nRRULE:FREQ=DAILY;UNTIL=20261021\nEND:VEVENT\n";
        let o = expand(
            &parse(text),
            ts("2026-10-01T00:00:00Z"),
            ts("2026-11-01T00:00:00Z"),
            &TimeZone::UTC,
        );
        assert_eq!(o.len(), 3, "the 19th, 20th and 21st");
    }

    #[test]
    fn hostile_rules_end_without_panicking() {
        for rule in [
            "FREQ=DAILY;INTERVAL=9223372036854775807",
            "FREQ=MONTHLY;BYDAY=-128MO",
            "FREQ=WEEKLY;BYDAY=éMO",
            "FREQ=YEARLY;INTERVAL=1000",
        ] {
            let text = format!(
                "BEGIN:VEVENT\nUID:h\nDTSTART:00010101T000000Z\nRRULE:{rule}\nEND:VEVENT\n"
            );
            let o = expand(
                &parse(&text),
                ts("2026-01-01T00:00:00Z"),
                ts("2027-01-01T00:00:00Z"),
                &TimeZone::UTC,
            );
            assert!(o.len() <= 366, "{rule}: {}", o.len());
        }
    }

    #[test]
    fn a_daily_meeting_begun_long_ago_still_shows() {
        let text = "BEGIN:VEVENT\nUID:d\nSUMMARY:Standup\nDTSTART:20140106T091500Z\nDURATION:PT15M\nRRULE:FREQ=DAILY\nEND:VEVENT\n";
        let o = expand(
            &parse(text),
            ts("2026-10-21T00:00:00Z"),
            ts("2026-10-23T00:00:00Z"),
            &TimeZone::UTC,
        );
        let days: Vec<String> = o.iter().map(|o| o.start.to_string()).collect();
        assert_eq!(days, ["2026-10-21T09:15:00Z", "2026-10-22T09:15:00Z"]);
    }

    #[test]
    fn folded_lines_and_escapes_read() {
        let text = "BEGIN:VEVENT\nUID:b\nSUMMARY:A long\n  title\\; really\nDTSTART:20261021T144900Z\nDESCRIPTION:Join: https://meet.google.com/abc-defg-hij\\nThanks\nEND:VEVENT\n";
        let e = &parse(text)[0];
        assert_eq!(e.summary, "A long title; really");
        assert_eq!(
            meeting_link(e).as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
    }
}
