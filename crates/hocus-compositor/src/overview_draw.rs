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
//! is drawn again but what clients commit. A waiting agent's panda waves
//! on a few pictures of its paw, bobbing as they are placed; a sleeping
//! one breathes by its place's size.

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
        AgentState, BREATH, Badge, CLOSE_BUTTON, DIM, LABEL_GAP, LABEL_LINE, LABEL_SIZE, MINIMIZED,
        NUMBER_GAP, NUMBER_LINE, NUMBER_SIZE, Overview, Screen, WAVE, close_button,
    },
    radial_draw::{
        Fonts, advance, argb, fonts, letter_colour, letter_of, paint, rgba, rounded, stroke, write,
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
/// The agents' badge: a panda of at least 42 px, 14 % of its window's
/// width up to 84, its pill 8 px higher; its words in proportion.
const PANDA: f64 = 42.0;
const PANDA_MOST: f64 = 84.0;
const PANDA_SHARE: f64 = 0.14;
const PILL_PAD: f64 = 4.0;
const BADGE_TEXT: f32 = 12.5;
const BADGE_IN: f64 = 10.0;
const BOB: f64 = 3.0;
/// The paw's pictures over its wave's loop.
const WAVE_STEPS: u8 = 12;
const WAVE_ANGLE: f32 = -28.0;
/// The sleeping panda: this share of the thumbnail's height, at most 64.
const SLEEP_SHARE: f64 = 0.5;
const SLEEP_MOST: f64 = 64.0;
/// The application's tile beside a title.
const TILE: f64 = 18.0;
const TILE_GAP: f64 = 8.0;

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
    /// A panda in its state's colour, its paw at a step of its wave; none:
    /// asleep.
    Panda {
        state: Option<AgentState>,
        wave: u8,
        side: u32,
    },
    /// The ✕ over a window.
    Close { scale: u32 },
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
        Pic::Panda { state, wave, side } => draw_panda(fonts, *state, *wave, *side),
        Pic::Close { .. } => draw_close(scale),
    }
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
    let px = BADGE_TEXT * k * (panda / PANDA) as f32;
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

