//! Applications / Applications (6.15, issue #449, variant A): the default
//! applications, and those that start with the session.
//!
//! - The browser, the mail, the videos and the images are the standard
//!   `$XDG_CONFIG_HOME/mimeapps.list`'s (`[Default Applications]`), which
//!   `xdg-open`, GLib, Qt and browsers read: only the row's types' lines
//!   are written, the rest of the file kept ([`hocus_xdg::mimeapps`]).
//! - The terminal is the file's `[programs] terminal` (a setting of the
//!   model, drawn by the page).
//! - "Rouvrir les applications de la dernière session" is the file's
//!   `[session] restore` (a switch of the model, under the start-up
//!   applications): the compositor reads it at the next log in.
//! - The start-up applications are the XDG autostart entries the session
//!   starts once the bar shows ([`hocus_xdg::autostart`], as the
//!   compositor reads them): a switch off is the user's copy with
//!   `Hidden=true`, one the user added is removed with its ✕.
//!
//! Everything that touches the disk is done on a thread of its own
//! ([`start`]): reading the desktop entries and the icons, resolving the
//! defaults, writing, and following the files' changes (inotify on the
//! configuration's folder and the autostart folder). The window hears it
//! through [`Heard`].

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
};

use hocus_xdg::{
    autostart,
    desktop::{self, Locale},
    icons::Themes,
    mimeapps::{self, Lists},
};

use crate::{
    lang::{Lang, Words, w},
    model::{Codec, Kind, Outside, RowGroup, SectionId, Setting, one},
};

/// The rows' ids: the terminal is the file's key, the others live outside
/// the file.
pub const BROWSER: &str = "applications.browser";
pub const MAIL: &str = "applications.mail";
pub const TERMINAL: &str = "programs.terminal";
pub const VIDEOS: &str = "applications.videos";
pub const IMAGES: &str = "applications.images";
pub const STARTUP: &str = "applications.startup";
/// The last session's applications reopened at log in (windows.md §14).
pub const RESTORE: &str = "session.restore";
/// The start-up row just added, lit as a search result.
pub const ADDED: &str = "applications.startup.added";

pub const DEFAULTS_GROUP: RowGroup = RowGroup {
    label: w("APPLICATIONS PAR DÉFAUT", "DEFAULT APPLICATIONS"),
    per_source: false,
};
pub const STARTUP_GROUP: RowGroup = RowGroup {
    label: w("AU DÉMARRAGE DE LA SESSION", "AT SESSION START"),
    per_source: false,
};

/// The terminals `Super+Return` looks for in `PATH`, in this order, when
/// the configuration names none: the compositor's list.
pub const TERMINALS: [&str; 4] = ["kitty", "foot", "alacritty", "xterm"];

/// A row of default applications written in `mimeapps.list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opens {
    Browser,
    Mail,
    Videos,
    Images,
}

impl Opens {
    pub const ALL: [Self; 4] = [Self::Browser, Self::Mail, Self::Videos, Self::Images];

    pub fn id(self) -> &'static str {
        match self {
            Self::Browser => BROWSER,
            Self::Mail => MAIL,
            Self::Videos => VIDEOS,
            Self::Images => IMAGES,
        }
    }

    pub fn of(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.id() == id)
    }

    pub fn name(self) -> Words {
        match self {
            Self::Browser => w("Navigateur web", "Web browser"),
            Self::Mail => w("Courriel", "Mail"),
            Self::Videos => w("Vidéos", "Videos"),
            Self::Images => w("Images", "Images"),
        }
    }

    pub fn line(self) -> Words {
        match self {
            Self::Browser => w(
                "Les liens, et les adresses tapées dans le lanceur.",
                "Links, and addresses typed in the launcher.",
            ),
            Self::Mail => w(
                "Les liens « mailto: », pour écrire à une adresse.",
                "mailto: links, to write to an address.",
            ),
            Self::Videos => w(
                "Les vidéos, et tes enregistrements d’écran quand tu cliques « Ouvrir ».",
                "Videos, and your screen recordings when you click “Open”.",
            ),
            Self::Images => w(
                "Les images ouvertes depuis un dossier, un courriel ou une autre application.",
                "Images opened from a folder, a mail or another application.",
            ),
        }
    }

    /// Under the menu's last application.
    pub fn hint(self) -> Words {
        match self {
            Self::Browser => w(
                "Celles qui ouvrent les pages web",
                "Those that open web pages",
            ),
            Self::Mail => w(
                "Celles qui ouvrent les liens « mailto: »",
                "Those that open mailto: links",
            ),
            Self::Videos => w("Celles qui ouvrent les vidéos", "Those that open videos"),
            Self::Images => w("Celles qui ouvrent les images", "Those that open images"),
        }
    }

    /// The row's line when no application opens its types.
    pub fn none(self) -> Words {
        match self {
            Self::Browser => w(
                "Aucune application installée n’ouvre les pages web.",
                "No installed application opens web pages.",
            ),
            Self::Mail => w(
                "Aucune application installée n’ouvre les liens « mailto: ».",
                "No installed application opens mailto: links.",
            ),
            Self::Videos => w(
                "Aucune application installée n’ouvre les vidéos.",
                "No installed application opens videos.",
            ),
            Self::Images => w(
                "Aucune application installée n’ouvre les images.",
                "No installed application opens images.",
            ),
        }
    }

    /// The types written, the first the one the row shows.
    pub fn types(self) -> &'static [&'static str] {
        match self {
            Self::Browser => &[
                "x-scheme-handler/http",
                "x-scheme-handler/https",
                "text/html",
                "application/xhtml+xml",
            ],
            Self::Mail => &["x-scheme-handler/mailto"],
            Self::Videos => &[
                "video/mp4",
                "video/webm",
                "video/x-matroska",
                "video/quicktime",
                "video/x-msvideo",
                "video/mpeg",
                "video/ogg",
            ],
            Self::Images => &[
                "image/png",
                "image/jpeg",
                "image/webp",
                "image/gif",
                "image/svg+xml",
                "image/avif",
                "image/bmp",
                "image/tiff",
            ],
        }
    }
}

