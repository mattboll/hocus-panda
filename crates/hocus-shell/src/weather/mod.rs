//! The weather in the bar (#814, bar.md "Weather"): Open-Meteo's forecast
//! for the place of `[weather]` (the compositor tells it,
//! [`hocus_link::ToShell::Weather`]), asked on a thread of its own every 30
//! minutes, at once when the place changes and as the machine resumes;
//! kept in `$XDG_CACHE_HOME/hocuspanda/weather.json`, so that a restart and
//! a machine offline show the last one with its time. With
//! `HOCUS_WEATHER_FIXTURE=FILE` it is read from that saved answer, never
//! the network (tests, screenshots).
//!
//! What the bar shows of it is made here, as of the wall clock: Today's
//! header and its rain and tomorrow lines ([`today`]), the mini form's item
//! ([`mini`]), the popover's ([`crate::popover::weather`]). From 18:00 the
//! bar shows tomorrow first. Whether it rains now is told to the
//! compositor ([`hocus_link::ToCompositor::Raining`]), for the rain on the
//! glass "only when it rains".

pub mod icon;

use std::{
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use hocus_link::{ToCompositor, WeatherPlace};
use hocus_weather::{Condition, Forecast, Icon, Units};
use hocus_words::shell::weather as txt;
use jiff::{Timestamp, Zoned};

use crate::clock::time::Lang;

/// A forecast is asked again this long after the last.
const EVERY: Duration = Duration::from_secs(30 * 60);
/// After a failure (offline), this long.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// What the weather's thread is asked.
#[derive(Debug)]
enum Ask {
    /// The place and units of `[weather]`, as the compositor says them.
    Place(WeatherPlace),
    /// The forecast again now (the machine resumed).
    Refresh,
}

/// What the bar shows from: the place, and its forecast once one is known.
#[derive(Debug, Default)]
struct Kept {
    place: WeatherPlace,
    forecast: Option<Arc<Forecast>>,
    /// Whether it rains, as last told to the compositor.
    told: Option<bool>,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
static ASK: Mutex<Option<mpsc::Sender<Ask>>> = Mutex::new(None);

fn kept<T>(with: impl FnOnce(&mut Kept) -> T) -> T {
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    with(kept.get_or_insert_with(Kept::default))
}

fn ask(ask: Ask) {
    let sender = ASK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    match sender {
        Some(sender) => {
            if sender.send(ask).is_err() {
                tracing::warn!("the weather's thread is gone");
            }
        }
        None => tracing::debug!(?ask, "the weather is not started"),
    }
}

/// Starts the weather's thread, which waits for the place.
pub fn start() {
    let (sender, asks) = mpsc::channel();
    *ASK.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let started = std::thread::Builder::new()
        .name("weather".to_owned())
        .spawn(move || run(&asks));
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the weather");
    }
}

/// `[weather]` as the compositor says it.
pub fn set_place(place: WeatherPlace) {
    ask(Ask::Place(place));
}

/// The machine resumed: the forecast again.
pub fn resumed() {
    ask(Ask::Refresh);
}

/// The clock's minute changed: whether it rains, again.
pub fn minute() {
    tell_raining();
}

/// The line to the compositor is up: it is told whether it rains.
pub fn linked() {
    kept(|k| k.told = None);
    tell_raining();
}

/// The thread: waits for a place, then asks its forecast every
/// [`EVERY`], at once when the place moves or on request.
fn run(asks: &mpsc::Receiver<Ask>) {
    let fixture = std::env::var_os(hocus_weather::FIXTURE_ENV).map(std::path::PathBuf::from);
    let cache = hocus_weather::cache::file();
    let mut place: Option<WeatherPlace> = None;
    let mut next: Option<Instant> = None;
    loop {
        let asked = match next {
            Some(at) => asks.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => asks
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let fetch = match asked {
            Ok(Ask::Place(now)) => {
                let moved = place.as_ref().is_none_or(|before| {
                    before.latitude != now.latitude || before.longitude != now.longitude
                });
                tracing::info!(
                    shown = now.shown(),
                    imperial = now.imperial,
                    "the weather's place"
                );
                let shown = now.shown();
                if moved && shown && fixture.is_none() {
                    // The last forecast kept for it, until the new one.
                    let cached = cache
                        .as_deref()
                        .and_then(hocus_weather::cache::load)
                        .filter(|f| f.is_for(now.latitude, now.longitude));
                    kept(|k| k.forecast = cached.map(Arc::new));
                } else if moved {
                    kept(|k| k.forecast = None);
                }
                kept(|k| k.place = now.clone());
                place = Some(now);
                publish();
                if !shown {
                    next = None;
                }
                shown && moved
            }
            Ok(Ask::Refresh) => place.as_ref().is_some_and(WeatherPlace::shown),
            Err(mpsc::RecvTimeoutError::Timeout) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let Some(at) = place.as_ref().filter(|_| fetch) else {
            continue;
        };
        let got = match &fixture {
            Some(file) => std::fs::read_to_string(file)
                .map_err(|err| format!("{}: {err}", file.display()))
                .and_then(|text| Forecast::parse(&text, None).map_err(|e| e.to_string())),
            None => hocus_weather::fetch::forecast(at.latitude, at.longitude),
        };
        match got {
            Ok(forecast) => {
                tracing::info!(
                    hours = forecast.hours.len(),
                    days = forecast.days.len(),
                    fixture = fixture.is_some(),
                    "the weather's forecast"
                );
                if fixture.is_none()
                    && let Some(file) = &cache
                    && let Err(err) = hocus_weather::cache::save(file, &forecast)
                {
                    tracing::warn!(err, "the weather's forecast is not kept");
                }
                kept(|k| k.forecast = Some(Arc::new(forecast)));
                publish();
                next = fixture.is_none().then(|| Instant::now() + EVERY);
            }
            Err(err) => {
                tracing::warn!(err, "no weather forecast: the last one stays");
                next = Some(Instant::now() + RETRY);
            }
        }
    }
}

/// Something new to show: Today, the mini form's item and an open
/// popover follow, and the compositor is told whether it rains.
fn publish() {
    crate::today::refresh();
    crate::popover::weather::changed();
    tell_raining();
}

/// What the bar shows from now: the place's forecast, while the weather
/// shows.
#[derive(Debug, Clone)]
pub struct Shown {
    pub place: String,
    pub units: Units,
    pub forecast: Arc<Forecast>,
}

/// The weather as the bar shows it, if it does.
pub fn shown() -> Option<Shown> {
    kept(|k| {
        let forecast = k.forecast.clone()?;
        k.place.shown().then(|| Shown {
            place: k.place.place.clone(),
            units: if k.place.imperial {
                Units::Imperial
            } else {
                Units::Metric
            },
            forecast,
        })
    })
}

/// Whether it rains now where the weather is for, told to the compositor
/// when it changes.
fn tell_raining() {
    let now = crate::notifications::wall().timestamp();
    let raining = shown().is_some_and(|s| s.forecast.raining(now));
    let changed = kept(|k| {
        let changed = k.told != Some(raining);
        k.told = Some(raining);
        changed
    });
    if changed {
        tracing::info!(raining, "the weather tells the compositor whether it rains");
        crate::popover::send(vec![crate::popover::Effect::Tell(ToCompositor::Raining(
            raining,
        ))]);
    }
}

// --- What the bar shows ------------------------------------------------------------

/// Today's header's weather (variant C): the weather now, "9°"; in the
/// evening tomorrow's, "demain 11°".
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub icon: Icon,
    pub said: String,
}

/// The rain coming later today, among Today's lines.
#[derive(Debug, Clone, PartialEq)]
pub struct RainLine {
    /// When it starts (seconds), for its place among the events; none
    /// while it rains (first).
    pub at: Option<i64>,
    /// Its start in the time's cell, "14:15"; empty while it rains.
    pub start: String,
    /// "Pluie → 15 h 45", "Il pleut → 15 h 45".
    pub said: String,
    pub icon: Icon,
}

/// Tomorrow, Today's last line: "Demain", its icon, "3° / 11°".
#[derive(Debug, Clone, PartialEq)]
pub struct TomorrowLine {
    pub icon: Icon,
    pub said: String,
}

/// What Today shows of the weather.
#[derive(Debug, Clone, PartialEq)]
pub struct Today {
    pub head: Head,
    pub rain: Option<RainLine>,
    pub tomorrow: Option<TomorrowLine>,
}

/// The mini form's item: the icon (tomorrow's in the evening), the
/// temperature, "dem." in the evening, the drops when rain comes within
/// 3 h.
#[derive(Debug, Clone, PartialEq)]
pub struct Mini {
    pub icon: Icon,
    pub said: String,
    pub tomorrow: Option<String>,
    pub drops: bool,
}

/// A temperature as the bar says it: "9°", "-2°".
pub fn degrees(celsius: f32, units: Units) -> String {
    format!("{}°", units.degrees(celsius))
}

/// Today's weather at `now`, if it shows.
pub fn today(now: &Zoned, lang: Lang) -> Option<Today> {
    let shown = shown()?;
    today_of(&shown.forecast, shown.units, now, Words::new(lang))
}

/// The mini form's item at `now`, if it shows.
pub fn mini(now: &Zoned, lang: Lang) -> Option<Mini> {
    let shown = shown()?;
    mini_of(&shown.forecast, shown.units, now, Words::new(lang))
}

/// What Today shows of `forecast` at `now`: none past what it says.
pub fn today_of(forecast: &Forecast, units: Units, now: &Zoned, words: Words) -> Option<Today> {
    let at = now.timestamp();
    let current = forecast.now(at)?;
    let tomorrow = now
        .date()
        .tomorrow()
        .ok()
        .and_then(|date| forecast.day(date));
    let head = match tomorrow {
        Some(day) if hocus_weather::evening(now) => Head {
            icon: day.icon(),
            said: format!("{} {}", words.tomorrow_lower(), degrees(day.max, units)),
        },
        _ => Head {
            icon: current.icon(),
            said: degrees(current.temperature, units),
        },
    };
    let rain = forecast.rain_today(now).map(|rain| {
        let end = rain.end.map(|end| words.time(end, now));
        let said = match (rain.start, rain.snow) {
            (Some(_), false) => words.rain(),
            (Some(_), true) => words.snow(),
            (None, false) => words.raining(),
            (None, true) => words.snowing(),
        };
        let said = match end {
            Some(end) => format!("{said} → {end}"),
            None => said.to_owned(),
        };
        RainLine {
            at: rain.start.map(Timestamp::as_second),
            start: rain
                .start
                .map(|s| crate::meeting::hhmm(s, now))
                .unwrap_or_default(),
            said,
            icon: if rain.snow { Icon::Snow } else { Icon::Rain },
        }
    });
    let tomorrow = tomorrow.map(|day| TomorrowLine {
        icon: day.icon(),
        said: format!("{} / {}", degrees(day.min, units), degrees(day.max, units)),
    });
    Some(Today {
        head,
        rain,
        tomorrow,
    })
}

/// The mini form's item for `forecast` at `now`.
pub fn mini_of(forecast: &Forecast, units: Units, now: &Zoned, words: Words) -> Option<Mini> {
    let at = now.timestamp();
    let current = forecast.now(at)?;
    let tomorrow = now
        .date()
        .tomorrow()
        .ok()
        .and_then(|date| forecast.day(date))
        .filter(|_| hocus_weather::evening(now));
    Some(match tomorrow {
        Some(day) => Mini {
            icon: day.icon(),
            said: degrees(day.max, units),
            tomorrow: Some(words.tomorrow_short().to_owned()),
            drops: forecast.rain_soon(at),
        },
        None => Mini {
            icon: current.icon(),
            said: degrees(current.temperature, units),
            tomorrow: None,
            drops: forecast.rain_soon(at),
        },
    })
}

// --- Words ----------------------------------------------------------------------

/// The weather's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn weather(self) -> &'static str {
        txt::WEATHER.get(self.lang)
    }

    pub fn tomorrow(self) -> &'static str {
        txt::TOMORROW.get(self.lang)
    }

    pub fn tomorrow_lower(self) -> &'static str {
        txt::TOMORROW_LOWER.get(self.lang)
    }

    pub fn tomorrow_short(self) -> &'static str {
        txt::TOMORROW_SHORT.get(self.lang)
    }

    pub fn today_short(self) -> &'static str {
        txt::TODAY_SHORT.get(self.lang)
    }

    pub fn rain(self) -> &'static str {
        txt::RAIN.get(self.lang)
    }

    pub fn snow(self) -> &'static str {
        txt::SNOW.get(self.lang)
    }

    pub fn raining(self) -> &'static str {
        txt::RAINING.get(self.lang)
    }

    pub fn snowing(self) -> &'static str {
        txt::SNOWING.get(self.lang)
    }

    /// What the weather is: "Couvert", "Nuit claire".
    pub fn condition(self, code: u8, day: bool) -> &'static str {
        let text = match Condition::of(code) {
            Condition::Clear if day => txt::SUNNY,
            Condition::Clear => txt::CLEAR_NIGHT,
            Condition::Partly => txt::PARTLY_CLOUDY,
            Condition::Cloudy => txt::CLOUDY,
            Condition::Fog => txt::FOG,
            Condition::Drizzle => txt::DRIZZLE,
            Condition::Rain => txt::RAIN,
            Condition::Showers => txt::SHOWERS,
            Condition::Snow => txt::SNOW,
            Condition::Thunder => txt::THUNDERSTORM,
        };
        text.get(self.lang)
    }

    /// `t` as the bar says a time of the weather: "15 h 45" in French,
    /// as the clock writes it in English ("15:45", "3:45 PM").
    pub fn time(self, t: Timestamp, now: &Zoned) -> String {
        if self.lang == Lang::French {
            let time = t.to_zoned(now.time_zone().clone()).time();
            format!("{} h {:02}", time.hour(), time.minute())
        } else {
            crate::meeting::hhmm(t, now)
        }
    }

    /// The header's weather for screen readers: "Météo, 9°, Couvert".
    pub fn said_now(self, temp: &str, condition: &str) -> String {
        txt::said_now(self.lang, txt::SaidNow { temp, condition })
    }
}

