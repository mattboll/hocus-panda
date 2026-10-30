//! What the compositor draws over the windows in this slice: the
//! desktops' dots while the strip moves (docs/design/windows.md §12), and
//! Alt+Tab's panel with its live thumbnails and titles (§11). Titles are
//! drawn with the system's sans-serif font, found and read on a thread of
//! its own; until it comes, the panel shows without them.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    reexports::calloop::{
        LoopHandle,
        channel::{self, Event},
    },
    utils::{Logical, Point, Rectangle, Size},
};

use crate::{
    App, State,
    decoration::{
        ACCENT, BoxStyle, FrameIds, Keyed, Placed, ShadowStyle, box_elements, buffer,
        shadow_elements,
    },
    desktops::Strip,
    model::WindowId,
    motion::{self, lerp},
    render::{Look, OutputElement, OutputElements},
    switcher::{self, Area, DIMMED, Switcher},
    wobble::Bend,
};

/// The dots: 8 px high, the current desktop's 22 px wide, 6 px apart,
/// 18 px from the screen's top (the prototype's).
const DOT: f64 = 8.0;
const DOT_CURRENT: f64 = 22.0;
const DOT_GAP: f64 = 6.0;
const DOT_TOP: f64 = 18.0;
const DOT_OTHER: [u8; 4] = [0xFF, 0xFF, 0xFF, 89];

/// The panel: dark at 86 % with a white rim at 12 %; the selection the
/// accent at 18 % with a 2 px accent ring; a deep shadow.
const PANEL: BoxStyle = BoxStyle {
    fill: [18, 22, 34, 219],
    rim: [0xFF, 0xFF, 0xFF, 31],
    rim_width: 1000,
    radius: 18_000,
};
const SELECTION: BoxStyle = BoxStyle {
    fill: [0x6C, 0xC2, 0x87, 46],
    rim: ACCENT,
    rim_width: 2000,
    radius: 12_000,
};
const PANEL_SHADOW: ShadowStyle = ShadowStyle {
    dy: 30.0,
    blur: 80.0,
    alpha: 0.55,
    radius: PANEL.radius as f64 / 1000.0,
};
/// A title, and the number of another desktop after it.
const TITLE: [u8; 4] = [0xEE, 0xF1, 0xF6, 0xFF];
const DESKTOP_NUMBER: [u8; 4] = [0x8C, 0x96, 0xA8, 0xFF];

/// A title drawn once per text, scale and width.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LabelKey {
    title: String,
    suffix: String,
    scale: u32,
    width: i32,
}

/// A title's texture and its size, physical pixels.
type Label = (MemoryRenderBuffer, (i32, i32));

/// The overlay's font, textures and element ids.
pub struct Overlay {
    font: Option<fontdue::Font>,
    labels: RefCell<HashMap<LabelKey, Option<Label>>>,
    panel: [Id; 9],
    shadow: FrameIds,
    selection: [Id; 9],
    dots: Vec<[Id; 9]>,
    /// A thumbnail's surfaces and a title, by window: their own ids, the
    /// window being drawn too.
    thumbs: RefCell<HashMap<(WindowId, usize), Id>>,
    titles: RefCell<HashMap<WindowId, Id>>,
}

