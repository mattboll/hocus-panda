//! Actions an AI's answer proposes (#197), under the answer in the command
//! field and on circling's card: open a settings page, change one key of
//! the configuration, run a request the command field understands offline
//! (#44), or hand the question and its answer to a coding agent (herdr,
//! #41). Nothing else, ever: no command, no free text that runs.
//!
//! The model is asked (in the instructions it already gets) to end its
//! answer with a fenced block tagged `hocus-actions`, one JSON object per
//! line. The block is never shown; each line is read strictly against the
//! fixed set here, and whatever does not fit is dropped: an unknown action,
//! page, key or value, a request the command field does not understand,
//! a malformed line. At most [`MAX`] are kept. Each becomes a button whose
//! choice shows what it will do (in words, a diff for the configuration,
//! the text an agent gets) and runs only once confirmed. While agents are
//! stopped (#176), none is offered.

use hocus_core::config_edit::{self, Change};
use serde_json::Value;

use crate::{
    i18n::Lang,
    intents::{self, Intent},
    launcher::App,
};

/// The fence's tag.
pub const FENCE: &str = "hocus-actions";
/// Proposals kept, at most.
pub const MAX: usize = 3;
/// A line longer than this is no proposal.
const MAX_LINE: usize = 400;
/// The agents offered to hand a question to, at most.
const MAX_AGENTS: usize = 3;
/// Lines of a long text shown before "…".
const PREVIEW_LINES: usize = 10;

/// A configuration value a proposal may set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Bool(bool),
    Int(i64),
    /// One of the key's own choices.
    Choice(&'static str),
}

impl Setting {
    fn toml(self) -> config_edit::Value {
        match self {
            Self::Bool(b) => b.into(),
            Self::Int(n) => n.into(),
            Self::Choice(s) => s.into(),
        }
    }

    /// As the file writes it: `true`, `120`, `"light"`.
    pub fn written(self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(n) => n.to_string(),
            Self::Choice(s) => format!("\"{s}\""),
        }
    }
}

/// What values a key takes.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Bool,
    Int(i64, i64),
    Choice(&'static [&'static str]),
}

/// The keys a proposal may change: plain looks and habits, nothing that
/// runs a program, names a file or an address, or weakens the lock.
const KEYS: &[(&str, &str, Kind)] = &[
    (
        "appearance",
        "theme",
        Kind::Choice(&["dark", "light", "matrix", "hud", "paper"]),
    ),
    (
        "appearance",
        "accent",
        Kind::Choice(&["theme", "bamboo", "amber", "blue", "pink", "wallpaper"]),
    ),
    ("appearance", "text_size", Kind::Int(80, 150)),
    ("appearance", "high_contrast", Kind::Bool),
    ("appearance", "reduced_motion", Kind::Bool),
    ("appearance", "transparency", Kind::Bool),
    ("windows", "snap", Kind::Bool),
    ("windows", "wobbly", Kind::Bool),
    ("windows.gaps", "inner", Kind::Int(0, 64)),
    ("windows.gaps", "outer", Kind::Int(0, 64)),
    ("decorations", "radius", Kind::Int(0, 32)),
    ("decorations", "shadow", Kind::Bool),
    ("decorations", "title_bars", Kind::Bool),
    ("decorations", "buttons", Kind::Choice(&["right", "left"])),
    ("keyboard", "repeat_delay", Kind::Int(100, 2000)),
    ("keyboard", "repeat_rate", Kind::Int(1, 100)),
    ("keyboard", "numlock", Kind::Bool),
    ("mouse", "natural_scroll", Kind::Bool),
    ("mouse", "left_handed", Kind::Bool),
    ("touchpad", "natural_scroll", Kind::Bool),
    ("touchpad", "tap_to_click", Kind::Bool),
    ("touchpad", "disable_while_typing", Kind::Bool),
    ("night_light", "enabled", Kind::Bool),
    ("night_light", "temperature", Kind::Int(1500, 6500)),
    ("notifications", "hold_while_sharing", Kind::Bool),
    ("notifications", "history", Kind::Int(0, 100)),
    ("screenshots", "save", Kind::Bool),
    ("screenshots", "copy", Kind::Bool),
    ("recording", "fps", Kind::Int(10, 60)),
    ("recording", "countdown", Kind::Int(0, 10)),
    ("recording", "show_clicks", Kind::Bool),
    ("idle", "curtain_after", Kind::Int(0, 86_400)),
    ("idle", "sleep_after", Kind::Int(0, 86_400)),
    ("idle", "fullscreen_awake", Kind::Bool),
    ("pandas", "enabled", Kind::Bool),
    ("dock", "enabled", Kind::Bool),
    ("clock", "format", Kind::Choice(&["24h", "12h"])),
    ("language", "interface", Kind::Choice(&["auto", "en", "fr"])),
];

