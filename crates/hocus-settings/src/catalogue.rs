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
        // The welcome and About are reached on their own (the first
        // session, the palette, the search), not from the lists: six pages
        // a family, as board R1 draws them.
        Page::ALL
            .into_iter()
            .filter(move |p| p.family() == self && !matches!(p, Page::Welcome | Page::About))
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
    About,
    /// The first session's (#82): a few choices, and where to go next.
    Welcome,
}

impl Page {
    pub const ALL: [Page; 26] = [
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
        Self::About,
        Self::Welcome,
    ];

    pub fn family(self) -> Family {
        use Page::*;
        match self {
            Appearance | Wallpaper | ColumnDock | Pandas | Accessibility | Language => {
                Family::Personalise
            }
            Desktops | Windows | Keyboard | Mouse | Captures | Clipboard => Family::Work,
            Screens | Sound | Network | Bluetooth | Power | Lock => Family::Hardware,
            Notifications | Apps | Agents | Search | Accounts | Account | About | Welcome => {
                Family::System
            }
        }
    }

    /// Whether the page is built.
    pub fn available(self) -> bool {
        matches!(
            self,
            Self::Appearance
                | Self::Wallpaper
                | Self::ColumnDock
                | Self::Pandas
                | Self::Accessibility
                | Self::Keyboard
                | Self::Screens
                | Self::Account
                | Self::Accounts
                | Self::Desktops
                | Self::Windows
                | Self::Mouse
                | Self::Captures
                | Self::Clipboard
                | Self::Sound
                | Self::Network
                | Self::Bluetooth
                | Self::Power
                | Self::Lock
                | Self::Language
                | Self::About
                | Self::Welcome
        )
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
            About => "about",
            Welcome => "welcome",
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
            About => Words::new("About", "À propos"),
            Welcome => Words::new("Welcome", "Bienvenue"),
        }
    }

    /// Other words people look for it with.
    fn synonyms(self) -> &'static [&'static str] {
        use Page::*;
        match self {
            Appearance => &[
                "look", "colour", "color", "couleur", "dark", "sombre", "style",
            ],
            Wallpaper => &[
                "background",
                "fond",
                "image",
                "picture",
                "stars",
                "étoiles",
                "rain",
                "pluie",
                "glow",
                "lueur",
            ],
            ColumnDock => &["column", "colonne", "dock", "panel", "panneau", "widgets"],
            Pandas => &["panda", "mascot", "mascotte"],
            Accessibility => &[
                "a11y",
                "magnifier",
                "loupe",
                "zoom",
                "contrast",
                "contraste",
                "motion",
                "mouvement",
            ],
            Language => &[
                "langue",
                "region",
                "région",
                "clock",
                "horloge",
                "time zone",
                "fuseau",
                "date",
                "heure",
            ],
            Screens => &[
                "display",
                "monitor",
                "moniteur",
                "affichage",
                "resolution",
                "résolution",
            ],
            Keyboard => &["keys", "touches", "layout", "azerty", "qwerty", "raccourci"],
            Desktops => &[
                "workspace",
                "espace de travail",
                "virtual",
                "virtuel",
                "switch desktop",
            ],
            Windows => &[
                "window", "fenêtre", "snap", "aimanter", "gaps", "espaces", "wobbly", "gelée",
                "rules", "règles", "effects", "effets",
            ],
            Mouse => &[
                "touchpad",
                "trackpad",
                "pavé tactile",
                "scroll",
                "défilement",
                "gesture",
                "geste",
                "corner",
                "coin",
                "pointer",
                "pointeur",
            ],
            Captures => &[
                "screenshot",
                "capture d’écran",
                "capture d'écran",
                "record",
                "enregistrer",
                "video",
                "vidéo",
                "screencast",
            ],
            Clipboard => &[
                "copy", "copier", "paste", "coller", "emoji", "émoji", "smiley",
            ],
            Sound => &[
                "audio",
                "volume",
                "speaker",
                "haut-parleur",
                "microphone",
                "micro",
                "mute",
                "muet",
            ],
            Power => &[
                "battery", "batterie", "sleep", "veille", "suspend", "charger", "chargeur",
                "énergie",
            ],
            Network => &[
                "wifi", "wi-fi", "internet", "wireless", "sans fil", "airplane", "avion",
            ],
            Bluetooth => &["headphones", "casque", "pairing", "appairer"],
            Lock => &[
                "lock screen",
                "écran de verrouillage",
                "verrouiller",
                "curtain",
                "rideau",
                "password",
                "mot de passe",
            ],
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
            About => "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7.5h0",
            Welcome => "M12 3l2.4 5.8 6.1.6-4.6 4 1.4 6.1L12 16.3 6.7 19.5l1.4-6.1-4.6-4 6.1-.6z",
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
            Self::About => format!("Hocuspanda {}", env!("CARGO_PKG_VERSION")),
            Self::Language => format!(
                "{} · {}",
                interface_name(ctx, lang),
                words::clock_short(ctx.config.clock.format)
            ),
            Self::Wallpaper => words::wallpaper_name(&ctx.config.wallpaper.kind, lang),
            Self::ColumnDock => {
                let dock = &ctx.config.dock;
                if dock.enabled {
                    format!("{} · {}", words::DOCK.get(lang), dock.pinned.len())
                } else {
                    format!("{} · {}", words::DOCK.get(lang), words::OFF.get(lang))
                }
            }
            Self::Pandas => on_off(ctx.config.pandas.enabled, lang),
            Self::Desktops => {
                let counts: Vec<String> =
                    ctx.screens.iter().map(|s| s.desktops.to_string()).collect();
                match counts.as_slice() {
                    [] => words::desktops(ctx.config.desktops.per_output, lang),
                    [_] => ctx
                        .screens
                        .first()
                        .map(|s| words::desktops(s.desktops, lang))
                        .unwrap_or_default(),
                    _ => format!("{} · {}", words::DESKTOPS.get(lang), counts.join(" · ")),
                }
            }
            Self::Windows => {
                let w = &ctx.config.windows;
                let mut said = Vec::new();
                if w.snap {
                    said.push(words::SNAPPING.get(lang).to_owned());
                }
                said.push(format!(
                    "{} {}",
                    words::GAPS.get(lang),
                    words::pixels(w.gaps.inner)
                ));
                if w.wobbly {
                    said.push(words::WOBBLY.get(lang).to_owned());
                }
                said.join(" · ")
            }
            Self::Mouse => {
                let t = &ctx.config.touchpad;
                let mut said = vec![if t.natural_scroll {
                    words::NATURAL_SCROLL.get(lang)
                } else {
                    words::CLASSIC_SCROLL.get(lang)
                }];
                if t.tap_to_click {
                    said.push(words::TAP_TO_CLICK.get(lang));
                }
                said.join(" · ")
            }
            Self::Captures => {
                let s = &ctx.config.screenshots;
                let shots = match (s.save, s.copy) {
                    (true, true) => words::SAVED_AND_COPIED,
                    (true, false) => words::SAVED,
                    (false, true) => words::COPIED,
                    (false, false) => words::NEITHER,
                };
                format!("{} · {} fps", shots.get(lang), ctx.config.recording.fps)
            }
            Self::Clipboard => {
                let c = &ctx.config.clipboard;
                if c.history {
                    words::copies(c.entries, lang)
                } else {
                    words::OFF.get(lang).to_owned()
                }
            }
            Self::Sound => {
                let h = &ctx.hardware;
                match (h.known, h.muted) {
                    (false, _) => words::UNKNOWN.get(lang).to_owned(),
                    (true, true) => words::MUTED.get(lang).to_owned(),
                    (true, false) => words::percent(u32::from(h.volume), lang),
                }
            }
            Self::Network => {
                let h = &ctx.hardware;
                if !h.known {
                    words::UNKNOWN.get(lang).to_owned()
                } else if h.airplane {
                    words::AIRPLANE.get(lang).to_owned()
                } else if !h.wifi {
                    format!("Wi-Fi · {}", words::OFF.get(lang))
                } else {
                    match &h.network {
                        Some(name) => format!("Wi-Fi · {name}"),
                        None => format!("Wi-Fi · {}", words::NOT_CONNECTED.get(lang)),
                    }
                }
            }
            Self::Bluetooth => {
                let h = &ctx.hardware;
                match (h.known, h.bluetooth, &h.bluetooth_device) {
                    (false, _, _) => words::UNKNOWN.get(lang).to_owned(),
                    (true, true, Some(device)) => device.clone(),
                    (true, on, _) => on_off(on, lang),
                }
            }
            Self::Power => {
                let profile = words::profile(&ctx.hardware.profile).get(lang);
                match &ctx.hardware.battery {
                    Some(b) => {
                        format!("{} · {profile}", words::percent(u32::from(b.percent), lang))
                    }
                    None if ctx.hardware.known => profile.to_owned(),
                    None => words::UNKNOWN.get(lang).to_owned(),
                }
            }
            Self::Lock => {
                let i = &ctx.config.idle;
                if !i.lock || i.curtain_after == 0 {
                    words::NOT_ON_IDLE.get(lang).to_owned()
                } else {
                    words::locks_after(i.curtain_after + i.lock_after, lang)
                }
            }
            Self::Accessibility => {
                let a = &ctx.config.appearance;
                let mut said = vec![words::percent(a.text_size, lang)];
                if a.high_contrast {
                    said.push(words::HIGH_CONTRAST.get(lang).to_owned());
                }
                if a.reduced_motion {
                    said.push(words::REDUCED_MOTION.get(lang).to_owned());
                }
                said.join(" · ")
            }
            _ => words::NOT_YET.get(lang).into(),
        }
    }
}

