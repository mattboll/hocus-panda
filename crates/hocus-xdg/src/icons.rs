//! Icons (freedesktop's Icon Theme Specification), as the launcher finds
//! them: in the user's icon theme, then the themes it inherits, then
//! `hicolor`, then `pixmaps`, at a size and a scale. Only where the file
//! is: drawing it is the caller's (the launcher's tiles, the Settings'
//! applications).
//!
//! The user's theme is GTK's setting (`gtk-icon-theme-name` in
//! `gtk-4.0/settings.ini`, then `gtk-3.0`'s), else KDE's (`kdeglobals`'
//! `[Icons] Theme`), else GTK's own default, Adwaita, when it is there. A
//! theme without an `index.theme` (one copied by hand) is read from its
//! directories' names (`48x48`, `48x48@2`, `scalable`).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

/// A theme's directory of icons, as its `index.theme` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dir {
    path: String,
    size: u32,
    scale: u32,
    kind: Kind,
    min: u32,
    max: u32,
    threshold: u32,
}

impl Dir {
    /// Whether it holds icons for `size` at `scale`.
    fn matches(&self, size: u32, scale: u32) -> bool {
        if self.scale != scale {
            return false;
        }
        match self.kind {
            Kind::Fixed => self.size == size,
            Kind::Scalable => (self.min..=self.max).contains(&size),
            Kind::Threshold => {
                size.saturating_sub(self.threshold) <= self.size
                    && self.size <= size + self.threshold
            }
        }
    }

    /// The sizes of its icons, in pixels: the least and the most.
    fn pixels(&self) -> (u32, u32) {
        match self.kind {
            Kind::Fixed => (self.size * self.scale, self.size * self.scale),
            Kind::Scalable => (self.min * self.scale, self.max * self.scale),
            Kind::Threshold => (
                self.size.saturating_sub(self.threshold) * self.scale,
                (self.size + self.threshold) * self.scale,
            ),
        }
    }

    /// How far its icons are from `size` at `scale`, in pixels.
    fn distance(&self, size: u32, scale: u32) -> u32 {
        let wanted = size * scale;
        let (low, high) = self.pixels();
        if wanted < low {
            low - wanted
        } else {
            wanted.saturating_sub(high)
        }
    }

    /// How good its icons are for `size` at `scale` when none is that
    /// size, the best least: larger ones first, scaled down they stay
    /// crisp (a 72 px tile takes 128 px icons before 64 px ones, #518),
    /// then the closest.
    fn rank(&self, size: u32, scale: u32) -> (bool, u32) {
        let smaller = self.pixels().1 < size * scale;
        (smaller, self.distance(size, scale))
    }

    /// One from a directory's name alone: `48x48` (or `48`), `48x48@2`,
    /// `scalable`, then its context (`48x48/apps`).
    fn guessed(path: &str) -> Option<Self> {
        let first = path.split('/').next()?;
        if first == "scalable" {
            return Some(Self {
                path: path.to_owned(),
                size: 48,
                scale: 1,
                kind: Kind::Scalable,
                min: 1,
                max: 1024,
                threshold: 0,
            });
        }
        let (dims, scale) = match first.split_once('@') {
            Some((dims, scale)) => (dims, scale.parse().ok()?),
            None => (first, 1),
        };
        let size: u32 = dims.split('x').next()?.parse().ok()?;
        Some(Self {
            path: path.to_owned(),
            size,
            scale,
            kind: Kind::Fixed,
            min: size,
            max: size,
            threshold: 0,
        })
    }
}

/// An icon theme: its directories and the themes it inherits.
#[derive(Debug, Clone, Default)]
struct Theme {
    dirs: Vec<Dir>,
    inherits: Vec<String>,
}

/// An `index.theme`'s sections, keys and values.
fn ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_owned();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(section.clone())
                .or_default()
                .insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    sections
}

