//! Offline intents (#44): short requests in English or French, understood
//! without a model ("capture de slack dans 3 secondes", "open the keyboard
//! settings"), shown in the command field for confirmation before anything
//! runs. Only what the shell already does: screenshots (a screen, every
//! screen, a window, a region or an application's window, at once or after
//! a delay), recording, opening an application or a settings page,
//! switching desktops, focus mode, night light, and notes.
//!
//! Both languages are understood whatever the interface speaks. A question
//! (ending with "?") is never an intent: it goes to the AI.

use hocus_core::{Action, ScreenshotMode};

use crate::{fuzzy, i18n::Lang, launcher::App};

/// What a request asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// An action, as from its shortcut.
    Run(Action),
    /// A screenshot of an application's window, after `delay` seconds.
    CaptureApp {
        /// Its name, as shown.
        name: String,
        /// What its windows' app id or desktop entry is matched against,
        /// lower case.
        matches: String,
        delay: u32,
    },
    Launch(App),
    /// A page of the settings, by its name in `hocus settings <page>`.
    Settings(String),
    /// A line added to the notes.
    Note(String),
}

/// The longest delay a screenshot waits, as its action allows.
const MAX_DELAY: u32 = 60;

/// Settings pages: name, English and French labels, words that say it.
const PAGES: [(&str, &str, &str, &[&str]); 19] = [
    ("keyboard", "Keyboard", "Clavier", &["keyboard", "clavier"]),
    (
        "mouse",
        "Mouse and touchpad",
        "Souris et pavé tactile",
        &["mouse", "souris", "touchpad", "trackpad"],
    ),
    (
        "screens",
        "Screens",
        "Écrans",
        &[
            "screens", "screen", "displays", "display", "monitors", "ecrans", "ecran",
        ],
    ),
    (
        "sound",
        "Sound",
        "Son",
        &["sound", "audio", "volume", "son"],
    ),
    (
        "network",
        "Network",
        "Réseau",
        &["network", "wifi", "internet", "reseau"],
    ),
    ("bluetooth", "Bluetooth", "Bluetooth", &["bluetooth"]),
    (
        "power",
        "Power",
        "Énergie",
        &[
            "power",
            "battery",
            "energy",
            "energie",
            "batterie",
            "alimentation",
        ],
    ),
    (
        "appearance",
        "Appearance",
        "Apparence",
        &["appearance", "apparence", "theme", "themes"],
    ),
    (
        "wallpaper",
        "Wallpaper",
        "Fond d’écran",
        &["wallpaper", "background", "fond"],
    ),
    (
        "language",
        "Language",
        "Langue",
        &["language", "languages", "langue", "langues"],
    ),
    (
        "desktops",
        "Desktops",
        "Bureaux",
        &["desktops", "desktop", "bureaux", "bureau"],
    ),
    (
        "windows",
        "Windows",
        "Fenêtres",
        &["windows", "window", "fenetres", "fenetre"],
    ),
    (
        "captures",
        "Captures",
        "Captures",
        &["captures", "capture", "screenshots", "screenshot"],
    ),
    (
        "clipboard",
        "Clipboard",
        "Presse-papiers",
        &["clipboard", "papiers"],
    ),
    (
        "lock",
        "Lock screen",
        "Verrouillage",
        &["lock", "verrouillage"],
    ),
    (
        "accessibility",
        "Accessibility",
        "Accessibilité",
        &["accessibility", "accessibilite"],
    ),
    ("pandas", "Pandas", "Pandas", &["pandas", "panda"]),
    (
        "notifications",
        "Notifications",
        "Notifications",
        &["notifications", "notification"],
    ),
    (
        "column",
        "Column and dock",
        "Colonne et dock",
        &["column", "colonne", "dock"],
    ),
];

/// Words that are only glue between the ones that matter.
const FILLERS: [&str; 26] = [
    "a", "an", "the", "of", "my", "this", "to", "on", "for", "un", "une", "de", "du", "des", "la",
    "le", "les", "l", "d", "mon", "ma", "mes", "ce", "cette", "cet", "sur",
];

