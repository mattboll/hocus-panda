//! What the launcher offers, provider by provider: each looks at what is
//! typed and adds its results. A plugin is one more [`Provider`] (decision
//! 0004: compiled in). The configuration turns each one off.

use hocus_ui::Color;

use crate::{
    answer::Source,
    calc, fuzzy,
    i18n::Lang,
    intents,
    launcher::{App, Choice, Document, Showcase},
    toasts::app_color,
    units,
};

/// A result as the launcher lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    /// What it is, on the right: "Application", "Calculator"…
    pub kind: String,
    /// Its square; the accent when `None`.
    pub color: Option<Color>,
    pub choice: Choice,
}

/// What a provider sees besides the query.
#[derive(Debug)]
pub struct Context<'a> {
    pub apps: &'a [App],
    /// Documents found for the query (#45), as the search answered.
    pub documents: &'a [Document],
    pub lang: Lang,
}

pub trait Provider {
    /// Its name in the configuration.
    fn name(&self) -> &'static str;
    /// Adds its results for `query` (already trimmed, not empty).
    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>);
}

/// Which providers run, from the configuration's `[launcher]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enabled {
    pub calculator: bool,
    pub units: bool,
    pub urls: bool,
    pub web_search: bool,
    /// `{}` where the words go.
    pub search_engine: String,
    pub large_type: bool,
    pub qr: bool,
    /// Requests understood offline, confirmed before they run (#44).
    pub intents: bool,
    /// Who answers what is asked; `None`: nothing is offered to ask.
    pub ask: Option<Source>,
    /// Documents searched for what is typed (#45).
    pub documents: bool,
}

impl Default for Enabled {
    fn default() -> Self {
        Self {
            calculator: true,
            units: true,
            urls: true,
            web_search: true,
            search_engine: "https://duckduckgo.com/?q={}".into(),
            large_type: true,
            qr: true,
            intents: true,
            ask: None,
            documents: false,
        }
    }
}

/// The providers, in the order their results come.
pub fn providers(enabled: &Enabled) -> Vec<Box<dyn Provider>> {
    let mut out: Vec<Box<dyn Provider>> = Vec::new();
    if enabled.intents {
        out.push(Box::new(Intents));
    }
    if let Some(source) = &enabled.ask {
        out.push(Box::new(Ask {
            source: source.clone(),
            questions: true,
        }));
    }
    if enabled.large_type {
        out.push(Box::new(LargeType));
    }
    if enabled.qr {
        out.push(Box::new(Qr));
    }
    if enabled.calculator {
        out.push(Box::new(Calculator));
    }
    if enabled.units {
        out.push(Box::new(Units));
    }
    if enabled.urls {
        out.push(Box::new(Urls));
    }
    out.push(Box::new(Settings));
    out.push(Box::new(SessionActions));
    out.push(Box::new(Apps));
    if enabled.documents {
        out.push(Box::new(Documents));
    }
    if let Some(source) = &enabled.ask {
        out.push(Box::new(Ask {
            source: source.clone(),
            questions: false,
        }));
    }
    if enabled.web_search {
        out.push(Box::new(WebSearch {
            engine: enabled.search_engine.clone(),
        }));
    }
    out
}

/// A request understood offline: shown for confirmation, first.
struct Intents;

impl Provider for Intents {
    fn name(&self) -> &'static str {
        "intents"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if let Some(intent) = intents::parse(query, cx.apps) {
            out.push(Hit {
                title: intent.title(cx.lang),
                kind: cx.lang.intent_kind().into(),
                color: None,
                choice: Choice::Intent(intent),
            });
        }
    }
}

/// Words that start a question, in either language.
const QUESTION_WORDS: [&str; 34] = [
    "why",
    "how",
    "what",
    "when",
    "where",
    "who",
    "which",
    "whose",
    "can",
    "could",
    "should",
    "would",
    "is",
    "are",
    "does",
    "do",
    "explain",
    "summarize",
    "translate",
    "pourquoi",
    "comment",
    "quoi",
    "que",
    "quel",
    "quelle",
    "quels",
    "quelles",
    "quand",
    "ou",
    "qui",
    "combien",
    "explique",
    "resume",
    "traduis",
];

/// What reads as a question: it ends with "?", or starts as one does.
pub fn is_question(query: &str) -> bool {
    let first = fuzzy::fold(query)
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty())
        .map(str::to_owned)
        .unwrap_or_default();
    query.trim_end().ends_with('?')
        || (QUESTION_WORDS.contains(&first.as_str()) && query.split_whitespace().count() >= 3)
        || first == "est" && fuzzy::fold(query).starts_with("est-ce")
}

