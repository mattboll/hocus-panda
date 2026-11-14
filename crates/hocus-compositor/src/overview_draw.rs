//! Drawing the overview (8.5 A, docs/design/windows.md §12b): the dimmed
//! wallpaper, each screen's desktops as live thumbnails (the windows'
//! own textures, nothing copied), the shown desktop's windows laid out
//! with their application's tile and title under each, the rim and ✕ of
//! the window under the pointer, the keyboard's ring, the agents' panda
//! badges and the empty desktops' sleeping pandas.
//!
//! What is not a window is a picture drawn once on the CPU (tiny-skia,
//! the shell's fonts) for each look it takes, at the screen's scale, and
//! kept: at rest the pictures stay, their elements' ids too, and nothing
//! is drawn again but what clients commit. The pandas are the validated
//! drawings (#649, `crate::pandas`): as the overview opens each badge's
//! plays its agent's mood once, a waiting one on and on, bobbing; the
//! sleeping ones breathe twice as two "z" rise. Still with reduced motion
//! or in power saver.

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    time::Duration,
};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, Kind, memory::MemoryRenderBuffer, solid::SolidColorRenderElement},
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Size},
};
use tiny_skia::{FillRule, PathBuilder, Pixmap, Transform};

use crate::{
    State,
    decoration::{ACCENT, BoxStyle, Keyed, Placed, box_elements, buffer},
    model::WindowId,
    overview::{
        AgentState, BREATHING, CLOSE_BUTTON, DIM, LABEL_GAP, LABEL_LINE, LABEL_SIZE, MINIMIZED,
        NUMBER_GAP, NUMBER_LINE, NUMBER_SIZE, Overview, Screen, Spot, WAVE, close_button,
    },
    radial_draw::{
        Fonts, advance, argb, fonts, letter_colour, letter_of, paint, rounded, stroke, write,
    },
    render::{OutputElement, OutputElements},
    switcher::Area,
};

const TEXT: [u8; 4] = [0xEE, 0xF1, 0xF6, 0xFF];
const FAINT: [u8; 4] = [0x8C, 0x96, 0xA8, 0xFF];
const DROP_INK: [u8; 4] = [0x7E, 0xE0, 0xA0, 0xFF];
/// The window under the pointer's rim, and a thumbnail a window would go
/// to: `#7EE0A0`, lit `rgba(108,194,135,0.28)`.
const HOVER: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: DROP_INK,
    rim_width: 2000,
    radius: 10_000,
};
const LIT: BoxStyle = BoxStyle {
    fill: [108, 194, 135, 71],
    rim: DROP_INK,
    rim_width: 2000,
    radius: 6_000,
};
const CURRENT: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: ACCENT,
    rim_width: 2000,
    radius: 6_000,
};
/// A thumbnail's edge, and its ground without a wallpaper.
const EDGE: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: [0xFF, 0xFF, 0xFF, 26],
    rim_width: 1000,
    radius: 6_000,
};
const GROUND: BoxStyle = BoxStyle {
    fill: [14, 15, 18, 255],
    rim: [0; 4],
    rim_width: 0,
    radius: 6_000,
};
/// The keyboard's selection: the accent, 4 px out.
const SELECTION: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: ACCENT,
    rim_width: 2000,
    radius: 12_000,
};
const SELECTION_OUT: f64 = 4.0;
/// The agents' badge (#649): a panda of at least 56 px, 18 % of its
/// window's width up to 110, its pill 8 px higher; its words 12.5 px at
/// 56 px, growing half as fast as the panda.
const PANDA: f64 = 56.0;
const PANDA_MOST: f64 = 110.0;
const PANDA_SHARE: f64 = 0.18;
const PILL_PAD: f64 = 4.0;
const BADGE_TEXT: f32 = 12.5;
const BADGE_IN: f64 = 10.0;
const BOB: f64 = 3.0;
/// Its halo, the state's colour.
const HALO: u8 = 56;
/// The sleeping panda: this share of the thumbnail's height, at most 96.
const SLEEP_SHARE: f64 = 0.75;
const SLEEP_MOST: f64 = 96.0;
/// The application's tile beside a title.
const TILE: f64 = 18.0;
const TILE_GAP: f64 = 8.0;
/// "En faire un projet" (docs/design/projects.md §2.1): 26 px high, the
/// accent's rim and tint, a 14 px folder-plus, 11.5 px 700.
pub const MAKE_HEIGHT: f64 = 26.0;
const MAKE_TEXT: f32 = 11.5;
const MAKE_ICON: f64 = 14.0;
const MAKE_PAD: (f64, f64) = (9.0, 11.0);
const MAKE_INK: [u8; 4] = [0x7E, 0xE0, 0xA0, 0xFF];
/// A project's label under its thumbnail: its monogram (7.5 px, 800) on
/// grey, the accent on the shown desktop, then "1 · hocus-panda" (11.5 px,
/// 700) on a 16 px line; its folder and what it holds (10.5 px,
/// `text.faint`) on 14 px lines, 2 px apart.
const NAME_LINE: f64 = 16.0;
const NAME_TEXT: f32 = 11.5;
const NAME_INK: [u8; 4] = [0xD5, 0xDB, 0xE5, 0xFF];
const MONO_TEXT: f32 = 7.5;
const MONO_INK: [u8; 4] = [0x0B, 0x10, 0x20, 0xFF];
const MONO_GREY: [u8; 4] = [0xA9, 0xB2, 0xC2, 0xFF];
const LABEL_LINES_GAP: f64 = 2.0;
/// A label reaches this far past its thumbnail's sides.
const LABEL_OVER: f64 = 18.0;

/// What a picture shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Pic {
    /// A window's tile and title, at most `width` pixels.
    Label {
        title: String,
        app: String,
        width: i32,
        scale: u32,
    },
    /// A thumbnail's number, or what letting go there does.
    Words {
        text: String,
        ink: [u8; 4],
        bold: bool,
        scale: u32,
    },
    /// The agents' badge's pill, the panda apart.
    Pill {
        words: String,
        state: AgentState,
        panda: u32,
        scale: u32,
    },
    /// A panda in `mood` at a frame of its burst, `side` pixels square,
    /// on a halo of its agents' state's colour (none: asleep).
    Panda {
        mood: hocus_pandas::Mood,
        frame: crate::pandas::Frame,
        side: u32,
        halo: Option<AgentState>,
    },
    /// The ✕ over a window.
    Close { scale: u32 },
    /// "En faire un projet".
    Make { text: String, scale: u32 },
    /// The project arrived on, under the desktops' dots as they switch.
    ProjectPill {
        mono: String,
        name: String,
        folder: String,
        scale: u32,
    },
    /// A project's label under its thumbnail, `width` pixels at most.
    Project {
        mono: String,
        title: String,
        folder: String,
        holds: String,
        current: bool,
        width: i32,
        scale: u32,
    },
}

/// Where a picture or a box is drawn: each its own element's ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Place {
    Label(WindowId),
    Number(String, usize),
    Pill(WindowId),
    Panda(WindowId),
    Sleep(String, usize),
    Close,
    Wall(String, usize),
    Ground(String, usize),
    Edge(String, usize),
    Ring(String, usize),
    Hover,
    Selection,
    Make(String, usize),
    MakeRing,
    ProjectPill(String),
}

