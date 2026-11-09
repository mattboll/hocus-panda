//! Her part of `config.toml` (`[secretary]`, §16): which provider and
//! model each role uses, the providers (no secret: their keys are in the
//! keyring), and the triage queue's pacing. `mode` is the compositor's;
//! any key she does not know is let through for later lots.
//!
//! The file is read again when it changed, at the next request: a role
//! switched in Settings applies to the next call, without a restart
//! (AC22).
//!
//! ```toml
//! [secretary.models]
//! answer = "anthropic/claude-sonnet-5-5"
//! triage = "local/qwen3:8b"
//! [secretary.providers.local]
//! kind = "ollama"
//! url = "http://127.0.0.1:11434/v1"
//! [secretary.triage]
//! battery = 50
//! ```

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Deserializer};

use crate::model::Role;

/// A role's provider and model: `PROVIDER/MODEL`, split at the first `/`
/// (a local model's name may hold a `:`, a remote one's a `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub provider: String,
    pub model: String,
}

impl Choice {
    pub fn of(text: &str) -> Option<Self> {
        let (provider, model) = text.split_once('/')?;
        (!provider.is_empty() && !model.is_empty()).then(|| Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
    }

    fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        }
    }
}

impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::of(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("{text:?} is not \"PROVIDER/MODEL\"")))
    }
}

/// The maintainer's choice (2026-11-09): Claude Sonnet 5.5 for what
/// needs understanding; a local 8B model for the triage only (the bench:
/// it triages well and fails the rest).
pub const CLAUDE: &str = "claude-sonnet-5-5";
pub const LOCAL: &str = "qwen3:8b";

/// Each role's choice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Models {
    pub answer: Choice,
    pub briefing: Choice,
    pub extraction: Choice,
    pub compaction: Choice,
    pub triage: Choice,
}

impl Default for Models {
    fn default() -> Self {
        let claude = Choice::new("anthropic", CLAUDE);
        Self {
            answer: claude.clone(),
            briefing: claude.clone(),
            extraction: claude.clone(),
            compaction: claude,
            triage: Choice::new("local", LOCAL),
        }
    }
}

impl Models {
    pub fn of(&self, role: Role) -> &Choice {
        match role {
            Role::Answer => &self.answer,
            Role::Briefing => &self.briefing,
            Role::Extraction => &self.extraction,
            Role::Compaction => &self.compaction,
            Role::Triage => &self.triage,
        }
    }
}

/// What a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The Anthropic Messages API; its key in the keyring.
    Anthropic,
    /// An OpenAI-compatible chat completions endpoint (llama.cpp, vLLM, a
    /// big model on another machine of the user's): no key unless the
    /// keyring has one.
    Openai,
    /// Ollama's OpenAI-compatible endpoint, which can also unload its
    /// model when the triage is idle.
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub kind: Kind,
    /// Anthropic: the API's root (`https://api.anthropic.com`); OpenAI
    /// kinds: the API's base, `…/v1`.
    pub url: String,
}

/// The triage queue's pacing (§11.1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Triage {
    /// On battery, the queue runs only above this charge, in percent.
    pub battery: u8,
    /// It pauses while the CPU package is above this, in °C.
    pub heat: u32,
    /// Idle this long, in seconds, the local model is told to unload.
    pub unload_after: u64,
    /// Seconds between two requests: one at a time, paced.
    pub pace: u64,
    /// The local model's context, in tokens, asked of Ollama (its own
    /// default, 40k, takes an 8B model past an 8 GB card): an item longer
    /// is cut to fit (#752).
    pub context: u32,
}

impl Default for Triage {
    fn default() -> Self {
        Self {
            battery: 50,
            heat: 95,
            unload_after: 120,
            pace: 2,
            context: 8192,
        }
    }
}

impl Triage {
    pub fn unload_after(&self) -> Duration {
        Duration::from_secs(self.unload_after)
    }

    pub fn pace(&self) -> Duration {
        Duration::from_secs(self.pace)
    }
}

/// When she speaks up (§6, §13 PRENDRE LA PAROLE): `[secretary.speak]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Speak {
    /// The morning briefing comes at the first session after this time.
    pub morning: String,
    /// Peeks a day at most, and minutes between two (VIPs aside).
    pub most: u32,
    pub apart: u32,
    /// Quiet hours of each notebook (`HH:MM-HH:MM`), the pro one's
    /// weekends too.
    pub quiet_pro: String,
    pub quiet_weekends: bool,
    pub quiet_perso: String,
    /// Seconds her peek stays out.
    pub stays: u32,
}

impl Default for Speak {
    fn default() -> Self {
        Self {
            morning: "07:00".to_owned(),
            most: 6,
            apart: 30,
            quiet_pro: "19:00-08:00".to_owned(),
            quiet_weekends: true,
            quiet_perso: "22:00-08:00".to_owned(),
            stays: 12,
        }
    }
}

impl Speak {
    /// The time the morning starts.
    pub fn morning(&self) -> jiff::civil::Time {
        self.morning
            .parse()
            .unwrap_or(jiff::civil::Time::constant(7, 0, 0, 0))
    }

    /// The shell's gate's settings.
    pub fn restraint(&self) -> hocus_secretary_link::gate::Restraint {
        use hocus_secretary_link::gate::{Hours, Restraint};
        let fallback = Restraint::default();
        Restraint {
            most: self.most,
            apart: self.apart,
            pro: Hours::of(&self.quiet_pro).unwrap_or(fallback.pro),
            pro_weekends: self.quiet_weekends,
            perso: Hours::of(&self.quiet_perso).unwrap_or(fallback.perso),
            stays: self.stays.clamp(4, 60),
        }
    }
}