/// A type as a sentence says it: "Les vidéos WebM" / "WebM videos".
pub fn type_said(mime: &str, lang: Lang) -> String {
    let (fr, en): (&str, &str) = match mime {
        "x-scheme-handler/http" => ("Les liens http:", "http: links"),
        "x-scheme-handler/https" => ("Les liens https:", "https: links"),
        "text/html" => ("Les pages HTML", "HTML pages"),
        "application/xhtml+xml" => ("Les pages XHTML", "XHTML pages"),
        "video/mp4" => ("Les vidéos MP4", "MP4 videos"),
        "video/webm" => ("Les vidéos WebM", "WebM videos"),
        "video/x-matroska" => ("Les vidéos MKV", "MKV videos"),
        "video/quicktime" => ("Les vidéos QuickTime", "QuickTime videos"),
        "video/x-msvideo" => ("Les vidéos AVI", "AVI videos"),
        "video/mpeg" => ("Les vidéos MPEG", "MPEG videos"),
        "video/ogg" => ("Les vidéos Ogg", "Ogg videos"),
        "image/png" => ("Les images PNG", "PNG images"),
        "image/jpeg" => ("Les images JPEG", "JPEG images"),
        "image/webp" => ("Les images WebP", "WebP images"),
        "image/gif" => ("Les images GIF", "GIF images"),
        "image/svg+xml" => ("Les images SVG", "SVG images"),
        "image/avif" => ("Les images AVIF", "AVIF images"),
        "image/bmp" => ("Les images BMP", "BMP images"),
        "image/tiff" => ("Les images TIFF", "TIFF images"),
        other => {
            return match lang {
                Lang::French => format!("Le type {other}"),
                Lang::English => format!("The type {other}"),
            };
        }
    };
    if lang.fr() { fr } else { en }.to_owned()
}

/// "Les vidéos WebM s’ouvrent avec VLC" / "WebM videos open with VLC".
pub fn differs_said(mime: &str, app: &str, lang: Lang) -> String {
    let what = type_said(mime, lang);
    let plural = !what.starts_with("Le type") && !what.starts_with("The type");
    match (lang, plural) {
        (Lang::French, true) => format!("{what} s’ouvrent avec {app}"),
        (Lang::French, false) => format!("{what} s’ouvre avec {app}"),
        (Lang::English, true) => format!("{what} open with {app}"),
        (Lang::English, false) => format!("{what} opens with {app}"),
    }
}

/// The terminal: a key of the file, its row drawn by the page; the
/// last session's applications reopened, a switch.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![
            Setting {
                id: TERMINAL,
                section: SectionId::Apps,
                group: DEFAULTS_GROUP,
                name: w("Terminal", "Terminal"),
                line: Some(w(
                    "Super+Entrée, et les applications qui s’ouvrent dans un terminal.",
                    "Super+Return, and applications that open in a terminal.",
                )),
                keywords: (
                    &[
                        "terminal",
                        "console",
                        "par défaut",
                        "super+entrée",
                        "ligne de commande",
                    ],
                    &[
                        "terminal",
                        "console",
                        "default",
                        "super+return",
                        "command line",
                    ],
                ),
                kind: Kind::Text,
                places: one(&["programs", "terminal"], Codec::Text),
                needs: None,
            },
            Setting {
                id: RESTORE,
                section: SectionId::Apps,
                group: STARTUP_GROUP,
                name: w(
                    "Rouvrir les applications de la dernière session",
                    "Reopen the applications of the last session",
                ),
                line: Some(w(
                    "Celles ouvertes à la déconnexion reviennent, chaque fenêtre à sa place.",
                    "Those open at log out come back, each window in its place.",
                )),
                keywords: (
                    &[
                        "session",
                        "rouvrir",
                        "restaurer",
                        "dernière session",
                        "fenêtres",
                        "déconnexion",
                    ],
                    &[
                        "session",
                        "reopen",
                        "restore",
                        "last session",
                        "windows",
                        "log out",
                    ],
                ),
                kind: Kind::Switch,
                places: one(&["session", "restore"], Codec::Bool),
                needs: None,
            },
        ]
    })
}