/// A picture, its size in pixels and its serial: a new one for each
/// picture drawn, so that what shows it is drawn again.
type Drawn = (MemoryRenderBuffer, (i32, i32), u64);

/// The overview's pictures and ids.
pub struct OverviewPaint {
    fonts: OnceCell<Option<Fonts>>,
    pictures: RefCell<HashMap<Pic, Drawn>>,
    ids: RefCell<HashMap<(Place, u64), Id>>,
    boxes: RefCell<HashMap<Place, [Id; 9]>>,
    thumbs: RefCell<HashMap<(WindowId, usize, usize), Id>>,
    serial: Cell<u64>,
    dim: Id,
}

impl std::fmt::Debug for OverviewPaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverviewPaint")
            .field("pictures", &self.pictures.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Default for OverviewPaint {
    fn default() -> Self {
        Self {
            fonts: OnceCell::new(),
            pictures: RefCell::default(),
            ids: RefCell::default(),
            boxes: RefCell::default(),
            thumbs: RefCell::default(),
            serial: Cell::new(0),
            dim: Id::new(),
        }
    }
}

impl OverviewPaint {
    /// A window went: its ids go.
    pub fn forget(&self, id: WindowId) {
        self.thumbs.borrow_mut().retain(|(w, _, _), _| *w != id);
        self.ids.borrow_mut().retain(|(p, _), _| {
            !matches!(p, Place::Label(w) | Place::Pill(w) | Place::Panda(w) if *w == id)
        });
    }

    fn picture(&self, pic: &Pic, scale: f64) -> Option<Drawn> {
        if let Some(drawn) = self.pictures.borrow().get(pic) {
            return Some(drawn.clone());
        }
        let fonts = self.fonts.get_or_init(fonts).as_ref()?;
        let (pixels, size) = draw(fonts, pic, scale)?;
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        let drawn = (buffer(&pixels, size.0, size.1), size, serial);
        let mut pictures = self.pictures.borrow_mut();
        if pictures.len() > 512 {
            pictures.clear();
            self.ids.borrow_mut().clear();
        }
        pictures.insert(pic.clone(), drawn.clone());
        Some(drawn)
    }

    fn id(&self, place: &Place, serial: u64) -> Id {
        self.ids
            .borrow_mut()
            .entry((place.clone(), serial))
            .or_insert_with(Id::new)
            .clone()
    }

    fn box_ids(&self, place: &Place) -> [Id; 9] {
        self.boxes
            .borrow_mut()
            .entry(place.clone())
            .or_insert_with(|| std::array::from_fn(|_| Id::new()))
            .clone()
    }

    fn thumb_id(&self, id: WindowId, desktop: usize, part: usize) -> Id {
        self.thumbs
            .borrow_mut()
            .entry((id, desktop, part))
            .or_insert_with(Id::new)
            .clone()
    }
}

/// Draws `pic` at `scale`: its premultiplied ARGB pixels and its size.
fn draw(fonts: &Fonts, pic: &Pic, scale: f64) -> Option<(Vec<u8>, (i32, i32))> {
    match pic {
        Pic::Label {
            title, app, width, ..
        } => draw_label(fonts, title, app, *width, scale),
        Pic::Words {
            text, ink, bold, ..
        } => {
            let font = if *bold { &fonts.bold } else { &fonts.semibold };
            let px = (NUMBER_SIZE * scale) as f32 + if *bold { 0.5 } else { 0.0 };
            draw_words(font, text, px, *ink, scale)
        }
        Pic::Pill {
            words,
            state,
            panda,
            ..
        } => draw_pill(fonts, words, *state, f64::from(*panda), scale),
        Pic::Panda {
            mood,
            frame,
            side,
            halo,
        } => draw_panda(*mood, *frame, *side, *halo),
        Pic::Close { .. } => draw_close(scale),
        Pic::Make { text, .. } => draw_make(fonts, text, scale),
        Pic::ProjectPill {
            mono, name, folder, ..
        } => draw_project_pill(fonts, (mono, name, folder), scale),
        Pic::Project {
            mono,
            title,
            folder,
            holds,
            current,
            width,
            ..
        } => draw_project(fonts, (mono, title, folder, holds), *current, *width, scale),
    }
}

/// `text` cut to `room` pixels with "…" at its start (a folder's end is
/// what tells it apart).
fn cut_start(font: &fontdue::Font, text: &str, px: f32, room: f32) -> String {
    if advance(font, text, px) <= room {
        return text.to_owned();
    }
    let ellipsis = advance(font, "…", px);
    let mut chars: std::collections::VecDeque<char> = text.chars().collect();
    while !chars.is_empty()
        && advance(font, &chars.iter().collect::<String>(), px) + ellipsis > room
    {
        chars.pop_front();
    }
    format!("…{}", chars.into_iter().collect::<String>())
}

/// The folder-plus icon (the mockup's, a 24 px drawing) `side` pixels
/// square at `(x, y)`, stroked in `ink`.
fn folder_plus(pixmap: &mut Pixmap, (x, y): (f32, f32), side: f32, ink: [u8; 4]) {
    let k = side / 24.0;
    let p = |a: f32, b: f32| (x + a * k, y + b * k);
    let mut pb = PathBuilder::new();
    let (a, b) = p(3.0, 7.0);
    pb.move_to(a, b);
    let (cx, cy) = p(3.0, 5.0);
    let (a, b) = p(5.0, 5.0);
    pb.quad_to(cx, cy, a, b);
    for (a, b) in [(9.0, 5.0), (11.0, 7.0), (19.0, 7.0)] {
        let (a, b) = p(a, b);
        pb.line_to(a, b);
    }
    for ((c1, c2), (e1, e2), (l1, l2)) in [
        ((21.0, 7.0), (21.0, 9.0), (21.0, 17.0)),
        ((21.0, 19.0), (19.0, 19.0), (5.0, 19.0)),
        ((3.0, 19.0), (3.0, 17.0), (3.0, 7.0)),
    ] {
        let (c1, c2) = p(c1, c2);
        let (e1, e2) = p(e1, e2);
        pb.quad_to(c1, c2, e1, e2);
        let (l1, l2) = p(l1, l2);
        pb.line_to(l1, l2);
    }
    pb.close();
    for ((a, b), (c, d)) in [((12.0, 11.0), (12.0, 16.0)), ((9.5, 13.5), (14.5, 13.5))] {
        let (a, b) = p(a, b);
        let (c, d) = p(c, d);
        pb.move_to(a, b);
        pb.line_to(c, d);
    }
    if let Some(path) = pb.finish() {
        let mut stroked = stroke(2.0 * k);
        stroked.line_cap = tiny_skia::LineCap::Round;
        stroked.line_join = tiny_skia::LineJoin::Round;
        pixmap.stroke_path(&path, &paint(ink), &stroked, Transform::identity(), None);
    }
}

/// "En faire un projet": a 26 px pill, the accent's tint inside its rim,
/// the folder-plus and the words in the lighter accent.
fn draw_make(fonts: &Fonts, text: &str, scale: f64) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let px = MAKE_TEXT * k;
    let icon = (MAKE_ICON * scale) as f32;
    let (left, right) = ((MAKE_PAD.0 * scale) as f32, (MAKE_PAD.1 * scale) as f32);
    let text_w = advance(&fonts.bold, text, px);
    let w = (1.0 + left + icon + 6.0 * k + text_w + right + 1.0).ceil() as u32;
    let h = (MAKE_HEIGHT * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let (fw, fh) = (w as f32, h as f32);
    if let Some(path) = rounded(0.5, 0.5, fw - 1.0, fh - 1.0, fh / 2.0 - 0.5) {
        pixmap.fill_path(
            &path,
            &paint([108, 194, 135, 31]),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint([108, 194, 135, 140]),
            &stroke(k),
            Transform::identity(),
            None,
        );
    }
    folder_plus(&mut pixmap, (1.0 + left, (fh - icon) / 2.0), icon, MAKE_INK);
    write(
        &mut pixmap,
        &fonts.bold,
        text,
        px,
        (1.0 + left + icon + 6.0 * k, fh / 2.0 + px * 0.36),
        MAKE_INK,
        0.0,
    );
    Some((argb(pixmap), (w as i32, h as i32)))
}

/// The project arrived on (docs/design/projects.md §3): a 30 px pill,
/// `rgba(14,17,28,0.88)` in the bar's rim, its monogram (9 px, 800, on the
/// accent), its name (13.5 px, 700), its folder (12 px, `text.faint`).
fn draw_project_pill(
    fonts: &Fonts,
    (mono, name, folder): (&str, &str, &str),
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let (mono_px, name_px, folder_px) = (9.0 * k, 13.5 * k, 12.0 * k);
    let badge_w = advance(&fonts.bold, mono, mono_px) + 8.0 * k;
    let badge_h = (mono_px * 1.37 + 4.0 * k).round();
    let name_w = advance(&fonts.bold, name, name_px);
    let folder_w = advance(&fonts.semibold, folder, folder_px);
    let gap = 8.0 * k;
    let h = (30.0 * k).ceil();
    let inner = badge_w
        + gap
        + name_w
        + if folder.is_empty() {
            0.0
        } else {
            gap + folder_w
        };
    let w = (8.0 * k + inner + 14.0 * k).ceil();
    let mut pixmap = Pixmap::new(w as u32 + 2, h as u32 + 2)?;
    if let Some(path) = rounded(1.0, 1.0, w, h, h / 2.0) {
        pixmap.fill_path(
            &path,
            &paint([14, 17, 28, 224]),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint([0xFF, 0xFF, 0xFF, 26]),
            &stroke(k),
            Transform::identity(),
            None,
        );
    }
    let middle = 1.0 + h / 2.0;
    let mut x = 1.0 + 8.0 * k;
    if let Some(path) = rounded(x, middle - badge_h / 2.0, badge_w, badge_h, 4.0 * k) {
        pixmap.fill_path(
            &path,
            &paint(ACCENT),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    write(
        &mut pixmap,
        &fonts.bold,
        mono,
        mono_px,
        (x + 4.0 * k, middle + mono_px * 0.36),
        MONO_INK,
        0.0,
    );
    x += badge_w + gap;
    write(
        &mut pixmap,
        &fonts.bold,
        name,
        name_px,
        (x, middle + name_px * 0.36),
        TEXT,
        0.0,
    );
    x += name_w + gap;
    write(
        &mut pixmap,
        &fonts.semibold,
        folder,
        folder_px,
        (x, middle + folder_px * 0.36),
        FAINT,
        0.0,
    );
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    Some((argb(pixmap), (pw, ph)))
}

/// A project's label: its monogram and "N · name", its folder, what it
/// holds, each line centred, `width` pixels at most.
fn draw_project(
    fonts: &Fonts,
    (mono, title, folder, holds): (&str, &str, &str, &str),
    current: bool,
    width: i32,
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let room = width as f32 - 4.0 * k;
    let name_px = NAME_TEXT * k;
    let small = (NUMBER_SIZE * scale) as f32;
    let mono_px = MONO_TEXT * k;
    let badge_w = advance(&fonts.bold, mono, mono_px) + 6.0 * k;
    let badge_h = (mono_px * 1.37 + 2.0 * k).round();
    let gap = 6.0 * k;
    let title = cut(&fonts.bold, title, name_px, room - badge_w - gap);
    let folder = cut_start(&fonts.semibold, folder, small, room);
    let holds = cut(&fonts.semibold, holds, small, room);
    let line1 = badge_w + gap + advance(&fonts.bold, &title, name_px);
    let line2 = advance(&fonts.semibold, &folder, small);
    let line3 = advance(&fonts.semibold, &holds, small);
    let w = (line1.max(line2).max(line3) + 4.0 * k).ceil() as u32;
    let (name_h, small_h, lines_gap) = (
        (NAME_LINE * scale) as f32,
        (NUMBER_LINE * scale) as f32,
        (LABEL_LINES_GAP * scale) as f32,
    );
    let h = (name_h + 2.0 * (lines_gap + small_h)).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let fw = w as f32;
    // The first line: the badge, then the name.
    let x = (fw - line1) / 2.0;
    let top = (name_h - badge_h) / 2.0;
    if let Some(path) = rounded(x, top, badge_w, badge_h, 3.0 * k) {
        pixmap.fill_path(
            &path,
            &paint(if current { ACCENT } else { MONO_GREY }),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    write(
        &mut pixmap,
        &fonts.bold,
        mono,
        mono_px,
        (x + 3.0 * k, top + badge_h / 2.0 + mono_px * 0.36),
        MONO_INK,
        0.0,
    );
    write(
        &mut pixmap,
        &fonts.bold,
        &title,
        name_px,
        (x + badge_w + gap, name_h / 2.0 + name_px * 0.36),
        NAME_INK,
        1.2 * k,
    );
    for (i, (line, line_w)) in [(&folder, line2), (&holds, line3)].into_iter().enumerate() {
        let middle = name_h + lines_gap + small_h / 2.0 + i as f32 * (small_h + lines_gap);
        write(
            &mut pixmap,
            &fonts.semibold,
            line,
            small,
            ((fw - line_w) / 2.0, middle + small * 0.36),
            FAINT,
            1.2 * k,
        );
    }
    Some((argb(pixmap), (w as i32, h as i32)))
}

/// `text` cut to `room` pixels with "…".
fn cut(font: &fontdue::Font, text: &str, px: f32, room: f32) -> String {
    if advance(font, text, px) <= room {
        return text.to_owned();
    }
    let ellipsis = advance(font, "…", px);
    let mut shown: String = text.to_owned();
    while !shown.is_empty() && advance(font, &shown, px) + ellipsis > room {
        shown.pop();
    }
    format!("{}…", shown.trim_end())
}

/// The application's letter tile, then the title, in `width` pixels at
/// most, on a 20 px line.
fn draw_label(
    fonts: &Fonts,
    title: &str,
    app: &str,
    width: i32,
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let px = (LABEL_SIZE * scale) as f32;
    let tile = (TILE * scale) as f32;
    let gap = (TILE_GAP * scale) as f32;
    let room = (width as f32 - tile - gap).max(px);
    let shown = cut(&fonts.semibold, title, px, room);
    let w = (tile + gap + advance(&fonts.semibold, &shown, px) + 3.0 * k).ceil() as u32;
    let h = (LABEL_LINE * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let top = (h as f32 - tile) / 2.0;
    if let Some(path) = rounded(0.0, top, tile, tile, 5.0 * k) {
        pixmap.fill_path(
            &path,
            &paint(letter_colour(app)),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let letter = letter_of(app);
    let lpx = 11.0 * k;
    let lw = advance(&fonts.bold, &letter, lpx);
    write(
        &mut pixmap,
        &fonts.bold,
        &letter,
        lpx,
        ((tile - lw) / 2.0, top + tile / 2.0 + lpx * 0.36),
        [0x0E, 0x11, 0x1C, 0xFF],
        0.0,
    );
    let baseline = h as f32 / 2.0 + px * 0.36;
    write(
        &mut pixmap,
        &fonts.semibold,
        &shown,
        px,
        (tile + gap, baseline),
        TEXT,
        1.5 * k,
    );
    Some((argb(pixmap), (w as i32, h as i32)))
}

/// `text` alone, outlined, on a 14 px line.
fn draw_words(
    font: &fontdue::Font,
    text: &str,
    px: f32,
    ink: [u8; 4],
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let w = (advance(font, text, px) + 4.0 * k).ceil() as u32;
    let h = (NUMBER_LINE * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    write(
        &mut pixmap,
        font,
        text,
        px,
        (2.0 * k, h as f32 / 2.0 + px * 0.36),
        ink,
        1.2 * k,
    );
    Some((argb(pixmap), (w as i32, h as i32)))
}

/// The badge's pill: dark, ringed in its state's colour at 70 %, room
/// for the panda on its left, its words on its right.
fn draw_pill(
    fonts: &Fonts,
    words: &str,
    state: AgentState,
    panda: f64,
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let px = BADGE_TEXT * k * (0.5 + 0.5 * panda / PANDA) as f32;
    let height = ((panda + 2.0 * PILL_PAD) * scale) as f32;
    let text_w = advance(&fonts.bold, words, px);
    let width = ((panda + PILL_PAD) * scale) as f32 + 6.0 * k + text_w + 14.0 * k;
    let (w, h) = (width.ceil() as u32 + 2, height.ceil() as u32 + 2);
    let mut pixmap = Pixmap::new(w, h)?;
    let ring = 1.5 * k;
    if let Some(path) = rounded(1.0, 1.0, width, height, height / 2.0) {
        pixmap.fill_path(
            &path,
            &paint([14, 17, 28, 230]),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let [r, g, b, _] = state.colour();
        pixmap.stroke_path(
            &path,
            &paint([r, g, b, 179]),
            &stroke(ring),
            Transform::identity(),
            None,
        );
    }
    write(
        &mut pixmap,
        &fonts.bold,
        words,
        px,
        (
            1.0 + ((panda + PILL_PAD) * scale) as f32 + 6.0 * k,
            1.0 + height / 2.0 + px * 0.36,
        ),
        state.ink(),
        0.0,
    );
    Some((argb(pixmap), (w as i32, h as i32)))
}

/// The ✕: a 24 px dark disc ringed in white at 18 %, a 10 px cross.
fn draw_close(scale: f64) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let side = (CLOSE_BUTTON * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(side, side)?;
    let c = side as f32 / 2.0;
    if let Some(path) = PathBuilder::from_circle(c, c, c - 1.0) {
        pixmap.fill_path(
            &path,
            &paint([14, 17, 28, 235]),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint([0xFF, 0xFF, 0xFF, 46]),
            &stroke(k),
            Transform::identity(),
            None,
        );
    }
    let arm = 5.0 * k;
    let mut pb = PathBuilder::new();
    pb.move_to(c - arm, c - arm);
    pb.line_to(c + arm, c + arm);
    pb.move_to(c + arm, c - arm);
    pb.line_to(c - arm, c + arm);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(
            &path,
            &paint(TEXT),
            &stroke(1.6 * k),
            Transform::identity(),
            None,
        );
    }
    Some((argb(pixmap), (side as i32, side as i32)))
}

/// A panda (`crate::pandas`) `side` pixels square, on a disc of its
/// agents' state's colour (`halo`).
fn draw_panda(
    mood: hocus_pandas::Mood,
    frame: crate::pandas::Frame,
    side: u32,
    halo: Option<AgentState>,
) -> Option<(Vec<u8>, (i32, i32))> {
    let mut pixels = crate::pandas::draw(mood, frame, side)?;
    if let Some(state) = halo {
        // The disc under the drawing: drawn alone, the drawing over it.
        let mut pixmap = Pixmap::new(side.max(1), side.max(1))?;
        let c = side as f32 / 2.0;
        if let Some(path) = PathBuilder::from_circle(c, c, c) {
            let [r, g, b, _] = state.colour();
            pixmap.fill_path(
                &path,
                &paint([r, g, b, HALO]),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let disc = argb(pixmap);
        let (over, under) = (pixels.as_chunks_mut::<4>().0, disc.as_chunks::<4>().0);
        for (over, under) in over.iter_mut().zip(under) {
            let a = u16::from(over[3]);
            for (over, under) in over.iter_mut().zip(under) {
                *over = over.saturating_add(((u16::from(*under) * (255 - a)) / 255) as u8);
            }
        }
    }
    Some((pixels, (side as i32, side as i32)))
}

/// What the overview's pieces are drawn with and into: the renderer, the
/// elements so far, the output's origin and scale, the time.
struct Canvas<'a, R: Renderer> {
    renderer: &'a mut R,
    out: &'a mut Vec<OutputElement<R>>,
    origin: Point<f64, Logical>,
    scale: f64,
    now: Duration,
}

/// The overview's windows laid out, front first, and their titles: each
/// title's place, picture, where it goes and its alpha.
type LaidOut = (
    Vec<(Place, Pic, Point<f64, Logical>, f32)>,
    Vec<(WindowId, crate::render::Look)>,
);

/// How a desktop's thumbnail is marked: a dragged window over it, the
/// current one, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Over,
    Current,
    Other,
}

fn local(a: Area, origin: Point<f64, Logical>) -> Rectangle<f64, Logical> {
    Rectangle::new((a.x - origin.x, a.y - origin.y).into(), (a.w, a.h).into())
}

impl State {
    /// The home a project's folder is shown from (`~`).
    fn project_home(&self) -> String {
        self.config
            .home
            .as_ref()
            .map(|h| h.display().to_string())
            .unwrap_or_default()
    }

    /// The project arrived on as `output`'s strip moves, its pill centred
    /// `top` pixels from the screen's top, as opaque as the dots.
    pub(crate) fn project_pill_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        top: f64,
        alpha: f32,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(project) = self.project_on(output, self.strip(output).current()) else {
            return;
        };
        let Some(geometry) = self.space.output_geometry(output) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let home = self.project_home();
        let pic = Pic::ProjectPill {
            mono: hocus_link::projects::monogram(&project.name),
            name: project.name.clone(),
            folder: hocus_link::projects::tilde(&project.folder.display().to_string(), &home),
            scale: milli(scale),
        };
        let Some((_, (w, _), _)) = self.overview_paint.picture(&pic, scale) else {
            return;
        };
        let lw = f64::from(w) / scale;
        let at = Point::<f64, Logical>::from(((f64::from(geometry.size.w) - lw) / 2.0, top));
        let mut c = Canvas {
            renderer,
            out,
            origin: geometry.loc.to_f64(),
            scale,
            now: Duration::ZERO,
        };
        self.overview_picture(
            &mut c,
            Place::ProjectPill(output.name()),
            &pic,
            at,
            None,
            alpha,
        );
    }

    /// The picture of the label of desktop `desktop` of `output` under its
    /// `thumb` (global), if it is a project.
    fn overview_project_pic(
        &self,
        output: &Output,
        desktop: usize,
        thumb: Area,
        scale: f64,
    ) -> Option<Pic> {
        use hocus_words::compositor::overview_draw as txt;
        let project = self.project_on(output, desktop)?;
        let lang = lang_of(self);
        let windows: Vec<WindowId> = self
            .windows_on(output)
            .into_iter()
            .filter(|id| self.model.desktop(*id) == desktop && !self.model.is_everywhere(*id))
            .collect();
        let agents: usize = self.overview.as_ref().map_or(0, |o| {
            windows
                .iter()
                .map(|id| o.agents.get(id).copied().unwrap_or(0))
                .sum()
        });
        let mut holds = match windows.len() {
            0 => txt::PROJECT_NO_WINDOW.get(lang).to_owned(),
            n => txt::project_windows(lang, n),
        };
        if agents > 0 {
            holds.push_str(" · ");
            holds.push_str(&txt::project_agents(lang, agents));
        }
        let home = self.project_home();
        Some(Pic::Project {
            mono: hocus_link::projects::monogram(&project.name),
            title: format!("{} · {}", desktop + 1, project.name),
            folder: hocus_link::projects::tilde(&project.folder.display().to_string(), &home),
            holds,
            current: desktop == self.strip(output).current(),
            width: ((thumb.w + 2.0 * LABEL_OVER) * scale) as i32,
            scale: milli(scale),
        })
    }

    /// Where a project's name is under its `thumb` (global): the label's
    /// first line, as wide as the label.
    pub(crate) fn overview_name_area(
        &self,
        output: &Output,
        desktop: usize,
        thumb: Area,
        scale: f64,
    ) -> Option<Area> {
        let pic = self.overview_project_pic(output, desktop, thumb, scale)?;
        let (_, (w, _), _) = self.overview_paint.picture(&pic, scale)?;
        let lw = f64::from(w) / scale;
        Some(Area {
            x: thumb.x + (thumb.w - lw) / 2.0,
            y: thumb.y + thumb.h + NUMBER_GAP,
            w: lw,
            h: NAME_LINE,
        })
    }

    /// Where "En faire un projet" is under `thumb` (global).
    pub(crate) fn overview_make_area(&self, thumb: Area, scale: f64) -> Option<Area> {
        let pic = Pic::Make {
            text: hocus_words::compositor::overview_draw::MAKE_PROJECT
                .get(lang_of(self))
                .to_owned(),
            scale: milli(scale),
        };
        let (_, (w, h), _) = self.overview_paint.picture(&pic, scale)?;
        let (lw, lh) = (f64::from(w) / scale, f64::from(h) / scale);
        Some(Area {
            x: thumb.x + (thumb.w - lw) / 2.0,
            y: thumb.y + thumb.h + NUMBER_GAP + NUMBER_LINE + crate::overview::MAKE_GAP,
            w: lw,
            h: lh,
        })
    }

    /// Whether desktop `desktop` of screen `output`'s button shows: the
    /// pointer rests on its thumbnail or under it, or the keyboard's ring
    /// is on it; never while a window is dragged nor on a project.
    fn overview_button_shows(&self, output: &str, desktop: usize) -> bool {
        let Some(overview) = self.overview.as_ref().filter(|o| o.open()) else {
            return false;
        };
        if overview.drag.is_some() {
            return false;
        }
        let hovered = matches!(
            &overview.hover,
            Some(Spot::Thumb(o, d) | Spot::Make(o, d) | Spot::Below(o, d))
                if o == output && *d == desktop
        );
        let ringed = overview.button.as_deref() == Some(output)
            && self
                .space
                .outputs()
                .find(|o| o.name() == output)
                .is_some_and(|o| self.strip(o).current() == desktop);
        hovered || ringed
    }
}

impl State {
    /// A picture's element at `at` (logical, output-local, its top-left),
    /// pixel for pixel; or stretched over `size` when given.
    fn overview_picture<R>(
        &self,
        c: &mut Canvas<'_, R>,
        place: Place,
        pic: &Pic,
        at: Point<f64, Logical>,
        size: Option<(f64, f64)>,
        alpha: f32,
    ) -> Option<(f64, f64)>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = c.scale;
        let paint = &self.overview_paint;
        let (texture, (w, h), serial) = paint.picture(pic, scale)?;
        let logical = (f64::from(w) / scale, f64::from(h) / scale);
        let corner = at.to_physical(scale).to_i32_round::<i32>();
        let dst: Rectangle<i32, Physical> = match size {
            Some((sw, sh)) => Rectangle::new(
                corner,
                Size::<f64, Logical>::from((sw, sh))
                    .to_physical(scale)
                    .to_i32_round::<i32>(),
            ),
            None => Rectangle::new(corner, (w, h).into()),
        };
        let all = Rectangle::from_size((f64::from(w), f64::from(h)).into());
        match Placed::new(c.renderer, &texture, all, dst, alpha) {
            Ok(e) => c.out.push(OutputElements::Frame(Keyed::new(
                e,
                paint.id(&place, serial),
            ))),
            Err(err) => tracing::warn!(?err, "could not draw the overview"),
        }
        Some(logical)
    }

    /// A box of `style` over `rect` (output-local).
    fn overview_box<R>(
        &self,
        c: &mut Canvas<'_, R>,
        place: Place,
        style: BoxStyle,
        rect: Rectangle<f64, Logical>,
        alpha: f32,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let ids = self.overview_paint.box_ids(&place);
        // High contrast: no tints, solid rims (crate::contrast).
        let style = BoxStyle {
            fill: crate::contrast::ink(style.fill),
            rim: crate::contrast::rim(style.rim),
            ..style
        };
        c.out.extend(
            box_elements(
                c.renderer,
                &self.textures,
                &ids,
                style,
                rect,
                c.scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Frame),
        );
    }

    /// The overview on `output` at `now`, front first; nothing without it.
    pub(crate) fn overview_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let mut out = Vec::new();
        let Some(overview) = &self.overview else {
            return out;
        };
        let Some(geometry) = self.space.output_geometry(output) else {
            return out;
        };
        let screens = self.overview_screens();
        let Some(screen) = screens.iter().find(|s| s.output == *output) else {
            return out;
        };
        let origin = geometry.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let shown = overview.shown(now);
        let alpha = shown as f32;
        let reduced = overview.reduced;
        let lang = self.radial_lang();
        let c = &mut Canvas {
            renderer,
            out: &mut out,
            origin,
            scale,
            now,
        };
        // The thumbnail a dragged window is over, lit, in front of it.
        if let Some((name, desktop)) = overview.drag.as_ref().and_then(|d| d.over.as_ref())
            && *name == output.name()
            && let Some(thumb) = screen.thumbs.get(*desktop)
        {
            self.overview_thumb_mark(c, name, *desktop, local(*thumb, origin), Mark::Over, alpha);
        }
        // The window dragged, over everything but the bar, on whichever
        // screen it is over.
        if let Some(drag) = &overview.drag {
            let rect = drag.rect(now, reduced);
            let global =
                Rectangle::<f64, Logical>::new((rect.x, rect.y).into(), (rect.w, rect.h).into());
            if global.overlaps(geometry.to_f64())
                && let Some(look) = self.look(drag.window, now)
            {
                let look = look.placed(global, 1.0, true);
                self.framed_window(c.renderer, drag.window, &look, origin, scale, c.out);
            }
        }
        let strip = self.strip(output);
        let current = strip.current();
        let slots = self.overview_slots(screen, current);
        let hovered = match &overview.hover {
            Some(crate::overview::Spot::Window(id) | crate::overview::Spot::Close(id)) => Some(*id),
            _ => None,
        };
        let settled = shown >= 1.0 && !strip_moving(self, output, now);
        // The ✕ of the window under the pointer.
        if settled
            && let Some((_, rect)) = hovered.and_then(|h| slots.iter().find(|(id, _)| *id == h))
        {
            let button = close_button(*rect);
            self.overview_picture(
                c,
                Place::Close,
                &Pic::Close {
                    scale: milli(scale),
                },
                local(button, origin).loc,
                None,
                1.0,
            );
        }
        // The agents' badges.
        for (id, rect) in &slots {
            if !overview.badges.contains_key(id) {
                continue;
            }
            let at = glide(self, *id, *rect, shown, reduced, now);
            self.overview_badge(c, overview, *id, at, alpha, lang);
        }
        // The rim of the window under the pointer, the keyboard's ring.
        if settled {
            if let Some((_, rect)) = hovered.and_then(|h| slots.iter().find(|(id, _)| *id == h)) {
                self.overview_box(c, Place::Hover, HOVER, local(*rect, origin), 1.0);
            }
            if overview.ring
                && let Some((_, rect)) = overview
                    .selected
                    .and_then(|s| slots.iter().find(|(id, _)| *id == s))
            {
                let around = Area {
                    x: rect.x - SELECTION_OUT,
                    y: rect.y - SELECTION_OUT,
                    w: rect.w + 2.0 * SELECTION_OUT,
                    h: rect.h + 2.0 * SELECTION_OUT,
                };
                self.overview_box(c, Place::Selection, SELECTION, local(around, origin), 1.0);
            }
        }
        let (titles, mut windows) =
            self.overview_laid_out(overview, screen, &slots, origin, scale, now);
        for (place, pic, at, a) in titles {
            self.overview_picture(c, place, &pic, at, None, a);
        }
        // A window let go nowhere, on its way back: over the others.
        if let Some((id, from, tween)) = overview.back
            && let Some(to) = slots.iter().find(|(w, _)| *w == id).map(|(_, r)| *r)
            && let Some(look) = self.look(id, now)
        {
            let r = from.lerp(to, tween.at(now));
            let visual = Rectangle::<f64, Logical>::new((r.x, r.y).into(), (r.w, r.h).into());
            let look = look.placed(visual, 1.0, true);
            self.framed_window(c.renderer, id, &look, origin, scale, c.out);
            windows.retain(|(w, _)| *w != id);
        }
        for (id, look) in &windows {
            self.framed_window(c.renderer, *id, look, origin, scale, c.out);
        }
        // The desktops' thumbnails.
        self.overview_thumbs(c, overview, screen, alpha);
        // The wallpaper dimmed.
        let whole: Rectangle<i32, Physical> = Rectangle::from_size(
            geometry
                .size
                .to_f64()
                .to_physical(scale)
                .to_i32_round::<i32>(),
        );
        let a = DIM[3] * alpha;
        c.out
            .push(OutputElements::Solid(SolidColorRenderElement::new(
                self.overview_paint.dim.clone(),
                whole,
                (a * 1000.0) as usize,
                [DIM[0] * a, DIM[1] * a, DIM[2] * a, a],
                Kind::Unspecified,
            )));
        out
    }

    /// The windows laid out on `screen`'s desktops, front first, with
    /// their titles; while the strip slides (a thumbnail clicked), the
    /// desktops it passes. `slots`: the current desktop's layout.
    fn overview_laid_out(
        &self,
        overview: &Overview,
        screen: &Screen,
        slots: &[(WindowId, Area)],
        origin: Point<f64, Logical>,
        scale: f64,
        now: Duration,
    ) -> LaidOut {
        let output = &screen.output;
        let strip = self.strip(output);
        let current = strip.current();
        let shown = overview.shown(now);
        let alpha = shown as f32;
        let reduced = overview.reduced;

        let position = strip.position(now);
        let stride = self.strip_stride(output);
        let mut titles = Vec::new();
        let mut windows = Vec::new();
        for desktop in 0..strip.count() {
            let (offset, fade) = if reduced {
                (0.0, strip.alpha(desktop, now))
            } else {
                let away = desktop as f64 - position;
                if away.abs() >= 1.0 {
                    continue;
                }
                (away * stride, 1.0)
            };
            if fade <= 0.0 {
                continue;
            }
            let laid = if desktop == current {
                slots.to_vec()
            } else {
                self.overview_slots(screen, desktop)
            };
            // Front first, as they stand.
            let mut laid = laid;
            let stack = self.model.stack();
            laid.sort_by_key(|(id, _)| std::cmp::Reverse(stack.iter().position(|s| s == id)));
            for (id, rect) in laid {
                let mut rect = rect;
                rect.x += offset;
                let drawn = glide(self, id, rect, shown, reduced, now);
                let minimized = self.model.is_minimized(id);
                let mut a = if reduced || minimized { alpha } else { 1.0 };
                if minimized {
                    a *= MINIMIZED;
                }
                a *= fade;
                // Its title, under its place.
                let title = crate::windows::title_of(self, id);
                let app = self
                    .windows
                    .get(&id)
                    .and_then(|m| crate::windows::app_id(&m.window))
                    .unwrap_or_default();
                let width = ((rect.w.max(160.0)) * scale) as i32;
                let pic = Pic::Label {
                    title,
                    app,
                    width,
                    scale: milli(scale),
                };
                if let Some(drawn_pic) = self.overview_paint.picture(&pic, scale) {
                    let (w, _) = drawn_pic.1;
                    let lw = f64::from(w) / scale;
                    let at = Point::<f64, Logical>::from((
                        drawn.x + (drawn.w - lw) / 2.0 - origin.x,
                        drawn.y + drawn.h + LABEL_GAP - origin.y,
                    ));
                    titles.push((
                        Place::Label(id),
                        pic,
                        at,
                        alpha * fade * if minimized { MINIMIZED } else { 1.0 },
                    ));
                }
                if let Some(look) = self.look(id, now) {
                    let visual = Rectangle::<f64, Logical>::new(
                        (drawn.x, drawn.y).into(),
                        (drawn.w, drawn.h).into(),
                    );
                    let look = look.placed(visual, a, false);
                    windows.push((id, look));
                }
            }
        }
        (titles, windows)
    }

    /// Window `id`'s agents' badge, if it has one: its pill at its
    /// top-left, the panda on it, bobbing and waving while one waits.
    fn overview_badge<R>(
        &self,
        c: &mut Canvas<'_, R>,
        overview: &Overview,
        id: WindowId,
        at: Area,
        alpha: f32,
        lang: hocus_words::Lang,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(&badge) = overview.badges.get(&id) else {
            return;
        };
        let (origin, scale, now, opened) = (c.origin, c.scale, c.now, overview.opened);
        let corner =
            Point::<f64, Logical>::from((at.x + BADGE_IN - origin.x, at.y + BADGE_IN - origin.y));
        let lively = self.pandas_lively();
        let mood = crate::pandas::mood(badge.state);
        let since = now.saturating_sub(opened);
        // Its mood once as the overview opens; a waiting one's again and
        // again, bobbing.
        let (frame, bob) = match (lively, badge.state) {
            (false, _) => (None, 0.0),
            (true, AgentState::Waiting) => {
                let lasts = hocus_pandas::lasts(mood.burst());
                let t = Duration::from_secs_f64(since.as_secs_f64() % lasts.as_secs_f64());
                let phase = (now.as_secs_f64() / WAVE.as_secs_f64()).fract();
                (
                    crate::pandas::frame(mood.burst(), t),
                    -BOB * (0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()),
                )
            }
            (true, _) => (crate::pandas::frame(mood.burst(), since), 0.0),
        };
        let panda = (at.w * PANDA_SHARE).clamp(PANDA, PANDA_MOST).round();
        let side = (panda * scale).ceil() as u32;
        let panda_at =
            Point::<f64, Logical>::from((corner.x + PILL_PAD, corner.y + PILL_PAD + bob));
        self.overview_picture(
            c,
            Place::Panda(id),
            &Pic::Panda {
                mood,
                frame,
                side,
                halo: Some(badge.state),
            },
            panda_at,
            Some((panda, panda)),
            alpha,
        );
        self.overview_picture(
            c,
            Place::Pill(id),
            &Pic::Pill {
                words: badge.words(lang),
                state: badge.state,
                panda: panda as u32,
                scale: milli(scale),
            },
            corner - Point::from((1.0 / scale, 1.0 / scale)),
            None,
            alpha,
        );
    }

    /// A thumbnail's number and its ring: the current one's accent, the
    /// one a dragged window is over lit, "Lâcher : bureau N" under it.
    fn overview_thumb_mark<R>(
        &self,
        c: &mut Canvas<'_, R>,
        name: &str,
        desktop: usize,
        rect: Rectangle<f64, Logical>,
        mark: Mark,
        alpha: f32,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = c.scale;
        let over = mark == Mark::Over;
        let output = self.space.outputs().find(|o| o.name() == name).cloned();
        let names: Vec<String> = output
            .as_ref()
            .map(|o| self.desktop_names_on(o))
            .unwrap_or_default();
        let name = name.to_owned();
        // A project's label (docs/design/projects.md §2.1), in place of its
        // number; "En faire un projet" under the number of the one the
        // pointer rests on, or the keyboard's ring holds.
        if !over && let Some(output) = &output {
            let thumb = Area {
                x: rect.loc.x + c.origin.x,
                y: rect.loc.y + c.origin.y,
                w: rect.size.w,
                h: rect.size.h,
            };
            if let Some(pic) = self.overview_project_pic(output, desktop, thumb, scale) {
                if let Some((_, (w, _), _)) = self.overview_paint.picture(&pic, scale) {
                    let lw = f64::from(w) / scale;
                    let at = Point::<f64, Logical>::from((
                        rect.loc.x + (rect.size.w - lw) / 2.0,
                        rect.loc.y + rect.size.h + NUMBER_GAP,
                    ));
                    self.overview_picture(
                        c,
                        Place::Number(name.clone(), desktop),
                        &pic,
                        at,
                        None,
                        alpha,
                    );
                }
                if mark == Mark::Current {
                    self.overview_box(c, Place::Ring(name.clone(), desktop), CURRENT, rect, alpha);
                }
                return;
            }
            if self.overview_button_shows(&name, desktop)
                && let Some(area) = self.overview_make_area(thumb, scale)
            {
                let pic = Pic::Make {
                    text: hocus_words::compositor::overview_draw::MAKE_PROJECT
                        .get(lang_of(self))
                        .to_owned(),
                    scale: milli(scale),
                };
                self.overview_picture(
                    c,
                    Place::Make(name.clone(), desktop),
                    &pic,
                    local(area, c.origin).loc,
                    None,
                    alpha,
                );
                if self
                    .overview
                    .as_ref()
                    .is_some_and(|o| o.button.as_deref() == Some(name.as_str()))
                {
                    let around = Area {
                        x: area.x - SELECTION_OUT,
                        y: area.y - SELECTION_OUT,
                        w: area.w + 2.0 * SELECTION_OUT,
                        h: area.h + 2.0 * SELECTION_OUT,
                    };
                    let style = BoxStyle {
                        radius: ((area.h / 2.0 + SELECTION_OUT) * 1000.0) as u32,
                        ..SELECTION
                    };
                    self.overview_box(c, Place::MakeRing, style, local(around, c.origin), alpha);
                }
            }
        }
        // Under it: its number, or what letting go there does.
        let (text, ink, bold) = if over {
            let words =
                hocus_words::compositor::overview_draw::drop_on_desktop(lang_of(self), desktop + 1);
            (words, DROP_INK, true)
        } else {
            let text = match names.get(desktop).filter(|n| !n.is_empty()) {
                Some(n) => format!("{} · {n}", desktop + 1),
                None => format!("{}", desktop + 1),
            };
            (text, FAINT, false)
        };
        let pic = Pic::Words {
            text,
            ink,
            bold,
            scale: milli(scale),
        };
        if let Some((_, (w, _), _)) = self.overview_paint.picture(&pic, scale) {
            let lw = f64::from(w) / scale;
            let at = Point::<f64, Logical>::from((
                rect.loc.x + (rect.size.w - lw) / 2.0,
                rect.loc.y + rect.size.h + NUMBER_GAP,
            ));
            self.overview_picture(
                c,
                Place::Number(name.clone(), desktop),
                &pic,
                at,
                None,
                alpha,
            );
        }
        // Ringed or lit.
        if over {
            self.overview_box(c, Place::Ring(name.clone(), desktop), LIT, rect, alpha);
        } else if mark == Mark::Current {
            self.overview_box(c, Place::Ring(name.clone(), desktop), CURRENT, rect, alpha);
        }
    }

    /// `screen`'s thumbnails: each desktop's windows small over its
    /// wallpaper, its number under it; the current one ringed in the
    /// accent, the one a dragged window is over lit; an empty one's
    /// sleeping panda.
    fn overview_thumbs<R>(
        &self,
        c: &mut Canvas<'_, R>,
        overview: &Overview,
        screen: &Screen,
        alpha: f32,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (origin, scale, now) = (c.origin, c.scale, c.now);
        let output = &screen.output;
        let name = output.name();
        let current = self.strip(output).current();
        let wall = (!self.config.plain_background)
            .then(|| {
                let backgrounds = self.graphics.backgrounds.borrow();
                backgrounds
                    .shown
                    .get(&name)
                    .map(|s| (s.buffer.clone(), s.picture))
            })
            .flatten();
        let lively = self.pandas_lively();
        let k = screen
            .thumbs
            .first()
            .map_or(0.0, |t| t.w / screen.geometry.w);
        for (desktop, thumb) in screen.thumbs.iter().enumerate() {
            let rect = local(*thumb, origin);
            let over = overview
                .drag
                .as_ref()
                .and_then(|d| d.over.as_ref())
                .is_some_and(|(o, d)| *o == name && *d == desktop);
            // Its number and ring; the one a window is over, in front of
            // that window.
            if !over {
                self.overview_thumb_mark(
                    c,
                    &name,
                    desktop,
                    rect,
                    if desktop == current {
                        Mark::Current
                    } else {
                        Mark::Other
                    },
                    alpha,
                );
            }
            self.overview_box(c, Place::Edge(name.clone(), desktop), EDGE, rect, alpha);
            // Its windows, front first.
            let here = self.thumb_windows(output, desktop);
            if here.is_empty() {
                // Asleep: two breaths as the overview opens, two "z"
                // rising.
                let size = (rect.size.h * SLEEP_SHARE).min(SLEEP_MOST);
                let frame = lively
                    .then(|| crate::pandas::frame(BREATHING, now.saturating_sub(overview.opened)))
                    .flatten();
                let at = Point::<f64, Logical>::from((
                    rect.loc.x + (rect.size.w - size) / 2.0,
                    rect.loc.y + (rect.size.h - size) / 2.0,
                ));
                self.overview_picture(
                    c,
                    Place::Sleep(name.clone(), desktop),
                    &Pic::Panda {
                        mood: hocus_pandas::Mood::Resting,
                        frame,
                        side: (size * scale).ceil() as u32,
                        halo: None,
                    },
                    at,
                    Some((size, size)),
                    alpha,
                );
            }
            for id in here {
                let Some(home) = self.home_rect(id) else {
                    continue;
                };
                let visual = Rectangle::<f64, Logical>::new(
                    (
                        thumb.x + (f64::from(home.x) - screen.geometry.x) * k,
                        thumb.y + (f64::from(home.y) - screen.geometry.y) * k,
                    )
                        .into(),
                    (f64::from(home.width) * k, f64::from(home.height) * k).into(),
                );
                let look = crate::render::Look::thumbnail(visual, alpha, 2.0);
                let mut parts = Vec::new();
                self.window_parts(c.renderer, id, &look, origin, scale, &mut parts);
                c.out
                    .extend(parts.into_iter().enumerate().filter_map(|(j, e)| match e {
                        OutputElements::Window(w) => Some(OutputElements::Thumbnail(Keyed::new(
                            w,
                            self.overview_paint.thumb_id(id, desktop, j),
                        ))),
                        _ => None,
                    }));
            }
            // Its ground: the screen's wallpaper, small.
            let dst: Rectangle<i32, Physical> = Rectangle::new(
                rect.loc.to_physical(scale).to_i32_round::<i32>(),
                rect.size.to_physical(scale).to_i32_round::<i32>(),
            );
            match &wall {
                Some((texture, (w, h))) => {
                    let all = Rectangle::from_size((f64::from(*w), f64::from(*h)).into());
                    match Placed::new(c.renderer, texture, all, dst, alpha) {
                        Ok(e) => c.out.push(OutputElements::Frame(Keyed::new(
                            e,
                            self.overview_paint
                                .id(&Place::Wall(name.clone(), desktop), 0),
                        ))),
                        Err(err) => tracing::warn!(?err, "could not draw a thumbnail's wallpaper"),
                    }
                }
                None => {
                    self.overview_box(c, Place::Ground(name.clone(), desktop), GROUND, rect, alpha)
                }
            }
        }
    }
}

/// Whether `output`'s strip slides now.
fn strip_moving(state: &State, output: &Output, now: Duration) -> bool {
    let strip = state.strip(output);
    (strip.position(now) - strip.current() as f64).abs() > 1e-6
}

fn lang_of(state: &State) -> hocus_words::Lang {
    state.radial_lang()
}

fn milli(scale: f64) -> u32 {
    (scale * 1000.0).round() as u32
}

/// Where window `id` is drawn at `shown` of the overview's way: from
/// where it stands to `to`, its place in the layout; there with reduced
/// motion, or minimised.
fn glide(state: &State, id: WindowId, to: Area, shown: f64, reduced: bool, now: Duration) -> Area {
    if reduced || shown >= 1.0 || state.model.is_minimized(id) {
        return to;
    }
    let Some(look) = state.look(id, now) else {
        return to;
    };
    let from = Area {
        x: look.visual.loc.x,
        y: look.visual.loc.y,
        w: look.visual.size.w,
        h: look.visual.size.h,
    };
    from.lerp(to, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_draws_at_any_scale() {
        let fonts = fonts().expect("the shell's fonts");
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let pics = [
                Pic::Label {
                    title: "Docs — Mozilla Firefox, a long title that is cut".into(),
                    app: "firefox".into(),
                    width: (300.0 * scale) as i32,
                    scale: milli(scale),
                },
                Pic::Words {
                    text: "Lâcher : bureau 3".into(),
                    ink: DROP_INK,
                    bold: true,
                    scale: milli(scale),
                },
                Pic::Pill {
                    words: "3 t’attendent".into(),
                    state: AgentState::Waiting,
                    panda: 56,
                    scale: milli(scale),
                },
                Pic::Panda {
                    mood: hocus_pandas::Mood::Waiting,
                    frame: crate::pandas::frame(hocus_pandas::BURST, Duration::from_millis(500)),
                    side: (56.0 * scale) as u32,
                    halo: Some(AgentState::Waiting),
                },
                Pic::Panda {
                    mood: hocus_pandas::Mood::Resting,
                    frame: None,
                    side: (72.0 * scale) as u32,
                    halo: None,
                },
                Pic::Close {
                    scale: milli(scale),
                },
                Pic::Make {
                    text: "En faire un projet".into(),
                    scale: milli(scale),
                },
                Pic::Project {
                    mono: "HP".into(),
                    title: "1 · hocus-panda".into(),
                    folder: "~/github/a/very/long/path/to/hocus-panda".into(),
                    holds: "3 fenêtres · 1 agent".into(),
                    current: false,
                    width: (200.0 * scale) as i32,
                    scale: milli(scale),
                },
                Pic::ProjectPill {
                    mono: "HS".into(),
                    name: "hocus-site".into(),
                    folder: "~/github/hocus-site".into(),
                    scale: milli(scale),
                },
            ];
            for pic in &pics {
                let (pixels, (w, h)) = draw(&fonts, pic, scale).expect("drawn");
                assert_eq!(pixels.len(), (w * h * 4) as usize, "{pic:?}");
                assert!(pixels.iter().any(|p| *p != 0), "{pic:?} is empty");
            }
            // A title is cut to its room; a project's label too.
            let (_, (w, _)) = draw(&fonts, &pics[0], scale).expect("drawn");
            assert!(w <= (300.0 * scale) as i32 + (4.0 * scale) as i32, "{w}");
            let (_, (w, h)) = draw(&fonts, &pics[7], scale).expect("drawn");
            assert!(w <= (200.0 * scale) as i32 + 1, "{w}");
            assert_eq!(
                h,
                ((NAME_LINE + 2.0 * (2.0 + NUMBER_LINE)) * scale).ceil() as i32
            );
            // The button and the pill keep their heights.
            let (_, (_, h)) = draw(&fonts, &pics[6], scale).expect("drawn");
            assert_eq!(h, (MAKE_HEIGHT * scale).ceil() as i32);
        }
    }

    #[test]
    fn a_burst_is_a_picture_a_frame_then_the_still_one() {
        let side = 56;
        let window = hocus_pandas::BURST;
        let at = |ms| crate::pandas::frame(window, Duration::from_millis(ms));
        let halo = Some(AgentState::Waiting);
        let draw = |frame| draw_panda(hocus_pandas::Mood::Waiting, frame, side, halo).map(|p| p.0);
        // Thirty a second while it plays: the same moment, the same picture.
        assert_eq!(at(500), at(510));
        assert_ne!(draw(at(500)), draw(at(900)));
        // Then none: the still picture, kept.
        assert_eq!(at(3000), None);
        assert_eq!(draw(None), draw(at(3000)));
    }
}