/// What may come before a request: politeness, and verbs that only say
/// "do it".
const LEADING: [&str; 20] = [
    "please", "can", "could", "you", "stp", "svp", "peux", "pourrais", "tu", "take", "make",
    "grab", "do", "prends", "prend", "prendre", "fais", "fait", "faire", "go",
];

const OPEN: [&str; 12] = [
    "open", "launch", "start", "run", "ouvre", "ouvrir", "lance", "lancer", "demarre", "demarrer",
    "execute", "executer",
];

const SETTINGS: [&str; 9] = [
    "settings",
    "setting",
    "preferences",
    "preference",
    "reglages",
    "reglage",
    "parametres",
    "parametre",
    "config",
];

/// The words of a request: lower case, without accents nor punctuation,
/// "d'écran" as two words.
fn words(query: &str) -> Vec<String> {
    fuzzy::fold(query)
        .replace(
            [
                '\'', '’', '-', ',', '.', '!', ':', ';', '«', '»', '"', '“', '”',
            ],
            " ",
        )
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn number(word: &str) -> Option<u32> {
    if let Ok(n) = word.parse() {
        return Some(n);
    }
    const EN: [&str; 11] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    const FR: [&str; 11] = [
        "zero", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix",
    ];
    let small = EN
        .iter()
        .position(|w| *w == word)
        .or_else(|| FR.iter().position(|w| *w == word))
        .or_else(|| (word == "une" || word == "a" || word == "an").then_some(1));
    small.map(|n| n as u32).or(match word {
        "fifteen" | "quinze" => Some(15),
        "twenty" | "vingt" => Some(20),
        "thirty" | "trente" => Some(30),
        _ => None,
    })
}

/// Seconds per unit, for the units a delay is said in.
fn unit(word: &str) -> Option<u32> {
    match word {
        "s" | "sec" | "secs" | "second" | "seconds" | "seconde" | "secondes" => Some(1),
        "min" | "mins" | "minute" | "minutes" => Some(60),
        _ => None,
    }
}

/// Takes a delay out of the words: "in 3 seconds", "dans trois
/// secondes", "after 5 s", "3s". `Some(None)`: none was said.
fn take_delay(words: &mut Vec<String>) -> Option<Option<u32>> {
    for i in 0..words.len() {
        // "3s", "10sec": the number and its unit as one word.
        let glued = words[i]
            .find(|c: char| !c.is_ascii_digit())
            .filter(|at| *at > 0)
            .and_then(|at| {
                let (n, u) = words[i].split_at(at);
                Some(n.parse::<u32>().ok()? * unit(u)?)
            });
        let (seconds, end) = match glued {
            Some(s) => (s, i + 1),
            None => {
                let (Some(n), Some(u)) =
                    (number(&words[i]), words.get(i + 1).and_then(|w| unit(w)))
                else {
                    continue;
                };
                (n * u, i + 2)
            }
        };
        let start = match i.checked_sub(1).map(|p| words[p].as_str()) {
            Some("in" | "dans" | "after" | "apres" | "within") => i - 1,
            _ => i,
        };
        words.drain(start..end);
        return (seconds <= MAX_DELAY).then_some(Some(seconds));
    }
    Some(None)
}

fn drop_leading(words: &mut Vec<String>) {
    while words
        .first()
        .is_some_and(|w| LEADING.contains(&w.as_str()) || FILLERS.contains(&w.as_str()))
    {
        words.remove(0);
    }
}

fn without_fillers(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .map(String::as_str)
        .filter(|w| !FILLERS.contains(w))
        .collect()
}

/// Reads `query` as a request; `None` when it is not one.
pub fn parse(query: &str, apps: &[App]) -> Option<Intent> {
    let query = query.trim();
    if query.ends_with('?') || query.is_empty() {
        return None;
    }
    if let Some(note) = note(query) {
        return Some(Intent::Note(note));
    }
    let mut words = words(query);
    let delay = take_delay(&mut words)?;
    drop_leading(&mut words);
    if let Some(intent) = screenshot(&words, delay, apps) {
        return Some(intent);
    }
    // Only screenshots wait.
    if delay.is_some() {
        return None;
    }
    record(&words)
        .or_else(|| settings(&words))
        .or_else(|| desktop(&words))
        .or_else(|| toggle(&words))
        .or_else(|| launch(&words, apps))
}

/// "note buy bread", "noter : appeler Léa".
fn note(query: &str) -> Option<String> {
    let lower = query.to_lowercase();
    ["note:", "noter:", "note ", "noter "]
        .iter()
        .find(|p| lower.starts_with(*p))
        .map(|p| query[p.len()..].trim_start_matches([' ', ':']).trim())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn screenshot(words: &[String], delay: Option<u32>, apps: &[App]) -> Option<Intent> {
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    // The words that say "screenshot", and what follows them.
    let rest: &[&str] = match w.as_slice() {
        [
            "screenshot" | "screencap" | "capture" | "capturer",
            "d",
            "ecran",
            rest @ ..,
        ]
        | ["screen" | "ecran", "shot" | "capture" | "grab", rest @ ..]
        | ["copie" | "impression", "d", "ecran", rest @ ..]
        | [
            "screenshot" | "screencap" | "capture" | "capturer",
            rest @ ..,
        ] => rest,
        _ => return None,
    };
    let rest: Vec<&str> = rest
        .iter()
        .copied()
        .filter(|w| !FILLERS.contains(w))
        .collect();
    let mode = |mode| {
        Some(Intent::Run(Action::Screenshot {
            mode,
            delay: delay.unwrap_or(0),
        }))
    };
    match rest.as_slice() {
        [] | ["screen" | "ecran"] => mode(ScreenshotMode::Screen),
        [
            "all" | "every" | "tous" | "toutes",
            "screens" | "screen" | "ecrans",
        ]
        | ["everything" | "tout" | "all"] => mode(ScreenshotMode::All),
        ["window" | "fenetre"] => mode(ScreenshotMode::Window),
        ["region" | "zone" | "area" | "selection" | "part" | "partie"] => {
            mode(ScreenshotMode::Region)
        }
        named => {
            // "the slack window", "la fenêtre de slack": the application.
            let named: Vec<&str> = named
                .iter()
                .copied()
                .filter(|w| !["window", "fenetre", "app", "application", "appli"].contains(w))
                .collect();
            if named.is_empty() || named.len() > 3 {
                return None;
            }
            let matches = named.join(" ");
            let app = best_app(&matches, apps);
            Some(Intent::CaptureApp {
                name: app.map_or_else(|| capitalized(&matches), |a| a.name.clone()),
                matches,
                delay: delay.unwrap_or(0),
            })
        }
    }
}

fn capitalized(words: &str) -> String {
    words
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The application `name` says, starting its name or a word of it.
fn best_app<'a>(name: &str, apps: &'a [App]) -> Option<&'a App> {
    apps.iter()
        .filter_map(|app| {
            let by_name = fuzzy::score(name, &app.name);
            let by_generic = app
                .generic
                .as_deref()
                .and_then(|g| fuzzy::score(name, g))
                .map(|s| s - 500);
            let best = by_name.max(by_generic)?;
            (best >= 4_000).then_some((best, app))
        })
        .max_by(|(a, x), (b, y)| a.cmp(b).then_with(|| y.name.cmp(&x.name)))
        .map(|(_, app)| app)
}

fn record(words: &[String]) -> Option<Intent> {
    let w = without_fillers(words);
    let (first, rest) = w.split_first()?;
    let recording = match *first {
        "record" | "enregistre" | "enregistrer" | "filme" | "filmer" => rest
            .iter()
            .all(|w| ["screen", "ecran", "video", "it"].contains(w)),
        "start" | "begin" | "demarre" | "lance" | "commence" => {
            matches!(rest, ["recording" | "enregistrement"])
        }
        _ => false,
    };
    recording.then_some(Intent::Run(Action::Record))
}

fn settings(words: &[String]) -> Option<Intent> {
    let w = without_fillers(words);
    if !w.iter().any(|w| SETTINGS.contains(w)) {
        return None;
    }
    let others: Vec<&str> = w
        .iter()
        .copied()
        .filter(|w| !SETTINGS.contains(w) && !OPEN.contains(w) && *w != "show" && *w != "affiche")
        .collect();
    let [word] = others.as_slice() else {
        return None;
    };
    PAGES
        .iter()
        .find(|(_, _, _, said)| said.contains(word))
        .map(|(page, ..)| Intent::Settings((*page).to_owned()))
}

/// The settings pages a request can open, by name.
pub fn pages() -> impl Iterator<Item = &'static str> {
    PAGES.iter().map(|(page, ..)| *page)
}

