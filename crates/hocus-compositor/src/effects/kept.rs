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
pub(crate) const CELL: i32 = 16;
/// At most this many rectangles told for one element: the damage tracker
/// spends its time on each, more than on the pixels they cover (#815).
const MOST: usize = 384;
/// The side of the largest cells of an element whose changes are many and
/// spread (the flakes, the drops), physical pixels.
const COARSE: i32 = 128;

/// `rects` (from `geometry`'s corner) told as what of them falls in each
/// cell of a grid on the screen's pixels, the cells' in rows: the damage
/// tracker joins rectangles scattered over the screen (the flakes) into
/// a few large ones, but keeps those that stand apart on its rows and
/// columns as they are.
pub(crate) fn cells(
    rects: &[[i32; 4]],
    geometry: Rectangle<i32, Physical>,
) -> Vec<Rectangle<i32, Physical>> {
    let (gx, gy) = (geometry.loc.x, geometry.loc.y);
    let (w, h) = (geometry.size.w, geometry.size.h);
    // Twice its area at least, overlapping or not (the fireflies' glows):
    // all of it, without looking further.
    let area: i64 = rects
        .iter()
        .map(|[_, _, rw, rh]| i64::from(*rw.max(&0)) * i64::from(*rh.max(&0)))
        .sum();
    if area >= 2 * i64::from(w) * i64::from(h) {
        return vec![Rectangle::from_size((w, h).into())];
    }
    // Many, or crossing many cells (large glows): by larger cells.
    let crossed: usize = rects
        .iter()
        .map(|[_, _, rw, rh]| {
            let across = |n: i32| (n.max(0) / CELL) as usize + 2;
            across(*rw) * across(*rh)
        })
        .sum();
    if rects.len() > MOST || crossed > 16 * MOST {
        return coarse(rects, (w, h));
    }
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
    if out.len() <= MOST {
        return out;
    }
    coarse(rects, (w, h))
}

/// `rects` (within `size`, physical), as many as they are when they are
/// few; else told by rows of larger cells.
pub(crate) fn fewer(
    rects: Vec<Rectangle<i32, Physical>>,
    size: (i32, i32),
) -> Vec<Rectangle<i32, Physical>> {
    if rects.len() <= MOST {
        return rects;
    }
    let rects: Vec<[i32; 4]> = rects
        .iter()
        .map(|r| [r.loc.x, r.loc.y, r.size.w, r.size.h])
        .collect();
    coarse(&rects, size)
}

/// `rects` (from an element's corner, within `size`), many and spread:
/// told by rows of larger cells, twice as large until they are few
/// enough; all of it once they cover over half of it.
fn coarse(rects: &[[i32; 4]], size: (i32, i32)) -> Vec<Rectangle<i32, Physical>> {
    let mut cell = CELL * 2;
    loop {
        let mut tiles = Tiles::with_cell(size, cell);
        for &[x, y, w, h] in rects {
            tiles.mark([x as f32, y as f32, (x + w) as f32, (y + h) as f32]);
        }
        let out = tiles.rects(size);
        // Over half of it: all of it, in one, which costs the damage
        // tracker and the draws least (the fireflies' glows, #815).
        let area: i64 = out
            .iter()
            .map(|r| i64::from(r.size.w) * i64::from(r.size.h))
            .sum();
        if area * 2 > i64::from(size.0) * i64::from(size.1) {
            return vec![Rectangle::from_size(size.into())];
        }
        if out.len() <= MOST || cell >= COARSE {
            return out;
        }
        cell *= 2;
    }
}

/// A part of an effect of few shapes drawn straight on the screen (the
/// fireflies' glows, the koi's pads): what it drew last on a screen, to
/// tell only what of it changed (#815).
#[derive(Debug)]
pub struct Traced {
    pub id: Id,
    commit: CommitCounter,
    last: Option<(Rectangle<i32, Physical>, [Mesh; 3], Clip)>,
    /// The commit before this one, and what changed since.
    since: Option<(CommitCounter, Vec<Rectangle<i32, Physical>>)>,
}

impl Default for Traced {
    fn default() -> Self {
        Self {
            id: Id::new(),
            commit: CommitCounter::default(),
            last: None,
            since: None,
        }
    }
}

impl Traced {
    /// `over`, `added` and `screened` where `clip` says, within
    /// `geometry` (output-local, physical; their places the output's):
    /// a new commit only when they changed, telling where.
    pub fn shapes(
        &mut self,
        geometry: Rectangle<i32, Physical>,
        meshes: [Mesh; 3],
        clip: Clip,
    ) -> FireElement {
        let same_place = self
            .last
            .as_ref()
            .is_some_and(|(g, _, c)| *g == geometry && *c == clip);
        let changed = match &self.last {
            Some((_, last, _)) if same_place => {
                let mut rects = Vec::new();
                for (a, b) in last.iter().zip(&meshes) {
                    mesh::changes(a, b, &mut rects);
                }
                // The shapes' places are the output's: from its corner.
                for r in &mut rects {
                    r[0] -= geometry.loc.x;
                    r[1] -= geometry.loc.y;
                }
                (!rects.is_empty()).then_some(Some(rects))
            }
            _ => Some(None),
        };
        if let Some(rects) = changed {
            let prev = self.commit;
            self.commit.increment();
            self.since = rects.map(|rects| (prev, cells(&rects, geometry)));
        }
        let [over, added, screened] = meshes.clone();
        self.last = Some((geometry, meshes, clip));
        let element =
            FireElement::shapes(self.id.clone(), self.commit, geometry, over, added, clip)
                .screened(screened);
        match &self.since {
            Some((prev, rects)) => element.changed_since(*prev, rects.clone()),
            None => element,
        }
    }
}