impl std::fmt::Debug for Overlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overlay")
            .field("font", &self.font.is_some())
            .field("labels", &self.labels.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            font: None,
            labels: RefCell::default(),
            panel: std::array::from_fn(|_| Id::new()),
            shadow: FrameIds::default(),
            selection: std::array::from_fn(|_| Id::new()),
            dots: (0..2 * crate::desktops::MAX_COUNT)
                .map(|_| std::array::from_fn(|_| Id::new()))
                .collect(),
            thumbs: RefCell::default(),
            titles: RefCell::default(),
        }
    }

    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    /// A window went: its ids go.
    pub fn forget(&self, id: WindowId) {
        self.thumbs.borrow_mut().retain(|(w, _), _| *w != id);
        self.titles.borrow_mut().remove(&id);
    }

    fn thumb_id(&self, id: WindowId, part: usize) -> Id {
        self.thumbs
            .borrow_mut()
            .entry((id, part))
            .or_insert_with(Id::new)
            .clone()
    }

    fn title_id(&self, id: WindowId) -> Id {
        self.titles
            .borrow_mut()
            .entry(id)
            .or_insert_with(Id::new)
            .clone()
    }

    /// A title in `width` physical pixels at most, ellipsed past it.
    fn label(
        &self,
        title: &str,
        suffix: &str,
        scale: f64,
        width: i32,
    ) -> Option<(MemoryRenderBuffer, (i32, i32))> {
        let font = self.font.as_ref()?;
        let key = LabelKey {
            title: title.to_owned(),
            suffix: suffix.to_owned(),
            scale: (scale * 1000.0).round() as u32,
            width,
        };
        let mut labels = self.labels.borrow_mut();
        if labels.len() > 256 {
            labels.clear();
        }
        labels
            .entry(key)
            .or_insert_with(|| {
                let px = (switcher::LABEL_SIZE * scale) as f32;
                text(font, title, suffix, px, width)
            })
            .clone()
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Fonts tried after fontconfig's choice, in order.
const FONTS: [&str; 6] = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
    "/usr/share/fonts/opentype/cantarell/Cantarell-Bold.otf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/usr/local/share/fonts/dejavu/DejaVuSans-Bold.ttf",
];

