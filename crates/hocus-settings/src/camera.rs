//! Caméra / Camera (#619): what the desktop does while an application uses
//! the camera. Its first rows, the screen light's (`[camera]`): on or off,
//! its warmth, its strength; the Hocuspanda camera's effects come with
//! their own lots.

use hocus_words::settings::camera as words;
use std::sync::OnceLock;

use crate::model::{Choice, Codec, Kind, RowGroup, SectionId, Setting, Value, one};

/// The screen light's rows.
pub const LIGHT: RowGroup = RowGroup {
    label: words::SCREEN_LIGHT,
    per_source: false,
};

/// The screen light's switch, which the other two need.
pub const SCREEN_LIGHT: &str = "camera.screen-light";
pub const WARMTH: &str = "camera.screen-light-warmth";
pub const STRENGTH: &str = "camera.screen-light-strength";

const WARMTHS: &[&str] = &["cool", "neutral", "warm"];
const STRENGTHS: &[&str] = &["soft", "medium", "strong"];

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
                    &["caméra", "lumière", "intensité", "luminosité"],
                    &["camera", "light", "strength", "brightness"],
                ),
                kind: Kind::Segmented(vec![
                    word(words::SOFT, "soft"),
                    word(words::MEDIUM, "medium"),
                    word(words::STRONG, "strong"),
                ]),
                places: one(&["camera", "screen-light-strength"], Codec::Word(STRENGTHS)),
                needs: Some(SCREEN_LIGHT),
            },
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Places;

    /// Its keys and their defaults are the configuration's: on, neutral,
    /// soft.
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
                    Value::Word("soft")
                ),
            ]
        );
    }
}