/// The panda (the mockup's, 30 units, 1 unit of room around), `side`
/// pixels square: awake in its state's colour, its paw raised at `wave`
/// of its loop; or asleep, its eyes shut, a "z" over it.
fn draw_panda(
    fonts: &Fonts,
    state: Option<AgentState>,
    wave: u8,
    side: u32,
) -> Option<(Vec<u8>, (i32, i32))> {
    let mut pixmap = Pixmap::new(side.max(1), side.max(1))?;
    let k = side as f32 / 32.0;
    let t = Transform::from_translate(1.0, 1.0).post_scale(k, k);
    let dark = [0x1D, 0x22, 0x30, 0xFF];
    let white = [0xF2, 0xF4, 0xF7, 0xFF];
    let circle = |pixmap: &mut Pixmap, x: f32, y: f32, r: f32, c: [u8; 4]| {
        if let Some(path) = PathBuilder::from_circle(x, y, r) {
            pixmap.fill_path(&path, &paint(c), FillRule::Winding, t, None);
        }
    };
    let ellipse =
        |pixmap: &mut Pixmap, x: f32, y: f32, rx: f32, ry: f32, degrees: f32, c: [u8; 4]| {
            let Some(rect) = tiny_skia::Rect::from_xywh(-rx, -ry, 2.0 * rx, 2.0 * ry) else {
                return;
            };
            if let Some(path) = PathBuilder::from_oval(rect) {
                let place = Transform::from_rotate(degrees)
                    .post_translate(x, y)
                    .post_concat(t);
                pixmap.fill_path(&path, &paint(c), FillRule::Winding, place, None);
            }
        };
    match state {
        Some(state) => {
            let [r, g, b, _] = state.colour();
            circle(&mut pixmap, 15.0, 16.0, 14.5, [r, g, b, 56]);
        }
        None => circle(&mut pixmap, 15.0, 16.0, 14.5, rgba(169, 178, 194, 0.12)),
    }
    circle(&mut pixmap, 8.5, 8.5, 4.2, dark);
    circle(&mut pixmap, 21.5, 8.5, 4.2, dark);
    circle(&mut pixmap, 15.0, 16.5, 10.5, white);
    ellipse(&mut pixmap, 10.8, 16.0, 2.8, 3.6, -20.0, dark);
    ellipse(&mut pixmap, 19.2, 16.0, 2.8, 3.6, 20.0, dark);
    ellipse(&mut pixmap, 15.0, 21.0, 1.6, 1.1, 0.0, dark);
    match state {
        Some(state) => {
            ellipse(&mut pixmap, 11.3, 15.4, 1.0, 1.0, 0.0, [0xFF; 4]);
            ellipse(&mut pixmap, 18.7, 15.4, 1.0, 1.0, 0.0, [0xFF; 4]);
            // The paw: raised and waving while its agent waits.
            let angle = if state == AgentState::Waiting {
                let phase = f32::from(wave) / f32::from(WAVE_STEPS);
                WAVE_ANGLE * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos())
            } else {
                0.0
            };
            let rect = tiny_skia::Rect::from_xywh(-2.6, -3.4, 5.2, 6.8)?;
            if let Some(path) = PathBuilder::from_oval(rect) {
                let place = Transform::from_translate(2.2, -2.0)
                    .post_rotate(angle)
                    .post_translate(24.0, 22.0)
                    .post_concat(t);
                pixmap.fill_path(&path, &paint(dark), FillRule::Winding, place, None);
            }
        }
        None => {
            // Shut eyes: two short arcs.
            let mut pb = PathBuilder::new();
            pb.move_to(10.0, 16.2);
            pb.quad_to(11.3, 17.4, 12.6, 16.2);
            pb.move_to(17.4, 16.2);
            pb.quad_to(18.7, 17.4, 20.0, 16.2);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint([0xFF; 4]), &stroke(0.8), t, None);
            }
            // Its paws under its chin, and a "z".
            ellipse(&mut pixmap, 11.0, 25.0, 2.6, 1.8, 0.0, dark);
            ellipse(&mut pixmap, 19.0, 25.0, 2.6, 1.8, 0.0, dark);
            let px = 7.0 * k;
            write(
                &mut pixmap,
                &fonts.bold,
                "z",
                px,
                (26.0 * k, 7.0 * k),
                [0xC9, 0xD0, 0xDC, 0xE0],
                0.0,
            );
        }
    }
    Some((argb(pixmap), (side as i32, side as i32)))
}

fn local(a: Area, origin: Point<f64, Logical>) -> Rectangle<f64, Logical> {
    Rectangle::new((a.x - origin.x, a.y - origin.y).into(), (a.w, a.h).into())
}

impl State {
    /// A picture's element at `at` (logical, output-local, its top-left),
    /// pixel for pixel; or stretched over `size` when given.
    #[allow(clippy::too_many_arguments)]
    fn overview_picture<R>(
        &self,
        renderer: &mut R,
        place: Place,
        pic: &Pic,
        at: Point<f64, Logical>,
        size: Option<(f64, f64)>,
        scale: f64,
        alpha: f32,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<(f64, f64)>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
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
        match Placed::new(renderer, &texture, all, dst, alpha) {
            Ok(e) => out.push(OutputElements::Frame(Keyed::new(
                e,
                paint.id(&place, serial),
            ))),
            Err(err) => tracing::warn!(?err, "could not draw the overview"),
        }
        Some(logical)
    }