fn names(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Theme {
    /// From its `index.theme`.
    fn read(text: &str) -> Self {
        let sections = ini(text);
        let head = sections.get("Icon Theme");
        let mut paths = names(head.and_then(|h| h.get("Directories")));
        paths.extend(names(head.and_then(|h| h.get("ScaledDirectories"))));
        let number = |s: Option<&HashMap<String, String>>, key: &str| -> Option<u32> {
            s?.get(key)?.parse().ok()
        };
        let dirs = paths
            .into_iter()
            .filter_map(|path| {
                let s = sections.get(&path);
                let size = number(s, "Size")?;
                let kind = match s.and_then(|s| s.get("Type")).map(String::as_str) {
                    Some("Fixed") => Kind::Fixed,
                    Some("Scalable") => Kind::Scalable,
                    _ => Kind::Threshold,
                };
                Some(Dir {
                    size,
                    scale: number(s, "Scale").unwrap_or(1),
                    kind,
                    min: number(s, "MinSize").unwrap_or(size),
                    max: number(s, "MaxSize").unwrap_or(size),
                    threshold: number(s, "Threshold").unwrap_or(2),
                    path,
                })
            })
            .collect();
        Self {
            dirs,
            inherits: names(head.and_then(|h| h.get("Inherits"))),
        }
    }

    /// From its directories' names, under each of `roots`.
    fn guessed(roots: &[PathBuf], name: &str) -> Self {
        let mut dirs = Vec::new();
        let mut seen = HashSet::new();
        for root in roots {
            let Ok(sizes) = std::fs::read_dir(root.join(name)) else {
                continue;
            };
            for size in sizes.flatten() {
                let Ok(contexts) = std::fs::read_dir(size.path()) else {
                    continue;
                };
                for context in contexts.flatten() {
                    let path = format!(
                        "{}/{}",
                        size.file_name().to_string_lossy(),
                        context.file_name().to_string_lossy()
                    );
                    if seen.insert(path.clone())
                        && let Some(dir) = Dir::guessed(&path)
                    {
                        dirs.push(dir);
                    }
                }
            }
        }
        dirs.sort_by(|a, b| a.path.cmp(&b.path));
        Self {
            dirs,
            inherits: Vec::new(),
        }
    }
}

/// The themes the icons are looked for in, in order, and the files of
/// the directories looked in, read once each.
#[derive(Debug, Default)]
pub struct Themes {
    roots: Vec<PathBuf>,
    pixmaps: Vec<PathBuf>,
    chain: Vec<(String, Theme)>,
    listings: HashMap<PathBuf, HashSet<String>>,
}

/// The user's icon theme, as GTK's and KDE's settings say it.
fn configured() -> Option<String> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let config = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    for gtk in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        let Ok(text) = std::fs::read_to_string(config.join(gtk)) else {
            continue;
        };
        let theme = ini(&text)
            .get("Settings")
            .and_then(|s| s.get("gtk-icon-theme-name"))
            .map(|t| t.trim_matches('"').to_owned())
            .filter(|t| !t.is_empty());
        if theme.is_some() {
            return theme;
        }
    }
    let text = std::fs::read_to_string(config.join("kdeglobals")).ok()?;
    ini(&text)
        .get("Icons")
        .and_then(|s| s.get("Theme"))
        .filter(|t| !t.is_empty())
        .cloned()
}

