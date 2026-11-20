//! Fond d'écran / Wallpaper (6.3, issue #403, variant A, the gallery): each
//! screen's background and the lock screen's. The keys are `[wallpaper]`
//! (`picture`, `per-screen`, `fit`, `color`, `lock`, `lock-picture`, and
//! for the wallpapers that move, #433, `animate` and `on-battery`; for
//! those that change by themselves, #894, `folder`, `every`, `order`,
//! `day`, `night`, `day-starts` and `night-starts`) and `[outputs.NAME]
//! wallpaper` (and `wallpaper-folder`, `wallpaper-day`,
//! `wallpaper-night`); the compositor follows them live, the shell draws
//! the lock from them.
//!
//! The page's own pieces (`src/app/wallpaper_page.rs`): the preview, the
//! gallery and the lock's choices; `fit` and `color` are rows of the
//! window's. Everything slow is done by the page's thread
//! ([`start`]): the thumbnails drawn with the compositor's function
//! ([`hocus_link::wallpaper::render`]), the folder of one's own pictures
//! listed, the desktop portal's file chooser asked, the chosen picture
//! checked and copied, a picture removed and put back.

use hocus_words::settings::wallpaper as words;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
    time::SystemTime,
};

use hocus_link::{
    backdrop::Backdrop,
    moving::Moving,
    theme::AccentColours,
    wallpaper::{Fit, LockLook, Look, Picture, SWATCHES},
};
use iced::widget::image::Handle;
use toml_edit::{DocumentMut, Item};

use crate::{
    lang::Lang,
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value, one},
};

pub const PREVIEW: RowGroup = RowGroup {
    label: words::PREVIEW,
    per_source: false,
};
pub const BACKGROUNDS: RowGroup = RowGroup {
    label: words::BACKGROUNDS,
    per_source: false,
};
pub const ANIMATION: RowGroup = RowGroup {
    label: words::ANIMATION,
    per_source: false,
};
pub const LOCK: RowGroup = RowGroup {
    label: words::LOCK_SCREEN,
    per_source: false,
};
/// A folder's pictures in turn (#894).
pub const FOLDER_GROUP: RowGroup = RowGroup {
    label: words::THE_FOLDER,
    per_source: false,
};
/// A day's picture and a night's (#894).
pub const DAY_NIGHT_GROUP: RowGroup = RowGroup {
    label: words::DAY_AND_NIGHT,
    per_source: false,
};

/// The ids of the page's rows.
pub const PER_SCREEN: &str = "wallpaper.per-screen";
pub const PICTURE: &str = "wallpaper.picture";
pub const BUILTINS: &str = "wallpaper.builtins";
pub const CHOOSE: &str = "wallpaper.choose";
pub const FIT: &str = "wallpaper.fit";
pub const COLOR: &str = "wallpaper.color";
pub const LOCK_ID: &str = "wallpaper.lock";
pub const LOCK_PICTURE: &str = "wallpaper.lock-picture";
/// The wallpapers that move (#433): their tiles, and the two rows.
pub const MOVING: &str = "wallpaper.moving";
pub const ANIMATE: &str = "wallpaper.animate";
pub const ON_BATTERY: &str = "wallpaper.on-battery";
const ON_BATTERY_WORDS: &[&str] = &["half", "still", "full"];
/// A folder's pictures in turn, and a day's and a night's (#894).
pub const FOLDER: &str = "wallpaper.folder";
pub const EVERY: &str = "wallpaper.every";
pub const ORDER: &str = "wallpaper.order";
pub const NEXT: &str = "wallpaper.next";
pub const DAY_NIGHT: &str = "wallpaper.day-night";
pub const SWITCH: &str = "wallpaper.switch";
pub const DAY_STARTS: &str = "wallpaper.day-starts";
pub const NIGHT_STARTS: &str = "wallpaper.night-starts";
const EVERY_WORDS: &[&str] = &["15m", "1h", "1d", "session"];
const ORDER_WORDS: &[&str] = &["shuffle", "name"];
/// When the day and the night can begin, every half hour.
const MORNINGS: &[&str] = &[
    "05:00", "05:30", "06:00", "06:30", "07:00", "07:30", "08:00", "08:30", "09:00", "09:30",
    "10:00",
];
const EVENINGS: &[&str] = &[
    "17:00", "17:30", "18:00", "18:30", "19:00", "19:30", "20:00", "20:30", "21:00", "21:30",
    "22:00", "22:30", "23:00",
];

