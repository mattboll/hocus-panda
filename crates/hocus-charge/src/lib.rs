//! The battery's charge limit (6.10, issue #896, docs/design/settings.md
//! "Énergie"): the kernel's charge thresholds, read by the compositor and
//! the Settings, written by the package's helper alone.
//!
//! The thresholds are the battery's own (the kernel's
//! `Documentation/ABI/testing/sysfs-class-power`):
//! `charge_control_end_threshold`, the level above which charging stops,
//! and on some laptops `charge_control_start_threshold`, the level below
//! which it begins again. The firmware keeps them, the computer off too,
//! so writing them again is harmless. Writing needs root: the helper
//! (`src/bin/charge_limit.rs`, installed as
//! `/usr/libexec/hocuspanda/charge-limit` and run through pkexec) writes
//! [`Target::Limit`] or [`Target::Full`] and nothing else, into those two
//! files of the system's batteries and no other.

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

/// Where the kernel lists its power supplies.
pub const SUPPLIES: &str = "/sys/class/power_supply";
/// The level above which charging stops, in percent.
pub const END: &str = "charge_control_end_threshold";
/// The level below which charging begins again, in percent.
pub const START: &str = "charge_control_start_threshold";

/// What the thresholds are set to: the only two values written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The limit on: charging stops at 80 %.
    Limit,
    /// No limit: the limit off, or lifted until the next unplug.
    Full,
}

impl Target {
    /// The end threshold.
    pub const fn end(self) -> u8 {
        match self {
            Self::Limit => 80,
            Self::Full => 100,
        }
    }

    /// The start threshold that goes with it, where a battery has one: a
    /// little below the end, so that charging begins again under it.
    pub const fn start(self) -> u8 {
        match self {
            Self::Limit => 75,
            Self::Full => 95,
        }
    }

    /// As the helper takes it.
    pub const fn arg(self) -> &'static str {
        match self {
            Self::Limit => "80",
            Self::Full => "100",
        }
    }
}

/// The helper's arguments, its own name left out: exactly one, `80` or
/// `100`. Anything else is refused.
pub fn read_args(args: &[OsString]) -> Option<Target> {
    match args {
        [one] => match one.to_str()? {
            "80" => Some(Target::Limit),
            "100" => Some(Target::Full),
            _ => None,
        },
        _ => None,
    }
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_owned())
}

fn percent(dir: &Path, name: &str) -> Option<u8> {
    read(dir, name)?.parse().ok()
}

/// The system's batteries in `supplies` that have an end threshold, in
/// their names' order: the supplies whose `type` is `Battery` and which
/// are not a device's own (`scope` `Device`: a mouse's, a headset's).
pub fn batteries(supplies: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(supplies)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| read(path, "type").as_deref() == Some("Battery"))
        .filter(|path| read(path, "scope").as_deref() != Some("Device"))
        .filter(|path| path.join(END).is_file())
        .collect();
    found.sort();
    found
}

/// Whether the charge can be limited here: a battery has the threshold.
pub fn supported(supplies: &Path) -> bool {
    !batteries(supplies).is_empty()
}

/// Whether every battery already holds `target`: its end threshold, and
/// for [`Target::Full`] no start threshold that would keep it from
/// charging (see [`apply`]). `false` without a battery that has one.
pub fn holds(supplies: &Path, target: Target) -> bool {
    let batteries = batteries(supplies);
    !batteries.is_empty()
        && batteries.iter().all(|battery| {
            percent(battery, END) == Some(target.end())
                && !(target == Target::Full && start_below(battery, target))
        })
}

/// A start threshold under `target`'s, 0 left out: some firmware says 0
/// for none.
fn start_below(battery: &Path, target: Target) -> bool {
    percent(battery, START).is_some_and(|start| (1..target.start()).contains(&start))
}

/// Writes `target` into every battery's thresholds (as root: the
/// helper's). An error on one battery does not stop the others; the last
/// one is returned.
pub fn apply(supplies: &Path, target: Target) -> io::Result<()> {
    apply_with(supplies, target, |path, value| {
        std::fs::write(path, value.to_string())
    })
}

/// [`apply`], each write made by `write`.
///
/// The end threshold first. Some firmware refuses an end at or under the
/// start threshold: then the start goes a little below ([`Target::start`])
/// and the end is written again. For [`Target::Full`], a start threshold
/// still low (75, left by the limit) is raised too: else a battery at 80 %
/// would not charge at all.
pub fn apply_with(
    supplies: &Path,
    target: Target,
    mut write: impl FnMut(&Path, u8) -> io::Result<()>,
) -> io::Result<()> {
    let batteries = batteries(supplies);
    if batteries.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no battery has a {END}"),
        ));
    }
    let mut failed = None;
    for battery in &batteries {
        if let Err(err) = set(battery, target, &mut write) {
            failed = Some(io::Error::new(
                err.kind(),
                format!("{}: {err}", battery.display()),
            ));
        }
    }
    failed.map_or(Ok(()), Err)
}