/// What changed of a screen-wide effect, by the [`CELL`]s of the screen
/// it falls in (#815): for an effect whose changes are many and spread
/// (the koi's water), told as rows of whole cells, not as each change.
#[derive(Debug)]
pub struct Tiles {
    cell: i32,
    across: usize,
    down: usize,
    dirty: Vec<bool>,
}

impl Tiles {
    /// Over a screen `size` (physical).
    pub fn new(size: (i32, i32)) -> Self {
        Self::with_cell(size, CELL)
    }

    /// Over `size` (physical), in cells of `cell` pixels.
    pub fn with_cell(size: (i32, i32), cell: i32) -> Self {
        let cell = cell.max(1);
        let across = (size.0.max(0) as usize).div_ceil(cell as usize);
        let down = (size.1.max(0) as usize).div_ceil(cell as usize);
        Self {
            cell,
            across,
            down,
            dirty: vec![false; across * down],
        }
    }

    /// `x0, y0, x1, y1` (physical) changed.
    pub fn mark(&mut self, [x0, y0, x1, y1]: [f32; 4]) {
        let cell = self.cell as f32;
        let at = |v: f32, n: usize| v.clamp(0.0, n as f32) as usize;
        let (i0, i1) = (
            at((x0 / cell).floor(), self.across),
            at((x1 / cell).ceil(), self.across),
        );
        let (j0, j1) = (
            at((y0 / cell).floor(), self.down),
            at((y1 / cell).ceil(), self.down),
        );
        for j in j0..j1 {
            let row = j * self.across;
            if let Some(cells) = self.dirty.get_mut(row + i0..row + i1) {
                cells.fill(true);
            }
        }
    }

    /// Whether a cell `x0, y0, x1, y1` (physical) crosses is marked.
    pub fn any(&self, [x0, y0, x1, y1]: [f32; 4]) -> bool {
        let cell = self.cell as f32;
        let at = |v: f32, n: usize| v.clamp(0.0, n as f32) as usize;
        let (i0, i1) = (
            at((x0 / cell).floor(), self.across),
            at((x1 / cell).ceil(), self.across),
        );
        let (j0, j1) = (
            at((y0 / cell).floor(), self.down),
            at((y1 / cell).ceil(), self.down),
        );
        (j0..j1).any(|j| {
            let row = j * self.across;
            self.dirty
                .get(row + i0..row + i1)
                .is_some_and(|cells| cells.contains(&true))
        })
    }

    /// How many of its cells changed, of how many.
    pub fn count(&self) -> (usize, usize) {
        (self.dirty.iter().filter(|d| **d).count(), self.dirty.len())
    }

    /// What changed, each row's runs of cells, within `size` (physical).
    pub fn rects(&self, size: (i32, i32)) -> Vec<Rectangle<i32, Physical>> {
        let mut out = Vec::new();
        for (j, row) in self.dirty.chunks(self.across.max(1)).enumerate() {
            let mut i = 0;
            while i < row.len() {
                if !row.get(i).copied().unwrap_or(false) {
                    i += 1;
                    continue;
                }
                let start = i;
                while row.get(i).copied().unwrap_or(false) {
                    i += 1;
                }
                let cell = self.cell;
                let (x, y) = (start as i32 * cell, j as i32 * cell);
                let (w, h) = (
                    (i as i32 * cell).min(size.0) - x,
                    (y + cell).min(size.1) - y,
                );
                if w > 0 && h > 0 {
                    out.push(Rectangle::new((x, y).into(), (w, h).into()));
                }
            }
        }
        out
    }
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

/// Where the wallpaper shows on a screen, by its cells: what opaque
/// elements in front leave of it (#815). What an effect on it draws
/// elsewhere nobody sees.
#[derive(Debug, Default)]
pub struct Seen(Option<Tiles>);

/// All of the wallpaper.
pub static ALL_SEEN: Seen = Seen(None);

impl Seen {
    /// `shown` (physical) of a screen `size`.
    pub fn of(size: (i32, i32), shown: &[Rectangle<i32, Physical>]) -> Self {
        let mut tiles = Tiles::new(size);
        for r in shown {
            tiles.mark([
                r.loc.x as f32,
                r.loc.y as f32,
                (r.loc.x + r.size.w) as f32,
                (r.loc.y + r.size.h) as f32,
            ]);
        }
        Self(Some(tiles))
    }

    /// Whether some of `x0, y0, x1, y1` (physical) may show.
    pub fn shows(&self, rect: [f32; 4]) -> bool {
        self.0.as_ref().is_none_or(|tiles| tiles.any(rect))
    }
}