fn hours(list: &'static [&'static str]) -> Kind {
    Kind::Menu(
        list.iter()
            .map(|h| Choice {
                label: hocus_words::Text::same(h),
                value: Value::Word(h),
            })
            .collect(),
    )
}

const FITS: &[&str] = &["cover", "contain", "center"];
const LOCKS: &[&str] = &["dimmed", "blurred", "picture"];

fn page(key: &'static [&'static str]) -> Places {
    Places::Page(Place {
        key,
        codec: Codec::Text,
        default: Value::Text(String::new()),
    })
}

/// The page's settings, in its order: what the search finds; the page
/// draws them itself, but `fit` and `color`, rows of the window's.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let own = |id, group, name, keywords, key| Setting {
            id,
            section: SectionId::Wallpaper,
            group,
            name,
            line: None,
            keywords,
            kind: Kind::Text,
            places: page(key),
            needs: None,
        };
        vec![
            own(
                PER_SCREEN,
                PREVIEW,
                words::A_BACKGROUND_PER_SCREEN,
                (
                    &["chaque écran", "tous les écrans", "moniteur"],
                    &["each screen", "all screens", "monitor", "per-screen"],
                ),
                &["wallpaper", "per-screen"],
            ),
            own(
                PICTURE,
                BACKGROUNDS,
                words::DESKTOP_BACKGROUND,
                (
                    &[
                        "image",
                        "photo",
                        "arrière-plan",
                        "papier peint",
                        "fond d’écran",
                    ],
                    &["wallpaper", "background", "picture", "photo"],
                ),
                &["wallpaper", "picture"],
            ),
            own(
                BUILTINS,
                BACKGROUNDS,
                words::BUILT_IN_BACKGROUNDS,
                (
                    &["nuit", "brume", "aube", "forêt", "dégradé"],
                    &["night", "mist", "dawn", "forest", "gradient"],
                ),
                &["wallpaper", "picture"],
            ),
            own(
                MOVING,
                BACKGROUNDS,
                words::ANIMATED_BACKGROUNDS,
                (
                    &[
                        "animé",
                        "animés",
                        "pluie",
                        "étoiles",
                        "aurore",
                        "matrix",
                        "givre",
                        "mouvement",
                    ],
                    &[
                        "animated",
                        "moving",
                        "rain",
                        "stars",
                        "aurora",
                        "matrix",
                        "frost",
                        "starfield",
                    ],
                ),
                &["wallpaper", "picture"],
            ),
            own(
                CHOOSE,
                BACKGROUNDS,
                words::CHOOSE_A_PICTURE,
                (&["fichier", "ajouter", "photo"], &["file", "add", "photo"]),
                &["wallpaper", "picture"],
            ),
            Setting {
                id: FIT,
                section: SectionId::Wallpaper,
                group: BACKGROUNDS,
                name: words::FIT,
                line: None,
                keywords: (
                    &["couvrir", "ajuster", "centrer", "recadrer"],
                    &["fit", "cover", "contain", "centre", "crop"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: words::COVER_THE_SCREEN,
                        value: Value::Word("cover"),
                    },
                    Choice {
                        label: words::THE_WHOLE_PICTURE,
                        value: Value::Word("contain"),
                    },
                    Choice {
                        label: words::CENTRE_UNSCALED,
                        value: Value::Word("center"),
                    },
                ]),
                places: one(&["wallpaper", "fit"], Codec::Word(FITS)),
                needs: None,
            },
            Setting {
                id: COLOR,
                section: SectionId::Wallpaper,
                group: BACKGROUNDS,
                name: words::SOLID_COLOUR,
                line: None,
                keywords: (&["couleur", "uni", "unie"], &["color", "colour", "plain"]),
                kind: Kind::Menu(
                    SWATCHES
                        .iter()
                        .enumerate()
                        .map(|(i, (rgb, _, _))| Choice {
                            label: hocus_words::Text::per_lang(|lang| {
                                hocus_words::link::wallpaper::SWATCHES
                                    .get(lang)
                                    .get(i)
                                    .copied()
                                    .unwrap_or_default()
                            }),
                            value: Value::Text(hocus_link::wallpaper::colour_word(*rgb)),
                        })
                        .collect(),
                ),
                places: one(&["wallpaper", "color"], Codec::Colour),
                needs: None,
            },
            Setting {
                id: ANIMATE,
                section: SectionId::Wallpaper,
                group: ANIMATION,
                name: words::ANIMATE_THE_BACKGROUND,
                line: Some(words::THE_RAIN_AT_15_FRAMES_A),
                keywords: (
                    &["animé", "animation", "pluie", "étoiles", "mouvement"],
                    &["animated", "animation", "rain", "stars", "moving"],
                ),
                kind: Kind::Switch,
                places: one(&["wallpaper", "animate"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: ON_BATTERY,
                section: SectionId::Wallpaper,
                group: ANIMATION,
                name: words::ON_BATTERY,
                line: None,
                keywords: (
                    &["batterie", "animé"],
                    &["battery", "power", "saving", "animated"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: words::SLOWED_HALF,
                        value: Value::Word("half"),
                    },
                    Choice {
                        label: words::STOPPED,
                        value: Value::Word("still"),
                    },
                    Choice {
                        label: words::AS_ON_MAINS_POWER,
                        value: Value::Word("full"),
                    },
                ]),
                places: one(&["wallpaper", "on-battery"], Codec::Word(ON_BATTERY_WORDS)),
                needs: None,
            },
            own(
                FOLDER,
                FOLDER_GROUP,
                words::WALLPAPER_FOLDER,
                (
                    &["dossier", "diaporama", "défiler", "images", "changer"],
                    &["folder", "slideshow", "rotate", "pictures", "change"],
                ),
                &["wallpaper", "folder"],
            ),
            Setting {
                id: EVERY,
                section: SectionId::Wallpaper,
                group: FOLDER_GROUP,
                name: words::CHANGE_PICTURE,
                line: None,
                keywords: (
                    &["dossier", "diaporama", "intervalle", "minutes", "heure"],
                    &["folder", "slideshow", "interval", "minutes", "hour"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: words::EVERY_15_MINUTES,
                        value: Value::Word("15m"),
                    },
                    Choice {
                        label: words::EVERY_HOUR,
                        value: Value::Word("1h"),
                    },
                    Choice {
                        label: words::EVERY_DAY,
                        value: Value::Word("1d"),
                    },
                    Choice {
                        label: words::EACH_TIME_A_SESSION_OPENS,
                        value: Value::Word("session"),
                    },
                ]),
                places: one(&["wallpaper", "every"], Codec::Word(EVERY_WORDS)),
                needs: None,
            },
            Setting {
                id: ORDER,
                section: SectionId::Wallpaper,
                group: FOLDER_GROUP,
                name: words::ORDER,
                line: None,
                keywords: (
                    &["dossier", "hasard", "aléatoire", "nom"],
                    &["folder", "shuffle", "random", "name"],
                ),
                kind: Kind::Segmented(vec![
                    Choice {
                        label: words::SHUFFLED,
                        value: Value::Word("shuffle"),
                    },
                    Choice {
                        label: words::BY_NAME,
                        value: Value::Word("name"),
                    },
                ]),
                places: one(&["wallpaper", "order"], Codec::Word(ORDER_WORDS)),
                needs: None,
            },
            own(
                NEXT,
                FOLDER_GROUP,
                words::NEXT_PICTURE,
                (
                    &["suivante", "dossier", "passer"],
                    &["next", "folder", "skip"],
                ),
                &["wallpaper", "folder"],
            ),
            own(
                DAY_NIGHT,
                DAY_NIGHT_GROUP,
                words::DAY_AND_NIGHT_PICTURES,
                (
                    &["jour", "nuit", "soir", "matin"],
                    &["day", "night", "evening", "morning"],
                ),
                &["wallpaper", "day"],
            ),
            own(
                SWITCH,
                DAY_NIGHT_GROUP,
                words::DAY_AND_NIGHT_SWITCH,
                (
                    &["soleil", "lever", "coucher", "bascule"],
                    &["sun", "sunrise", "sunset", "switch"],
                ),
                &["wallpaper", "night"],
            ),
            Setting {
                id: DAY_STARTS,
                section: SectionId::Wallpaper,
                group: DAY_NIGHT_GROUP,
                name: words::DAY_BEGINS_AT,
                line: None,
                keywords: (&["jour", "matin", "heure"], &["day", "morning", "time"]),
                kind: hours(MORNINGS),
                places: one(&["wallpaper", "day-starts"], Codec::Word(MORNINGS)),
                needs: None,
            },
            Setting {
                id: NIGHT_STARTS,
                section: SectionId::Wallpaper,
                group: DAY_NIGHT_GROUP,
                name: words::NIGHT_BEGINS_AT,
                line: None,
                keywords: (&["nuit", "soir", "heure"], &["night", "evening", "time"]),
                kind: hours(EVENINGS),
                places: one(&["wallpaper", "night-starts"], Codec::Word(EVENINGS)),
                needs: None,
            },
            Setting {
                id: LOCK_ID,
                section: SectionId::Wallpaper,
                group: LOCK,
                name: words::LOCK_SCREEN_2,
                line: None,
                keywords: (
                    &["verrouiller", "flou", "assombri", "verrouillage"],
                    &["lock", "blur", "blurred", "dimmed"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: words::THE_DESKTOP_S_BACKGROUND_DIMMED,
                        value: Value::Word("dimmed"),
                    },
                    Choice {
                        label: words::THE_DESKTOP_S_BACKGROUND_BLURRED,
                        value: Value::Word("blurred"),
                    },
                    Choice {
                        label: words::ANOTHER_PICTURE,
                        value: Value::Word("picture"),
                    },
                ]),
                places: one(&["wallpaper", "lock"], Codec::Word(LOCKS)),
                needs: None,
            },
            own(
                LOCK_PICTURE,
                LOCK,
                words::LOCK_SCREEN_PICTURE,
                (
                    &["verrouillage", "image"],
                    &["lock", "picture", "lock-picture"],
                ),
                &["wallpaper", "lock-picture"],
            ),
        ]
    })
}

