//! Souris et pavé tactile / Mouse and touchpad (6.12, variant A): the
//! touchpad's rows (`[touchpad]`), the mouse's (`[pointer]`), the
//! touchpad's gestures, then a test area (the window's own,
//! [`crate::app`]'s `mouse` page).
//!
//! The compositor gives these to every device as it is plugged in and
//! again when the file changes; it says the devices plugged, what each can
//! take and what it was given (`hocus msg input-devices follow`, read by
//! [`crate::devices`]). The page names the devices by their kind and where
//! they are plugged, never by their vendor's name.

use std::sync::OnceLock;

use hocus_link::input::{Bus, DeviceKind, InputDevice, RightClick};

use crate::{
    lang::{Lang, Words, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

pub const TOUCHPAD: RowGroup = RowGroup {
    label: w("PAVÉ TACTILE", "TOUCHPAD"),
    per_source: false,
};
pub const MOUSE: RowGroup = RowGroup {
    label: w("SOURIS", "MOUSE"),
    per_source: false,
};
/// The gestures' rows (6.13).
pub const SWIPE_DESKTOPS: &str = "touchpad.swipe-desktops";
pub const SWIPE_LAUNCHER: &str = "touchpad.swipe-launcher";
pub const LEFT_TO_APPS: &str = "touchpad.left-to-applications";

pub const GESTURES: RowGroup = RowGroup {
    label: w("GESTES DU PAVÉ TACTILE", "TOUCHPAD GESTURES"),
    per_source: false,
};

const CLICKS: &[&str] = &["two-fingers", "corner"];
const SCROLLS: &[&str] = &["two-fingers", "edge"];
const PROFILES: &[&str] = &["adaptive", "flat"];

/// A speed's slider: tenths from -1 to 1.
const SPEED: Kind = Kind::Speed { min: -10, max: 10 };
const TENTHS: Codec = Codec::Tenths { min: -10, max: 10 };

fn place(key: &'static [&'static str], codec: Codec, default: Value) -> Places {
    Places::One(Place {
        key,
        codec,
        default,
    })
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: w(fr, en),
        value: Value::Word(word),
    }
}

#[allow(clippy::too_many_arguments)]
fn setting(
    id: &'static str,
    group: RowGroup,
    name: Words,
    line: Option<Words>,
    keywords: (&'static [&'static str], &'static [&'static str]),
    kind: Kind,
    places: Places,
    needs: Option<&'static str>,
) -> Setting {
    Setting {
        id,
        section: SectionId::Mouse,
        group,
        name,
        line,
        keywords,
        kind,
        places,
        needs,
    }
}

