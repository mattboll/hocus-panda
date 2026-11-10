//! Whether the machine can afford the local model now (§11.1): on mains
//! power, or a battery above the threshold, and not too hot. The power
//! supplies are read from sysfs on her loop (a few small files), only when
//! the triage has something to do; the heat is a steady reading, sampled
//! off her loop ([`crate::heat`], #756).
//!
//! `HOCUS_POWER_SUPPLY_DIR` stands for `/sys/class/power_supply`,
//! `HOCUS_THERMAL_DIR` for `/sys/class/thermal`, as for the Settings,
//! `HOCUS_HWMON_DIR` for `/sys/class/hwmon` and `HOCUS_NVIDIA_SMI` for
//! `nvidia-smi` (tests).

use std::path::{Path, PathBuf};

/// What the machine runs on, and how hot it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Power {
    /// Mains power is plugged in (or there is no battery at all).
    pub on_ac: bool,
    /// The battery's charge, in percent, when there is one.
    pub battery: Option<u8>,
    /// Too hot for the triage, as the steady reading says.
    pub heat: Option<crate::heat::Hot>,
}

/// Where power and heat are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensors {
    pub supplies: PathBuf,
    pub thermal: PathBuf,
    /// The graphics cards' temperatures (amdgpu, nouveau, radeon).
    pub hwmon: PathBuf,
    /// The NVIDIA proprietary driver's tool, which has no hwmon.
    pub nvidia_smi: PathBuf,
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
            hwmon: var("HOCUS_HWMON_DIR", "/sys/class/hwmon"),
            nvidia_smi: var("HOCUS_NVIDIA_SMI", "nvidia-smi"),
        }
    }
}

pub(crate) fn read(path: &Path) -> Option<String> {
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
        Power {
            // Without a battery, a desktop: on mains.
            on_ac: battery.is_none() || mains.unwrap_or(false),
            battery,
            heat: None,
        }
    }
}