/// The rows outside the file, for the search.
pub fn outside() -> &'static [Outside] {
    const ALL: [Outside; 5] = [
        Outside {
            id: BROWSER,
            section: SectionId::Apps,
            group: DEFAULTS_GROUP.label,
            name: w("Navigateur web", "Web browser"),
            keywords: (
                &[
                    "navigateur",
                    "web",
                    "firefox",
                    "liens",
                    "par défaut",
                    "ouvrir avec",
                    "mimeapps",
                ],
                &[
                    "browser",
                    "web",
                    "firefox",
                    "links",
                    "default",
                    "open with",
                    "mimeapps",
                ],
            ),
        },
        Outside {
            id: MAIL,
            section: SectionId::Apps,
            group: DEFAULTS_GROUP.label,
            name: w("Courriel", "Mail"),
            keywords: (
                &["courriel", "mail", "e-mail", "mailto", "messagerie"],
                &["mail", "e-mail", "email", "mailto"],
            ),
        },
        Outside {
            id: VIDEOS,
            section: SectionId::Apps,
            group: DEFAULTS_GROUP.label,
            name: w("Vidéos", "Videos"),
            keywords: (
                &["vidéos", "lecteur", "film", "enregistrement"],
                &["videos", "player", "movie", "recording"],
            ),
        },
        Outside {
            id: IMAGES,
            section: SectionId::Apps,
            group: DEFAULTS_GROUP.label,
            name: w("Images", "Images"),
            keywords: (
                &["images", "photos", "visionneuse"],
                &["images", "photos", "pictures", "viewer"],
            ),
        },
        Outside {
            id: STARTUP,
            section: SectionId::Apps,
            group: STARTUP_GROUP.label,
            name: w("Au démarrage de la session", "At session start"),
            keywords: (
                &[
                    "démarrage",
                    "au lancement",
                    "session",
                    "démarrer",
                    "autostart",
                    "login",
                ],
                &[
                    "startup",
                    "start-up",
                    "autostart",
                    "login",
                    "session",
                    "start",
                ],
            ),
        },
    ];
    &ALL
}

// --- What the thread reads ------------------------------------------------------------

/// An installed application, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Its desktop file id, without `.desktop`.
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub mime_types: Vec<String>,
    /// `Categories=…TerminalEmulator`.
    pub terminal_emulator: bool,
    /// Its command, field codes out, as a shell reads it.
    pub command: String,
    /// The program it runs.
    pub program: String,
    pub file: PathBuf,
}

/// The arguments as a shell reads them back: quoted when they need it.
pub fn joined(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=+,@%".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Types and the application each is given (`None`: none).
pub type Lines = Vec<(String, Option<String>)>;

/// Everything the page shows, as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Known {
    /// The applications the launcher shows, by name.
    pub apps: Vec<App>,
    /// Each row's types and the application each opens with, if one.
    pub defaults: Vec<(Opens, Lines)>,
    /// The autostart entries, by name, all of them.
    pub startup: Vec<autostart::Entry>,
    /// What "Automatique" starts: the first of [`TERMINALS`] in `PATH`.
    pub found_terminal: Option<String>,
}

