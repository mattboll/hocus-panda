//! What calendar servers send is read without trusting it: mangled
//! iCalendar and WebDAV answers never panic, nor take long (#75).

use std::time::{Duration, Instant};

use hocus_platform::calendar::{caldav, ics};
use hocus_test::fuzz::{Rng, mangle};
use jiff::{Timestamp, tz::TimeZone};

const WEEKLY: &str = include_str!("../../../tests/scenarios/calendar/work/weekly.ics");
const DESIGN: &str = include_str!("../../../tests/scenarios/calendar/work/design.ics");
const MONTHLY: &str = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:m\nSUMMARY:Demo\nDTSTART;TZID=Europe/Paris:20260130T150000\nDURATION:PT1H\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;BYMONTHDAY=1,-1;INTERVAL=2;COUNT=40\nEXDATE;TZID=Europe/Paris:20260327T150000\nEND:VEVENT\nEND:VCALENDAR\n";

const REPORT: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
<d:response><d:href>/dav/work/1.ics</d:href><d:propstat><d:prop>
<c:calendar-data>BEGIN:VCALENDAR&#13;
BEGIN:VEVENT
UID:a&amp;b
END:VEVENT
END:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/work/2.ics</d:href><d:propstat><d:prop>
<c:calendar-data><![CDATA[BEGIN:VCALENDAR
END:VCALENDAR]]></c:calendar-data></d:prop></d:propstat></d:response>
</d:multistatus>"#;

#[test]
fn mangled_calendars_are_read_without_panicking() {
    let from: Timestamp = "2026-01-01T00:00:00Z".parse().expect("a time");
    let to: Timestamp = "2027-01-01T00:00:00Z".parse().expect("a time");
    let tz = TimeZone::get("Europe/Paris").unwrap_or(TimeZone::UTC);
    let mut rng = Rng(0x0ca1_e2da);
    let started = Instant::now();
    for i in 0..4000 {
        let seed = [WEEKLY, DESIGN, MONTHLY][i % 3];
        let text = mangle(seed, &mut rng);
        let events = ics::parse(&text);
        let occurrences = ics::expand(&events, from, to, &tz);
        assert!(occurrences.len() <= 4000 * events.len().max(1));
        for e in &events {
            let _ = ics::meeting_link(e);
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn mangled_webdav_answers_are_read_without_panicking() {
    assert_eq!(caldav::calendar_data(REPORT).len(), 2);
    let mut rng = Rng(0xda7a_0001);
    for _ in 0..4000 {
        let xml = mangle(REPORT, &mut rng);
        for data in caldav::calendar_data(&xml) {
            let _ = ics::parse(&data);
        }
    }
}
