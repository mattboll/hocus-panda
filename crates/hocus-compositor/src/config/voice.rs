//! The voice, `[voice]` (#619, lot 5, docs/design/voice.md): what the
//! Hocuspanda microphone makes of the real one. Followed live.

use serde::Deserialize;

/// The Hocuspanda microphone's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Voice {
    /// One at a time: `none` (Telle quelle), `quiet` (Sans bruit, the
    /// default), `studio`, `panda`, `robot`, `deep` (Grave).
    pub effect: hocus_link::Voice,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_read_and_others_are_refused() -> anyhow::Result<()> {
        let read = |text: &str| toml::from_str::<Voice>(text);
        assert_eq!(read("")?.effect, hocus_link::Voice::Quiet);
        assert_eq!(read("effect = \"none\"")?.effect, hocus_link::Voice::AsItIs);
        assert_eq!(read("effect = \"deep\"")?.effect, hocus_link::Voice::Deep);
        assert!(read("effect = \"chipmunk\"").is_err());
        Ok(())
    }
}