/// Asks the AI: first for a question, else after the applications.
struct Ask {
    source: Source,
    /// This one offers questions only; the other everything else.
    questions: bool,
}

impl Provider for Ask {
    fn name(&self) -> &'static str {
        "ai"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if is_question(query) != self.questions {
            return;
        }
        out.push(Hit {
            title: cx.lang.ask(query),
            kind: self.source.label(cx.lang),
            color: None,
            choice: Choice::Ask(query.to_owned()),
        });
    }
}

/// `large hello`: the words over the whole screen.
struct LargeType;

impl Provider for LargeType {
    fn name(&self) -> &'static str {
        "large_type"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        let text = ["large ", "grand "]
            .iter()
            .find_map(|p| query.strip_prefix(p))
            .map(str::trim);
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            out.push(Hit {
                title: cx.lang.large_type(text),
                kind: cx.lang.large_type_kind().into(),
                color: None,
                choice: Choice::Show(Showcase::LargeText(text.into())),
            });
        }
    }
}

/// `qr hello`: a QR code of the words over the whole screen.
struct Qr;

impl Provider for Qr {
    fn name(&self) -> &'static str {
        "qr"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if let Some(text) = query
            .strip_prefix("qr ")
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            out.push(Hit {
                title: cx.lang.qr_of(text),
                kind: "QR".into(),
                color: None,
                choice: Choice::Show(Showcase::Qr(text.into())),
            });
        }
    }
}

struct Calculator;

impl Provider for Calculator {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if let Some(value) = calc::evaluate(query) {
            out.push(Hit {
                title: format!("= {value}"),
                kind: cx.lang.calculator().into(),
                color: None,
                choice: Choice::Copy(value),
            });
        }
    }
}

struct Units;

impl Provider for Units {
    fn name(&self) -> &'static str {
        "units"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if let Some((shown, number)) = units::convert(query) {
            out.push(Hit {
                title: format!("= {shown}"),
                kind: cx.lang.conversion().into(),
                color: None,
                choice: Choice::Copy(number),
            });
        }
    }
}

/// An address: opened in the browser.
struct Urls;

/// `query` as an address, if it reads as one.
fn as_url(query: &str) -> Option<String> {
    if query.contains(char::is_whitespace) {
        return None;
    }
    if query.starts_with("https://") || query.starts_with("http://") {
        return Some(query.into());
    }
    let host = query.split('/').next()?;
    let labels: Vec<&str> = host.split('.').collect();
    let looks_like_host = labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(char::is_alphabetic));
    looks_like_host.then(|| format!("https://{query}"))
}

impl Provider for Urls {
    fn name(&self) -> &'static str {
        "urls"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        if let Some(url) = as_url(query) {
            out.push(Hit {
                title: cx.lang.open(&url),
                kind: "Web".into(),
                color: None,
                choice: Choice::Open(url),
            });
        }
    }
}

struct Apps;

impl Provider for Apps {
    fn name(&self) -> &'static str {
        "applications"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        let mut scored: Vec<(i32, &App)> = cx
            .apps
            .iter()
            .filter_map(|app| {
                let by_name = fuzzy::score(query, &app.name);
                let by_generic = app
                    .generic
                    .as_deref()
                    .and_then(|g| fuzzy::score(query, g))
                    .map(|s| s - 500);
                let by_keyword = app
                    .keywords
                    .iter()
                    .filter_map(|k| fuzzy::score(query, k))
                    .max()
                    .map(|s| s - 800);
                [by_name, by_generic, by_keyword]
                    .into_iter()
                    .flatten()
                    .max()
                    .map(|s| (s, app))
            })
            .collect();
        scored.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.name.cmp(&y.name)));
        out.extend(scored.into_iter().map(|(_, app)| Hit {
            title: app.name.clone(),
            kind: cx.lang.application().into(),
            color: Some(app_color(&app.name)),
            choice: Choice::Launch(app.clone()),
        }));
    }
}

/// The documents the search found for what is typed (#45), after the
/// applications.
struct Documents;

/// Their square's colour.
pub const DOCUMENT_COLOR: Color = Color::rgb(0x3E, 0x6E, 0xA8);

impl Provider for Documents {
    fn name(&self) -> &'static str {
        "documents"
    }

    fn hits(&self, _query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        out.extend(cx.documents.iter().map(|d| Hit {
            title: d.name.clone(),
            kind: d.folder.clone(),
            color: Some(DOCUMENT_COLOR),
            choice: Choice::OpenFile(d.path.clone()),
        }));
    }
}

/// `settings`, `réglages`: the settings app.
struct Settings;

