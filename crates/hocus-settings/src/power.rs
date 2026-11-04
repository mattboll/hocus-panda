//! Énergie / Power (the 6.1 mockup's page, filled in as the first
//! section; 6.10 makes the desktop act on it): when the screen turns off,
//! when the session locks, when the computer sleeps, on battery and on AC;
//! the lid and the power button; the automatic power saver.
//!
//! The compositor reads the screen's and the lock's rows (7.2,
//! docs/design/idle.md): the idle delay, after which the curtain comes,
//! is `[idle] delay` on AC (seconds, or `"off"`; 5 min by default) and
//! `[power.battery] screen-off` on battery (3 min); `[power.ac]` and
//! `[power.battery]` `lock-after` say when the session locks once idle
//! (`"0s"`: as the curtain ends, the default; a delay: that long after the
//! screens went off; `"never"`). The other keys are 6.10's proposal,
//! which nothing reads yet: `suspend-after` (`"5m"`, `"1h"`, `"never"`),
//! and `[power]` `lid`, `lid-ignored-when-docked`, `power-button`,
//! `auto-saver`, `auto-saver-below`.

use std::sync::OnceLock;

use crate::{
    lang::w,
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

const SCREEN: RowGroup = RowGroup {
    label: w("ÉCRAN ET VEILLE", "SCREEN AND SLEEP"),
    per_source: true,
};
const LID: RowGroup = RowGroup {
    label: w("COUVERCLE ET BOUTON", "LID AND BUTTON"),
    per_source: false,
};
const SAVING: RowGroup = RowGroup {
    label: w("ÉCONOMIE D’ÉNERGIE", "POWER SAVING"),
    per_source: false,
};

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
                id: "power.screen-off",
                section: SectionId::Power,
                group: SCREEN,
                name: w("Éteindre l’écran après", "Turn the screen off after"),
                line: Some(w(
                    "Sans clic ni touche pendant ce temps.",
                    "With no click or key for that long.",
                )),
                keywords: (
                    &[
                        "écran noir",
                        "extinction",
                        "éteindre",
                        "économiseur",
                        "inactivité",
                        "inactif",
                    ],
                    &["blank", "screen saver", "idle", "inactivity", "display off"],
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
                        &["power", "battery", "screen-off"],
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
                id: "power.lock-after",
                section: SectionId::Power,
                group: SCREEN,
                name: w("Verrouiller la session", "Lock the session"),
                line: Some(w("Une fois l’écran éteint.", "Once the screen is off.")),
                keywords: (
                    &["verrou", "verrouillage", "mot de passe", "sécurité"],
                    &["lock", "password", "security"],
                ),
                kind: Kind::Menu(vec![
                    minutes("Aussitôt", "At once", Some(0)),
                    minutes("Après 1 min", "After 1 min", Some(60)),
                    minutes("Après 5 min", "After 5 min", Some(300)),
                    never.clone(),
                ]),
                places: Places::PerSource {
                    battery: place(
                        &["power", "battery", "lock-after"],
                        Codec::Delay,
                        Value::Seconds(Some(0)),
                    ),
                    ac: place(
                        &["power", "ac", "lock-after"],
                        Codec::Delay,
                        Value::Seconds(Some(0)),
                    ),
                },
                needs: None,
            },
            Setting {
                id: "power.suspend-after",
                section: SectionId::Power,
                group: SCREEN,
                name: w("Mettre en veille après", "Suspend after"),
                line: Some(w(
                    "L’ordinateur s’endort ; la session reste ouverte.",
                    "The computer sleeps; the session stays open.",
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
                id: "power.lid",
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
                id: "power.lid-ignored-when-docked",
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
                id: "power.power-button",
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
                id: "power.auto-saver",
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
                id: "power.auto-saver-below",
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
                needs: Some("power.auto-saver"),
            },
        ]
    })
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
    fn the_screens_delay_on_ac_is_the_idle_delay() {
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
            "[power.battery] screen-off"
        );
        assert_eq!(screen.say(&Value::Seconds(None), Lang::French), "Jamais");
    }
}
