//! Everything the settings app knows of: four families of pages, the
//! settings on each page, and the search across them by label, synonym and
//! value, in English and French.

use hocus_api::OutputInfo;
use hocus_core::config::Config;
use hocus_shell::{Lang, fuzzy};
use hocus_ui::Accent;

use crate::words::{self, Words};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Personalise,
    Work,
    Hardware,
    System,
}

impl Family {
    pub const ALL: [Family; 4] = [Self::Personalise, Self::Work, Self::Hardware, Self::System];

    pub fn label(self) -> Words {
        match self {
            Self::Personalise => Words::new("Personalise", "Personnaliser"),
            Self::Work => Words::new("Work", "Travailler"),
            Self::Hardware => Words::new("Hardware", "Matériel"),
            Self::System => Words::new("System", "Système"),
        }
    }

    /// The colour of its tiles' icons.
    pub fn tint(self) -> Accent {
        match self {
            Self::Personalise => Accent::Bamboo,
            Self::Work => Accent::Blue,
            Self::Hardware => Accent::Amber,
            Self::System => Accent::Pink,
        }
    }

    pub fn pages(self) -> impl Iterator<Item = Page> {
        Page::ALL.into_iter().filter(move |p| p.family() == self)
    }
}

/// A page of settings. Those not built yet show on the home, greyed, and
/// come with #67.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Appearance,
    Wallpaper,
    ColumnDock,
    Pandas,
    Accessibility,
    Language,
    Desktops,
    Windows,
    Keyboard,
    Mouse,
    Captures,
    Clipboard,
    Screens,
    Sound,
    Network,
    Bluetooth,
    Power,
    Lock,
    Notifications,
    Apps,
    Agents,
    Search,
    Accounts,
    Account,
}

impl Page {
    pub const ALL: [Page; 24] = [
        Self::Appearance,
        Self::Wallpaper,
        Self::ColumnDock,
        Self::Pandas,
        Self::Accessibility,
        Self::Language,
        Self::Desktops,
        Self::Windows,
        Self::Keyboard,
        Self::Mouse,
        Self::Captures,
        Self::Clipboard,
        Self::Screens,
        Self::Sound,
        Self::Network,
        Self::Bluetooth,
        Self::Power,
        Self::Lock,
        Self::Notifications,
        Self::Apps,
        Self::Agents,
        Self::Search,
        Self::Accounts,
        Self::Account,
    ];

    pub fn family(self) -> Family {
        use Page::*;
        match self {
            Appearance | Wallpaper | ColumnDock | Pandas | Accessibility | Language => {
                Family::Personalise
            }
            Desktops | Windows | Keyboard | Mouse | Captures | Clipboard => Family::Work,
            Screens | Sound | Network | Bluetooth | Power | Lock => Family::Hardware,
            Notifications | Apps | Agents | Search | Accounts | Account => Family::System,
        }
    }

    /// Whether the page is built.
    pub fn available(self) -> bool {
        matches!(self, Self::Appearance | Self::Keyboard | Self::Screens)
    }