    /// A box of `style` over `rect` (output-local).
    #[allow(clippy::too_many_arguments)]
    fn overview_box<R>(
        &self,
        renderer: &mut R,
        place: Place,
        style: BoxStyle,
        rect: Rectangle<f64, Logical>,
        scale: f64,
        alpha: f32,
        out: &mut Vec<OutputElement<R>>,
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
        out.extend(
            box_elements(renderer, &self.textures, &ids, style, rect, scale, alpha)
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
        let french = self.radial_french();
        // The thumbnail a dragged window is over, lit, in front of it.
        if let Some((name, desktop)) = overview.drag.as_ref().and_then(|d| d.over.as_ref())
            && *name == output.name()
            && let Some(thumb) = screen.thumbs.get(*desktop)
        {
            let current = self.strip(output).current();
            self.overview_thumb_mark(
                renderer,
                name,
                *desktop,
                local(*thumb, origin),
                true,
                current,
                scale,
                alpha,
                &mut out,
            );
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
                self.framed_window(renderer, drag.window, &look, origin, scale, &mut out);
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
                renderer,
                Place::Close,
                &Pic::Close {
                    scale: milli(scale),
                },
                local(button, origin).loc,
                None,
                scale,
                1.0,
                &mut out,
            );
        }
        // The agents' badges.
        for (id, rect) in &slots {
            let Some(badge) = overview.badges.get(id) else {
                continue;
            };
            let at = glide(self, *id, *rect, shown, reduced, now);
            self.overview_badge(
                renderer, *id, *badge, at, origin, scale, alpha, french, now, &mut out,
            );
        }
        // The rim of the window under the pointer, the keyboard's ring.
        if settled {
            if let Some((_, rect)) = hovered.and_then(|h| slots.iter().find(|(id, _)| *id == h)) {
                self.overview_box(
                    renderer,
                    Place::Hover,
                    HOVER,
                    local(*rect, origin),
                    scale,
                    1.0,
                    &mut out,
                );
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
                self.overview_box(
                    renderer,
                    Place::Selection,
                    SELECTION,
                    local(around, origin),
                    scale,
                    1.0,
                    &mut out,
                );
            }
        }
        // The windows laid out, with their titles; while the strip
        // slides (a thumbnail clicked), the desktops it passes.
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
                slots.clone()
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
        for (place, pic, at, a) in titles {
            self.overview_picture(renderer, place, &pic, at, None, scale, a, &mut out);
        }
        // A window let go nowhere, on its way back: over the others.
        if let Some((id, from, tween)) = overview.back
            && let Some(to) = slots.iter().find(|(w, _)| *w == id).map(|(_, r)| *r)
            && let Some(look) = self.look(id, now)
        {
            let r = from.lerp(to, tween.at(now));
            let visual = Rectangle::<f64, Logical>::new((r.x, r.y).into(), (r.w, r.h).into());
            let look = look.placed(visual, 1.0, true);
            self.framed_window(renderer, id, &look, origin, scale, &mut out);
            windows.retain(|(w, _)| *w != id);
        }
        for (id, look) in &windows {
            self.framed_window(renderer, *id, look, origin, scale, &mut out);
        }
        // The desktops' thumbnails.
        self.overview_thumbs(
            renderer, overview, screen, origin, scale, alpha, now, &mut out,
        );
        // The wallpaper dimmed.
        let whole: Rectangle<i32, Physical> = Rectangle::from_size(
            geometry
                .size
                .to_f64()
                .to_physical(scale)
                .to_i32_round::<i32>(),
        );
        let a = DIM[3] * alpha;
        out.push(OutputElements::Solid(SolidColorRenderElement::new(
            self.overview_paint.dim.clone(),
            whole,
            (a * 1000.0) as usize,
            [DIM[0] * a, DIM[1] * a, DIM[2] * a, a],
            Kind::Unspecified,
        )));
        out
    }

