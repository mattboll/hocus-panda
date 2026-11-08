//! The day's file: one JSON line per sample, appended, so that a day reads
//! after a crash up to its last whole line. Fields at zero are left out:
//! a sample at rest is about 80 bytes, a day under 1 MB.
//!
//! ```text
//! {"start":1762500600,"interval":10}
//! {"t":1762500610,"c":[3,1],"m":[118234,62012],"w":2}
//! {"t":1762500620,"c":[812,345],"m":[138234,72012],"f":600,"s":620,"w":900,"i":120,
//!  "a":200,"k":400,"tu":4500,"fu":3800,"p":7200,"e":[[1762500613,"window-opened","foot"]]}
//! {"notified":"frames-at-rest","t":1762500620}
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::signals::SignalKind;

/// How often a sample is taken.
pub const INTERVAL_MS: u32 = 10_000;

/// The processes a sample measures, in the order of its lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Who {
    Compositor,
    Shell,
    Settings,
}

impl Who {
    pub const ALL: [Self; 3] = [Self::Compositor, Self::Shell, Self::Settings];

    pub fn index(self) -> usize {
        match self {
            Self::Compositor => 0,
            Self::Shell => 1,
            Self::Settings => 2,
        }
    }
}

/// What happened, written with the sample that follows it: what explains a
/// peak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    /// A window mapped; `what` is its application's id.
    WindowOpened,
    WindowClosed,
    /// A screen plugged (or the lid opened on one); `what` its connector.
    ScreenPlugged,
    ScreenUnplugged,
    Locked,
    Unlocked,
    /// The machine about to sleep, and woken.
    Suspend,
    Resume,
    /// A page of the Settings opened; `what` is its name.
    SettingsPage,
    /// The configuration changed on disk; `what` names its sections
    /// (`[appearance] [wallpaper]`).
    Config,
    OnBattery,
    OnAc,
    /// A moving wallpaper started, or stopped (covered, paused, changed).
    WallpaperMoving,
    WallpaperStill,
    /// A kind a later version writes.
    #[serde(other)]
    Other,
}

/// An event: when (Unix seconds), what kind, about what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "(i64, EventKind, String)", into = "(i64, EventKind, String)")]
pub struct Event {
    pub t: i64,
    pub kind: EventKind,
    pub what: String,
}

impl From<(i64, EventKind, String)> for Event {
    fn from((t, kind, what): (i64, EventKind, String)) -> Self {
        Self { t, kind, what }
    }
}

impl From<Event> for (i64, EventKind, String) {
    fn from(e: Event) -> Self {
        (e.t, e.kind, e.what)
    }
}

fn interval() -> u32 {
    INTERVAL_MS
}

fn is_interval(d: &u32) -> bool {
    *d == INTERVAL_MS
}

fn zero(n: &u32) -> bool {
    *n == 0
}

fn zero64(n: &u64) -> bool {
    *n == 0
}

/// One sample: what the interval ending at `t` cost. The counts are the
/// interval's; the memory is as of `t`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    /// Its end, Unix seconds.
    pub t: i64,
    /// Its length in milliseconds: 10 s, unless the session started or
    /// the machine slept within it.
    #[serde(default = "interval", skip_serializing_if = "is_interval")]
    pub d: u32,
    /// CPU time (user and system), milliseconds: the compositor, the
    /// shell, the Settings ([`Who`]'s order; the Settings' only while they
    /// run).
    #[serde(default)]
    pub c: Vec<u32>,
    /// Resident memory, kilobytes, in the same order.
    #[serde(default)]
    pub m: Vec<u32>,
    /// Frames drawn, all screens.
    #[serde(default, skip_serializing_if = "zero")]
    pub f: u32,
    /// Scenes built (a screen's elements gathered to draw, drawn or not).
    #[serde(default, skip_serializing_if = "zero")]
    pub s: u32,
    /// Turns of the compositor's loop: its wakeups.
    #[serde(default, skip_serializing_if = "zero")]
    pub w: u32,
    /// Input events from the user's devices.
    #[serde(default, skip_serializing_if = "zero")]
    pub i: u32,
    /// Frames drawn while one of the compositor's motions ran.
    #[serde(default, skip_serializing_if = "zero")]
    pub a: u32,
    /// Commits of the applications' windows (not the shell's surfaces).
    #[serde(default, skip_serializing_if = "zero")]
    pub k: u32,
    /// The longest turn of the loop, microseconds.
    #[serde(default, skip_serializing_if = "zero")]
    pub tu: u32,
    /// The longest frame, microseconds.
    #[serde(default, skip_serializing_if = "zero")]
    pub fu: u32,
    /// Allocations, when the program counts them.
    #[serde(default, skip_serializing_if = "zero64")]
    pub al: u64,
    /// The machine's power on battery, milliwatts; none on AC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<u32>,
    /// Each screen's frames and scenes, when there are several.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub o: BTreeMap<String, [u32; 2]>,
    /// What happened during it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub e: Vec<Event>,
}

impl Sample {
    /// When it starts, Unix seconds.
    pub fn from(&self) -> i64 {
        self.t - i64::from(self.d / 1000)
    }