/// Finds and reads the titles' font on a thread of its own (fontconfig,
/// then common places), and hands it to the loop.
pub fn load_font(handle: &LoopHandle<'static, App>) {
    let (sender, receiver) = channel::channel::<fontdue::Font>();
    let inserted = handle.insert_source(receiver, |event, _, app| {
        if let Event::Msg(font) = event {
            app.state.overlay.font = Some(font);
            app.state.overlay.labels.borrow_mut().clear();
            if app.state.switcher.is_some() {
                app.state.redraw = true;
            }
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no font for the titles");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("font".into())
        .spawn(move || {
            let found = fontconfig_choice()
                .into_iter()
                .chain(FONTS.iter().map(PathBuf::from))
                .find_map(|path| read_font(&path));
            match found {
                Some(font) => {
                    let _ = sender.send(font);
                }
                None => tracing::warn!("no font found for the titles"),
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "could not look for a font");
    }
}

/// The file fontconfig gives for a semibold sans-serif.
fn fontconfig_choice() -> Option<PathBuf> {
    let output = std::process::Command::new("fc-match")
        .args(["--format=%{file}", "sans-serif:weight=600"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    (output.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

fn read_font(path: &Path) -> Option<fontdue::Font> {
    let bytes = std::fs::read(path).ok()?;
    match fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
        Ok(font) => {
            tracing::debug!(font = %path.display(), "titles' font");
            Some(font)
        }
        Err(err) => {
            tracing::debug!(font = %path.display(), err, "not a font");
            None
        }
    }
}

/// `title` then `suffix` in their colours, `px` high, at most `width`
/// wide (ellipsed), as premultiplied ARGB; and its size.
fn text(
    font: &fontdue::Font,
    title: &str,
    suffix: &str,
    px: f32,
    width: i32,
) -> Option<(MemoryRenderBuffer, (i32, i32))> {
    let line = font.horizontal_line_metrics(px)?;
    let advance = |s: &str| -> f32 { s.chars().map(|c| font.metrics(c, px).advance_width).sum() };
    let room = width as f32 - advance(suffix);
    let mut shown: String = title.to_owned();
    if advance(&shown) > room {
        let ellipsis = advance("…");
        while !shown.is_empty() && advance(&shown) + ellipsis > room {
            shown.pop();
        }
        shown = format!("{}…", shown.trim_end());
    }
    let runs = [(shown.as_str(), TITLE), (suffix, DESKTOP_NUMBER)];
    let w = advance(&shown) + advance(suffix);
    let (w, h) = (w.ceil() as i32, (line.ascent - line.descent).ceil() as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mut pen = 0.0f32;
    for (run, colour) in runs {
        for c in run.chars() {
            let (metrics, coverage) = font.rasterize(c, px);
            let gx = (pen + metrics.xmin as f32).round() as i32;
            let gy = (line.ascent - (metrics.height as f32 + metrics.ymin as f32)).round() as i32;
            for (row, line) in coverage.chunks(metrics.width.max(1)).enumerate() {
                for (col, &a) in line.iter().enumerate() {
                    let (x, y) = (gx + col as i32, gy + row as i32);
                    if a == 0 || x < 0 || y < 0 || x >= w || y >= h {
                        continue;
                    }
                    let at = ((y * w + x) * 4) as usize;
                    let alpha = u32::from(a) * u32::from(colour[3]) / 255;
                    let premultiply = |v: u8| (u32::from(v) * alpha / 255) as u8;
                    let px = [
                        premultiply(colour[2]),
                        premultiply(colour[1]),
                        premultiply(colour[0]),
                        alpha as u8,
                    ];
                    for k in 0..4 {
                        pixels[at + k] = pixels[at + k].max(px[k]);
                    }
                }
            }
            pen += metrics.advance_width;
        }
    }
    Some((buffer(&pixels, w, h), (w, h)))
}

fn rect(area: Area) -> Rectangle<f64, Logical> {
    Rectangle::new((area.x, area.y).into(), (area.w, area.h).into())
}

impl State {
    /// The dots and Alt+Tab's panel on `output`, front first.
    pub(crate) fn overlay_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + crate::going::Streaks,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        self.dots_elements(renderer, output, now, out);
        self.switcher_elements(renderer, output, now, out);
    }

    /// The desktops' dots, while the strip moves: the current one wide
    /// and in the accent, following the strip as it slides.
    fn dots_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + crate::going::Streaks,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let strip: Strip = self.strip(output);
        let alpha = strip.dots_alpha(now);
        if alpha <= 0.0 {
            return;
        }
        let Some(geometry) = self.space.output_geometry(output) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let at = strip.position(now);
        let near: Vec<f64> = (0..strip.count())
            .map(|d| (1.0 - (d as f64 - at).abs()).clamp(0.0, 1.0))
            .collect();
        let widths: Vec<f64> = near.iter().map(|n| lerp(DOT, DOT_CURRENT, *n)).collect();
        let total = widths.iter().sum::<f64>() + DOT_GAP * (widths.len() as f64 - 1.0);
        let mut x = (f64::from(geometry.size.w) - total) / 2.0;
        let textures = &self.textures;
        for (d, (w, n)) in widths.iter().zip(&near).enumerate() {
            let r = Rectangle::new((x, DOT_TOP).into(), (*w, DOT).into());
            let dot = |fill: [u8; 4]| BoxStyle {
                fill,
                rim: [0; 4],
                rim_width: 0,
                radius: (DOT / 2.0 * 1000.0) as u32,
            };
            for (k, (style, a)) in [(dot(ACCENT), *n), (dot(DOT_OTHER), 1.0 - n)]
                .into_iter()
                .enumerate()
            {
                let ids = &self.overlay.dots[(2 * d + k) % self.overlay.dots.len()];
                out.extend(
                    box_elements(renderer, textures, ids, style, r, scale, alpha * a as f32)
                        .into_iter()
                        .map(OutputElements::Frame),
                );
            }
            x += w + DOT_GAP;
        }
    }

    /// Alt+Tab's panel, appearing or vanishing, on its screen.
    fn switcher_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + crate::going::Streaks,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let reduced = self.reduced_motion();
        let (switcher, shown): (&Switcher, f64) = match (&self.switcher, &self.switcher_out) {
            (Some(s), _) if s.shown(now) => (s, s.appear(now)),
            (_, Some((s, tween))) => (s, 1.0 - tween.at(now)),
            _ => return,
        };
        if switcher.output != *output || shown <= 0.0 {
            return;
        }
        let Some(geometry) = self.space.output_geometry(output) else {
            return;
        };
        let Some((panel, items)) = self.switcher_layout(switcher) else {
            return;
        };
        let origin = geometry.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let alpha = shown as f32;
        let k = if reduced {
            1.0
        } else {
            lerp(motion::SWITCHER_SCALE, 1.0, shown)
        };
        let centre = panel.centre();
        let place = |a: Area| rect(a.scaled(k, centre));
        let textures = &self.textures;
        // Titles, front first.
        for (i, &(id, here, desktop)) in switcher.items.iter().enumerate() {
            let Some(p) = items.get(i) else {
                continue;
            };
            let dim = if here && !self.model.is_minimized(id) {
                1.0
            } else {
                DIMMED
            };
            let suffix = if here {
                String::new()
            } else {
                format!(" · {}", desktop + 1)
            };
            let label = place(p.label);
            let width = (label.size.w * scale).floor() as i32;
            let title = crate::windows::title_of(self, id);
            if let Some((texture, (w, h))) = self.overlay.label(&title, &suffix, scale, width) {
                let size = Size::<f64, Logical>::from((f64::from(w) / scale, f64::from(h) / scale));
                let at = Point::<f64, Logical>::from((
                    label.loc.x + (label.size.w - size.w) / 2.0,
                    label.loc.y + (label.size.h - size.h) / 2.0,
                ))
                .to_physical(scale)
                .to_i32_round::<i32>()
                .to_f64();
                // All of it, pixel for pixel: drawn at the screen's
                // scale, its pixels are not logical ones.
                let all = Rectangle::from_size((f64::from(w), f64::from(h)).into());
                let dst = Rectangle::new(at.to_i32_round(), (w, h).into());
                match Placed::new(renderer, &texture, all, dst, alpha * dim) {
                    Ok(e) => out.push(OutputElements::Frame(Keyed::new(
                        e,
                        self.overlay.title_id(id),
                    ))),
                    Err(err) => tracing::warn!(?err, "could not draw a title"),
                }
            }
        }
        // Live thumbnails.
        for (i, &(id, here, _)) in switcher.items.iter().enumerate() {
            let Some(p) = items.get(i) else {
                continue;
            };
            let dim = if here && !self.model.is_minimized(id) {
                1.0
            } else {
                DIMMED
            };
            let mut visual = place(p.thumb);
            visual.loc += origin;
            let look = Look::thumbnail(visual, alpha * dim, switcher::THUMB_RADIUS * k);
            let mut parts = Vec::new();
            self.window_parts(renderer, id, &look, origin, scale, &mut parts);
            out.extend(parts.into_iter().enumerate().filter_map(|(j, e)| match e {
                OutputElements::Window(w) => Some(OutputElements::Thumbnail(Keyed::new(
                    w,
                    self.overlay.thumb_id(id, j),
                ))),
                _ => None,
            }));
        }
        // The selection's box, moving from the item it left.
        let (from, t) = switcher.ring(now);
        let t = if reduced { 1.0 } else { t };
        if let (Some(a), Some(b)) = (items.get(from), items.get(switcher.selected)) {
            out.extend(
                box_elements(
                    renderer,
                    textures,
                    &self.overlay.selection,
                    SELECTION,
                    place(a.item.lerp(b.item, t)),
                    scale,
                    alpha,
                )
                .into_iter()
                .map(OutputElements::Frame),
            );
        }
        let panel_rect = place(panel);
        out.extend(
            box_elements(
                renderer,
                textures,
                &self.overlay.panel,
                PANEL,
                panel_rect,
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Frame),
        );
        out.extend(
            shadow_elements(
                renderer,
                textures,
                &self.overlay.shadow,
                PANEL_SHADOW,
                panel_rect,
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Shadow),
        );
    }
}