    /// Its name in `hocus settings <page>`.
    pub fn slug(self) -> &'static str {
        use Page::*;
        match self {
            Appearance => "appearance",
            Wallpaper => "wallpaper",
            ColumnDock => "column",
            Pandas => "pandas",
            Accessibility => "accessibility",
            Language => "language",
            Desktops => "desktops",
            Windows => "windows",
            Keyboard => "keyboard",
            Mouse => "mouse",
            Captures => "captures",
            Clipboard => "clipboard",
            Screens => "screens",
            Sound => "sound",
            Network => "network",
            Bluetooth => "bluetooth",
            Power => "power",
            Lock => "lock",
            Notifications => "notifications",
            Apps => "apps",
            Agents => "agents",
            Search => "search",
            Accounts => "accounts",
            Account => "account",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.slug() == slug)
    }

    pub fn label(self) -> Words {
        use Page::*;
        match self {
            Appearance => Words::new("Appearance", "Apparence"),
            Wallpaper => Words::new("Wallpaper", "Fond d’écran"),
            ColumnDock => Words::new("Column and dock", "Colonne et dock"),
            Pandas => Words::new("Pandas", "Pandas"),
            Accessibility => Words::new("Accessibility", "Accessibilité"),
            Language => Words::new("Language and region", "Langue et région"),
            Desktops => Words::new("Desktops", "Bureaux"),
            Windows => Words::new("Windows", "Fenêtres"),
            Keyboard => Words::new("Keyboard and shortcuts", "Clavier et raccourcis"),
            Mouse => Words::new("Mouse and touchpad", "Souris et pavé"),
            Captures => Words::new("Screenshots and recording", "Captures et enregistrement"),
            Clipboard => Words::new("Clipboard and emoji", "Presse-papiers et emoji"),
            Screens => Words::new("Screens", "Écrans"),
            Sound => Words::new("Sound", "Son"),
            Network => Words::new("Network", "Réseau"),
            Bluetooth => Words::new("Bluetooth", "Bluetooth"),
            Power => Words::new("Power", "Énergie"),
            Lock => Words::new("Lock", "Verrouillage"),
            Notifications => Words::new("Notifications", "Notifications"),
            Apps => Words::new("Applications", "Applications"),
            Agents => Words::new("AI and agents", "IA et agents"),
            Search => Words::new("Search", "Recherche"),
            Accounts => Words::new("Accounts and calendar", "Comptes et agenda"),
            Account => Words::new("Account and updates", "Compte et mises à jour"),
        }
    }

    /// Other words people look for it with.
    fn synonyms(self) -> &'static [&'static str] {
        use Page::*;
        match self {
            Appearance => &[
                "look", "colour", "color", "couleur", "dark", "sombre", "style",
            ],
            Wallpaper => &["background", "fond", "image", "picture"],
            Screens => &[
                "display",
                "monitor",
                "moniteur",
                "affichage",
                "resolution",
                "résolution",
            ],
            Keyboard => &["keys", "touches", "layout", "azerty", "qwerty", "raccourci"],
            Sound => &["audio", "volume"],
            Power => &["battery", "batterie", "sleep", "veille"],
            Network => &["wifi", "wi-fi", "internet"],
            _ => &[],
        }
    }

    pub fn icon(self) -> &'static str {
        use Page::*;
        match self {
            Appearance => {
                "M12 3a9 9 0 1 0 9 9c0-1-1-1.5-2-1.5h-2a2 2 0 0 1-2-2V7c0-1.5-1-4-3-4zM7.5 12h0M9 7.5h0M14 6.5h0"
            }
            Wallpaper => "M3 5h18v14H3zM3 15l5-5 4 4 3-3 6 6",
            ColumnDock => "M15 3h6v18h-6zM3 3h9M3 8h9M3 13h9",
            Pandas => {
                "M4 6a3 3 0 1 0 6 0 3 3 0 1 0-6 0M14 6a3 3 0 1 0 6 0 3 3 0 1 0-6 0M5 15a7 7 0 1 0 14 0 7 7 0 1 0-14 0"
            }
            Accessibility => "M10 4a2 2 0 1 0 4 0 2 2 0 1 0-4 0M5 8l7 2 7-2M12 10v5M9 21l3-6 3 6",
            Language => "M3 5h10M8 3v2M5 5c1 4 4 7 7 8M11 5c-1 4-4 7-7 8M13 21l4-10 4 10M14.5 17h5",
            Desktops => "M3 4h8v7H3zM13 4h8v7h-8zM3 13h8v7H3zM13 13h8v7h-8z",
            Windows => "M3 5h12v10H3zM9 9h12v10H9z",
            Keyboard => "M3 6h18v12H3zM7 10h0M11 10h0M15 10h0M8 14h8",
            Mouse => "M12 3a6 6 0 0 0-6 6v6a6 6 0 0 0 12 0V9a6 6 0 0 0-6-6zM12 7v4",
            Captures => "M4 8V5h3M17 5h3v3M20 16v3h-3M7 19H4v-3M9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0",
            Clipboard => "M9 4h6v3H9zM7 5H5v16h14V5h-2M9 12h6M9 16h4",
            Screens => "M2 4h13v9H2zM17 7h5v10h-5zM6 17h5M8.5 13v4",
            Sound => "M4 10v4h4l5 4V6L8 10zM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11",
            Network => "M2 9a15 15 0 0 1 20 0M5 13a10 10 0 0 1 14 0M8.5 16.5a5 5 0 0 1 7 0M12 20h0",
            Bluetooth => "M7 7l10 10-5 5V2l5 5L7 17",
            Power => "M12 3v8M6.3 6.3a8 8 0 1 0 11.4 0",
            Lock => "M6 11h12v10H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3",
            Notifications => "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0",
            Apps => "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 17h6M17 14v6",
            Agents => {
                "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z"
            }
            Search => "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
            Accounts => "M4 6h16v14H4zM4 10h16M8 3v5M16 3v5",
            Account => "M8 8a4 4 0 1 0 8 0 4 4 0 1 0-8 0M4 21c1-4 4-6 8-6s7 2 8 6",
        }
    }

    /// What its tile says under its name: the values in effect.
    pub fn summary(self, ctx: &Context, lang: Lang) -> String {
        let a = &ctx.config.appearance;
        match self {
            Self::Appearance => format!(
                "{} · {} · Manrope",
                words::value_name(&a.theme, lang),
                words::value_name(&a.accent, lang)
            ),
            Self::Keyboard => {
                let layouts: Vec<String> = ctx
                    .config
                    .keyboard
                    .layouts
                    .iter()
                    .map(|l| l.to_uppercase())
                    .collect();
                if layouts.is_empty() {
                    "US".into()
                } else {
                    layouts.join(" · ")
                }
            }
            Self::Screens => {
                let n = ctx.outputs.len();
                match lang {
                    Lang::En if n == 1 => "1 screen".into(),
                    Lang::En => format!("{n} screens"),
                    Lang::Fr if n <= 1 => format!("{n} écran"),
                    Lang::Fr => format!("{n} écrans"),
                }
            }
            _ => words::NOT_YET.get(lang).into(),
        }
    }
}

