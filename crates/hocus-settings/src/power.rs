//! Énergie / Power (6.10, docs/design/settings.md "Énergie"): when the
//! screen dims, locks and turns off, when the computer sleeps, on battery
//! and on AC (7.2's order); locking; the lid and the power button; the
//! power mode and the automatic power saver. The compositor acts on every
//! key, live (docs/design/idle.md).
//!
//! The dim's delay is `[idle] delay` on AC (seconds, or `"off"`) and
//! `[power.battery] idle` on battery; the others are `[power.*]
//! screen-off-after`, `suspend-after` (`"0s"`, `"10s"`, `"5m"`, `"1h"`,
//! `"never"`), `[power] lock-on-idle`, `lock-before-sleep`, `lid`,
//! `lid-ignored-when-docked`, `power-button`, `auto-saver`,
//! `auto-saver-below`. 6.1's `[power.battery] screen-off` and `[power.*]
//! lock-after` are read where the new keys are absent (`store`), never
//! written. The power mode is power-profiles-daemon's, not the file's.

use hocus_words::settings::power as words;
use std::sync::OnceLock;

use std::time::Duration;

use crate::{
    lang::{Lang, Words},
    model::{Choice, Codec, Kind, Outside, Place, Places, RowGroup, SectionId, Setting, Value},
    power_bus::Charge,
};

const SCREEN: RowGroup = RowGroup {
    label: words::SCREEN_AND_SLEEP,
    per_source: true,
};
const LOCKING: RowGroup = RowGroup {
    label: words::LOCKING,
    per_source: false,
};
const LID: RowGroup = RowGroup {
    label: words::LID_AND_BUTTON,
    per_source: false,
};
const SAVING: RowGroup = RowGroup {
    label: words::POWER_SAVING,
    per_source: false,
};

/// The rows' ids.
pub const IDLE: &str = "power.idle";
pub const SCREEN_OFF: &str = "power.screen-off-after";
pub const SUSPEND: &str = "power.suspend-after";
pub const LOCK_ON_IDLE: &str = "power.lock-on-idle";
pub const LOCK_BEFORE_SLEEP: &str = "power.lock-before-sleep";
pub const LID_CLOSED: &str = "power.lid";
pub const DOCKED: &str = "power.lid-ignored-when-docked";
pub const BUTTON: &str = "power.power-button";
pub const AUTO_SAVER: &str = "power.auto-saver";
pub const SAVER_BELOW: &str = "power.auto-saver-below";
/// The rows outside the file.
pub const BATTERY: &str = "power.battery";
pub const MODE: &str = "power.mode";
pub const CHARGE_LIMIT: &str = "power.charge-limit";

const LIDS: &[&str] = &["suspend", "lock", "screen-off", "nothing"];
const BUTTONS: &[&str] = &["ask", "suspend", "power-off", "nothing"];

fn minutes(text: hocus_words::Text, seconds: Option<u64>) -> Choice {
    Choice {
        label: text,
        value: Value::Seconds(seconds),
    }
}

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: text,
        value: Value::Word(word),
    }
}

