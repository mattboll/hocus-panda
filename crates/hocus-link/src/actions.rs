//! The actions palette (#736, docs/design/actions.md): `Super+P` lists
//! every action the desktop can run, searchable, and runs the one chosen
//! on the window that had the keyboard, or its screen.
//!
//! [`catalogue`] is the one list of those actions, which the compositor,
//! the shell and the Settings share: the shortcuts' actions (`[shortcuts]`,
//! the families of nine folded into one action asking for a desktop), the
//! radial menu's, the bar's, the agents', accessibility's, the session's
//! and each page of the Settings. Each says its names in both languages,
//! its keywords, its category, whether it acts on a window, whether it is a
//! switch, what it asks for before it runs, its shortcut's key, and who
//! runs it. The compositor and the Settings check in their tests that each
//! of their shortcuts and pages is listed, so that a new one shows here.
//!
//! - To the shell: `actions OUTPUT SCALE WINDOW X WIDTH\tON\tINERT\tSHORTCUTS\tTITLE`
//!   ([`ActionsShown`]): `Super+P` on the target's screen (its work area
//!   across, from `X`, `WIDTH` wide), what is on, what
//!   cannot run now and why, the shortcuts as the configuration binds them;
//!   open, it closes. `actions-end`: the target went, or the session locks.
//! - From the shell: `action ID ARG WINDOW` ([`RunAction`]): run action
//!   `ID` on window `WINDOW` (`-` none), with `ARG` (a desktop from 0, an
//!   agent's id; `-` none).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::text::tidy;

/// The palette's layer surface.
pub const ACTIONS_NAMESPACE: &str = "hocus-actions";
/// Its width, its top under the screen's top, its corners (logical px).
pub const ACTIONS_WIDTH: f64 = 720.0;
pub const ACTIONS_TOP: f64 = 64.0;
pub const ACTIONS_RADIUS: f64 = 16.0;
/// In over this, dropping [`ACTIONS_DROP`] and fading; out over
/// [`ACTIONS_CLOSE`].
pub const ACTIONS_OPEN: Duration = Duration::from_millis(140);
pub const ACTIONS_CLOSE: Duration = Duration::from_millis(100);
pub const ACTIONS_DROP: f64 = 8.0;
/// What dims the screen behind it: `rgba(6,8,14,.25)`.
pub const ACTIONS_DIM: ([u8; 3], f32) = ([6, 8, 14], 0.25);
/// The target window's ring: 2.5 px of `#7EE0A0`, and its glow, 4 px at
/// 25 %.
pub const ACTIONS_RING: [u8; 3] = [0x7E, 0xE0, 0xA0];
pub const ACTIONS_RING_WIDTH: f64 = 2.5;
pub const ACTIONS_GLOW_WIDTH: f64 = 4.0;
pub const ACTIONS_GLOW_ALPHA: f32 = 0.25;

/// The palette's categories, in the order the empty field lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Window,
    Desktops,
    Screens,
    Agents,
    Bar,
    Captures,
    Accessibility,
    Session,
    Settings,
}

impl Category {
    pub const ALL: [Self; 9] = [
        Self::Window,
        Self::Desktops,
        Self::Screens,
        Self::Agents,
        Self::Bar,
        Self::Captures,
        Self::Accessibility,
        Self::Session,
        Self::Settings,
    ];

    pub const fn name(self, fr: bool) -> &'static str {
        match (self, fr) {
            (Self::Window, true) => "Fenêtre",
            (Self::Window, false) => "Window",
            (Self::Desktops, true) => "Bureaux",
            (Self::Desktops, false) => "Desktops",
            (Self::Screens, true) => "Écrans",
            (Self::Screens, false) => "Screens",
            (Self::Agents, _) => "Agents",
            (Self::Bar, true) => "Barre",
            (Self::Bar, false) => "Bar",
            (Self::Captures, true) => "Captures",
            (Self::Captures, false) => "Screenshots",
            (Self::Accessibility, true) => "Accessibilité",
            (Self::Accessibility, false) => "Accessibility",
            (Self::Session, _) => "Session",
            (Self::Settings, true) => "Réglages",
            (Self::Settings, false) => "Settings",
        }
    }
}

/// What an action asks for before it runs, in a second step of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A desktop of the target's screen; a digit picks it at once.
    Desktop,
    /// An agent, in the order of the bar's rows.
    Agent,
}

/// Who runs an action: the compositor, told [`RunAction`], or the shell
/// itself (the bar's popovers, the Settings, the agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    Compositor,
    Shell,
}