    /// A window's agents' badge: its pill at its top-left, the panda on
    /// it, bobbing and waving while one waits.
    #[allow(clippy::too_many_arguments)]
    fn overview_badge<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        badge: Badge,
        at: Area,
        origin: Point<f64, Logical>,
        scale: f64,
        alpha: f32,
        french: bool,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let corner =
            Point::<f64, Logical>::from((at.x + BADGE_IN - origin.x, at.y + BADGE_IN - origin.y));
        let waiting = badge.state == AgentState::Waiting && !self.reduced_motion();
        let (wave, bob) = if waiting {
            let phase = (now.as_secs_f64() / WAVE.as_secs_f64()).fract();
            let step = (phase * f64::from(WAVE_STEPS)).floor() as u8 % WAVE_STEPS;
            (
                step,
                -BOB * (0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()),
            )
        } else {
            (0, 0.0)
        };
        let panda = (at.w * PANDA_SHARE).clamp(PANDA, PANDA_MOST).round();
        let side = ((panda + 2.0 * panda / 30.0) * scale).ceil() as u32;
        let panda_at = Point::<f64, Logical>::from((
            corner.x + PILL_PAD - panda / 30.0,
            corner.y + PILL_PAD - panda / 30.0 + bob,
        ));
        self.overview_picture(
            renderer,
            Place::Panda(id),
            &Pic::Panda {
                state: Some(badge.state),
                wave,
                side,
            },
            panda_at,
            None,
            scale,
            alpha,
            out,
        );
        self.overview_picture(
            renderer,
            Place::Pill(id),
            &Pic::Pill {
                words: badge.words(french),
                state: badge.state,
                panda: panda as u32,
                scale: milli(scale),
            },
            corner - Point::from((1.0 / scale, 1.0 / scale)),
            None,
            scale,
            alpha,
            out,
        );
    }

    /// A thumbnail's number and its ring: the current one's accent, the
    /// one a dragged window is `over` lit, "Lâcher : bureau N" under it.
    #[allow(clippy::too_many_arguments)]
    fn overview_thumb_mark<R>(
        &self,
        renderer: &mut R,
        name: &str,
        desktop: usize,
        rect: Rectangle<f64, Logical>,
        over: bool,
        current: usize,
        scale: f64,
        alpha: f32,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let names = self.config.desktops.shown_names();
        let name = name.to_owned();
        // Under it: its number, or what letting go there does.
        let (text, ink, bold) = if over {
            let words = if french_of(self) {
                format!("Lâcher : bureau {}", desktop + 1)
            } else {
                format!("Drop: desktop {}", desktop + 1)
            };
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
                renderer,
                Place::Number(name.clone(), desktop),
                &pic,
                at,
                None,
                scale,
                alpha,
                out,
            );
        }
        // Ringed or lit.
        if over {
            self.overview_box(
                renderer,
                Place::Ring(name.clone(), desktop),
                LIT,
                rect,
                scale,
                alpha,
                out,
            );
        } else if desktop == current {
            self.overview_box(
                renderer,
                Place::Ring(name.clone(), desktop),
                CURRENT,
                rect,
                scale,
                alpha,
                out,
            );
        }
    }

    /// `screen`'s thumbnails: each desktop's windows small over its
    /// wallpaper, its number under it; the current one ringed in the
    /// accent, the one a dragged window is over lit; an empty one's
    /// sleeping panda.
    #[allow(clippy::too_many_arguments)]
    fn overview_thumbs<R>(
        &self,
        renderer: &mut R,
        overview: &Overview,
        screen: &Screen,
        origin: Point<f64, Logical>,
        scale: f64,
        alpha: f32,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let output = &screen.output;
        let name = output.name();
        let current = self.strip(output).current();
        let wall = (!self.config.plain_background)
            .then(|| {
                let backgrounds = self.backgrounds.borrow();
                backgrounds
                    .shown
                    .get(&name)
                    .map(|s| (s.buffer.clone(), s.picture))
            })
            .flatten();
        let reduced = self.reduced_motion();
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
                    renderer, &name, desktop, rect, false, current, scale, alpha, out,
                );
            }
            self.overview_box(
                renderer,
                Place::Edge(name.clone(), desktop),
                EDGE,
                rect,
                scale,
                alpha,
                out,
            );
            // Its windows, front first.
            let here = self.thumb_windows(output, desktop);
            if here.is_empty() {
                // Asleep: breathing a while as the overview opens.
                let base = (rect.size.h * SLEEP_SHARE).min(SLEEP_MOST);
                let breath = if reduced {
                    1.0
                } else {
                    let t = now.saturating_sub(overview.opened).as_secs_f64();
                    let t = t.min(crate::overview::BREATHING.as_secs_f64());
                    1.0 + 0.04
                        * (0.5 - 0.5 * (t * std::f64::consts::TAU / BREATH.as_secs_f64()).cos())
                };
                let side_px = (base * scale).ceil() as u32;
                let size = base * breath;
                let at = Point::<f64, Logical>::from((
                    rect.loc.x + (rect.size.w - size) / 2.0,
                    rect.loc.y + (rect.size.h - size) / 2.0 + (breath - 1.0) * 25.0,
                ));
                self.overview_picture(
                    renderer,
                    Place::Sleep(name.clone(), desktop),
                    &Pic::Panda {
                        state: None,
                        wave: 0,
                        side: side_px,
                    },
                    at,
                    Some((size, size)),
                    scale,
                    alpha,
                    out,
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
                self.window_parts(renderer, id, &look, origin, scale, &mut parts);
                out.extend(parts.into_iter().enumerate().filter_map(|(j, e)| match e {
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
                    match Placed::new(renderer, texture, all, dst, alpha) {
                        Ok(e) => out.push(OutputElements::Frame(Keyed::new(
                            e,
                            self.overview_paint
                                .id(&Place::Wall(name.clone(), desktop), 0),
                        ))),
                        Err(err) => tracing::warn!(?err, "could not draw a thumbnail's wallpaper"),
                    }
                }
                None => self.overview_box(
                    renderer,
                    Place::Ground(name.clone(), desktop),
                    GROUND,
                    rect,
                    scale,
                    alpha,
                    out,
                ),
            }
        }
    }
}