/// What a model may propose, read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// A page of the settings, by its name in `hocus settings <page>`.
    Settings(String),
    /// One key of the configuration file set.
    Config {
        section: &'static str,
        key: &'static str,
        value: Setting,
    },
    /// A request, as the command field understands it offline.
    Intent(Intent),
    /// The question and the answer handed to a coding agent.
    Agent,
}

/// How a model is asked to propose actions: appended to its instructions.
/// Not shown.
pub fn instructions() -> String {
    let pages = intents::pages().collect::<Vec<_>>().join(", ");
    let keys = KEYS
        .iter()
        .map(|(section, key, kind)| {
            let values = match kind {
                Kind::Bool => "true or false".to_owned(),
                Kind::Int(low, high) => format!("{low} to {high}"),
                Kind::Choice(choices) => choices
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            format!("{section}.{key} ({values})")
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "\n\nWhen one of the desktop's own actions would directly help, end the answer with a \
         fenced code block tagged {FENCE}, one JSON object per line, at most {MAX}; otherwise \
         leave it out. Only these, nothing else and never a shell command (the user confirms \
         each one):\n\
         {{\"action\":\"settings\",\"page\":\"PAGE\"}} opens a page of the settings; PAGE is \
         one of {pages}.\n\
         {{\"action\":\"config\",\"section\":\"SECTION\",\"key\":\"KEY\",\"value\":VALUE}} \
         changes one key of the configuration file, among: {keys}.\n\
         {{\"action\":\"intent\",\"request\":\"REQUEST\"}} runs a short request such as \
         \"screenshot in 3 seconds\", \"night light\", \"focus mode\", \"go to desktop 2\", \
         \"open firefox\".\n\
         {{\"action\":\"agent\"}} hands the question and the answer to a coding agent, for \
         work on code.\n\
         Never mention the block in the answer's text."
    )
}

/// Where the block starts in `text`, its fence's line included; a fence
/// still coming in (`` ``` ``, `` ```hocus ``) at the very end counts.
fn block_start(text: &str) -> Option<usize> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(tag) = trimmed.strip_prefix("```") {
            let tag = tag.trim();
            let last = !line.ends_with('\n');
            if tag == FENCE || (last && FENCE.starts_with(tag)) {
                return Some(at);
            }
        }
        at += line.len();
    }
    None
}

/// The answer's text without the block, as the card shows it.
pub fn shown(text: &str) -> &str {
    match block_start(text) {
        Some(at) => text[..at].trim_end(),
        None => text,
    }
}

/// The proposals at the end of `text`, read strictly: `apps` for the
/// requests that name one.
pub fn read(text: &str, apps: &[App]) -> Vec<Proposal> {
    let Some(at) = block_start(text) else {
        return Vec::new();
    };
    let mut proposals = Vec::new();
    for line in text[at..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("```") {
            break;
        }
        if line.is_empty() || line.len() > MAX_LINE {
            continue;
        }
        let Some(proposal) = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| proposal(&v, apps))
        else {
            continue;
        };
        if !proposals.contains(&proposal) {
            proposals.push(proposal);
        }
        if proposals.len() == MAX {
            break;
        }
    }
    proposals
}