/// The page's settings, in its order.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let speed_words: (&[&str], &[&str]) = (
            &["vitesse", "sensibilité", "rapide", "lent"],
            &["speed", "sensitivity", "fast", "slow"],
        );
        vec![
            setting(
                "touchpad.speed",
                TOUCHPAD,
                w("Vitesse du pointeur", "Pointer speed"),
                None,
                (
                    &["vitesse", "sensibilité", "pavé", "touchpad"],
                    &["speed", "sensitivity", "touchpad", "trackpad"],
                ),
                SPEED,
                place(&["touchpad", "speed"], TENTHS, Value::Int(0)),
                None,
            ),
            setting(
                "touchpad.tap-to-click",
                TOUCHPAD,
                w("Toucher pour cliquer", "Tap to click"),
                Some(w(
                    "Un doigt clique, deux font un clic droit, trois un clic du milieu.",
                    "One finger clicks, two right-click, three middle-click.",
                )),
                (
                    &["tap", "taper", "toucher", "pavé", "clic"],
                    &["tap", "touchpad", "click"],
                ),
                Kind::Switch,
                place(&["touchpad", "tap-to-click"], Codec::Bool, Value::Bool(true)),
                None,
            ),
            setting(
                "touchpad.tap-and-drag",
                TOUCHPAD,
                w("Toucher puis glisser", "Tap and drag"),
                Some(w(
                    "Deux touchers, le second gardé : déplacer sans appuyer.",
                    "Tap twice and hold the second: drag without pressing.",
                )),
                (
                    &["glisser", "déplacer", "tap", "pavé"],
                    &["drag", "tap", "touchpad"],
                ),
                Kind::Switch,
                place(&["touchpad", "tap-and-drag"], Codec::Bool, Value::Bool(true)),
                Some("touchpad.tap-to-click"),
            ),
            setting(
                "touchpad.right-click",
                TOUCHPAD,
                w("Clic droit en appuyant", "Right-click by pressing"),
                Some(w(
                    "À deux doigts n’importe où, ou d’un doigt dans le coin inférieur droit.",
                    "With two fingers anywhere, or one finger in the bottom-right corner.",
                )),
                (
                    &["clic droit", "deux doigts", "coin", "clickfinger", "pavé"],
                    &["right click", "two fingers", "corner", "clickfinger", "touchpad"],
                ),
                Kind::Menu(vec![
                    word("À deux doigts", "With two fingers", "two-fingers"),
                    word("Coin inférieur droit", "Bottom-right corner", "corner"),
                ]),
                // Without the key, the touchpad's own (libinput's default
                // for it): the window shows the device's.
                place(
                    &["touchpad", "right-click"],
                    Codec::Word(CLICKS),
                    Value::Word("two-fingers"),
                ),
                None,
            ),
            setting(
                "touchpad.disable-while-typing",
                TOUCHPAD,
                w("Désactivé pendant la frappe", "Off while typing"),
                Some(w(
                    "La paume ne déplace pas le pointeur pendant que tu tapes.",
                    "Your palm does not move the pointer while you type.",
                )),
                (
                    &["paume", "frappe", "taper", "clavier", "pavé"],
                    &["palm", "typing", "keyboard", "touchpad"],
                ),
                Kind::Switch,
                place(
                    &["touchpad", "disable-while-typing"],
                    Codec::Bool,
                    Value::Bool(true),
                ),
                None,
            ),
            setting(
                "touchpad.natural-scroll",
                TOUCHPAD,
                w("Défilement naturel", "Natural scrolling"),
                Some(w("Le contenu suit tes doigts.", "Content follows your fingers.")),
                (
                    &["défilement", "inversé", "sens", "pavé"],
                    &["natural", "scroll", "reverse", "touchpad"],
                ),
                Kind::Switch,
                place(
                    &["touchpad", "natural-scroll"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                None,
            ),
            setting(
                "touchpad.scroll",
                TOUCHPAD,
                w("Défiler", "Scroll"),
                None,
                (
                    &["défilement", "deux doigts", "bord", "pavé"],
                    &["scroll", "two fingers", "edge", "touchpad"],
                ),
                Kind::Menu(vec![
                    word("À deux doigts", "With two fingers", "two-fingers"),
                    word("Sur le bord droit", "On the right edge", "edge"),
                ]),
                place(
                    &["touchpad", "scroll"],
                    Codec::Word(SCROLLS),
                    Value::Word("two-fingers"),
                ),
                None,
            ),
            setting(
                "touchpad.off-with-mouse",
                TOUCHPAD,
                w(
                    "Coupé quand une souris est branchée",
                    "Off while a mouse is plugged in",
                ),
                Some(w(
                    "Il revient dès qu’elle est débranchée.",
                    "It comes back as soon as it is unplugged.",
                )),
                (
                    &["souris", "couper", "désactiver", "pavé"],
                    &["mouse", "disable", "touchpad"],
                ),
                Kind::Switch,
                place(
                    &["touchpad", "off-with-mouse"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                None,
            ),
            setting(
                "pointer.speed",
                MOUSE,
                w("Vitesse du pointeur", "Pointer speed"),
                None,
                speed_words,
                SPEED,
                place(&["pointer", "speed"], TENTHS, Value::Int(0)),
                None,
            ),
            setting(
                "pointer.acceleration",
                MOUSE,
                w("Accélération", "Acceleration"),
                Some(w(
                    "Adaptative : un geste vif va plus loin. Aucune : la même distance à toute vitesse.",
                    "Adaptive: a quick move goes further. None: the same distance at any speed.",
                )),
                (
                    &["accélération", "précision", "souris"],
                    &["acceleration", "flat", "precision", "mouse"],
                ),
                Kind::Menu(vec![
                    word("Adaptative", "Adaptive", "adaptive"),
                    word("Aucune", "None", "flat"),
                ]),
                place(
                    &["pointer", "acceleration"],
                    Codec::Word(PROFILES),
                    Value::Word("adaptive"),
                ),
                None,
            ),
            setting(
                "pointer.natural-scroll",
                MOUSE,
                w("Défilement naturel", "Natural scrolling"),
                Some(w(
                    "Le contenu suit la molette, comme sous un doigt.",
                    "Content follows the wheel, as under a finger.",
                )),
                (
                    &["défilement", "inversé", "molette", "souris"],
                    &["natural", "scroll", "wheel", "reverse", "mouse"],
                ),
                Kind::Switch,
                place(
                    &["pointer", "natural-scroll"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                None,
            ),
            setting(
                "pointer.left-handed",
                MOUSE,
                w("Gaucher", "Left-handed"),
                Some(w(
                    "Le bouton droit devient le principal.",
                    "The right button becomes the main one.",
                )),
                (
                    &["gaucher", "boutons", "inverser", "souris"],
                    &["left-handed", "buttons", "swap", "mouse"],
                ),
                Kind::Switch,
                place(&["pointer", "left-handed"], Codec::Bool, Value::Bool(false)),
                None,
            ),
            setting(
                SWIPE_DESKTOPS,
                GESTURES,
                w("Trois doigts de côté", "Three fingers sideways"),
                Some(w(
                    "Changer de bureau sur l’écran sous le pointeur ; le bureau suit tes doigts, puis se pose ou revient.",
                    "Switch desktop on the screen under the pointer; the desktop follows your fingers, then settles or springs back.",
                )),
                (
                    &[
                        "geste", "glisser", "doigts", "trois doigts", "bureau", "pavé",
                    ],
                    &[
                        "gesture", "swipe", "fingers", "three fingers", "desktop", "touchpad",
                    ],
                ),
                Kind::Switch,
                place(
                    &["touchpad", "swipe-desktops"],
                    Codec::Bool,
                    Value::Bool(true),
                ),
                None,
            ),
            setting(
                SWIPE_LAUNCHER,
                GESTURES,
                w("Quatre doigts vers le haut ou le bas", "Four fingers up or down"),
                Some(w(
                    "Vers le haut, le lanceur, qui grandit avec tes doigts ; vers le bas, la vue d’ensemble de tes bureaux, ou le lanceur se referme.",
                    "Up opens the launcher, which grows with your fingers; down shows the overview of your desktops, or closes the launcher.",
                )),
                (
                    &[
                        "geste",
                        "glisser",
                        "doigts",
                        "quatre doigts",
                        "lanceur",
                        "pavé",
                        "vue d’ensemble",
                        "bureaux",
                    ],
                    &[
                        "gesture",
                        "swipe",
                        "fingers",
                        "four fingers",
                        "launcher",
                        "touchpad",
                        "overview",
                        "desktops",
                    ],
                ),
                Kind::Switch,
                place(
                    &["touchpad", "swipe-launcher"],
                    Codec::Bool,
                    Value::Bool(true),
                ),
                None,
            ),
            // Not a setting: what the desktop never takes.
            setting(
                LEFT_TO_APPS,
                GESTURES,
                w("Ce qui reste aux applications", "What is left to applications"),
                Some(w(
                    "Deux doigts pour défiler et pincer ; trois doigts vers le haut ou le bas ; quatre doigts de côté.",
                    "Two fingers to scroll and pinch; three fingers up or down; four fingers sideways.",
                )),
                (
                    &["geste", "doigts", "pincer", "applications", "pavé"],
                    &["gesture", "fingers", "pinch", "applications", "touchpad"],
                ),
                Kind::Note(w("Toujours", "Always")),
                Places::Page(Place {
                    key: &["touchpad", "swipe-desktops"],
                    codec: Codec::Bool,
                    default: Value::Bool(true),
                }),
                None,
            ),
        ]
    })
}

/// Which devices a setting of this page is for.
pub fn kind_of(setting: &str) -> Option<DeviceKind> {
    if setting.starts_with("touchpad.") {
        Some(DeviceKind::Touchpad)
    } else if setting.starts_with("pointer.") {
        Some(DeviceKind::Pointer)
    } else {
        None
    }
}

/// The device the toast adds to a row's name: ", pavé tactile".
pub fn aside(setting: &str) -> Option<Words> {
    match kind_of(setting)? {
        DeviceKind::Touchpad if setting.starts_with("touchpad.swipe") => None,
        DeviceKind::Touchpad => Some(w(", pavé tactile", ", touchpad")),
        DeviceKind::Pointer => Some(w(", souris", ", mouse")),
    }
}

/// Whether some device of the setting's kind can take it; `true` while the
/// devices are not known.
pub fn takes(setting: &str, devices: Option<&[InputDevice]>) -> bool {
    let (Some(kind), Some(devices)) = (kind_of(setting), devices) else {
        return true;
    };
    devices.iter().filter(|d| d.kind == kind).any(|d| {
        let can = d.can;
        match setting {
            "touchpad.speed" | "pointer.speed" => can.accel,
            "touchpad.tap-to-click" | "touchpad.tap-and-drag" => can.tap,
            // A menu: both choices, or nothing to choose.
            "touchpad.right-click" => can.clickfinger && can.button_areas,
            "touchpad.scroll" => can.two_fingers && can.edge,
            "touchpad.disable-while-typing" => can.dwt,
            "touchpad.natural-scroll" | "pointer.natural-scroll" => can.natural,
            "touchpad.off-with-mouse" => can.off_with_mouse,
            "pointer.acceleration" => can.flat && can.adaptive,
            "pointer.left-handed" => can.left_handed,
            // 0: a compositor that does not say it.
            SWIPE_DESKTOPS => can.fingers == 0 || can.fingers >= 3,
            SWIPE_LAUNCHER => can.fingers == 0 || can.fingers >= 4,
            _ => true,
        }
    })
}

/// What applies to a setting when the file says nothing, when a device
/// says it: the touchpad's own right click.
pub fn device_default(setting: &str, devices: Option<&[InputDevice]>) -> Option<Value> {
    if setting != "touchpad.right-click" {
        return None;
    }
    let pad = devices?.iter().find(|d| d.kind == DeviceKind::Touchpad)?;
    Some(Value::Word(match pad.click_default? {
        RightClick::TwoFingers => "two-fingers",
        RightClick::Corner => "corner",
    }))
}

/// The touchpad's or the mice's name on its group's label line: by kind
/// and where it is plugged, never the vendor's name.
pub fn named(kind: DeviceKind, devices: &[InputDevice], lang: Lang) -> Option<String> {
    let of: Vec<&InputDevice> = devices.iter().filter(|d| d.kind == kind).collect();
    let fr = lang.fr();
    match (kind, of.as_slice()) {
        (_, []) => None,
        (DeviceKind::Touchpad, [one, ..]) => Some(
            if one.built_in {
                w("Intégré au portable", "Built in")
            } else if one.bus == Bus::Bluetooth {
                w("Pavé tactile Bluetooth", "Bluetooth touchpad")
            } else {
                w("Pavé tactile USB", "USB touchpad")
            }
            .get(lang)
            .to_owned(),
        ),
        (DeviceKind::Pointer, [one]) => Some(
            match (one.bus, one.built_in) {
                (Bus::Bluetooth, _) => w("Souris Bluetooth", "Bluetooth mouse"),
                (Bus::Usb, _) => w("Souris USB", "USB mouse"),
                (Bus::Internal, _) => w("Intégrée au portable", "Built in"),
            }
            .get(lang)
            .to_owned(),
        ),
        (DeviceKind::Pointer, many) => Some(if fr {
            format!("{} souris", many.len())
        } else {
            format!("{} mice", many.len())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::input::{Applied, Can};

    #[test]
    fn every_default_is_a_choice_and_writes() {
        for setting in settings() {
            for place in setting.places.all() {
                assert!(
                    place.codec.write(&place.default).is_some(),
                    "{}",
                    setting.id
                );
                assert_eq!(
                    setting.id.split_once('.').map(|(t, _)| t),
                    place.key.first().copied(),
                    "{} is in its table",
                    setting.id
                );
                if let Kind::Menu(choices) = &setting.kind {
                    assert!(choices.iter().any(|c| c.value == place.default));
                }
            }
        }
    }

    fn device(id: u64, kind: DeviceKind, bus: Bus, can: Can) -> InputDevice {
        InputDevice {
            id,
            kind,
            bus,
            built_in: bus == Bus::Internal,
            can,
            click_default: Some(RightClick::Corner),
            applied: Applied::default(),
        }
    }

    #[test]
    fn devices_are_named_by_kind_and_bus() {
        let pad = device(1, DeviceKind::Touchpad, Bus::Internal, Can::default());
        let usb = device(2, DeviceKind::Pointer, Bus::Usb, Can::default());
        let bt = device(3, DeviceKind::Pointer, Bus::Bluetooth, Can::default());
        let all = [pad.clone(), usb.clone()];
        assert_eq!(
            named(DeviceKind::Touchpad, &all, Lang::French).as_deref(),
            Some("Intégré au portable")
        );
        assert_eq!(
            named(DeviceKind::Pointer, &all, Lang::French).as_deref(),
            Some("Souris USB")
        );
        assert_eq!(
            named(DeviceKind::Pointer, &[usb, bt], Lang::English).as_deref(),
            Some("2 mice")
        );
        assert_eq!(
            named(
                DeviceKind::Pointer,
                std::slice::from_ref(&pad),
                Lang::French
            ),
            None
        );
        assert_eq!(
            device_default("touchpad.right-click", Some(&[pad])),
            Some(Value::Word("corner"))
        );
    }

    #[test]
    fn a_row_no_device_can_take_is_said() {
        let pad = device(
            1,
            DeviceKind::Touchpad,
            Bus::Internal,
            Can {
                tap: true,
                two_fingers: true,
                clickfinger: true,
                ..Can::default()
            },
        );
        let devices = [pad];
        assert!(takes("touchpad.tap-to-click", Some(&devices)));
        assert!(!takes("touchpad.scroll", Some(&devices)), "no edge here");
        assert!(!takes("touchpad.right-click", Some(&devices)));
        assert!(takes("touchpad.scroll", None), "devices not known yet");
        assert_eq!(
            aside("touchpad.natural-scroll").map(|w| w.fr),
            Some(", pavé tactile")
        );
        assert_eq!(aside("pointer.speed").map(|w| w.en), Some(", mouse"));
    }
}
