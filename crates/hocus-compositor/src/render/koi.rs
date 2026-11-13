//! The koi's pond drawn (#803): over the wallpaper, which it covers,
//! under the layers of the bottom and background, the windows and the
//! bar; on the screens it shows on. Each frame the koi and their shadows
//! are drawn over the pond's floor into a picture (the board's `under`
//! canvas, at the screen's logical size), the water's heights given to the
//! GPU, and the water drawn from both; the lily pads over it. Behind the
//! glass of the bar stand the floor under still water and a blank for the
//! pads, which never change, so that the bar's glass is not blurred again
//! at each frame. On the screen alone: never in a capture, a share's or a
//! recording's frames.
//!
//! As the koi swim, only what changed is drawn again on the screen
//! (#815): where the water's heights moved enough to be seen, and around
//! the koi that moved as far as the water may bend the floor's picture;
//! the pads, each as it bobs. Still water elsewhere stays as it was.

use smithay::{
    backend::renderer::element::Id,
    utils::{Physical, Rectangle},
};

use super::Frame;
use crate::{
    State,
    effects::{
        FireElement, Plays,
        draw::{Clip, KoiLook},
        kept::{self, Tiles},
        koi::{CELL, Pond, Water},
        mesh::{self, Mesh},
    },
    going::Shaders,
};

/// The board's stage, which the water's bending is a share of.
const BOARD: (f32, f32) = (1200.0, 720.0);
/// How far the water bends what is under it at most (logical pixels,
/// across and down): `KOI`'s `0.035 × bend` of the screen, a pixel more.
const BENT: (f32, f32) = (0.035 * BOARD.0 + 1.0, 0.035 * BOARD.1 + 1.0);
/// A height that moved by less than this since it was drawn is not seen:
/// the floor bent by less than a tenth of a pixel, the crests' light by
/// less than a level of 255 (`KOI`'s `smoothstep` of the heights'
/// laplacian, eight of them at most).
const UNSEEN: f32 = 0.0002;
/// The cells around a height that a pixel's colour is drawn from: its
/// neighbours', each sampled between two.
const HEIGHT_REACH: f32 = 2.0;

/// The koi's pond on a screen this frame: the still water standing for it
/// behind the glass, itself, the blank standing for its pads there and
/// its pads.
pub(super) struct KoiLayer {
    pub stand_in: FireElement,
    pub water: FireElement,
    pub pads_blank: FireElement,
    pub pads: FireElement,
}

impl State {
    /// The pond on `frame`'s output, when it shows there.
    fn pond_on(&self, frame: &Frame) -> Option<&Pond> {
        if !frame.purpose.screen() || !self.koi_shows_on(frame.output) {
            return None;
        }
        self.fx.koi.as_ref()?.panes.get(&frame.output.name())
    }

    /// The pond on `frame`'s output.
    pub(super) fn koi_layer<R>(&self, renderer: &mut R, frame: &Frame) -> Option<KoiLayer>
    where
        R: Shaders,
    {
        let _measure = crate::effects::timing::build(crate::effects::timing::Fx::Koi);
        let pond = self.pond_on(frame)?;
        let screen: Rectangle<i32, Physical> = Rectangle::from_size(
            frame
                .area
                .size
                .to_f64()
                .to_physical_precise_round(frame.scale),
        );
        let (w, h) = pond.size;
        let size = (w.ceil() as i32, h.ceil() as i32);
        if let Some(pixels) = pond.floor_pixels() {
            let texture = renderer.sheet(&pixels, size)?;
            pond.floor_made(texture);
        }
        let floor = pond.floor_texture.borrow().clone()?;
        let generation = pond.generation();
        let fresh = pond.shown.get() != Some(generation);
        let under_mesh = fresh.then(|| pond.under_mesh());
        let under = if let Some(mesh) = &under_mesh {
            renderer.koi_under(&pond.under, size, &floor, mesh)?
        } else {
            pond.under.borrow().as_ref()?.1.clone()
        };
        let (gw, gh) = pond.grid;
        let grid = (gw as i32, gh as i32);
        let (heights, half) = if fresh {
            renderer.koi_heights(&pond.heights, grid, &pond.cur)?
        } else {
            let kept = pond.heights.borrow();
            let kept = kept.as_ref()?;
            (kept.texture.clone(), kept.half)
        };
        pond.shown.set(Some(generation));
        let look = KoiLook {
            screen: [screen.size.w as f32, screen.size.h as f32],
            grid: [w / (gw as f32 * CELL), h / (gh as f32 * CELL)],
            texel: [1.0 / gw as f32, 1.0 / gh as f32],
            bend: [BOARD.0 / w, BOARD.1 / h],
            zero: if half { 0.0 } else { 0.5 },
        };
        let (commit, prev) = pond.commit();
        let mut drawn = pond.water.borrow_mut();
        if let Some(prev) = prev {
            let under = under_mesh.unwrap_or_else(|| pond.under_mesh());
            drawn.since = water_changes(&mut drawn, pond, under, frame.scale, screen)
                .map(|rects| (prev, rects));
        }
        let pads = pond.pads_mesh(frame.scale as f32);
        let clip = Clip::new([-1.0e7, -1.0e7, 1.0e7, 1.0e7], [], 1.0);
        let pads = pond.pads_drawn.borrow_mut().shapes(
            screen,
            [pads, Mesh::default(), Mesh::default()],
            clip,
        );
        let mut water = FireElement::koi(
            pond.ids[0].clone(),
            commit,
            screen,
            under,
            Some(heights),
            look,
        );
        if let Some((prev, rects)) = &drawn.since {
            water = water.changed_since(*prev, rects.clone());
        }
        let moving = self.koi_plays() == Some(Plays::Moving);
        tracing::trace!(moving, "the koi's pond drawn");
        Some(KoiLayer {
            stand_in: FireElement::koi(
                pond.ids[1].clone(),
                pond.floor_commit.get(),
                screen,
                floor,
                None,
                look,
            ),
            water,
            pads_blank: FireElement::blank(pond.ids[2].clone(), screen),
            pads,
        })
    }