/// What the settings show: the configuration in effect and the screens.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub config: Config,
    pub outputs: Vec<OutputInfo>,
}

/// A setting a search can lead to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Setting {
    Theme,
    Accent,
    TextSize,
    ReducedMotion,
    HighContrast,
    Transparency,
    ThemePerDesktop,
    Layouts,
    /// A shortcut, by its combination.
    Shortcut(String),
    /// A screen, by its name.
    Screen(String),
}

impl Setting {
    pub fn page(&self) -> Page {
        match self {
            Self::Theme
            | Self::Accent
            | Self::TextSize
            | Self::ReducedMotion
            | Self::HighContrast
            | Self::Transparency
            | Self::ThemePerDesktop => Page::Appearance,
            Self::Layouts | Self::Shortcut(_) => Page::Keyboard,
            Self::Screen(_) => Page::Screens,
        }
    }
}

/// A search result: a setting on its page, or a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub page: Page,
    pub setting: Option<Setting>,
    pub label: String,
    /// Its value in effect, or where it is.
    pub detail: String,
    score: i32,
}

/// Every setting with what it is found by.
fn entries(ctx: &Context, lang: Lang) -> Vec<(Setting, String, Vec<String>, String)> {
    let a = &ctx.config.appearance;
    let on_off = |b: bool| if b { words::ON } else { words::OFF }.get(lang).to_owned();
    let both = |w: Words| vec![w.en.to_owned(), w.fr.to_owned()];
    let mut out = vec![
        (
            Setting::Theme,
            words::THEME.get(lang).to_owned(),
            [
                both(words::THEME),
                vec![
                    "dark".into(),
                    "light".into(),
                    "clair".into(),
                    "sombre".into(),
                    "mode".into(),
                ],
            ]
            .concat(),
            words::value_name(&a.theme, lang),
        ),
        (
            Setting::Accent,
            words::ACCENT.get(lang).to_owned(),
            [
                both(words::ACCENT),
                vec!["colour".into(), "color".into(), "couleur".into()],
            ]
            .concat(),
            words::value_name(&a.accent, lang),
        ),
        (
            Setting::TextSize,
            words::TEXT_SIZE.get(lang).to_owned(),
            [
                both(words::TEXT_SIZE),
                vec!["font".into(), "police".into(), "zoom".into()],
            ]
            .concat(),
            words::percent(a.text_size, lang),
        ),
        (
            Setting::ReducedMotion,
            words::REDUCED_MOTION.get(lang).to_owned(),
            [
                both(words::REDUCED_MOTION),
                vec!["animation".into(), "mouvement".into()],
            ]
            .concat(),
            on_off(a.reduced_motion),
        ),
        (
            Setting::HighContrast,
            words::HIGH_CONTRAST.get(lang).to_owned(),
            [
                both(words::HIGH_CONTRAST),
                vec![
                    "contrast".into(),
                    "contraste".into(),
                    "accessibility".into(),
                    "accessibilité".into(),
                ],
            ]
            .concat(),
            on_off(a.high_contrast),
        ),
        (
            Setting::Transparency,
            words::TRANSPARENCY.get(lang).to_owned(),
            [
                both(words::TRANSPARENCY),
                vec!["blur".into(), "flou".into(), "glass".into()],
            ]
            .concat(),
            on_off(a.transparency),
        ),
        (
            Setting::ThemePerDesktop,
            words::THEME_PER_DESKTOP.get(lang).to_owned(),
            both(words::THEME_PER_DESKTOP),
            words::NOT_YET.get(lang).to_owned(),
        ),
        (
            Setting::Layouts,
            words::LAYOUTS.get(lang).to_owned(),
            [
                both(words::LAYOUTS),
                vec![
                    "layout".into(),
                    "azerty".into(),
                    "qwerty".into(),
                    "keyboard".into(),
                    "clavier".into(),
                ],
            ]
            .concat(),
            ctx.config.keyboard.layouts.join(", ").to_uppercase(),
        ),
    ];
    for (combo, action) in &ctx.config.shortcuts().bindings {
        let combo = combo.to_string();
        let (label, _, _) = words::action(action, lang);
        out.push((
            Setting::Shortcut(combo.clone()),
            label,
            vec![combo.clone(), action.to_string()],
            combo,
        ));
    }
    for output in &ctx.outputs {
        out.push((
            Setting::Screen(output.name.clone()),
            output.name.clone(),
            vec![output.name.clone()],
            format!("{} × {}", output.pixel_width, output.pixel_height),
        ));
    }
    out
}