/// The glyph a row shows in its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Close,
    Minimize,
    Maximize,
    Restore,
    HalfLeft,
    HalfRight,
    Quarter,
    Arrow(Side4),
    Switch,
    Send,
    Desktops,
    Grid,
    Screen,
    Above,
    Everywhere,
    Agent,
    Terminal,
    Search,
    Bar,
    Clipboard,
    Emoji,
    Meeting,
    Bell,
    Moon,
    Calendar,
    Task,
    Sound,
    Wifi,
    Brightness,
    Camera,
    Record,
    Magnifier,
    Speech,
    Contrast,
    Motion,
    Keyboard,
    Lock,
    LogOut,
    Restart,
    Power,
    Gear,
}

/// A direction, for the arrows' glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side4 {
    Left,
    Right,
    Up,
    Down,
}

/// An action of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Def {
    /// What the line names it by: its `[shortcuts]` key when it has one.
    pub id: &'static str,
    /// Its names; one ending in "…" asks for a [`Step`].
    pub fr: &'static str,
    pub en: &'static str,
    /// Other words it is found by, in both languages.
    pub keywords: &'static [&'static str],
    pub category: Category,
    /// It acts on the target window: inert without one.
    pub window: bool,
    /// On or off, its state shown instead of a shortcut.
    pub switch: bool,
    pub step: Option<Step>,
    /// Its `[shortcuts]` key, or a family's prefix (`send-to-desktop-`),
    /// whose shortcut shows with its digit as "1…9".
    pub shortcut: Option<&'static str>,
    pub glyph: Glyph,
    pub runs: Runs,
}

impl Def {
    pub fn name(&self, fr: bool) -> &'static str {
        if fr { self.fr } else { self.en }
    }

    /// Its name without its "…", for a step's target after it: "Envoyer
    /// au bureau".
    pub fn stem(&self, fr: bool) -> &'static str {
        self.name(fr).trim_end_matches('…').trim_end()
    }

    /// The Settings' page it opens, if it is one: its slug.
    pub fn settings_page(&self) -> Option<&'static str> {
        self.id.strip_prefix("settings-")
    }
}

const fn d(
    id: &'static str,
    fr: &'static str,
    en: &'static str,
    keywords: &'static [&'static str],
    category: Category,
    glyph: Glyph,
) -> Def {
    Def {
        id,
        fr,
        en,
        keywords,
        category,
        window: false,
        switch: false,
        step: None,
        shortcut: None,
        glyph,
        runs: Runs::Compositor,
    }
}

impl Def {
    const fn on_window(mut self) -> Self {
        self.window = true;
        self
    }

    const fn switch(mut self) -> Self {
        self.switch = true;
        self
    }

    const fn step(mut self, step: Step) -> Self {
        self.step = Some(step);
        self
    }

    const fn key(mut self, key: &'static str) -> Self {
        self.shortcut = Some(key);
        self
    }

    const fn shell(mut self) -> Self {
        self.runs = Runs::Shell;
        self
    }
}

use Category as C;
use Glyph as G;

/// A page of the Settings: its slug, its names. The Settings check that
/// this is their list of sections.
pub const SETTINGS_PAGES: [(&str, &str, &str); 19] = [
    (
        "appearance",
        "Apparence et langue",
        "Appearance and language",
    ),
    ("wallpaper", "Fond d’écran", "Wallpaper"),
    ("bar", "La barre", "The bar"),
    ("accessibility", "Accessibilité", "Accessibility"),
    ("agenda", "Agenda et tâches", "Calendar and tasks"),
    ("screens", "Écrans", "Screens"),
    ("sound", "Son", "Sound"),
    (
        "keyboard",
        "Clavier et raccourcis",
        "Keyboard and shortcuts",
    ),
    ("mouse", "Souris et pavé tactile", "Mouse and touchpad"),
    ("network", "Réseau", "Network"),
    ("bluetooth", "Bluetooth", "Bluetooth"),
    ("desktops", "Bureaux et fenêtres", "Desktops and windows"),
    ("applications", "Applications", "Applications"),
    ("agents", "Agents", "Agents"),
    (
        "captures",
        "Captures et presse-papiers",
        "Screenshots and clipboard",
    ),
    ("power", "Énergie", "Power"),
    ("account", "Compte", "Account"),
    ("date-and-time", "Date et heure", "Date and time"),
    ("performance", "Performances", "Performance"),
];

macro_rules! settings_pages {
    ($($slug:literal, $fr:literal, $en:literal;)*) => {
        [$(
            d(
                concat!("settings-", $slug),
                concat!("Réglages › ", $fr),
                concat!("Settings › ", $en),
                &["réglages", "paramètres", "settings", "preferences"],
                C::Settings,
                G::Gear,
            )
            .shell(),
        )*]
    };
}