/// What the file says of the wallpaper: the words as written (a word not
/// understood stays as it is, for the page's warning).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct File {
    pub picture: Option<String>,
    pub per_screen: bool,
    pub lock: Option<String>,
    pub lock_picture: Option<String>,
    pub fit: Option<String>,
    pub color: Option<String>,
    /// `[outputs.NAME] wallpaper`, by name.
    pub outputs: Vec<(String, String)>,
    /// A folder's pictures and a day's and a night's (#894): every
    /// screen's, and the screens' own by name.
    pub changing: Changing,
    pub own: Vec<(String, Changing)>,
    /// `[weather]`'s place: its name, latitude and longitude.
    pub place: Option<WeatherPlace>,
}

/// A place's name, latitude and longitude (degrees).
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherPlace(pub String, pub f64, pub f64);

// The file's numbers: never NaN where a place is read.
impl Eq for WeatherPlace {}

/// The keys of a wallpaper that changes by itself, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changing {
    pub folder: Option<String>,
    pub day: Option<String>,
    pub night: Option<String>,
}

impl Changing {
    pub fn day(&self) -> Picture {
        self.day
            .as_deref()
            .and_then(|p| Picture::of(p).ok())
            .filter(|p| !p.changes())
            .unwrap_or(Picture::Builtin(Backdrop::Dawn))
    }

