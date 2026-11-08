//! Free slots, computed from calendars (§2.4): the working hours of the
//! days asked, less every busy event of every calendar read (hidden ones
//! included), weekends and public holidays left out. For several people,
//! a slot is free when it is free for each.

use jiff::{SignedDuration, Timestamp, ToSpan, civil::Date, civil::Time, tz::TimeZone};
use serde::{Deserialize, Serialize};

use super::holidays::{Region, holiday};
use crate::item::{Content, Item};

/// The hours a slot may be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hours {
    pub start: Time,
    pub end: Time,
    /// Saturdays and Sundays are not offered.
    pub weekdays_only: bool,
}

impl Default for Hours {
    fn default() -> Self {
        Self {
            start: Time::constant(9, 0, 0, 0),
            end: Time::constant(18, 0, 0, 0),
            weekdays_only: true,
        }
    }
}

/// A span of time, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl Span {
    pub fn length(&self) -> SignedDuration {
        self.end.duration_since(self.start)
    }
}

/// The busy spans of `items`' events: timed, not cancelled, not
/// declined, not marked free. All-day events take no slot.
pub fn busy(items: &[Item]) -> Vec<Span> {
    items
        .iter()
        .filter_map(|item| match &item.content {
            Content::Event(event) if event.busy && !event.cancelled && !event.declined => {
                match event.when {
                    hocus_day::model::When::Timed { start, end } if end > start => {
                        Some(Span { start, end })
                    }
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// The free slots of at least `least` from `from` to `to` (dates, in
/// `zone`), within `hours`, outside every span of every list in `busy`
/// (one list per person), and not on a public holiday of `region`.
pub fn free(
    busy: &[Vec<Span>],
    from: Date,
    to: Date,
    hours: Hours,
    zone: &TimeZone,
    region: Region,
    least: SignedDuration,
) -> Vec<Span> {
    let mut taken: Vec<Span> = busy.iter().flatten().copied().collect();
    taken.sort();
    let mut out = Vec::new();
    let mut day = from;
    while day <= to {
        let weekend = matches!(
            day.weekday(),
            jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday
        );
        if !(hours.weekdays_only && weekend) && holiday(day, region).is_none() {
            let at = |time: Time| day.to_datetime(time).to_zoned(zone.clone()).ok();
            if let (Some(open), Some(close)) = (at(hours.start), at(hours.end)) {
                let (open, close) = (open.timestamp(), close.timestamp());
                let mut cursor = open;
                for span in taken.iter().filter(|s| s.end > open && s.start < close) {
                    if span.start > cursor {
                        push(&mut out, cursor, span.start.min(close), least);
                    }
                    cursor = cursor.max(span.end);
                }
                push(&mut out, cursor, close, least);
            }
        }
        match day.checked_add(1.day()) {
            Ok(next) => day = next,
            Err(_) => break,
        }
    }
    out
}

fn push(out: &mut Vec<Span>, start: Timestamp, end: Timestamp, least: SignedDuration) {
    if end > start && end.duration_since(start) >= least {
        out.push(Span { start, end });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn at(day: i8, hour: i8, minute: i8) -> Timestamp {
        date(2026, 11, day)
            .at(hour, minute, 0, 0)
            .to_zoned(TimeZone::UTC)
            .map(|z| z.timestamp())
            .unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn a_slot_is_free_for_everyone_or_not_at_all() {
        let alex = vec![Span {
            start: at(9, 10, 0),
            end: at(9, 11, 0),
        }];
        let lea = vec![
            Span {
                start: at(9, 10, 30),
                end: at(9, 12, 0),
            },
            Span {
                start: at(9, 14, 0),
                end: at(9, 17, 30),
            },
        ];
        let slots = free(
            &[alex, lea],
            date(2026, 11, 9),
            date(2026, 11, 9),
            Hours::default(),
            &TimeZone::UTC,
            Region::Metropole,
            SignedDuration::from_mins(30),
        );
        assert_eq!(
            slots,
            [
                Span {
                    start: at(9, 9, 0),
                    end: at(9, 10, 0)
                },
                Span {
                    start: at(9, 12, 0),
                    end: at(9, 14, 0)
                },
                Span {
                    start: at(9, 17, 30),
                    end: at(9, 18, 0)
                },
            ]
        );
    }

    #[test]
    fn holidays_and_weekends_offer_no_slot() {
        // Tuesday 10 to Thursday 12 November 2026: the 11th is Armistice.
        let slots = free(
            &[],
            date(2026, 11, 10),
            date(2026, 11, 14),
            Hours::default(),
            &TimeZone::UTC,
            Region::Metropole,
            SignedDuration::from_mins(30),
        );
        let days: Vec<i8> = slots
            .iter()
            .map(|s| s.start.to_zoned(TimeZone::UTC).day())
            .collect();
        assert_eq!(days, [10, 12, 13]);
    }
}
