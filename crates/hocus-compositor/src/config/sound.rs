//! The sound, `[sound]` (#897, docs/design/settings.md "Son"): whether the
//! outputs' volume may go above 100 %. The volumes themselves are
//! PipeWire's, which remembers them; the compositor only tells the shell,
//! which keeps the sound. Followed live.

use serde::Deserialize;

/// `[sound]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct SoundSettings {
    /// Up to 150 %, the bar's slider's last third amplified.
    pub boost: bool,
}

#[cfg(test)]
mod tests {
    #[test]
    fn boost_reads_off_by_default_and_unknown_keys_are_refused() -> anyhow::Result<()> {
        let config: crate::Config = toml::from_str("")?;
        assert!(!config.sound.boost);
        let config: crate::Config = toml::from_str("[sound]\nboost = true\n")?;
        assert!(config.sound.boost);
        assert!(toml::from_str::<crate::Config>("[sound]\nloud = true\n").is_err());
        Ok(())
    }
}