#[cfg(test)]
mod tests {
    use jiff::{civil, tz::TimeZone};

    use super::*;

    fn forecast() -> Forecast {
        let text = include_str!("../../../hocus-weather/fixtures/forecast.json");
        Forecast::parse(text, None).unwrap()
    }

    fn at(hour: i8, minute: i8) -> Zoned {
        civil::date(2026, 11, 19)
            .at(hour, minute, 0, 0)
            .to_zoned(TimeZone::get("Europe/Paris").unwrap())
            .unwrap()
    }

    #[test]
    fn today_says_the_weather_now_the_rain_coming_and_tomorrow() {
        let fr = Words::new(Lang::French);
        let today = today_of(&forecast(), Units::Metric, &at(13, 47), fr).unwrap();
        assert_eq!(
            today.head,
            Head {
                icon: Icon::Cloudy,
                said: "9°".to_owned()
            }
        );
        let rain = today.rain.unwrap();
        assert_eq!(rain.start, "14:15");
        assert_eq!(rain.said, "Pluie → 15 h 45");
        assert_eq!(rain.icon, Icon::Rain);
        assert_eq!(
            today.tomorrow,
            Some(TomorrowLine {
                icon: Icon::PartlyCloudy,
                said: "3° / 11°".to_owned()
            })
        );
        // While it rains: no time, "Il pleut".
        let today = today_of(&forecast(), Units::Metric, &at(14, 32), fr).unwrap();
        let rain = today.rain.unwrap();
        assert_eq!((rain.at, rain.start.as_str()), (None, ""));
        assert_eq!(rain.said, "Il pleut → 15 h 45");
        assert_eq!(today.head.icon, Icon::Rain);
        // In English, in °F.
        let en = Words::new(Lang::English);
        let today = today_of(&forecast(), Units::Imperial, &at(13, 47), en).unwrap();
        assert_eq!(today.head.said, "48°");
        assert!(today.rain.unwrap().said.starts_with("Rain → "));
    }