/// The page's settings, in its order.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let never = minutes(words::NEVER_2, None);
        vec![
            Setting {
                id: IDLE,
                section: SectionId::Power,
                group: SCREEN,
                name: words::DIM_THE_SCREEN_AFTER,
                line: Some(words::WITH_NO_CLICK_OR_KEY_MOVING),
                keywords: (
                    &[
                        "rideau",
                        "assombrir",
                        "sombre",
                        "écran noir",
                        "inactivité",
                        "inactif",
                        "économiseur",
                    ],
                    &[
                        "dim",
                        "curtain",
                        "idle",
                        "blank",
                        "inactivity",
                        "screen saver",
                    ],
                ),
                kind: Kind::Menu(vec![
                    minutes(words::N_1_MIN, Some(60)),
                    minutes(words::N_2_MIN, Some(120)),
                    minutes(words::N_3_MIN, Some(180)),
                    minutes(words::N_5_MIN, Some(300)),
                    minutes(words::N_10_MIN, Some(600)),
                    minutes(words::N_15_MIN, Some(900)),
                    minutes(words::N_30_MIN, Some(1800)),
                    never.clone(),
                ]),
                places: Places::PerSource {
                    // The compositor's defaults (7.2): 3 min on battery,
                    // 5 on AC.
                    battery: Place::new(&["power", "battery", "idle"], Codec::Delay),
                    ac: Place::new(&["idle", "delay"], Codec::IdleSeconds),
                },
                needs: None,
            },
            Setting {
                id: SCREEN_OFF,
                section: SectionId::Power,
                group: SCREEN,
                name: words::TURN_THE_SCREEN_OFF,
                line: Some(words::ONCE_LOCKED_IF_NOTHING_HAPPENS_STILL),
                keywords: (
                    &["extinction", "veille de l’écran", "éteindre", "noir"],
                    &["screen off", "display off", "dpms", "blank"],
                ),
                kind: Kind::Menu(vec![
                    minutes(words::AT_ONCE, Some(0)),
                    minutes(words::N_10_S_AFTER, Some(10)),
                    minutes(words::N_30_S_AFTER, Some(30)),
                    minutes(words::N_1_MIN_AFTER, Some(60)),
                    minutes(words::N_5_MIN_AFTER, Some(300)),
                ]),
                places: Places::PerSource {
                    battery: Place::new(&["power", "battery", "screen-off-after"], Codec::Delay),
                    ac: Place::new(&["power", "ac", "screen-off-after"], Codec::Delay),
                },
                needs: None,
            },
            Setting {
                id: SUSPEND,
                section: SectionId::Power,
                group: SCREEN,
                name: words::SUSPEND_AFTER,
                line: Some(words::SINCE_THE_LAST_ACTIVITY_THE_SESSION),
                keywords: (
                    &["suspendre", "sommeil", "dormir", "inactivité"],
                    &["sleep", "suspend", "standby", "hibernate", "idle"],
                ),
                kind: Kind::Menu(vec![
                    minutes(words::N_5_MIN, Some(300)),
                    minutes(words::N_15_MIN, Some(900)),
                    minutes(words::N_30_MIN, Some(1800)),
                    minutes(words::N_1_H, Some(3600)),
                    minutes(words::N_2_H, Some(7200)),
                    never,
                ]),
                places: Places::PerSource {
                    battery: Place::new(&["power", "battery", "suspend-after"], Codec::Delay),
                    ac: Place::new(&["power", "ac", "suspend-after"], Codec::Delay),
                },
                needs: None,
            },
            Setting {
                id: LOCK_ON_IDLE,
                section: SectionId::Power,
                group: LOCKING,
                name: words::LOCK_AFTER_DIMMING,
                line: Some(words::OFF_THE_SCREEN_DIMS_THEN_TURNS),
                keywords: (
                    &["verrou", "verrouillage", "mot de passe", "sécurité"],
                    &["lock", "auto-lock", "password", "security"],
                ),
                kind: Kind::Switch,
                places: Places::One(Place::new(&["power", "lock-on-idle"], Codec::Bool)),
                needs: None,
            },
            Setting {
                id: LOCK_BEFORE_SLEEP,
                section: SectionId::Power,
                group: LOCKING,
                name: words::LOCK_BEFORE_SLEEPING,
                line: Some(words::THE_LOCK_SCREEN_IS_ALREADY_THERE),
                keywords: (
                    &["réveil", "verrou", "veille"],
                    &["lock on suspend", "resume", "wake"],
                ),
                kind: Kind::Switch,
                places: Places::One(Place::new(&["power", "lock-before-sleep"], Codec::Bool)),
                needs: None,
            },
            Setting {
                id: LID_CLOSED,
                section: SectionId::Power,
                group: LID,
                name: words::LID_CLOSED,
                line: None,
                keywords: (
                    &["capot", "fermer", "portable", "veille"],
                    &["lid", "laptop", "close", "sleep"],
                ),
                kind: Kind::Menu(vec![
                    word(words::SUSPEND, "suspend"),
                    word(words::LOCK, "lock"),
                    word(words::TURN_THE_SCREEN_OFF, "screen-off"),
                    word(words::DO_NOTHING, "nothing"),
                ]),
                places: Places::One(Place::new(&["power", "lid"], Codec::Word(LIDS))),
                needs: None,
            },
            Setting {
                id: DOCKED,
                section: SectionId::Power,
                group: LID,
                name: words::STAY_ON_WITH_A_SCREEN_PLUGGED,
                line: Some(words::LID_CLOSED_ON_AC_AN_EXTERNAL),
                keywords: (
                    &["écran externe", "station", "dock", "moniteur", "couvercle"],
                    &["external screen", "dock", "monitor", "docked", "lid"],
                ),
                kind: Kind::Switch,
                places: Places::One(Place::new(
                    &["power", "lid-ignored-when-docked"],
                    Codec::Bool,
                )),
                needs: None,
            },
            Setting {
                id: BUTTON,
                section: SectionId::Power,
                group: LID,
                name: words::POWER_BUTTON,
                line: Some(words::A_SHORT_PRESS_A_LONG_PRESS),
                keywords: (
                    &["power", "arrêt", "éteindre", "veille"],
                    &["power", "shut down", "power off", "sleep"],
                ),
                kind: Kind::Menu(vec![
                    word(words::ASK_WHAT_TO_DO, "ask"),
                    word(words::SUSPEND, "suspend"),
                    word(words::POWER_OFF, "power-off"),
                    word(words::DO_NOTHING, "nothing"),
                ]),
                places: Places::One(Place::new(&["power", "power-button"], Codec::Word(BUTTONS))),
                needs: None,
            },
            Setting {
                id: AUTO_SAVER,
                section: SectionId::Power,
                group: SAVING,
                name: words::AUTOMATIC_POWER_SAVER,
                line: Some(words::TURNS_ITSELF_ON_UNDER_A_BATTERY),
                keywords: (
                    &["batterie", "autonomie", "économiser"],
                    &["battery", "battery life", "power saving", "saver"],
                ),
                kind: Kind::Switch,
                places: Places::One(Place::new(&["power", "auto-saver"], Codec::Bool)),
                needs: None,
            },
            Setting {
                id: SAVER_BELOW,
                section: SectionId::Power,
                group: SAVING,
                name: words::UNDER_THIS_BATTERY_LEVEL,
                line: None,
                keywords: (
                    &["batterie", "pourcentage", "niveau", "économie"],
                    &["battery", "percent", "level", "saver"],
                ),
                kind: Kind::Slider {
                    min: 5,
                    max: 50,
                    step: 5,
                    unit: " %",
                },
                places: Places::One(Place::new(
                    &["power", "auto-saver-below"],
                    Codec::Int { min: 5, max: 50 },
                )),
                needs: Some(AUTO_SAVER),
            },
        ]
    })
}

