//! Écrans / Screens (6.9, docs/design/settings.md "Écrans"): the screens
//! as the compositor says them, its line, the arrangement's geometry and
//! the page's settings for the search.
//!
//! - **The compositor is the source.** A line of its control socket
//!   (`msg screens`, the same socket as `hocus msg`) stays open on threads
//!   of its own: the compositor writes the screens as one JSON line each
//!   time they change, and the trials' news; the window asks for a trial
//!   (`try NAME mode=WxH@HZ`), keeps it or takes it back. The countdown is
//!   the compositor's: the window only shows the seconds it is told, and a
//!   window gone takes the trial back with its line.
//! - **The file** holds `[outputs.NAME] mode`, `position`, `scale`,
//!   `enabled`, `vrr` and `[panel] output`, written by the page itself
//!   (several keys at once for a place: every screen's), through the
//!   file's worker; the compositor follows the file.
//! - **The arrangement**: where a dragged screen lands against the other
//!   (the side nearest its centre, flush, 200 px shared at least, caught
//!   by aligned edges or middles within 120 px), the places of a size
//!   changed, and how a place is said.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{OnceLock, mpsc},
};

use crate::{
    lang::{Lang, w},
    model::{Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

/// A mode: width, height (pixels), refresh (mHz).
pub type Mode3 = (i32, i32, i32);

/// A screen, as the compositor says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub name: String,
    pub built_in: bool,
    /// Millimetres; 0 when unknown.
    pub mm: (i32, i32),
    /// Its modes, the largest first; and which is its native size.
    pub modes: Vec<(Mode3, bool)>,
    pub mode: Option<Mode3>,
    pub scale: f64,
    /// Its scale chosen by the compositor, not the file.
    pub scale_auto: bool,
    /// What it takes by itself.
    pub auto_scale: f64,
    /// Logical pixels.
    pub position: (i32, i32),
    pub size: (i32, i32),
    pub vrr_capable: bool,
    pub vrr: bool,
    pub enabled: bool,
    pub connected: bool,
    pub lid_closed: bool,
    pub primary: bool,
    /// The file's mode, as written, and whether the screen offers it.
    pub file_mode: Option<String>,
    pub mode_offered: bool,
    /// When it was unplugged (seconds since 1970).
    pub unplugged_at: Option<i64>,
    /// 1 the built-in panel, then the others in the connectors' order.
    pub number: usize,
    /// Its bar's form now (`"full"`, `"mini"`), none without a bar (6.4).
    pub bar: Option<&'static str>,
    /// The form chosen there with a click or Super+B, remembered.
    pub bar_chosen: Option<&'static str>,
}

impl Screen {
    /// Its native size (the one it prefers).
    pub fn native(&self) -> Option<(i32, i32)> {
        self.modes
            .iter()
            .find(|(_, preferred)| *preferred)
            .map(|((w, h, _), _)| (*w, *h))
    }

    /// Its sizes, the native first, each once.
    pub fn sizes(&self) -> Vec<(i32, i32)> {
        let mut sizes: Vec<(i32, i32)> = Vec::new();
        if let Some(native) = self.native() {
            sizes.push(native);
        }
        for ((w, h, _), _) in &self.modes {
            if !sizes.contains(&(*w, *h)) {
                sizes.push((*w, *h));
            }
        }
        sizes
    }

    /// The rates of a size, highest first (mHz).
    pub fn rates(&self, size: (i32, i32)) -> Vec<i32> {
        let mut rates: Vec<i32> = self
            .modes
            .iter()
            .filter(|((w, h, _), _)| (*w, *h) == size)
            .map(|((_, _, r), _)| *r)
            .collect();
        rates.sort_unstable_by(|a, b| b.cmp(a));
        rates.dedup();
        rates
    }

    /// Its diagonal in inches, when its size is known.
    pub fn inches(&self) -> Option<u32> {
        let (w, h) = self.mm;
        if w <= 0 || h <= 0 {
            return None;
        }
        let d = (f64::from(w).powi(2) + f64::from(h).powi(2)).sqrt() / 25.4;
        Some(d.round() as u32)
    }

    /// Pixels per inch across, when its size is known.
    pub fn density(&self) -> Option<u32> {
        let (w, _) = self.mm;
        let (px, _, _) = self.mode?;
        (w > 0).then(|| (f64::from(px) / (f64::from(w) / 25.4)).round() as u32)
    }

    /// "Écran du portable", "Écran externe 34 pouces", "Écran 2".
    pub fn title(&self, lang: Lang) -> String {
        match (self.built_in, self.inches(), lang) {
            (true, _, Lang::French) => "Écran du portable".to_owned(),
            (true, _, Lang::English) => "Laptop screen".to_owned(),
            (false, Some(i), Lang::French) => format!("Écran externe {i} pouces"),
            (false, Some(i), Lang::English) => format!("{i}-inch external screen"),
            (false, None, Lang::French) => format!("Écran {}", self.number),
            (false, None, Lang::English) => format!("Screen {}", self.number),
        }
    }

    /// The name in a small rectangle: "Portable", "Écran externe".
    pub fn short(&self, lang: Lang) -> String {
        match (self.built_in, self.inches(), lang) {
            (true, _, Lang::French) => "Portable".to_owned(),
            (true, _, Lang::English) => "Laptop".to_owned(),
            (false, Some(_), Lang::French) => "Écran externe".to_owned(),
            (false, Some(_), Lang::English) => "External".to_owned(),
            (false, None, _) => self.title(lang),
        }
    }

    /// Its rectangle, logical pixels.
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.position.0,
            y: self.position.1,
            w: self.size.0,
            h: self.size.1,
        }
    }
}

