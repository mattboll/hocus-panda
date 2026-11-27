//! Drawing every desktop at once (#944, docs/design/windows.md §12c): the
//! dimmed wallpaper; each desktop's picture (the screen's wallpaper, its
//! rim, its windows at their places from their own textures, nothing
//! copied); the ring of the desktop shown, the pointer's and the
//! keyboard's; each desktop's label; the agents' smaller badges; an empty
//! desktop's resting panda; the window carried, lifted, over all.
//!
//! The zoom moves the pictures: each from where the strip (or the
//! overview) has it to its place in the grid, and back into a desktop as
//! the view closes. Labels, rings, badges and pandas fade with it.

use super::*;
use crate::overview::grid::{self as level, From, Selection};

/// The badge's panda and words, smaller than the overview's.
pub(super) const PANDA: f64 = 26.0;
pub(super) const BADGE_TEXT: f32 = 11.5;
/// A label's words: 13.5 px on a 20 px line; a project's folder 11 px.
const WORDS: f32 = 13.5;
const LINE: f64 = 20.0;
const FOLDER: f32 = 11.0;
/// A desktop's monogram, as the project's pill: 22 × 20.
const MONO: (f64, f64) = (22.0, 20.0);
const DIM_INK: [u8; 4] = [0xA9, 0xB2, 0xC2, 0xFF];
/// A desktop's corners, and its window's at full size.
const RADIUS: f64 = 10.0;
const WINDOW_RADIUS: f64 = 10.0;
/// A desktop's rim: 1 px `rgba(255,255,255,0.12)`.
const EDGE: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: [0xFF, 0xFF, 0xFF, 31],
    rim_width: 1000,
    radius: 10_000,
};
/// The pointer's: a 2 px `rgba(255,255,255,0.38)` inner ring.
const HOVER: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: [0xFF, 0xFF, 0xFF, 97],
    rim_width: 2000,
    radius: 10_000,
};
/// Where a carried window would go: lit, a 2 px `#7EE0A0` inner ring.
const LIT: BoxStyle = BoxStyle {
    fill: [108, 194, 135, 71],
    rim: DROP_INK,
    rim_width: 2000,
    radius: 10_000,
};
/// The desktop shown: 2 px in the accent, 4 px out.
const CURRENT_OUT: f64 = 4.0;
/// The keyboard's: 2 px `#EEF1F6`, 9 px out.
const SELECTED: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: TEXT,
    rim_width: 2000,
    radius: 19_000,
};
const SELECTED_OUT: f64 = 9.0;
/// A window carried: a 1 px `#7EE0A0` rim; its place shows at 25 %.
const CARRIED: BoxStyle = BoxStyle {
    fill: [0; 4],
    rim: DROP_INK,
    rim_width: 1000,
    radius: 4_000,
};
const PLACE_SHOWS: f32 = 0.25;

/// Grows `a` by `by` on every side.
fn around(a: Area, by: f64) -> Area {
    Area {
        x: a.x - by,
        y: a.y - by,
        w: a.w + 2.0 * by,
        h: a.h + 2.0 * by,
    }
}

