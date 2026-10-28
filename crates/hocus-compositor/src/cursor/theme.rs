//! The system's Xcursor theme: which one, where its files are, the names a
//! shape goes by in it, and its images at a size. Nothing here touches
//! the loop: the files are read on the theme's own thread.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use smithay::input::pointer::CursorIcon;

/// A cursor's size when nothing says otherwise, as libXcursor's.
pub const DEFAULT_SIZE: u32 = 24;
/// The sizes `XCURSOR_SIZE` and `[appearance] cursor-size` may give.
pub const SIZES: std::ops::RangeInclusive<u32> = 8..=256;
/// The theme looked for when none is named; it usually inherits the
/// distribution's (Adwaita, Breeze…).
pub const DEFAULT_THEME: &str = "default";
/// Themes inheriting themes: past this depth, a loop or a mistake.
const MAX_DEPTH: usize = 16;
/// A frame shown for less than this is shown this long: no cursor asks
/// for more frames than a screen shows.
const MIN_DELAY: Duration = Duration::from_millis(10);

/// What the program's environment says about cursors, read once as it
/// starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorEnv {
    /// `XCURSOR_THEME`.
    pub theme: Option<String>,
    /// `XCURSOR_SIZE`.
    pub size: Option<u32>,
    /// Where themes are looked for, in this order. Empty: no theme, the
    /// cursors are drawn (tests).
    pub search: Vec<PathBuf>,
}

impl CursorEnv {
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            theme: var("XCURSOR_THEME"),
            size: var("XCURSOR_SIZE")
                .and_then(|s| s.trim().parse().ok())
                .filter(|s| SIZES.contains(s)),
            search: search_path(&SearchVars {
                xcursor_path: var("XCURSOR_PATH"),
                home: var("HOME").map(PathBuf::from),
                data_home: var("XDG_DATA_HOME").map(PathBuf::from),
                data_dirs: var("XDG_DATA_DIRS"),
            }),
        }
    }
}

/// The variables the search path is made of.
#[derive(Debug, Default)]
pub struct SearchVars {
    pub xcursor_path: Option<String>,
    pub home: Option<PathBuf>,
    pub data_home: Option<PathBuf>,
    pub data_dirs: Option<String>,
}

/// Where themes are: `XCURSOR_PATH` when set, else as libXcursor and
/// icon themes look: the user's icons, `~/.icons`, the system's data
/// directories' `icons`, `/usr/share/pixmaps`.
pub fn search_path(vars: &SearchVars) -> Vec<PathBuf> {
    let home = vars.home.as_deref();
    let mut dirs: Vec<PathBuf> = match &vars.xcursor_path {
        Some(list) => list
            .split(':')
            .filter(|d| !d.is_empty())
            .filter_map(|d| match d.strip_prefix("~/") {
                Some(rest) => home.map(|h| h.join(rest)),
                None => Some(PathBuf::from(d)),
            })
            .collect(),
        None => {
            let data_home = vars
                .data_home
                .clone()
                .or_else(|| home.map(|h| h.join(".local/share")));
            let data_dirs = vars
                .data_dirs
                .as_deref()
                .unwrap_or("/usr/local/share:/usr/share");
            data_home
                .map(|d| d.join("icons"))
                .into_iter()
                .chain(home.map(|h| h.join(".icons")))
                .chain(
                    data_dirs
                        .split(':')
                        .filter(|d| !d.is_empty())
                        .map(|d| Path::new(d).join("icons")),
                )
                .chain([PathBuf::from("/usr/share/icons")])
                .chain([PathBuf::from("/usr/share/pixmaps")])
                .collect()
        }
    };
    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// The theme and size cursors are shown in, also given to the programs
/// the desktop starts so that theirs match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub theme: String,
    /// Logical pixels.
    pub size: u32,
}

impl Choice {
    /// `[appearance]`'s, else the environment's, else `default` at 24.
    pub fn of(theme: Option<&str>, size: Option<u32>, env: &CursorEnv) -> Self {
        Self {
            theme: theme
                .or(env.theme.as_deref())
                .unwrap_or(DEFAULT_THEME)
                .to_owned(),
            size: size.or(env.size).unwrap_or(DEFAULT_SIZE),
        }
    }
}