/// The page's label.
pub fn page_label(page: &str, lang: Lang) -> &'static str {
    PAGES
        .iter()
        .find(|(p, ..)| *p == page)
        .map_or("", |(_, en, fr, _)| lang.pick(en, fr))
}

fn desktop(words: &[String]) -> Option<Intent> {
    let w = without_fillers(words);
    let at = w.iter().position(|w| *w == "desktop" || *w == "bureau")?;
    let n = w
        .get(at + 1)
        .and_then(|n| number(n))
        .filter(|n| (1..=9).contains(n))? as usize;
    let before = &w[..at];
    let sending = [
        "send", "move", "put", "envoie", "envoyer", "deplace", "deplacer", "mets", "mettre",
    ];
    let going = [
        "switch", "show", "va", "aller", "passe", "passer", "montre", "affiche", "au", "vers",
    ];
    if w.len() != at + 2 {
        return None;
    }
    if before.first().is_some_and(|v| sending.contains(v)) {
        let rest_ok = before[1..]
            .iter()
            .all(|w| ["window", "fenetre", "it", "la", "au", "vers", "dans"].contains(w));
        return rest_ok.then_some(Intent::Run(Action::SendToDesktop(n)));
    }
    before
        .iter()
        .all(|w| going.contains(w))
        .then_some(Intent::Run(Action::Desktop(n)))
}