/// The night's compaction (§7.2, lot 6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compaction {
    /// The hour it runs at, where the user is; on battery, only above
    /// `[secretary.triage] battery`, else an hour later, again.
    pub hour: i8,
}

impl Default for Compaction {
    fn default() -> Self {
        Self { hour: 3 }
    }
}

/// Her settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub models: Models,
    /// Added to, or replacing, the default `anthropic` and `local`.
    pub providers: BTreeMap<String, Provider>,
    pub triage: Triage,
    pub speak: Speak,
    pub compaction: Compaction,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models: Models::default(),
            providers: default_providers(),
            triage: Triage::default(),
            speak: Speak::default(),
            compaction: Compaction::default(),
        }
    }
}

fn default_providers() -> BTreeMap<String, Provider> {
    BTreeMap::from([
        (
            "anthropic".to_owned(),
            Provider {
                kind: Kind::Anthropic,
                url: "https://api.anthropic.com".to_owned(),
            },
        ),
        (
            "local".to_owned(),
            Provider {
                kind: Kind::Ollama,
                url: "http://127.0.0.1:11434/v1".to_owned(),
            },
        ),
    ])
}

impl Settings {
    /// `[secretary]` of a whole configuration file's text.
    pub fn of_config(text: &str) -> Result<Self, String> {
        let file: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let Some(section) = file.get("secretary").cloned() else {
            return Ok(Self::default());
        };
        let mut settings: Self = section
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string())?;
        let mut providers = default_providers();
        providers.append(&mut settings.providers);
        settings.providers = providers;
        Ok(settings)
    }

    /// A role's choice and its provider.
    pub fn provider(&self, role: Role) -> Result<(&Choice, &Provider), String> {
        let choice = self.models.of(role);
        let provider = self.providers.get(&choice.provider).ok_or_else(|| {
            format!(
                "[secretary.models] {} names no provider {:?}",
                role.word(),
                choice.provider
            )
        })?;
        Ok((choice, provider))
    }
}

/// The configuration file, read again when it changed.
#[derive(Debug)]
pub struct File {
    path: Option<PathBuf>,
    read: Mutex<(Option<(SystemTime, u64)>, Settings)>,
}

impl File {
    /// The file the compositor names (`HOCUS_CONFIG`), else
    /// `$XDG_CONFIG_HOME/hocuspanda/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        var("HOCUS_CONFIG").map(PathBuf::from).or_else(|| {
            var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))
                .map(|c| c.join("hocuspanda/config.toml"))
        })
    }

    pub fn at(path: Option<PathBuf>) -> Self {
        let file = Self {
            path,
            read: Mutex::new((None, Settings::default())),
        };
        file.current();
        file
    }

    /// Fixed settings, no file (tests).
    pub fn fixed(settings: Settings) -> Self {
        Self {
            path: None,
            read: Mutex::new((None, settings)),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The settings now: the file read again if it changed since; one that
    /// does not read leaves the last that did.
    pub fn current(&self) -> Settings {
        let mut read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(path) = &self.path else {
            return read.1.clone();
        };
        // Its time and length: a file rewritten within the clock's
        // granularity is still seen as changed when its length changed.
        let modified = std::fs::metadata(path)
            .and_then(|m| Ok((m.modified()?, m.len())))
            .ok();
        if modified != read.0 {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            match Settings::of_config(&text) {
                Ok(settings) => *read = (modified, settings),
                Err(err) => {
                    tracing::warn!(path = %path.display(), %err, "[secretary] does not read: the last settings kept");
                    read.0 = modified;
                }
            }
        }
        read.1.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_default_to_claude_and_the_triage_to_the_local_model() {
        let settings = Settings::of_config("[secretary]\nmode = \"assistant\"\n").expect("reads");
        assert_eq!(settings, Settings::default());
        let (choice, provider) = settings.provider(Role::Answer).expect("a provider");
        assert_eq!(choice.model, "claude-sonnet-5-5");
        assert_eq!(provider.kind, Kind::Anthropic);
        let (choice, provider) = settings.provider(Role::Triage).expect("a provider");
        assert_eq!(choice.model, "qwen3:8b");
        assert_eq!(provider.kind, Kind::Ollama);
    }

    #[test]
    fn a_role_names_a_provider_and_a_model() {
        let settings = Settings::of_config(
            "[secretary]\nbriefing = true\n[secretary.models]\nbriefing = \"anthropic/claude-opus-5-5\"\n\
             answer = \"big/org/model-70b\"\n[secretary.providers.big]\nkind = \"openai\"\n\
             url = \"https://gpu.example.org/v1\"\n",
        )
        .expect("reads");
        assert_eq!(settings.models.briefing.model, "claude-opus-5-5");
        assert_eq!(settings.models.answer.model, "org/model-70b");
        assert_eq!(
            settings.provider(Role::Answer).expect("big").1.kind,
            Kind::Openai
        );
        assert!(settings.providers.contains_key("anthropic"));
        assert!(Settings::of_config("[secretary.models]\nanswer = \"claude\"\n").is_err());
    }

    /// The commented `[secretary]` of the shipped configuration,
    /// uncommented as the compositor's test does, reads as her defaults.
    #[test]
    fn the_shipped_example_is_her_defaults() {
        let example = include_str!("../../../packaging/config.toml");
        let uncommented: String = example
            .lines()
            .filter_map(|l| l.strip_prefix("# ").filter(|l| !l.starts_with(' ')))
            .filter(|l| l.starts_with('[') || l.contains(" = "))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(uncommented.contains("[secretary.models]"), "{uncommented}");
        assert_eq!(
            Settings::of_config(&uncommented).expect("reads"),
            Settings::default()
        );
    }
}