/// The Settings' pages as actions, in [`SETTINGS_PAGES`]' order.
const PAGES: [Def; 19] = settings_pages! {
    "appearance", "Apparence et langue", "Appearance and language";
    "wallpaper", "Fond d’écran", "Wallpaper";
    "bar", "La barre", "The bar";
    "accessibility", "Accessibilité", "Accessibility";
    "agenda", "Agenda et tâches", "Calendar and tasks";
    "screens", "Écrans", "Screens";
    "sound", "Son", "Sound";
    "keyboard", "Clavier et raccourcis", "Keyboard and shortcuts";
    "mouse", "Souris et pavé tactile", "Mouse and touchpad";
    "network", "Réseau", "Network";
    "bluetooth", "Bluetooth", "Bluetooth";
    "desktops", "Bureaux et fenêtres", "Desktops and windows";
    "applications", "Applications", "Applications";
    "agents", "Agents", "Agents";
    "captures", "Captures et presse-papiers", "Screenshots and clipboard";
    "power", "Énergie", "Power";
    "account", "Compte", "Account";
    "date-and-time", "Date et heure", "Date and time";
    "performance", "Performances", "Performance";
};

const SNAP: &[&str] = &["ancrer", "côte à côte", "snap", "tile"];
const CORNER: &[&str] = &["ancrer", "coin", "snap", "corner"];
const FOCUS: &[&str] = &["focus", "aller"];
const WORKSPACE: &[&str] = &["espace de travail", "workspace"];
const MOVE_SCREEN: &[&str] = &["déplacer", "moniteur", "move", "monitor", "display"];

