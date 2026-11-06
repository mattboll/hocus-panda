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

use std::sync::OnceLock;

use std::time::Duration;

use crate::{
    lang::{Lang, Words, w},
    model::{Choice, Codec, Kind, Outside, Place, Places, RowGroup, SectionId, Setting, Value},
    power_bus::Charge,
};

const SCREEN: RowGroup = RowGroup {
    label: w("ÉCRAN ET VEILLE", "SCREEN AND SLEEP"),
    per_source: true,
};
const LOCKING: RowGroup = RowGroup {
    label: w("VERROUILLAGE", "LOCKING"),
    per_source: false,
};
const LID: RowGroup = RowGroup {
    label: w("COUVERCLE ET BOUTON", "LID AND BUTTON"),
    per_source: false,
};
const SAVING: RowGroup = RowGroup {
    label: w("ÉCONOMIE D’ÉNERGIE", "POWER SAVING"),
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

fn minutes(fr: &'static str, en: &'static str, seconds: Option<u64>) -> Choice {
    Choice {
        label: w(fr, en),
        value: Value::Seconds(seconds),
    }
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: w(fr, en),
        value: Value::Word(word),
    }
}

fn place(key: &'static [&'static str], codec: Codec, default: Value) -> Place {
    Place {
        key,
        codec,
        default,
    }
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let never = minutes("Jamais", "Never", None);
        vec![
            Setting {
                id: IDLE,
                section: SectionId::Power,
                group: SCREEN,
                name: w("Assombrir l’écran après", "Dim the screen after"),
                line: Some(w(
                    "Sans clic ni touche. Bouger la souris pendant les 10 s d’assombrissement l’arrête.",
                    "With no click or key. Moving the mouse during the 10 s of dimming stops it.",
                )),
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
                    &["dim", "curtain", "idle", "blank", "inactivity", "screen saver"],
                ),
                kind: Kind::Menu(vec![
                    minutes("1 min", "1 min", Some(60)),
                    minutes("2 min", "2 min", Some(120)),
                    minutes("3 min", "3 min", Some(180)),
                    minutes("5 min", "5 min", Some(300)),
                    minutes("10 min", "10 min", Some(600)),
                    minutes("15 min", "15 min", Some(900)),
                    minutes("30 min", "30 min", Some(1800)),
                    never.clone(),
                ]),
                places: Places::PerSource {
                    // The compositor's defaults (7.2): 3 min on battery,
                    // 5 on AC.
                    battery: place(
                        &["power", "battery", "idle"],
                        Codec::Delay,
                        Value::Seconds(Some(180)),
                    ),
                    ac: place(
                        &["idle", "delay"],
                        Codec::IdleSeconds,
                        Value::Seconds(Some(300)),
                    ),
                },
                needs: None,
            },
            Setting {
                id: SCREEN_OFF,
                section: SectionId::Power,
                group: SCREEN,
                name: w("Éteindre l’écran", "Turn the screen off"),
                line: Some(w(
                    "Une fois verrouillé, s’il ne se passe toujours rien.",
                    "Once locked, if nothing happens still.",
                )),
                keywords: (
                    &["extinction", "veille de l’écran", "éteindre", "noir"],
                    &["screen off", "display off", "dpms", "blank"],
                ),
                kind: Kind::Menu(vec![
                    minutes("Aussitôt", "At once", Some(0)),
                    minutes("10 s après", "10 s after", Some(10)),
                    minutes("30 s après", "30 s after", Some(30)),
                    minutes("1 min après", "1 min after", Some(60)),
                    minutes("5 min après", "5 min after", Some(300)),
                ]),
                places: Places::PerSource {
                    battery: place(
                        &["power", "battery", "screen-off-after"],
                        Codec::Delay,
                        Value::Seconds(Some(10)),
                    ),
                    ac: place(
                        &["power", "ac", "screen-off-after"],
                        Codec::Delay,
                        Value::Seconds(Some(30)),
                    ),
                },
                needs: None,
            },
            Setting {
                id: SUSPEND,
                section: SectionId::Power,
                group: SCREEN,
                name: w("Mettre en veille après", "Suspend after"),
                line: Some(w(
                    "Depuis la dernière activité ; la session reste ouverte, verrouillée.",
                    "Since the last activity; the session stays open, locked.",
                )),
                keywords: (
                    &["suspendre", "sommeil", "dormir", "inactivité"],
                    &["sleep", "suspend", "standby", "hibernate", "idle"],
                ),
                kind: Kind::Menu(vec![
                    minutes("5 min", "5 min", Some(300)),
                    minutes("15 min", "15 min", Some(900)),
                    minutes("30 min", "30 min", Some(1800)),
                    minutes("1 h", "1 h", Some(3600)),
                    minutes("2 h", "2 h", Some(7200)),
                    never,
                ]),
                places: Places::PerSource {
                    battery: place(
                        &["power", "battery", "suspend-after"],
                        Codec::Delay,
                        Value::Seconds(Some(900)),
                    ),
                    ac: place(
                        &["power", "ac", "suspend-after"],
                        Codec::Delay,
                        Value::Seconds(None),
                    ),
                },
                needs: None,
            },
            Setting {
                id: LOCK_ON_IDLE,
                section: SectionId::Power,
                group: LOCKING,
                name: w(
                    "Verrouiller après l’assombrissement",
                    "Lock after dimming",
                ),
                line: Some(w(
                    "Coupé, l’écran s’assombrit puis s’éteint sans verrouiller.",
                    "Off, the screen dims then turns off without locking.",
                )),
                keywords: (
                    &["verrou", "verrouillage", "mot de passe", "sécurité"],
                    &["lock", "auto-lock", "password", "security"],
                ),
                kind: Kind::Switch,
                places: Places::One(place(
                    &["power", "lock-on-idle"],
                    Codec::Bool,
                    Value::Bool(true),
                )),
                needs: None,
            },
            Setting {
                id: LOCK_BEFORE_SLEEP,
                section: SectionId::Power,
                group: LOCKING,
                name: w(
                    "Verrouiller avant la mise en veille",
                    "Lock before sleeping",
                ),
                line: Some(w(
                    "L’écran verrouillé est déjà là au réveil, sans montrer le bureau.",
                    "The lock screen is already there on waking, without showing the desktop.",
                )),
                keywords: (
                    &["réveil", "verrou", "veille"],
                    &["lock on suspend", "resume", "wake"],
                ),
                kind: Kind::Switch,
                places: Places::One(place(
                    &["power", "lock-before-sleep"],
                    Codec::Bool,
                    Value::Bool(true),
                )),
                needs: None,
            },
            Setting {
                id: LID_CLOSED,
                section: SectionId::Power,
                group: LID,
                name: w("Couvercle fermé", "Lid closed"),
                line: None,
                keywords: (
                    &["capot", "fermer", "portable", "veille"],
                    &["lid", "laptop", "close", "sleep"],
                ),
                kind: Kind::Menu(vec![
                    word("Mettre en veille", "Suspend", "suspend"),
                    word("Verrouiller", "Lock", "lock"),
                    word("Éteindre l’écran", "Turn the screen off", "screen-off"),
                    word("Ne rien faire", "Do nothing", "nothing"),
                ]),
                places: Places::One(place(
                    &["power", "lid"],
                    Codec::Word(LIDS),
                    Value::Word("suspend"),
                )),
                needs: None,
            },
            Setting {
                id: DOCKED,
                section: SectionId::Power,
                group: LID,
                name: w(
                    "Rester allumé avec un écran branché",
                    "Stay on with a screen plugged in",
                ),
                line: Some(w(
                    "Couvercle fermé, sur secteur, un écran externe allumé.",
                    "Lid closed, on AC, an external screen on.",
                )),
                keywords: (
                    &["écran externe", "station", "dock", "moniteur", "couvercle"],
                    &["external screen", "dock", "monitor", "docked", "lid"],
                ),
                kind: Kind::Switch,
                places: Places::One(place(
                    &["power", "lid-ignored-when-docked"],
                    Codec::Bool,
                    Value::Bool(true),
                )),
                needs: None,
            },
            Setting {
                id: BUTTON,
                section: SectionId::Power,
                group: LID,
                name: w("Bouton d’alimentation", "Power button"),
                line: Some(w(
                    "Un appui court ; un appui long éteint toujours.",
                    "A short press; a long press always powers off.",
                )),
                keywords: (
                    &["power", "arrêt", "éteindre", "veille"],
                    &["power", "shut down", "power off", "sleep"],
                ),
                kind: Kind::Menu(vec![
                    word("Demander quoi faire", "Ask what to do", "ask"),
                    word("Mettre en veille", "Suspend", "suspend"),
                    word("Éteindre", "Power off", "power-off"),
                    word("Ne rien faire", "Do nothing", "nothing"),
                ]),
                places: Places::One(place(
                    &["power", "power-button"],
                    Codec::Word(BUTTONS),
                    Value::Word("ask"),
                )),
                needs: None,
            },
            Setting {
                id: AUTO_SAVER,
                section: SectionId::Power,
                group: SAVING,
                name: w("Économie d’énergie automatique", "Automatic power saver"),
                line: Some(w(
                    "S’allume seule sous un niveau de batterie, s’éteint sur secteur.",
                    "Turns itself on under a battery level, off on AC.",
                )),
                keywords: (
                    &["batterie", "autonomie", "économiser"],
                    &["battery", "battery life", "power saving", "saver"],
                ),
                kind: Kind::Switch,
                places: Places::One(place(
                    &["power", "auto-saver"],
                    Codec::Bool,
                    Value::Bool(true),
                )),
                needs: None,
            },
            Setting {
                id: SAVER_BELOW,
                section: SectionId::Power,
                group: SAVING,
                name: w("Sous ce niveau de batterie", "Under this battery level"),
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
                places: Places::One(place(
                    &["power", "auto-saver-below"],
                    Codec::Int { min: 5, max: 50 },
                    Value::Int(20),
                )),
                needs: Some(AUTO_SAVER),
            },
        ]
    })
}