impl Themes {
    /// The user's theme and those it inherits, then `hicolor`, in
    /// `~/.icons` and each data directory's `icons`.
    pub fn load(data_dirs: &[PathBuf]) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let roots: Vec<PathBuf> = home
            .iter()
            .map(|h| h.join(".icons"))
            .chain(data_dirs.iter().map(|d| d.join("icons")))
            .collect();
        let pixmaps = data_dirs.iter().map(|d| d.join("pixmaps")).collect();
        let mut themes = Self {
            roots,
            pixmaps,
            ..Self::default()
        };
        let theme = configured().or_else(|| {
            themes
                .index_of("Adwaita")
                .is_some()
                .then(|| "Adwaita".to_owned())
        });
        let mut queue: Vec<String> = theme.into_iter().collect();
        let mut seen = HashSet::new();
        while let Some(name) = queue.first().cloned() {
            queue.remove(0);
            if name == "hicolor" || !seen.insert(name.clone()) {
                continue;
            }
            let Some(theme) = themes.theme(&name) else {
                continue;
            };
            queue.extend(theme.inherits.iter().cloned());
            themes.chain.push((name, theme));
        }
        if let Some(hicolor) = themes.theme("hicolor") {
            themes.chain.push(("hicolor".to_owned(), hicolor));
        }
        tracing::debug!(
            themes = ?themes.chain.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            "the launcher's icon themes"
        );
        themes
    }

    fn index_of(&self, name: &str) -> Option<PathBuf> {
        self.roots
            .iter()
            .map(|r| r.join(name).join("index.theme"))
            .find(|f| f.is_file())
    }

    fn theme(&self, name: &str) -> Option<Theme> {
        if name.contains('/') || name.is_empty() {
            return None;
        }
        match self.index_of(name) {
            Some(index) => std::fs::read_to_string(index).ok().map(|t| Theme::read(&t)),
            None => {
                let theme = Theme::guessed(&self.roots, name);
                (!theme.dirs.is_empty()).then_some(theme)
            }
        }
    }

    /// Whether `dir` holds `file`, its listing read once.
    fn holds(&mut self, dir: &Path, file: &str) -> bool {
        self.listings
            .entry(dir.to_owned())
            .or_insert_with(|| {
                std::fs::read_dir(dir)
                    .map(|read| {
                        read.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .contains(file)
    }

    /// Icon `name`'s file for `size` at `scale`: in the first theme that
    /// has it, a directory of that size first, else the closest larger,
    /// else the closest smaller; else in `pixmaps`. An absolute path is
    /// its own file.
    pub fn find(&mut self, name: &str, size: u32, scale: u32) -> Option<PathBuf> {
        let path = Path::new(name);
        if path.is_absolute() {
            return path.is_file().then(|| path.to_owned());
        }
        if name.is_empty() || name.contains('/') {
            return None;
        }
        // An `Icon` that names its file's extension.
        let name = name
            .strip_suffix(".png")
            .or_else(|| name.strip_suffix(".svg"))
            .unwrap_or(name);
        let files = [format!("{name}.png"), format!("{name}.svg")];
        let chain = self.chain.clone();
        let roots = self.roots.clone();
        for (theme, found) in &chain {
            let mut closest: Option<((bool, u32), PathBuf)> = None;
            for dir in &found.dirs {
                let exact = dir.matches(size, scale);
                let distance = dir.rank(size, scale);
                if !exact && closest.as_ref().is_some_and(|(d, _)| *d <= distance) {
                    continue;
                }
                for root in &roots {
                    let at = root.join(theme).join(&dir.path);
                    for file in &files {
                        if self.holds(&at, file) {
                            if exact {
                                return Some(at.join(file));
                            }
                            if closest.as_ref().is_none_or(|(d, _)| distance < *d) {
                                closest = Some((distance, at.join(file)));
                            }
                        }
                    }
                }
            }
            if let Some((_, file)) = closest {
                return Some(file);
            }
        }
        let pixmaps = self.pixmaps.clone();
        for dir in &pixmaps {
            for file in &files {
                if self.holds(dir, file) {
                    return Some(dir.join(file));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "[Icon Theme]\nName=Mine\nInherits=Other,hicolor\n\
        Directories=48x48/apps,scalable/apps,48x48@2/apps\n\n\
        [48x48/apps]\nSize=48\nType=Fixed\n\n\
        [48x48@2/apps]\nSize=48\nScale=2\nType=Fixed\n\n\
        [scalable/apps]\nSize=64\nMinSize=16\nMaxSize=256\nType=Scalable\n";

    #[test]
    fn a_themes_directories_and_their_sizes() {
        let theme = Theme::read(INDEX);
        assert_eq!(theme.inherits, ["Other", "hicolor"]);
        let fixed = &theme.dirs[0];
        assert!(fixed.matches(48, 1));
        assert!(!fixed.matches(48, 2));
        assert_eq!(fixed.distance(48, 2), 48);
        let scalable = &theme.dirs[1];
        assert!(scalable.matches(48, 1));
        assert_eq!(scalable.distance(300, 1), 44);
        assert!(theme.dirs[2].matches(48, 2));
        assert_eq!(
            Dir::guessed("96x96@2/apps").map(|d| (d.size, d.scale)),
            Some((96, 2))
        );
        assert_eq!(Dir::guessed("symbolic/apps"), None);
    }

    #[test]
    fn the_users_theme_then_hicolor_at_the_closest_size() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let icons = dir.path().join("icons");
        let put = |path: &str| -> std::io::Result<()> {
            let file = icons.join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(file, b"x")
        };
        std::fs::create_dir_all(icons.join("Mine"))?;
        std::fs::write(icons.join("Mine/index.theme"), INDEX)?;
        put("Mine/48x48/apps/editor.png")?;
        put("Mine/48x48@2/apps/editor.png")?;
        put("hicolor/scalable/apps/editor.svg")?;
        put("hicolor/48x48/apps/browser.png")?;
        put("hicolor/256x256/apps/music.png")?;
        put("hicolor/32x32/apps/music.png")?;
        put("hicolor/64x64/apps/photos.png")?;
        put("hicolor/128x128/apps/photos.png")?;
        let mut themes = Themes {
            roots: vec![icons.clone()],
            pixmaps: vec![dir.path().join("pixmaps")],
            ..Themes::default()
        };
        let mine = themes.theme("Mine").unwrap_or_default();
        let hicolor = themes.theme("hicolor").unwrap_or_default();
        themes.chain = vec![("Mine".into(), mine), ("hicolor".into(), hicolor)];
        // The user's theme first, at its scale.
        assert_eq!(
            themes.find("editor", 48, 1),
            Some(icons.join("Mine/48x48/apps/editor.png"))
        );
        assert_eq!(
            themes.find("editor", 48, 2),
            Some(icons.join("Mine/48x48@2/apps/editor.png"))
        );
        // hicolor's, guessed from its directories' names.
        assert_eq!(
            themes.find("browser", 48, 1),
            Some(icons.join("hicolor/48x48/apps/browser.png"))
        );
        // None of the size: a larger one, scaled down, before a closer
        // smaller one scaled up (#518): 256 for 48, 128 for 72.
        assert_eq!(
            themes.find("music", 48, 1),
            Some(icons.join("hicolor/256x256/apps/music.png"))
        );
        assert_eq!(
            themes.find("photos", 72, 1),
            Some(icons.join("hicolor/128x128/apps/photos.png"))
        );
        // Larger than any: the largest.
        assert_eq!(
            themes.find("photos", 192, 1),
            Some(icons.join("hicolor/128x128/apps/photos.png"))
        );
        assert_eq!(themes.find("nothing", 48, 1), None);
        assert_eq!(themes.find("../editor", 48, 1), None);
        Ok(())
    }
}