/// Whether `output`'s strip slides now.
fn strip_moving(state: &State, output: &Output, now: Duration) -> bool {
    let strip = state.strip(output);
    (strip.position(now) - strip.current() as f64).abs() > 1e-6
}

fn french_of(state: &State) -> bool {
    state.radial_french()
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
                    state: Some(AgentState::Waiting),
                    wave: 5,
                    side: (44.8 * scale) as u32,
                },
                Pic::Panda {
                    state: None,
                    wave: 0,
                    side: (60.0 * scale) as u32,
                },
                Pic::Close {
                    scale: milli(scale),
                },
            ];
            for pic in &pics {
                let (pixels, (w, h)) = draw(&fonts, pic, scale).expect("drawn");
                assert_eq!(pixels.len(), (w * h * 4) as usize, "{pic:?}");
                assert!(pixels.iter().any(|p| *p != 0), "{pic:?} is empty");
            }
            // A title is cut to its room.
            let (_, (w, _)) = draw(&fonts, &pics[0], scale).expect("drawn");
            assert!(w <= (300.0 * scale) as i32 + (4.0 * scale) as i32, "{w}");
        }
    }

    #[test]
    fn a_waving_paw_is_a_few_pictures_not_one_a_frame() {
        let fonts = fonts().expect("the shell's fonts");
        let side = 45;
        let pictures: std::collections::HashSet<Vec<u8>> = (0..WAVE_STEPS)
            .filter_map(|wave| {
                draw_panda(&fonts, Some(AgentState::Waiting), wave, side).map(|(p, _)| p)
            })
            .collect();
        assert!(pictures.len() > 4 && pictures.len() <= usize::from(WAVE_STEPS));
        // Working, the paw stays down.
        let still: std::collections::HashSet<Vec<u8>> = (0..WAVE_STEPS)
            .filter_map(|wave| {
                draw_panda(&fonts, Some(AgentState::Working), wave, side).map(|(p, _)| p)
            })
            .collect();
        assert_eq!(still.len(), 1);
    }
}