fn proposal(value: &Value, apps: &[App]) -> Option<Proposal> {
    let field = |name: &str| value.get(name);
    let text = |name: &str| field(name).and_then(Value::as_str);
    match text("action")? {
        "settings" => {
            let page = text("page")?;
            intents::pages()
                .find(|p| *p == page)
                .map(|p| Proposal::Settings(p.to_owned()))
        }
        "config" => {
            let (section, key) = (text("section")?, text("key")?);
            let (section, key, kind) = KEYS.iter().find(|(s, k, _)| *s == section && *k == key)?;
            let value = match (kind, field("value")?) {
                (Kind::Bool, Value::Bool(b)) => Setting::Bool(*b),
                (Kind::Int(low, high), Value::Number(n)) => {
                    Setting::Int(n.as_i64().filter(|n| (*low..=*high).contains(n))?)
                }
                (Kind::Choice(choices), Value::String(s)) => {
                    Setting::Choice(choices.iter().find(|c| **c == s)?)
                }
                _ => return None,
            };
            Some(Proposal::Config {
                section,
                key,
                value,
            })
        }
        "intent" => {
            let request = text("request").filter(|r| r.chars().count() <= 120)?;
            intents::parse(request, apps).map(Proposal::Intent)
        }
        "agent" => Some(Proposal::Agent),
        _ => None,
    }
}

/// What runs once confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Settings(String),
    Config {
        section: &'static str,
        key: &'static str,
        value: Setting,
    },
    Intent(Intent),
    /// `text` goes to the agent `key` of the board, called `name`.
    Agent {
        key: String,
        name: String,
        text: String,
    },
}

impl Run {
    /// The change the configuration's writer makes.
    pub fn change(&self) -> Option<Change> {
        match self {
            Self::Config {
                section,
                key,
                value,
            } => Some(Change::new(section, *key, value.toml())),
            _ => None,
        }
    }
}

/// A proposal as offered: its button, and what its confirmation shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The answer's button.
    pub label: String,
    /// What it will do, in words.
    pub title: String,
    /// Shown as code under the words: a diff, the text an agent gets.
    pub detail: Option<String>,
    /// Small facts as chips.
    pub chips: Vec<String>,
    /// The confirmation's buttons, and what each runs: several when it
    /// asks which agent.
    pub confirms: Vec<(String, Run)>,
}

/// What the desktop has for proposals to act on.
#[derive(Debug, Clone, Default)]
pub struct Context<'a> {
    /// Every agent is stopped (#176): nothing is offered.
    pub stopped: bool,
    /// The configuration file as last read; `None`: it is not changed.
    pub config: Option<&'a str>,
    /// herdr's agents a question can go to, best first: key and name.
    pub agents: &'a [(String, String)],
    /// What an agent gets: see [`prompt`].
    pub prompt: &'a str,
}

/// The proposals that can be offered, as offered.
pub fn offers(proposals: &[Proposal], context: &Context<'_>, lang: Lang) -> Vec<Offer> {
    if context.stopped {
        return Vec::new();
    }
    proposals
        .iter()
        .filter_map(|p| offer(p, context, lang))
        .collect()
}

