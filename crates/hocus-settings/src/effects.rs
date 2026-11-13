//! Apparence › Effets / Appearance › Effects (#434, #793): the desktop's
//! effects, one switch each, all off by default, grouped as the summary
//! board "Les quatorze effets" groups them (the wallpaper, the windows,
//! the moments, the agents); the second garland (#813) beside the first. Each writes its key of `[effects]`; the
//! effects not built yet show "Bientôt" and their switch is inert.

use hocus_words::settings::effects as words;
use std::sync::OnceLock;

use crate::model::{Codec, Kind, RowGroup, SectionId, Setting, one};

const WALLPAPER: RowGroup = RowGroup {
    label: words::WALLPAPER,
    per_source: false,
};
const WINDOWS: RowGroup = RowGroup {
    label: words::WINDOWS,
    per_source: false,
};
const MOMENTS: RowGroup = RowGroup {
    label: words::MOMENTS,
    per_source: false,
};
const AGENTS: RowGroup = RowGroup {
    label: words::AGENTS,
    per_source: false,
};

/// The effects the desktop plays so far: the others' rows say "Bientôt".
pub const BUILT: &[&str] = &[
    "effects.snow",
    "effects.rain-on-glass",
    "effects.koi",
    "effects.fireflies",
    "effects.daylight",
    "effects.depth",
    "effects.flames",
    "effects.shatter",
    "effects.jelly",
    "effects.garland",
    "effects.garland-top",
    "effects.creatures",
];

/// Whether `id` is an effect's row not built yet.
pub fn soon(id: &str) -> bool {
    id.starts_with("effects.") && !BUILT.contains(&id)
}

