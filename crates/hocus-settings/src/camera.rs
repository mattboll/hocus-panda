//! Caméra / Camera (#619): what the desktop does while an application uses
//! the camera. The screen light's rows (`[camera]`): on or off, its
//! warmth, its strength (`auto` follows the light on the face while the
//! Hocuspanda camera is read); then the Hocuspanda camera's effects, as
//! the camera's window in the bar sets them: Lumière, the banner, its
//! name and its project.

use hocus_words::settings::camera as words;
use std::sync::OnceLock;

use crate::model::{Choice, Codec, Kind, RowGroup, SectionId, Setting, Value, one};

/// The screen light's rows.
pub const LIGHT: RowGroup = RowGroup {
    label: words::SCREEN_LIGHT,
    per_source: false,
};

/// The Hocuspanda camera's rows.
pub const HOCUSPANDA: RowGroup = RowGroup {
    label: words::HOCUSPANDA_CAMERA,
    per_source: false,
};

/// The screen light's switch, which the other two need.
pub const SCREEN_LIGHT: &str = "camera.screen-light";
pub const WARMTH: &str = "camera.screen-light-warmth";
pub const STRENGTH: &str = "camera.screen-light-strength";
pub const LIGHTING: &str = "camera.lighting";
/// The banner's switch, which its name and project need.
pub const BANNER: &str = "camera.banner";
pub const BANNER_NAME: &str = "camera.banner-name";
pub const BANNER_PROJECT: &str = "camera.banner-project";

const WARMTHS: &[&str] = &["cool", "neutral", "warm"];
const STRENGTHS: &[&str] = &["auto", "soft", "medium", "strong"];

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: text,
        value: Value::Word(word),
    }
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![
            Setting {
                id: SCREEN_LIGHT,
                section: SectionId::Camera,
                group: LIGHT,
                name: words::SCREEN_LIGHT_2,
                line: Some(words::DURING_A_CALL_THE_EDGES_OF),
                keywords: (
                    &["caméra", "webcam", "appel", "visio", "visage", "lumière"],
                    &["camera", "webcam", "call", "video", "face", "light"],
                ),
                kind: Kind::Switch,
                places: one(&["camera", "screen-light"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: WARMTH,
                section: SectionId::Camera,
                group: LIGHT,
                name: words::WARMTH,
                line: Some(words::THE_LIGHT_S_COLOUR),
                keywords: (
                    &["caméra", "lumière", "couleur", "chaude", "froide"],
                    &["camera", "light", "colour", "color", "warm", "cool"],
                ),
                kind: Kind::Segmented(vec![
                    word(words::COOL, "cool"),
                    word(words::NEUTRAL, "neutral"),
                    word(words::WARM, "warm"),
                ]),
                places: one(&["camera", "screen-light-warmth"], Codec::Word(WARMTHS)),
                needs: Some(SCREEN_LIGHT),
            },
            Setting {
                id: STRENGTH,
                section: SectionId::Camera,
                group: LIGHT,
                name: words::STRENGTH,
                line: Some(words::HOW_FAR_IN_IT_REACHES_AND),
                keywords: (
                    &["caméra", "lumière", "intensité", "luminosité", "auto"],
                    &["camera", "light", "strength", "brightness", "auto"],
                ),
                kind: Kind::Segmented(vec![
                    word(words::AUTO, "auto"),
                    word(words::SOFT, "soft"),
                    word(words::MEDIUM, "medium"),
                    word(words::STRONG, "strong"),
                ]),
                places: one(&["camera", "screen-light-strength"], Codec::Word(STRENGTHS)),
                needs: Some(SCREEN_LIGHT),
            },
            Setting {
                id: LIGHTING,
                section: SectionId::Camera,
                group: HOCUSPANDA,
                name: words::LIGHTING,
                line: Some(words::EXPOSURE_WHITES_AND_WARMTH),
                keywords: (
                    &[
                        "caméra",
                        "hocuspanda",
                        "effet",
                        "lumière",
                        "exposition",
                        "blancs",
                    ],
                    &[
                        "camera",
                        "hocuspanda",
                        "effect",
                        "light",
                        "exposure",
                        "white balance",
                    ],
                ),
                kind: Kind::Switch,
                places: one(&["camera", "lighting"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: BANNER,
                section: SectionId::Camera,
                group: HOCUSPANDA,
                name: words::BANNER,
                line: Some(words::YOUR_NAME_ACROSS_THE_BOTTOM),
                keywords: (
                    &["caméra", "hocuspanda", "effet", "bandeau", "nom"],
                    &[
                        "camera",
                        "hocuspanda",
                        "effect",
                        "banner",
                        "name",
                        "lower third",
                    ],
                ),
                kind: Kind::Switch,
                places: one(&["camera", "banner"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: BANNER_NAME,
                section: SectionId::Camera,
                group: HOCUSPANDA,
                name: words::BANNER_NAME,
                line: Some(words::EMPTY_YOUR_ACCOUNT_S_NAME),
                keywords: (&["caméra", "bandeau", "nom"], &["camera", "banner", "name"]),
                kind: Kind::Text,
                places: one(&["camera", "banner-name"], Codec::Text),
                needs: Some(BANNER),
            },
            Setting {
                id: BANNER_PROJECT,
                section: SectionId::Camera,
                group: HOCUSPANDA,
                name: words::BANNER_PROJECT,
                line: Some(words::UNDER_YOUR_NAME_THE_PROJECT),
                keywords: (
                    &["caméra", "bandeau", "projet"],
                    &["camera", "banner", "project"],
                ),
                kind: Kind::Switch,
                places: one(&["camera", "banner-project"], Codec::Bool),
                needs: Some(BANNER),
            },
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Places;

    /// Its keys and their defaults are the configuration's: the light on,
    /// neutral, auto; Lumière on, no banner, the account's name, the
    /// project.
    #[test]
    fn the_rows_write_the_camera_s_keys_with_their_defaults() {
        let keys: Vec<(String, Value)> = settings()
            .iter()
            .map(|s| match &s.places {
                Places::One(place) => (place.dotted(), place.default.clone()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            keys,
            [
                ("camera.screen-light".to_owned(), Value::Bool(true)),
                (
                    "camera.screen-light-warmth".to_owned(),
                    Value::Word("neutral")
                ),
                (
                    "camera.screen-light-strength".to_owned(),
                    Value::Word("auto")
                ),
                ("camera.lighting".to_owned(), Value::Bool(true)),
                ("camera.banner".to_owned(), Value::Bool(false)),
                ("camera.banner-name".to_owned(), Value::Text(String::new())),
                ("camera.banner-project".to_owned(), Value::Bool(true)),
            ]
        );
    }
}