/// What `query` finds, the best first: a setting's own name beats its
/// synonyms, which beat its value; pages are found by name too.
pub fn search(query: &str, ctx: &Context, lang: Lang) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (setting, label, synonyms, value) in entries(ctx, lang) {
        let by_name = fuzzy::score(query, &label);
        let by_synonym = synonyms.iter().filter_map(|s| fuzzy::score(query, s)).max();
        let by_value = fuzzy::score(query, &value);
        // Letters scattered in order are too loose for settings: the words
        // must be there.
        let strong = |s: Option<i32>| s.filter(|s| *s >= 2_000);
        let score = [
            strong(by_name).map(|s| s + 3_000),
            strong(by_synonym).map(|s| s + 1_500),
            strong(by_value),
        ]
        .into_iter()
        .flatten()
        .max();
        if let Some(score) = score {
            let page = setting.page();
            let detail = format!("{} · {}", page.label().get(lang), value);
            hits.push(Hit {
                page,
                setting: Some(setting),
                label,
                detail,
                score,
            });
        }
    }
    for page in Page::ALL {
        let names = [page.label().en, page.label().fr];
        let score = names
            .iter()
            .copied()
            .chain(page.synonyms().iter().copied())
            .filter_map(|n| fuzzy::score(query, n))
            .max();
        if let Some(score) = score.filter(|s| *s >= 2_000) {
            hits.push(Hit {
                page,
                setting: None,
                label: page.label().get(lang).to_owned(),
                detail: page.family().label().get(lang).to_owned(),
                score: score + 1_000,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.page.available().cmp(&a.page.available()))
    });
    hits.truncate(12);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context::default()
    }

    #[test]
    fn contrast_finds_the_high_contrast_switch_first() {
        for (query, lang) in [("contraste", Lang::Fr), ("contrast", Lang::En)] {
            let hits = search(query, &ctx(), lang);
            assert_eq!(
                hits.first().and_then(|h| h.setting.clone()),
                Some(Setting::HighContrast),
                "{query}: {hits:?}"
            );
            assert_eq!(hits[0].page, Page::Appearance);
        }
    }

    #[test]
    fn values_and_shortcuts_are_found() {
        let hits = search("Super+A", &ctx(), Lang::En);
        assert!(
            matches!(&hits[0].setting, Some(Setting::Shortcut(c)) if c == "Super+A"),
            "{hits:?}"
        );
        let hits = search("dark", &ctx(), Lang::En);
        assert_eq!(hits[0].setting, Some(Setting::Theme), "{hits:?}");
        let hits = search("luminosité", &ctx(), Lang::Fr);
        assert!(hits.iter().all(|h| h.setting != Some(Setting::Theme)));
    }

    #[test]
    fn pages_are_found_by_name_in_either_language() {
        let hits = search("écrans", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Screens, "{hits:?}");
        let hits = search("wallpaper", &ctx(), Lang::En);
        assert_eq!(hits[0].page, Page::Wallpaper, "{hits:?}");
    }

    #[test]
    fn every_family_has_six_pages_and_slugs_are_unique() {
        for family in Family::ALL {
            assert_eq!(family.pages().count(), 6, "{family:?}");
        }
        for page in Page::ALL {
            assert_eq!(Page::from_slug(page.slug()), Some(page));
        }
    }
}