fn set(
    battery: &Path,
    target: Target,
    write: &mut impl FnMut(&Path, u8) -> io::Result<()>,
) -> io::Result<()> {
    let end = battery.join(END);
    let start = battery.join(START);
    let has_start = start.is_file();
    match write(&end, target.end()) {
        Ok(()) => {}
        Err(_) if has_start => {
            write(&start, target.start())?;
            write(&end, target.end())?;
        }
        Err(err) => return Err(err),
    }
    if target == Target::Full && has_start && start_below(battery, target) {
        write(&start, target.start())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, files: &[(&str, &str)]) -> std::io::Result<()> {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir)?;
        for (file, text) in files {
            std::fs::write(dir.join(file), format!("{text}\n"))?;
        }
        Ok(())
    }

    fn text(root: &Path, name: &str, file: &str) -> String {
        std::fs::read_to_string(root.join(name).join(file))
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    /// A ThinkPad's two batteries (the second without a start), a mains
    /// supply, a mouse's battery and an old battery without thresholds.
    fn laptop() -> std::io::Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        supply(
            root,
            "BAT0",
            &[("type", "Battery"), (END, "100"), (START, "96")],
        )?;
        supply(root, "BAT1", &[("type", "Battery"), (END, "100")])?;
        supply(
            root,
            "AC",
            &[("type", "Mains"), ("online", "1"), (END, "1")],
        )?;
        supply(
            root,
            "hidpp_battery_0",
            &[("type", "Battery"), ("scope", "Device"), (END, "100")],
        )?;
        supply(root, "CMB0", &[("type", "Battery"), ("capacity", "50")])?;
        Ok(dir)
    }

    #[test]
    fn the_helper_takes_80_or_100_and_nothing_else() {
        let args = |list: &[&str]| -> Vec<OsString> { list.iter().map(OsString::from).collect() };
        assert_eq!(read_args(&args(&["80"])), Some(Target::Limit));
        assert_eq!(read_args(&args(&["100"])), Some(Target::Full));
        for refused in [
            &[][..],
            &["79"],
            &["0"],
            &["80 "],
            &["080"],
            &["+80"],
            &["80", "100"],
            &["100", "/etc/passwd"],
            &["--help"],
            &["/sys/class/power_supply/BAT0/charge_control_end_threshold"],
        ] {
            assert_eq!(read_args(&args(refused)), None, "{refused:?}");
        }
        assert_eq!(Target::Limit.arg(), "80");
        assert_eq!(Target::Full.arg(), "100");
    }

    #[test]
    fn only_the_systems_batteries_with_a_threshold_are_written() -> std::io::Result<()> {
        let dir = laptop()?;
        let root = dir.path();
        let names: Vec<String> = batteries(root)
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert_eq!(names, ["BAT0", "BAT1"]);
        assert!(supported(root));
        assert!(!supported(&root.join("nowhere")));
        // The writes, in order: BAT0's end refused under its start of 96
        // (as a ThinkPad does), so its start first; BAT1's end alone.
        let mut written = Vec::new();
        apply_with(root, Target::Limit, |path, value| {
            let start = text(root, "BAT0", START).parse::<u8>().unwrap_or(0);
            if path.ends_with(format!("BAT0/{END}")) && value <= start {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            }
            written.push((path.strip_prefix(root).map(Path::to_path_buf), value));
            std::fs::write(path, value.to_string())
        })?;
        let written: Vec<(String, u8)> = written
            .into_iter()
            .filter_map(|(p, v)| Some((p.ok()?.display().to_string(), v)))
            .collect();
        assert_eq!(
            written,
            [
                (format!("BAT0/{START}"), 75),
                (format!("BAT0/{END}"), 80),
                (format!("BAT1/{END}"), 80),
            ]
        );
        assert!(holds(root, Target::Limit));
        assert!(!holds(root, Target::Full));
        // The others untouched.
        assert_eq!(text(root, "AC", END), "1");
        assert_eq!(text(root, "hidpp_battery_0", END), "100");
        assert!(!root.join("CMB0").join(END).exists());
        // Full: the end at 100, and the start left at 75 raised, so that a
        // battery at 80 % charges.
        apply(root, Target::Full)?;
        assert_eq!(text(root, "BAT0", END), "100");
        assert_eq!(text(root, "BAT0", START), "95");
        assert_eq!(text(root, "BAT1", END), "100");
        assert!(holds(root, Target::Full));
        Ok(())
    }

    #[test]
    fn without_a_battery_nothing_is_written() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        supply(dir.path(), "AC", &[("type", "Mains"), (END, "1")])?;
        let mut writes = 0;
        let done = apply_with(dir.path(), Target::Limit, |_, _| {
            writes += 1;
            Ok(())
        });
        assert!(done.is_err());
        assert_eq!(writes, 0);
        assert!(!holds(dir.path(), Target::Limit));
        Ok(())
    }
}