/// The actions, in each category's order.
const ACTIONS: [Def; 66] = [
    // The window.
    d(
        "close",
        "Fermer",
        "Close",
        &["fermer la fenêtre", "quitter", "close the window", "quit"],
        C::Window,
        G::Close,
    )
    .on_window()
    .key("close"),
    d(
        "minimize",
        "Réduire",
        "Minimise",
        &["cacher", "minimiser", "hide", "minimize"],
        C::Window,
        G::Minimize,
    )
    .on_window()
    .key("minimize"),
    d(
        "maximize",
        "Agrandir",
        "Maximise",
        &["plein écran", "maximiser", "maximize", "full"],
        C::Window,
        G::Maximize,
    )
    .on_window()
    .key("maximize"),
    d(
        "restore",
        "Restaurer",
        "Restore",
        &["réduire", "unmaximize"],
        C::Window,
        G::Restore,
    )
    .on_window()
    .key("restore"),
    d(
        "snap-left",
        "Moitié gauche",
        "Left half",
        SNAP,
        C::Window,
        G::HalfLeft,
    )
    .on_window()
    .key("snap-left"),
    d(
        "snap-right",
        "Moitié droite",
        "Right half",
        SNAP,
        C::Window,
        G::HalfRight,
    )
    .on_window()
    .key("snap-right"),
    d(
        "quarter-up",
        "Quart du haut",
        "Top quarter",
        CORNER,
        C::Window,
        G::Quarter,
    )
    .on_window()
    .key("quarter-up"),
    d(
        "quarter-down",
        "Quart du bas",
        "Bottom quarter",
        CORNER,
        C::Window,
        G::Quarter,
    )
    .on_window()
    .key("quarter-down"),
    d(
        "quarter-left",
        "Quart de gauche",
        "Left quarter",
        CORNER,
        C::Window,
        G::Quarter,
    )
    .on_window()
    .key("quarter-left"),
    d(
        "quarter-right",
        "Quart de droite",
        "Right quarter",
        CORNER,
        C::Window,
        G::Quarter,
    )
    .on_window()
    .key("quarter-right"),
    d(
        "above",
        "Toujours au premier plan",
        "Always on top",
        &["dessus", "épingler", "pin", "above"],
        C::Window,
        G::Above,
    )
    .on_window()
    .switch(),
    d(
        "all-desktops",
        "Sur tous les bureaux",
        "On all desktops",
        &["partout", "épingler", "sticky", "everywhere"],
        C::Window,
        G::Everywhere,
    )
    .on_window()
    .switch(),
    d(
        "send-to-desktop",
        "Envoyer au bureau…",
        "Send to desktop…",
        &["déplacer", "move", "espace de travail", "workspace"],
        C::Window,
        G::Send,
    )
    .on_window()
    .step(Step::Desktop)
    .key("send-to-desktop-"),
    d(
        "take-to-desktop",
        "Emmener au bureau…",
        "Take to desktop…",
        &["déplacer", "suivre", "move", "follow"],
        C::Window,
        G::Send,
    )
    .on_window()
    .step(Step::Desktop)
    .key("take-to-desktop-"),
    d(
        "previous-desktop",
        "Envoyer au bureau précédent",
        "Send to the previous desktop",
        &["gauche", "left"],
        C::Window,
        G::Arrow(Side4::Left),
    )
    .on_window(),
    d(
        "next-desktop",
        "Envoyer au bureau suivant",
        "Send to the next desktop",
        &["droite", "right"],
        C::Window,
        G::Arrow(Side4::Right),
    )
    .on_window(),
    d(
        "take-to-desktop-left",
        "Emmener au bureau de gauche",
        "Take to the desktop on the left",
        &["déplacer", "move"],
        C::Window,
        G::Arrow(Side4::Left),
    )
    .on_window()
    .key("take-to-desktop-left"),
    d(
        "take-to-desktop-right",
        "Emmener au bureau de droite",
        "Take to the desktop on the right",
        &["déplacer", "move"],
        C::Window,
        G::Arrow(Side4::Right),
    )
    .on_window()
    .key("take-to-desktop-right"),
    d(
        "move-to-screen-left",
        "Envoyer sur l’écran de gauche",
        "Move to the screen on the left",
        MOVE_SCREEN,
        C::Window,
        G::Screen,
    )
    .on_window()
    .key("move-to-screen-left"),
    d(
        "move-to-screen-right",
        "Envoyer sur l’écran de droite",
        "Move to the screen on the right",
        MOVE_SCREEN,
        C::Window,
        G::Screen,
    )
    .on_window()
    .key("move-to-screen-right"),
    d(
        "other-screen",
        "Envoyer sur l’autre écran",
        "Move to the other screen",
        MOVE_SCREEN,
        C::Window,
        G::Screen,
    )
    .on_window(),
    d(
        "focus-left",
        "Fenêtre de gauche",
        "Window on the left",
        FOCUS,
        C::Window,
        G::Arrow(Side4::Left),
    )
    .key("focus-left"),
    d(
        "focus-right",
        "Fenêtre de droite",
        "Window on the right",
        FOCUS,
        C::Window,
        G::Arrow(Side4::Right),
    )
    .key("focus-right"),
    d(
        "focus-up",
        "Fenêtre du haut",
        "Window above",
        FOCUS,
        C::Window,
        G::Arrow(Side4::Up),
    )
    .key("focus-up"),
    d(
        "focus-down",
        "Fenêtre du bas",
        "Window below",
        FOCUS,
        C::Window,
        G::Arrow(Side4::Down),
    )
    .key("focus-down"),
    d(
        "switch-windows",
        "Passer d’une fenêtre à l’autre",
        "Switch windows",
        &["alt tab", "basculer", "switcher"],
        C::Window,
        G::Switch,
    )
    .key("switch-windows"),
    d(
        "switch-windows-backward",
        "Passer d’une fenêtre à l’autre, à rebours",
        "Switch windows backward",
        &["alt tab", "basculer", "switcher"],
        C::Window,
        G::Switch,
    )
    .key("switch-windows-backward"),
    // The desktops.
    d(
        "desktop",
        "Aller au bureau…",
        "Go to desktop…",
        WORKSPACE,
        C::Desktops,
        G::Desktops,
    )
    .step(Step::Desktop)
    .key("desktop-"),
    d(
        "desktop-left",
        "Bureau de gauche",
        "Desktop on the left",
        &["espace de travail", "geste", "workspace", "gesture"],
        C::Desktops,
        G::Arrow(Side4::Left),
    )
    .key("desktop-left"),
    d(
        "desktop-right",
        "Bureau de droite",
        "Desktop on the right",
        &["espace de travail", "geste", "workspace", "gesture"],
        C::Desktops,
        G::Arrow(Side4::Right),
    )
    .key("desktop-right"),
    d(
        "overview",
        "Vue d’ensemble",
        "Overview",
        &[
            "exposé",
            "bureaux",
            "fenêtres",
            "tout voir",
            "expose",
            "desktops",
            "windows",
            "workspaces",
        ],
        C::Desktops,
        G::Grid,
    )
    .key("overview"),
    // The screens.
    d(
        "brightness-up",
        "Plus de luminosité",
        "Brightness up",
        &["luminosité", "écran", "brightness"],
        C::Screens,
        G::Brightness,
    )
    .key("brightness-up"),
    d(
        "brightness-down",
        "Moins de luminosité",
        "Brightness down",
        &["luminosité", "écran", "brightness"],
        C::Screens,
        G::Brightness,
    )
    .key("brightness-down"),
    // The agents.
    d(
        "waiting-agent",
        "Aller à l’agent qui attend",
        "Go to the waiting agent",
        &["agent", "attend", "panda", "waiting"],
        C::Agents,
        G::Agent,
    )
    .shell(),
    d(
        "go-to-agent",
        "Aller à un agent…",
        "Go to an agent…",
        &["agent", "panda", "claude", "codex"],
        C::Agents,
        G::Agent,
    )
    .step(Step::Agent)
    .shell(),
    d(
        "stop-agents",
        "Arrêter tous les agents",
        "Stop every agent",
        &["agent", "arrêter", "stop", "mcp", "claude"],
        C::Agents,
        G::Agent,
    )
    .key("stop-agents"),
    // The bar.
    d(
        "terminal",
        "Ouvrir un terminal",
        "Open a terminal",
        &["console", "shell"],
        C::Bar,
        G::Terminal,
    )
    .key("terminal"),
    d(
        "launcher",
        "Ouvrir le lanceur",
        "Open the launcher",
        &["applications", "chercher", "apps", "search"],
        C::Bar,
        G::Search,
    )
    .key("launcher"),
    d(
        "toggle-bar",
        "Changer la forme de la barre",
        "Change the bar's form",
        &["mini", "barre", "panel", "bar"],
        C::Bar,
        G::Bar,
    )
    .key("toggle-bar"),
    d(
        "briefing",
        "Briefing de la secrétaire",
        "The secretary's briefing",
        &["secrétaire", "agents", "résumé", "secretary", "summary"],
        C::Bar,
        G::Agent,
    )
    .shell(),
    d(
        "sources",
        "Sources des agents",
        "The agents' sources",
        &[
            "outils", "brancher", "claude", "codex", "herdr", "tools", "plug",
        ],
        C::Bar,
        G::Agent,
    )
    .shell(),
    d(
        "clipboard",
        "Historique du presse-papiers",
        "Clipboard history",
        &["copier", "coller", "copy", "paste"],
        C::Bar,
        G::Clipboard,
    )
    .key("clipboard"),
    d(
        "emoji",
        "Émojis",
        "Emoji",
        &["émoticône", "smiley"],
        C::Bar,
        G::Emoji,
    )
    .key("emoji"),
    d(
        "notifications",
        "Notifications",
        "Notifications",
        &["historique", "cloche", "history", "bell"],
        C::Bar,
        G::Bell,
    )
    .shell(),
    d(
        "dnd",
        "Ne pas déranger",
        "Do not disturb",
        &["notifications", "silence", "calme", "quiet"],
        C::Bar,
        G::Moon,
    )
    .switch()
    .shell(),
    d(
        "calendar",
        "Calendrier",
        "Calendar",
        &["agenda", "réunions", "date", "meetings"],
        C::Bar,
        G::Calendar,
    )
    .shell(),
    d(
        "join",
        "Rejoindre la réunion",
        "Join the meeting",
        &["visio", "appel", "call", "video"],
        C::Bar,
        G::Meeting,
    )
    .key("join"),
    d(
        "add-task",
        "Ajouter une tâche",
        "Add a task",
        &["tâches", "à faire", "todo", "tasks"],
        C::Bar,
        G::Task,
    )
    .shell(),
    d(
        "sound",
        "Son",
        "Sound",
        &["volume", "haut-parleurs", "micro", "speakers"],
        C::Bar,
        G::Sound,
    )
    .shell(),
    d(
        "volume-up",
        "Monter le son",
        "Volume up",
        &["volume", "son", "sound"],
        C::Bar,
        G::Sound,
    )
    .key("volume-up"),
    d(
        "volume-down",
        "Baisser le son",
        "Volume down",
        &["volume", "son", "sound"],
        C::Bar,
        G::Sound,
    )
    .key("volume-down"),
    d(
        "mute",
        "Couper le son",
        "Mute",
        &["muet", "silence", "mute"],
        C::Bar,
        G::Sound,
    )
    .key("mute"),
    d(
        "wifi",
        "Wi-Fi",
        "Wi-Fi",
        &["réseau", "internet", "wifi", "network"],
        C::Bar,
        G::Wifi,
    )
    .shell(),
    // The captures.
    d(
        "capture",
        "Capture d’écran",
        "Screenshot",
        &["capture", "copie d’écran", "screenshot"],
        C::Captures,
        G::Camera,
    )
    .key("capture"),
    d(
        "capture-screen",
        "Capturer l’écran",
        "Capture the screen",
        &["capture", "écran entier", "screenshot", "full screen"],
        C::Captures,
        G::Camera,
    )
    .key("capture-screen"),
    d(
        "capture-window",
        "Capturer la fenêtre",
        "Capture the window",
        &["capture", "screenshot"],
        C::Captures,
        G::Camera,
    )
    .on_window()
    .key("capture-window"),
    d(
        "record",
        "Enregistrer l’écran",
        "Record the screen",
        &[
            "capture vidéo",
            "vidéo",
            "enregistrement",
            "screen recording",
            "video",
        ],
        C::Captures,
        G::Record,
    )
    .key("record"),
    // Accessibility.
    d(
        "magnifier-in",
        "Zoomer",
        "Zoom in",
        &[
            "loupe",
            "grossir",
            "agrandir",
            "malvoyant",
            "magnifier",
            "magnify",
            "zoom",
            "low vision",
        ],
        C::Accessibility,
        G::Magnifier,
    )
    .key("magnifier-in"),
    d(
        "magnifier-out",
        "Dézoomer",
        "Zoom out",
        &["loupe", "réduire", "magnifier", "zoom"],
        C::Accessibility,
        G::Magnifier,
    )
    .key("magnifier-out"),
    d(
        "magnifier-off",
        "Éteindre la loupe",
        "Turn the magnifier off",
        &["loupe", "zoom", "magnifier"],
        C::Accessibility,
        G::Magnifier,
    )
    .key("magnifier-off"),
    d(
        "screen-reader",
        "Lecteur d’écran",
        "Screen reader",
        &["orca", "aveugle", "voix", "blind", "speech"],
        C::Accessibility,
        G::Speech,
    )
    .switch()
    .key("screen-reader"),
    d(
        "high-contrast",
        "Contraste élevé",
        "High contrast",
        &["contraste", "lisibilité", "contrast", "legibility"],
        C::Accessibility,
        G::Contrast,
    )
    .switch(),
    d(
        "reduce-motion",
        "Réduire les animations",
        "Reduce motion",
        &["animations", "mouvement", "motion", "animations"],
        C::Accessibility,
        G::Motion,
    )
    .switch(),
    d(
        "focus-bar",
        "Aller dans la barre",
        "Go to the bar",
        &[
            "clavier",
            "barre",
            "accessibilité",
            "focus",
            "keyboard",
            "panel",
            "accessibility",
        ],
        C::Accessibility,
        G::Keyboard,
    )
    .key("focus-bar"),
    // The session.
    d(
        "lock",
        "Verrouiller",
        "Lock",
        &["verrou", "mot de passe", "lock the session"],
        C::Session,
        G::Lock,
    )
    .key("lock"),
    d(
        "suspend",
        "Mettre en veille",
        "Suspend",
        &["veille", "dormir", "sleep"],
        C::Session,
        G::Moon,
    ),
];