/// The screens, as last said.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screens {
    /// Plugged first, by number, then those unplugged.
    pub list: Vec<Screen>,
    /// The file's places were refused: the compositor put them left to
    /// right.
    pub places_refused: bool,
}

impl Screens {
    pub fn get(&self, name: &str) -> Option<&Screen> {
        self.list.iter().find(|s| s.name == name)
    }

    /// The screens plugged.
    pub fn plugged(&self) -> impl Iterator<Item = &Screen> {
        self.list.iter().filter(|s| s.connected)
    }

    /// The screens on.
    pub fn on(&self) -> impl Iterator<Item = &Screen> {
        self.plugged().filter(|s| s.enabled && !s.lid_closed)
    }
}

/// What the compositor's line says.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    Screens(Screens),
    /// A trial under way, the seconds left.
    Trial {
        id: u64,
        output: String,
        mode: String,
        before: String,
        left: u64,
    },
    Ended {
        id: u64,
        output: String,
        kept: bool,
    },
    Refused(String),
    /// The line could not open, or closed.
    Gone,
    /// How many desktops there are, and how many windows each has, every
    /// screen's together (6.14).
    Desktops {
        count: usize,
        windows: Vec<usize>,
    },
    /// Desktop `desktop` (from 1) taken out, its `windows` going to `to`
    /// (numbered after).
    DesktopRemoved {
        desktop: usize,
        to: usize,
        windows: Vec<u64>,
    },
    /// A desktop put back at `desktop`, `windows` on it.
    DesktopRestored {
        desktop: usize,
        windows: Vec<u64>,
    },
}

/// A bar's form as the compositor says it.
fn form_word(value: Option<&serde_json::Value>) -> Option<&'static str> {
    match value?.as_str()? {
        "full" => Some("full"),
        "mini" => Some("mini"),
        _ => None,
    }
}

