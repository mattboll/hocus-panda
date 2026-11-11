//! Accessibilité / Accessibility (#422, #423, #424): one page for whoever
//! needs to see larger, see clearer or work without a mouse. The
//! magnifier, the view (high contrast, reduced transparency, reduced
//! motion), and the keyboard's way into the bar. The keys stay where they
//! were in the file (`[appearance]`, `[magnifier]`, `[shortcuts]`); the
//! Appearance page keeps a link here.
//!
//! High contrast implies reduced transparency: while it is on, the
//! transparency's switch shows on and does not take clicks ("Comprise dans
//! le contraste élevé"); the file keeps its own value.

use hocus_words::settings::accessibility as words;
use std::sync::OnceLock;

use crate::{
    lang::{pair, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value, one},
};

/// The magnifier's rows (#424).
pub const MAGNIFIER: RowGroup = RowGroup {
    label: w(words::MAGNIFIER),
    per_source: false,
};
/// What the desktop looks like (#423).
pub const VIEW: RowGroup = RowGroup {
    label: w(words::VIEW),
    per_source: false,
};
/// The screen reader (#421, #734).
pub const READER: RowGroup = RowGroup {
    label: w(words::SCREEN_READER),
    per_source: false,
};
/// The bar by the keyboard (#422).
pub const KEYBOARD: RowGroup = RowGroup {
    label: w(words::KEYBOARD),
    per_source: false,
};

/// High contrast, which implies reduced transparency.
pub const HIGH_CONTRAST: &str = "appearance.high-contrast";
pub const REDUCE_TRANSPARENCY: &str = "appearance.reduce-transparency";
/// Orca with the session (`[accessibility] screen-reader`, #734).
pub const SCREEN_READER: &str = "accessibility.screen-reader";

/// The magnifier's zooms, in hundredths (`[magnifier] zoom`).
const ZOOMS: [i64; 8] = [150, 200, 300, 400, 600, 800, 1200, 1600];
const FOLLOWS: &[&str] = &["edge", "centre", "proportional"];

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: pair(fr, en),
        value: Value::Word(word),
    }
}

