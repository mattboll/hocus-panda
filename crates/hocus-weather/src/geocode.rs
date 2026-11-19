//! Places found by name: Open-Meteo's geocoding (`/v1/search`), for the
//! Settings' "Lieu" field. Only the name typed leaves the machine.

use serde::Deserialize;

/// A place found: its name, the region and the country it is in, where it
/// is.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Place {
    pub name: String,
    #[serde(default)]
    pub admin1: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// "Loire, France": what tells it from the others of its name.
    pub fn region(&self) -> String {
        [self.admin1.as_deref(), self.country.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    results: Vec<Place>,
}

/// The search's address for `name`, its places named in `language`
/// (`fr`, `en`), five at most.
pub fn search_url(name: &str, language: &str) -> String {
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language={language}&format=json",
        encode(name.trim())
    )
}

/// `text` for a query string: what is not unreserved, percent-encoded.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The places geocoding's answer `json` lists (none when it lists none).
pub fn places(json: &str) -> Result<Vec<Place>, String> {
    serde_json::from_str::<Answer>(json)
        .map(|a| a.results)
        .map_err(|err| format!("geocoding: {err}"))
}

/// `HOCUS_WEATHER_PLACES=FILE`: the places are found in this saved
/// geocoding answer instead of the network, those whose name starts with
/// what is typed (tests, screenshots).
pub const FIXTURE_ENV: &str = "HOCUS_WEATHER_PLACES";

/// The places named `name`, blocking: on a thread of its own. Two letters
/// at least (geocoding answers nothing shorter).
pub fn search(name: &str, language: &str) -> Result<Vec<Place>, String> {
    if name.trim().chars().count() < 2 {
        return Ok(Vec::new());
    }
    if let Some(file) = std::env::var_os(FIXTURE_ENV) {
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let typed = name.trim().to_lowercase();
        return Ok(places(&text)?
            .into_iter()
            .filter(|p| p.name.to_lowercase().starts_with(&typed))
            .collect());
    }
    places(&crate::fetch::get(&search_url(name, language))?)
}