/// One JSON line of the compositor's.
pub fn read_line(line: &str) -> Option<Said> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if let Some(list) = value.get("outputs").and_then(|o| o.as_array()) {
        let int = |v: &serde_json::Value| v.as_i64().and_then(|n| i32::try_from(n).ok());
        let pair = |v: Option<&serde_json::Value>| -> (i32, i32) {
            v.and_then(|v| v.as_array())
                .and_then(|a| Some((int(a.first()?)?, int(a.get(1)?)?)))
                .unwrap_or_default()
        };
        let mut screens: Vec<Screen> = list
            .iter()
            .filter_map(|o| {
                let modes = o
                    .get("modes")
                    .and_then(|m| m.as_array())
                    .map(|m| {
                        m.iter()
                            .filter_map(|m| {
                                let a = m.as_array()?;
                                Some((
                                    (int(a.first()?)?, int(a.get(1)?)?, int(a.get(2)?)?),
                                    a.get(3)?.as_bool()?,
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mode = o
                    .get("mode")
                    .and_then(|m| m.as_array())
                    .and_then(|a| Some((int(a.first()?)?, int(a.get(1)?)?, int(a.get(2)?)?)));
                let flag = |key: &str, default: bool| {
                    o.get(key)
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(default)
                };
                Some(Screen {
                    name: o.get("name")?.as_str()?.to_owned(),
                    built_in: flag("built_in", false),
                    mm: pair(o.get("mm")),
                    modes,
                    mode,
                    scale: o.get("scale").and_then(|s| s.as_f64()).unwrap_or(1.0),
                    scale_auto: o.get("scale_from").and_then(|s| s.as_str()) != Some("file"),
                    auto_scale: o.get("auto_scale").and_then(|s| s.as_f64()).unwrap_or(1.0),
                    position: pair(o.get("position")),
                    size: pair(o.get("size")),
                    vrr_capable: flag("vrr_capable", false),
                    vrr: flag("vrr", false),
                    enabled: flag("enabled", true),
                    connected: flag("connected", true),
                    lid_closed: flag("lid_closed", false),
                    primary: flag("primary", false),
                    file_mode: o
                        .get("file_mode")
                        .and_then(|m| m.as_str())
                        .map(ToOwned::to_owned),
                    mode_offered: flag("mode_offered", true),
                    unplugged_at: o.get("unplugged_at").and_then(serde_json::Value::as_i64),
                    number: 0,
                    bar: form_word(o.get("bar")),
                    bar_chosen: form_word(o.get("bar_chosen")),
                })
            })
            .collect();
        number(&mut screens);
        return Some(Said::Screens(Screens {
            list: screens,
            places_refused: value
                .get("places_refused")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }));
    }
    if let Some(trial) = value.get("trial") {
        let text = |key: &str| {
            trial
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        return Some(Said::Trial {
            id: trial.get("id")?.as_u64()?,
            output: text("output"),
            mode: text("mode"),
            before: text("before"),
            left: trial.get("left")?.as_u64()?,
        });
    }
    if let Some(ended) = value.get("ended") {
        return Some(Said::Ended {
            id: ended.get("id")?.as_u64()?,
            output: ended
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
            kept: ended.get("kept")?.as_bool()?,
        });
    }
    let numbers = |v: Option<&serde_json::Value>| -> Vec<u64> {
        v.and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default()
    };
    let at = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
    };
    if let Some(desktops) = value.get("desktops") {
        return Some(Said::Desktops {
            count: at(desktops, "count")?,
            windows: numbers(desktops.get("windows"))
                .into_iter()
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .collect(),
        });
    }
    if let Some(removed) = value.get("desktop-removed") {
        return Some(Said::DesktopRemoved {
            desktop: at(removed, "desktop")?,
            to: at(removed, "to")?,
            windows: numbers(removed.get("windows")),
        });
    }
    if let Some(restored) = value.get("desktop-restored") {
        return Some(Said::DesktopRestored {
            desktop: at(restored, "desktop")?,
            windows: numbers(restored.get("windows")),
        });
    }
    value
        .get("refused")
        .and_then(|r| r.as_str())
        .map(|r| Said::Refused(r.to_owned()))
}

/// The numbers: 1 the built-in panel, then the others by connector; the
/// plugged ones first.
fn number(screens: &mut [Screen]) {
    screens.sort_by(|a, b| {
        (!a.connected, !a.built_in, &a.name).cmp(&(!b.connected, !b.built_in, &b.name))
    });
    for (i, s) in screens.iter_mut().enumerate() {
        s.number = i + 1;
    }
}

/// The window's end of the compositor's line: what it asks goes to the
/// line's writing thread.
#[derive(Debug, Clone)]
pub struct Line {
    asks: mpsc::Sender<String>,
}

impl Line {
    /// `line` to the compositor; whether the line took it.
    pub fn ask(&self, line: String) -> bool {
        self.asks.send(line).is_ok()
    }
}

/// The compositor's control socket: `HOCUS_SOCKET`, which it sets for
/// the programs it starts, else beside `$WAYLAND_DISPLAY`'s.
fn socket() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOCUS_SOCKET").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY")?);
    let display = if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display)
    };
    let mut path = display.into_os_string();
    path.push(".control");
    Some(PathBuf::from(path))
}