fn toggle(words: &[String]) -> Option<Intent> {
    const GLUE: [&str; 14] = [
        "turn",
        "toggle",
        "enable",
        "disable",
        "on",
        "off",
        "stop",
        "active",
        "activer",
        "desactive",
        "desactiver",
        "mode",
        "en",
        "mets",
    ];
    let w: Vec<&str> = without_fillers(words)
        .into_iter()
        .filter(|w| !GLUE.contains(w))
        .collect();
    let action = match w.as_slice() {
        ["focus"] | ["concentration"] => Action::FocusMode,
        ["night", "light"] | ["lumiere" | "eclairage", "nocturne"] | ["veilleuse"] => {
            Action::NightLight
        }
        _ => return None,
    };
    Some(Intent::Run(action))
}

fn launch(words: &[String], apps: &[App]) -> Option<Intent> {
    let (verb, rest) = words.split_first()?;
    if !OPEN.contains(&verb.as_str()) {
        return None;
    }
    let rest: Vec<&str> = rest
        .iter()
        .map(String::as_str)
        .filter(|w| !FILLERS.contains(w) && !["app", "application", "appli"].contains(w))
        .collect();
    if rest.is_empty() || rest.len() > 3 {
        return None;
    }
    best_app(&rest.join(" "), apps).map(|app| Intent::Launch(app.clone()))
}

