//! Removable drives, `[removable]` (#898, docs/design/bar.md "Removable
//! drives"): what the desktop does when a USB stick, an external disk or
//! an SD card is plugged in. The compositor only passes it on to the
//! shell, which follows UDisks2; followed live.

use serde::Deserialize;

/// `[removable]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Removable {
    /// A bubble that offers to open it, open it at once, or nothing.
    pub on_plug: hocus_link::OnPlug,
}

#[cfg(test)]
mod tests {
    use hocus_link::OnPlug;

    #[test]
    fn removable_reads_on_plug_and_refuses_what_it_does_not_know() {
        let config: crate::Config = toml::from_str("").unwrap();
        assert_eq!(config.removable.on_plug, OnPlug::Ask);
        for (word, on_plug) in [
            ("ask", OnPlug::Ask),
            ("open", OnPlug::Open),
            ("nothing", OnPlug::Nothing),
        ] {
            let config: crate::Config =
                toml::from_str(&format!("[removable]\non-plug = \"{word}\"\n")).unwrap();
            assert_eq!(config.removable.on_plug, on_plug);
        }
        assert!(toml::from_str::<crate::Config>("[removable]\non-plug = \"mount\"\n").is_err());
        assert!(toml::from_str::<crate::Config>("[removable]\nauto = true\n").is_err());
    }
}