fn offer(proposal: &Proposal, context: &Context<'_>, lang: Lang) -> Option<Offer> {
    Some(match proposal {
        Proposal::Settings(page) => {
            let intent = Intent::Settings(page.clone());
            Offer {
                label: lang.pick("Open the settings", "Ouvrir les réglages").into(),
                title: intent.title(lang),
                detail: None,
                chips: Vec::new(),
                confirms: vec![(intent.confirm(lang).into(), Run::Settings(page.clone()))],
            }
        }
        Proposal::Config {
            section,
            key,
            value,
        } => {
            let old = context.config?;
            let run = Run::Config {
                section,
                key,
                value: *value,
            };
            let new = config_edit::apply(old, &run.change()?).ok()?;
            // Nothing to change: not offered.
            let before = hocus_core::Config::parse(old).ok()?;
            let after = hocus_core::Config::parse(&new).ok()?;
            if before == after {
                return None;
            }
            let shown = value.written();
            Offer {
                label: lang
                    .pick("Fix the configuration", "Corriger la configuration")
                    .into(),
                title: match lang {
                    Lang::En => format!("Set {key} of [{section}] to {shown}"),
                    Lang::Fr => format!("Passer {key} de [{section}] à {shown}"),
                },
                detail: Some(diff(old, &new)),
                chips: vec![lang.pick("Comments kept", "Commentaires gardés").into()],
                confirms: vec![(lang.pick("Change it", "Modifier").into(), run)],
            }
        }
        Proposal::Intent(intent) => Offer {
            label: intent.title(lang),
            title: intent.title(lang),
            detail: None,
            chips: intent.facts(lang),
            confirms: vec![(intent.confirm(lang).into(), Run::Intent(intent.clone()))],
        },
        Proposal::Agent => {
            if context.agents.is_empty() {
                return None;
            }
            Offer {
                label: lang.pick("Hand to an agent", "Confier à un agent").into(),
                title: lang
                    .pick(
                        "Give the question and the answer to a coding agent",
                        "Donner la question et la réponse à un agent de code",
                    )
                    .into(),
                detail: Some(preview(context.prompt)),
                chips: vec![lang.pick("Through herdr", "Par herdr").into()],
                confirms: context
                    .agents
                    .iter()
                    .take(MAX_AGENTS)
                    .map(|(key, name)| {
                        let label = match lang {
                            Lang::En => format!("Give to {name}"),
                            Lang::Fr => format!("Donner à {name}"),
                        };
                        let run = Run::Agent {
                            key: key.clone(),
                            name: name.clone(),
                            text: context.prompt.to_owned(),
                        };
                        (label, run)
                    })
                    .collect(),
            }
        }
    })
}

/// The start of a long text, "…" after.
fn preview(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().take(PREVIEW_LINES + 1).collect();
    if lines.len() > PREVIEW_LINES {
        lines.truncate(PREVIEW_LINES);
        lines.push("…");
    }
    lines.join("\n")
}

/// What an agent gets: the question, what was circled (#46), the answer.
pub fn prompt(lang: Lang, question: &str, circled: Option<&str>, answer: &str) -> String {
    let mut out = question.trim().to_owned();
    if let Some(circled) = circled.map(str::trim).filter(|c| !c.is_empty()) {
        out.push_str("\n\n");
        out.push_str(lang.pick("Circled on the screen:", "Entouré à l’écran\u{a0}:"));
        out.push('\n');
        out.push_str(circled);
    }
    out.push_str("\n\n");
    out.push_str(lang.pick("An AI answered:", "Une IA a répondu\u{a0}:"));
    out.push('\n');
    out.push_str(answer.trim());
    out
}

