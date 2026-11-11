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

use crate::lang::French as _;
use hocus_words::settings::mouse as words;
use std::sync::OnceLock;

use hocus_link::input::{Bus, DeviceKind, InputDevice, RightClick};

use crate::{
    lang::{Lang, Words, pair, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value, one},
};

pub const TOUCHPAD: RowGroup = RowGroup {
    label: w(words::TOUCHPAD),
    per_source: false,
};
pub const MOUSE: RowGroup = RowGroup {
    label: w(words::MOUSE),
    per_source: false,
};
/// The gestures' rows (6.13).
pub const SWIPE_DESKTOPS: &str = "touchpad.swipe-desktops";
pub const SWIPE_LAUNCHER: &str = "touchpad.swipe-launcher";
pub const LEFT_TO_APPS: &str = "touchpad.left-to-applications";

pub const GESTURES: RowGroup = RowGroup {
    label: w(words::TOUCHPAD_GESTURES),
    per_source: false,
};

const CLICKS: &[&str] = &["two-fingers", "corner"];
const SCROLLS: &[&str] = &["two-fingers", "edge"];
const PROFILES: &[&str] = &["adaptive", "flat"];

/// A speed's slider: tenths from -1 to 1.
const SPEED: Kind = Kind::Speed { min: -10, max: 10 };
const TENTHS: Codec = Codec::Tenths { min: -10, max: 10 };

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: pair(fr, en),
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
                w(words::POINTER_SPEED),
                None,
                (
                    &["vitesse", "sensibilité", "pavé", "touchpad"],
                    &["speed", "sensitivity", "touchpad", "trackpad"],
                ),
                SPEED,
                one(&["touchpad", "speed"], TENTHS),
                None,
            ),
            setting(
                "touchpad.tap-to-click",
                TOUCHPAD,
                w(words::TAP_TO_CLICK),
                Some(w(words::ONE_FINGER_CLICKS_TWO_RIGHT_CLICK)),
                (
                    &["tap", "taper", "toucher", "pavé", "clic"],
                    &["tap", "touchpad", "click"],
                ),
                Kind::Switch,
                one(&["touchpad", "tap-to-click"], Codec::Bool),
                None,
            ),
            setting(
                "touchpad.tap-and-drag",
                TOUCHPAD,
                w(words::TAP_AND_DRAG),
                Some(w(words::TAP_TWICE_AND_HOLD_THE_SECOND)),
                (
                    &["glisser", "déplacer", "tap", "pavé"],
                    &["drag", "tap", "touchpad"],
                ),
                Kind::Switch,
                one(&["touchpad", "tap-and-drag"], Codec::Bool),
                Some("touchpad.tap-to-click"),
            ),
            setting(
                "touchpad.right-click",
                TOUCHPAD,
                w(words::RIGHT_CLICK_BY_PRESSING),
                Some(w(words::WITH_TWO_FINGERS_ANYWHERE_OR_ONE)),
                (
                    &["clic droit", "deux doigts", "coin", "clickfinger", "pavé"],
                    &[
                        "right click",
                        "two fingers",
                        "corner",
                        "clickfinger",
                        "touchpad",
                    ],
                ),
                Kind::Menu(vec![
                    word("À deux doigts", "With two fingers", "two-fingers"),
                    word("Coin inférieur droit", "Bottom-right corner", "corner"),
                ]),
                // Without the key, the touchpad's own (libinput's default
                // for it): the window shows the device's.
                Places::One(Place::unset(
                    &["touchpad", "right-click"],
                    Codec::Word(CLICKS),
                    Value::Word("two-fingers"),
                )),
                None,
            ),
            setting(
                "touchpad.disable-while-typing",
                TOUCHPAD,
                w(words::OFF_WHILE_TYPING),
                Some(w(words::YOUR_PALM_DOES_NOT_MOVE_THE)),
                (
                    &["paume", "frappe", "taper", "clavier", "pavé"],
                    &["palm", "typing", "keyboard", "touchpad"],
                ),
                Kind::Switch,
                one(&["touchpad", "disable-while-typing"], Codec::Bool),
                None,
            ),
            setting(
                "touchpad.natural-scroll",
                TOUCHPAD,
                w(words::NATURAL_SCROLLING),
                Some(w(words::CONTENT_FOLLOWS_YOUR_FINGERS)),
                (
                    &["défilement", "inversé", "sens", "pavé"],
                    &["natural", "scroll", "reverse", "touchpad"],
                ),
                Kind::Switch,
                one(&["touchpad", "natural-scroll"], Codec::Bool),
                None,
            ),
            setting(
                "touchpad.scroll",
                TOUCHPAD,
                w(words::SCROLL),
                None,
                (
                    &["défilement", "deux doigts", "bord", "pavé"],
                    &["scroll", "two fingers", "edge", "touchpad"],
                ),
                Kind::Menu(vec![
                    word("À deux doigts", "With two fingers", "two-fingers"),
                    word("Sur le bord droit", "On the right edge", "edge"),
                ]),
                one(&["touchpad", "scroll"], Codec::Word(SCROLLS)),
                None,
            ),
            setting(
                "touchpad.off-with-mouse",
                TOUCHPAD,
                w(words::OFF_WHILE_A_MOUSE_IS_PLUGGED),
                Some(w(words::IT_COMES_BACK_AS_SOON_AS)),
                (
                    &["souris", "couper", "désactiver", "pavé"],
                    &["mouse", "disable", "touchpad"],
                ),
                Kind::Switch,
                one(&["touchpad", "off-with-mouse"], Codec::Bool),
                None,
            ),
            setting(
                "pointer.speed",
                MOUSE,
                w(words::POINTER_SPEED),
                None,
                speed_words,
                SPEED,
                one(&["pointer", "speed"], TENTHS),
                None,
            ),
            setting(
                "pointer.acceleration",
                MOUSE,
                w(words::ACCELERATION),
                Some(w(words::ADAPTIVE_A_QUICK_MOVE_GOES_FURTHER)),
                (
                    &["accélération", "précision", "souris"],
                    &["acceleration", "flat", "precision", "mouse"],
                ),
                Kind::Menu(vec![
                    word("Adaptative", "Adaptive", "adaptive"),
                    word("Aucune", "None", "flat"),
                ]),
                one(&["pointer", "acceleration"], Codec::Word(PROFILES)),
                None,
            ),
            setting(
                "pointer.natural-scroll",
                MOUSE,
                w(words::NATURAL_SCROLLING),
                Some(w(words::CONTENT_FOLLOWS_THE_WHEEL_AS_UNDER)),
                (
                    &["défilement", "inversé", "molette", "souris"],
                    &["natural", "scroll", "wheel", "reverse", "mouse"],
                ),
                Kind::Switch,
                one(&["pointer", "natural-scroll"], Codec::Bool),
                None,
            ),
            setting(
                "pointer.left-handed",
                MOUSE,
                w(words::LEFT_HANDED),
                Some(w(words::THE_RIGHT_BUTTON_BECOMES_THE_MAIN)),
                (
                    &["gaucher", "boutons", "inverser", "souris"],
                    &["left-handed", "buttons", "swap", "mouse"],
                ),
                Kind::Switch,
                one(&["pointer", "left-handed"], Codec::Bool),
                None,
            ),
            setting(
                SWIPE_DESKTOPS,
                GESTURES,
                w(words::THREE_FINGERS_SIDEWAYS),
                Some(w(words::SWITCH_DESKTOP_ON_THE_SCREEN_UNDER)),
                (
                    &[
                        "geste",
                        "glisser",
                        "doigts",
                        "trois doigts",
                        "bureau",
                        "pavé",
                    ],
                    &[
                        "gesture",
                        "swipe",
                        "fingers",
                        "three fingers",
                        "desktop",
                        "touchpad",
                    ],
                ),
                Kind::Switch,
                one(&["touchpad", "swipe-desktops"], Codec::Bool),
                None,
            ),
            setting(
                SWIPE_LAUNCHER,
                GESTURES,
                w(words::FOUR_FINGERS_UP_OR_DOWN),
                Some(w(words::UP_OPENS_THE_LAUNCHER_WHICH_GROWS)),
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
                one(&["touchpad", "swipe-launcher"], Codec::Bool),
                None,
            ),
            // Not a setting: what the desktop never takes.
            setting(
                LEFT_TO_APPS,
                GESTURES,
                w(words::WHAT_IS_LEFT_TO_APPLICATIONS),
                Some(w(words::TWO_FINGERS_TO_SCROLL_AND_PINCH)),
                (
                    &["geste", "doigts", "pincer", "applications", "pavé"],
                    &["gesture", "fingers", "pinch", "applications", "touchpad"],
                ),
                Kind::Note(w(words::ALWAYS)),
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
        DeviceKind::Touchpad => Some(w(words::TOUCHPAD_2)),
        DeviceKind::Pointer => Some(w(words::MOUSE_2)),
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
                w(words::BUILT_IN)
            } else if one.bus == Bus::Bluetooth {
                w(words::BLUETOOTH_TOUCHPAD)
            } else {
                w(words::USB_TOUCHPAD)
            }
            .get(lang)
            .to_owned(),
        ),
        (DeviceKind::Pointer, [one]) => Some(
            match (one.bus, one.built_in) {
                (Bus::Bluetooth, _) => w(words::BLUETOOTH_MOUSE),
                (Bus::Usb, _) => w(words::USB_MOUSE),
                (Bus::Internal, _) => w(words::BUILT_IN_2),
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