/// The page's rows outside the file, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const BATTERY_GROUP: Words = words::BATTERY;
    const LATER: Words = words::LATER;
    const ALL: [Outside; 3] = [
        Outside {
            id: BATTERY,
            section: SectionId::Power,
            group: BATTERY_GROUP,
            name: words::BATTERY_HEALTH,
            keywords: (
                &["santé", "capacité", "usure", "autonomie", "batterie"],
                &["battery health", "capacity", "wear", "time left", "battery"],
            ),
        },
        Outside {
            id: MODE,
            section: SectionId::Power,
            group: SAVING.label,
            name: words::POWER_MODE,
            keywords: (
                &["profil", "performance", "économie", "équilibré"],
                &[
                    "power mode",
                    "profile",
                    "balanced",
                    "power saver",
                    "performance",
                ],
            ),
        },
        Outside {
            id: CHARGE_LIMIT,
            section: SectionId::Power,
            group: LATER,
            name: words::LIMIT_CHARGING_TO_80,
            keywords: (
                &["80 %", "seuil", "charge"],
                &["charge limit", "threshold", "charging"],
            ),
        },
    ];
    &ALL
}

/// power-profiles-daemon's modes, in the control's order.
pub const MODES: [&str; 3] = ["power-saver", "balanced", "performance"];

/// A mode's name: "Économie", "Équilibré", "Performance".
pub fn mode_name(mode: &str, lang: Lang) -> &'static str {
    match mode {
        "power-saver" => words::POWER_SAVER.get(lang),
        "performance" => "Performance",
        _ => words::BALANCED.get(lang),
    }
}

/// Why performance is held back (power-profiles-daemon's
/// `PerformanceDegraded`), as the battery's popover says it (3.7).
pub fn degraded(reason: &str, lang: Lang) -> Option<&'static str> {
    Some(
        match reason {
            "" => return None,
            "lap-detected" => words::PERFORMANCE_REDUCED_THE_COMPUTER_IS_ON,
            "high-operating-temperature" => words::PERFORMANCE_REDUCED_THE_COMPUTER_IS_TOO,
            _ => words::PERFORMANCE_REDUCED_BY_THE_SYSTEM,
        }
        .get(lang),
    )
}