/// The session's last three: after [`ACTIONS`], before the Settings.
const SESSION: [Def; 3] = [
    d(
        "log-out",
        "Se déconnecter",
        "Log out",
        &["quitter la session", "log off", "sign out"],
        C::Session,
        G::LogOut,
    )
    .key("log-out"),
    d(
        "restart",
        "Redémarrer",
        "Restart",
        &["redémarrage", "reboot"],
        C::Session,
        G::Restart,
    ),
    d(
        "power-off",
        "Éteindre",
        "Power off",
        &["arrêter", "shut down", "shutdown"],
        C::Session,
        G::Power,
    ),
];

/// Every action, by category in [`Category::ALL`]'s order.
pub fn catalogue() -> &'static [Def] {
    static ALL: std::sync::OnceLock<Vec<Def>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut all: Vec<Def> = ACTIONS
            .iter()
            .chain(&SESSION)
            .chain(&PAGES)
            .copied()
            .collect();
        all.sort_by_key(|d| d.category);
        all
    })
}

/// The action named `id`.
pub fn find(id: &str) -> Option<&'static Def> {
    catalogue().iter().find(|d| d.id == id)
}

/// The action a `[shortcuts]` key runs: its own, or its family's
/// (`send-to-desktop-3`); `actions` (the palette itself) has none.
pub fn of_shortcut(key: &str) -> Option<&'static Def> {
    catalogue().iter().find(|d| match d.shortcut {
        Some(prefix) if prefix.ends_with('-') => key.strip_prefix(prefix).is_some_and(|n| {
            n.len() == 1 && ('1'..='9').contains(&n.chars().next().unwrap_or('0'))
        }),
        Some(own) => own == key,
        None => false,
    })
}