/// The page's rows outside the file, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const BATTERY_GROUP: Words = w("BATTERIE", "BATTERY");
    const LATER: Words = w("PLUS TARD", "LATER");
    const ALL: [Outside; 3] = [
        Outside {
            id: BATTERY,
            section: SectionId::Power,
            group: BATTERY_GROUP,
            name: w("État de la batterie", "Battery health"),
            keywords: (
                &["santé", "capacité", "usure", "autonomie", "batterie"],
                &["battery health", "capacity", "wear", "time left", "battery"],
            ),
        },
        Outside {
            id: MODE,
            section: SectionId::Power,
            group: SAVING.label,
            name: w("Mode d’énergie", "Power mode"),
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
            name: w("Limiter la charge à 80 %", "Limit charging to 80 %"),
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
        "power-saver" => w("Économie", "Power saver").get(lang),
        "performance" => "Performance",
        _ => w("Équilibré", "Balanced").get(lang),
    }
}

/// Why performance is held back (power-profiles-daemon's
/// `PerformanceDegraded`), as the battery's popover says it (3.7).
pub fn degraded(reason: &str, lang: Lang) -> Option<&'static str> {
    Some(
        match reason {
            "" => return None,
            "lap-detected" => w(
                "Performance réduite\u{a0}: l’ordinateur est sur tes genoux.",
                "Performance reduced: the computer is on a lap.",
            ),
            "high-operating-temperature" => w(
                "Performance réduite\u{a0}: l’ordinateur est trop chaud.",
                "Performance reduced: the computer is too hot.",
            ),
            _ => w(
                "Performance réduite par le système.",
                "Performance reduced by the system.",
            ),
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
        (Charge::Discharging, None) => w("Sur batterie", "On battery").get(lang).to_owned(),
        (Charge::Charging, Some(t)) if fr => {
            format!("Branché · en charge, plein dans {}", duration(t))
        }
        (Charge::Charging, Some(t)) => format!("Plugged in · charging, full in {}", duration(t)),
        (Charge::Charging, None) => w("Branché · en charge", "Plugged in · charging")
            .get(lang)
            .to_owned(),
        (Charge::Full, _) => w("Branché, plein", "Plugged in, full").get(lang).to_owned(),
        (Charge::Held | Charge::Unknown, _) => {
            w("Branché, ne charge pas", "Plugged in, not charging")
                .get(lang)
                .to_owned()
        }
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
            Self::Start => w("dernière activité", "last activity"),
            Self::Dim => w("assombri", "dimmed"),
            Self::Lock => w("verrouillé", "locked"),
            Self::Off => w("écran éteint", "screen off"),
            Self::Sleep => w("en veille", "asleep"),
            Self::NoSleep => w("pas de veille", "no sleep"),
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
            w("jamais", "never").get(lang).to_owned(),
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