/// Opens the compositor's line on threads of its own; `heard` takes what
/// it says, and returns `false` once the window is gone.
pub fn open(mut heard: impl FnMut(Said) -> bool + Send + 'static) -> Line {
    let (asks, taken) = mpsc::channel::<String>();
    let started = std::thread::Builder::new()
        .name("screens' line".to_owned())
        .spawn(move || {
            let Some(path) = socket() else {
                tracing::info!("no compositor's socket: the screens are not known");
                let _ = heard(Said::Gone);
                return;
            };
            let stream = match UnixStream::connect(&path) {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::info!(%err, socket = %path.display(), "the compositor's line does not open");
                    let _ = heard(Said::Gone);
                    return;
                }
            };
            let Ok(mut writer) = stream.try_clone() else {
                let _ = heard(Said::Gone);
                return;
            };
            if writer.write_all(b"msg screens\n").is_err() {
                let _ = heard(Said::Gone);
                return;
            }
            let out = std::thread::Builder::new()
                .name("screens' asks".to_owned())
                .spawn(move || {
                    for line in taken {
                        tracing::debug!(line, "asked of the compositor");
                        if writer.write_all(format!("{line}\n").as_bytes()).is_err() {
                            return;
                        }
                    }
                    // The window is gone: so is the line, and the trial with it.
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                });
            if out.is_err() {
                let _ = heard(Said::Gone);
                return;
            }
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else {
                    break;
                };
                match read_line(&line) {
                    Some(said) => {
                        if !heard(said) {
                            return;
                        }
                    }
                    None => tracing::debug!(line, "a line of the compositor's not understood"),
                }
            }
            let _ = heard(Said::Gone);
        });
    if let Err(err) = started {
        tracing::warn!(%err, "the screens' line does not start");
    }
    Line { asks }
}

// --- The arrangement --------------------------------------------------------------------

/// A rectangle, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn cx(self) -> f64 {
        f64::from(self.x) + f64::from(self.w) / 2.0
    }

    fn cy(self) -> f64 {
        f64::from(self.y) + f64::from(self.h) / 2.0
    }
}

/// Which side of the other a screen is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
    Below,
}

/// How it is aligned along that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Tops (beside) or lefts (above, below).
    Start,
    Middle,
    /// Bottoms or rights.
    End,
    /// Neither: this far from the start.
    Off(i32),
}

/// At least this much of the edge is shared, so the pointer can cross.
pub const SHARED: i32 = 200;
/// Edges or middles this close catch.
pub const CATCH: i32 = 120;

/// The side of `other` nearest `moving`'s centre: the centres' offset
/// over half their summed sizes, the larger axis winning.
pub fn side_of(moving: Rect, other: Rect) -> Side {
    let dx = (moving.cx() - other.cx()) / ((f64::from(moving.w) + f64::from(other.w)) / 2.0);
    let dy = (moving.cy() - other.cy()) / ((f64::from(moving.h) + f64::from(other.h)) / 2.0);
    if dx.abs() >= dy.abs() {
        if dx < 0.0 { Side::Left } else { Side::Right }
    } else if dy < 0.0 {
        Side::Above
    } else {
        Side::Below
    }
}

/// How `moving` is aligned along `side` of `other`.
pub fn align_of(moving: Rect, other: Rect, side: Side) -> Align {
    let (start, len, o_start, o_len) = match side {
        Side::Left | Side::Right => (moving.y, moving.h, other.y, other.h),
        Side::Above | Side::Below => (moving.x, moving.w, other.x, other.w),
    };
    if start == o_start {
        Align::Start
    } else if start + len == o_start + o_len {
        Align::End
    } else if 2 * start + len == 2 * o_start + o_len {
        Align::Middle
    } else {
        Align::Off(start - o_start)
    }
}

/// Where `moving` (its size) lands on `side` of `other`, aligned so.
pub fn placed(moving: (i32, i32), other: Rect, side: Side, align: Align) -> (i32, i32) {
    let (len, o_start, o_len) = match side {
        Side::Left | Side::Right => (moving.1, other.y, other.h),
        Side::Above | Side::Below => (moving.0, other.x, other.w),
    };
    let share = SHARED.min(len).min(o_len);
    let along = match align {
        Align::Start => o_start,
        Align::End => o_start + o_len - len,
        Align::Middle => o_start + (o_len - len) / 2,
        Align::Off(off) => (o_start + off).clamp(o_start - len + share, o_start + o_len - share),
    };
    match side {
        Side::Left => (other.x - moving.0, along),
        Side::Right => (other.x + other.w, along),
        Side::Above => (along, other.y - moving.1),
        Side::Below => (along, other.y + other.h),
    }
}

