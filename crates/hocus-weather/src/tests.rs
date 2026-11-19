use jiff::{Zoned, civil, tz::TimeZone};

use super::*;

const FIXTURE: &str = include_str!("../fixtures/forecast.json");

fn forecast() -> Forecast {
    Forecast::parse(FIXTURE, None).unwrap()
}

/// 19 November 2026 at `hour`:`minute`, Paris.
fn at(hour: i8, minute: i8) -> Zoned {
    civil::date(2026, 11, 19)
        .at(hour, minute, 0, 0)
        .to_zoned(TimeZone::get("Europe/Paris").unwrap())
        .unwrap()
}

fn hhmm(t: Timestamp) -> String {
    t.to_zoned(TimeZone::get("Europe/Paris").unwrap())
        .strftime("%H:%M")
        .to_string()
}

#[test]
fn wmo_codes_map_to_the_bars_icons() {
    assert_eq!(Icon::of(0, true), Icon::Sun);
    assert_eq!(Icon::of(1, false), Icon::Moon);
    assert_eq!(Icon::of(2, true), Icon::PartlyCloudy);
    assert_eq!(Icon::of(2, false), Icon::Cloudy);
    assert_eq!(Icon::of(3, true), Icon::Cloudy);
    assert_eq!(Icon::of(45, true), Icon::Fog);
    assert_eq!(Icon::of(53, true), Icon::Rain);
    assert_eq!(Icon::of(65, true), Icon::Rain);
    assert_eq!(Icon::of(73, true), Icon::Snow);
    assert_eq!(Icon::of(86, true), Icon::Snow);
    assert_eq!(Icon::of(81, true), Icon::Showers);
    assert_eq!(Icon::of(95, true), Icon::Thunder);
    assert_eq!(Icon::of(42, true), Icon::Cloudy);
    assert!(Icon::Rain.wet() && Icon::Showers.wet() && Icon::Thunder.wet());
    assert!(!Icon::Snow.wet() && !Icon::Cloudy.wet() && !Icon::Fog.wet());
    assert_eq!(Condition::of(51), Condition::Drizzle);
}

#[test]
fn open_meteos_answer_reads_its_local_times_as_instants() {
    let f = forecast();
    assert_eq!(hhmm(f.current.at), "13:45");
    assert_eq!(f.fetched, f.current.at);
    assert_eq!(f.current.code, 3);
    assert_eq!(f.hours.len(), 7 * 24);
    assert_eq!(f.days.len(), 7);
    assert_eq!(f.quarters.len(), 96);
    // A quarter is the one before its time: the first ends at 14:00.
    assert_eq!(hhmm(f.quarters[0].start), "13:45");
    assert_eq!(f.days[1].date, civil::date(2026, 11, 20));
    assert_eq!((f.days[1].min, f.days[1].max), (3.0, 11.0));
}

#[test]
fn rain_later_today_says_when_it_starts_and_ends() {
    let f = forecast();
    // Before: from 14:15 to 15:45.
    let rain = f.rain_today(&at(13, 47)).unwrap();
    assert_eq!(rain.start.map(hhmm).as_deref(), Some("14:15"));
    assert_eq!(rain.end.map(hhmm).as_deref(), Some("15:45"));
    assert!(!rain.snow);
    // While it rains: no start.
    let rain = f.rain_today(&at(14, 32)).unwrap();
    assert_eq!(rain.start, None);
    assert_eq!(rain.end.map(hhmm).as_deref(), Some("15:45"));
    // After, and in the evening: none.
    assert_eq!(f.rain_today(&at(16, 0)), None);
    assert_eq!(f.rain_today(&at(19, 20)), None);
}

#[test]
fn rain_past_the_quarters_is_read_by_the_hour_and_tomorrows_does_not_count() {
    let mut f = forecast();
    // No quarter hour's rain: the hours' (15:00 holds 14:00 to 15:00's).
    f.quarters.clear();
    let rain = f.rain_today(&at(10, 0)).unwrap();
    assert_eq!(rain.start.map(hhmm).as_deref(), Some("14:00"));
    assert_eq!(rain.end.map(hhmm).as_deref(), Some("16:00"));
    // Rain only tomorrow: nothing today.
    for h in &mut f.hours {
        if h.moment.at < at(23, 59).timestamp() {
            h.moment.precipitation = 0.0;
        }
    }
    assert_eq!(f.rain_today(&at(10, 0)), None);
}