/// A time left, to the minute: `35 min`, `3 h 40`, `2 h`.
pub fn duration(time: Duration) -> String {
    let minutes = ((time.as_secs_f64() / 60.0).round() as u64).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// The battery card's line, 3.7's words: "Sur batterie · 3 h 40
/// restantes", "Branché · en charge, plein dans 25 min", "Branché, plein",
/// "Branché, ne charge pas".
pub fn battery_line(charge: Charge, left: Option<Duration>, lang: Lang) -> String {
    let fr = lang == Lang::French;
    match (charge, left) {
        (Charge::Discharging, Some(t)) if fr => format!("Sur batterie · {} restantes", duration(t)),
        (Charge::Discharging, Some(t)) => format!("On battery · {} left", duration(t)),
        (Charge::Discharging, None) => words::ON_BATTERY.get(lang).to_owned(),
        (Charge::Charging, Some(t)) if fr => {
            format!("Branché · en charge, plein dans {}", duration(t))
        }
        (Charge::Charging, Some(t)) => format!("Plugged in · charging, full in {}", duration(t)),
        (Charge::Charging, None) => words::PLUGGED_IN_CHARGING.get(lang).to_owned(),
        (Charge::Full, _) => words::PLUGGED_IN_FULL.get(lang).to_owned(),
        (Charge::Held | Charge::Unknown, _) => words::PLUGGED_IN_NOT_CHARGING.get(lang).to_owned(),
    }
}

/// A step of the timeline under the delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// The last activity, at 0.
    Start,
    Dim,
    Lock,
    Off,
    Sleep,
    /// No sleep: the line's end.
    NoSleep,
}

/// A step: its kind, its time above its dot, and where it is along the
/// line (0 to 1, in order, not to scale).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: StepKind,
    pub time: String,
    pub at: f32,
}

impl StepKind {
    /// Its name, under its dot.
    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Self::Start => words::LAST_ACTIVITY,
            Self::Dim => words::DIMMED,
            Self::Lock => words::LOCKED,
            Self::Off => words::SCREEN_OFF,
            Self::Sleep => words::ASLEEP,
            Self::NoSleep => words::NO_SLEEP,
        }
        .get(lang)
    }
}

/// The curtain's length (7.2).
const CURTAIN: u64 = 10;
/// A step at least this far from the next, along the line.
const GAP: f32 = 0.12;

fn short(seconds: u64) -> String {
    match seconds {
        s if s < 60 => format!("{s} s"),
        s if s % 3600 == 0 => format!("{} h", s / 3600),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h {:02}", s / 3600, (s % 3600) / 60),
    }
}