/// Where `moving`, dropped at `dropped`, lands against `other`: flush on
/// the side nearest its centre, at least 200 px shared, caught by aligned
/// edges or middles within 120 px.
pub fn land(dropped: Rect, other: Rect) -> (i32, i32) {
    let side = side_of(dropped, other);
    let (start, len, o_start, o_len) = match side {
        Side::Left | Side::Right => (dropped.y, dropped.h, other.y, other.h),
        Side::Above | Side::Below => (dropped.x, dropped.w, other.x, other.w),
    };
    let catches = [
        (Align::Start, o_start),
        (Align::End, o_start + o_len - len),
        (Align::Middle, o_start + (o_len - len) / 2),
    ];
    let align = catches
        .iter()
        .map(|(a, at)| (*a, (start - at).abs()))
        .filter(|(_, d)| *d <= CATCH)
        .min_by_key(|(_, d)| *d)
        .map_or(Align::Off(start - o_start), |(a, _)| a);
    placed((dropped.w, dropped.h), other, side, align)
}

/// `places` moved so that the smallest x and y are 0.
pub fn normalised(places: &[(String, (i32, i32))]) -> Vec<(String, (i32, i32))> {
    let x0 = places.iter().map(|(_, p)| p.0).min().unwrap_or(0);
    let y0 = places.iter().map(|(_, p)| p.1).min().unwrap_or(0);
    places
        .iter()
        .map(|(n, (x, y))| (n.clone(), (x - x0, y - y0)))
        .collect()
}

/// "à gauche de l’écran du portable, hauts alignés".
pub fn say_place(side: Side, align: Align, other: &str, lang: Lang) -> String {
    let other = lower_first(other);
    let beside = matches!(side, Side::Left | Side::Right);
    let (where_, how) = match lang {
        Lang::French => (
            match side {
                Side::Left => format!("à gauche de l’{other}"),
                Side::Right => format!("à droite de l’{other}"),
                Side::Above => format!("au-dessus de l’{other}"),
                Side::Below => format!("au-dessous de l’{other}"),
            },
            match (align, beside) {
                (Align::Start, true) => Some("hauts alignés"),
                (Align::End, true) => Some("bas alignés"),
                (Align::Start, false) => Some("gauches alignées"),
                (Align::End, false) => Some("droites alignées"),
                (Align::Middle, _) => Some("centrés"),
                (Align::Off(_), _) => None,
            },
        ),
        Lang::English => (
            match side {
                Side::Left => format!("to the left of the {other}"),
                Side::Right => format!("to the right of the {other}"),
                Side::Above => format!("above the {other}"),
                Side::Below => format!("below the {other}"),
            },
            match (align, beside) {
                (Align::Start, true) => Some("tops aligned"),
                (Align::End, true) => Some("bottoms aligned"),
                (Align::Start, false) => Some("lefts aligned"),
                (Align::End, false) => Some("rights aligned"),
                (Align::Middle, _) => Some("centred"),
                (Align::Off(_), _) => None,
            },
        ),
    };
    match how {
        Some(how) => format!("{where_}, {how}"),
        None => where_,
    }
}

pub fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `2560x1600@60` as the file writes it; `59.94` when not whole.
pub fn mode_word(m: Mode3) -> String {
    if m.2 % 1000 == 0 {
        format!("{}x{}@{}", m.0, m.1, m.2 / 1000)
    } else {
        let hz = format!("{:.3}", f64::from(m.2) / 1000.0);
        let hz = hz.trim_end_matches('0').trim_end_matches('.');
        format!("{}x{}@{hz}", m.0, m.1)
    }
}

/// `120 Hz`, `59,94 Hz`.
pub fn say_rate(mhz: i32, lang: Lang) -> String {
    let hz = if mhz % 1000 == 0 {
        (mhz / 1000).to_string()
    } else {
        let hz = format!("{:.2}", f64::from(mhz) / 1000.0);
        let hz = hz.trim_end_matches('0').trim_end_matches('.').to_owned();
        if lang.fr() { hz.replace('.', ",") } else { hz }
    };
    format!("{hz} Hz")
}