impl Known {
    pub fn app(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// The applications that open one of `opens`' types, by name.
    pub fn handlers(&self, opens: Opens) -> Vec<&App> {
        self.apps
            .iter()
            .filter(|a| {
                opens
                    .types()
                    .iter()
                    .any(|t| a.mime_types.iter().any(|m| m == t))
            })
            .collect()
    }

    /// The terminals installed (`TerminalEmulator`), by name.
    pub fn terminals(&self) -> Vec<&App> {
        self.apps.iter().filter(|a| a.terminal_emulator).collect()
    }

    /// The types of `opens` and their applications.
    pub fn defaults_of(&self, opens: Opens) -> &[(String, Option<String>)] {
        self.defaults
            .iter()
            .find(|(o, _)| *o == opens)
            .map_or(&[], |(_, d)| d.as_slice())
    }

    /// The application `opens` shows: its first type's; when the lists
    /// say none for it, another type's; else the first application that
    /// opens them, as `xdg-open` would take one.
    pub fn chosen(&self, opens: Opens) -> Option<&str> {
        let defaults = self.defaults_of(opens);
        defaults
            .first()
            .and_then(|(_, app)| app.as_deref())
            .or_else(|| defaults.iter().find_map(|(_, app)| app.as_deref()))
            .or_else(|| self.handlers(opens).first().map(|a| a.id.as_str()))
    }

    /// The first of `opens`' types that the lists give to another
    /// application than the one shown, and that application.
    pub fn differs(&self, opens: Opens) -> Option<(&str, &str)> {
        let chosen = self.chosen(opens);
        self.defaults_of(opens)
            .iter()
            .find_map(|(mime, app)| match app.as_deref() {
                Some(app) if Some(app) != chosen => Some((mime.as_str(), app)),
                _ => None,
            })
    }

    /// The start-up rows: the entries that run in this session (hidden
    /// ones too), sorted by name.
    pub fn rows(&self) -> Vec<&autostart::Entry> {
        self.startup
            .iter()
            .filter(|e| e.here == autostart::Here::Starts)
            .collect()
    }

    /// The entries for other desktops only.
    pub fn others(&self) -> Vec<&autostart::Entry> {
        self.startup
            .iter()
            .filter(|e| e.here == autostart::Here::OtherDesktops)
            .collect()
    }

    /// The applications that can be added: not there already.
    pub fn addable(&self) -> Vec<&App> {
        self.apps
            .iter()
            .filter(|a| !self.startup.iter().any(|e| e.id == a.id))
            .collect()
    }
}

/// Whether a start-up entry is the user's own (not an installed one's
/// copy): its ✕ removes it.
pub fn users_own(entry: &autostart::Entry) -> bool {
    entry.user && entry.installed.is_none()
}

/// Where the thread reads and writes.
#[derive(Debug, Clone)]
pub struct Dirs {
    pub data: Vec<PathBuf>,
    pub lists: Lists,
    /// The autostart directories, the user's first.
    pub autostart: Vec<PathBuf>,
    pub path: Option<OsString>,
}

impl Dirs {
    pub fn from_env() -> Self {
        let data = desktop::data_dirs();
        Self {
            lists: Lists::of(&data),
            data,
            autostart: autostart::dirs(),
            path: std::env::var_os("PATH"),
        }
    }

    fn user_autostart(&self) -> Option<&Path> {
        self.autostart.first().map(PathBuf::as_path)
    }
}

/// Reads everything the page shows.
pub fn read(dirs: &Dirs, locale: &Locale) -> Known {
    let desktops = vec![hocus_link_desktop()];
    let entries = desktop::scan(&dirs.data, locale, &desktops);
    let mut apps: Vec<App> = entries
        .into_iter()
        .map(|e| App {
            program: e.argv.first().cloned().unwrap_or_default(),
            command: joined(&e.argv),
            terminal_emulator: e.categories.iter().any(|c| c == "TerminalEmulator"),
            id: e.id,
            name: e.name,
            icon: e.icon,
            mime_types: e.mime_types,
            file: e.file,
        })
        .collect();
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let installed = |id: &str| apps.iter().any(|a| a.id == id);
    let defaults = Opens::ALL
        .into_iter()
        .map(|opens| {
            let types = opens
                .types()
                .iter()
                .map(|t| {
                    let found = mimeapps::default_for(&dirs.lists, t, &installed);
                    ((*t).to_owned(), found)
                })
                .collect();
            (opens, types)
        })
        .collect();
    let startup = autostart::scan_in(&dirs.autostart, locale, &desktops, dirs.path.as_deref());
    let found_terminal = first_terminal(dirs.path.as_deref());
    Known {
        apps,
        defaults,
        startup,
        found_terminal,
    }
}

/// The session's desktop, as the compositor names it.
fn hocus_link_desktop() -> String {
    "Hocuspanda".to_owned()
}

/// Whether `program` is an executable file, at its path or in `path`.
pub fn in_path(program: &str, path: Option<&std::ffi::OsStr>) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable = |file: &Path| {
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        return executable(Path::new(program));
    }
    path.is_some_and(|path| std::env::split_paths(path).any(|d| executable(&d.join(program))))
}

/// The first of [`TERMINALS`] in `path`.
pub fn first_terminal(path: Option<&std::ffi::OsStr>) -> Option<String> {
    TERMINALS
        .into_iter()
        .find(|t| in_path(t, path))
        .map(str::to_owned)
}

/// A command's program: its first word.
pub fn program_of(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .next()
        .map(|p| p.trim_matches(|c| c == '\'' || c == '"').to_owned())
}

// --- The thread -----------------------------------------------------------------------

/// An icon's pixels, read on the thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    Rgba {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Svg(Vec<u8>),
}