fn on_off(on: bool, lang: Lang) -> String {
    if on { words::ON } else { words::OFF }.get(lang).to_owned()
}

/// The keys of an action, or that it has none.
fn keys_of(ctx: &Context, action: &hocus_core::Action, lang: Lang) -> String {
    ctx.config
        .shortcuts()
        .bindings
        .iter()
        .find(|(_, a)| *a == action)
        .map_or_else(
            || words::NO_KEYS.get(lang).to_owned(),
            |(combo, _)| combo.to_string(),
        )
}

/// The magnifier's keys: zoom in, then out.
pub fn zoom_keys(ctx: &Context, lang: Lang) -> String {
    use hocus_core::Action;
    let shortcuts = ctx.config.shortcuts();
    let keys: Vec<String> = [Action::ZoomIn, Action::ZoomOut]
        .iter()
        .filter_map(|action| {
            shortcuts
                .bindings
                .iter()
                .find(|(_, a)| *a == action)
                .map(|(combo, _)| combo.to_string())
        })
        .collect();
    if keys.is_empty() {
        words::NO_KEYS.get(lang).to_owned()
    } else {
        keys.join(", ")
    }
}

/// The interface's language as set.
fn interface_name(ctx: &Context, lang: Lang) -> String {
    match ctx.config.language.interface {
        hocus_core::config::InterfaceLanguage::Auto => words::LIKE_THE_SYSTEM,
        hocus_core::config::InterfaceLanguage::En => words::ENGLISH,
        hocus_core::config::InterfaceLanguage::Fr => words::FRENCH,
    }
    .get(lang)
    .to_owned()
}