impl Provider for Settings {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        const NAMES: [&str; 6] = [
            "settings",
            "réglages",
            "preferences",
            "préférences",
            "paramètres",
            "configuration",
        ];
        if NAMES
            .iter()
            .any(|n| fuzzy::score(query, n).is_some_and(|s| s >= 10_000))
        {
            out.push(Hit {
                title: cx.lang.settings().into(),
                kind: "Hocuspanda".into(),
                color: None,
                choice: Choice::Settings,
            });
        }
    }
}

/// `shut down`, `éteindre`, `lock`…: locking, sleeping, leaving.
struct SessionActions;

impl Provider for SessionActions {
    fn name(&self) -> &'static str {
        "session"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        use crate::launcher::SessionAction as S;
        let actions: [(S, &[&str], &str); 5] = [
            (S::Lock, &["lock", "verrouiller"], cx.lang.lock()),
            (
                S::Suspend,
                &["suspend", "sleep", "veille", "mettre en veille"],
                cx.lang.suspend(),
            ),
            (
                S::LogOut,
                &["log out", "logout", "se déconnecter", "déconnexion"],
                cx.lang.log_out(),
            ),
            (
                S::Restart,
                &["restart", "reboot", "redémarrer"],
                cx.lang.restart(),
            ),
            (
                S::ShutDown,
                &["shut down", "shutdown", "power off", "éteindre"],
                cx.lang.shut_down(),
            ),
        ];
        for (action, names, title) in actions {
            // What is typed starts one of its names.
            if names
                .iter()
                .any(|n| fuzzy::score(query, n).is_some_and(|s| s >= 9_000))
            {
                out.push(Hit {
                    title: title.into(),
                    kind: "Hocuspanda".into(),
                    color: None,
                    choice: Choice::Session(action),
                });
            }
        }
    }
}

struct WebSearch {
    engine: String,
}

/// Percent-encoding for a query string: unreserved characters stay.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Provider for WebSearch {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn hits(&self, query: &str, cx: &Context<'_>, out: &mut Vec<Hit>) {
        out.push(Hit {
            title: cx.lang.search_web(query),
            kind: "Web".into(),
            color: None,
            choice: Choice::Open(self.engine.replace("{}", &encode(query))),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(query: &str, enabled: &Enabled) -> Vec<Hit> {
        let cx = Context {
            apps: &[],
            documents: &[],
            lang: Lang::En,
        };
        let mut out = Vec::new();
        for p in providers(enabled) {
            p.hits(query, &cx, &mut out);
        }
        out
    }

    #[test]
    fn leaving_and_locking_are_found_in_both_languages() {
        let first = |q: &str| {
            hits(q, &Enabled::default())
                .into_iter()
                .find(|h| matches!(h.choice, Choice::Session(_)))
                .map(|h| h.choice)
        };
        use crate::launcher::SessionAction as S;
        assert_eq!(first("shut down"), Some(Choice::Session(S::ShutDown)));
        assert_eq!(first("éteindre"), Some(Choice::Session(S::ShutDown)));
        assert_eq!(first("redémarrer"), Some(Choice::Session(S::Restart)));
        assert_eq!(first("verrouiller"), Some(Choice::Session(S::Lock)));
        assert_eq!(first("hello"), None);
    }

    #[test]
    fn addresses_are_recognised() {
        assert_eq!(
            as_url("example.org").as_deref(),
            Some("https://example.org")
        );
        assert_eq!(
            as_url("docs.rs/zbus").as_deref(),
            Some("https://docs.rs/zbus")
        );
        assert_eq!(
            as_url("http://localhost:8080").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(as_url("hello world"), None);
        assert_eq!(as_url("v1.2"), None);
    }

    #[test]
    fn web_search_comes_last_and_can_be_turned_off() {
        let all = hits("2*21", &Enabled::default());
        assert_eq!(all.first().map(|h| h.title.as_str()), Some("= 42"));
        assert!(
            matches!(&all.last().map(|h| &h.choice), Some(Choice::Open(u)) if u == "https://duckduckgo.com/?q=2%2A21")
        );
        let off = Enabled {
            web_search: false,
            ..Enabled::default()
        };
        assert!(hits("hello there", &off).is_empty());
    }

    #[test]
    fn commands_take_the_rest_of_the_line() {
        let h = hits("qr hello", &Enabled::default());
        assert_eq!(
            h.first().map(|h| &h.choice),
            Some(&Choice::Show(Showcase::Qr("hello".into())))
        );
        let h = hits("large Hello there", &Enabled::default());
        assert_eq!(
            h.first().map(|h| &h.choice),
            Some(&Choice::Show(Showcase::LargeText("Hello there".into())))
        );
    }
}