/// What the window asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The row's types written for application `id`.
    Default {
        opens: Opens,
        id: String,
        serial: u64,
    },
    /// `mimeapps.list` put back: `to` (`None`: no file) when it still holds
    /// `expect`, else the lines `lines` written in what it holds.
    DefaultsBack {
        opens: Opens,
        to: Option<String>,
        expect: Option<String>,
        lines: Vec<(String, Option<String>)>,
        serial: u64,
    },
    /// An autostart entry switched on or off.
    Switch { id: String, on: bool, serial: u64 },
    /// The user's autostart entry removed.
    Remove { id: String, serial: u64 },
    /// Application `id`'s entry copied into the user's autostart folder.
    Add { id: String, serial: u64 },
    /// The user's file of autostart entry `id` made `bytes` (`None`: none).
    Put {
        id: String,
        bytes: Option<Vec<u8>>,
        serial: u64,
    },
    /// Whether a terminal's command's program is in `PATH`.
    Check { command: String },
    /// Read again.
    Again,
}

/// What a write did, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Defaults {
        opens: Opens,
        /// The file before (`None`: none) and after.
        before: Option<String>,
        after: Option<String>,
        /// The row's lines before and after, as written.
        lines_before: Vec<(String, Option<String>)>,
        lines_after: Vec<(String, Option<String>)>,
    },
    Startup {
        id: String,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    },
}

/// What the window hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Known(Box<Known>),
    Icons(Vec<(String, Picture)>),
    Done {
        serial: u64,
        result: Result<Done, String>,
    },
    Checked {
        command: String,
        found: bool,
    },
}

/// The thread's end of the line.
#[derive(Debug)]
pub struct Worker {
    asks: mpsc::Sender<Ask>,
}

impl Worker {
    pub fn ask(&self, ask: Ask) -> bool {
        self.asks.send(ask).is_ok()
    }
}

/// Starts the thread: it reads at once, then each time the files change.
pub fn start(heard: impl FnMut(Heard) -> bool + Send + 'static) -> Option<Worker> {
    let (asks, taken) = mpsc::channel();
    let dirs = Dirs::from_env();
    watch(&dirs, asks.clone());
    let started = std::thread::Builder::new()
        .name("applications".to_owned())
        .spawn(move || run(&dirs, &taken, heard));
    match started {
        Ok(_) => Some(Worker { asks }),
        Err(err) => {
            tracing::warn!(%err, "the applications' thread did not start");
            None
        }
    }
}

fn run(dirs: &Dirs, taken: &mpsc::Receiver<Ask>, mut heard: impl FnMut(Heard) -> bool) {
    let locale = Locale::from_env();
    let mut known = read(dirs, &locale);
    if !heard(Heard::Known(Box::new(known.clone()))) {
        return;
    }
    let icons = pictures(dirs, &known);
    if !heard(Heard::Icons(icons)) {
        return;
    }
    while let Ok(ask) = taken.recv() {
        let said = match ask {
            Ask::Again => None,
            Ask::Check { command } => {
                let found = program_of(&command).is_some_and(|p| in_path(&p, dirs.path.as_deref()));
                Some(Heard::Checked { command, found })
            }
            Ask::Default { opens, id, serial } => Some(Heard::Done {
                serial,
                result: set_default(dirs, opens, &id),
            }),
            Ask::DefaultsBack {
                opens,
                to,
                expect,
                lines,
                serial,
            } => Some(Heard::Done {
                serial,
                result: defaults_back(dirs, opens, to, expect, lines),
            }),
            Ask::Switch { id, on, serial } => Some(Heard::Done {
                serial,
                result: startup(dirs, &known, &id, |entry, copy, installed| {
                    switch(entry, copy, installed, on)
                }),
            }),
            Ask::Remove { id, serial } => Some(Heard::Done {
                serial,
                result: startup(dirs, &known, &id, |_, _, _| Ok(None)),
            }),
            Ask::Add { id, serial } => Some(Heard::Done {
                serial,
                result: add(dirs, &known, &id),
            }),
            Ask::Put { id, bytes, serial } => Some(Heard::Done {
                serial,
                result: put(dirs, &id, bytes),
            }),
        };
        if let Some(said) = said
            && !heard(said)
        {
            return;
        }
        // Whatever was asked, what is on disk now (only when it changed).
        let now = read(dirs, &locale);
        if now != known {
            let new_icons = now.apps.len() != known.apps.len()
                || now
                    .startup
                    .iter()
                    .any(|e| e.icon.is_some() && !known.startup.iter().any(|k| k.icon == e.icon));
            known = now;
            if !heard(Heard::Known(Box::new(known.clone()))) {
                return;
            }
            if new_icons && !heard(Heard::Icons(pictures(dirs, &known))) {
                return;
            }
        }
    }
}