/// Why an action cannot run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inert {
    /// It acts on a window, and none had the keyboard.
    NoWindow,
    /// It goes to another screen, and there is one.
    OneScreen,
    /// No screen on that side.
    NoScreenThere,
    /// The window's (or the screen's) desktop is the first, or the last.
    FirstDesktop,
    LastDesktop,
    /// The magnifier is off.
    MagnifierOff,
    /// logind does not allow it.
    NotAllowed,
    /// The desktop is the current one (a step's target).
    Current,
    /// No agent, or none waits.
    NoAgent,
    NoneWaiting,
    /// Nested, the screen reader starts with the session only.
    SessionOnly,
}

impl Inert {
    pub const ALL: [Self; 11] = [
        Self::NoWindow,
        Self::OneScreen,
        Self::NoScreenThere,
        Self::FirstDesktop,
        Self::LastDesktop,
        Self::MagnifierOff,
        Self::NotAllowed,
        Self::Current,
        Self::NoAgent,
        Self::NoneWaiting,
        Self::SessionOnly,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::NoWindow => "no-window",
            Self::OneScreen => "one-screen",
            Self::NoScreenThere => "no-screen-there",
            Self::FirstDesktop => "first-desktop",
            Self::LastDesktop => "last-desktop",
            Self::MagnifierOff => "magnifier-off",
            Self::NotAllowed => "not-allowed",
            Self::Current => "current",
            Self::NoAgent => "no-agent",
            Self::NoneWaiting => "none-waiting",
            Self::SessionOnly => "session-only",
        }
    }

    /// What the row says, in the user's language.
    pub const fn said(self, fr: bool) -> &'static str {
        match (self, fr) {
            (Self::NoWindow, true) => "aucune fenêtre active",
            (Self::NoWindow, false) => "no active window",
            (Self::OneScreen, true) => "un seul écran",
            (Self::OneScreen, false) => "one screen only",
            (Self::NoScreenThere, true) => "aucun écran de ce côté",
            (Self::NoScreenThere, false) => "no screen on that side",
            (Self::FirstDesktop, true) => "déjà le premier bureau",
            (Self::FirstDesktop, false) => "already the first desktop",
            (Self::LastDesktop, true) => "déjà le dernier bureau",
            (Self::LastDesktop, false) => "already the last desktop",
            (Self::MagnifierOff, true) => "loupe éteinte",
            (Self::MagnifierOff, false) => "magnifier off",
            (Self::NotAllowed, true) => "non autorisé",
            (Self::NotAllowed, false) => "not allowed",
            (Self::Current, true) => "actuel",
            (Self::Current, false) => "current",
            (Self::NoAgent, true) => "aucun agent",
            (Self::NoAgent, false) => "no agent",
            (Self::NoneWaiting, true) => "aucun agent n’attend",
            (Self::NoneWaiting, false) => "no agent waiting",
            (Self::SessionOnly, true) => "avec la session seulement",
            (Self::SessionOnly, false) => "with the session only",
        }
    }
}

