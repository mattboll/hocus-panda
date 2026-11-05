//! Fond d'écran / Wallpaper (6.3, issue #403, variant A, the gallery): each
//! screen's background and the lock screen's. The keys are `[wallpaper]`
//! (`picture`, `per-screen`, `fit`, `color`, `lock`, `lock-picture`) and
//! `[outputs.NAME] wallpaper`; the compositor follows them live, the shell
//! draws the lock from them.
//!
//! The page's own pieces (`src/app/wallpaper_page.rs`): the preview, the
//! gallery and the lock's choices; `fit` and `color` are rows of the
//! window's. Everything slow is done by the page's thread
//! ([`start`]): the thumbnails drawn with the compositor's function
//! ([`hocus_link::wallpaper::render`]), the folder of one's own pictures
//! listed, the desktop portal's file chooser asked, the chosen picture
//! checked and copied, a picture removed and put back.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
    time::SystemTime,
};

use hocus_link::{
    backdrop::Backdrop,
    theme::AccentColours,
    wallpaper::{Fit, LockLook, Look, Picture, SWATCHES},
};
use iced::widget::image::Handle;
use toml_edit::{DocumentMut, Item};

use crate::{
    lang::{Lang, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

pub const PREVIEW: RowGroup = RowGroup {
    label: w("APERÇU", "PREVIEW"),
    per_source: false,
};
pub const BACKGROUNDS: RowGroup = RowGroup {
    label: w("FONDS", "BACKGROUNDS"),
    per_source: false,
};
pub const LOCK: RowGroup = RowGroup {
    label: w("ÉCRAN DE VERROUILLAGE", "LOCK SCREEN"),
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
                w("Un fond par écran", "A background per screen"),
                (
                    &["chaque écran", "tous les écrans", "moniteur"],
                    &["each screen", "all screens", "monitor", "per-screen"],
                ),
                &["wallpaper", "per-screen"],
            ),
            own(
                PICTURE,
                BACKGROUNDS,
                w("Fond du bureau", "Desktop background"),
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
                w("Fonds fournis", "Built-in backgrounds"),
                (
                    &["nuit", "brume", "aube", "forêt", "dégradé"],
                    &["night", "mist", "dawn", "forest", "gradient"],
                ),
                &["wallpaper", "picture"],
            ),
            own(
                CHOOSE,
                BACKGROUNDS,
                w("Choisir une image", "Choose a picture"),
                (&["fichier", "ajouter", "photo"], &["file", "add", "photo"]),
                &["wallpaper", "picture"],
            ),
            Setting {
                id: FIT,
                section: SectionId::Wallpaper,
                group: BACKGROUNDS,
                name: w("Remplissage", "Fit"),
                line: None,
                keywords: (
                    &["couvrir", "ajuster", "centrer", "recadrer"],
                    &["fit", "cover", "contain", "centre", "crop"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: w("Couvrir l’écran", "Cover the screen"),
                        value: Value::Word("cover"),
                    },
                    Choice {
                        label: w("L’image entière", "The whole picture"),
                        value: Value::Word("contain"),
                    },
                    Choice {
                        label: w("Centrer, sans agrandir", "Centre, unscaled"),
                        value: Value::Word("center"),
                    },
                ]),
                places: Places::One(Place {
                    key: &["wallpaper", "fit"],
                    codec: Codec::Word(FITS),
                    default: Value::Word("cover"),
                }),
                needs: None,
            },
            Setting {
                id: COLOR,
                section: SectionId::Wallpaper,
                group: BACKGROUNDS,
                name: w("Couleur unie", "Solid colour"),
                line: None,
                keywords: (&["couleur", "uni", "unie"], &["color", "colour", "plain"]),
                kind: Kind::Menu(
                    SWATCHES
                        .iter()
                        .map(|(rgb, fr, en)| Choice {
                            label: w(fr, en),
                            value: Value::Text(hocus_link::wallpaper::colour_word(*rgb)),
                        })
                        .collect(),
                ),
                places: Places::One(Place {
                    key: &["wallpaper", "color"],
                    codec: Codec::Colour,
                    default: Value::Text(hocus_link::wallpaper::colour_word(
                        hocus_link::wallpaper::DEFAULT_COLOR,
                    )),
                }),
                needs: None,
            },
            Setting {
                id: LOCK_ID,
                section: SectionId::Wallpaper,
                group: LOCK,
                name: w("Écran de verrouillage", "Lock screen"),
                line: None,
                keywords: (
                    &["verrouiller", "flou", "assombri", "verrouillage"],
                    &["lock", "blur", "blurred", "dimmed"],
                ),
                kind: Kind::Menu(vec![
                    Choice {
                        label: w(
                            "Le fond du bureau, assombri",
                            "The desktop’s background, dimmed",
                        ),
                        value: Value::Word("dimmed"),
                    },
                    Choice {
                        label: w(
                            "Le fond du bureau, flouté",
                            "The desktop’s background, blurred",
                        ),
                        value: Value::Word("blurred"),
                    },
                    Choice {
                        label: w("Une autre image", "Another picture"),
                        value: Value::Word("picture"),
                    },
                ]),
                places: Places::One(Place {
                    key: &["wallpaper", "lock"],
                    codec: Codec::Word(LOCKS),
                    default: Value::Word("dimmed"),
                }),
                needs: None,
            },
            own(
                LOCK_PICTURE,
                LOCK,
                w("Image du verrouillage", "Lock screen picture"),
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
        Picture::Builtin(b) => {
            let (fr, en) = b.names();
            if lang.fr() { fr } else { en }.to_owned()
        }
        Picture::Plain => w("Couleur unie", "Solid colour").get(lang).to_owned(),
        Picture::File(text) => Path::new(text)
            .file_name()
            .map_or_else(|| text.clone(), |n| n.to_string_lossy().into_owned()),
    }
}

/// The accent a background gives, when known without drawing it.
pub fn accent_of(look: &Look) -> Option<AccentColours> {
    match look.picture {
        Picture::Builtin(b) => Some(b.accent()),
        Picture::Plain => Some(AccentColours::lifted(look.color)),
        Picture::File(_) => None,
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

/// What the page asks its thread.
#[derive(Debug)]
pub enum Ask {
    Draw(Thumb),
    List,
    /// The file chooser, for the desktop (`false`) or the lock.
    Pick {
        lock: bool,
        lang: Lang,
    },
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
        lock: bool,
        picked: Picked,
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
                Ask::Pick { lock, lang } => Heard::Picked {
                    lock,
                    picked: pick(lock, lang),
                },
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
fn pick(lock: bool, lang: Lang) -> Picked {
    let title = match (lock, lang) {
        (false, Lang::French) => "Choisir une image pour le fond",
        (false, Lang::English) => "Choose a picture for the background",
        (true, Lang::French) => "Choisir une image pour le verrouillage",
        (true, Lang::English) => "Choose a picture for the lock screen",
    };
    let filter = match lang {
        Lang::French => "Images (JPEG, PNG, WebP)",
        Lang::English => "Pictures (JPEG, PNG, WebP)",
    };
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
        tracing::info!(why, "the chosen file is not a picture");
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