/// The icons of the applications and the start-up entries, 64 px.
fn pictures(dirs: &Dirs, known: &Known) -> Vec<(String, Picture)> {
    let mut themes = Themes::load(&dirs.data);
    let mut names: Vec<&str> = known
        .apps
        .iter()
        .filter_map(|a| a.icon.as_deref())
        .chain(known.startup.iter().filter_map(|e| e.icon.as_deref()))
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut out = Vec::new();
    for name in names {
        let Some(file) = themes.find(name, 32, 2) else {
            continue;
        };
        if let Some(picture) = picture(&file) {
            out.push((name.to_owned(), picture));
        }
    }
    out
}

/// An icon file's pixels: an SVG as it is, a PNG decoded, 64 px at most.
fn picture(file: &Path) -> Option<Picture> {
    if std::fs::metadata(file).ok()?.len() > 4 << 20 {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    match file.extension().and_then(|e| e.to_str()) {
        Some("svg") => Some(Picture::Svg(bytes)),
        Some("png") => {
            let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .ok()?
                .into_rgba8();
            let (w, h) = decoded.dimensions();
            let decoded = if w.max(h) > 64 {
                let k = 64.0 / w.max(h) as f32;
                image::imageops::resize(
                    &decoded,
                    ((w as f32 * k).round() as u32).max(1),
                    ((h as f32 * k).round() as u32).max(1),
                    image::imageops::FilterType::CatmullRom,
                )
            } else {
                decoded
            };
            let (width, height) = decoded.dimensions();
            Some(Picture::Rgba {
                width,
                height,
                pixels: decoded.into_raw(),
            })
        }
        _ => None,
    }
}

/// `text` in `file`, beside it then renamed; read back. `None`: removed.
fn write(file: &Path, text: Option<&[u8]>) -> Result<(), String> {
    match text {
        Some(bytes) => {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            hocus_config::replace(file, bytes, None).map_err(|e| e.to_string())?;
            let back = std::fs::read(file).map_err(|e| e.to_string())?;
            if back != bytes {
                return Err("the file does not read back as written".to_owned());
            }
            Ok(())
        }
        None => match std::fs::remove_file(file) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.to_string()),
        },
    }
}

fn read_text(file: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

/// The lines of `types` in `text`'s `[Default Applications]`.
fn lines_of(text: &str, types: &[&str]) -> Vec<(String, Option<String>)> {
    let defaults = mimeapps::defaults(text);
    types
        .iter()
        .map(|t| {
            (
                (*t).to_owned(),
                defaults
                    .iter()
                    .find(|(k, _)| k == t)
                    .map(|(_, v)| v.clone()),
            )
        })
        .collect()
}

fn set_default(dirs: &Dirs, opens: Opens, id: &str) -> Result<Done, String> {
    let file = &dirs.lists.user;
    let before = read_text(file)?;
    let text = before.clone().unwrap_or_default();
    let lines_before = lines_of(&text, opens.types());
    let lines_after: Vec<(String, Option<String>)> = opens
        .types()
        .iter()
        .map(|t| ((*t).to_owned(), Some(mimeapps::value_of(id))))
        .collect();
    let after = mimeapps::edited(&text, &lines_after);
    write(file, Some(after.as_bytes()))?;
    tracing::info!(row = opens.id(), app = id, file = %file.display(), "a default application written");
    Ok(Done::Defaults {
        opens,
        before,
        after: Some(after),
        lines_before,
        lines_after,
    })
}

fn defaults_back(
    dirs: &Dirs,
    opens: Opens,
    to: Option<String>,
    expect: Option<String>,
    lines: Vec<(String, Option<String>)>,
) -> Result<Done, String> {
    let file = &dirs.lists.user;
    let before = read_text(file)?;
    let lines_before = lines_of(before.as_deref().unwrap_or_default(), opens.types());
    // Untouched since: as it was, byte for byte; else only the lines.
    let after = if before == expect {
        to
    } else {
        Some(mimeapps::edited(
            before.as_deref().unwrap_or_default(),
            &lines,
        ))
    };
    write(file, after.as_deref().map(str::as_bytes))?;
    Ok(Done::Defaults {
        opens,
        before,
        after,
        lines_before,
        lines_after: lines,
    })
}

/// Autostart entry `id`'s user file made what `make` says from the entry,
/// the user's copy and the installed file's text (`None`: removed).
fn startup(
    dirs: &Dirs,
    known: &Known,
    id: &str,
    make: impl FnOnce(&autostart::Entry, Option<&str>, Option<&str>) -> Result<Option<String>, String>,
) -> Result<Done, String> {
    let entry = known
        .startup
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("no autostart entry {id}"))?;
    let user = dirs
        .user_autostart()
        .ok_or("no autostart folder")?
        .join(format!("{id}.desktop"));
    let copy = read_text(&user)?;
    let installed = match &entry.installed {
        Some(file) => read_text(file)?,
        None => None,
    };
    let after = make(entry, copy.as_deref(), installed.as_deref())?;
    write(&user, after.as_deref().map(str::as_bytes))?;
    tracing::info!(id, file = %user.display(), written = after.is_some(), "an autostart entry changed");
    Ok(Done::Startup {
        id: id.to_owned(),
        before: copy.map(String::into_bytes),
        after: after.map(String::into_bytes),
    })
}