    /// The ids of the still water and of the blank standing for the pond
    /// and its pads on `frame`'s output.
    pub(super) fn koi_stand_ins(&self, frame: &Frame) -> Option<(Id, Id)> {
        let pond = self.fx.koi.as_ref()?.panes.get(&frame.output.name())?;
        Some((pond.ids[1].clone(), pond.ids[2].clone()))
    }
}

/// Where the water of `pond` changed since `drawn` (physical, from the
/// screen's corner), now drawn from its heights and `under` (the koi and
/// their shadows) at `scale` over `screen`; `None`: all of it (first
/// drawn, or another scale or size).
fn water_changes(
    drawn: &mut Water,
    pond: &Pond,
    under: Mesh,
    scale: f64,
    screen: Rectangle<i32, Physical>,
) -> Option<Vec<Rectangle<i32, Physical>>> {
    let size = (screen.size.w, screen.size.h);
    let at = (scale.to_bits(), size);
    if drawn.at != Some(at) || drawn.heights.len() != pond.cur.len() {
        drawn.at = Some(at);
        drawn.heights.clone_from(&pond.cur);
        drawn.under = under;
        return None;
    }
    // Rippling over most of it, as it does while the koi swim (they stir
    // it at each step, and its waves fade slowly): all of it.
    let moved = drawn
        .heights
        .iter()
        .zip(&pond.cur)
        .filter(|(was, is)| (*was - *is).abs() >= UNSEEN)
        .count();
    if moved > pond.cur.len() / 4 {
        drawn.heights.clone_from(&pond.cur);
        drawn.under = under;
        return None;
    }
    let s = scale as f32;
    let mut tiles = Tiles::new(size);
    // Each height moved enough to be seen, as far as the pixels drawn
    // from it; those that moved less wait, kept as they were drawn.
    let (gw, _) = pond.grid;
    let r = HEIGHT_REACH;
    for (k, (was, is)) in drawn.heights.iter_mut().zip(&pond.cur).enumerate() {
        if (*was - *is).abs() < UNSEEN {
            continue;
        }
        *was = *is;
        let (i, j) = ((k % gw.max(1)) as f32, (k / gw.max(1)) as f32);
        tiles.mark([
            (i - r) * CELL * s,
            (j - r) * CELL * s,
            (i + 1.0 + r) * CELL * s,
            (j + 1.0 + r) * CELL * s,
        ]);
    }
    // Where the koi moved, as far as the water bends them.
    let mut rects = Vec::new();
    mesh::changes(&drawn.under, &under, &mut rects);
    for [x, y, w, h] in rects {
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
        tiles.mark([
            (x - BENT.0) * s,
            (y - BENT.1) * s,
            (x + w + BENT.0) * s,
            (y + h + BENT.1) * s,
        ]);
    }
    drawn.under = under;
    let (changed, all) = tiles.count();
    tracing::trace!(changed, all, "the koi's water changed");
    Some(kept::fewer(tiles.rects(size), size))
}