/// The system's time zone, as timedated says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SystemZone {
    /// Not heard yet.
    #[default]
    Unknown,
    Known(String),
    /// No timedated to ask, and why: the zone is not changed here.
    Absent(String),
}

impl SystemZone {
    /// Its name, empty when unknown.
    pub fn name(&self) -> &str {
        match self {
            Self::Known(name) => name,
            Self::Unknown | Self::Absent(_) => "",
        }
    }
}

/// What the settings show: the configuration in effect and the screens.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub config: Config,
    /// The system's time zone, for Language and region.
    pub zone: SystemZone,
    pub outputs: Vec<OutputInfo>,
    /// Every connected screen, on or off, for the Screens page.
    pub screens: Vec<hocus_api::ScreenInfo>,
    /// System updates, for the Account page.
    pub updates: hocus_api::UpdatesInfo,
    /// Calendar accounts, for the Accounts page.
    pub calendars: hocus_api::CalendarsInfo,
    /// The sound, network, Bluetooth and power, for the Hardware pages.
    pub hardware: hocus_api::HardwareInfo,
    /// The account's name.
    pub user: String,
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
    HotCorners,
    DesktopCount,
    DesktopNames,
    Snap,
    Gaps,
    Wobbly,
    TitleBars,
    Rules,
    NaturalScroll,
    TapToClick,
    Gestures,
    Screenshots,
    Recording,
    ClipboardHistory,
    Emoji,
    InterfaceLanguage,
    Clock,
    TimeZone,
    Dock,
    Pandas,
    Magnifier,
    Volume,
    Wifi,
    Airplane,
    Bluetooth,
    PowerProfile,
    IdleDelays,
    SleepAction,
    FullscreenAwake,
    LockOnIdle,
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
            Self::DesktopCount | Self::DesktopNames => Page::Desktops,
            Self::Snap | Self::Gaps | Self::Wobbly | Self::TitleBars | Self::Rules => Page::Windows,
            Self::NaturalScroll | Self::TapToClick | Self::Gestures | Self::HotCorners => {
                Page::Mouse
            }
            Self::Screenshots | Self::Recording => Page::Captures,
            Self::ClipboardHistory | Self::Emoji => Page::Clipboard,
            Self::InterfaceLanguage | Self::Clock | Self::TimeZone => Page::Language,
            Self::Dock => Page::ColumnDock,
            Self::Pandas => Page::Pandas,
            Self::Magnifier => Page::Accessibility,
            Self::Volume => Page::Sound,
            Self::Wifi | Self::Airplane => Page::Network,
            Self::Bluetooth => Page::Bluetooth,
            Self::PowerProfile | Self::IdleDelays | Self::SleepAction | Self::FullscreenAwake => {
                Page::Power
            }
            Self::LockOnIdle => Page::Lock,
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
            Setting::InterfaceLanguage,
            words::INTERFACE_LANGUAGE.get(lang).to_owned(),
            [
                both(words::INTERFACE_LANGUAGE),
                vec![
                    "language".into(),
                    "langue".into(),
                    "french".into(),
                    "français".into(),
                    "english".into(),
                    "anglais".into(),
                ],
            ]
            .concat(),
            interface_name(ctx, lang),
        ),
        (
            Setting::Clock,
            words::CLOCK.get(lang).to_owned(),
            [
                both(words::CLOCK),
                vec![
                    "time".into(),
                    "heure".into(),
                    "12h".into(),
                    "24h".into(),
                    "am".into(),
                    "pm".into(),
                ],
            ]
            .concat(),
            words::clock_short(ctx.config.clock.format).to_owned(),
        ),
        (
            Setting::TimeZone,
            words::TIME_ZONE.get(lang).to_owned(),
            [
                both(words::TIME_ZONE),
                vec!["timezone".into(), "fuseau".into(), "utc".into()],
            ]
            .concat(),
            words::zone_name(ctx.zone.name(), lang),
        ),
        (
            Setting::Dock,
            words::DOCK.get(lang).to_owned(),
            [
                both(words::DOCK),
                both(words::PINNED),
                vec!["pinned".into(), "épinglé".into()],
            ]
            .concat(),
            on_off(ctx.config.dock.enabled),
        ),
        (
            Setting::Pandas,
            words::SHOW_PANDAS.get(lang).to_owned(),
            [
                both(words::SHOW_PANDAS),
                vec!["panda".into(), "mascot".into()],
            ]
            .concat(),
            on_off(ctx.config.pandas.enabled),
        ),
        (
            Setting::Magnifier,
            words::MAGNIFIER.get(lang).to_owned(),
            [
                both(words::MAGNIFIER),
                vec!["zoom".into(), "magnify".into()],
            ]
            .concat(),
            zoom_keys(ctx, lang),
        ),
        (
            Setting::HotCorners,
            words::HOT_CORNERS.get(lang).to_owned(),
            [
                both(words::HOT_CORNERS),
                vec![
                    "corner".into(),
                    "coin".into(),
                    "overview".into(),
                    "show desktop".into(),
                ],
            ]
            .concat(),
            {
                let c = &ctx.config.hot_corners;
                let set = [c.top_left, c.top_right, c.bottom_left, c.bottom_right]
                    .into_iter()
                    .filter(|t| *t != hocus_core::config::Trigger::None)
                    .count();
                if set == 0 {
                    words::OFF.get(lang).to_owned()
                } else {
                    format!("{set}/4")
                }
            },
        ),
        (
            Setting::DesktopCount,
            words::DESKTOP_COUNT.get(lang).to_owned(),
            [
                both(words::DESKTOP_COUNT),
                both(words::DESKTOPS),
                vec!["workspaces".into(), "how many".into(), "combien".into()],
            ]
            .concat(),
            Page::Desktops.summary(ctx, lang),
        ),
        (
            Setting::DesktopNames,
            words::DESKTOP_NAMES.get(lang).to_owned(),
            [
                both(words::DESKTOP_NAMES),
                vec![
                    "name".into(),
                    "nom".into(),
                    "rename".into(),
                    "renommer".into(),
                ],
            ]
            .concat(),
            {
                let names: Vec<&str> = ctx
                    .config
                    .outputs
                    .iter()
                    .flat_map(|o| o.desktop_names.iter())
                    .map(String::as_str)
                    .filter(|n| !n.is_empty())
                    .collect();
                names.join(", ")
            },
        ),
        (
            Setting::Snap,
            words::SNAP_EDGES.get(lang).to_owned(),
            [
                both(words::SNAP_EDGES),
                both(words::SNAPPING),
                vec![
                    "snap".into(),
                    "aimanter".into(),
                    "halves".into(),
                    "moitiés".into(),
                    "quarters".into(),
                    "quarts".into(),
                ],
            ]
            .concat(),
            on_off(ctx.config.windows.snap),
        ),
        (
            Setting::Gaps,
            words::GAPS.get(lang).to_owned(),
            [
                both(words::GAPS),
                vec![
                    "gap".into(),
                    "margin".into(),
                    "marge".into(),
                    "espace".into(),
                ],
            ]
            .concat(),
            words::pixels(ctx.config.windows.gaps.inner),
        ),
        (
            Setting::Wobbly,
            words::WOBBLY.get(lang).to_owned(),
            [
                both(words::WOBBLY),
                vec![
                    "wobbly".into(),
                    "jelly".into(),
                    "gelée".into(),
                    "compiz".into(),
                    "effects".into(),
                    "effets".into(),
                ],
            ]
            .concat(),
            on_off(ctx.config.windows.wobbly),
        ),
        (
            Setting::TitleBars,
            words::TITLE_BARS.get(lang).to_owned(),
            [
                both(words::TITLE_BARS),
                vec![
                    "title".into(),
                    "titre".into(),
                    "decorations".into(),
                    "décorations".into(),
                ],
            ]
            .concat(),
            on_off(ctx.config.decorations.title_bars),
        ),
        (
            Setting::Rules,
            words::RULES.get(lang).to_owned(),
            [
                both(words::RULES),
                vec!["rule".into(), "règle".into(), "app_id".into()],
            ]
            .concat(),
            ctx.config.rules.len().to_string(),
        ),
        (
            Setting::NaturalScroll,
            words::NATURAL_SCROLL.get(lang).to_owned(),
            [
                both(words::NATURAL_SCROLL),
                vec![
                    "scroll".into(),
                    "défilement".into(),
                    "reverse".into(),
                    "inverser".into(),
                ],
            ]
            .concat(),
            on_off(ctx.config.touchpad.natural_scroll),
        ),
        (
            Setting::TapToClick,
            words::TAP_TO_CLICK.get(lang).to_owned(),
            [
                both(words::TAP_TO_CLICK),
                vec!["tap".into(), "taper".into(), "click".into(), "clic".into()],
            ]
            .concat(),
            on_off(ctx.config.touchpad.tap_to_click),
        ),
        (
            Setting::Gestures,
            words::GESTURES.get(lang).to_owned(),
            [
                both(words::GESTURES),
                vec![
                    "gesture".into(),
                    "geste".into(),
                    "fingers".into(),
                    "doigts".into(),
                    "swipe".into(),
                ],
            ]
            .concat(),
            words::trigger(ctx.config.gestures.three_up)
                .get(lang)
                .to_owned(),
        ),
        (
            Setting::Screenshots,
            words::SCREENSHOTS.get(lang).to_owned(),
            [
                both(words::SCREENSHOTS),
                vec![
                    "screenshot".into(),
                    "capture".into(),
                    "print".into(),
                    "impr".into(),
                ],
            ]
            .concat(),
            Page::Captures.summary(ctx, lang),
        ),
        (
            Setting::Recording,
            words::RECORDING.get(lang).to_owned(),
            [
                both(words::RECORDING),
                vec![
                    "record".into(),
                    "screencast".into(),
                    "video".into(),
                    "vidéo".into(),
                    "micro".into(),
                    "webcam".into(),
                ],
            ]
            .concat(),
            format!("{} fps", ctx.config.recording.fps),
        ),
        (
            Setting::ClipboardHistory,
            words::CLIPBOARD_HISTORY.get(lang).to_owned(),
            [
                both(words::CLIPBOARD_HISTORY),
                vec![
                    "clipboard".into(),
                    "presse-papiers".into(),
                    "copy".into(),
                    "paste".into(),
                    "coller".into(),
                ],
            ]
            .concat(),
            Page::Clipboard.summary(ctx, lang),
        ),
        (
            Setting::Emoji,
            words::EMOJI.get(lang).to_owned(),
            [
                both(words::EMOJI),
                vec!["emoji".into(), "smiley".into(), "émoticône".into()],
            ]
            .concat(),
            keys_of(ctx, &hocus_core::Action::Emoji, lang),
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
    out.extend(hardware_entries(ctx, lang));
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

/// The Hardware pages' settings.
fn hardware_entries(ctx: &Context, lang: Lang) -> Vec<(Setting, String, Vec<String>, String)> {
    let h = &ctx.hardware;
    let i = &ctx.config.idle;
    let both = |w: Words| vec![w.en.to_owned(), w.fr.to_owned()];
    let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    vec![
        (
            Setting::Volume,
            words::VOLUME.get(lang).to_owned(),
            [
                both(words::VOLUME),
                both(words::MUTE),
                words(&["sound", "son", "speaker", "haut-parleur", "audio"]),
            ]
            .concat(),
            Page::Sound.summary(ctx, lang),
        ),
        (
            Setting::Wifi,
            "Wi-Fi".to_owned(),
            words(&["wifi", "wireless", "sans fil", "internet", "réseau"]),
            Page::Network.summary(ctx, lang),
        ),
        (
            Setting::Airplane,
            words::AIRPLANE.get(lang).to_owned(),
            [both(words::AIRPLANE), words(&["flight", "vol", "radio"])].concat(),
            on_off(h.airplane, lang),
        ),
        (
            Setting::Bluetooth,
            "Bluetooth".to_owned(),
            words(&["bluetooth", "headphones", "casque", "écouteurs"]),
            Page::Bluetooth.summary(ctx, lang),
        ),
        (
            Setting::PowerProfile,
            words::POWER_PROFILE.get(lang).to_owned(),
            [
                both(words::POWER_PROFILE),
                words(&[
                    "profile",
                    "profil",
                    "performance",
                    "saver",
                    "économie",
                    "balanced",
                    "équilibré",
                ]),
            ]
            .concat(),
            words::profile(&h.profile).get(lang).to_owned(),
        ),
        (
            Setting::IdleDelays,
            words::DELAYS.get(lang).to_owned(),
            [
                both(words::DELAYS),
                both(words::DIM_AFTER),
                both(words::ON_BATTERY),
                words(&[
                    "delay",
                    "délai",
                    "dim",
                    "assombrir",
                    "idle",
                    "inactivité",
                    "battery",
                    "batterie",
                ]),
            ]
            .concat(),
            words::duration(i.curtain_after, lang),
        ),
        (
            Setting::SleepAction,
            words::SLEEP_HOW.get(lang).to_owned(),
            [
                both(words::SLEEP_HOW),
                both(words::SLEEP_AFTER),
                words(&["suspend", "hibernate", "hiberner", "veille", "sleep"]),
            ]
            .concat(),
            words::sleep_name(i.sleep).get(lang).to_owned(),
        ),
        (
            Setting::FullscreenAwake,
            words::FULLSCREEN_AWAKE.get(lang).to_owned(),
            [
                both(words::FULLSCREEN_AWAKE),
                words(&[
                    "fullscreen",
                    "plein écran",
                    "video",
                    "vidéo",
                    "film",
                    "awake",
                ]),
            ]
            .concat(),
            on_off(i.fullscreen_awake, lang),
        ),
        (
            Setting::LockOnIdle,
            words::LOCK_ON_IDLE.get(lang).to_owned(),
            [
                both(words::LOCK_ON_IDLE),
                words(&["lock", "verrouiller", "curtain", "rideau", "screen saver"]),
            ]
            .concat(),
            Page::Lock.summary(ctx, lang),
        ),
    ]
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
    fn the_clock_and_time_zone_are_found_in_either_language() {
        let hits = search("fuseau", &ctx(), Lang::Fr);
        assert_eq!(hits[0].setting, Some(Setting::TimeZone), "{hits:?}");
        let hits = search("12h", &ctx(), Lang::En);
        assert_eq!(hits[0].setting, Some(Setting::Clock), "{hits:?}");
        let hits = search("loupe", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Accessibility, "{hits:?}");
    }

    #[test]
    fn the_work_settings_are_found_in_either_language() {
        let hits = search("wobbly", &ctx(), Lang::En);
        assert_eq!(hits[0].setting, Some(Setting::Wobbly), "{hits:?}");
        let hits = search("pavé tactile", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Mouse, "{hits:?}");
        let hits = search("défilement naturel", &ctx(), Lang::Fr);
        assert_eq!(hits[0].setting, Some(Setting::NaturalScroll), "{hits:?}");
        let hits = search("coins actifs", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Mouse, "{hits:?}");
        let hits = search("emoji", &ctx(), Lang::En);
        assert_eq!(hits[0].page, Page::Clipboard, "{hits:?}");
        let hits = search("règles", &ctx(), Lang::Fr);
        assert_eq!(hits[0].setting, Some(Setting::Rules), "{hits:?}");
        for page in Family::Work.pages() {
            assert!(page.available(), "{page:?}");
        }
    }

    #[test]
    fn the_hardware_settings_are_found_in_either_language() {
        let hits = search("veille", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Power, "{hits:?}");
        let hits = search("mode avion", &ctx(), Lang::Fr);
        assert_eq!(hits[0].setting, Some(Setting::Airplane), "{hits:?}");
        let hits = search("volume", &ctx(), Lang::En);
        assert_eq!(hits[0].setting, Some(Setting::Volume), "{hits:?}");
        let hits = search("maintient éveillé", &ctx(), Lang::Fr);
        assert_eq!(hits[0].setting, Some(Setting::FullscreenAwake), "{hits:?}");
        let hits = search("hibernate", &ctx(), Lang::En);
        assert_eq!(hits[0].setting, Some(Setting::SleepAction), "{hits:?}");
        let hits = search("verrouillage", &ctx(), Lang::Fr);
        assert_eq!(hits[0].page, Page::Lock, "{hits:?}");
        for page in Family::Hardware.pages() {
            assert!(page.available(), "{page:?}");
        }
    }

    #[test]
    fn the_hardware_tiles_say_what_is_in_effect() {
        let mut c = ctx();
        assert_eq!(Page::Sound.summary(&c, Lang::En), "Unknown");
        c.hardware = hocus_api::HardwareInfo {
            known: true,
            wifi: true,
            network: Some("Bamboo grove".into()),
            volume: 40,
            profile: "balanced".into(),
            battery: Some(hocus_api::BatteryInfo {
                percent: 80,
                minutes_left: None,
                state: "charging".into(),
            }),
            ..Default::default()
        };
        assert_eq!(Page::Sound.summary(&c, Lang::En), "40%");
        assert_eq!(Page::Network.summary(&c, Lang::En), "Wi-Fi · Bamboo grove");
        assert_eq!(Page::Power.summary(&c, Lang::Fr), "80\u{202f}% · Équilibré");
        assert_eq!(Page::Lock.summary(&c, Lang::En), "Locks after 5 min 30 s");
        c.config.idle.lock = false;
        assert_eq!(Page::Lock.summary(&c, Lang::En), "Not when idle");
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