/// `2560 × 1600`.
pub fn say_size(size: (i32, i32)) -> String {
    format!("{} × {}", size.0, size.1)
}

/// `125 %`.
pub fn say_scale(scale: f64) -> String {
    format!("{} %", (scale * 100.0).round())
}

/// The scales offered.
pub const SCALES: [f64; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

// --- The page's settings, for the search ------------------------------------------------

const ARRANGEMENT: RowGroup = RowGroup {
    label: w("DISPOSITION", "ARRANGEMENT"),
    per_source: false,
};
/// The selected screen's group: its label is the screen's name.
pub const SCREEN: RowGroup = RowGroup {
    label: w("CET ÉCRAN", "THIS SCREEN"),
    per_source: false,
};
const ALL: RowGroup = RowGroup {
    label: w("TOUS LES ÉCRANS", "ALL SCREENS"),
    per_source: false,
};
const NIGHT: RowGroup = RowGroup {
    label: w("ÉCLAIRAGE NOCTURNE", "NIGHT LIGHT"),
    per_source: false,
};

fn page(key: &'static [&'static str]) -> Places {
    Places::Page(Place {
        key,
        codec: Codec::Text,
        default: Value::Text(String::new()),
    })
}

/// The ids of the page's rows.
pub const ARRANGE: &str = "screens.arrangement";
pub const IDENTIFY: &str = "screens.identify";
pub const USE: &str = "screens.enabled";
pub const RESOLUTION: &str = "screens.resolution";
pub const RATE: &str = "screens.rate";
pub const SCALE: &str = "screens.scale";
pub const VRR: &str = "screens.vrr";
pub const ROTATION: &str = "screens.rotation";
pub const PRIMARY: &str = "screens.primary";
pub const MIRROR: &str = "screens.mirror";
pub const NIGHT_LIGHT: &str = "screens.night-light";