/// The user's file of an entry switched `on` or off.
fn switch(
    entry: &autostart::Entry,
    copy: Option<&str>,
    installed: Option<&str>,
    on: bool,
) -> Result<Option<String>, String> {
    if users_own(entry) {
        let copy = copy.ok_or("the entry's file is gone")?;
        return Ok(Some(autostart::with_hidden(
            copy,
            if on { None } else { Some(true) },
        )));
    }
    match (on, copy) {
        (false, Some(copy)) => Ok(Some(autostart::with_hidden(copy, Some(true)))),
        (false, None) => {
            let installed = installed.ok_or("the installed entry is gone")?;
            Ok(Some(autostart::with_hidden(installed, Some(true))))
        }
        (true, Some(copy)) if autostart::only_hidden(copy, installed) => Ok(None),
        (true, Some(copy)) => Ok(Some(autostart::with_hidden(copy, Some(false)))),
        (true, None) => Ok(None),
    }
}

fn add(dirs: &Dirs, known: &Known, id: &str) -> Result<Done, String> {
    let app = known
        .app(id)
        .ok_or_else(|| format!("no application {id}"))?;
    let bytes = std::fs::read(&app.file).map_err(|e| e.to_string())?;
    let user = dirs
        .user_autostart()
        .ok_or("no autostart folder")?
        .join(format!("{id}.desktop"));
    let before = match std::fs::read(&user) {
        Ok(b) => Some(b),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    write(&user, Some(&bytes))?;
    tracing::info!(id, file = %user.display(), "an application added to start-up");
    Ok(Done::Startup {
        id: id.to_owned(),
        before,
        after: Some(bytes),
    })
}

fn put(dirs: &Dirs, id: &str, bytes: Option<Vec<u8>>) -> Result<Done, String> {
    let user = dirs
        .user_autostart()
        .ok_or("no autostart folder")?
        .join(format!("{id}.desktop"));
    let before = match std::fs::read(&user) {
        Ok(b) => Some(b),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    write(&user, bytes.as_deref())?;
    Ok(Done::Startup {
        id: id.to_owned(),
        before,
        after: bytes,
    })
}

/// Follows `mimeapps.list` and the user's autostart folder: inotify on
/// their folders, the thread asked to read again.
fn watch(dirs: &Dirs, asks: mpsc::Sender<Ask>) {
    use rustix::fs::inotify;
    let mut folders: Vec<PathBuf> = dirs
        .lists
        .user
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    if let Some(autostart) = dirs.user_autostart() {
        folders.push(autostart.to_path_buf());
    }
    let fd = match inotify::init(inotify::CreateFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "mimeapps.list and autostart are not followed");
            return;
        }
    };
    let flags = inotify::WatchFlags::CLOSE_WRITE
        | inotify::WatchFlags::MOVED_TO
        | inotify::WatchFlags::MOVED_FROM
        | inotify::WatchFlags::CREATE
        | inotify::WatchFlags::DELETE;
    let mut watched = HashMap::new();
    for folder in &folders {
        match inotify::add_watch(&fd, folder, flags) {
            Ok(wd) => {
                watched.insert(wd, folder.clone());
            }
            Err(err) => tracing::debug!(%err, folder = %folder.display(), "a folder not followed"),
        }
    }
    let list = dirs
        .lists
        .user
        .file_name()
        .map(|n| n.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    let autostart_name = dirs
        .user_autostart()
        .and_then(Path::file_name)
        .map(|n| n.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    let started = std::thread::Builder::new()
        .name("applications' watch".to_owned())
        .spawn(move || {
            let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4096];
            let mut reader = inotify::Reader::new(&fd, &mut buffer);
            loop {
                match reader.next() {
                    Ok(event) => {
                        let name = event.file_name().map(|n| n.to_bytes().to_vec());
                        let ours = match name.as_deref() {
                            // In the autostart folder: any entry.
                            Some(n) if n.ends_with(b".desktop") => true,
                            Some(n) => n == list.as_slice() || n == autostart_name.as_slice(),
                            None => false,
                        };
                        if ours && asks.send(Ask::Again).is_err() {
                            return;
                        }
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(err) => {
                        tracing::warn!(%err, "the applications' watch stopped");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "mimeapps.list and autostart are not followed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Places, Value};

    /// "Rouvrir les applications de la dernière session": a switch of the
    /// start-up group, `[session] restore`, on when the file says nothing;
    /// turned off, the file says so, the rest kept.
    #[test]
    fn the_last_session_switch_is_session_restore_on_by_default() -> anyhow::Result<()> {
        use crate::{
            model::Source,
            store::{self, Raw, Reading, Slot},
        };
        let setting = crate::model::setting(RESTORE).ok_or_else(|| anyhow::anyhow!("none"))?;
        assert_eq!(setting.section, SectionId::Apps);
        assert_eq!(setting.group, STARTUP_GROUP);
        assert_eq!(setting.kind, Kind::Switch);
        let Places::One(place) = &setting.places else {
            anyhow::bail!("not one key");
        };
        assert_eq!(place.key, &["session", "restore"]);
        assert_eq!(place.default, Value::Bool(true));
        let slot = Slot::of(setting, Source::Ac);
        let text = "[programs]\nterminal = \"foot\"\n";
        assert_eq!(
            store::read(text).readings.get(&slot),
            Some(&Reading::Absent)
        );
        let off = store::written(text, slot, &Raw::Set(Value::Bool(false)))
            .map_err(anyhow::Error::msg)?;
        assert!(off.contains("[session]\nrestore = false"), "{off}");
        assert!(off.contains("terminal = \"foot\""), "{off}");
        assert_eq!(
            store::read(&off).readings.get(&slot),
            Some(&Reading::Value(Value::Bool(false)))
        );
        Ok(())
    }

    fn entry_of(id: &str, user: bool, installed: bool) -> autostart::Entry {
        autostart::Entry {
            id: id.to_owned(),
            file: PathBuf::from(format!("/a/{id}.desktop")),
            user,
            installed: installed.then(|| PathBuf::from(format!("/etc/xdg/autostart/{id}.desktop"))),
            name: id.to_owned(),
            comment: None,
            icon: None,
            exec: id.to_owned(),
            argv: vec![id.to_owned()],
            terminal: false,
            dir: None,
            hidden: false,
            here: autostart::Here::Starts,
            only_in: Vec::new(),
        }
    }

    #[test]
    fn an_installed_entry_off_is_a_hidden_copy_and_on_again_none() -> Result<(), String> {
        let installed = "[Desktop Entry]\nType=Application\nName=Slack\nExec=slack\n";
        let entry = entry_of("slack", false, true);
        let off = switch(&entry, None, Some(installed), false)?;
        assert_eq!(
            off.as_deref(),
            Some("[Desktop Entry]\nType=Application\nName=Slack\nExec=slack\nHidden=true\n")
        );
        let entry = entry_of("slack", true, true);
        assert_eq!(switch(&entry, off.as_deref(), Some(installed), true)?, None);
        // A copy changed by hand keeps its changes.
        let mine = "[Desktop Entry]\nType=Application\nName=Slack\nExec=slack --x\nHidden=true\n";
        assert_eq!(
            switch(&entry, Some(mine), Some(installed), true)?.as_deref(),
            Some("[Desktop Entry]\nType=Application\nName=Slack\nExec=slack --x\nHidden=false\n")
        );
        // The user's own: the key set, then taken out.
        let own = entry_of("notes", true, false);
        let text = "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes\n";
        let off = switch(&own, Some(text), None, false)?;
        assert_eq!(
            switch(&own, off.as_deref(), None, true)?.as_deref(),
            Some(text)
        );
        Ok(())
    }

    #[test]
    fn a_command_joined_reads_back_in_a_shell() {
        let argv = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            joined(&argv(&["kitty", "--single-instance"])),
            "kitty --single-instance"
        );
        assert_eq!(
            joined(&argv(&["sh", "-c", "echo it's"])),
            "sh -c 'echo it'\\''s'"
        );
        assert_eq!(program_of("  foot -e x").as_deref(), Some("foot"));
    }

    #[test]
    fn the_row_shows_its_first_type_and_says_one_that_differs() {
        let known = Known {
            defaults: vec![(
                Opens::Videos,
                vec![
                    ("video/mp4".into(), Some("mpv".into())),
                    ("video/webm".into(), Some("vlc".into())),
                    ("video/ogg".into(), None),
                ],
            )],
            ..Known::default()
        };
        assert_eq!(known.chosen(Opens::Videos), Some("mpv"));
        assert_eq!(known.differs(Opens::Videos), Some(("video/webm", "vlc")));
        assert_eq!(
            differs_said("video/webm", "VLC", Lang::French),
            "Les vidéos WebM s’ouvrent avec VLC"
        );
        assert_eq!(
            differs_said("video/webm", "VLC", Lang::English),
            "WebM videos open with VLC"
        );
        assert_eq!(known.chosen(Opens::Mail), None);
    }
}