    pub fn night(&self) -> Picture {
        self.night
            .as_deref()
            .and_then(|p| Picture::of(p).ok())
            .filter(|p| !p.changes())
            .unwrap_or_default()
    }

    /// The folder, as written; none when empty.
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref().filter(|f| !f.trim().is_empty())
    }
}

impl File {
    pub fn of(doc: &DocumentMut) -> Self {
        let word = |key: &str| {
            doc.get("wallpaper")
                .and_then(|t| t.get(key))
                .and_then(Item::as_str)
                .map(ToOwned::to_owned)
        };
        Self {
            picture: word("picture"),
            per_screen: doc
                .get("wallpaper")
                .and_then(|t| t.get("per-screen"))
                .and_then(Item::as_bool)
                .unwrap_or(false),
            lock: word("lock"),
            lock_picture: word("lock-picture"),
            fit: word("fit"),
            color: word("color"),
            outputs: doc
                .get("outputs")
                .and_then(Item::as_table_like)
                .map(|outputs| {
                    outputs
                        .iter()
                        .filter_map(|(name, o)| {
                            Some((name.to_owned(), o.get("wallpaper")?.as_str()?.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            changing: Changing {
                folder: word("folder"),
                day: word("day"),
                night: word("night"),
            },
            own: doc
                .get("outputs")
                .and_then(Item::as_table_like)
                .map(|outputs| {
                    outputs
                        .iter()
                        .map(|(name, o)| {
                            let key =
                                |k: &str| o.get(k).and_then(Item::as_str).map(ToOwned::to_owned);
                            (
                                name.to_owned(),
                                Changing {
                                    folder: key("wallpaper-folder"),
                                    day: key("wallpaper-day"),
                                    night: key("wallpaper-night"),
                                },
                            )
                        })
                        .filter(|(_, c)| *c != Changing::default())
                        .collect()
                })
                .unwrap_or_default(),
            place: doc.get("weather").and_then(|w| {
                let place = w.get("place")?.as_str()?.trim().to_owned();
                let number = |k: &str| {
                    let v = w.get(k)?;
                    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
                };
                let (lat, lon) = (number("latitude")?, number("longitude")?);
                (!place.is_empty() && lat.is_finite() && lon.is_finite())
                    .then_some(WeatherPlace(place, lat, lon))
            }),
        }
    }

    /// What changes by itself on `output` (every screen with `None`):
    /// its own keys with per-screen, else every screen's, each key.
    pub fn changing_of(&self, output: Option<&str>) -> Changing {
        let own = output
            .filter(|_| self.per_screen)
            .filter(|o| self.outputs.iter().any(|(name, _)| name == o))
            .and_then(|o| self.own.iter().find(|(name, _)| name == o))
            .map(|(_, c)| c.clone())
            .unwrap_or_default();
        Changing {
            folder: own.folder.or_else(|| self.changing.folder.clone()),
            day: own.day.or_else(|| self.changing.day.clone()),
            night: own.night.or_else(|| self.changing.night.clone()),
        }
    }

    /// Every screen's picture (Nuit when it does not read).
    pub fn picture(&self) -> Picture {
        self.picture
            .as_deref()
            .and_then(|p| Picture::of(p).ok())
            .unwrap_or_default()
    }

    /// `output`'s picture: its own with per-screen, else every screen's.
    pub fn picture_of(&self, output: &str) -> Picture {
        self.per_screen
            .then(|| {
                self.outputs
                    .iter()
                    .find(|(name, _)| name == output)
                    .and_then(|(_, p)| Picture::of(p).ok())
            })
            .flatten()
            .unwrap_or_else(|| self.picture())
    }

    pub fn fit(&self) -> Fit {
        self.fit.as_deref().and_then(Fit::of).unwrap_or_default()
    }

    pub fn color(&self) -> [u8; 3] {
        self.color
            .as_deref()
            .and_then(hocus_link::wallpaper::colour_of)
            .unwrap_or(hocus_link::wallpaper::DEFAULT_COLOR)
    }

    pub fn lock(&self) -> LockLook {
        self.lock
            .as_deref()
            .and_then(LockLook::of)
            .unwrap_or_default()
    }

    pub fn lock_picture(&self) -> Picture {
        self.lock_picture
            .as_deref()
            .and_then(|p| Picture::of(p).ok())
            .unwrap_or_default()
    }

    /// What `picture` looks like with the file's fit and colour.
    pub fn look(&self, picture: Picture) -> Look {
        Look {
            picture,
            fit: self.fit(),
            color: self.color(),
        }
    }
}

/// A picture's name as a tile says it: the built-ins' names, "Couleur
/// unie", a file's name.
pub fn name_of(picture: &Picture, lang: Lang) -> String {
    match picture {
        Picture::Builtin(b) => b.name(lang).to_owned(),
        Picture::Moving(m) => m.name(lang).to_owned(),
        Picture::Plain => words::SOLID_COLOUR.get(lang).to_owned(),
        Picture::Folder => hocus_words::settings::app_wallpaper_page::A_FOLDER
            .get(lang)
            .to_owned(),
        Picture::DayNight => hocus_words::settings::app_wallpaper_page::DAY_AND_NIGHT
            .get(lang)
            .to_owned(),
        Picture::File(text) => Path::new(text)
            .file_name()
            .map_or_else(|| text.clone(), |n| n.to_string_lossy().into_owned()),
    }
}

/// The accent a background gives, when known without drawing it.
pub fn accent_of(look: &Look) -> Option<AccentColours> {
    match look.picture {
        Picture::Builtin(b) => Some(b.accent()),
        Picture::Moving(m) => Some(m.accent()),
        Picture::Plain => Some(AccentColours::lifted(look.color)),
        Picture::File(_) | Picture::Folder | Picture::DayNight => None,
    }
}

/// A path as the file writes it: under the home with `~/`.
pub fn written(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// One of one's own pictures, in the wallpapers' folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mine {
    pub path: PathBuf,
    pub name: String,
    /// As the file writes it.
    pub word: String,
}

/// A thumbnail asked: what it shows, its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thumb {
    pub look: Look,
    pub size: (u32, u32),
    /// The lock's "blurred".
    pub soft: bool,
}

/// What a picture chosen in the file chooser is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum For {
    Desktop,
    Lock,
    /// Day and night's (#894).
    Day,
    Night,
}

/// What the page asks its thread.
#[derive(Debug)]
pub enum Ask {
    Draw(Thumb),
    List,
    /// The file chooser, for a picture.
    Pick {
        to: For,
        lang: Lang,
    },
    /// The file chooser, for a folder of pictures (#894).
    PickFolder {
        lang: Lang,
    },
    /// A folder's pictures counted, its first two named (#894).
    Count(PathBuf),
    Remove(PathBuf),
    PutBack {
        path: PathBuf,
        bytes: Vec<u8>,
    },
}

/// What the thread says.
#[derive(Debug, Clone)]
pub enum Heard {
    Drawn {
        thumb: Thumb,
        handle: Handle,
        accent: AccentColours,
        failed: Option<String>,
    },
    Listed(Vec<Mine>),
    Picked {
        to: For,
        picked: Picked,
    },
    /// A folder chosen, or not (#894).
    PickedFolder(crate::portal::Chosen),
    /// `folder` holds `count` pictures, `first` its first two.
    Counted {
        folder: PathBuf,
        count: usize,
        first: Vec<PathBuf>,
    },
    Removed {
        path: PathBuf,
        bytes: std::sync::Arc<Vec<u8>>,
    },
    PutBack(PathBuf),
    Failed(String),
}

/// What came of the file chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// Copied there, its name the chosen file's.
    Copied {
        copy: PathBuf,
        name: String,
    },
    Cancelled,
    /// Not a picture: nothing written.
    NotAPicture(String),
    /// No portal answers on the bus.
    NoPortal,
    Failed(String),
}

/// The page's thread; `heard` takes what it says and returns `false` once
/// the window is gone.
#[derive(Debug, Clone)]
pub struct Worker {
    asks: mpsc::Sender<Ask>,
}

impl Worker {
    pub fn ask(&self, ask: Ask) -> bool {
        self.asks.send(ask).is_ok()
    }
}

pub fn start(heard: impl FnMut(Heard) -> bool + Send + 'static) -> Option<Worker> {
    let (asks, taken) = mpsc::channel::<Ask>();
    let started = std::thread::Builder::new()
        .name("settings: wallpaper".to_owned())
        .spawn(move || run(&taken, heard));
    match started {
        Ok(_) => Some(Worker { asks }),
        Err(err) => {
            tracing::warn!(%err, "no thread for the wallpaper's page");
            None
        }
    }
}

fn run(taken: &mpsc::Receiver<Ask>, mut heard: impl FnMut(Heard) -> bool) {
    while let Ok(first) = taken.recv() {
        let mut asks = vec![first];
        asks.extend(taken.try_iter());
        for ask in asks {
            let said = match ask {
                Ask::Draw(thumb) => {
                    let size = (thumb.size.0 as i32, thumb.size.1 as i32);
                    let mut rendered = hocus_link::wallpaper::render(&thumb.look, size);
                    if thumb.soft {
                        rendered = hocus_link::wallpaper::soften(&rendered);
                    }
                    Heard::Drawn {
                        handle: Handle::from_rgba(
                            rendered.width as u32,
                            rendered.height as u32,
                            rendered.pixels,
                        ),
                        accent: rendered.accent,
                        failed: rendered.failed,
                        thumb,
                    }
                }
                Ask::List => Heard::Listed(list()),
                Ask::Pick { to, lang } => Heard::Picked {
                    to,
                    picked: pick(to, lang),
                },
                Ask::PickFolder { lang } => Heard::PickedFolder(crate::portal::open_folder(
                    words::CHOOSE_A_FOLDER_OF_PICTURES.get(lang),
                    &pictures_folder(),
                )),
                Ask::Count(folder) => {
                    let listed = hocus_link::wallpaper::list(&folder);
                    Heard::Counted {
                        count: listed.pictures.len(),
                        first: listed.pictures.into_iter().take(2).collect(),
                        folder,
                    }
                }
                Ask::Remove(path) => match std::fs::read(&path)
                    .and_then(|bytes| std::fs::remove_file(&path).map(|()| bytes))
                {
                    Ok(bytes) => Heard::Removed {
                        path,
                        bytes: std::sync::Arc::new(bytes),
                    },
                    Err(err) => Heard::Failed(format!("{}: {err}", path.display())),
                },
                Ask::PutBack { path, bytes } => match std::fs::write(&path, bytes) {
                    Ok(()) => Heard::PutBack(path),
                    Err(err) => Heard::Failed(format!("{}: {err}", path.display())),
                },
            };
            let listed_again = matches!(
                said,
                Heard::Removed { .. }
                    | Heard::PutBack(_)
                    | Heard::Picked {
                        picked: Picked::Copied { .. },
                        ..
                    }
            );
            if !heard(said) {
                return;
            }
            if listed_again && !heard(Heard::Listed(list())) {
                return;
            }
        }
    }
}

/// The pictures of one's own, in the order they were added.
fn list() -> Vec<Mine> {
    let Some(folder) = hocus_link::wallpaper::folder() else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, Mine)> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path.extension()?.to_str()?.to_lowercase();
            if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                return None;
            }
            let meta = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let at = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((
                at,
                Mine {
                    word: written(&path),
                    name,
                    path,
                },
            ))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().map(|(_, mine)| mine).collect()
}

/// The file chooser, then the chosen picture checked and copied.
fn pick(to: For, lang: Lang) -> Picked {
    let title = match to {
        For::Desktop | For::Day | For::Night => {
            words::CHOOSE_A_PICTURE_FOR_THE_BACKGROUND.get(lang)
        }
        For::Lock => words::CHOOSE_A_PICTURE_FOR_THE_LOCK.get(lang),
    };
    let filter = words::PICTURES_JPEG_PNG_WEBP.get(lang);
    let chosen = match crate::portal::open_file(title, filter, &pictures_folder()) {
        crate::portal::Chosen::Path(path) => path,
        crate::portal::Chosen::Cancelled => return Picked::Cancelled,
        crate::portal::Chosen::NoPortal => return Picked::NoPortal,
    };
    copy(&chosen)
}

/// The user's pictures folder: `XDG_PICTURES_DIR`, else `~/Pictures`.
fn pictures_folder() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_PICTURES_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME").map_or_else(PathBuf::new, |h| Path::new(&h).join("Pictures"))
}