/// The page's settings: what the search finds; the page draws them itself.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let s = |id, group, name, keywords, key| Setting {
            id,
            section: SectionId::Screens,
            group,
            name,
            line: None,
            keywords,
            kind: Kind::Text,
            places: page(key),
            needs: None,
        };
        vec![
            s(
                ARRANGE,
                ARRANGEMENT,
                w("Disposition des écrans", "Screens' arrangement"),
                (
                    &["position", "placer", "gauche", "au-dessus", "glisser"],
                    &["layout", "arrange", "place", "left", "above", "drag"],
                ),
                &["outputs", "NAME", "position"],
            ),
            s(
                IDENTIFY,
                ARRANGEMENT,
                w("Identifier les écrans", "Identify the screens"),
                (&["numéro"], &["identify", "number"]),
                &["outputs", "NAME"],
            ),
            s(
                USE,
                SCREEN,
                w("Utiliser cet écran", "Use this screen"),
                (&["éteindre", "allumer"], &["enable", "disable", "turn off"]),
                &["outputs", "NAME", "enabled"],
            ),
            s(
                RESOLUTION,
                SCREEN,
                w("Résolution", "Resolution"),
                (&["définition", "pixels", "taille"], &["mode", "size"]),
                &["outputs", "NAME", "mode"],
            ),
            s(
                RATE,
                SCREEN,
                w("Fréquence", "Refresh rate"),
                (&["hz", "rafraîchissement"], &["hz", "refresh"]),
                &["outputs", "NAME", "mode"],
            ),
            s(
                SCALE,
                SCREEN,
                w("Échelle", "Scale"),
                (
                    &[
                        "hidpi",
                        "zoom",
                        "agrandir",
                        "fractionnaire",
                        "125 %",
                        "150 %",
                    ],
                    &["hidpi", "zoom", "bigger", "fractional", "scale"],
                ),
                &["outputs", "NAME", "scale"],
            ),
            s(
                VRR,
                SCREEN,
                w("Fréquence variable (VRR)", "Variable refresh rate (VRR)"),
                (
                    &["saccades", "jeux"],
                    &[
                        "vrr",
                        "adaptive sync",
                        "freesync",
                        "variable refresh",
                        "games",
                    ],
                ),
                &["outputs", "NAME", "vrr"],
            ),
            s(
                ROTATION,
                SCREEN,
                w("Rotation", "Rotation"),
                (&["pivoter", "portrait"], &["rotate", "orientation"]),
                &["outputs", "NAME", "transform"],
            ),
            s(
                PRIMARY,
                ALL,
                w("Écran principal", "Primary screen"),
                (&["primaire", "barre"], &["primary", "main display", "bar"]),
                &["panel", "output"],
            ),
            s(
                MIRROR,
                ALL,
                w("Recopie", "Mirroring"),
                (
                    &["miroir", "dupliquer", "projecteur", "cloner"],
                    &["mirror", "duplicate", "projector", "clone"],
                ),
                &["outputs", "NAME", "mirror"],
            ),
            s(
                NIGHT_LIGHT,
                NIGHT,
                w("Éclairage nocturne", "Night light"),
                (&["nuit", "lumière bleue"], &["night light", "blue light"]),
                &[],
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Rect = Rect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1600,
    };

    #[test]
    fn a_dropped_screen_lands_flush_on_the_nearest_side() {
        // Near the laptop's top, a little to the right: above it, its
        // middle caught.
        let dropped = Rect {
            x: -400,
            y: -1500,
            w: 3440,
            h: 1440,
        };
        assert_eq!(land(dropped, LAPTOP), (-440, -1440));
        // Overlapping its left half: on its left, flush, tops caught.
        let over = Rect {
            x: -2000,
            y: 10,
            w: 3440,
            h: 1440,
        };
        assert_eq!(land(over, LAPTOP), (-3440, 0));
        // Far away below: touching, 200 px shared at least.
        let far = Rect {
            x: 9000,
            y: 5000,
            w: 3440,
            h: 1440,
        };
        assert_eq!(land(far, LAPTOP), (2560 - 200, 1600));
    }

    #[test]
    fn a_place_is_said_and_kept_through_a_size_change() {
        let ext = Rect {
            x: -3440,
            y: 0,
            w: 3440,
            h: 1440,
        };
        let side = side_of(ext, LAPTOP);
        let align = align_of(ext, LAPTOP, side);
        assert_eq!((side, align), (Side::Left, Align::Start));
        assert_eq!(
            say_place(side, align, "Écran du portable", Lang::French),
            "à gauche de l’écran du portable, hauts alignés"
        );
        // Smaller now: still on the left, tops aligned.
        assert_eq!(placed((2560, 1080), LAPTOP, side, align), (-2560, 0));
        assert_eq!(
            normalised(&[("A".to_owned(), (0, 0)), ("B".to_owned(), (-2560, -10))]),
            vec![("A".to_owned(), (2560, 10)), ("B".to_owned(), (0, 0))]
        );
    }

    #[test]
    fn the_compositors_line_reads() {
        let said = read_line(
            r#"{"outputs":[{"name":"HEADLESS-2","built_in":false,"mm":[800,335],"modes":[[3440,1440,60000,true],[2560,1080,60000,false]],"mode":[3440,1440,60000],"scale":1.0,"scale_from":"auto","auto_scale":1.0,"position":[2560,0],"size":[3440,1440],"vrr_capable":false,"vrr":false,"enabled":true,"connected":true,"lid_closed":false,"primary":false,"file_mode":null,"mode_offered":true},{"name":"HEADLESS-1","modes":[],"mode":[2560,1600,60000],"position":[0,0],"size":[2560,1600],"primary":true}],"places_refused":false,"trial":null}"#,
        );
        let Some(Said::Screens(screens)) = said else {
            panic!("{said:?}");
        };
        assert_eq!(screens.list[0].name, "HEADLESS-1");
        assert_eq!(screens.list[0].number, 1);
        let ext = &screens.list[1];
        assert_eq!(ext.title(Lang::French), "Écran externe 34 pouces");
        assert_eq!(ext.sizes(), vec![(3440, 1440), (2560, 1080)]);
        assert_eq!(ext.rates((3440, 1440)), vec![60_000]);
        assert_eq!(mode_word((1920, 1080, 59_940)), "1920x1080@59.94");
        assert_eq!(say_rate(59_940, Lang::French), "59,94 Hz");
        assert_eq!(
            read_line(
                r#"{"trial":{"id":3,"output":"HEADLESS-2","mode":"2560x1080@60","before":"3440x1440@60","left":12}}"#
            ),
            Some(Said::Trial {
                id: 3,
                output: "HEADLESS-2".to_owned(),
                mode: "2560x1080@60".to_owned(),
                before: "3440x1440@60".to_owned(),
                left: 12
            })
        );
    }
}