impl Intent {
    /// What it does, as the confirmation says it.
    pub fn title(&self, lang: Lang) -> String {
        let after = |delay: u32| match (delay, lang) {
            (0, _) => String::new(),
            (d, Lang::En) => format!(" in {d}\u{a0}s"),
            (d, Lang::Fr) => format!(" dans {d}\u{a0}s"),
        };
        match self {
            Self::Run(Action::Screenshot { mode, delay }) => {
                let what = match mode {
                    ScreenshotMode::Screen => {
                        lang.pick("Screenshot of the screen", "Capture de l’écran")
                    }
                    ScreenshotMode::All => {
                        lang.pick("Screenshot of every screen", "Capture de tous les écrans")
                    }
                    ScreenshotMode::Window => lang.pick(
                        "Screenshot of the window you click",
                        "Capture de la fenêtre cliquée",
                    ),
                    ScreenshotMode::Region => lang.pick(
                        "Screenshot of a region you draw",
                        "Capture d’une zone tracée",
                    ),
                };
                format!("{what}{}", after(*delay))
            }
            Self::CaptureApp { name, delay, .. } => match lang {
                Lang::En => format!("Screenshot of {name}{}", after(*delay)),
                Lang::Fr => format!("Capture de {name}{}", after(*delay)),
            },
            Self::Run(Action::Desktop(n)) => match lang {
                Lang::En => format!("Go to desktop {n}"),
                Lang::Fr => format!("Aller au bureau {n}"),
            },
            Self::Run(Action::SendToDesktop(n)) => match lang {
                Lang::En => format!("Send the window to desktop {n}"),
                Lang::Fr => format!("Envoyer la fenêtre au bureau {n}"),
            },
            Self::Run(action) => crate::actions::describe(action, lang).name,
            Self::Launch(app) => match lang {
                Lang::En => format!("Open {}", app.name),
                Lang::Fr => format!("Ouvrir {}", app.name),
            },
            Self::Settings(page) => match lang {
                Lang::En => format!("Open the settings: {}", page_label(page, lang)),
                Lang::Fr => format!("Ouvrir les réglages\u{202f}: {}", page_label(page, lang)),
            },
            Self::Note(text) => match lang {
                Lang::En => format!("Add “{text}” to the notes"),
                Lang::Fr => format!("Ajouter «\u{202f}{text}\u{202f}» aux notes"),
            },
        }
    }

    /// Its details, as chips under the title: for a screenshot, what is
    /// taken and when.
    pub fn facts(&self, lang: Lang) -> Vec<String> {
        let wait = |delay: u32| match (delay, lang) {
            (0, Lang::En) => "At once".to_owned(),
            (0, Lang::Fr) => "Tout de suite".to_owned(),
            (d, Lang::En) => format!("Delay: {d}\u{a0}s"),
            (d, Lang::Fr) => format!("Délai\u{202f}: {d}\u{a0}s"),
        };
        match self {
            Self::CaptureApp { name, delay, .. } => vec![
                match lang {
                    Lang::En => format!("Window: {name}"),
                    Lang::Fr => format!("Fenêtre\u{202f}: {name}"),
                },
                wait(*delay),
            ],
            Self::Run(Action::Screenshot { mode, delay }) => vec![
                match mode {
                    ScreenshotMode::Screen => {
                        lang.pick("Screen under the pointer", "Écran sous le pointeur")
                    }
                    ScreenshotMode::All => lang.pick("Every screen", "Tous les écrans"),
                    ScreenshotMode::Window => {
                        lang.pick("A window, clicked", "Une fenêtre, cliquée")
                    }
                    ScreenshotMode::Region => lang.pick("A region, drawn", "Une zone, tracée"),
                }
                .to_owned(),
                wait(*delay),
            ],
            _ => Vec::new(),
        }
    }

