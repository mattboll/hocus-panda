//! Whether the machine can afford the local model now (§11.1): on mains
//! power, or a battery above the threshold, and a CPU package under the
//! heat limit. Read from sysfs on her loop (a few small files), only when
//! the triage has something to do.
//!
//! `HOCUS_POWER_SUPPLY_DIR` stands for `/sys/class/power_supply` and
//! `HOCUS_THERMAL_DIR` for `/sys/class/thermal`, as for the Settings
//! (tests).

use std::path::{Path, PathBuf};

/// What the machine runs on, and how hot it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Power {
    /// Mains power is plugged in (or there is no battery at all).
    pub on_ac: bool,
    /// The battery's charge, in percent, when there is one.
    pub battery: Option<u8>,
    /// The CPU package's temperature, in °C, when known.
    pub heat: Option<u32>,
}

/// Where power and heat are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensors {
    pub supplies: PathBuf,
    pub thermal: PathBuf,
}

impl Default for Sensors {
    fn default() -> Self {
        let var = |name: &str, default: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map_or_else(|| PathBuf::from(default), PathBuf::from)
        };
        Self {
            supplies: var("HOCUS_POWER_SUPPLY_DIR", "/sys/class/power_supply"),
            thermal: var("HOCUS_THERMAL_DIR", "/sys/class/thermal"),
        }
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_owned())
}

impl Sensors {
    pub fn read(&self) -> Power {
        let mut mains = None;
        let mut battery = None;
        if let Ok(entries) = std::fs::read_dir(&self.supplies) {
            for entry in entries.flatten() {
                let dir = entry.path();
                match read(&dir.join("type")).as_deref() {
                    Some("Mains" | "USB") => {
                        let online = read(&dir.join("online")).as_deref() == Some("1");
                        mains = Some(mains.unwrap_or(false) || online);
                    }
                    Some("Battery") => {
                        // A peripheral's battery (a mouse) is not the
                        // machine's.
                        if read(&dir.join("scope")).as_deref() == Some("Device") {
                            continue;
                        }
                        if let Some(c) = read(&dir.join("capacity")).and_then(|c| c.parse().ok()) {
                            battery = Some(battery.map_or(c, |b: u8| b.min(c)));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut heat = None;
        if let Ok(entries) = std::fs::read_dir(&self.thermal) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if read(&dir.join("type")).as_deref() == Some("x86_pkg_temp")
                    && let Some(t) = read(&dir.join("temp")).and_then(|t| t.parse::<u32>().ok())
                {
                    heat = Some(heat.map_or(t / 1000, |h: u32| h.max(t / 1000)));
                }
            }
        }
        Power {
            // Without a battery, a desktop: on mains.
            on_ac: battery.is_none() || mains.unwrap_or(false),
            battery,
            heat,
        }
    }
}
