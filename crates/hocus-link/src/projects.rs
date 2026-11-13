//! Project desktops (docs/design/projects.md, #789): a desktop given a name
//! and a folder. What the compositor and the shell both know of one: its
//! monogram, the name's rules, how its folder is shown; and what the
//! messages about the form that makes one and the card that says what it
//! holds carry.

use serde::{Deserialize, Serialize};

use crate::{Rect, text::tidy};

/// The form that makes a desktop a project (§2.1), under its thumbnail in
/// the overview: a layer surface of the shell's, moved as the emoji picker
/// is.
pub const PROJECT_FORM_NAMESPACE: &str = "hocus-project-form";
/// The card that says what a project holds (§4), under its thumbnail.
pub const PROJECT_CARD_NAMESPACE: &str = "hocus-project-card";
/// Their corners (the mockup's).
pub const PROJECT_FORM_RADIUS: f64 = 14.0;
pub const PROJECT_CARD_RADIUS: f64 = 16.0;
/// Their widths, logical pixels.
pub const PROJECT_FORM_WIDTH: u32 = 400;
pub const PROJECT_CARD_WIDTH: u32 = 600;
/// They come down this far as they open (the mockup's `pop`).
pub const PROJECT_RISE: f64 = 6.0;
/// A name's characters at most (§1).
pub const NAME_MOST: usize = 40;

/// Whether `namespace` is one of the projects' surfaces.
pub fn is_project_surface(namespace: &str) -> bool {
    matches!(namespace, PROJECT_FORM_NAMESPACE | PROJECT_CARD_NAMESPACE)
}

/// A desktop that is a project, as a bar's tiles show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskProject {
    /// From 0.
    pub desktop: u8,
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
    /// Its folder, absolute.
    #[serde(serialize_with = "tidy::name")]
    pub folder: String,
}

impl DeskProject {
    pub fn monogram(&self) -> String {
        monogram(&self.name)
    }
}

/// A project's monogram (§1): two capitals, the first letters of its
/// first two words (split at `-`, `_`, `.` and spaces), else its first two
/// letters.
pub fn monogram(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| matches!(c, '-' | '_' | '.') || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        [one] => one.chars().take(2).collect(),
        [] => String::new(),
    };
    letters.to_uppercase()
}

/// A name as names are compared (§1): case and accents ignored.
pub fn name_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    name.trim()
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// What is wrong with a name, if something is (§1): 1 to 40 characters,
/// spaces around it aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameFault {
    Empty,
    TooLong,
}

pub fn name_fault(name: &str) -> Option<NameFault> {
    let name = name.trim();
    if name.is_empty() {
        Some(NameFault::Empty)
    } else if name.chars().count() > NAME_MOST {
        Some(NameFault::TooLong)
    } else {
        None
    }
}

/// A project's file's id (§9): its name in lower case, other characters
/// `-`, `-2` and on when `taken` says it is.
pub fn id_of(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut id = String::new();
    for c in name_key(name).chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_matches('-');
    let id = if id.is_empty() { "project" } else { id };
    if !taken(id) {
        return id.to_owned();
    }
    (2..)
        .map(|n| format!("{id}-{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| id.to_owned())
}

/// A folder as it is shown: under `home`, from `~`.
pub fn tilde(folder: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return folder.to_owned();
    }
    match folder.strip_prefix(home) {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => folder.to_owned(),
    }
}

/// The form opens (§2.1), under desktop `desktop`'s thumbnail (`at`,
/// logical pixels from its screen's top left corner) on screen `output`,
/// `screen` large; `terminals`, the processes of the terminals on that
/// desktop, whose folders come first (§2.3); `taken`, the projects' names
/// (none of them is free); `folders`, their folders (none of them is
/// listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectForm {
    pub output: String,
    pub desktop: u8,
    pub at: Rect,
    pub screen: (i32, i32),
    pub terminals: Vec<u32>,
    #[serde(serialize_with = "tidy::names")]
    pub taken: Vec<String>,
    #[serde(serialize_with = "tidy::names")]
    pub folders: Vec<String>,
}

/// Why a project was not made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectRefusal {
    /// Another project is named so (its name as it is written).
    NameTaken(#[serde(serialize_with = "tidy::name")] String),
    Name(NameFault),
    /// The folder is not an absolute path to a directory.
    NoFolder,
    /// The desktop is a project already, or is not there.
    Desktop,
}

/// Where a window is on its desktop, as a project's card says it (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowPlace {
    Floating,
    Maximized,
    Fullscreen,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Minimized,
}

/// A project's card opens (§4), under desktop `desktop`'s thumbnail (`at`)
/// on screen `output`, `screen` large: its name and folder, since when it
/// is open (seconds of the Unix epoch: the user reads it), and where each
/// of its windows is, front first. Their titles, applications and
/// sketches are the desktop's, as the bar's tiles have them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCard {
    pub output: String,
    pub desktop: u8,
    pub at: Rect,
    pub screen: (i32, i32),
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
    #[serde(serialize_with = "tidy::name")]
    pub folder: String,
    pub since: i64,
    pub windows: Vec<(u64, WindowPlace)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monogram_is_two_words_initials_else_two_letters() {
        assert_eq!(monogram("hocus-site"), "HS");
        assert_eq!(monogram("hocus_panda tools"), "HP");
        assert_eq!(monogram("api.meteo"), "AM");
        assert_eq!(monogram("blog"), "BL");
        assert_eq!(monogram("x"), "X");
        assert_eq!(monogram("  été  "), "ÉT");
        assert_eq!(monogram("--"), "");
    }

    #[test]
    fn names_are_one_to_forty_characters_compared_without_case_or_accents() {
        assert_eq!(name_fault(""), Some(NameFault::Empty));
        assert_eq!(name_fault("   "), Some(NameFault::Empty));
        assert_eq!(name_fault(&"é".repeat(40)), None);
        assert_eq!(name_fault(&"a".repeat(41)), Some(NameFault::TooLong));
        assert_eq!(name_key("Été-Site"), name_key("ete-site"));
        assert_ne!(name_key("hocus-site"), name_key("hocus-sites"));
    }

    #[test]
    fn an_id_is_the_name_in_lower_case_then_numbered() {
        assert_eq!(id_of("Hocus Site", |_| false), "hocus-site");
        assert_eq!(id_of("Été/2026", |_| false), "ete-2026");
        assert_eq!(id_of("hocus-site", |id| id == "hocus-site"), "hocus-site-2");
        assert_eq!(
            id_of("hocus-site", |id| id == "hocus-site"
                || id == "hocus-site-2"),
            "hocus-site-3"
        );
        assert_eq!(id_of("💡", |_| false), "project");
    }

    #[test]
    fn a_folder_under_home_starts_with_a_tilde() {
        assert_eq!(
            tilde("/home/alex/github/hocus", "/home/alex"),
            "~/github/hocus"
        );
        assert_eq!(tilde("/home/alex", "/home/alex/"), "~");
        assert_eq!(
            tilde("/home/alexandre/x", "/home/alex"),
            "/home/alexandre/x"
        );
        assert_eq!(tilde("/srv/x", ""), "/srv/x");
    }
}