/// The names `icon` goes by in themes, the cursor-shape and CSS name
/// first, then the older X11 ones.
pub fn names(icon: CursorIcon) -> Vec<&'static str> {
    use CursorIcon as C;
    let more: &[&str] = match icon {
        C::EwResize => &["sb_h_double_arrow"],
        C::NsResize => &["sb_v_double_arrow"],
        // One edge: the double arrow along it, when the theme has no
        // single one.
        C::EResize | C::WResize => &["ew-resize", "sb_h_double_arrow", "size_hor"],
        C::NResize | C::SResize => &["ns-resize", "sb_v_double_arrow", "size_ver"],
        C::NeResize | C::SwResize => &["nesw-resize", "fd_double_arrow", "size_bdiag"],
        C::NwResize | C::SeResize => &["nwse-resize", "bd_double_arrow", "size_fdiag"],
        C::Move => &["fleur", "size_all", "all-scroll"],
        C::AllScroll => &["fleur", "size_all"],
        C::Grabbing => &["fleur"],
        C::Crosshair => &["crosshair", "tcross"],
        C::Copy => &["dnd-copy"],
        C::Alias => &["dnd-link"],
        C::NoDrop => &["dnd-no-drop", "not-allowed"],
        C::NotAllowed => &["circle", "no-drop"],
        C::Progress => &["watch", "wait"],
        _ => &[],
    };
    let mut names = vec![icon.name()];
    for name in icon.alt_names().iter().chain(more) {
        if !names.contains(name) {
            names.push(name);
        }
    }
    names
}

/// The directories of `theme` and of the themes it inherits, in the order
/// a cursor is looked for in them; `default`'s last, when not already
/// there. Empty when no theme is found.
pub fn theme_dirs(search: &[PathBuf], theme: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut walked = HashSet::new();
    walk(search, theme, 0, &mut walked, &mut dirs);
    walk(search, DEFAULT_THEME, 0, &mut walked, &mut dirs);
    dirs
}

fn walk(
    search: &[PathBuf],
    theme: &str,
    depth: usize,
    walked: &mut HashSet<String>,
    dirs: &mut Vec<PathBuf>,
) {
    // A name, not a path.
    let plain = !theme.is_empty() && !theme.contains('/') && theme != "." && theme != "..";
    if !plain || depth > MAX_DEPTH || !walked.insert(theme.to_owned()) {
        return;
    }
    let here: Vec<PathBuf> = search
        .iter()
        .map(|s| s.join(theme))
        .filter(|d| d.is_dir())
        .collect();
    // The first `index.theme` says what the theme inherits.
    let parents = here
        .iter()
        .find_map(|d| inherits(&d.join("index.theme")))
        .unwrap_or_default();
    dirs.extend(here);
    for parent in parents {
        walk(search, &parent, depth + 1, walked, dirs);
    }
}

/// `Inherits=` of an `index.theme`'s `[Icon Theme]`; `None` when the
/// file does not read.
fn inherits(index: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(index).ok()?;
    let mut in_section = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line == "[Icon Theme]";
        } else if in_section && let Some(value) = line.strip_prefix("Inherits") {
            let Some(value) = value.trim_start().strip_prefix('=') else {
                continue;
            };
            return Some(
                value
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .collect(),
            );
        }
    }
    Some(Vec::new())
}

/// The first file of `names` in the theme's directories: each theme is
/// looked through for all the names before the one it inherits, so that a
/// theme's own older name wins over another theme's newer one.
pub fn find(dirs: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    dirs.iter()
        .flat_map(|d| names.iter().map(move |n| d.join("cursors").join(n)))
        .find(|p| p.is_file())
}

/// One image of a cursor, premultiplied ARGB8888 (B, G, R, A in memory),
/// as Xcursor files hold them.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// The pointer's place in the image, in its pixels.
    pub hotspot: (u32, u32),
    /// How long it shows, when the cursor is animated.
    pub delay: Duration,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("size", &(self.width, self.height))
            .field("hotspot", &self.hotspot)
            .field("delay", &self.delay)
            .finish_non_exhaustive()
    }
}

/// The frames of the file at `path` whose size is nearest `pixels` (the
/// larger of two as near). `None` when it does not read or holds no
/// image.
pub fn load(path: &Path, pixels: u32) -> Option<Vec<Frame>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(file = %path.display(), %err, "could not read a cursor");
            return None;
        }
    };
    let Some(images) = xcursor::parser::parse_xcursor(&bytes) else {
        tracing::warn!(file = %path.display(), "not an Xcursor file");
        return None;
    };
    let nominal = images
        .iter()
        .map(|i| i.size)
        .min_by_key(|&s| (s.abs_diff(pixels), std::cmp::Reverse(s)))?;
    let frames: Vec<Frame> = images
        .into_iter()
        .filter(|i| i.size == nominal)
        .filter(|i| i.pixels_rgba.len() == (i.width * i.height * 4) as usize)
        .map(|i| Frame {
            width: i.width,
            height: i.height,
            hotspot: (i.xhot.min(i.width), i.yhot.min(i.height)),
            delay: Duration::from_millis(u64::from(i.delay)).max(MIN_DELAY),
            // Despite its name, the file's bytes as they are: B, G, R, A.
            pixels: i.pixels_rgba,
        })
        .collect();
    (!frames.is_empty()).then_some(frames)
}