#[test]
fn the_weather_now_is_the_current_block_then_the_nearest_hour() {
    let f = forecast();
    let now = f.now(at(13, 47).timestamp()).unwrap();
    assert_eq!(now.icon(), Icon::Cloudy);
    assert_eq!(now.temperature, 9.1);
    // 47 min later: the hour's (15:00, rain).
    let later = f.now(at(14, 32).timestamp()).unwrap();
    assert_eq!(later.icon(), Icon::Rain);
    // The night: clear.
    assert_eq!(f.now(at(19, 20).timestamp()).unwrap().icon(), Icon::Moon);
    // Past the forecast: nothing.
    let far = civil::date(2026, 12, 30)
        .at(12, 0, 0, 0)
        .to_zoned(TimeZone::UTC)
        .unwrap();
    assert_eq!(f.now(far.timestamp()), None);
    // Raining, for the rain on the glass.
    assert!(!f.raining(at(13, 47).timestamp()));
    assert!(f.raining(at(14, 32).timestamp()));
    assert!(!f.raining(at(19, 20).timestamp()));
}

#[test]
fn the_next_quarter_hours_hours_and_days() {
    let f = forecast();
    let q = f.next_quarters(at(13, 47).timestamp(), 12);
    assert_eq!(
        q,
        [0.0, 0.0, 0.3, 1.4, 2.6, 2.2, 1.6, 0.9, 0.0, 0.0, 0.0, 0.0]
    );
    assert!(f.rain_soon(at(13, 47).timestamp()));
    assert!(!f.rain_soon(at(16, 0).timestamp()));
    let hours = f.next_hours(at(13, 47).timestamp(), 12);
    assert_eq!(hours.len(), 12);
    assert_eq!(hhmm(hours[0].moment.at), "14:00");
    assert_eq!(hours[1].probability, 90);
    let week = f.week(civil::date(2026, 11, 19), 7);
    assert_eq!(week.len(), 7);
    assert_eq!(week[0].icon(), Icon::Rain);
    assert_eq!(
        f.day(civil::date(2026, 11, 20)).map(|d| d.icon()),
        Some(Icon::PartlyCloudy)
    );
}

#[test]
fn the_evening_starts_at_six() {
    assert!(!evening(&at(17, 59)));
    assert!(evening(&at(18, 0)));
    assert!(evening(&at(23, 30)));
    assert!(!evening(&at(0, 10)));
}

#[test]
fn units_convert_what_is_shown() {
    assert_eq!(Units::of("imperial"), Units::Imperial);
    assert_eq!(Units::of("metric"), Units::Metric);
    assert_eq!(Units::Metric.degrees(9.4), 9);
    assert_eq!(Units::Imperial.degrees(10.0), 50);
    assert_eq!(Units::Metric.amount(6.2).as_deref(), Some("6 mm"));
    assert_eq!(Units::Metric.amount(0.2), None);
    assert_eq!(Units::Imperial.amount(25.4).as_deref(), Some("1.0 in"));
    assert_eq!(Units::Metric.wind(18.0), "18 km/h");
    assert_eq!(Units::Imperial.wind(16.1), "10 mph");
}

#[test]
fn geocoding_reads_its_places_and_its_address_says_the_name_only() {
    let url = geocode::search_url("Saint-Ét", "fr");
    assert!(url.contains("name=Saint-%C3%89t&"), "{url}");
    assert!(url.contains("language=fr"), "{url}");
    let places = geocode::places(
        r#"{"results":[{"id":1,"name":"Saint-Étienne","latitude":45.43389,"longitude":4.39,
        "admin1":"Auvergne-Rhône-Alpes","country":"France"},
        {"id":2,"name":"Nowhere","latitude":1.0,"longitude":2.0}]}"#,
    )
    .unwrap();
    assert_eq!(places.len(), 2);
    assert_eq!(places[0].region(), "Auvergne-Rhône-Alpes, France");
    assert_eq!(places[1].region(), "");
    assert!(
        geocode::places(r#"{"generationtime_ms":0.2}"#)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_forecast_is_kept_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hocuspanda/weather.json");
    let f = forecast();
    cache::save(&file, &f).unwrap();
    assert_eq!(cache::load(&file), Some(f.clone()));
    assert!(f.is_for(45.44, 4.39) && !f.is_for(48.85, 2.35));
}

#[test]
fn the_forecasts_address_asks_what_the_bar_shows() {
    let url = fetch::forecast_url(45.4339, 4.39);
    assert!(
        url.starts_with("https://api.open-meteo.com/v1/forecast?latitude=45.4339&longitude=4.3900")
    );
    for part in [
        "minutely_15=precipitation",
        "precipitation_probability",
        "temperature_2m_min",
        "timezone=auto",
        "forecast_days=7",
    ] {
        assert!(url.contains(part), "{part}");
    }
}
