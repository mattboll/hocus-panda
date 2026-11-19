//! The last forecast, kept on disk (`$XDG_CACHE_HOME/hocuspanda/weather.json`)
//! so that a restart, and a machine offline, show it with its time.

use std::path::{Path, PathBuf};

use crate::Forecast;

/// `$XDG_CACHE_HOME/hocuspanda/weather.json`, or under `~/.cache`; none
/// without an absolute place for it.
pub fn file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let cache = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    cache
        .is_absolute()
        .then(|| cache.join("hocuspanda/weather.json"))
}

/// The forecast kept at `file`, if one reads there.
pub fn load(file: &Path) -> Option<Forecast> {
    let text = std::fs::read_to_string(file).ok()?;
    match serde_json::from_str(&text) {
        Ok(forecast) => Some(forecast),
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "the weather's cache does not read");
            None
        }
    }
}

/// `forecast` kept at `file`: written beside it, then renamed over it.
pub fn save(file: &Path, forecast: &Forecast) -> Result<(), String> {
    let text = serde_json::to_string(forecast).map_err(|e| e.to_string())?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let new = file.with_extension("json.new");
    std::fs::write(&new, text).map_err(|e| format!("{}: {e}", new.display()))?;
    std::fs::rename(&new, file).map_err(|e| format!("{}: {e}", file.display()))
}