/// The timeline for these delays (seconds; `None`: never): the dim, the
/// lock, the screen off after it (or after the dim, locking off), the
/// sleep since the last activity. A step that is off is not drawn; a
/// sleep sooner than the dim comes first.
pub fn timeline(
    dim: Option<u64>,
    lock: bool,
    off: Option<u64>,
    sleep: Option<u64>,
    lang: Lang,
) -> Vec<Step> {
    // (kind, time label, when since the last activity).
    let mut steps: Vec<(StepKind, String, u64)> = vec![(StepKind::Start, "0".to_owned(), 0)];
    if let Some(dim) = dim {
        steps.push((StepKind::Dim, short(dim), dim));
        if lock {
            let locked = dim + CURTAIN;
            steps.push((StepKind::Lock, format!("+{CURTAIN} s"), locked));
            if let Some(off) = off {
                steps.push((StepKind::Off, format!("+{}", short(off)), locked + off));
            }
        } else if let Some(off) = off {
            let dark = dim + CURTAIN + off;
            steps.push((StepKind::Off, format!("+{}", short(CURTAIN + off)), dark));
        }
    }
    match sleep {
        Some(sleep) => steps.push((StepKind::Sleep, short(sleep), sleep)),
        None => steps.push((
            StepKind::NoSleep,
            words::NEVER.get(lang).to_owned(),
            u64::MAX,
        )),
    }
    // In time's order; the sleep before what it comes sooner than.
    steps.sort_by_key(|(kind, _, at)| (*at, *kind == StepKind::Sleep));
    // Not to scale: by time, then each step at least `GAP` from the last,
    // the end at 1.
    let finite: Vec<u64> = steps
        .iter()
        .map(|(_, _, at)| *at)
        .filter(|at| *at != u64::MAX)
        .collect();
    let longest = finite.iter().copied().max().unwrap_or(1).max(1) as f32;
    let open_end = steps.last().is_some_and(|(_, _, at)| *at == u64::MAX);
    let span = if open_end { 0.7 } else { 1.0 };
    let n = steps.len();
    let mut at: Vec<f32> = steps
        .iter()
        .map(|(_, _, t)| {
            if *t == u64::MAX {
                1.0
            } else {
                span * (*t as f32) / longest
            }
        })
        .collect();
    for i in 1..n {
        at[i] = at[i].max(at[i - 1] + GAP);
    }
    if let Some(last) = at.last_mut() {
        *last = 1.0;
    }
    // Room for those before the end.
    for i in (0..n.saturating_sub(1)).rev() {
        at[i] = at[i].min(at[i + 1] - GAP).max(0.0);
    }
    if let Some(first) = at.first_mut() {
        *first = 0.0;
    }
    steps
        .into_iter()
        .zip(at)
        .map(|((kind, time, _), at)| Step { kind, time, at })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lang::Lang, model::Source};

    #[test]
    fn every_default_is_a_choice_and_writes() {
        for setting in settings() {
            for place in setting.places.all() {
                assert!(
                    place.codec.write(&place.default).is_some(),
                    "{}: {place:?}",
                    setting.id
                );
                if let Kind::Menu(choices) = &setting.kind {
                    assert!(
                        choices.iter().any(|c| c.value == place.default),
                        "{}'s default is not among its choices",
                        setting.id
                    );
                    for choice in choices {
                        assert!(place.codec.write(&choice.value).is_some(), "{choice:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_timeline_follows_the_delays_in_order_not_to_scale() {
        let fr = Lang::French;
        let steps = timeline(Some(180), true, Some(10), Some(900), fr);
        let kinds: Vec<StepKind> = steps.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                StepKind::Start,
                StepKind::Dim,
                StepKind::Lock,
                StepKind::Off,
                StepKind::Sleep
            ]
        );
        let times: Vec<&str> = steps.iter().map(|s| s.time.as_str()).collect();
        assert_eq!(times, ["0", "3 min", "+10 s", "+10 s", "15 min"]);
        let at: Vec<f32> = steps.iter().map(|s| (s.at * 100.0).round()).collect();
        assert_eq!(at, [0.0, 20.0, 32.0, 44.0, 100.0]);
        // Locking off: the screen off after the dim; no sleep at the end.
        let steps = timeline(Some(300), false, Some(30), None, fr);
        let kinds: Vec<StepKind> = steps.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                StepKind::Start,
                StepKind::Dim,
                StepKind::Off,
                StepKind::NoSleep
            ]
        );
        assert_eq!(steps[2].time, "+40 s");
        // A sleep sooner than the dim first; the dim never: not drawn.
        let steps = timeline(Some(1800), true, Some(30), Some(300), fr);
        assert_eq!(steps[1].kind, StepKind::Sleep);
        let steps = timeline(None, true, Some(30), Some(900), fr);
        assert_eq!(steps.len(), 2);
        for pair in timeline(Some(60), true, Some(0), Some(300), fr).windows(2) {
            assert!(pair[1].at - pair[0].at >= GAP - 1e-4, "{pair:?}");
        }
    }

    #[test]
    fn the_batterys_line_says_it_as_the_popover() {
        let fr = Lang::French;
        let hours = Some(Duration::from_secs(3 * 3600 + 40 * 60));
        assert_eq!(
            battery_line(Charge::Discharging, hours, fr),
            "Sur batterie · 3 h 40 restantes"
        );
        assert_eq!(
            battery_line(Charge::Charging, Some(Duration::from_secs(25 * 60)), fr),
            "Branché · en charge, plein dans 25 min"
        );
        assert_eq!(battery_line(Charge::Full, None, fr), "Branché, plein");
        assert_eq!(
            battery_line(Charge::Held, None, fr),
            "Branché, ne charge pas"
        );
    }

    #[test]
    fn the_dims_delay_on_ac_is_the_idle_delay() {
        let screen = &settings()[0];
        let ac = screen.places.of(Source::Ac);
        assert_eq!(ac.shown(), "[idle] delay");
        assert_eq!(
            ac.codec
                .write(&Value::Seconds(Some(900)))
                .map(|v| v.to_string()),
            Some("900".to_owned())
        );
        assert_eq!(
            screen.places.of(Source::Battery).shown(),
            "[power.battery] idle"
        );
        assert_eq!(screen.say(&Value::Seconds(None), Lang::French), "Jamais");
    }
}
