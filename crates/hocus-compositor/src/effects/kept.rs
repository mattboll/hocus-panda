//! What an effect that plays on its own drew last on a screen (#815): its
//! shapes, kept in a texture, drawn again only as they change, and what of
//! them changed, for the screen to draw again there alone.

use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Physical, Rectangle},
};

use super::{
    draw::{Clip, Fire, FireElement},
    mesh::{self, Mesh},
};
use crate::wobble::Kept;

/// A part of an effect as last drawn on a screen.
#[derive(Debug)]
pub struct Shown {
    pub id: Id,
    commit: CommitCounter,
    /// What it drew last, where, and within what.
    last: Option<Last>,
    /// What its shapes were made from, for those who tell.
    pub made_of: Vec<f64>,
    texture: RefCell<Option<Kept>>,
}

#[derive(Debug)]
struct Last {
    over: Mesh,
    added: Mesh,
    clip: Clip,
    geometry: Rectangle<i32, Physical>,
    area: Rectangle<i32, Physical>,
    texture: Rc<dyn std::any::Any>,
}

impl Default for Shown {
    fn default() -> Self {
        Self::new(Id::new())
    }
}

impl Shown {
    /// Drawn as element `id`.
    pub fn new(id: Id) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            last: None,
            made_of: Vec::new(),
            texture: RefCell::default(),
        }
    }

    /// The same as last drawn, within `geometry`, if it was there.
    pub fn again(&self, geometry: Rectangle<i32, Physical>) -> Option<FireElement> {
        let last = self.last.as_ref().filter(|l| l.geometry == geometry)?;
        Some(FireElement::kept(
            self.id.clone(),
            self.commit,
            geometry,
            last.texture.clone(),
            last.area,
        ))
    }

    /// The same as last drawn, when made from `made_of` (as last said)
    /// and drawn within `geometry` where `clip` says.
    pub fn again_for(
        &self,
        made_of: &[f64],
        geometry: Rectangle<i32, Physical>,
        clip: &Clip,
    ) -> Option<FireElement> {
        let last = self.last.as_ref()?;
        (self.made_of == made_of && last.clip == *clip)
            .then(|| self.again(geometry))
            .flatten()
    }

    /// `over` and `added` where `clip` says, within `geometry` (output-
    /// local, physical; their places from its corner): drawn into the
    /// texture again only when they changed, a new commit then, telling
    /// what changed. `None`: not drawn so, or nothing to draw.
    pub fn shapes<R: Fire>(
        &mut self,
        renderer: &mut R,
        geometry: Rectangle<i32, Physical>,
        over: Mesh,
        added: Mesh,
        clip: Clip,
    ) -> Option<FireElement> {
        if let Some(last) = &self.last
            && last.geometry == geometry
            && last.clip == clip
            && last.over.vertices == over.vertices
            && last.over.indices == over.indices
            && last.added.vertices == added.vertices
            && last.added.indices == added.indices
        {
            return self.again(geometry);
        }
        self.redraw(renderer, geometry, over, added, clip, Changed::Shapes)
    }

    /// The same, `over` and `added` changed as `changed` says: drawn into
    /// the texture again, a new commit.
    pub fn redraw<R: Fire>(
        &mut self,
        renderer: &mut R,
        geometry: Rectangle<i32, Physical>,
        over: Mesh,
        added: Mesh,
        clip: Clip,
        changed: Changed,
    ) -> Option<FireElement> {
        // The texture over what they cover on the element, its size in
        // steps: not made again at each of their moves.
        let whole = Rectangle::from_size(geometry.size);
        let [x0, y0, x1, y1] = [&over, &added]
            .iter()
            .filter_map(|m| m.bounds())
            .map(|[x, y, w, h]| [x, y, x + w, y + h])
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })
            .unwrap_or_default();
        const STEP: i32 = 64;
        let (x0, y0) = ((x0 - 1).max(0), (y0 - 1).max(0));
        let size = (
            ((x1 + 1 - x0 + STEP - 1) / STEP * STEP).min(whole.size.w - x0),
            ((y1 + 1 - y0 + STEP - 1) / STEP * STEP).min(whole.size.h - y0),
        );
        let area = Rectangle::new((x0, y0).into(), size.into()).intersection(whole);
        let prev = self.commit;
        self.commit.increment();
        let Some(area) = area.filter(|_| !over.is_empty() || !added.is_empty()) else {
            self.last = None;
            return None;
        };
        let texture = renderer.keep_shapes(&self.texture, area, &over, &added, &clip)?;
        let element = FireElement::kept(
            self.id.clone(),
            self.commit,
            geometry,
            texture.clone(),
            area,
        );
        // What changed, when it was there before and stays as it was.
        let element = match &self.last {
            Some(last) if last.geometry == geometry && last.clip == clip => {
                let rects = match changed {
                    Changed::Shapes => {
                        let mut rects = Vec::new();
                        mesh::changes(&last.over, &over, &mut rects);
                        mesh::changes(&last.added, &added, &mut rects);
                        Some(rects)
                    }
                    Changed::Within(rects) => Some(rects),
                    Changed::All => None,
                };
                match rects {
                    Some(rects) => element.changed_since(prev, cells(&rects, geometry)),
                    None => element,
                }
            }
            _ => element,
        };
        self.last = Some(Last {
            over,
            added,
            clip,
            geometry,
            area,
            texture,
        });
        Some(element)
    }
}