/// The effects' rows, at the end of the Appearance page, in their order.
// Over 100 lines: a table, one entry per effect.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let effect =
            |id: &'static str,
             key: &'static [&'static str],
             group: RowGroup,
             name: hocus_words::Text,
             line: hocus_words::Text,
             keywords: (&'static [&'static str], &'static [&'static str])| Setting {
                id,
                section: SectionId::Appearance,
                group,
                name,
                line: Some(line),
                keywords,
                kind: Kind::Switch,
                places: one(key, Codec::Bool),
                needs: None,
            };
        vec![
            effect(
                "effects.snow",
                &["effects", "snow"],
                WALLPAPER,
                words::SNOW,
                words::SNOW_LINE,
                (&["neige", "hiver", "effet"], &["snow", "winter", "effect"]),
            ),
            effect(
                "effects.aurora",
                &["effects", "aurora"],
                WALLPAPER,
                words::AURORA,
                words::AURORA_LINE,
                (
                    &["aurore", "musique", "effet"],
                    &["aurora", "music", "effect"],
                ),
            ),
            effect(
                "effects.rain-on-glass",
                &["effects", "rain-on-glass"],
                WALLPAPER,
                words::RAIN_ON_GLASS,
                words::RAIN_ON_GLASS_LINE,
                (
                    &["pluie", "gouttes", "vitre", "effet"],
                    &["rain", "drops", "glass", "effect"],
                ),
            ),
            effect(
                "effects.koi",
                &["effects", "koi"],
                WALLPAPER,
                words::KOI,
                words::KOI_LINE,
                (
                    &["koï", "carpes", "bassin", "effet"],
                    &["koi", "carp", "pond", "effect"],
                ),
            ),
            effect(
                "effects.fireflies",
                &["effects", "fireflies"],
                WALLPAPER,
                words::FIREFLIES,
                words::FIREFLIES_LINE,
                (
                    &["lucioles", "soir", "effet"],
                    &["fireflies", "evening", "effect"],
                ),
            ),
            effect(
                "effects.daylight",
                &["effects", "daylight"],
                WALLPAPER,
                words::DAYLIGHT,
                words::DAYLIGHT_LINE,
                (
                    &["soleil", "ciel", "ombres", "lumière", "effet"],
                    &["sun", "sky", "shadows", "light", "effect"],
                ),
            ),
            effect(
                "effects.depth",
                &["effects", "depth"],
                WALLPAPER,
                words::DEPTH,
                words::DEPTH_LINE,
                (
                    &["profondeur", "paysage", "effet"],
                    &["depth", "parallax", "landscape", "effect"],
                ),
            ),
            effect(
                "effects.flames",
                &["effects", "flames"],
                WINDOWS,
                words::FLAMES,
                words::FLAMES_LINE,
                (
                    &["flammes", "feu", "brûler", "fermer", "effet"],
                    &["flames", "fire", "burn", "close", "effect"],
                ),
            ),
            effect(
                "effects.shatter",
                &["effects", "shatter"],
                WINDOWS,
                words::SHATTER,
                words::SHATTER_LINE,
                (
                    &["éclats", "verre", "briser", "effet"],
                    &["shatter", "glass", "shards", "effect"],
                ),
            ),
            effect(
                "effects.jelly",
                &["effects", "jelly"],
                WINDOWS,
                words::JELLY,
                words::JELLY_LINE,
                (&["gelée", "effet"], &["jelly", "effect"]),
            ),
            effect(
                "effects.garland",
                &["effects", "garland"],
                WINDOWS,
                words::GARLAND,
                words::GARLAND_LINE,
                (
                    &["guirlande", "noël", "effet"],
                    &["garland", "christmas", "lights", "effect"],
                ),
            ),
            effect(
                "effects.garland-top",
                &["effects", "garland-top"],
                WINDOWS,
                words::GARLAND_TOP,
                words::GARLAND_TOP_LINE,
                (
                    &["guirlande", "noël", "fêtes", "haut", "effet"],
                    &[
                        "garland",
                        "christmas",
                        "holidays",
                        "lights",
                        "top",
                        "effect",
                    ],
                ),
            ),
            effect(
                "effects.fireworks",
                &["effects", "fireworks"],
                MOMENTS,
                words::FIREWORKS,
                words::FIREWORKS_LINE,
                (
                    &["feu d’artifice", "tests", "effet"],
                    &["fireworks", "tests", "effect"],
                ),
            ),
            effect(
                "effects.confetti",
                &["effects", "confetti"],
                MOMENTS,
                words::CONFETTI,
                words::CONFETTI_LINE,
                (
                    &["confettis", "bonne nouvelle", "effet"],
                    &["confetti", "good news", "celebrate", "effect"],
                ),
            ),
            effect(
                "effects.creatures",
                &["effects", "creatures"],
                AGENTS,
                words::CREATURES,
                words::CREATURES_LINE,
                (
                    &["créatures", "panda", "agents", "effet"],
                    &["creatures", "panda", "agents", "effect"],
                ),
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fifteen (the board's fourteen, and the second garland), each with
    /// its key of `[effects]`, off by default; the snow, the rain on the
    /// glass, the koi, the fireflies, the daylight, depth, the flames, the
    /// glass, the jelly, the garlands and the creatures built.
    #[test]
    fn fifteen_effects_off_by_default_twelve_built() {
        let all = settings();
        assert_eq!(all.len(), 15);
        for s in all {
            let key = s.places.of(crate::model::Source::Ac).key;
            assert_eq!(key[0], "effects", "{}", s.id);
            assert_eq!(
                hocus_config::defaults::value(key).and_then(toml_edit::Value::as_bool),
                Some(false),
                "{}",
                s.id
            );
            assert_eq!(s.id, format!("effects.{}", key[1]));
            let built = [
                "effects.snow",
                "effects.rain-on-glass",
                "effects.koi",
                "effects.fireflies",
                "effects.daylight",
                "effects.depth",
                "effects.flames",
                "effects.shatter",
                "effects.jelly",
                "effects.garland",
                "effects.garland-top",
                "effects.creatures",
            ];
            assert_eq!(soon(s.id), !built.contains(&s.id));
        }
    }
}