/// The lines that differ between `old` and `new`, `- ` before and `+ `
/// after, under the table they are in.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let end = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out = Vec::new();
    if let Some(header) = a[..start]
        .iter()
        .rev()
        .find(|l| l.trim_start().starts_with('['))
    {
        out.push(format!("  {header}"));
    }
    let written = |l: &&&str| !l.trim().is_empty();
    out.extend(
        a[start..a.len() - end]
            .iter()
            .filter(written)
            .map(|l| format!("- {l}")),
    );
    out.extend(
        b[start..b.len() - end]
            .iter()
            .filter(written)
            .map(|l| format!("+ {l}")),
    );
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use hocus_core::{Action, Config};

    use super::*;

    fn block(lines: &[&str]) -> String {
        format!(
            "The theme is dark.\n\n```{FENCE}\n{}\n```\n",
            lines.join("\n")
        )
    }

    #[test]
    fn the_block_is_never_shown_even_while_it_streams_in() {
        let text = block(&[r#"{"action":"agent"}"#]);
        assert_eq!(shown(&text), "The theme is dark.");
        assert_eq!(shown("The theme is dark.\n\n```hoc"), "The theme is dark.");
        assert_eq!(shown("The theme is dark.\n\n```"), "The theme is dark.");
        // Another code block stays.
        let code = "Run:\n```sh\nls\n```\n";
        assert_eq!(shown(code), code);
    }

    #[test]
    fn only_the_fixed_set_is_read_and_at_most_three() {
        let text = block(&[
            r#"{"action":"config","section":"appearance","key":"theme","value":"light"}"#,
            r#"{"action":"shell","command":"rm -rf ~"}"#,
            r#"{"action":"config","section":"settings","key":"command","value":"sh -c x"}"#,
            r#"{"action":"config","section":"appearance","key":"theme","value":"neon"}"#,
            r#"{"action":"config","section":"appearance","key":"text_size","value":400}"#,
            r#"{"action":"settings","page":"../../etc"}"#,
            r#"{"action":"intent","request":"rm -rf ~"}"#,
            "not json at all",
            r#"{"action":"config","section":"appearance","key":"theme","value":"light"}"#,
            r#"{"action":"intent","request":"night light","label":"ignored"}"#,
            r#"{"action":"settings","page":"keyboard"}"#,
            r#"{"action":"agent"}"#,
        ]);
        assert_eq!(
            read(&text, &[]),
            vec![
                Proposal::Config {
                    section: "appearance",
                    key: "theme",
                    value: Setting::Choice("light"),
                },
                Proposal::Intent(Intent::Run(Action::NightLight)),
                Proposal::Settings("keyboard".into()),
            ]
        );
        assert!(read("No block here.", &[]).is_empty());
    }

    #[test]
    fn every_key_offered_is_one_the_configuration_takes() {
        for (section, key, kind) in KEYS {
            let value = match kind {
                Kind::Bool => Setting::Bool(true),
                Kind::Int(low, _) => Setting::Int(*low),
                Kind::Choice(c) => Setting::Choice(c[0]),
            };
            let change = Change::new(section, *key, value.toml());
            assert!(
                config_edit::apply("", &change).is_ok(),
                "{section}.{key} is refused"
            );
        }
        assert!(instructions().contains("appearance.theme (\"dark\", \"light\""));
    }

    #[test]
    fn a_configuration_change_shows_as_a_diff_and_keeps_the_comments() {
        let old = "# mine\n[appearance]\ntheme = \"dark\" # at night\ntext_size = 110\n";
        let proposals = [Proposal::Config {
            section: "appearance",
            key: "theme",
            value: Setting::Choice("light"),
        }];
        let context = Context {
            config: Some(old),
            ..Context::default()
        };
        let offers = offers(&proposals, &context, Lang::En);
        let [offer] = offers.as_slice() else {
            panic!("one offer: {offers:?}");
        };
        assert_eq!(offer.label, "Fix the configuration");
        assert_eq!(offer.title, "Set theme of [appearance] to \"light\"");
        assert_eq!(
            offer.detail.as_deref(),
            Some("  [appearance]\n- theme = \"dark\" # at night\n+ theme = \"light\" # at night")
        );
        // Already so: nothing offered.
        let same = Context {
            config: Some("[appearance]\ntheme = \"light\"\n"),
            ..Context::default()
        };
        assert!(super::offers(&proposals, &same, Lang::En).is_empty());
        // A key added to a file without it.
        let added = diff(
            "",
            &config_edit::apply("", &proposals_change()).expect("valid"),
        );
        assert_eq!(added, "+ [appearance]\n+ theme = \"light\"");
        assert!(Config::parse(old).is_ok());
    }

    fn proposals_change() -> Change {
        Change::new("appearance", "theme", Setting::Choice("light").toml())
    }

    #[test]
    fn nothing_is_offered_while_agents_are_stopped_nor_an_agent_without_one() {
        let proposals = [Proposal::Agent, Proposal::Settings("keyboard".into())];
        let agents = [("0/p1".to_owned(), "refacto".to_owned())];
        let prompt = prompt(Lang::En, "why?", Some("error: x"), "Because.");
        assert_eq!(
            prompt,
            "why?\n\nCircled on the screen:\nerror: x\n\nAn AI answered:\nBecause."
        );
        let mut context = Context {
            agents: &agents,
            prompt: &prompt,
            ..Context::default()
        };
        let all = offers(&proposals, &context, Lang::Fr);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].label, "Confier à un agent");
        assert_eq!(
            all[0].confirms,
            vec![(
                "Donner à refacto".to_owned(),
                Run::Agent {
                    key: "0/p1".into(),
                    name: "refacto".into(),
                    text: prompt.clone(),
                }
            )]
        );
        context.agents = &[];
        assert_eq!(offers(&proposals, &context, Lang::En).len(), 1);
        context.stopped = true;
        assert!(offers(&proposals, &context, Lang::En).is_empty());
    }
}