    pub fn cpu_ms(&self, who: Who) -> u32 {
        self.c.get(who.index()).copied().unwrap_or(0)
    }

    pub fn rss_kb(&self, who: Who) -> u32 {
        self.m.get(who.index()).copied().unwrap_or(0)
    }

    /// The compositor's and the shell's CPU, in percent of one core.
    pub fn desktop_percent(&self) -> f64 {
        if self.d == 0 {
            return 0.0;
        }
        f64::from(self.cpu_ms(Who::Compositor) + self.cpu_ms(Who::Shell)) * 100.0
            / f64::from(self.d)
    }

    /// Nothing the user did, and no application drew: what the desktop
    /// draws then, it draws for nothing but its own motions.
    pub fn at_rest(&self) -> bool {
        self.i == 0 && self.k == 0
    }
}

/// A line of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    /// The recorder started (the session, or recording turned on).
    Start {
        start: i64,
        interval: u32,
    },
    /// A signal's notification was sent: once a day per kind, even across
    /// sessions.
    Notified {
        notified: SignalKind,
        t: i64,
    },
    Sample(Sample),
}

impl Line {
    /// Its text, without the line break.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A line of the file; `None` for one cut short or unknown.
    pub fn decode(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_at_rest_says_only_what_is_not_zero() {
        let s = Sample {
            t: 1_762_500_610,
            d: INTERVAL_MS,
            c: vec![3, 1],
            m: vec![118_234, 62_012],
            w: 2,
            ..Sample::default()
        };
        let line = Line::Sample(s.clone()).encode();
        assert_eq!(
            line,
            r#"{"t":1762500610,"c":[3,1],"m":[118234,62012],"w":2}"#
        );
        assert_eq!(Line::decode(&line), Some(Line::Sample(s)));
    }

    #[test]
    fn every_field_goes_and_comes_back() {
        let s = Sample {
            t: 1_762_500_620,
            d: 7_000,
            c: vec![812, 345, 120],
            m: vec![138_234, 72_012, 80_123],
            f: 600,
            s: 620,
            w: 900,
            i: 120,
            a: 200,
            k: 400,
            tu: 4_500,
            fu: 3_800,
            al: 123_456,
            p: Some(7_200),
            o: [
                ("DP-2".to_owned(), [300, 310]),
                ("eDP-1".to_owned(), [300, 310]),
            ]
            .into(),
            e: vec![Event {
                t: 1_762_500_613,
                kind: EventKind::WindowOpened,
                what: "foot".to_owned(),
            }],
        };
        let line = Line::Sample(s.clone()).encode();
        assert!(
            line.contains(r#""e":[[1762500613,"window-opened","foot"]]"#),
            "{line}"
        );
        assert_eq!(Line::decode(&line), Some(Line::Sample(s)));
    }

    #[test]
    fn the_other_lines_read_as_theirs() {
        let start = Line::Start {
            start: 1_762_500_600,
            interval: 10_000,
        };
        assert_eq!(Line::decode(&start.encode()), Some(start));
        let notified = Line::Notified {
            notified: SignalKind::FramesAtRest,
            t: 1_762_500_620,
        };
        assert_eq!(
            notified.encode(),
            r#"{"notified":"frames-at-rest","t":1762500620}"#
        );
        assert_eq!(Line::decode(&notified.encode()), Some(notified));
    }

    /// The budget: a day of 24 h, 10 of them at work (an event every
    /// minute), is under 1 MB.
    #[test]
    fn a_working_day_is_under_a_megabyte() {
        let mut bytes = 0;
        for i in 0..8_640_i64 {
            let working = i < 3_600;
            let s = if working {
                Sample {
                    t: 1_762_500_000 + 10 * i,
                    d: INTERVAL_MS,
                    c: vec![145, 60],
                    m: vec![138_234, 72_012],
                    f: 220,
                    s: 224,
                    w: 650,
                    i: 380,
                    a: 60,
                    k: 190,
                    tu: 4_500,
                    fu: 3_800,
                    p: Some(7_200),
                    e: if i % 6 == 0 {
                        vec![Event {
                            t: 1_762_500_000 + 10 * i - 4,
                            kind: EventKind::WindowOpened,
                            what: "org.mozilla.firefox".to_owned(),
                        }]
                    } else {
                        Vec::new()
                    },
                    ..Sample::default()
                }
            } else {
                Sample {
                    t: 1_762_500_000 + 10 * i,
                    d: INTERVAL_MS,
                    c: vec![3, 1],
                    m: vec![138_234, 72_012],
                    w: 2,
                    ..Sample::default()
                }
            };
            bytes += Line::Sample(s).encode().len() + 1;
        }
        assert!(bytes < 1_000_000, "{bytes} bytes");
    }

    #[test]
    fn a_line_cut_short_or_an_unknown_event_does_not_stop_the_day() {
        assert_eq!(Line::decode(r#"{"t":1762500610,"c":[3,"#), None);
        let Some(Line::Sample(s)) =
            Line::decode(r#"{"t":1,"c":[1,1],"e":[[1,"something-later","x"]]}"#)
        else {
            panic!("a sample");
        };
        assert_eq!(s.e[0].kind, EventKind::Other);
    }
}