/// The page's settings, in its order.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![
            Setting {
                id: "magnifier.enabled",
                section: SectionId::Accessibility,
                group: MAGNIFIER,
                name: w(words::MAGNIFIER_2),
                line: Some(w(words::THE_SCREEN_UNDER_THE_POINTER_MAGNIFIED)),
                keywords: (
                    &[
                        "loupe",
                        "zoom",
                        "grossir",
                        "agrandir",
                        "malvoyant",
                        "accessibilité",
                    ],
                    &[
                        "magnifier",
                        "zoom",
                        "magnify",
                        "low vision",
                        "accessibility",
                    ],
                ),
                kind: Kind::Switch,
                places: one(&["magnifier", "enabled"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "magnifier.zoom",
                section: SectionId::Accessibility,
                group: MAGNIFIER,
                name: w(words::ZOOM),
                line: Some(w(words::THE_ONE_IT_TURNS_ON_AT)),
                keywords: (&["loupe", "grossissement"], &["magnifier", "magnification"]),
                kind: Kind::Menu(ZOOMS.iter().map(|&z| zoom(z)).collect()),
                places: one(
                    &["magnifier", "zoom"],
                    Codec::Hundredths {
                        min: 100,
                        max: 1600,
                    },
                ),
                needs: None,
            },
            Setting {
                id: "magnifier.follow-pointer",
                section: SectionId::Accessibility,
                group: MAGNIFIER,
                name: w(words::THE_VIEW_FOLLOWS_THE_POINTER),
                line: Some(w(words::AT_THE_EDGE_IT_MOVES_ONLY)),
                keywords: (&["loupe", "souris"], &["magnifier", "mouse", "tracking"]),
                kind: Kind::Menu(vec![
                    word("Au bord", "At the edge", "edge"),
                    word("Au centre", "Centred", "centre"),
                    word("Proportionnellement", "Proportionally", "proportional"),
                ]),
                places: one(&["magnifier", "follow-pointer"], Codec::Word(FOLLOWS)),
                needs: None,
            },
            Setting {
                id: "magnifier.follow-keyboard",
                section: SectionId::Accessibility,
                group: MAGNIFIER,
                name: w(words::FOLLOW_THE_KEYBOARD),
                line: Some(w(words::THE_VIEW_JUMPS_TO_WHAT_TAKES)),
                keywords: (
                    &["loupe", "focus", "curseur"],
                    &["magnifier", "focus", "caret"],
                ),
                kind: Kind::Switch,
                places: one(&["magnifier", "follow-keyboard"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "magnifier.smooth",
                section: SectionId::Accessibility,
                group: MAGNIFIER,
                name: w(words::SMOOTHING),
                line: Some(w(words::WITHOUT_HARD_PIXELS)),
                keywords: (
                    &["loupe", "flou", "pixels"],
                    &["magnifier", "smooth", "pixels"],
                ),
                kind: Kind::Switch,
                places: one(&["magnifier", "smooth"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "appearance.high-contrast",
                section: SectionId::Accessibility,
                group: VIEW,
                name: w(words::HIGH_CONTRAST),
                line: Some(w(words::THE_CHOSEN_THEME_REINFORCED_TEXT_AT)),
                keywords: (
                    &["contraste", "malvoyant", "accessibilité"],
                    &["contrast", "low vision", "accessibility"],
                ),
                kind: Kind::Switch,
                places: one(&["appearance", "high-contrast"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "appearance.reduce-transparency",
                section: SectionId::Accessibility,
                group: VIEW,
                name: w(words::REDUCED_TRANSPARENCY),
                line: Some(w(words::THE_BAR_ITS_MENUS_AND_THE)),
                keywords: (
                    &["verre", "flou", "opacité", "transparence"],
                    &["glass", "blur", "opacity", "transparency"],
                ),
                kind: Kind::Switch,
                places: one(&["appearance", "reduce-transparency"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "appearance.reduced-motion",
                section: SectionId::Accessibility,
                group: VIEW,
                name: w(words::REDUCED_MOTION),
                line: Some(w(words::WINDOWS_AND_DESKTOPS_FADE_INSTEAD_OF)),
                keywords: (
                    &["animation", "onduler", "mouvement"],
                    &["animation", "motion", "wobbly", "reduce motion"],
                ),
                kind: Kind::Switch,
                places: one(&["appearance", "reduced-motion"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "accessibility.text-and-cursor",
                section: SectionId::Accessibility,
                group: VIEW,
                name: w(words::TEXT_AND_CURSOR_SIZE),
                line: Some(w(words::IN_APPEARANCE_AND_LANGUAGE)),
                keywords: (
                    &["texte", "police", "curseur", "pointeur", "grand"],
                    &["text", "font", "cursor", "pointer", "large"],
                ),
                kind: Kind::Link(SectionId::Appearance),
                places: Places::Page(Place {
                    key: &["appearance", "text-scale"],
                    codec: Codec::Text,
                    default: Value::Text(String::new()),
                }),
                needs: None,
            },
            Setting {
                id: SCREEN_READER,
                section: SectionId::Accessibility,
                group: READER,
                name: w(words::SCREEN_READER_2),
                line: Some(w(words::ORCA_STARTS_WITH_THE_SESSION_SUPER)),
                keywords: (
                    &[
                        "lecteur d’écran",
                        "orca",
                        "aveugle",
                        "voix",
                        "accessibilité",
                    ],
                    &["screen reader", "orca", "blind", "speech", "accessibility"],
                ),
                kind: Kind::Switch,
                places: one(&["accessibility", "screen-reader"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: "accessibility.focus-bar",
                section: SectionId::Accessibility,
                group: KEYBOARD,
                name: w(words::GO_TO_THE_BAR),
                line: Some(w(words::SUPER_B_THEN_TAB_AND_THE)),
                keywords: (
                    &["clavier", "barre", "super+b", "focus", "sans souris"],
                    &["keyboard", "bar", "super+b", "focus", "without a mouse"],
                ),
                kind: Kind::Link(SectionId::Keyboard),
                places: Places::Page(Place {
                    key: &["shortcuts", "focus-bar"],
                    codec: Codec::Text,
                    default: Value::Text(String::new()),
                }),
                needs: None,
            },
        ]
    })
}

/// A zoom of the magnifier's menu, in hundredths: "×1,5", "×8".
fn zoom(hundredths: i64) -> Choice {
    let label = |sep: &str| {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        if hundredths % 100 == 0 {
            leak(format!("×{}", hundredths / 100))
        } else {
            leak(format!(
                "×{}{sep}{}",
                hundredths / 100,
                hundredths % 100 / 10
            ))
        }
    };
    Choice {
        label: pair(label(","), label(".")),
        value: Value::Int(hundredths),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_has_its_groups_in_order() {
        let mut groups: Vec<&str> = settings().iter().map(|s| s.group.label.fr).collect();
        groups.dedup();
        assert_eq!(groups, ["LOUPE", "VUE", "LECTEUR D’ÉCRAN", "CLAVIER"]);
        let ids: Vec<&str> = settings()
            .iter()
            .filter(|s| s.group == VIEW)
            .map(|s| s.id)
            .collect();
        assert_eq!(
            ids,
            [
                HIGH_CONTRAST,
                REDUCE_TRANSPARENCY,
                "appearance.reduced-motion",
                "accessibility.text-and-cursor"
            ]
        );
        // The keys stay where they were.
        let key = |id: &str| {
            settings()
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.places.all().first().map(|p| p.key))
        };
        assert_eq!(
            key(HIGH_CONTRAST),
            Some(&["appearance", "high-contrast"][..])
        );
        assert_eq!(key("magnifier.zoom"), Some(&["magnifier", "zoom"][..]));
    }

    #[test]
    fn the_screen_reader_is_a_switch_on_its_key_off_by_default() {
        let reader = settings()
            .iter()
            .find(|s| s.id == SCREEN_READER)
            .expect("the screen reader's row");
        assert_eq!(reader.group, READER);
        assert!(matches!(reader.kind, Kind::Switch));
        assert!(
            reader
                .line
                .is_some_and(|l| l.fr.contains("Super+Alt+S") && l.en.contains("Super+Alt+S"))
        );
        let places = reader.places.all();
        assert_eq!(places.len(), 1);
        let place = places[0];
        assert_eq!(place.key, &["accessibility", "screen-reader"][..]);
        assert_eq!(place.default, Value::Bool(false));
        // What it writes, the compositor reads (`[accessibility]
        // screen-reader = true`).
        let written = place.codec.write(&Value::Bool(true)).expect("written");
        assert_eq!(place.codec.read(&written), Some(Value::Bool(true)));
    }

    #[test]
    fn the_magnifiers_zoom_reads_and_writes_as_the_compositor_takes_it() {
        let zoom = settings()
            .iter()
            .find(|s| s.id == "magnifier.zoom")
            .expect("the zoom's row");
        let Kind::Menu(choices) = &zoom.kind else {
            panic!("a menu");
        };
        assert_eq!(choices[0].label.fr, "×1,5");
        assert_eq!(choices[0].label.en, "×1.5");
        assert_eq!(choices[7].label.en, "×16");
        for place in zoom.places.all() {
            assert!(place.codec.write(&Value::Int(150)).is_some());
            assert!(place.codec.write(&place.default).is_some());
        }
    }
}
