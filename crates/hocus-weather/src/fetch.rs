//! Asking Open-Meteo, blocking: on a worker's thread, never a loop's.
//! `ureq`, rustls on ring against the system's certificates (the
//! platform's verifier, no bundled roots), as `hocus-day`'s adapters.

use std::{sync::Arc, time::Duration};

use jiff::Timestamp;

use crate::Forecast;

/// What an answer may take.
const TIMEOUT: Duration = Duration::from_secs(20);
/// The most an answer is read (a week's forecast is about 30 kB).
const MOST: u64 = 1 << 20;

/// The forecast's address for the place at `latitude`, `longitude`: the
/// weather now, the next 24 h by quarter hour, 7 days by hour and by
/// day, in the place's zone, metric.
pub fn forecast_url(latitude: f64, longitude: f64) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.4}&longitude={longitude:.4}\
         &current=temperature_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m,is_day\
         &minutely_15=precipitation&forecast_minutely_15=96\
         &hourly=temperature_2m,apparent_temperature,precipitation_probability,precipitation,weather_code,wind_speed_10m,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum\
         &timezone=auto&forecast_days=7"
    )
}

/// The HTTP client: answers within [`TIMEOUT`]; a status is an answer
/// to read, a redirect is not followed.
pub(crate) fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(concat!("hocuspanda/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// `url`'s answer as text, when it is a success.
pub(crate) fn get(url: &str) -> Result<String, String> {
    let mut answer = agent().get(url).call().map_err(|err| match err {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => format!("TLS: {err}"),
        _ => err.to_string(),
    })?;
    let status = answer.status();
    let text = answer
        .body_mut()
        .with_config()
        .limit(MOST)
        .read_to_string()
        .map_err(|err| err.to_string())?;
    if !status.is_success() {
        let said: String = text.chars().take(200).collect();
        return Err(format!("{status}: {said}"));
    }
    Ok(text)
}

/// The forecast for the place at `latitude`, `longitude`, asked now: it
/// keeps the coordinates asked (the answer's are its grid's).
pub fn forecast(latitude: f64, longitude: f64) -> Result<Forecast, String> {
    let text = get(&forecast_url(latitude, longitude))?;
    let mut forecast = Forecast::parse(&text, Some(Timestamp::now())).map_err(|e| e.to_string())?;
    forecast.latitude = latitude;
    forecast.longitude = longitude;
    Ok(forecast)
}