/// `Super+P`: the palette opens on `output` (its scale in 120ths), for
/// window `window` (its title), else for that screen (its name); what is
/// on, what cannot run, the shortcuts bound. Open, it closes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionsShown {
    pub output: String,
    pub scale: u32,
    pub window: Option<u64>,
    #[serde(serialize_with = "tidy::title")]
    pub title: String,
    /// The screen's work area across: its left from the screen's and its
    /// width (logical pixels), the palette centred in it.
    pub area: (i32, i32),
    /// The switches on.
    pub on: Vec<String>,
    /// The actions that cannot run now, and why.
    pub inert: Vec<(String, Inert)>,
    /// Each `[shortcuts]` key and its chords as the file writes them
    /// (`Super+Shift+3`), in the file's order; none for an unbound one.
    pub shortcuts: Vec<(String, Vec<String>)>,
}

impl ActionsShown {
    /// Why action `id` cannot run, if it cannot.
    pub fn inert(&self, id: &str) -> Option<Inert> {
        self.inert
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, why)| *why)
    }

    pub fn is_on(&self, id: &str) -> bool {
        self.on.iter().any(|o| o == id)
    }

    /// The chords of `def`'s shortcut, as written: its own, or its
    /// family's first member's (its digit standing for the nine).
    pub fn chords(&self, def: &Def) -> Option<(&[String], bool)> {
        let key = def.shortcut?;
        let (key, family) = if key.ends_with('-') {
            (format!("{key}1"), true)
        } else {
            (key.to_owned(), false)
        };
        self.shortcuts
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, chords)| (chords.as_slice(), family))
            .filter(|(chords, _)| !chords.is_empty())
    }
}

/// The shell runs action `id` on `window`, with `arg`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAction {
    pub id: String,
    /// A desktop (from 0), an agent's id.
    pub arg: Option<u64>,
    pub window: Option<u64>,
}

/// A chord as the file writes it (`Super+Shift+Left`) as keycaps, in the
/// user's language: `["Super", "Maj", "←"]`. A family's (`family`) digit
/// shows as "1…9".
pub fn caps(chord: &str, fr: bool, family: bool) -> Vec<String> {
    let mut parts: Vec<&str> = chord.split('+').map(str::trim).collect();
    let Some(key) = parts.pop().filter(|k| !k.is_empty()) else {
        return vec![chord.to_owned()];
    };
    let mut held = [false; 4];
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "super" | "logo" | "mod4" => held[0] = true,
            "ctrl" | "control" => held[1] = true,
            "alt" | "mod1" => held[2] = true,
            "shift" => held[3] = true,
            _ => return vec![chord.to_owned()],
        }
    }
    let mut caps: Vec<String> = ["Super", "Ctrl", "Alt", if fr { "Maj" } else { "Shift" }]
        .iter()
        .zip(held)
        .filter(|(_, on)| *on)
        .map(|(name, _)| (*name).to_owned())
        .collect();
    let digit = key.len() == 1 && key.chars().all(|c| c.is_ascii_digit());
    caps.push(if family && digit {
        "1…9".to_owned()
    } else {
        key_shown(key, fr)
    });
    caps
}