/// `chosen` checked (it opens as a picture: its size read from its
/// header) and copied into the wallpapers' folder, under its name, or
/// "NAME (2).jpg" when another file has it.
pub fn copy(chosen: &Path) -> Picked {
    match hocus_link::wallpaper::folder() {
        Some(folder) => copy_into(chosen, &folder),
        None => Picked::Failed("no home".to_owned()),
    }
}

fn copy_into(chosen: &Path, folder: &Path) -> Picked {
    let name = chosen
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if let Err(why) = hocus_link::wallpaper::dimensions(chosen) {
        tracing::info!(%why, "the chosen file is not a picture");
        return Picked::NotAPicture(name);
    }
    if let Err(err) = std::fs::create_dir_all(folder) {
        return Picked::Failed(format!("{}: {err}", folder.display()));
    }
    let bytes = match std::fs::read(chosen) {
        Ok(bytes) => bytes,
        Err(err) => return Picked::Failed(format!("{name}: {err}")),
    };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
        _ => (name.clone(), String::new()),
    };
    let mut n = 1;
    let copy = loop {
        let candidate = if n == 1 {
            folder.join(&name)
        } else {
            folder.join(format!("{stem} ({n}){ext}"))
        };
        match std::fs::read(&candidate) {
            // The same picture chosen again: its copy is there already.
            Ok(there) if there == bytes => break candidate,
            Ok(_) => n += 1,
            Err(_) => {
                if let Err(err) = std::fs::write(&candidate, &bytes) {
                    return Picked::Failed(format!("{}: {err}", candidate.display()));
                }
                break candidate;
            }
        }
    };
    let name = copy
        .file_name()
        .map_or(name, |n| n.to_string_lossy().into_owned());
    tracing::info!(copy = %copy.display(), "the chosen picture is copied");
    Picked::Copied { copy, name }
}