/// What changed of an effect's shapes since they were last drawn.
#[derive(Debug)]
pub enum Changed {
    /// Where their shapes differ.
    Shapes,
    /// Within these (from the element's corner, physical: `x, y, w, h`).
    Within(Vec<[i32; 4]>),
    /// All of them.
    All,
}

/// The side of the cells what changed is told in, physical pixels.
const CELL: i32 = 16;

/// `rects` (from `geometry`'s corner) told as what of them falls in each
/// cell of a grid on the screen's pixels, the cells' in rows: the damage
/// tracker joins rectangles scattered over the screen (the flakes) into
/// a few large ones, but keeps those that stand apart on its rows and
/// columns as they are.
fn cells(rects: &[[i32; 4]], geometry: Rectangle<i32, Physical>) -> Vec<Rectangle<i32, Physical>> {
    let (gx, gy) = (geometry.loc.x, geometry.loc.y);
    let (w, h) = (geometry.size.w, geometry.size.h);
    // Each rectangle's part in each cell it crosses (on the screen,
    // within the element), by cell.
    let mut parts: Vec<((i32, i32), [i32; 4])> = Vec::with_capacity(rects.len() * 2);
    for &[x, y, rw, rh] in rects {
        let (x0, y0) = ((x + gx).max(gx), (y + gy).max(gy));
        let (x1, y1) = ((x + rw + gx).min(gx + w), (y + rh + gy).min(gy + h));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        for cy in y0.div_euclid(CELL)..=(y1 - 1).div_euclid(CELL) {
            for cx in x0.div_euclid(CELL)..=(x1 - 1).div_euclid(CELL) {
                let (c0, d0) = (cx * CELL, cy * CELL);
                let part = [x0.max(c0), y0.max(d0), x1.min(c0 + CELL), y1.min(d0 + CELL)];
                parts.push(((cy, cx), part));
            }
        }
    }
    parts.sort_unstable_by_key(|(cell, _)| *cell);
    let mut out: Vec<Rectangle<i32, Physical>> = Vec::new();
    let mut last: Option<((i32, i32), [i32; 4])> = None;
    let mut done = |[x0, y0, x1, y1]: [i32; 4]| {
        out.push(Rectangle::new(
            (x0 - gx, y0 - gy).into(),
            (x1 - x0, y1 - y0).into(),
        ));
    };
    for (cell, p) in parts {
        last = Some(match last {
            Some((at, b)) if at == cell => (
                at,
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[2]),
                    b[3].max(p[3]),
                ],
            ),
            Some((_, b)) => {
                done(b);
                (cell, p)
            }
            None => (cell, p),
        });
    }
    if let Some((_, b)) = last {
        done(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_changed_is_told_by_cells_of_the_screen() {
        // An element at (10, 20); a rectangle across four cells, another
        // within one of them.
        let geometry = Rectangle::new((10, 20).into(), (100, 100).into());
        let out = cells(&[[0, 0, 10, 10], [2, 2, 3, 3]], geometry);
        let out: Vec<_> = out
            .iter()
            .map(|r| [r.loc.x, r.loc.y, r.size.w, r.size.h])
            .collect();
        // Cells on the screen: x 10..16 and 16..20, y 20..30 (one cell
        // row, 16..32).
        assert_eq!(out, vec![[0, 0, 6, 10], [6, 0, 4, 10]]);
    }
}