/// A key's XKB name as a keycap shows it.
pub fn key_shown(key: &str, fr: bool) -> String {
    let named = match key.to_ascii_lowercase().as_str() {
        "return" => Some(if fr { "Entrée" } else { "Enter" }),
        "space" => Some(if fr { "Espace" } else { "Space" }),
        "left" => Some("←"),
        "right" => Some("→"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "print" => Some(if fr { "Impr. écran" } else { "Print Screen" }),
        "period" => Some("."),
        "comma" => Some(","),
        "equal" => Some("="),
        "plus" => Some("+"),
        "minus" => Some("-"),
        "kp_add" => Some(if fr { "+ (pavé)" } else { "+ (keypad)" }),
        "kp_subtract" => Some(if fr { "- (pavé)" } else { "- (keypad)" }),
        "tab" => Some("Tab"),
        "escape" => Some(if fr { "Échap" } else { "Esc" }),
        "backspace" => Some(if fr { "Retour arrière" } else { "Backspace" }),
        "delete" => Some(if fr { "Suppr" } else { "Delete" }),
        "xf86audioraisevolume" => Some("Volume +"),
        "xf86audiolowervolume" => Some("Volume −"),
        "xf86audiomute" => Some(if fr { "Muet" } else { "Mute" }),
        "xf86monbrightnessup" => Some(if fr { "Luminosité +" } else { "Brightness +" }),
        "xf86monbrightnessdown" => Some(if fr {
            "Luminosité −"
        } else {
            "Brightness −"
        }),
        _ => None,
    };
    match named {
        Some(name) => name.to_owned(),
        None if key.chars().count() == 1 => key.to_uppercase(),
        None => key.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_by_category_each_id_once() {
        let all = catalogue();
        assert!(all.len() >= 80, "{}", all.len());
        let mut ids: Vec<&str> = all.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "an id twice");
        assert!(all.windows(2).all(|w| w[0].category <= w[1].category));
        // A step's name ends with "…", and only a step's.
        for def in all {
            assert_eq!(def.step.is_some(), def.fr.ends_with('…'), "{}", def.id);
            assert_eq!(def.step.is_some(), def.en.ends_with('…'), "{}", def.id);
        }
        assert_eq!(
            all.iter()
                .filter(|d| d.category == Category::Settings)
                .count(),
            SETTINGS_PAGES.len()
        );
        for (def, (slug, fr, en)) in PAGES.iter().zip(SETTINGS_PAGES) {
            assert_eq!(def.settings_page(), Some(slug));
            assert!(def.fr.ends_with(fr) && def.en.ends_with(en), "{}", def.id);
        }
        assert_eq!(
            find("send-to-desktop").map(|d| d.stem(true)),
            Some("Envoyer au bureau")
        );
        assert_eq!(
            of_shortcut("send-to-desktop-4").map(|d| d.id),
            Some("send-to-desktop")
        );
        assert_eq!(
            of_shortcut("desktop-left").map(|d| d.id),
            Some("desktop-left")
        );
        assert_eq!(of_shortcut("desktop-0"), None);
        assert_eq!(of_shortcut("actions"), None);
    }

    #[test]
    fn an_actions_chords_are_found_by_its_shortcut() {
        let shown = ActionsShown {
            shortcuts: vec![
                ("close".into(), vec!["Super+Q".into(), "Alt+F4".into()]),
                ("focus-left".into(), Vec::new()),
                ("send-to-desktop-1".into(), vec!["Super+Shift+1".into()]),
            ],
            ..ActionsShown::default()
        };
        assert_eq!(
            shown
                .chords(find("close").expect("close"))
                .map(|c| c.0.len()),
            Some(2)
        );
        assert_eq!(shown.chords(find("focus-left").expect("focus")), None);
        assert_eq!(
            shown
                .chords(find("send-to-desktop").expect("send"))
                .map(|c| c.1),
            Some(true)
        );
    }

    #[test]
    fn chords_show_as_keycaps() {
        assert_eq!(caps("Super+Shift+3", true, true), ["Super", "Maj", "1…9"]);
        assert_eq!(
            caps("Super+Shift+Left", false, false),
            ["Super", "Shift", "←"]
        );
        assert_eq!(caps("Alt+F4", true, false), ["Alt", "F4"]);
        assert_eq!(caps("Super+m", true, false), ["Super", "M"]);
        assert_eq!(
            caps("Super+Ctrl+Shift+Right", true, false),
            ["Super", "Ctrl", "Maj", "→"]
        );
    }
}