/// The thumbnails drawn so far, and the accents the pictures gave.
#[derive(Debug, Clone, Default)]
pub struct Thumbs {
    pub drawn: HashMap<Thumb, Handle>,
    pub asked: std::collections::HashSet<Thumb>,
    /// By picture's path as written: its accent, or why it does not read.
    pub pictures: HashMap<String, Result<AccentColours, String>>,
}

/// The built-ins in the gallery's order.
pub const BUILTIN_TILES: [Backdrop; 4] = Backdrop::ALL;
/// The wallpapers that move, after them.
pub const MOVING_TILES: [Moving; 3] = Moving::ALL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_says_each_screens_picture() {
        let doc: DocumentMut = "[wallpaper]\npicture = \"mist\"\nper-screen = true\n\
                                color = \"#22324d\"\n[outputs.DP-2]\nwallpaper = \"forest\"\n"
            .parse()
            .unwrap_or_default();
        let file = File::of(&doc);
        assert_eq!(file.picture_of("DP-2"), Picture::Builtin(Backdrop::Forest));
        assert_eq!(file.picture_of("eDP-1"), Picture::Builtin(Backdrop::Mist));
        assert_eq!(file.color(), [0x22, 0x32, 0x4D]);
        let off = File {
            per_screen: false,
            ..file
        };
        assert_eq!(off.picture_of("DP-2"), Picture::Builtin(Backdrop::Mist));
    }

    #[test]
    fn the_file_says_each_screens_folder_and_day_and_night() {
        let doc: DocumentMut = "[wallpaper]\npicture = \"folder\"\nfolder = \"~/Images/Fonds\"\n\
                                per-screen = true\nnight = \"forest\"\n\
                                [outputs.DP-2]\nwallpaper = \"day-night\"\nwallpaper-day = \"mist\"\n\
                                [weather]\nplace = \"Lyon\"\nlatitude = 45.76\nlongitude = 4.84\n"
            .parse()
            .unwrap_or_default();
        let file = File::of(&doc);
        assert_eq!(file.picture(), Picture::Folder);
        assert_eq!(file.picture_of("DP-2"), Picture::DayNight);
        let every = file.changing_of(None);
        assert_eq!(every.folder(), Some("~/Images/Fonds"));
        assert_eq!(every.day(), Picture::Builtin(Backdrop::Dawn));
        let own = file.changing_of(Some("DP-2"));
        assert_eq!(own.day(), Picture::Builtin(Backdrop::Mist));
        assert_eq!(own.night(), Picture::Builtin(Backdrop::Forest));
        assert_eq!(
            file.place,
            Some(WeatherPlace("Lyon".to_owned(), 45.76, 4.84))
        );
    }

    #[test]
    fn a_copy_takes_a_number_when_its_name_is_taken() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let folder = dir.path().join("hocuspanda/wallpapers");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("Lac.png"), b"another").map_err(|e| e.to_string())?;
        let chosen = dir.path().join("Lac.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(&chosen)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            copy_into(&chosen, &folder),
            Picked::Copied {
                copy: folder.join("Lac (2).png"),
                name: "Lac (2).png".to_owned()
            }
        );
        // Chosen again: the same copy.
        assert_eq!(
            copy_into(&chosen, &folder),
            Picked::Copied {
                copy: folder.join("Lac (2).png"),
                name: "Lac (2).png".to_owned()
            }
        );
        let text = dir.path().join("notes.png");
        std::fs::write(&text, b"not a picture").map_err(|e| e.to_string())?;
        assert_eq!(
            copy_into(&text, &folder),
            Picked::NotAPicture("notes.png".to_owned())
        );
        Ok(())
    }
}