/// The frame shown at `now`, and when the next one shows: animations run
/// on the monotonic clock from its start, so that nothing keeps a phase.
pub fn frame_at(frames: &[Frame], now: Duration) -> (usize, Option<Duration>) {
    if frames.len() < 2 {
        return (0, None);
    }
    let total: Duration = frames.iter().map(|f| f.delay).sum();
    let into = Duration::from_nanos((now.as_nanos() % total.as_nanos()) as u64);
    let start = now - into;
    let mut end = Duration::ZERO;
    for (i, frame) in frames.iter().enumerate() {
        end += frame.delay;
        if into < end {
            return (i, Some(start + end));
        }
    }
    (0, Some(start + total))
}

/// An image of a test's cursor: nominal size, width, height, hotspot,
/// delay in ms, and its one premultiplied ARGB colour.
#[doc(hidden)]
pub type TestImage = (u32, u32, u32, (u32, u32), u32, u32);

/// An Xcursor file holding `images`, for tests.
#[doc(hidden)]
pub fn encode(images: &[TestImage]) -> Vec<u8> {
    const IMAGE: u32 = 0xfffd_0002;
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(b"Xcur");
    put(&mut out, 16);
    put(&mut out, 0x1_0000);
    put(&mut out, images.len() as u32);
    let mut position = 16 + 12 * images.len() as u32;
    for &(size, w, h, ..) in images {
        put(&mut out, IMAGE);
        put(&mut out, size);
        put(&mut out, position);
        position += 36 + w * h * 4;
    }
    for &(size, w, h, (x, y), delay, colour) in images {
        for v in [36, IMAGE, size, 1, w, h, x, y, delay] {
            put(&mut out, v);
        }
        for _ in 0..w * h {
            put(&mut out, colour);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_go_by_their_css_name_then_the_older_ones() {
        assert_eq!(names(CursorIcon::Default)[..2], ["default", "left_ptr"]);
        let ew = names(CursorIcon::EwResize);
        assert_eq!(ew[0], "ew-resize");
        for alias in ["sb_h_double_arrow", "size_hor"] {
            assert!(ew.contains(&alias), "{ew:?}");
        }
        assert!(names(CursorIcon::NwseResize).contains(&"size_fdiag"));
        assert!(names(CursorIcon::NeswResize).contains(&"size_bdiag"));
        assert!(names(CursorIcon::Grab).contains(&"openhand"));
        assert!(names(CursorIcon::Grabbing).contains(&"closedhand"));
        assert!(names(CursorIcon::Move).contains(&"fleur"));
        assert!(names(CursorIcon::Wait).contains(&"watch"));
        // One edge falls back on the double arrow along it.
        let e = names(CursorIcon::EResize);
        assert_eq!(e[..2], ["e-resize", "right_side"]);
        assert!(e.contains(&"ew-resize"));
        // No name twice.
        for icon in [CursorIcon::ColResize, CursorIcon::Progress] {
            let n = names(icon);
            assert_eq!(n.iter().collect::<HashSet<_>>().len(), n.len(), "{n:?}");
        }
    }

    #[test]
    fn the_search_path_is_xcursor_paths_or_the_usual_places() {
        let home = Some(PathBuf::from("/home/u"));
        let usual = search_path(&SearchVars {
            home: home.clone(),
            ..SearchVars::default()
        });
        assert_eq!(
            usual,
            [
                "/home/u/.local/share/icons",
                "/home/u/.icons",
                "/usr/local/share/icons",
                "/usr/share/icons",
                "/usr/share/pixmaps",
            ]
            .map(PathBuf::from)
        );
        let given = search_path(&SearchVars {
            xcursor_path: Some("~/.my-icons::/opt/icons".into()),
            home,
            ..SearchVars::default()
        });
        assert_eq!(
            given,
            ["/home/u/.my-icons", "/opt/icons"].map(PathBuf::from)
        );
    }

    #[test]
    fn the_appearance_wins_over_the_environment_over_the_defaults() {
        let env = CursorEnv {
            theme: Some("Breeze".into()),
            size: Some(32),
            search: Vec::new(),
        };
        let none = CursorEnv::default();
        assert_eq!(
            Choice::of(None, None, &none),
            Choice {
                theme: "default".into(),
                size: 24
            }
        );
        assert_eq!(Choice::of(None, None, &env).theme, "Breeze");
        assert_eq!(Choice::of(Some("Adwaita"), Some(48), &env).size, 48);
        assert_eq!(Choice::of(Some("Adwaita"), None, &env).theme, "Adwaita");
    }

    fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// `user` (in the first directory) inherits `base` (in the second),
    /// which `default` inherits too; `default` has the arrow, `base` an
    /// older name for the double arrow, `user` the newer one only for a
    /// corner.
    #[test]
    fn a_theme_is_looked_through_before_those_it_inherits() -> anyhow::Result<()> {
        let first = tempfile::tempdir()?;
        let second = tempfile::tempdir()?;
        let search = [first.path().to_owned(), second.path().to_owned()];
        let file = encode(&[(24, 24, 24, (1, 1), 0, 0xff00_00ff)]);
        write(
            &first.path().join("user/index.theme"),
            b"[Icon Theme]\nName=User\nInherits = base, missing\n",
        )?;
        write(&first.path().join("user/cursors/nwse-resize"), &file)?;
        write(
            &second.path().join("base/index.theme"),
            b"[Icon Theme]\nInherits=user\n",
        )?;
        write(&second.path().join("base/cursors/sb_h_double_arrow"), &file)?;
        write(&second.path().join("base/cursors/nwse-resize"), &file)?;
        write(&second.path().join("default/cursors/left_ptr"), &file)?;

        let dirs = theme_dirs(&search, "user");
        assert_eq!(
            dirs,
            [
                first.path().join("user"),
                second.path().join("base"),
                second.path().join("default"),
            ]
        );
        let found = |icon| find(&dirs, &names(icon));
        assert_eq!(
            found(CursorIcon::EwResize),
            Some(second.path().join("base/cursors/sb_h_double_arrow"))
        );
        assert_eq!(
            found(CursorIcon::NwseResize),
            Some(first.path().join("user/cursors/nwse-resize"))
        );
        assert_eq!(
            found(CursorIcon::Default),
            Some(second.path().join("default/cursors/left_ptr"))
        );
        assert_eq!(found(CursorIcon::Text), None);
        // No such theme: `default` still, alone; nothing at all without one.
        assert_eq!(
            theme_dirs(&search, "nothing"),
            [second.path().join("default")]
        );
        assert!(theme_dirs(&[first.path().to_owned()], "nothing").is_empty());
        assert!(theme_dirs(&search, "../base").len() == 1);
        Ok(())
    }

    #[test]
    fn the_nearest_size_is_loaded_with_its_frames() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("wait");
        std::fs::write(
            &path,
            encode(&[
                (24, 24, 24, (12, 12), 50, 0xff00_0000),
                (24, 24, 24, (12, 12), 50, 0xffff_ffff),
                (48, 48, 48, (24, 24), 50, 0xff00_0000),
                (48, 48, 48, (24, 24), 50, 0xffff_ffff),
                (32, 32, 32, (16, 16), 0, 0xff00_0000),
            ]),
        )?;
        let at = |pixels| load(&path, pixels).map(|f| (f.len(), f[0].width));
        assert_eq!(at(24), Some((2, 24)));
        assert_eq!(at(48), Some((2, 48)));
        assert_eq!(at(30), Some((1, 32)));
        // Between 32 and 48: the larger.
        assert_eq!(at(40), Some((2, 48)));
        assert_eq!(at(96), Some((2, 48)));
        let frames = load(&path, 24).unwrap_or_default();
        assert_eq!(frames[0].hotspot, (12, 12));
        assert_eq!(&frames[1].pixels[..4], &[0xff; 4]);
        assert_eq!(load(&dir.path().join("none"), 24), None);
        std::fs::write(&path, b"not a cursor")?;
        assert_eq!(load(&path, 24), None);
        Ok(())
    }

    #[test]
    fn animations_follow_the_clock() {
        let frame = |ms| Frame {
            width: 1,
            height: 1,
            hotspot: (0, 0),
            delay: Duration::from_millis(ms),
            pixels: vec![0; 4],
        };
        let frames = [frame(30), frame(20)];
        let ms = Duration::from_millis;
        assert_eq!(frame_at(&frames, ms(0)), (0, Some(ms(30))));
        assert_eq!(frame_at(&frames, ms(29)), (0, Some(ms(30))));
        assert_eq!(frame_at(&frames, ms(30)), (1, Some(ms(50))));
        assert_eq!(frame_at(&frames, ms(1_010)), (0, Some(ms(1_030))));
        assert_eq!(frame_at(&frames[..1], ms(10)), (0, None));
    }
}