/// `text` on a 20 px line, 13.5 px: 600 in `text.dim`, or 700.
pub(super) fn draw_words(
    fonts: &Fonts,
    text: &str,
    ink: [u8; 4],
    bold: bool,
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let font = if bold { &fonts.bold } else { &fonts.semibold };
    let px = WORDS * k;
    let w = (advance(font, text, px) + 4.0 * k).ceil() as u32;
    let h = (LINE * scale).ceil() as u32;
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

/// A project's label: its monogram (22 × 20, as the pill), "2 · name"
/// (13.5 px, 700), its folder (Martian Mono 11 px, `text.faint`).
pub(super) fn draw_project(
    fonts: &Fonts,
    (mono, title, folder): (&str, &str, &str),
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    let k = scale as f32;
    let (badge_w, badge_h) = ((MONO.0 * scale) as f32, (MONO.1 * scale) as f32);
    let (name_px, folder_px, mono_px) = (WORDS * k, FOLDER * k, 9.0 * k);
    let gap = 8.0 * k;
    let name_w = advance(&fonts.bold, title, name_px);
    let folder_w = advance(&fonts.mono, folder, folder_px);
    let w = (badge_w + gap + name_w + gap + folder_w + 4.0 * k).ceil() as u32;
    let h = (LINE * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let middle = h as f32 / 2.0;
    if let Some(path) = rounded(0.0, middle - badge_h / 2.0, badge_w, badge_h, 6.0 * k) {
        pixmap.fill_path(
            &path,
            &paint(ACCENT),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let mono_w = advance(&fonts.bold, mono, mono_px);
    write(
        &mut pixmap,
        &fonts.bold,
        mono,
        mono_px,
        ((badge_w - mono_w) / 2.0, middle + mono_px * 0.36),
        MONO_INK,
        0.0,
    );
    let x = badge_w + gap;
    write(
        &mut pixmap,
        &fonts.bold,
        title,
        name_px,
        (x, middle + name_px * 0.36),
        TEXT,
        1.2 * k,
    );
    write(
        &mut pixmap,
        &fonts.mono,
        folder,
        folder_px,
        (x + name_w + gap, middle + folder_px * 0.36),
        FAINT,
        1.2 * k,
    );
    Some((argb(pixmap), (w as i32, h as i32)))
}

impl State {
    /// Every desktop on `output` at `now`, front first.
    pub(super) fn grid_elements<R>(
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
        let Some(grid) = &overview.grid else {
            return out;
        };
        let Some(geometry) = self.space.output_geometry(output) else {
            return out;
        };
        let screens = self.grid_screens();
        let Some(screen) = screens.iter().find(|s| s.output == *output) else {
            return out;
        };
        let origin = geometry.loc.to_f64();
        let scale = output.current_scale().fractional_scale();
        let shown = overview.shown(now);
        let alpha = shown as f32;
        let reduced = overview.reduced;
        let closing = overview.closing.is_some();
        let from_overview = grid.from == From::Overview && !closing;
        let name = output.name();
        let current = self.strip(output).current();
        let linked = self.desktops_linked();
        let c = &mut Canvas {
            renderer,
            out: &mut out,
            origin,
            scale,
            now,
        };
        // The window carried, over everything but the bar, on whichever
        // screen it is over; its rim in front of it.
        if let Some(carry) = &grid.carry {
            let rect = carry.rect(now, reduced);
            let global =
                Rectangle::<f64, Logical>::new((rect.x, rect.y).into(), (rect.w, rect.h).into());
            if global.overlaps(geometry.to_f64())
                && let Some(look) = self.look(carry.window, now)
            {
                self.overview_box(c, Place::GridCarry, CARRIED, local(rect, origin), 1.0);
                let look = look.placed(global, 1.0, true);
                self.framed_window(c.renderer, carry.window, &look, origin, scale, c.out);
            }
        }
        // A window let go nowhere, on its way back to its place.
        let back = grid.back.and_then(|(id, from, tween)| {
            let (desktop, to) = screen.places.iter().enumerate().find_map(|(d, _)| {
                self.grid_windows(output, d)
                    .contains(&id)
                    .then(|| self.grid_window_rect(screen, d, id).map(|r| (d, r)))
                    .flatten()
            })?;
            Some((id, desktop, from.lerp(to, tween.at(now))))
        });
        if let Some((id, _, r)) = back
            && let Some(look) = self.look(id, now)
        {
            let visual = Rectangle::<f64, Logical>::new((r.x, r.y).into(), (r.w, r.h).into());
            let look = look.placed(visual, 1.0, true);
            self.framed_window(c.renderer, id, &look, origin, scale, c.out);
        }
        let over = grid.carry.as_ref().and_then(|c| c.over.clone());
        // The overview's places, for the way from it.
        let (thumbs, slots) = if from_overview && !reduced {
            let screens = self.overview_screens();
            let at = screens.iter().find(|s| s.output == *output);
            (
                at.map(|s| s.thumbs.clone()).unwrap_or_default(),
                at.map(|s| self.overview_slots(s, current))
                    .unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let stride = self.strip_stride(output);
        let marked = |at: &Option<(String, usize)>, desktop: usize| {
            at.as_ref()
                .is_some_and(|(o, d)| *d == desktop && (linked || *o == name))
        };
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
        for (desktop, place) in screen.places.iter().enumerate() {
            // Its picture now: from where the strip, or the overview, has
            // it to its place in the grid.
            let from = match thumbs.get(desktop) {
                Some(thumb) => *thumb,
                None => Area {
                    x: screen.geometry.x + (desktop as f64 - current as f64) * stride,
                    ..screen.geometry
                },
            };
            let picture = if reduced {
                *place
            } else {
                from.lerp(*place, shown)
            };
            let k = picture.w / screen.geometry.w.max(1.0);
            let rect = local(picture, origin);
            let label_at = Area {
                y: picture.y + picture.h + level::LABEL_GAP,
                ..picture
            };
            // What a carried window would do there, or its label.
            // On its screen alone: the window goes to that screen's.
            let dropping = over
                .as_ref()
                .is_some_and(|(o, d)| *d == desktop && *o == name);
            self.grid_label_elements(c, output, desktop, label_at, alpha);
            // Its marks: lit under a carried window, the pointer's, the
            // keyboard's, the desktop shown.
            if dropping {
                self.overview_box(c, Place::GridMark(name.clone(), desktop), LIT, rect, alpha);
            } else if over.is_none() && marked(&grid.hover, desktop) {
                self.overview_box(
                    c,
                    Place::GridMark(name.clone(), desktop),
                    HOVER,
                    rect,
                    alpha,
                );
            }
            let selected = match &grid.selection {
                Selection::Desktop(o, d) => Some((o.clone(), *d)),
                _ => None,
            };
            if marked(&selected, desktop) {
                let ring = local(around(picture, SELECTED_OUT), origin);
                self.overview_box(
                    c,
                    Place::GridSelect(name.clone(), desktop),
                    SELECTED,
                    ring,
                    alpha,
                );
            }
            if desktop == current {
                let style = BoxStyle {
                    radius: ((RADIUS + CURRENT_OUT) * 1000.0) as u32,
                    ..CURRENT
                };
                let ring = local(around(picture, CURRENT_OUT), origin);
                self.overview_box(
                    c,
                    Place::GridCurrent(name.clone(), desktop),
                    style,
                    ring,
                    alpha,
                );
            }
            self.overview_box(c, Place::GridEdge(name.clone(), desktop), EDGE, rect, alpha);
            // Its windows, front first: badges, the keyboard's ring, the
            // windows.
            let mut here = self.grid_windows(output, desktop);
            here.reverse();
            let place_of = |id: WindowId| -> Option<Area> {
                let home = self.home_rect(id)?;
                let at = level::inside(home, screen.geometry, picture);
                // The overview's windows go back to their places.
                Some(
                    match slots
                        .iter()
                        .find(|(w, _)| *w == id)
                        .filter(|_| desktop == current)
                    {
                        Some((_, slot)) => {
                            slot.lerp(level::inside(home, screen.geometry, *place), shown)
                        }
                        None => at,
                    },
                )
            };
            for id in &here {
                if let Some(at) = place_of(*id) {
                    self.grid_badge(c, overview, *id, at, alpha);
                }
            }
            if let Selection::Window(_, d, w) = &grid.selection
                && *d == desktop
                && here.contains(w)
                && let Some(at) = place_of(*w)
            {
                let style = BoxStyle {
                    radius: 8_000,
                    ..SELECTION
                };
                let ring = local(around(at, SELECTION_OUT), origin);
                self.overview_box(c, Place::GridTab, style, ring, alpha);
            }
            let corner = WINDOW_RADIUS * k;
            for id in &here {
                if back.is_some_and(|(b, d, _)| b == *id && d == desktop) {
                    continue;
                }
                let Some(at) = place_of(*id) else {
                    continue;
                };
                let a = if reduced { alpha } else { 1.0 };
                self.grid_window(c, *id, desktop, at, a, corner);
            }
            // The carried one's place, faint.
            if let Some(carry) = &grid.carry
                && self.output_of(carry.window).as_ref() == Some(output)
                && (self.model.desktop(carry.window) == desktop
                    || self.model.is_everywhere(carry.window))
                && let Some(home) = self.home_rect(carry.window)
            {
                let at = level::inside(home, screen.geometry, picture);
                self.grid_window(c, carry.window, desktop, at, PLACE_SHOWS * alpha, corner);
            }
            // An empty desktop's resting panda, breathing as the view
            // opens.
            if self.thumb_windows(output, desktop).is_empty() {
                let size = (picture.h * SLEEP_SHARE).min(SLEEP_MOST);
                let frame = lively
                    .then(|| crate::pandas::frame(BREATHING, now.saturating_sub(grid.since)))
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
            // Its ground: the screen's wallpaper.
            let dst: Rectangle<i32, Physical> = Rectangle::new(
                rect.loc.to_physical(scale).to_i32_round::<i32>(),
                rect.size.to_physical(scale).to_i32_round::<i32>(),
            );
            let ground = if reduced { alpha } else { 1.0 };
            match &wall {
                Some((texture, (w, h))) => {
                    let all = Rectangle::from_size((f64::from(*w), f64::from(*h)).into());
                    match Placed::new(c.renderer, texture, all, dst, ground) {
                        Ok(e) => c.out.push(OutputElements::Frame(Keyed::new(
                            e,
                            self.overview_paint
                                .id(&Place::Wall(name.clone(), desktop), 0),
                        ))),
                        Err(err) => tracing::warn!(?err, "could not draw a desktop's wallpaper"),
                    }
                }
                None => self.overview_box(
                    c,
                    Place::Ground(name.clone(), desktop),
                    GROUND,
                    rect,
                    ground,
                ),
            }
        }
        // The wallpaper dimmed; whole from the overview, which dimmed it.
        let whole: Rectangle<i32, Physical> = Rectangle::from_size(
            geometry
                .size
                .to_f64()
                .to_physical(scale)
                .to_i32_round::<i32>(),
        );
        let a = DIM[3] * if from_overview { 1.0 } else { alpha };
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

    /// Window `id` of desktop `desktop` drawn at `at` (global), `alpha`
    /// of it, its corners `corner` round, from its own textures.
    fn grid_window<R>(
        &self,
        c: &mut Canvas<'_, R>,
        id: WindowId,
        desktop: usize,
        at: Area,
        alpha: f32,
        corner: f64,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let visual = Rectangle::<f64, Logical>::new((at.x, at.y).into(), (at.w, at.h).into());
        let look = crate::render::Look::thumbnail(visual, alpha, corner);
        let mut parts = Vec::new();
        self.window_parts(c.renderer, id, &look, c.origin, c.scale, &mut parts);
        c.out
            .extend(parts.into_iter().enumerate().filter_map(|(j, e)| match e {
                OutputElements::Window(w) => Some(OutputElements::Thumbnail(Keyed::new(
                    w,
                    self.overview_paint.thumb_id(id, desktop, j),
                ))),
                _ => None,
            }));
    }

    /// Desktop `desktop`'s label, centred at `at`'s top (global): its
    /// number and name, a project's monogram, name and folder; under a
    /// carried window, "Lâcher : bureau N".
    fn grid_label_elements<R>(
        &self,
        c: &mut Canvas<'_, R>,
        output: &Output,
        desktop: usize,
        at: Area,
        alpha: f32,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = c.scale;
        let pic = match self.grid_label(output, desktop) {
            level::Label::Drop(text) => Pic::GridWords {
                text,
                ink: DROP_INK,
                bold: true,
                scale: milli(scale),
            },
            level::Label::Project {
                mono,
                title,
                folder,
            } => Pic::GridProject {
                mono,
                title,
                folder,
                scale: milli(scale),
            },
            level::Label::Number(text) => Pic::GridWords {
                text,
                ink: DIM_INK,
                bold: false,
                scale: milli(scale),
            },
        };
        let Some((_, (w, _), _)) = self.overview_paint.picture(&pic, scale) else {
            return;
        };
        let lw = f64::from(w) / scale;
        let place =
            Point::<f64, Logical>::from((at.x + (at.w - lw) / 2.0 - c.origin.x, at.y - c.origin.y));
        self.overview_picture(
            c,
            Place::GridLabel(output.name(), desktop),
            &pic,
            place,
            None,
            alpha,
        );
    }

    /// Window `id`'s agents' badge over its top-left corner, 10 px in:
    /// a 26 px panda, its words 11.5 px; a waiting one bobs.
    fn grid_badge<R>(
        &self,
        c: &mut Canvas<'_, R>,
        overview: &Overview,
        id: WindowId,
        at: Area,
        alpha: f32,
    ) where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(&badge) = overview.badges.get(&id) else {
            return;
        };
        let (origin, scale, now) = (c.origin, c.scale, c.now);
        let since = overview.grid.as_ref().map_or(overview.opened, |g| g.since);
        let corner =
            Point::<f64, Logical>::from((at.x + BADGE_IN - origin.x, at.y + BADGE_IN - origin.y));
        let mood = crate::pandas::mood(badge.state);
        let since = now.saturating_sub(since);
        let (frame, bob) = match (self.pandas_lively(), badge.state) {
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
        let side = (PANDA * scale).ceil() as u32;
        let panda_at =
            Point::<f64, Logical>::from((corner.x + PILL_PAD, corner.y + PILL_PAD + bob));
        self.overview_picture(
            c,
            Place::GridPanda(id),
            &Pic::Panda {
                mood,
                frame,
                side,
                halo: Some(badge.state),
            },
            panda_at,
            Some((PANDA, PANDA)),
            alpha,
        );
        self.overview_picture(
            c,
            Place::GridPill(id),
            &Pic::SmallPill {
                words: badge.words(lang_of(self)),
                state: badge.state,
                scale: milli(scale),
            },
            corner - Point::from((1.0 / scale, 1.0 / scale)),
            None,
            alpha,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_desktop_s_pictures_draw_at_any_scale() {
        let fonts = fonts().expect("the shell's fonts");
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let pics = [
                Pic::GridWords {
                    text: "3 · Courrier".into(),
                    ink: DIM_INK,
                    bold: false,
                    scale: milli(scale),
                },
                Pic::GridWords {
                    text: "Lâcher : bureau 3".into(),
                    ink: DROP_INK,
                    bold: true,
                    scale: milli(scale),
                },
                Pic::GridProject {
                    mono: "HS".into(),
                    title: "2 · hocus-site".into(),
                    folder: "~/github/hocus-site".into(),
                    scale: milli(scale),
                },
                Pic::SmallPill {
                    words: "1 t’attend".into(),
                    state: AgentState::Waiting,
                    scale: milli(scale),
                },
            ];
            for pic in &pics {
                let (pixels, (w, h)) = draw(&fonts, pic, scale).expect("drawn");
                assert_eq!(pixels.len(), (w * h * 4) as usize, "{pic:?}");
                assert!(pixels.iter().any(|p| *p != 0), "{pic:?} is empty");
            }
        }
    }
}