    /// The confirmation's button.
    pub fn confirm(&self, lang: Lang) -> &'static str {
        match self {
            Self::Run(Action::Screenshot { delay: 0, .. }) | Self::CaptureApp { delay: 0, .. } => {
                lang.pick("Take it", "Capturer")
            }
            Self::Run(Action::Screenshot { .. }) | Self::CaptureApp { .. } => {
                lang.pick("Start the delay", "Lancer le délai")
            }
            Self::Launch(_) | Self::Settings(_) => lang.pick("Open", "Ouvrir"),
            Self::Note(_) => lang.pick("Add", "Ajouter"),
            Self::Run(_) => lang.pick("Do it", "Faire"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<App> {
        ["Slack", "Firefox", "Terminal"]
            .iter()
            .map(|name| App {
                id: format!("{}.desktop", name.to_lowercase()),
                name: (*name).into(),
                generic: None,
                keywords: Vec::new(),
                exec: vec![name.to_lowercase()],
            })
            .collect()
    }

    fn read(query: &str) -> Option<Intent> {
        parse(query, &apps())
    }

    #[test]
    fn a_screenshot_of_an_app_after_a_delay_in_either_language() {
        let intent = Intent::CaptureApp {
            name: "Slack".into(),
            matches: "slack".into(),
            delay: 3,
        };
        let slack = Some(intent.clone());
        assert_eq!(read("capture de slack dans 3 secondes"), slack);
        assert_eq!(read("Capture de Slack dans trois secondes"), slack);
        assert_eq!(read("screenshot of slack in 3 seconds"), slack);
        assert_eq!(read("take a screenshot of the Slack window in 3s"), slack);
        assert_eq!(read("fais une capture d'écran de slack dans 3 s"), slack);
        assert_eq!(intent.title(Lang::Fr), "Capture de Slack dans 3\u{a0}s");
        assert_eq!(intent.title(Lang::En), "Screenshot of Slack in 3\u{a0}s");
        assert_eq!(
            intent.facts(Lang::En),
            vec!["Window: Slack".to_owned(), "Delay: 3\u{a0}s".to_owned()]
        );
    }

    #[test]
    fn screens_windows_and_regions_are_the_screenshot_action() {
        let shot = |mode, delay| Some(Intent::Run(Action::Screenshot { mode, delay }));
        assert_eq!(read("capture d'écran"), shot(ScreenshotMode::Screen, 0));
        assert_eq!(
            read("screenshot in 5 seconds"),
            shot(ScreenshotMode::Screen, 5)
        );
        assert_eq!(
            read("capture de tous les écrans"),
            shot(ScreenshotMode::All, 0)
        );
        assert_eq!(read("capture d'une zone"), shot(ScreenshotMode::Region, 0));
        assert_eq!(
            read("screenshot of a window after 10 s"),
            shot(ScreenshotMode::Window, 10)
        );
        assert_eq!(read("screenshot in 5 minutes"), None, "longer than allowed");
    }

    #[test]
    fn opening_settings_desktops_toggles_and_notes() {
        assert_eq!(
            read("open firefox"),
            Some(Intent::Launch(apps()[1].clone()))
        );
        assert_eq!(
            read("lance le terminal").map(|i| i.title(Lang::Fr)),
            Some("Ouvrir Terminal".into())
        );
        assert_eq!(read("open nothing-installed"), None);
        assert_eq!(
            read("réglages du clavier"),
            Some(Intent::Settings("keyboard".into()))
        );
        assert_eq!(
            read("open the sound settings"),
            Some(Intent::Settings("sound".into()))
        );
        assert_eq!(
            read("va sur le bureau 2"),
            Some(Intent::Run(Action::Desktop(2)))
        );
        assert_eq!(read("desktop 3"), Some(Intent::Run(Action::Desktop(3))));
        assert_eq!(
            read("envoie la fenêtre sur le bureau 4"),
            Some(Intent::Run(Action::SendToDesktop(4)))
        );
        assert_eq!(
            read("mode concentration"),
            Some(Intent::Run(Action::FocusMode))
        );
        assert_eq!(
            read("turn on night light"),
            Some(Intent::Run(Action::NightLight))
        );
        assert_eq!(
            read("enregistre l'écran"),
            Some(Intent::Run(Action::Record))
        );
        assert_eq!(
            read("note: acheter du pain"),
            Some(Intent::Note("acheter du pain".into()))
        );
    }

    #[test]
    fn questions_and_plain_words_are_no_intent() {
        for query in [
            "pourquoi le build est lent depuis ce matin ?",
            "how do I capture a screenshot?",
            "fir",
            "slack",
            "open firefox in 3 seconds",
            "2*21",
        ] {
            assert_eq!(read(query), None, "{query}");
        }
    }
}