    #[test]
    fn the_evening_shows_tomorrow_first() {
        let fr = Words::new(Lang::French);
        let today = today_of(&forecast(), Units::Metric, &at(19, 20), fr).unwrap();
        assert_eq!(
            today.head,
            Head {
                icon: Icon::PartlyCloudy,
                said: "demain 11°".to_owned()
            }
        );
        assert_eq!(today.rain, None);
        let mini = mini_of(&forecast(), Units::Metric, &at(19, 20), fr).unwrap();
        assert_eq!(
            mini,
            Mini {
                icon: Icon::PartlyCloudy,
                said: "11°".to_owned(),
                tomorrow: Some("dem.".to_owned()),
                drops: false
            }
        );
        // Before 18:00, the weather now, and the drops of the rain coming.
        let mini = mini_of(&forecast(), Units::Metric, &at(13, 47), fr).unwrap();
        assert_eq!((mini.icon, mini.said.as_str()), (Icon::Cloudy, "9°"));
        assert!(mini.drops && mini.tomorrow.is_none());
    }

    #[test]
    fn conditions_in_both_languages() {
        let (fr, en) = (Words::new(Lang::French), Words::new(Lang::English));
        assert_eq!(fr.condition(3, true), "Couvert");
        assert_eq!(fr.condition(0, false), "Nuit claire");
        assert_eq!(en.condition(81, true), "Showers");
        assert_eq!(en.condition(95, true), "Thunderstorm");
    }
}
